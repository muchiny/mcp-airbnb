#![allow(clippy::too_many_lines)]

use std::sync::Arc;

use async_trait::async_trait;

use mcp_airbnb::domain::analytics::{
    HostProfile, MonthlyOccupancy, NeighborhoodStats, OccupancyEstimate, PropertyTypeCount,
};
use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar};
use mcp_airbnb::domain::listing::{Listing, ListingDetail, SearchResult};
use mcp_airbnb::domain::review::{Review, ReviewsPage, ReviewsSummary};
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::error::Result;
use mcp_airbnb::mcp::server::AirbnbMcpServer;
use mcp_airbnb::ports::airbnb_client::AirbnbClient;

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientInfo, ReadResourceRequestParams};
use rmcp::{ClientHandler, ServiceExt};

// ---------------------------------------------------------------------------
// AnalyticalMock — returns varied, realistic data to exercise analytics
// ---------------------------------------------------------------------------

struct AnalyticalMock;

#[async_trait]
impl AirbnbClient for AnalyticalMock {
    async fn search_listings(&self, params: &SearchParams) -> Result<SearchResult> {
        Ok(SearchResult {
            listings: vec![
                Listing {
                    id: "1".into(),
                    name: "Charming Studio in Le Marais".into(),
                    location: params.location.clone(),
                    price_per_night: 95.0,
                    currency: "€".into(),
                    rating: Some(4.82),
                    review_count: 127,
                    thumbnail_url: Some("https://example.com/photo1.jpg".into()),
                    property_type: Some("Entire home".into()),
                    host_name: Some("Marie".into()),
                    host_id: Some("host-marie".into()),
                    url: "https://www.airbnb.com/rooms/1".into(),
                    is_superhost: Some(true),
                    is_guest_favorite: Some(true),
                    instant_book: Some(true),
                    total_price: Some(285.0),
                    photos: vec!["https://example.com/p1.jpg".into()],
                    latitude: Some(48.8566),
                    longitude: Some(2.3522),
                },
                Listing {
                    id: "2".into(),
                    name: "Modern Loft near Eiffel Tower".into(),
                    location: params.location.clone(),
                    price_per_night: 175.0,
                    currency: "€".into(),
                    rating: Some(4.55),
                    review_count: 43,
                    thumbnail_url: None,
                    property_type: Some("Entire home".into()),
                    host_name: Some("Marie".into()),
                    host_id: Some("host-marie".into()),
                    url: "https://www.airbnb.com/rooms/2".into(),
                    is_superhost: Some(true),
                    is_guest_favorite: None,
                    instant_book: Some(false),
                    total_price: Some(525.0),
                    photos: vec![],
                    latitude: Some(48.8584),
                    longitude: Some(2.2945),
                },
                Listing {
                    id: "3".into(),
                    name: "Cozy Room in Montmartre".into(),
                    location: params.location.clone(),
                    price_per_night: 55.0,
                    currency: "€".into(),
                    rating: Some(4.91),
                    review_count: 210,
                    thumbnail_url: None,
                    property_type: Some("Private room".into()),
                    host_name: Some("Jean-Pierre".into()),
                    host_id: Some("host-jp".into()),
                    url: "https://www.airbnb.com/rooms/3".into(),
                    is_superhost: None,
                    is_guest_favorite: None,
                    instant_book: None,
                    total_price: None,
                    photos: vec![],
                    latitude: Some(48.8867),
                    longitude: Some(2.3431),
                },
                Listing {
                    id: "4".into(),
                    name: "Luxury Apartment Saint-Germain".into(),
                    location: params.location.clone(),
                    price_per_night: 320.0,
                    currency: "€".into(),
                    rating: Some(4.97),
                    review_count: 15,
                    thumbnail_url: None,
                    property_type: Some("Entire home".into()),
                    host_name: Some("Sophie".into()),
                    host_id: Some("host-sophie".into()),
                    url: "https://www.airbnb.com/rooms/4".into(),
                    is_superhost: Some(true),
                    is_guest_favorite: Some(true),
                    instant_book: Some(true),
                    total_price: None,
                    photos: vec![
                        "https://example.com/lux1.jpg".into(),
                        "https://example.com/lux2.jpg".into(),
                    ],
                    latitude: None,
                    longitude: None,
                },
            ],
            total_count: Some(4),
            next_cursor: None,
        })
    }

