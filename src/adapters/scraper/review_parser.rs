use scraper::{Html, Selector};

use crate::domain::review::{Review, ReviewsPage, ReviewsSummary};
use crate::error::{AirbnbError, Result};

/// Parse reviews from Airbnb listing page HTML.
pub fn parse_reviews(html: &str, listing_id: &str) -> Result<ReviewsPage> {
    // Try __NEXT_DATA__ JSON first
    if let Some(page) = try_parse_next_data_reviews(html, listing_id) {
        return Ok(page);
    }

    // Try deferred state (current format with niobeClientData)
    if let Some(page) = try_parse_deferred_state_reviews(html, listing_id) {
        return Ok(page);
    }

    // CSS fallback
    parse_reviews_css(html, listing_id)
}

fn try_parse_next_data_reviews(html: &str, listing_id: &str) -> Option<ReviewsPage> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r"script#__NEXT_DATA__").ok()?;
    let script = document.select(&selector).next()?;
    let json_text = script.text().collect::<String>();
    let data: serde_json::Value = serde_json::from_str(&json_text).ok()?;

    extract_reviews_from_json(&data, listing_id)
}

fn try_parse_deferred_state_reviews(html: &str, listing_id: &str) -> Option<ReviewsPage> {
    let document = Html::parse_document(html);
    let selector =
        Selector::parse("script[data-deferred-state], script[id^='data-deferred-state']").ok()?;

    for script in document.select(&selector) {
        let json_text = script.text().collect::<String>();
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_text) {
            // Try niobeClientData wrapper (current Airbnb format)
            if let Some(entries) = data.get("niobeClientData").and_then(|v| v.as_array()) {
                for entry in entries {
                    if let Some(inner) = entry.as_array().and_then(|arr| arr.get(1)) {
                        // Try PDP sections format for reviews
                        if let Some(page) = extract_reviews_from_pdp_sections(inner, listing_id) {
                            return Some(page);
                        }
                        // Try legacy format
                        if let Some(page) = extract_reviews_from_json(inner, listing_id) {
                            return Some(page);
                        }
                    }
                }
            }
            // Legacy: try direct JSON structure
            if let Some(page) = extract_reviews_from_json(&data, listing_id) {
                return Some(page);
            }
        }
    }
    None
}

/// Extract reviews from current Airbnb PDP sections format.
fn extract_reviews_from_pdp_sections(
    data: &serde_json::Value,
    listing_id: &str,
) -> Option<ReviewsPage> {
    let pdp = data
        .get("data")?
        .get("presentation")?
        .get("stayProductDetailPage")?;
    let sections_container = pdp.get("sections")?;
    let sections = sections_container.get("sections")?.as_array()?;

    // Find REVIEWS_DEFAULT section
    let review_section = sections.iter().find_map(|s| {
        if s.get("sectionComponentType").and_then(|v| v.as_str()) == Some("REVIEWS_DEFAULT") {
            s.get("section")
        } else {
            None
        }
    })?;

    // Extract ratings summary
    let overall_rating = review_section
        .get("overallRating")
        .and_then(serde_json::Value::as_f64)?;
    let total_reviews = review_section
        .get("overallCount")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;

    // Extract individual ratings
    let ratings = review_section
        .get("ratings")
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
        .clone();

    let mut cleanliness = None;
    let mut accuracy = None;
    let mut communication = None;
    let mut location = None;
    let mut check_in = None;
    let mut value = None;

    for rating in &ratings {
        let label = rating.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let score = rating
            .get("localizedRating")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| rating.get("rating").and_then(serde_json::Value::as_f64));

        match label.to_lowercase().as_str() {
            "cleanliness" => cleanliness = score,
            "accuracy" => accuracy = score,
            "communication" => communication = score,
            "location" => location = score,
            "check-in" | "checkin" => check_in = score,
            "value" => value = score,
            _ => {}
        }
    }

    let summary = Some(ReviewsSummary {
        overall_rating,
        total_reviews,
        cleanliness,
        accuracy,
        communication,
        location,
        check_in,
        value,
    });

    // Extract individual reviews from reviewsData if available
    let mut reviews = Vec::new();
    if let Some(reviews_data) = review_section.get("reviewsData")
        && let Some(review_arr) = reviews_data.get("reviews").and_then(|v| v.as_array())
    {
        for item in review_arr {
            if let Some(review) = extract_single_review(item) {
                reviews.push(review);
            }
        }
    }

    // Also check sbuiData for review content in GUEST_FAVORITE_BANNER
    // (these contain featured review snippets)
    if reviews.is_empty()
        && let Some(sbui) = sections_container.get("sbuiData")
    {
        extract_sbui_reviews(sbui, &mut reviews);
    }

    Some(ReviewsPage {
        listing_id: listing_id.to_string(),
        summary,
        reviews,
        next_cursor: None,
    })
}

