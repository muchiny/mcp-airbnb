use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub location: String,
    pub price_per_night: f64,
    pub currency: String,
    pub rating: Option<f64>,
    pub review_count: u32,
    pub thumbnail_url: Option<String>,
    pub property_type: Option<String>,
    pub host_name: Option<String>,
    #[serde(default)]
    pub host_id: Option<String>,
    pub url: String,
    #[serde(default)]
    pub is_superhost: Option<bool>,
    #[serde(default)]
    pub is_guest_favorite: Option<bool>,
    #[serde(default)]
    pub instant_book: Option<bool>,
    #[serde(default)]
    pub total_price: Option<f64>,
    #[serde(default)]
    pub photos: Vec<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingDetail {
    pub id: String,
    pub name: String,
    pub location: String,
    pub description: String,
    pub price_per_night: f64,
    pub currency: String,
    pub rating: Option<f64>,
    pub review_count: u32,
    pub property_type: Option<String>,
    pub host_name: Option<String>,
    pub url: String,
    pub amenities: Vec<String>,
    pub house_rules: Vec<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub photos: Vec<String>,
    pub bedrooms: Option<u32>,
    pub beds: Option<u32>,
    pub bathrooms: Option<f64>,
    pub max_guests: Option<u32>,
    pub check_in_time: Option<String>,
    pub check_out_time: Option<String>,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub host_is_superhost: Option<bool>,
    #[serde(default)]
    pub host_response_rate: Option<String>,
    #[serde(default)]
    pub host_response_time: Option<String>,
    #[serde(default)]
    pub host_joined: Option<String>,
    #[serde(default)]
    pub host_total_listings: Option<u32>,
    #[serde(default)]
    pub host_languages: Vec<String>,
    #[serde(default)]
    pub cancellation_policy: Option<String>,
    #[serde(default)]
    pub instant_book: Option<bool>,
    #[serde(default)]
    pub cleaning_fee: Option<f64>,
    #[serde(default)]
    pub service_fee: Option<f64>,
    #[serde(default)]
    pub neighborhood: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub listings: Vec<Listing>,
    pub total_count: Option<u32>,
    pub next_cursor: Option<String>,
}

impl std::fmt::Display for Listing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} - {} ({}{}/night",
            self.name, self.location, self.currency, self.price_per_night
        )?;
        if let Some(rating) = self.rating {
            write!(
                f,
                ", {rating:.1}* {reviews} reviews",
                reviews = self.review_count
            )?;
        }
        if self.is_superhost == Some(true) {
            write!(f, " | Superhost")?;
        }
        if self.is_guest_favorite == Some(true) {
            write!(f, " | Guest Favorite")?;
        }
        if let Some(ref hid) = self.host_id {
            write!(f, " | Host ID: {hid}")?;
        }
        if let Some(total) = self.total_price {
            write!(f, " | Total: {}{total:.0}", self.currency)?;
        }
        write!(f, ")")
    }
}