    async fn get_listing_detail(&self, id: &str) -> Result<ListingDetail> {
        match id {
            "1" => Ok(ListingDetail {
                id: "1".into(),
                name: "Charming Studio in Le Marais".into(),
                location: "Paris, France".into(),
                description: "A beautifully renovated studio in the heart of Le Marais.".into(),
                price_per_night: 95.0,
                currency: "€".into(),
                rating: Some(4.82),
                review_count: 127,
                property_type: Some("Entire home".into()),
                host_name: Some("Marie".into()),
                url: "https://www.airbnb.com/rooms/1".into(),
                amenities: vec![
                    "WiFi".into(),
                    "Kitchen".into(),
                    "Washer".into(),
                    "Air conditioning".into(),
                    "Hair dryer".into(),
                    "Iron".into(),
                    "TV".into(),
                    "Coffee maker".into(),
                    "Dishwasher".into(),
                    "Elevator".into(),
                ],
                house_rules: vec!["No smoking".into(), "No parties".into()],
                latitude: Some(48.8566),
                longitude: Some(2.3522),
                photos: vec![
                    "https://example.com/1a.jpg".into(),
                    "https://example.com/1b.jpg".into(),
                    "https://example.com/1c.jpg".into(),
                    "https://example.com/1d.jpg".into(),
                    "https://example.com/1e.jpg".into(),
                ],
                bedrooms: Some(1),
                beds: Some(1),
                bathrooms: Some(1.0),
                max_guests: Some(2),
                check_in_time: Some("15:00".into()),
                check_out_time: Some("11:00".into()),
                host_id: Some("host-marie".into()),
                host_is_superhost: Some(true),
                host_response_rate: Some("100%".into()),
                host_response_time: Some("within an hour".into()),
                host_joined: Some("2018".into()),
                host_total_listings: Some(3),
                host_languages: vec!["English".into(), "French".into(), "Spanish".into()],
                cancellation_policy: Some("Moderate".into()),
                instant_book: Some(true),
                cleaning_fee: Some(35.0),
                service_fee: Some(15.0),
                neighborhood: Some("Le Marais".into()),
            }),
            "2" => Ok(ListingDetail {
                id: "2".into(),
                name: "Modern Loft near Eiffel Tower".into(),
                location: "Paris, France".into(),
                description: "Spacious loft with stunning Eiffel Tower view.".into(),
                price_per_night: 175.0,
                currency: "€".into(),
                rating: Some(4.55),
                review_count: 43,
                property_type: Some("Entire home".into()),
                host_name: Some("Marie".into()),
                url: "https://www.airbnb.com/rooms/2".into(),
                amenities: vec![
                    "WiFi".into(),
                    "Kitchen".into(),
                    "Washer".into(),
                    "Dryer".into(),
                    "Air conditioning".into(),
                    "TV".into(),
                    "Balcony".into(),
                    "Free parking".into(),
                ],
                house_rules: vec!["No smoking".into()],
                latitude: Some(48.8584),
                longitude: Some(2.2945),
                photos: vec![
                    "https://example.com/2a.jpg".into(),
                    "https://example.com/2b.jpg".into(),
                    "https://example.com/2c.jpg".into(),
                ],
                bedrooms: Some(2),
                beds: Some(2),
                bathrooms: Some(1.0),
                max_guests: Some(4),
                check_in_time: Some("14:00".into()),
                check_out_time: Some("10:00".into()),
                host_id: Some("host-marie".into()),
                host_is_superhost: Some(true),
                host_response_rate: Some("98%".into()),
                host_response_time: Some("within a few hours".into()),
                host_joined: Some("2018".into()),
                host_total_listings: Some(3),
                host_languages: vec!["English".into(), "French".into()],
                cancellation_policy: Some("Strict".into()),
                instant_book: Some(false),
                cleaning_fee: Some(50.0),
                service_fee: Some(25.0),
                neighborhood: Some("7th arrondissement".into()),
            }),
            "3" => Ok(ListingDetail {
                id: "3".into(),
                name: "Cozy Room in Montmartre".into(),
                location: "Paris, France".into(),
                description: "A private room in a charming Montmartre apartment.".into(),
                price_per_night: 55.0,
                currency: "€".into(),
                rating: Some(4.91),
                review_count: 210,
                property_type: Some("Private room".into()),
                host_name: Some("Jean-Pierre".into()),
                url: "https://www.airbnb.com/rooms/3".into(),
                amenities: vec!["WiFi".into(), "Kitchen".into(), "Washer".into()],
                house_rules: vec!["No smoking".into(), "Quiet hours after 10pm".into()],
                latitude: Some(48.8867),
                longitude: Some(2.3431),
                photos: vec!["https://example.com/3a.jpg".into()],
                bedrooms: Some(1),
                beds: Some(1),
                bathrooms: Some(1.0),
                max_guests: Some(2),
                check_in_time: None,
                check_out_time: None,
                host_id: Some("host-jp".into()),
                host_is_superhost: None,
                host_response_rate: None,
                host_response_time: None,
                host_joined: Some("2021".into()),
                host_total_listings: Some(1),
                host_languages: vec!["French".into()],
                cancellation_policy: Some("Flexible".into()),
                instant_book: None,
                cleaning_fee: None,
                service_fee: None,
                neighborhood: Some("Montmartre".into()),
            }),
            _ => Ok(ListingDetail {
                id: id.into(),
                name: "Luxury Apartment Saint-Germain".into(),
                location: "Paris, France".into(),
                description: "An exquisite apartment in Saint-Germain-des-Pres.".into(),
                price_per_night: 320.0,
                currency: "€".into(),
                rating: Some(4.97),
                review_count: 15,
                property_type: Some("Entire home".into()),
                host_name: Some("Sophie".into()),
                url: format!("https://www.airbnb.com/rooms/{id}"),
                amenities: vec![
                    "WiFi".into(),
                    "Kitchen".into(),
                    "Washer".into(),
                    "Dryer".into(),
                    "Air conditioning".into(),
                    "Heating".into(),
                    "TV".into(),
                    "Coffee maker".into(),
                    "Dishwasher".into(),
                    "Hair dryer".into(),
                    "Iron".into(),
                    "Elevator".into(),
                    "Balcony".into(),
                    "Free parking".into(),
                    "Pool".into(),
                    "Gym".into(),
                ],
                house_rules: vec!["No smoking".into(), "No pets".into()],
                latitude: None,
                longitude: None,
                photos: vec![
                    "https://example.com/4a.jpg".into(),
                    "https://example.com/4b.jpg".into(),
                    "https://example.com/4c.jpg".into(),
                    "https://example.com/4d.jpg".into(),
                    "https://example.com/4e.jpg".into(),
                    "https://example.com/4f.jpg".into(),
                    "https://example.com/4g.jpg".into(),
                ],
                bedrooms: Some(3),
                beds: Some(4),
                bathrooms: Some(2.0),
                max_guests: Some(6),
                check_in_time: Some("16:00".into()),
                check_out_time: Some("11:00".into()),
                host_id: Some("host-sophie".into()),
                host_is_superhost: Some(true),
                host_response_rate: Some("100%".into()),
                host_response_time: Some("within an hour".into()),
                host_joined: Some("2016".into()),
                host_total_listings: Some(5),
                host_languages: vec!["English".into(), "French".into(), "Italian".into()],
                cancellation_policy: Some("Strict".into()),
                instant_book: Some(true),
                cleaning_fee: Some(80.0),
                service_fee: Some(45.0),
                neighborhood: Some("Saint-Germain-des-Pres".into()),
            }),
        }
    }

