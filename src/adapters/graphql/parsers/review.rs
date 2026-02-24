use serde_json::Value;

use crate::domain::review::{Review, ReviewsPage, ReviewsSummary};
use crate::error::{AirbnbError, Result};

/// Parse the GraphQL `StaysPdpReviewsQuery` response into a `ReviewsPage`.
pub fn parse_reviews_response(json: &Value, listing_id: &str) -> Result<ReviewsPage> {
    let reviews_data = json
        .pointer("/data/presentation/stayProductDetailPage/reviews")
        .ok_or_else(|| AirbnbError::Parse {
            reason: "GraphQL reviews: could not find reviews object".into(),
        })?;

    // Parse summary
    let summary = parse_summary(reviews_data);

    // Parse individual reviews
    let reviews_array = reviews_data
        .get("reviews")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .clone();

    let reviews: Vec<Review> = reviews_array
        .iter()
        .filter_map(parse_single_review)
        .collect();

    // Pagination: if we got a full page (50), there might be more
    let has_more = reviews_data
        .get("reviewsCount")
        .or_else(|| reviews_data.pointer("/metadata/reviewsCount"))
        .and_then(Value::as_u64)
        .is_some_and(|total| {
            let current_offset: u64 = reviews_data
                .pointer("/metadata/offset")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            current_offset + reviews.len() as u64 <= total
        });

    let next_cursor = if has_more && !reviews.is_empty() {
        let current_offset: u64 = reviews_data
            .pointer("/metadata/offset")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        Some((current_offset + reviews.len() as u64).to_string())
    } else {
        None
    };

    Ok(ReviewsPage {
        listing_id: listing_id.to_string(),
        summary,
        reviews,
        next_cursor,
    })
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
        .to_string();

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
}
