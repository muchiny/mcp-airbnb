#![allow(clippy::cast_possible_truncation)]

use std::time::Duration;

use proptest::prelude::*;

use mcp_airbnb::adapters::cache::memory_cache::MemoryCache;
use mcp_airbnb::domain::analytics::{compute_neighborhood_stats, compute_occupancy_estimate};
use mcp_airbnb::domain::calendar::{CalendarDay, PriceCalendar};
use mcp_airbnb::domain::listing::Listing;
use mcp_airbnb::domain::search_params::SearchParams;
use mcp_airbnb::ports::cache::ListingCache;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn arb_calendar_day() -> impl Strategy<Value = CalendarDay> {
    (
        "[0-9]{4}-[0-9]{2}-[0-9]{2}",
        prop::option::of(1.0..10000.0_f64),
        any::<bool>(),
    )
        .prop_map(|(date, price, available)| CalendarDay {
            date,
            price,
            available,
            min_nights: None,
            max_nights: None,
            closed_to_arrival: None,
            closed_to_departure: None,
        })
}

fn arb_price_calendar() -> impl Strategy<Value = PriceCalendar> {
    prop::collection::vec(arb_calendar_day(), 0..100).prop_map(|days| PriceCalendar {
        listing_id: "test".to_string(),
        currency: "USD".to_string(),
        days,
        average_price: None,
        occupancy_rate: None,
        min_price: None,
        max_price: None,
    })
}

fn arb_listing() -> impl Strategy<Value = Listing> {
    (
        0.0..5000.0_f64,                             // price
        prop::option::of(1.0..5.0_f64),              // rating
        0..1000_u32,                                 // review_count
        prop::option::of(prop::bool::ANY),           // is_superhost
        prop::option::of("[A-Za-z ]{1,20}".boxed()), // property_type
    )
        .prop_map(
            |(price, rating, review_count, is_superhost, property_type)| Listing {
                id: "1".to_string(),
                name: "Test".to_string(),
                location: "Paris".to_string(),
                price_per_night: price,
                currency: "USD".to_string(),
                rating,
                review_count,
                thumbnail_url: None,
                property_type,
                host_name: None,
                url: "https://airbnb.com/rooms/1".to_string(),
                is_superhost,
                is_guest_favorite: None,
                instant_book: None,
                total_price: None,
                photos: Vec::new(),
                latitude: None,
                longitude: None,
            },
        )
}

// ---------------------------------------------------------------------------
// SearchParams::validate() properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn prop_valid_params_always_pass(
        location in "[A-Za-z]{1,30}",
        offset in 1..365_i64,
        duration in 1..30_i64,
    ) {
        let base = chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let checkin = base + chrono::TimeDelta::days(offset);
        let checkout = checkin + chrono::TimeDelta::days(duration);
        let params = SearchParams {
            location,
            checkin: Some(checkin.format("%Y-%m-%d").to_string()),
            checkout: Some(checkout.format("%Y-%m-%d").to_string()),
            adults: None,
            children: None,
            infants: None,
            pets: None,
            min_price: None,
            max_price: None,
            property_type: None,
            cursor: None,
        };
        prop_assert!(params.validate().is_ok());
    }

    #[test]
    fn prop_empty_location_always_fails(
        spaces in " {0,10}",
    ) {
        let params = SearchParams {
            location: spaces,
            ..Default::default()
        };
        prop_assert!(params.validate().is_err());
    }

    #[test]
    fn prop_checkin_only_always_fails(
        offset in 1..365_i64,
    ) {
        let base = chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let checkin = base + chrono::TimeDelta::days(offset);
        let params = SearchParams {
            location: "Paris".into(),
            checkin: Some(checkin.format("%Y-%m-%d").to_string()),
            checkout: None,
            ..Default::default()
        };
        prop_assert!(params.validate().is_err());
    }

    #[test]
    fn prop_checkout_before_checkin_fails(
        offset in 1..365_i64,
        duration in 1..30_i64,
    ) {
        let base = chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let later = base + chrono::TimeDelta::days(offset + duration);
        let earlier = base + chrono::TimeDelta::days(offset);
        let params = SearchParams {
            location: "Paris".into(),
            checkin: Some(later.format("%Y-%m-%d").to_string()),
            checkout: Some(earlier.format("%Y-%m-%d").to_string()),
            ..Default::default()
        };
        prop_assert!(params.validate().is_err());
    }

    #[test]
    fn prop_min_gt_max_price_fails(
        min in 1..10000_u32,
        delta in 1..5000_u32,
    ) {
        let params = SearchParams {
            location: "Paris".into(),
            min_price: Some(min + delta),
            max_price: Some(min),
            ..Default::default()
        };
        prop_assert!(params.validate().is_err());
    }
}