/// Extract review snippets from sbuiData if available.
fn extract_sbui_reviews(sbui: &serde_json::Value, reviews: &mut Vec<Review>) {
    // Navigate sbuiData.sectionConfiguration.root.sections to find review data
    if let Some(root_sections) = sbui
        .get("sectionConfiguration")
        .and_then(|sc| sc.get("root"))
        .and_then(|root| root.get("sections"))
        .and_then(|s| s.as_array())
    {
        for section in root_sections {
            if let Some(section_data) = section.get("sectionData") {
                // Look for review highlight sections
                if let Some(highlights) = section_data
                    .get("reviewHighlights")
                    .and_then(|rh| rh.as_array())
                {
                    for highlight in highlights {
                        if let Some(review_text) =
                            highlight.get("reviewText").and_then(|v| v.as_str())
                        {
                            reviews.push(Review {
                                author: highlight
                                    .get("reviewerName")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Guest")
                                    .to_string(),
                                date: String::new(),
                                rating: None,
                                comment: review_text.to_string(),
                                response: None,
                                reviewer_location: None,
                                language: None,
                                is_translated: None,
                            });
                        }
                    }
                }
            }
        }
    }
}

fn extract_reviews_from_json(data: &serde_json::Value, listing_id: &str) -> Option<ReviewsPage> {
    // Find reviews array in JSON
    let reviews_data = find_reviews_data(data)?;
    let reviews_arr = reviews_data.as_array()?;

    let mut reviews = Vec::new();
    for item in reviews_arr {
        if let Some(review) = extract_single_review(item) {
            reviews.push(review);
        }
    }

    if reviews.is_empty() {
        return None;
    }

    let summary = extract_reviews_summary(data);

    Some(ReviewsPage {
        listing_id: listing_id.to_string(),
        summary,
        reviews,
        next_cursor: None,
    })
}

fn find_reviews_data(data: &serde_json::Value) -> Option<&serde_json::Value> {
    let paths: &[&[&str]] = &[
        &["props", "pageProps", "reviews"],
        &["props", "pageProps", "listing", "reviews"],
        &[
            "data",
            "presentation",
            "stayProductDetailPage",
            "reviews",
            "reviews",
        ],
    ];

    for path in paths {
        let mut current = data;
        let mut found = true;
        for key in *path {
            if let Some(next) = current.get(key) {
                current = next;
            } else {
                found = false;
                break;
            }
        }
        if found && current.is_array() {
            return Some(current);
        }
    }

    // Deep search
    deep_find_reviews(data, 20)
}