    async fn get_reviews(&self, id: &str, _cursor: Option<&str>) -> Result<ReviewsPage> {
        Ok(ReviewsPage {
            listing_id: id.into(),
            summary: Some(ReviewsSummary {
                overall_rating: 4.82,
                total_reviews: 127,
                cleanliness: Some(4.9),
                accuracy: Some(4.8),
                communication: Some(4.95),
                location: Some(4.7),
                check_in: Some(4.85),
                value: Some(4.6),
            }),
            reviews: vec![
                Review {
                    author: "Sarah".into(),
                    date: "2025-12-15".into(),
                    rating: Some(5.0),
                    comment: "Absolutely wonderful stay! The apartment was spotlessly clean and the location is perfect. Marie was incredibly responsive and helpful.".into(),
                    response: Some("Thank you Sarah!".into()),
                    reviewer_location: Some("New York, USA".into()),
                    language: Some("en".into()),
                    is_translated: None,
                },
                Review {
                    author: "Thomas".into(),
                    date: "2025-11-28".into(),
                    rating: Some(4.0),
                    comment: "Good location and clean apartment. The bed was uncomfortable and WiFi was slow. Communication with the host was excellent though.".into(),
                    response: None,
                    reviewer_location: Some("London, UK".into()),
                    language: Some("en".into()),
                    is_translated: None,
                },
                Review {
                    author: "Akiko".into(),
                    date: "2025-11-10".into(),
                    rating: Some(5.0),
                    comment: "Perfect location in Le Marais! Very clean, well-equipped kitchen. The air conditioning was a lifesaver in summer. Highly recommend!".into(),
                    response: None,
                    reviewer_location: Some("Tokyo, Japan".into()),
                    language: Some("en".into()),
                    is_translated: Some(true),
                },
                Review {
                    author: "Marco".into(),
                    date: "2025-10-22".into(),
                    rating: Some(5.0),
                    comment: "Best Airbnb we have ever stayed in. Incredible value for the location. Spotless cleanliness, comfortable bed, fast WiFi.".into(),
                    response: Some("Grazie Marco!".into()),
                    reviewer_location: Some("Rome, Italy".into()),
                    language: Some("en".into()),
                    is_translated: None,
                },
                Review {
                    author: "Lisa".into(),
                    date: "2025-10-05".into(),
                    rating: Some(3.0),
                    comment: "The location is great but the apartment was smaller than expected. Noisy street at night. Check-in was smooth and host responded quickly.".into(),
                    response: None,
                    reviewer_location: Some("Berlin, Germany".into()),
                    language: Some("en".into()),
                    is_translated: None,
                },
            ],
            next_cursor: None,
        })
    }