// ---------------------------------------------------------------------------
// PriceCalendar::compute_stats() properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn prop_avg_between_min_max(mut cal in arb_price_calendar()) {
        cal.compute_stats();
        if let (Some(avg), Some(min), Some(max)) = (cal.average_price, cal.min_price, cal.max_price) {
            prop_assert!(avg >= min - f64::EPSILON, "avg {avg} < min {min}");
            prop_assert!(avg <= max + f64::EPSILON, "avg {avg} > max {max}");
        }
    }

    #[test]
    fn prop_occupancy_0_to_100(mut cal in arb_price_calendar()) {
        cal.compute_stats();
        if let Some(occ) = cal.occupancy_rate {
            prop_assert!(occ >= 0.0, "occupancy {occ} < 0");
            prop_assert!(occ <= 100.0 + f64::EPSILON, "occupancy {occ} > 100");
        }
    }

    #[test]
    fn prop_empty_days_all_none(
        listing_id in "[a-z]{1,10}",
    ) {
        let mut cal = PriceCalendar {
            listing_id,
            currency: "USD".to_string(),
            days: vec![],
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        prop_assert!(cal.average_price.is_none());
        prop_assert!(cal.min_price.is_none());
        prop_assert!(cal.max_price.is_none());
        prop_assert!(cal.occupancy_rate.is_none());
    }

    #[test]
    fn prop_all_available_zero_occupancy(
        n in 1..50_usize,
        price in 1.0..1000.0_f64,
    ) {
        let mut cal = PriceCalendar {
            listing_id: "t".to_string(),
            currency: "USD".to_string(),
            days: (0..n).map(|i| CalendarDay {
                date: format!("2026-01-{:02}", (i % 28) + 1),
                price: Some(price),
                available: true,
                min_nights: None,
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
            }).collect(),
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        prop_assert!((cal.occupancy_rate.unwrap() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn prop_all_unavailable_full_occupancy(
        n in 1..50_usize,
    ) {
        let mut cal = PriceCalendar {
            listing_id: "t".to_string(),
            currency: "USD".to_string(),
            days: (0..n).map(|i| CalendarDay {
                date: format!("2026-01-{:02}", (i % 28) + 1),
                price: Some(100.0),
                available: false,
                min_nights: None,
                max_nights: None,
                closed_to_arrival: None,
                closed_to_departure: None,
            }).collect(),
            average_price: None,
            occupancy_rate: None,
            min_price: None,
            max_price: None,
        };
        cal.compute_stats();
        prop_assert!((cal.occupancy_rate.unwrap() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn prop_min_le_max(mut cal in arb_price_calendar()) {
        cal.compute_stats();
        if let (Some(min), Some(max)) = (cal.min_price, cal.max_price) {
            prop_assert!(min <= max + f64::EPSILON, "min {min} > max {max}");
        }
    }
}

// ---------------------------------------------------------------------------
// compute_neighborhood_stats() properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn prop_total_matches_input(
        listings in prop::collection::vec(arb_listing(), 0..30),
    ) {
        let stats = compute_neighborhood_stats("Paris", &listings);
        prop_assert_eq!(stats.total_listings, listings.len() as u32);
    }

    #[test]
    fn prop_avg_in_price_range(
        listings in prop::collection::vec(arb_listing(), 1..30),
    ) {
        let stats = compute_neighborhood_stats("Paris", &listings);
        if let (Some(avg), Some((min, max))) = (stats.average_price, stats.price_range) {
            prop_assert!(avg >= min - f64::EPSILON, "avg {avg} < min {min}");
            prop_assert!(avg <= max + f64::EPSILON, "avg {avg} > max {max}");
        }
    }

    #[test]
    fn prop_superhost_pct_bounded(
        listings in prop::collection::vec(arb_listing(), 1..30),
    ) {
        let stats = compute_neighborhood_stats("Paris", &listings);
        if let Some(pct) = stats.superhost_percentage {
            prop_assert!(pct >= 0.0, "superhost% {pct} < 0");
            prop_assert!(pct <= 100.0 + f64::EPSILON, "superhost% {pct} > 100");
        }
    }

    #[test]
    fn prop_property_type_counts_sum(
        listings in prop::collection::vec(arb_listing(), 0..30),
    ) {
        let stats = compute_neighborhood_stats("Paris", &listings);
        let sum: u32 = stats.property_type_distribution.iter().map(|p| p.count).sum();
        prop_assert_eq!(sum, stats.total_listings);
    }
}

// ---------------------------------------------------------------------------
// compute_occupancy_estimate() properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn prop_occupied_plus_available_eq_total(cal in arb_price_calendar()) {
        let est = compute_occupancy_estimate("test", &cal);
        prop_assert_eq!(
            est.occupied_days + est.available_days,
            est.total_days,
            "occupied {} + available {} != total {}",
            est.occupied_days, est.available_days, est.total_days
        );
    }

    #[test]
    fn prop_occupancy_rate_bounded(cal in arb_price_calendar()) {
        let est = compute_occupancy_estimate("test", &cal);
        prop_assert!(est.occupancy_rate >= 0.0, "rate {} < 0", est.occupancy_rate);
        prop_assert!(est.occupancy_rate <= 100.0 + f64::EPSILON, "rate {} > 100", est.occupancy_rate);
    }
}

// ---------------------------------------------------------------------------
// MemoryCache properties
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn prop_set_then_get_returns_value(
        key in "[a-z]{1,20}",
        value in "[a-zA-Z0-9]{1,100}",
    ) {
        let cache = MemoryCache::new(100);
        cache.set(&key, &value, Duration::from_secs(3600));
        let result = cache.get(&key);
        prop_assert_eq!(result, Some(value));
    }

    #[test]
    fn prop_capacity_respected(
        n in 1..200_usize,
    ) {
        let capacity = 50;
        let cache = MemoryCache::new(capacity);
        for i in 0..n {
            cache.set(&format!("k{i}"), &format!("v{i}"), Duration::from_secs(3600));
        }
        // Count how many keys are still present
        let mut found = 0;
        for i in 0..n {
            if cache.get(&format!("k{i}")).is_some() {
                found += 1;
            }
        }
        prop_assert!(found <= capacity, "found {found} > capacity {capacity}");
    }
}