fn deep_find_reviews(data: &serde_json::Value, max_depth: u32) -> Option<&serde_json::Value> {
    if max_depth == 0 {
        return None;
    }
    match data {
        serde_json::Value::Object(map) => {
            if let Some(reviews) = map.get("reviews")
                && reviews.is_array()
            {
                let arr = reviews.as_array().unwrap();
                let has_review_data = arr.iter().any(|item| {
                    item.get("comments").is_some()
                        || item.get("comment").is_some()
                        || item.get("reviewer").is_some()
                });
                if has_review_data {
                    return Some(reviews);
                }
            }
            for value in map.values() {
                if let Some(result) = deep_find_reviews(value, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(result) = deep_find_reviews(item, max_depth - 1) {
                    return Some(result);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_single_review(data: &serde_json::Value) -> Option<Review> {
    let author = data
        .get("reviewer")
        .and_then(|r| r.get("firstName").or_else(|| r.get("name")))
        .or_else(|| data.get("author"))
        .or_else(|| data.get("authorName"))
        .and_then(|v| v.as_str())
        .unwrap_or("Anonymous")
        .to_string();

    let comment = data
        .get("comments")
        .or_else(|| data.get("comment"))
        .or_else(|| data.get("text"))
        .and_then(|v| v.as_str())?
        .to_string();

    let date = data
        .get("createdAt")
        .or_else(|| data.get("date"))
        .or_else(|| data.get("localizedDate"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let rating = data.get("rating").and_then(serde_json::Value::as_f64);

    let response = data
        .get("response")
        .and_then(|r| r.get("comments").or_else(|| r.get("text")))
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(Review {
        author,
        date,
        reviewer_location: None,
        language: None,
        is_translated: None,
        rating,
        comment,
        response,
    })
}

fn extract_reviews_summary(data: &serde_json::Value) -> Option<ReviewsSummary> {
    let paths: &[&[&str]] = &[
        &["props", "pageProps", "listing"],
        &[
            "data",
            "presentation",
            "stayProductDetailPage",
            "reviewsSummary",
        ],
    ];

    for path in paths {
        let mut current = data;
        let mut found = true;
        for key in *path {
            if let Some(next) = current.get(key) {
                current = next;
            } else {
                found = false;
                break;
            }
        }
        if found
            && let Some(overall) = current
                .get("avgRating")
                .or_else(|| current.get("overallRating"))
                .and_then(serde_json::Value::as_f64)
        {
            return Some(ReviewsSummary {
                overall_rating: overall,
                total_reviews: current
                    .get("reviewsCount")
                    .or_else(|| current.get("totalReviews"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32,
                cleanliness: current
                    .get("cleanlinessRating")
                    .and_then(serde_json::Value::as_f64),
                accuracy: current
                    .get("accuracyRating")
                    .and_then(serde_json::Value::as_f64),
                communication: current
                    .get("communicationRating")
                    .and_then(serde_json::Value::as_f64),
                location: current
                    .get("locationRating")
                    .and_then(serde_json::Value::as_f64),
                check_in: current
                    .get("checkinRating")
                    .and_then(serde_json::Value::as_f64),
                value: current
                    .get("valueRating")
                    .and_then(serde_json::Value::as_f64),
            });
        }
    }
    None
}

fn parse_reviews_css(html: &str, listing_id: &str) -> Result<ReviewsPage> {
    let document = Html::parse_document(html);

    let review_selector =
        Selector::parse("[data-testid='review'], [itemprop='review']").map_err(|e| {
            AirbnbError::Parse {
                reason: format!("invalid selector: {e}"),
            }
        })?;

    let mut reviews = Vec::new();
    for review_el in document.select(&review_selector) {
        let text = review_el.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            reviews.push(Review {
                author: "Guest".into(),
                date: String::new(),
                rating: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
                comment: text,
                response: None,
            });
        }
    }

    Ok(ReviewsPage {
        listing_id: listing_id.to_string(),
        summary: None,
        reviews,
        next_cursor: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reviews_from_next_data() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"reviews":[
            {"reviewer":{"firstName":"Alice"},"comments":"Great place!","createdAt":"2024-01-15","rating":5.0},
            {"reviewer":{"firstName":"Bob"},"comments":"Nice stay","createdAt":"2024-02-10","rating":4.0}
        ],"listing":{"avgRating":4.5,"reviewsCount":100}}}}
        </script></head><body></body></html>"#;

        let page = parse_reviews(html, "123").unwrap();
        assert_eq!(page.reviews.len(), 2);
        assert_eq!(page.reviews[0].author, "Alice");
        assert_eq!(page.reviews[0].comment, "Great place!");
        assert!(page.summary.is_some());
    }

    #[test]
    fn parse_empty_reviews() {
        let html = "<html><body></body></html>";
        let page = parse_reviews(html, "123").unwrap();
        assert!(page.reviews.is_empty());
    }

    #[test]
    fn parse_reviews_with_host_response() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"reviews":[
            {"reviewer":{"firstName":"Charlie"},"comments":"Good","createdAt":"2024-03-01","rating":4.0,
             "response":{"comments":"Thank you for staying!"}}
        ]}}}
        </script></head><body></body></html>"#;

        let page = parse_reviews(html, "1").unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(
            page.reviews[0].response,
            Some("Thank you for staying!".to_string())
        );
    }

    #[test]
    fn parse_reviews_summary_ratings() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"reviews":[
            {"reviewer":{"firstName":"A"},"comments":"Good","createdAt":"2024-01-01"}
        ],"listing":{
            "avgRating":4.7,
            "reviewsCount":50,
            "cleanlinessRating":4.8,
            "accuracyRating":4.9,
            "communicationRating":4.7,
            "locationRating":4.6,
            "checkinRating":4.9,
            "valueRating":4.5
        }}}}
        </script></head><body></body></html>"#;

        let page = parse_reviews(html, "1").unwrap();
        let summary = page.summary.unwrap();
        assert!((summary.overall_rating - 4.7).abs() < f64::EPSILON);
        assert_eq!(summary.total_reviews, 50);
        assert_eq!(summary.cleanliness, Some(4.8));
        assert_eq!(summary.accuracy, Some(4.9));
        assert_eq!(summary.communication, Some(4.7));
        assert_eq!(summary.location, Some(4.6));
        assert_eq!(summary.check_in, Some(4.9));
        assert_eq!(summary.value, Some(4.5));
    }

    #[test]
    fn parse_reviews_deferred_state() {
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"props":{"pageProps":{"reviews":[
            {"reviewer":{"firstName":"Dave"},"comments":"Lovely","createdAt":"2024-05-01","rating":5.0}
        ]}}}
        </script></head><body></body></html>"#;

        let page = parse_reviews(html, "2").unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Dave");
    }

    #[test]
    fn parse_reviews_css_fallback() {
        let html = r#"<html><body>
        <div data-testid="review">Great experience, would come again!</div>
        <div data-testid="review">Very clean and comfortable.</div>
        </body></html>"#;

        let page = parse_reviews(html, "3").unwrap();
        assert_eq!(page.reviews.len(), 2);
        assert_eq!(page.reviews[0].author, "Guest");
        assert!(page.reviews[0].comment.contains("Great experience"));
    }

    #[test]
    fn empty_comments_skipped() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">
        {"props":{"pageProps":{"reviews":[
            {"reviewer":{"firstName":"Eve"},"createdAt":"2024-06-01","rating":3.0},
            {"reviewer":{"firstName":"Frank"},"comments":"Good place","createdAt":"2024-06-02","rating":4.0}
        ]}}}
        </script></head><body></body></html>"#;

        let page = parse_reviews(html, "4").unwrap();
        assert_eq!(page.reviews.len(), 1);
        assert_eq!(page.reviews[0].author, "Frank");
    }

    #[test]
    fn parse_niobe_pdp_reviews() {
        let html = r#"<html><head><script data-deferred-state="true" type="application/json">
        {"niobeClientData":[["StaysPdpSections:test",{
            "data":{"presentation":{"stayProductDetailPage":{
                "sections":{
                    "metadata":{},
                    "sections":[
                        {"sectionComponentType":"REVIEWS_DEFAULT","section":{
                            "overallRating":4.85,
                            "overallCount":200,
                            "ratings":[
                                {"label":"Cleanliness","localizedRating":"4.9"},
                                {"label":"Accuracy","localizedRating":"4.8"},
                                {"label":"Communication","localizedRating":"5.0"},
                                {"label":"Location","localizedRating":"4.7"},
                                {"label":"Check-in","localizedRating":"4.9"},
                                {"label":"Value","localizedRating":"4.6"}
                            ],
                            "reviewsData":{"reviews":[]}
                        }}
                    ]
                }
            }}}
        }]]}
        </script></head><body></body></html>"#;

        let page = parse_reviews(html, "456").unwrap();
        let summary = page.summary.unwrap();
        assert!((summary.overall_rating - 4.85).abs() < f64::EPSILON);
        assert_eq!(summary.total_reviews, 200);
        assert_eq!(summary.cleanliness, Some(4.9));
        assert_eq!(summary.accuracy, Some(4.8));
        assert_eq!(summary.communication, Some(5.0));
        assert_eq!(summary.location, Some(4.7));
        assert_eq!(summary.check_in, Some(4.9));
        assert_eq!(summary.value, Some(4.6));
    }
}