    async fn get_price_calendar(&self, id: &str, _months: u32) -> Result<PriceCalendar> {
        let days = vec![
            CalendarDay {
                date: "2025-06-02".into(),
                price: Some(95.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-03".into(),
                price: Some(95.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-04".into(),
                price: Some(95.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            // 1-night orphan gap
            CalendarDay {
                date: "2025-06-05".into(),
                price: Some(105.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-06".into(),
                price: Some(130.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-07".into(),
                price: Some(140.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-08".into(),
                price: Some(140.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            // Available stretch with weekend premium
            CalendarDay {
                date: "2025-06-09".into(),
                price: Some(90.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-10".into(),
                price: Some(90.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-11".into(),
                price: Some(90.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-12".into(),
                price: Some(90.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-13".into(),
                price: Some(120.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-14".into(),
                price: Some(130.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-15".into(),
                price: Some(130.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            // Mostly booked with 2-night gap
            CalendarDay {
                date: "2025-06-16".into(),
                price: Some(95.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-17".into(),
                price: Some(95.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-18".into(),
                price: Some(95.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-19".into(),
                price: Some(100.0),
                available: true,
                min_nights: Some(3),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-20".into(),
                price: Some(110.0),
                available: true,
                min_nights: Some(3),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-21".into(),
                price: Some(140.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-22".into(),
                price: Some(140.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            // Peak summer pricing
            CalendarDay {
                date: "2025-06-23".into(),
                price: Some(110.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-24".into(),
                price: Some(110.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-25".into(),
                price: Some(115.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-26".into(),
                price: Some(115.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-27".into(),
                price: Some(145.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-28".into(),
                price: Some(155.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-29".into(),
                price: Some(150.0),
                available: false,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
            CalendarDay {
                date: "2025-06-30".into(),
                price: Some(100.0),
                available: true,
                min_nights: Some(2),
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
                unavailability_reason: None,
            },
        ];

        Ok(PriceCalendar {
            listing_id: id.into(),
            currency: "€".into(),
            days,
            average_price: Some(113.0),
            occupancy_rate: Some(48.3),
            min_price: Some(90.0),
            max_price: Some(155.0),
        })
    }

    async fn get_host_profile(&self, _listing_id: &str) -> Result<HostProfile> {
        Ok(HostProfile {
            host_id: Some("host-marie".into()),
            name: "Marie".into(),
            is_superhost: Some(true),
            response_rate: Some("100%".into()),
            response_time: Some("within an hour".into()),
            member_since: Some("2018".into()),
            languages: vec!["English".into(), "French".into(), "Spanish".into()],
            total_listings: Some(3),
            description: Some("Passionate Parisian host.".into()),
            profile_picture_url: Some("https://example.com/marie.jpg".into()),
            identity_verified: Some(true),
        })
    }

    async fn get_neighborhood_stats(&self, params: &SearchParams) -> Result<NeighborhoodStats> {
        let (avg, median, range, rating, total, superhost) = match params.location.as_str() {
            "Paris" | "Paris, France" => (145.0, 125.0, (45.0, 550.0), 4.65, 1250, 35.0),
            "London" | "London, UK" => (165.0, 140.0, (55.0, 800.0), 4.52, 980, 28.0),
            "Barcelona" | "Barcelona, Spain" => (110.0, 95.0, (35.0, 400.0), 4.71, 870, 42.0),
            _ => (120.0, 100.0, (40.0, 500.0), 4.60, 500, 30.0),
        };

        Ok(NeighborhoodStats {
            location: params.location.clone(),
            total_listings: total,
            average_price: Some(avg),
            median_price: Some(median),
            price_range: Some(range),
            average_rating: Some(rating),
            property_type_distribution: vec![
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                PropertyTypeCount {
                    property_type: "Entire home".into(),
                    count: (f64::from(total) * 0.6) as u32,
                    percentage: 60.0,
                },
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                PropertyTypeCount {
                    property_type: "Private room".into(),
                    count: (f64::from(total) * 0.3) as u32,
                    percentage: 30.0,
                },
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                PropertyTypeCount {
                    property_type: "Shared room".into(),
                    count: (f64::from(total) * 0.1) as u32,
                    percentage: 10.0,
                },
            ],
            superhost_percentage: Some(superhost),
        })
    }

    async fn get_occupancy_estimate(&self, id: &str, _months: u32) -> Result<OccupancyEstimate> {
        Ok(OccupancyEstimate {
            listing_id: id.into(),
            period_start: "2025-06-01".into(),
            period_end: "2025-08-31".into(),
            total_days: 92,
            occupied_days: 68,
            available_days: 24,
            occupancy_rate: 73.9,
            average_available_price: Some(112.0),
            weekend_avg_price: Some(138.0),
            weekday_avg_price: Some(95.0),
            monthly_breakdown: vec![
                MonthlyOccupancy {
                    month: "June 2025".into(),
                    total_days: 30,
                    occupied_days: 20,
                    available_days: 10,
                    occupancy_rate: 66.7,
                    average_price: Some(105.0),
                },
                MonthlyOccupancy {
                    month: "July 2025".into(),
                    total_days: 31,
                    occupied_days: 26,
                    available_days: 5,
                    occupancy_rate: 83.9,
                    average_price: Some(125.0),
                },
                MonthlyOccupancy {
                    month: "August 2025".into(),
                    total_days: 31,
                    occupied_days: 22,
                    available_days: 9,
                    occupancy_rate: 71.0,
                    average_price: Some(115.0),
                },
            ],
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Dummy client handler required by rmcp to create a client-server pair.
#[derive(Debug, Clone, Default)]
struct DummyClientHandler;

impl ClientHandler for DummyClientHandler {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
}

fn extract_text(result: &CallToolResult) -> String {
    result
        .content
        .first()
        .and_then(|c| c.raw.as_text())
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

#[allow(clippy::needless_pass_by_value)]
fn tool_params(name: &str, args: serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams {
        meta: None,
        name: std::borrow::Cow::Owned(name.to_string()),
        arguments: Some(args.as_object().unwrap().clone()),
        task: None,
    }
}

/// Create a client connected to our mock server over an in-memory transport.
/// Returns the client handle and a join handle for the server task.
async fn setup() -> (
    rmcp::service::RunningService<rmcp::RoleClient, DummyClientHandler>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(65536);

    let server = AirbnbMcpServer::new(Arc::new(AnalyticalMock));
    let server_handle = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });

    let client = DummyClientHandler
        .serve(client_transport)
        .await
        .expect("client should connect");

    (client, server_handle)
}

/// Shut down the client and server cleanly.
async fn teardown(
    client: rmcp::service::RunningService<rmcp::RoleClient, DummyClientHandler>,
    server_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let _ = client.cancel().await;
    let _ = server_handle.await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn compare_listings_by_ids() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_compare_listings",
            serde_json::json!({ "ids": ["1", "2"] }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty(), "Result text should be non-empty");
    assert!(
        text.contains("Listing Comparison"),
        "Should contain comparison header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn compare_listings_by_location() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_compare_listings",
            serde_json::json!({ "location": "Paris", "max_listings": 20 }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Listing Comparison"),
        "Should contain comparison header, got: {text}"
    );
    assert!(
        text.contains("Fetched") || text.contains("listings"),
        "Should reference fetched listings, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn compare_listings_needs_ids_or_location() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_compare_listings",
            serde_json::json!({}),
        ))
        .await
        .expect("call_tool should succeed");

    assert_eq!(
        result.is_error,
        Some(true),
        "Should return error when neither ids nor location provided"
    );
    let text = extract_text(&result);
    assert!(
        text.contains("ids") || text.contains("location"),
        "Error should mention ids or location, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn price_trends_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_price_trends",
            serde_json::json!({ "id": "1", "months": 6 }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Price Trends"),
        "Should contain price trends header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn gap_finder_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_gap_finder",
            serde_json::json!({ "id": "1", "months": 3 }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Gap Analysis"),
        "Should contain gap analysis header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn revenue_estimate_by_id() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_revenue_estimate",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Revenue Estimate"),
        "Should contain revenue estimate header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn revenue_estimate_by_location() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_revenue_estimate",
            serde_json::json!({ "location": "Paris" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Revenue Estimate"),
        "Should contain revenue estimate header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn listing_score_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_listing_score",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Listing Score"),
        "Should contain listing score header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn amenity_analysis_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_amenity_analysis",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Amenity Analysis"),
        "Should contain amenity analysis header, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn market_comparison_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_market_comparison",
            serde_json::json!({ "locations": ["Paris", "London"] }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Market Comparison"),
        "Should contain market comparison header, got: {text}"
    );
    assert!(text.contains("Paris"), "Should mention Paris, got: {text}");
    assert!(
        text.contains("London"),
        "Should mention London, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn market_comparison_needs_two_locations() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_market_comparison",
            serde_json::json!({ "locations": ["Paris"] }),
        ))
        .await
        .expect("call_tool should succeed");

    assert_eq!(
        result.is_error,
        Some(true),
        "Should return error when fewer than 2 locations provided"
    );
    let text = extract_text(&result);
    assert!(
        text.contains('2') || text.contains("locations"),
        "Error should mention minimum locations, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn host_portfolio_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_host_portfolio",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Host Portfolio"),
        "Should contain host portfolio header, got: {text}"
    );
    assert!(
        text.contains("Marie"),
        "Should contain host name Marie, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn review_sentiment_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_review_sentiment",
            serde_json::json!({ "id": "1", "max_pages": 1 }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.to_lowercase().contains("sentiment") || text.contains("Review"),
        "Should contain sentiment analysis content, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn competitive_positioning_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_competitive_positioning",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.contains("Competitive")
            || text.contains("Positioning")
            || text.contains("positioning"),
        "Should contain competitive positioning content, got: {text}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn optimal_pricing_success() {
    let (client, server_handle) = setup().await;

    let result = client
        .call_tool(tool_params(
            "airbnb_optimal_pricing",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let text = extract_text(&result);
    assert!(
        result.is_error.is_none() || result.is_error == Some(false),
        "Expected success but got error: {text}"
    );
    assert!(!text.is_empty());
    assert!(
        text.to_lowercase().contains("pricing") || text.to_lowercase().contains("price"),
        "Should contain pricing recommendation content, got: {text}"
    );

    teardown(client, server_handle).await;
}

// ---------------------------------------------------------------------------
// MCP Resource Handler Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_resource_templates_returns_expected_templates() {
    let (client, server_handle) = setup().await;

    let result = client
        .peer()
        .list_resource_templates(None)
        .await
        .expect("list_resource_templates should succeed");

    let templates = result.resource_templates;
    assert!(
        templates.len() >= 8,
        "Expected at least 8 resource templates, got {}",
        templates.len()
    );

    let uris: Vec<String> = templates
        .iter()
        .map(|t| t.raw.uri_template.clone())
        .collect();
    assert!(uris.iter().any(|u| u.contains("listing/{id}")));
    assert!(uris.iter().any(|u| u.contains("calendar")));
    assert!(uris.iter().any(|u| u.contains("reviews")));
    assert!(uris.iter().any(|u| u.contains("search")));
    assert!(uris.iter().any(|u| u.contains("analysis")));

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn list_resources_empty_initially() {
    let (client, server_handle) = setup().await;

    let result = client
        .peer()
        .list_resources(None)
        .await
        .expect("list_resources should succeed");

    assert!(
        result.resources.is_empty(),
        "Resources should be empty initially"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn list_resources_populated_after_tool_call() {
    let (client, server_handle) = setup().await;

    // Call a tool to populate resources
    let _ = client
        .call_tool(tool_params(
            "airbnb_listing_details",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    let result = client
        .peer()
        .list_resources(None)
        .await
        .expect("list_resources should succeed");

    assert!(
        !result.resources.is_empty(),
        "Resources should be populated after tool call"
    );

    let uris: Vec<String> = result.resources.iter().map(|r| r.raw.uri.clone()).collect();
    assert!(
        uris.iter().any(|u| u.contains("listing/1")),
        "Should contain listing/1 resource, got: {uris:?}"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn read_resource_returns_content() {
    let (client, server_handle) = setup().await;

    // Populate a resource by calling a tool
    let _ = client
        .call_tool(tool_params(
            "airbnb_listing_details",
            serde_json::json!({ "id": "1" }),
        ))
        .await
        .expect("call_tool should succeed");

    // Read the resource
    let result = client
        .peer()
        .read_resource(ReadResourceRequestParams {
            uri: "airbnb://listing/1".into(),
            meta: None,
        })
        .await
        .expect("read_resource should succeed");

    assert!(
        !result.contents.is_empty(),
        "Resource contents should not be empty"
    );

    teardown(client, server_handle).await;
}

#[tokio::test]
async fn read_resource_not_found_returns_error() {
    let (client, server_handle) = setup().await;

    let result = client
        .peer()
        .read_resource(ReadResourceRequestParams {
            uri: "airbnb://listing/nonexistent".into(),
            meta: None,
        })
        .await;

    assert!(
        result.is_err(),
        "read_resource for nonexistent URI should return error"
    );

    teardown(client, server_handle).await;
}
