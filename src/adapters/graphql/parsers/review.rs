use base64::Engine as _;
use serde_json::Value;

use crate::domain::review::{Review, ReviewsPage, ReviewsSummary};
use crate::error::{AirbnbError, Result};

/// Reviews per `StaysPdpReviewsQuery` page, as requested by the airbnb.com web client (2026-09).
pub const REVIEWS_PAGE_SIZE: u64 = 24;

/// Variables for `StaysPdpReviewsQuery`, identical to the airbnb.com web client
/// capture of 2026-09 except for `offset`. `id` is the relay id
/// `base64("StayListing:{listing_id}")`, like the other PDP operations.
pub fn build_reviews_variables(listing_id: &str, offset: u64) -> Value {
    let relay_id =
        base64::engine::general_purpose::STANDARD.encode(format!("StayListing:{listing_id}"));
    serde_json::json!({
        "id": relay_id,
        "pdpReviewsRequest": {
            "fieldSelector": "for_p3_translation_only",
            "forPreview": false,
            "limit": REVIEWS_PAGE_SIZE,
            "offset": offset.to_string(),
            "showingTranslationButton": false,
            "first": REVIEWS_PAGE_SIZE,
            "sortingPreference": "BEST_QUALITY",
            "numberOfAdults": "1",
            "numberOfChildren": "0",
            "numberOfInfants": "0",
            "numberOfPets": 0,
            "amenityFilters": null,
        }
    })
}

/// Parse a `StaysPdpReviewsQuery` response to a **first-page** request (offset 0).
///
/// Kept with its original signature for the fuzz target and single-page callers.
/// Anything that paginates must call [`parse_reviews_page`] with the offset it sent.
pub fn parse_reviews_response(json: &Value, listing_id: &str) -> Result<ReviewsPage> {
    parse_reviews_page(json, listing_id, 0)
}

/// Parse the GraphQL `StaysPdpReviewsQuery` response into a `ReviewsPage`.
///
/// `request_offset` is the offset that was *sent*: Airbnb does not echo it, so
/// the next cursor is `request_offset` plus the number of items returned (with
/// or without a comment), and only while that stays below `reviewsCount` (or,
/// when the total is unknown, while pages come back full).
///
/// # Errors
///
/// [`AirbnbError::UpstreamSchema`] when the `reviews` object is missing, when
/// its `reviews` array is missing or not an array (unless `reviewsCount` is 0),
/// or when a first page (`request_offset` 0) is empty while `reviewsCount` > 0.
pub fn parse_reviews_page(
    json: &Value,
    listing_id: &str,
    request_offset: u64,
) -> Result<ReviewsPage> {
    let reviews_data = json
        .pointer("/data/presentation/stayProductDetailPage/reviews")
        .filter(|node| node.is_object())
        .ok_or_else(|| AirbnbError::UpstreamSchema {
            operation: "StaysPdpReviewsQuery".into(),
            detail: "response has no data.presentation.stayProductDetailPage.reviews object".into(),
        })?;

    let summary = parse_summary(reviews_data);

    let total = reviews_data
        .get("reviewsCount")
        .or_else(|| reviews_data.pointer("/metadata/reviewsCount"))
        .and_then(Value::as_u64);

    // A missing or renamed `reviews` array, or an empty first page of a
    // listing that has reviews, is drift: report it so the composite can
    // fall back instead of answering with an empty success.
    let raw_reviews: &[Value] = match reviews_data.get("reviews").and_then(Value::as_array) {
        Some(items) => items,
        None if total == Some(0) => &[],
        None => {
            return Err(AirbnbError::UpstreamSchema {
                operation: "StaysPdpReviewsQuery".into(),
                detail: "reviews is not an array".into(),
            });
        }
    };
    if request_offset == 0
        && raw_reviews.is_empty()
        && let Some(total) = total.filter(|&total| total > 0)
    {
        return Err(AirbnbError::UpstreamSchema {
            operation: "StaysPdpReviewsQuery".into(),
            detail: format!("first page is empty but reviewsCount is {total}"),
        });
    }
    let reviews: Vec<Review> = raw_reviews.iter().filter_map(parse_single_review).collect();

    let next_cursor =
        next_review_offset(request_offset, raw_reviews.len(), total).map(|next| next.to_string());

    Ok(ReviewsPage {
        listing_id: listing_id.to_string(),
        summary,
        reviews,
        next_cursor,
    })
}

