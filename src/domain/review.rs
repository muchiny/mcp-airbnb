use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub author: String,
    pub date: String,
    pub rating: Option<f64>,
    pub comment: String,
    pub response: Option<String>,
    #[serde(default)]
    pub reviewer_location: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub is_translated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewsSummary {
    pub overall_rating: f64,
    pub total_reviews: u32,
    pub cleanliness: Option<f64>,
    pub accuracy: Option<f64>,
    pub communication: Option<f64>,
    pub location: Option<f64>,
    pub check_in: Option<f64>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewsPage {
    pub listing_id: String,
    pub summary: Option<ReviewsSummary>,
    pub reviews: Vec<Review>,
    pub next_cursor: Option<String>,
}

impl std::fmt::Display for Review {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "**{}**", self.author)?;
        if let Some(ref loc) = self.reviewer_location {
            write!(f, " from {loc}")?;
        }
        write!(f, " ({})", self.date)?;
        if let Some(rating) = self.rating {
            write!(f, " - {rating:.1}*")?;
        }
        writeln!(f)?;
        writeln!(f, "{}", self.comment)?;
        if let Some(ref resp) = self.response {
            writeln!(f, "> Host response: {resp}")?;
        }
        Ok(())
    }
}

impl std::fmt::Display for ReviewsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref summary) = self.summary {
            writeln!(
                f,
                "Overall: {:.2} ({} reviews)",
                summary.overall_rating, summary.total_reviews
            )?;
            if let Some(v) = summary.cleanliness {
                write!(f, "Cleanliness: {v:.1} | ")?;
            }
            if let Some(v) = summary.accuracy {
                write!(f, "Accuracy: {v:.1} | ")?;
            }
            if let Some(v) = summary.communication {
                write!(f, "Communication: {v:.1} | ")?;
            }
            if let Some(v) = summary.location {
                write!(f, "Location: {v:.1} | ")?;
            }
            if let Some(v) = summary.check_in {
                write!(f, "Check-in: {v:.1} | ")?;
            }
            if let Some(v) = summary.value {
                write!(f, "Value: {v:.1}")?;
            }
            writeln!(f)?;
            writeln!(f, "---")?;
        }
        for review in &self.reviews {
            writeln!(f, "{review}")?;
        }
        if self.next_cursor.is_some() {
            writeln!(f, "\n[More reviews available — use cursor to paginate]")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_display_with_rating() {
        let review = Review {
            author: "Alice".into(),
            date: "2025-01-15".into(),
            rating: Some(5.0),
            comment: "Wonderful stay!".into(),
            response: None,
            reviewer_location: None,
            language: None,
            is_translated: None,
        };
        let s = review.to_string();
        assert!(s.contains("**Alice**"));
        assert!(s.contains("2025-01-15"));
        assert!(s.contains("5.0*"));
        assert!(s.contains("Wonderful stay!"));
    }

    #[test]
    fn review_display_without_rating() {
        let review = Review {
            author: "Bob".into(),
            date: "2025-02-10".into(),
            rating: None,
            comment: "Nice place".into(),
            response: None,
            reviewer_location: None,
            language: None,
            is_translated: None,
        };
        let s = review.to_string();
        assert!(s.contains("**Bob**"));
        // Rating format is "X.Y*" — should NOT appear without a rating
        assert!(!s.contains(".0*"));
    }

    #[test]
    fn review_display_with_host_response() {
        let review = Review {
            author: "Charlie".into(),
            date: "2025-03-01".into(),
            rating: Some(4.0),
            comment: "Good location".into(),
            response: Some("Thank you!".into()),
            reviewer_location: None,
            language: None,
            is_translated: None,
        };
        let s = review.to_string();
        assert!(s.contains("> Host response: Thank you!"));
    }

    #[test]
    fn reviews_page_display_with_summary() {
        let page = ReviewsPage {
            listing_id: "123".into(),
            summary: Some(ReviewsSummary {
                overall_rating: 4.7,
                total_reviews: 100,
                cleanliness: Some(4.8),
                accuracy: Some(4.9),
                communication: Some(4.7),
                location: Some(4.6),
                check_in: Some(4.9),
                value: Some(4.5),
            }),
            reviews: vec![Review {
                author: "Alice".into(),
                date: "2025-01-15".into(),
                rating: Some(5.0),
                comment: "Great!".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            }],
            next_cursor: None,
        };
        let s = page.to_string();
        assert!(s.contains("Overall: 4.70 (100 reviews)"));
        assert!(s.contains("Cleanliness: 4.8"));
        assert!(s.contains("Accuracy: 4.9"));
        assert!(s.contains("Communication: 4.7"));
        assert!(s.contains("Location: 4.6"));
        assert!(s.contains("Check-in: 4.9"));
        assert!(s.contains("Value: 4.5"));
    }

    #[test]
    fn reviews_page_display_with_cursor() {
        let page = ReviewsPage {
            listing_id: "123".into(),
            summary: None,
            reviews: vec![Review {
                author: "Alice".into(),
                date: "2025-01-15".into(),
                rating: None,
                comment: "Good".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            }],
            next_cursor: Some("next_page_token".into()),
        };
        let s = page.to_string();
        assert!(s.contains("More reviews available"));
    }

    #[test]
    fn review_display_with_location() {
        let review = Review {
            author: "Jean".into(),
            date: "2025-05-10".into(),
            rating: Some(4.5),
            comment: "Magnifique!".into(),
            response: None,
            reviewer_location: Some("Paris".into()),
            language: None,
            is_translated: None,
        };
        let s = review.to_string();
        assert!(
            s.contains("from Paris"),
            "Display should contain 'from Paris', got: {s}"
        );
        assert!(s.contains("**Jean**"));
    }

    #[test]
    fn reviews_page_display_multiple_reviews() {
        let reviews = vec![
            Review {
                author: "Alice".into(),
                date: "2025-01-10".into(),
                rating: Some(5.0),
                comment: "Wonderful stay!".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            },
            Review {
                author: "Bob".into(),
                date: "2025-02-15".into(),
                rating: Some(3.0),
                comment: "Average experience".into(),
                response: Some("Thank you for the feedback".into()),
                reviewer_location: Some("London".into()),
                language: None,
                is_translated: None,
            },
            Review {
                author: "Charlie".into(),
                date: "2025-03-20".into(),
                rating: None,
                comment: "No complaints".into(),
                response: None,
                reviewer_location: None,
                language: None,
                is_translated: None,
            },
        ];
        let page = ReviewsPage {
            listing_id: "456".into(),
            summary: None,
            reviews,
            next_cursor: None,
        };
        let s = page.to_string();

        // All three reviews should appear
        assert!(
            s.contains("**Alice**"),
            "Display should contain Alice's review"
        );
        assert!(s.contains("**Bob**"), "Display should contain Bob's review");
        assert!(
            s.contains("**Charlie**"),
            "Display should contain Charlie's review"
        );
        assert!(
            s.contains("Wonderful stay!"),
            "Display should contain Alice's comment"
        );
        assert!(
            s.contains("Average experience"),
            "Display should contain Bob's comment"
        );
        assert!(
            s.contains("No complaints"),
            "Display should contain Charlie's comment"
        );
        assert!(
            s.contains("from London"),
            "Display should contain Bob's location"
        );
        assert!(
            s.contains("Host response: Thank you for the feedback"),
            "Display should contain host response"
        );
        // Should NOT have cursor message
        assert!(!s.contains("More reviews available"));
    }
}
