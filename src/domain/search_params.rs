use chrono::NaiveDate;

use crate::error::{AirbnbError, Result};

#[derive(Debug, Clone)]
pub struct SearchParams {
    pub location: String,
    pub checkin: Option<String>,
    pub checkout: Option<String>,
    pub adults: Option<u32>,
    pub children: Option<u32>,
    pub infants: Option<u32>,
    pub pets: Option<u32>,
    pub min_price: Option<u32>,
    pub max_price: Option<u32>,
    pub property_type: Option<String>,
    pub cursor: Option<String>,
}

impl SearchParams {
    pub fn validate(&self) -> Result<()> {
        if self.location.trim().is_empty() {
            return Err(AirbnbError::InvalidParams {
                reason: "location is required".into(),
            });
        }

        // If one date is set, the other must be too
        match (&self.checkin, &self.checkout) {
            (Some(ci), Some(co)) => {
                let checkin_date = NaiveDate::parse_from_str(ci, "%Y-%m-%d").map_err(|_| {
                    AirbnbError::InvalidParams {
                        reason: format!("invalid checkin date format '{ci}', expected YYYY-MM-DD"),
                    }
                })?;
                let checkout_date = NaiveDate::parse_from_str(co, "%Y-%m-%d").map_err(|_| {
                    AirbnbError::InvalidParams {
                        reason: format!("invalid checkout date format '{co}', expected YYYY-MM-DD"),
                    }
                })?;
                if checkout_date <= checkin_date {
                    return Err(AirbnbError::InvalidParams {
                        reason: "checkout date must be after checkin date".into(),
                    });
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(AirbnbError::InvalidParams {
                    reason: "both checkin and checkout must be provided together".into(),
                });
            }
            _ => {}
        }

        if let Some(min) = self.min_price
            && let Some(max) = self.max_price
            && min > max
        {
            return Err(AirbnbError::InvalidParams {
                reason: "min_price cannot be greater than max_price".into(),
            });
        }

        Ok(())
    }

    pub fn to_query_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = Vec::new();

        if let Some(ref checkin) = self.checkin {
            pairs.push(("checkin".into(), checkin.clone()));
        }
        if let Some(ref checkout) = self.checkout {
            pairs.push(("checkout".into(), checkout.clone()));
        }
        if let Some(adults) = self.adults {
            pairs.push(("adults".into(), adults.to_string()));
        }
        if let Some(children) = self.children {
            pairs.push(("children".into(), children.to_string()));
        }
        if let Some(infants) = self.infants {
            pairs.push(("infants".into(), infants.to_string()));
        }
        if let Some(pets) = self.pets {
            pairs.push(("pets".into(), pets.to_string()));
        }
        if let Some(min_price) = self.min_price {
            pairs.push(("price_min".into(), min_price.to_string()));
        }
        if let Some(max_price) = self.max_price {
            pairs.push(("price_max".into(), max_price.to_string()));
        }
        if let Some(ref property_type) = self.property_type {
            pairs.push(("property_type".into(), property_type.clone()));
        }
        if let Some(ref cursor) = self.cursor {
            pairs.push(("cursor".into(), cursor.clone()));
        }

        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_params() -> SearchParams {
        SearchParams {
            location: "Paris".into(),
            checkin: None,
            checkout: None,
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        }
    }

    #[test]
    fn valid_location_only() {
        assert!(base_params().validate().is_ok());
    }

    #[test]
    fn empty_location_fails() {
        let mut p = base_params();
        p.location = String::new();
        assert!(p.validate().is_err());
    }

    #[test]
    fn checkin_without_checkout_fails() {
        let mut p = base_params();
        p.checkin = Some("2025-06-01".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn min_greater_than_max_fails() {
        let mut p = base_params();
        p.min_price = Some(500);
        p.max_price = Some(100);
        assert!(p.validate().is_err());
    }

    #[test]
    fn query_pairs_built_correctly() {
        let mut p = base_params();
        p.checkin = Some("2025-06-01".into());
        p.checkout = Some("2025-06-05".into());
        p.adults = Some(2);
        let pairs = p.to_query_pairs();
        assert_eq!(pairs.len(), 3);
    }

    #[test]
    fn checkout_without_checkin_fails() {
        let mut p = base_params();
        p.checkout = Some("2025-06-05".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn valid_dates_and_price_pass() {
        let mut p = base_params();
        p.checkin = Some("2025-06-01".into());
        p.checkout = Some("2025-06-05".into());
        p.adults = Some(2);
        p.min_price = Some(50);
        p.max_price = Some(200);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn whitespace_only_location_fails() {
        let mut p = base_params();
        p.location = "   ".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn invalid_checkin_date_format_fails() {
        let mut p = base_params();
        p.checkin = Some("06-01-2025".into());
        p.checkout = Some("2025-06-05".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn invalid_checkout_date_format_fails() {
        let mut p = base_params();
        p.checkin = Some("2025-06-01".into());
        p.checkout = Some("not-a-date".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn checkout_before_checkin_fails() {
        let mut p = base_params();
        p.checkin = Some("2025-06-05".into());
        p.checkout = Some("2025-06-01".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn checkout_same_as_checkin_fails() {
        let mut p = base_params();
        p.checkin = Some("2025-06-01".into());
        p.checkout = Some("2025-06-01".into());
        assert!(p.validate().is_err());
    }

    #[test]
    fn property_type_included_in_query_pairs() {
        let mut p = base_params();
        p.property_type = Some("Entire home".into());
        let pairs = p.to_query_pairs();
        assert!(
            pairs
                .iter()
                .any(|(k, v)| k == "property_type" && v == "Entire home")
        );
    }
}