impl std::fmt::Display for ListingDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "# {}", self.name)?;
        writeln!(f, "Location: {}", self.location)?;
        writeln!(f, "Price: {}{}/night", self.currency, self.price_per_night)?;
        if let Some(rating) = self.rating {
            writeln!(f, "Rating: {rating:.2} ({} reviews)", self.review_count)?;
        }
        if let Some(ref pt) = self.property_type {
            writeln!(f, "Type: {pt}")?;
        }
        if let Some(ref host) = self.host_name {
            write!(f, "Host: {host}")?;
            if self.host_is_superhost == Some(true) {
                write!(f, " (Superhost)")?;
            }
            if let Some(ref rate) = self.host_response_rate {
                write!(f, " | Response rate: {rate}")?;
            }
            if let Some(ref time) = self.host_response_time {
                write!(f, " | Response time: {time}")?;
            }
            writeln!(f)?;
            if let Some(ref joined) = self.host_joined {
                writeln!(f, "Host since: {joined}")?;
            }
            if let Some(count) = self.host_total_listings {
                writeln!(f, "Host listings: {count}")?;
            }
            if !self.host_languages.is_empty() {
                writeln!(f, "Languages: {}", self.host_languages.join(", "))?;
            }
        }
        if let Some(bedrooms) = self.bedrooms {
            write!(f, "Bedrooms: {bedrooms}")?;
            if let Some(beds) = self.beds {
                write!(f, " | Beds: {beds}")?;
            }
            if let Some(baths) = self.bathrooms {
                write!(f, " | Bathrooms: {baths}")?;
            }
            writeln!(f)?;
        }
        if let Some(max) = self.max_guests {
            writeln!(f, "Max guests: {max}")?;
        }
        if let Some(ref policy) = self.cancellation_policy {
            writeln!(f, "Cancellation: {policy}")?;
        }
        if let Some(ref nb) = self.neighborhood {
            writeln!(f, "Neighborhood: {nb}")?;
        }
        if self.cleaning_fee.is_some() || self.service_fee.is_some() {
            write!(f, "Fees:")?;
            if let Some(fee) = self.cleaning_fee {
                write!(f, " Cleaning {}{fee:.0}", self.currency)?;
            }
            if let Some(fee) = self.service_fee {
                write!(f, " Service {}{fee:.0}", self.currency)?;
            }
            writeln!(f)?;
        }
        if !self.description.is_empty() {
            writeln!(f, "\n## Description\n{}", self.description)?;
        }
        if !self.amenities.is_empty() {
            writeln!(f, "\n## Amenities\n{}", self.amenities.join(", "))?;
        }
        if !self.house_rules.is_empty() {
            writeln!(f, "\n## House Rules\n{}", self.house_rules.join(", "))?;
        }
        writeln!(f, "\nURL: {}", self.url)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_display_with_rating() {
        let listing = Listing {
            id: "123".into(),
            name: "Cozy Apartment".into(),
            location: "Paris, France".into(),
            price_per_night: 120.0,
            currency: "$".into(),
            rating: Some(4.85),
            review_count: 42,
            thumbnail_url: None,
            property_type: Some("Apartment".into()),
            host_name: Some("Alice".into()),
            host_id: None,
            url: "https://airbnb.com/rooms/123".into(),
            is_superhost: None,
            is_guest_favorite: None,
            instant_book: None,
            total_price: None,
            photos: vec![],
            latitude: None,
            longitude: None,
        };
        let s = listing.to_string();
        assert!(s.contains("Cozy Apartment"));
        assert!(s.contains("$120"));
        assert!(s.contains("4.8"));
    }

    #[test]
    fn listing_display_without_rating() {
        let listing = Listing {
            id: "456".into(),
            name: "Beach House".into(),
            location: "Malibu".into(),
            price_per_night: 300.0,
            currency: "$".into(),
            rating: None,
            review_count: 0,
            thumbnail_url: None,
            property_type: None,
            host_name: None,
            host_id: None,
            url: "https://airbnb.com/rooms/456".into(),
            is_superhost: None,
            is_guest_favorite: None,
            instant_book: None,
            total_price: None,
            photos: vec![],
            latitude: None,
            longitude: None,
        };
        let s = listing.to_string();
        assert!(s.contains("Beach House"));
        assert!(!s.contains("reviews"));
    }

    #[test]
    fn listing_detail_display_full() {
        let detail = ListingDetail {
            id: "789".into(),
            name: "Villa Rosa".into(),
            location: "Rome, Italy".into(),
            description: "A beautiful villa".into(),
            price_per_night: 200.0,
            currency: "€".into(),
            rating: Some(4.9),
            review_count: 55,
            property_type: Some("Villa".into()),
            host_name: Some("Marco".into()),
            url: "https://airbnb.com/rooms/789".into(),
            amenities: vec!["WiFi".into(), "Pool".into()],
            house_rules: vec!["No parties".into()],
            latitude: Some(41.9028),
            longitude: Some(12.4964),
            photos: vec!["https://example.com/photo.jpg".into()],
            bedrooms: Some(3),
            beds: Some(4),
            bathrooms: Some(2.0),
            max_guests: Some(6),
            check_in_time: Some("15:00".into()),
            check_out_time: Some("11:00".into()),
            host_id: None,
            host_is_superhost: None,
            host_response_rate: None,
            host_response_time: None,
            host_joined: None,
            host_total_listings: None,
            host_languages: vec![],
            cancellation_policy: None,
            instant_book: None,
            cleaning_fee: None,
            service_fee: None,
            neighborhood: None,
        };
        let s = detail.to_string();
        assert!(s.contains("# Villa Rosa"));
        assert!(s.contains("Location: Rome, Italy"));
        assert!(s.contains("€200"));
        assert!(s.contains("4.90"));
        assert!(s.contains("Type: Villa"));
        assert!(s.contains("Host: Marco"));
        assert!(s.contains("## Description"));
        assert!(s.contains("A beautiful villa"));
        assert!(s.contains("## Amenities"));
        assert!(s.contains("WiFi, Pool"));
        assert!(s.contains("## House Rules"));
        assert!(s.contains("No parties"));
        assert!(s.contains("URL: https://airbnb.com/rooms/789"));
    }

    #[test]
    fn listing_detail_display_minimal() {
        let detail = ListingDetail {
            id: "1".into(),
            name: "Simple Room".into(),
            location: "London".into(),
            description: String::new(),
            price_per_night: 50.0,
            currency: "£".into(),
            rating: None,
            review_count: 0,
            property_type: None,
            host_name: None,
            url: "https://airbnb.com/rooms/1".into(),
            amenities: vec![],
            house_rules: vec![],
            latitude: None,
            longitude: None,
            photos: vec![],
            bedrooms: None,
            beds: None,
            bathrooms: None,
            max_guests: None,
            check_in_time: None,
            check_out_time: None,
            host_id: None,
            host_is_superhost: None,
            host_response_rate: None,
            host_response_time: None,
            host_joined: None,
            host_total_listings: None,
            host_languages: vec![],
            cancellation_policy: None,
            instant_book: None,
            cleaning_fee: None,
            service_fee: None,
            neighborhood: None,
        };
        let s = detail.to_string();
        assert!(s.contains("# Simple Room"));
        assert!(!s.contains("## Description"));
        assert!(!s.contains("## Amenities"));
        assert!(!s.contains("## House Rules"));
        assert!(!s.contains("Rating"));
        assert!(!s.contains("Type:"));
        assert!(!s.contains("Host:"));
    }

    #[test]
    fn listing_detail_display_with_bedrooms() {
        let detail = ListingDetail {
            id: "2".into(),
            name: "Apt".into(),
            location: "NYC".into(),
            description: String::new(),
            price_per_night: 100.0,
            currency: "$".into(),
            rating: None,
            review_count: 0,
            property_type: None,
            host_name: None,
            url: "https://airbnb.com/rooms/2".into(),
            amenities: vec![],
            house_rules: vec![],
            latitude: None,
            longitude: None,
            photos: vec![],
            bedrooms: Some(2),
            beds: Some(3),
            bathrooms: Some(1.5),
            max_guests: None,
            check_in_time: None,
            check_out_time: None,
            host_id: None,
            host_is_superhost: None,
            host_response_rate: None,
            host_response_time: None,
            host_joined: None,
            host_total_listings: None,
            host_languages: vec![],
            cancellation_policy: None,
            instant_book: None,
            cleaning_fee: None,
            service_fee: None,
            neighborhood: None,
        };
        let s = detail.to_string();
        assert!(s.contains("Bedrooms: 2"));
        assert!(s.contains("Beds: 3"));
        assert!(s.contains("Bathrooms: 1.5"));
    }

    #[test]
    fn listing_display_with_host_id() {
        let listing = Listing {
            id: "100".into(),
            name: "Test Place".into(),
            location: "Berlin".into(),
            price_per_night: 90.0,
            currency: "$".into(),
            rating: Some(4.0),
            review_count: 5,
            thumbnail_url: None,
            property_type: None,
            host_name: None,
            host_id: Some("12345".into()),
            url: "https://airbnb.com/rooms/100".into(),
            is_superhost: None,
            is_guest_favorite: None,
            instant_book: None,
            total_price: None,
            photos: vec![],
            latitude: None,
            longitude: None,
        };
        let s = listing.to_string();
        assert!(
            s.contains("Host ID: 12345"),
            "Display should contain 'Host ID: 12345', got: {s}"
        );
    }

    #[test]
    fn listing_display_with_total_price() {
        let listing = Listing {
            id: "101".into(),
            name: "Cozy Flat".into(),
            location: "Madrid".into(),
            price_per_night: 100.0,
            currency: "$".into(),
            rating: None,
            review_count: 0,
            thumbnail_url: None,
            property_type: None,
            host_name: None,
            host_id: None,
            url: "https://airbnb.com/rooms/101".into(),
            is_superhost: None,
            is_guest_favorite: None,
            instant_book: None,
            total_price: Some(500.0),
            photos: vec![],
            latitude: None,
            longitude: None,
        };
        let s = listing.to_string();
        assert!(
            s.contains("Total: $500"),
            "Display should contain 'Total: $500', got: {s}"
        );
    }

    #[test]
    fn listing_display_guest_favorite() {
        let listing = Listing {
            id: "102".into(),
            name: "Great Stay".into(),
            location: "Lisbon".into(),
            price_per_night: 120.0,
            currency: "$".into(),
            rating: Some(4.9),
            review_count: 30,
            thumbnail_url: None,
            property_type: None,
            host_name: None,
            host_id: None,
            url: "https://airbnb.com/rooms/102".into(),
            is_superhost: None,
            is_guest_favorite: Some(true),
            instant_book: None,
            total_price: None,
            photos: vec![],
            latitude: None,
            longitude: None,
        };
        let s = listing.to_string();
        assert!(
            s.contains("Guest Favorite"),
            "Display should contain 'Guest Favorite', got: {s}"
        );
    }

    #[test]
    fn listing_detail_with_fees() {
        let detail = ListingDetail {
            id: "200".into(),
            name: "Fee Test".into(),
            location: "Tokyo".into(),
            description: String::new(),
            price_per_night: 150.0,
            currency: "$".into(),
            rating: None,
            review_count: 0,
            property_type: None,
            host_name: None,
            url: "https://airbnb.com/rooms/200".into(),
            amenities: vec![],
            house_rules: vec![],
            latitude: None,
            longitude: None,
            photos: vec![],
            bedrooms: None,
            beds: None,
            bathrooms: None,
            max_guests: None,
            check_in_time: None,
            check_out_time: None,
            host_id: None,
            host_is_superhost: None,
            host_response_rate: None,
            host_response_time: None,
            host_joined: None,
            host_total_listings: None,
            host_languages: vec![],
            cancellation_policy: None,
            instant_book: None,
            cleaning_fee: Some(75.0),
            service_fee: Some(45.0),
            neighborhood: None,
        };
        let s = detail.to_string();
        assert!(
            s.contains("Fees:"),
            "Display should contain 'Fees:', got: {s}"
        );
        assert!(
            s.contains("Cleaning $75"),
            "Display should contain 'Cleaning $75', got: {s}"
        );
        assert!(
            s.contains("Service $45"),
            "Display should contain 'Service $45', got: {s}"
        );
    }

    #[test]
    fn listing_detail_with_host_details() {
        let detail = ListingDetail {
            id: "201".into(),
            name: "Host Detail Test".into(),
            location: "Rome".into(),
            description: "Nice place".into(),
            price_per_night: 100.0,
            currency: "$".into(),
            rating: Some(4.5),
            review_count: 10,
            property_type: Some("Apartment".into()),
            host_name: Some("Maria".into()),
            url: "https://airbnb.com/rooms/201".into(),
            amenities: vec![],
            house_rules: vec![],
            latitude: None,
            longitude: None,
            photos: vec![],
            bedrooms: None,
            beds: None,
            bathrooms: None,
            max_guests: None,
            check_in_time: None,
            check_out_time: None,
            host_id: None,
            host_is_superhost: Some(true),
            host_response_rate: Some("99%".into()),
            host_response_time: Some("within an hour".into()),
            host_joined: Some("2019".into()),
            host_total_listings: Some(3),
            host_languages: vec!["English".into(), "Italian".into()],
            cancellation_policy: None,
            instant_book: None,
            cleaning_fee: None,
            service_fee: None,
            neighborhood: None,
        };
        let s = detail.to_string();
        assert!(
            s.contains("Host: Maria"),
            "Display should contain 'Host: Maria', got: {s}"
        );
        assert!(
            s.contains("(Superhost)"),
            "Display should contain '(Superhost)', got: {s}"
        );
        assert!(
            s.contains("Response rate: 99%"),
            "Display should contain 'Response rate: 99%', got: {s}"
        );
        assert!(
            s.contains("Response time: within an hour"),
            "Display should contain 'Response time: within an hour', got: {s}"
        );
        assert!(
            s.contains("Host since: 2019"),
            "Display should contain 'Host since: 2019', got: {s}"
        );
        assert!(
            s.contains("Host listings: 3"),
            "Display should contain 'Host listings: 3', got: {s}"
        );
        assert!(
            s.contains("Languages: English, Italian"),
            "Display should contain 'Languages: English, Italian', got: {s}"
        );
    }
}