/// Offset of the page after one that started at `request_offset` and returned
/// `returned` items, or `None` when there is no further page (or on overflow).
fn next_review_offset(request_offset: u64, returned: usize, total: Option<u64>) -> Option<u64> {
    let returned = u64::try_from(returned).ok()?;
    if returned == 0 {
        return None;
    }
    let next = request_offset.checked_add(returned)?;
    match total {
        Some(total) => (next < total).then_some(next),
        None => (returned >= REVIEWS_PAGE_SIZE).then_some(next),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn parse_summary(data: &Value) -> Option<ReviewsSummary> {
    let overall = data
        .get("overallRating")
        .or_else(|| data.pointer("/reviewSummary/overallRating"))
        .and_then(Value::as_f64)?;

    let total = data
        .get("reviewsCount")
        .or_else(|| data.get("overallCount"))
        .or_else(|| data.pointer("/reviewSummary/totalReviews"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    // Try multiple rating array field names: ratings (PDP), categoryRatings (reviews endpoint)
    let ratings = data
        .get("ratings")
        .or_else(|| data.get("categoryRatings"))
        .or_else(|| data.pointer("/reviewSummary/categoryRatings"))
        .and_then(Value::as_array);

    let mut cleanliness = None;
    let mut accuracy = None;
    let mut communication = None;
    let mut location = None;
    let mut check_in = None;
    let mut value = None;

    if let Some(cats) = ratings {
        for cat in cats {
            // Category name: try "label", "name", "categoryType"
            let cat_name = cat
                .get("label")
                .or_else(|| cat.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let cat_type = cat
                .get("categoryType")
                .and_then(Value::as_str)
                .unwrap_or_default();

            // Rating value: try "value" (f64), "localizedRating" (string), "percentage" (* 5.0)
            let rating_val = cat
                .get("value")
                .and_then(Value::as_f64)
                .or_else(|| {
                    cat.get("localizedRating")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<f64>().ok())
                })
                .or_else(|| {
                    cat.get("percentage")
                        .and_then(Value::as_f64)
                        .map(|p| p * 5.0)
                });

            match (cat_name, cat_type) {
                (n, t) if n == "Cleanliness" || t == "CLEANLINESS" => cleanliness = rating_val,
                (n, t) if n == "Accuracy" || t == "ACCURACY" => accuracy = rating_val,
                (n, t) if n == "Communication" || t == "COMMUNICATION" => {
                    communication = rating_val;
                }
                (n, t) if n == "Location" || t == "LOCATION" => location = rating_val,
                (n, t) if n == "Check-in" || n == "check_in" || t == "CHECKIN" => {
                    check_in = rating_val;
                }
                (n, t) if n == "Value" || t == "VALUE" => value = rating_val,
                _ => {}
            }
        }
    }

    Some(ReviewsSummary {
        overall_rating: overall,
        total_reviews: total,
        cleanliness,
        accuracy,
        communication,
        location,
        check_in,
        value,
    })
}

fn parse_single_review(review: &Value) -> Option<Review> {
    let comment = review
        .get("comments")
        .or_else(|| review.get("comment"))
        .or_else(|| review.get("text"))
        .or_else(|| review.get("body"))
        .or_else(|| review.get("content"))
        .and_then(Value::as_str)?
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n");

    let author = review
        .pointer("/reviewer/firstName")
        .or_else(|| review.get("reviewerName"))
        .and_then(Value::as_str)
        .unwrap_or("Anonymous")
        .to_string();

    let date = review
        .get("createdAt")
        .or_else(|| review.get("localizedDate"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let rating = review.get("rating").and_then(Value::as_f64);

    let response = review
        .get("response")
        .or_else(|| review.pointer("/hostResponse/comments"))
        .and_then(Value::as_str)
        .map(String::from);

    let reviewer_location = review
        .pointer("/reviewer/location")
        .and_then(Value::as_str)
        .map(String::from);

    let language = review
        .get("language")
        .and_then(Value::as_str)
        .map(String::from);

    let is_translated = review.get("isTranslated").and_then(Value::as_bool);

    Some(Review {
        author,
        date,
        rating,
        comment,
        response,
        reviewer_location,
        language,
        is_translated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reviews_basic() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "reviews": {
                            "overallRating": 4.85,
                            "reviewsCount": 100,
                            "reviews": [{
                                "reviewer": {
                                    "firstName": "Alice",
                                    "location": "New York"
                                },
                                "createdAt": "2025-01-15",
                                "rating": 5.0,
                                "comments": "Wonderful stay!",
                                "language": "en"
                            }]
                        }
                    }
                }
            }
        });

        let page = parse_reviews_response(&json, "12345").unwrap();
        assert_eq!(page.listing_id, "12345");
        assert!(page.summary.is_some());
        let summary = page.summary.unwrap();
        assert!((summary.overall_rating - 4.85).abs() < 0.01);
        assert_eq!(summary.total_reviews, 100);
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Alice");
        assert_eq!(page.reviews[0].comment, "Wonderful stay!");
    }

    #[test]
    fn parse_reviews_with_category_ratings() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "reviews": {
                            "overallRating": 4.9,
                            "reviewsCount": 50,
                            "categoryRatings": [
                                { "name": "Cleanliness", "value": 5.0 },
                                { "name": "Accuracy", "value": 4.8 },
                                { "name": "Communication", "value": 4.9 },
                                { "name": "Location", "value": 4.7 },
                                { "name": "Check-in", "value": 5.0 },
                                { "name": "Value", "value": 4.6 }
                            ],
                            "reviews": [{
                                "reviewer": { "firstName": "Test" },
                                "comments": "Great!",
                                "createdAt": "2025-01-01"
                            }]
                        }
                    }
                }
            }
        });
        let page = parse_reviews_response(&json, "42").unwrap();
        let summary = page.summary.unwrap();
        assert!((summary.cleanliness.unwrap() - 5.0).abs() < 0.01);
        assert!((summary.accuracy.unwrap() - 4.8).abs() < 0.01);
        assert!((summary.communication.unwrap() - 4.9).abs() < 0.01);
        assert!((summary.location.unwrap() - 4.7).abs() < 0.01);
        assert!((summary.check_in.unwrap() - 5.0).abs() < 0.01);
        assert!((summary.value.unwrap() - 4.6).abs() < 0.01);
    }

    #[test]
    fn parse_reviews_pagination_cursor() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "reviews": {
                            "overallRating": 4.5,
                            "reviewsCount": 100,
                            "metadata": { "offset": 0 },
                            "reviews": [{
                                "reviewer": { "firstName": "A" },
                                "comments": "Nice",
                                "createdAt": "2025-01-01"
                            }]
                        }
                    }
                }
            }
        });
        let page = parse_reviews_response(&json, "42").unwrap();
        assert!(page.next_cursor.is_some());
        assert_eq!(page.next_cursor.unwrap(), "1");
    }

    #[test]
    fn parse_reviews_no_comments_skipped() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "reviews": {
                            "reviews": [
                                { "reviewer": { "firstName": "NoComment" }, "rating": 5.0 },
                                { "reviewer": { "firstName": "WithComment" }, "comments": "Hello!", "createdAt": "2025-01-01" }
                            ]
                        }
                    }
                }
            }
        });
        let page = parse_reviews_response(&json, "42").unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "WithComment");
    }

    #[test]
    fn parse_reviews_empty() {
        let json = serde_json::json!({
            "data": {
                "presentation": {
                    "stayProductDetailPage": {
                        "reviews": {
                            "reviews": []
                        }
                    }
                }
            }
        });

        let page = parse_reviews_response(&json, "12345").unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.summary.is_none());
    }

    #[test]
    fn parses_2026_09_web_capture() {
        let json = crate::test_helpers::fixture_json("graphql/StaysPdpReviewsQuery.response.json");
        let raw = json
            .pointer("/data/presentation/stayProductDetailPage/reviews/reviews")
            .and_then(Value::as_array)
            .expect("capture has a reviews array");
        let page = parse_reviews_response(&json, "38817969").unwrap();
        assert_eq!(page.reviews.len(), 24);
        assert_eq!(page.reviews.len(), raw.len());
        assert_eq!(
            page.reviews[0].author,
            raw[0]["reviewer"]["firstName"]
                .as_str()
                .expect("reviewer name")
        );
        assert!(
            page.reviews
                .iter()
                .all(|r| !r.comment.is_empty() && !r.comment.contains("<br"))
        );
        assert!(page.reviews.iter().all(|r| r.rating.is_some()));
        assert_eq!(page.next_cursor.as_deref(), Some("24"));
    }

    #[test]
    fn next_cursor_comes_from_the_request_offset() {
        let json = crate::test_helpers::fixture_json("graphql/StaysPdpReviewsQuery.response.json");
        let page = parse_reviews_page(&json, "38817969", 48).unwrap();
        assert_eq!(page.next_cursor.as_deref(), Some("72"));
    }

    #[test]
    fn last_page_that_reaches_the_total_has_no_next_cursor() {
        let json = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"reviews": {
            "metadata": { "reviewsCount": 30 },
            "reviews": [
                {"comments": "a"}, {"comments": "b"}, {"comments": "c"},
                {"comments": "d"}, {"comments": "e"}, {"comments": "f"}
            ]
        }}}}});
        let page = parse_reviews_page(&json, "1", 24).unwrap();
        assert_eq!(page.reviews.len(), 6);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn comment_less_items_still_advance_the_offset() {
        let json = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"reviews": {
            "metadata": { "reviewsCount": 100 },
            "reviews": [ {"rating": 5}, {"comments": "ok"} ]
        }}}}});
        let page = parse_reviews_page(&json, "1", 0).unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("2"));
    }

    #[test]
    fn huge_offsets_do_not_overflow() {
        let json = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"reviews": {
            "reviews": [ {"comments": "a"}, {"comments": "b"} ]
        }}}}});
        let page = parse_reviews_page(&json, "1", u64::MAX - 1).unwrap();
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn unknown_total_continues_only_after_a_full_page() {
        let short = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"reviews": {
            "reviews": [ {"comments": "a"}, {"comments": "b"}, {"comments": "c"} ]
        }}}}});
        assert!(
            parse_reviews_page(&short, "1", 0)
                .unwrap()
                .next_cursor
                .is_none()
        );

        let full_items: Vec<Value> = (0..24)
            .map(|i| serde_json::json!({ "comments": format!("review {i}") }))
            .collect();
        let full = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": {"reviews": {
            "reviews": full_items
        }}}}});
        assert_eq!(
            parse_reviews_page(&full, "1", 0)
                .unwrap()
                .next_cursor
                .as_deref(),
            Some("24")
        );
    }

    #[test]
    fn two_argument_wrapper_is_the_first_page() {
        let json = crate::test_helpers::fixture_json("graphql/StaysPdpReviewsQuery.response.json");
        let wrapper = parse_reviews_response(&json, "38817969").unwrap();
        let first = parse_reviews_page(&json, "38817969", 0).unwrap();
        assert_eq!(wrapper.next_cursor, first.next_cursor);
        assert_eq!(wrapper.reviews.len(), first.reviews.len());
    }

    #[test]
    fn missing_reviews_object_is_upstream_schema() {
        let json = serde_json::json!({"data": {"presentation": {"stayProductDetailPage": null}}});
        let err = parse_reviews_page(&json, "1", 0).unwrap_err();
        assert!(
            matches!(err, AirbnbError::UpstreamSchema { .. }),
            "got {err:?}"
        );
    }

    /// The 2026-09 capture (`metadata.reviewsCount` 490) with its `reviews`
    /// object rewritten by `edit`.
    fn capture_with(edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Value {
        let mut json =
            crate::test_helpers::fixture_json("graphql/StaysPdpReviewsQuery.response.json");
        let reviews = json
            .pointer_mut("/data/presentation/stayProductDetailPage/reviews")
            .and_then(Value::as_object_mut)
            .expect("capture has a reviews object");
        edit(reviews);
        json
    }

    fn assert_upstream_schema(result: Result<ReviewsPage>, detail_part: &str) {
        match result {
            Err(AirbnbError::UpstreamSchema { operation, detail }) => {
                assert_eq!(operation, "StaysPdpReviewsQuery");
                assert!(detail.contains(detail_part), "detail was {detail:?}");
            }
            other => panic!("expected UpstreamSchema, got {other:?}"),
        }
    }

    #[test]
    fn removed_reviews_array_is_upstream_schema() {
        let json = capture_with(|reviews| {
            reviews.remove("reviews");
        });
        assert_upstream_schema(
            parse_reviews_page(&json, "38817969", 0),
            "reviews is not an array",
        );
    }

    #[test]
    fn renamed_reviews_array_is_upstream_schema() {
        let json = capture_with(|reviews| {
            let items = reviews.remove("reviews").expect("capture has reviews");
            reviews.insert("reviewItems".into(), items);
        });
        assert_upstream_schema(
            parse_reviews_page(&json, "38817969", 0),
            "reviews is not an array",
        );
    }

    #[test]
    fn empty_first_page_with_a_positive_total_is_upstream_schema() {
        let json = capture_with(|reviews| {
            reviews.insert("reviews".into(), serde_json::json!([]));
        });
        assert_upstream_schema(
            parse_reviews_page(&json, "38817969", 0),
            "first page is empty but reviewsCount is 490",
        );
    }

    #[test]
    fn empty_first_page_with_a_zero_total_is_ok_empty() {
        let json = capture_with(|reviews| {
            reviews.insert("reviews".into(), serde_json::json!([]));
            reviews.insert("metadata".into(), serde_json::json!({ "reviewsCount": 0 }));
        });
        let page = parse_reviews_page(&json, "38817969", 0).unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn missing_reviews_array_with_a_zero_total_is_ok_empty() {
        let json = capture_with(|reviews| {
            reviews.remove("reviews");
            reviews.insert("metadata".into(), serde_json::json!({ "reviewsCount": 0 }));
        });
        let page = parse_reviews_page(&json, "38817969", 0).unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn empty_later_page_stays_ok_without_a_cursor() {
        let json = capture_with(|reviews| {
            reviews.insert("reviews".into(), serde_json::json!([]));
        });
        let page = parse_reviews_page(&json, "38817969", 480).unwrap();
        assert!(page.reviews.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn variables_match_the_2026_09_web_client_capture() {
        let expected = serde_json::json!({
            "id": "U3RheUxpc3Rpbmc6Mzg4MTc5Njk=",
            "pdpReviewsRequest": {
                "fieldSelector": "for_p3_translation_only",
                "forPreview": false,
                "limit": 24,
                "offset": "0",
                "showingTranslationButton": false,
                "first": 24,
                "sortingPreference": "BEST_QUALITY",
                "numberOfAdults": "1",
                "numberOfChildren": "0",
                "numberOfInfants": "0",
                "numberOfPets": 0,
                "amenityFilters": null
            }
        });
        assert_eq!(build_reviews_variables("38817969", 0), expected);
    }
}
