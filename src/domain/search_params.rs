use chrono::{Days, NaiveDate, Utc};

use crate::domain::limits;
use crate::error::{AirbnbError, Result, quote_input};

/// `property_type` values accepted by the MCP and CLI search filters: the
/// "Type of place" options of Airbnb's own filter panel (Home, Room, Hotel;
/// captured 2026-09). Airbnb no longer offers a shared-room filter.
pub const SUPPORTED_PROPERTY_TYPES: [&str; 3] = ["Entire home", "Private room", "Hotel room"];

/// Listings per search page (`itemsPerGrid` of the airbnb.com web client, 2026-09).
pub const SEARCH_PAGE_SIZE: usize = 18;

/// The `SUPPORTED_PROPERTY_TYPES` entry for a user-supplied property type (case,
/// surrounding whitespace and Airbnb's own labels accepted), or `None` when
/// Airbnb has no such filter.
pub fn canonical_property_type(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "entire home" | "entire home/apt" | "entire place" | "home" => Some("Entire home"),
        "private room" | "room" => Some("Private room"),
        "hotel room" | "hotel" => Some("Hotel room"),
        _ => None,
    }
}

/// Longest accepted `location`, in characters.
pub const MAX_LOCATION_CHARS: usize = 200;

/// Amenity filters of Airbnb's search panel that the tools accept, as
/// `(name, Airbnb amenity id)`. Each id was checked against live results
/// (2026-11): a search with `amenities[]=<id>` returns listings with it.
pub const AMENITY_FILTERS: [(&str, u32); 14] = [
    ("wifi", 4),
    ("air conditioning", 5),
    ("pool", 7),
    ("kitchen", 8),
    ("free parking", 9),
    ("hot tub", 25),
    ("heating", 30),
    ("washer", 33),
    ("dryer", 34),
    ("dedicated workspace", 47),
    ("self check-in", 51),
    ("tv", 58),
    ("bbq grill", 99),
    ("crib", 286),
];

/// Airbnb amenity id of a supported amenity filter (case and surrounding
/// whitespace ignored), or `None` when the tools do not know the amenity.
pub fn amenity_filter_id(name: &str) -> Option<u32> {
    let wanted = name.trim().to_ascii_lowercase();
    AMENITY_FILTERS
        .iter()
        .find(|(known, _)| *known == wanted)
        .map(|(_, id)| *id)
}

/// Names of the supported amenity filters, for error messages and schemas.
pub fn amenity_filter_names() -> String {
    AMENITY_FILTERS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Filters of Airbnb's search panel beyond dates, guests, price and type of
/// place. The default applies no filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    /// Fewest bedrooms a result may have.
    pub min_bedrooms: Option<u32>,
    /// Fewest beds a result may have.
    pub min_beds: Option<u32>,
    /// Fewest bathrooms a result may have.
    pub min_bathrooms: Option<u32>,
    /// Amenities every result must have (names of [`AMENITY_FILTERS`]).
    pub amenities: Vec<String>,
    /// Keep only Guest Favorite listings.
    pub guest_favorite: bool,
    /// Keep only Instant Book listings.
    pub instant_book: bool,
}

impl SearchFilters {
    /// Whether no filter is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Check the room counts (1–16), the amenity names and their number.
    pub fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("min_bedrooms", self.min_bedrooms),
            ("min_beds", self.min_beds),
            ("min_bathrooms", self.min_bathrooms),
        ] {
            if let Some(value) = value {
                limits::in_range(
                    field,
                    value,
                    limits::ROOMS_FILTER_MIN,
                    limits::ROOMS_FILTER_MAX,
                )?;
            }
        }
        if self.amenities.len() > limits::AMENITY_FILTERS_MAX {
            return Err(invalid(format!(
                "amenities must contain at most {} entries, got {}",
                limits::AMENITY_FILTERS_MAX,
                self.amenities.len()
            )));
        }
        for amenity in &self.amenities {
            if amenity_filter_id(amenity).is_none() {
                return Err(invalid(format!(
                    "unsupported amenity filter {}; expected one of: {}",
                    quote_input(amenity),
                    amenity_filter_names()
                )));
            }
        }
        Ok(())
    }

    /// Airbnb ids of the amenity filters, sorted and without duplicates.
    pub fn amenity_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .amenities
            .iter()
            .filter_map(|name| amenity_filter_id(name))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The amenity ids as one comma-separated value (`4,7`), or `None`.
    pub fn amenity_id_list(&self) -> Option<String> {
        let ids = self.amenity_ids();
        (!ids.is_empty()).then(|| ids.iter().map(u32::to_string).collect::<Vec<_>>().join(","))
    }
}

#[derive(Debug, Clone, Default)]
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
    /// Panel filters: rooms, amenities, Guest Favorite, Instant Book.
    pub filters: SearchFilters,
}

impl SearchParams {
    /// Validate against today's UTC date. See [`SearchParams::validate_at`].
    pub fn validate(&self) -> Result<()> {
        self.validate_at(Utc::now().date_naive())
    }

    /// Validate the parameters against `today`.
    ///
    /// Rules:
    /// - the location passes `limits::check_location` (non-blank, at most
    ///   `limits::LOCATION_MAX_CHARS` characters, no control characters, at
    ///   least one letter or digit);
    /// - dates come as a pair, spelled exactly `YYYY-MM-DD`;
    /// - check-in is no earlier than yesterday (one day of slack for callers
    ///   whose local date is behind UTC) and no later than
    ///   `limits::MAX_BOOKING_HORIZON_DAYS` days ahead;
    /// - the stay lasts 1 to `limits::MAX_STAY_NIGHTS` nights;
    /// - there is at least 1 adult when dates are given, and guest counts are bounded;
    /// - `min_price <= max_price`, `property_type` is a supported filter, the
    ///   panel filters pass [`SearchFilters::validate`], and the cursor length
    ///   is bounded.
    pub fn validate_at(&self, today: NaiveDate) -> Result<()> {
        limits::check_location(&self.location)?;

        match (&self.checkin, &self.checkout) {
            (Some(ci), Some(co)) => {
                check_stay_dates(ci, co, today)?;
                if self.adults == Some(0) {
                    return Err(invalid(
                        "adults must be at least 1 when dates are given".into(),
                    ));
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(invalid(
                    "both checkin and checkout must be provided together".into(),
                ));
            }
            (None, None) => {}
        }

        self.validate_guests()?;

        if let Some(min) = self.min_price
            && let Some(max) = self.max_price
            && min > max
        {
            return Err(invalid("min_price cannot be greater than max_price".into()));
        }

        if let Some(ref property_type) = self.property_type
            && canonical_property_type(property_type).is_none()
        {
            return Err(AirbnbError::InvalidParams {
                reason: format!(
                    "unsupported property_type; expected one of: {}",
                    SUPPORTED_PROPERTY_TYPES.join(", ")
                ),
            });
        }

        self.filters.validate()?;

        if let Some(ref cursor) = self.cursor {
            limits::check_cursor(cursor)?;
        }

        Ok(())
    }

    fn validate_guests(&self) -> Result<()> {
        let adults = self.adults.unwrap_or(0);
        let guests = adults.saturating_add(self.children.unwrap_or(0));
        if adults > limits::MAX_ADULTS {
            return Err(invalid(format!(
                "adults must be at most {}, got {adults}",
                limits::MAX_ADULTS
            )));
        }
        if guests > limits::MAX_GUESTS {
            return Err(invalid(format!(
                "adults + children must be at most {}, got {guests}",
                limits::MAX_GUESTS
            )));
        }
        if let Some(infants) = self.infants
            && infants > limits::MAX_INFANTS
        {
            return Err(invalid(format!(
                "infants must be at most {}, got {infants}",
                limits::MAX_INFANTS
            )));
        }
        if let Some(pets) = self.pets
            && pets > limits::MAX_PETS
        {
            return Err(invalid(format!(
                "pets must be at most {}, got {pets}",
                limits::MAX_PETS
            )));
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
        // Airbnb's search page ignores `property_type=`: send the filter it reads.
        if let Some((key, value)) = self.property_type.as_deref().and_then(property_type_filter) {
            pairs.push((key.into(), value.into()));
        }
        // Panel filters, spelled as Airbnb's own search URL spells them.
        for (key, value) in [
            ("min_bedrooms", self.filters.min_bedrooms),
            ("min_beds", self.filters.min_beds),
            ("min_bathrooms", self.filters.min_bathrooms),
        ] {
            if let Some(value) = value {
                pairs.push((key.into(), value.to_string()));
            }
        }
        for id in self.filters.amenity_ids() {
            pairs.push(("amenities[]".into(), id.to_string()));
        }
        if self.filters.guest_favorite {
            pairs.push(("guest_favorite".into(), "true".into()));
        }
        if self.filters.instant_book {
            pairs.push(("ib".into(), "true".into()));
        }
        if let Some(ref cursor) = self.cursor {
            pairs.push(("cursor".into(), cursor.clone()));
        }

        pairs
    }
    /// Cache key covering every field that changes Airbnb's answer: location
    /// (trimmed, lower-cased, with `%` and the `:` separator escaped), dates,
    /// guests, prices, property type, panel filters and cursor.
    /// Shared by the GraphQL (`gql:search:` prefix) and HTML (`search:` prefix) adapters.
    pub fn cache_key(&self) -> String {
        let mut key = self
            .location
            .trim()
            .to_lowercase()
            .replace('%', "%25")
            .replace(':', "%3a");
        if let Some(ref checkin) = self.checkin {
            key.push_str(&format!(":ci={checkin}"));
        }
        if let Some(ref checkout) = self.checkout {
            key.push_str(&format!(":co={checkout}"));
        }
        if let Some(adults) = self.adults {
            key.push_str(&format!(":a={adults}"));
        }
        if let Some(children) = self.children {
            key.push_str(&format!(":ch={children}"));
        }
        if let Some(infants) = self.infants {
            key.push_str(&format!(":inf={infants}"));
        }
        if let Some(pets) = self.pets {
            key.push_str(&format!(":p={pets}"));
        }
        if let Some(min_price) = self.min_price {
            key.push_str(&format!(":min={min_price}"));
        }
        if let Some(max_price) = self.max_price {
            key.push_str(&format!(":max={max_price}"));
        }
        if let Some(ref property_type) = self.property_type {
            key.push_str(&format!(":pt={}", property_type.trim().to_lowercase()));
        }
        for (label, value) in [
            ("br", self.filters.min_bedrooms),
            ("beds", self.filters.min_beds),
            ("ba", self.filters.min_bathrooms),
        ] {
            if let Some(value) = value {
                key.push_str(&format!(":{label}={value}"));
            }
        }
        if let Some(ids) = self.filters.amenity_id_list() {
            key.push_str(&format!(":am={ids}"));
        }
        if self.filters.guest_favorite {
            key.push_str(":gf");
        }
        if self.filters.instant_book {
            key.push_str(":ib");
        }
        if let Some(ref cursor) = self.cursor {
            key.push_str(&format!(":cur={cursor}"));
        }
        key
    }
}

fn invalid(reason: String) -> AirbnbError {
    AirbnbError::InvalidParams { reason }
}

/// Check a stay against `today`: both dates spelled exactly `YYYY-MM-DD`,
/// check-in no earlier than yesterday (one day of slack for callers whose
/// local date is behind UTC) and at most `limits::MAX_BOOKING_HORIZON_DAYS`
/// days ahead, 1 to `limits::MAX_STAY_NIGHTS` nights. Returns the two dates.
pub fn check_stay_dates(ci: &str, co: &str, today: NaiveDate) -> Result<(NaiveDate, NaiveDate)> {
    let checkin = parse_canonical_date("checkin", ci)?;
    let checkout = parse_canonical_date("checkout", co)?;
    if checkout <= checkin {
        return Err(invalid("checkout date must be after checkin date".into()));
    }
    let earliest = today.checked_sub_days(Days::new(1)).unwrap_or(today);
    if checkin < earliest {
        return Err(invalid(format!(
            "checkin date {ci} is in the past (today is {today})"
        )));
    }
    if checkin.signed_duration_since(today).num_days() > limits::MAX_BOOKING_HORIZON_DAYS {
        return Err(invalid(format!(
            "checkin date {ci} is more than {} days ahead",
            limits::MAX_BOOKING_HORIZON_DAYS
        )));
    }
    let nights = checkout.signed_duration_since(checkin).num_days();
    if nights > limits::MAX_STAY_NIGHTS {
        return Err(invalid(format!(
            "stay of {nights} nights exceeds the maximum of {}",
            limits::MAX_STAY_NIGHTS
        )));
    }
    Ok((checkin, checkout))
}

/// Parse a `YYYY-MM-DD` date and require that exact spelling.
///
/// chrono's `%Y-%m-%d` also accepts `2026-1-5`, ` 2026-01-05` and
/// `+2026-01-05` (VAL-3). Only the canonical spelling may reach Airbnb and
/// the cache keys. The rejected value is echoed through `quote_input`, as
/// P3 (SEC-12) requires: quoted, escaped and cut at 32 characters.
fn parse_canonical_date(field: &str, value: &str) -> Result<NaiveDate> {
    let bad_format = || {
        invalid(format!(
            "invalid {field} date format {}, expected YYYY-MM-DD",
            quote_input(value)
        ))
    };
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| bad_format())?;
    if date.format("%Y-%m-%d").to_string() == value {
        Ok(date)
    } else {
        Err(bad_format())
    }
}

/// Query parameter of Airbnb's `/s/{location}/homes` page for a supported
/// `property_type`, as `(key, value)`.
///
/// The values are those the "Type of place" options of Airbnb's own filter
/// panel send (captured 2026-09): "Home" -> `room_types[]=Entire home/apt`,
/// "Room" -> `room_types[]=Private room`, "Hotel" -> `kg_and_tags[]=Tag:9613`.
/// `None` exactly when [`canonical_property_type`] rejects the value.
pub fn property_type_filter(value: &str) -> Option<(&'static str, &'static str)> {
    match canonical_property_type(value)? {
        "Entire home" => Some(("room_types[]", "Entire home/apt")),
        "Private room" => Some(("room_types[]", "Private room")),
        "Hotel room" => Some(("kg_and_tags[]", "Tag:9613")),
        _ => None,
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
            ..SearchParams::default()
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
    fn min_equal_to_max_is_allowed() {
        // Boundary: min == max is a valid (degenerate) range, not an error.
        // This kills the `min > max` -> `min >= max` mutant.
        let mut p = base_params();
        p.min_price = Some(150);
        p.max_price = Some(150);
        assert!(p.validate().is_ok());
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

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).expect("valid date")
    }

    fn dated(checkin: &str, checkout: &str) -> SearchParams {
        let mut p = base_params();
        p.checkin = Some(checkin.into());
        p.checkout = Some(checkout.into());
        p
    }

    #[test]
    fn valid_dates_and_price_pass() {
        let mut p = dated("2026-10-01", "2026-10-05");
        p.adults = Some(2);
        p.min_price = Some(50);
        p.max_price = Some(200);
        assert!(p.validate_at(today()).is_ok());
    }

    #[test]
    fn past_checkin_is_rejected() {
        let err = dated("2025-10-01", "2025-10-05")
            .validate_at(today())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("checkin date 2025-10-01 is in the past"),
            "{err}"
        );
    }

    #[test]
    fn validate_at_allows_yesterday_for_callers_behind_utc() {
        assert!(
            dated("2026-09-27", "2026-09-30")
                .validate_at(today())
                .is_ok()
        );
        assert!(
            dated("2026-09-26", "2026-09-30")
                .validate_at(today())
                .is_err()
        );
    }

    #[test]
    fn stays_longer_than_365_nights_are_rejected() {
        assert!(
            dated("2026-10-01", "2027-10-01")
                .validate_at(today())
                .is_ok()
        );
        let err = dated("2026-10-01", "2027-10-02")
            .validate_at(today())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("stay of 366 nights exceeds the maximum of 365"),
            "{err}"
        );
    }

    #[test]
    fn checkin_beyond_the_booking_horizon_is_rejected() {
        // 2026-09-28 + 730 days = 2028-09-27 (2028 is a leap year).
        assert!(
            dated("2028-09-27", "2028-09-30")
                .validate_at(today())
                .is_ok()
        );
        let err = dated("2028-09-28", "2028-09-30")
            .validate_at(today())
            .unwrap_err()
            .to_string();
        assert!(err.contains("more than 730 days ahead"), "{err}");
    }

    #[test]
    fn zero_adults_is_rejected_only_when_dates_are_given() {
        let mut p = dated("2026-10-01", "2026-10-05");
        p.adults = Some(0);
        let err = p.validate_at(today()).unwrap_err().to_string();
        assert!(
            err.contains("adults must be at least 1 when dates are given"),
            "{err}"
        );
        let mut undated = base_params();
        undated.adults = Some(0);
        assert!(undated.validate_at(today()).is_ok());
    }

    #[test]
    fn guest_counts_are_bounded() {
        let mut p = base_params();
        p.adults = Some(17);
        assert!(
            p.validate_at(today())
                .unwrap_err()
                .to_string()
                .contains("adults must be at most 16, got 17")
        );
        p.adults = Some(u32::MAX);
        assert!(p.validate_at(today()).is_err());
        p.adults = Some(10);
        p.children = Some(7);
        assert!(
            p.validate_at(today())
                .unwrap_err()
                .to_string()
                .contains("adults + children must be at most 16, got 17")
        );
        let mut p = base_params();
        p.infants = Some(6);
        assert!(
            p.validate_at(today())
                .unwrap_err()
                .to_string()
                .contains("infants must be at most 5, got 6")
        );
        let mut p = base_params();
        p.pets = Some(6);
        assert!(
            p.validate_at(today())
                .unwrap_err()
                .to_string()
                .contains("pets must be at most 5, got 6")
        );
    }

    #[test]
    fn non_canonical_dates_are_rejected() {
        for (checkin, checkout) in [
            ("2026-10-1", "2026-10-05"),
            (" 2026-10-01", "2026-10-05"),
            ("+2026-10-01", "2026-10-05"),
            ("2026-10-01", "2026-10-5"),
        ] {
            let err = dated(checkin, checkout)
                .validate_at(today())
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("expected YYYY-MM-DD"),
                "{checkin}/{checkout}: {err}"
            );
        }
    }

    #[test]
    fn overlong_location_and_cursor_are_rejected() {
        let mut p = base_params();
        p.location = "x".repeat(201);
        assert!(p.validate_at(today()).is_err());
        let mut p = base_params();
        p.cursor = Some("c".repeat(1025));
        assert!(p.validate_at(today()).is_err());
    }

    #[test]
    fn validate_uses_the_wall_clock() {
        let start = chrono::Utc::now().date_naive() + chrono::Days::new(30);
        let end = start + chrono::Days::new(3);
        let p = dated(
            &start.format("%Y-%m-%d").to_string(),
            &end.format("%Y-%m-%d").to_string(),
        );
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
    fn property_type_is_sent_as_the_airbnb_filter() {
        let mut p = base_params();
        p.property_type = Some("Entire home".into());
        assert!(
            p.to_query_pairs()
                .contains(&("room_types[]".to_string(), "Entire home/apt".to_string()))
        );
        p.property_type = Some("private ROOM".into());
        assert!(
            p.to_query_pairs()
                .contains(&("room_types[]".to_string(), "Private room".to_string()))
        );
        p.property_type = Some("Hotel room".into());
        assert!(
            p.to_query_pairs()
                .contains(&("kg_and_tags[]".to_string(), "Tag:9613".to_string()))
        );
        assert!(
            !p.to_query_pairs()
                .iter()
                .any(|(key, _)| key == "property_type")
        );
        assert!(p.validate().is_ok());
    }

    #[test]
    fn every_supported_property_type_has_a_url_filter() {
        for property_type in SUPPORTED_PROPERTY_TYPES {
            assert!(
                property_type_filter(property_type).is_some(),
                "{property_type}"
            );
        }
        assert_eq!(
            property_type_filter("Home"),
            Some(("room_types[]", "Entire home/apt"))
        );
        assert_eq!(
            property_type_filter("Room"),
            Some(("room_types[]", "Private room"))
        );
        assert_eq!(
            property_type_filter("hotel"),
            Some(("kg_and_tags[]", "Tag:9613"))
        );
        assert_eq!(property_type_filter("Shared room"), None);
        assert_eq!(property_type_filter("Castle"), None);
    }

    #[test]
    fn unsupported_property_type_never_reaches_the_url() {
        let mut p = base_params();
        p.property_type = Some("Shared room".into());
        let err = p.validate().unwrap_err();
        assert!(err.to_string().contains("property_type"), "{err}");
        assert!(
            !p.to_query_pairs()
                .iter()
                .any(|(key, _)| key == "property_type")
        );
    }

    fn filtered() -> SearchParams {
        SearchParams {
            filters: SearchFilters {
                min_bedrooms: Some(2),
                min_beds: Some(3),
                min_bathrooms: Some(1),
                amenities: vec![" Pool ".into(), "wifi".into(), "POOL".into()],
                guest_favorite: true,
                instant_book: true,
            },
            ..base_params()
        }
    }

    #[test]
    fn amenity_filters_are_known_by_name_whatever_the_case() {
        assert_eq!(amenity_filter_id("pool"), Some(7));
        assert_eq!(amenity_filter_id("  Air Conditioning "), Some(5));
        assert_eq!(amenity_filter_id("self check-in"), Some(51));
        assert_eq!(amenity_filter_id("sauna"), None);
        assert_eq!(amenity_filter_id(""), None);
        let names = amenity_filter_names();
        assert!(names.starts_with("wifi, air conditioning, pool"), "{names}");
        assert_eq!(names.split(", ").count(), AMENITY_FILTERS.len());
        let mut ids: Vec<u32> = AMENITY_FILTERS.iter().map(|(_, id)| *id).collect();
        ids.dedup();
        assert_eq!(
            ids.len(),
            AMENITY_FILTERS.len(),
            "ids are distinct and sorted"
        );
        assert!(ids.is_sorted());
    }

    #[test]
    fn filters_are_validated_never_adjusted() {
        type Change = fn(&mut SearchFilters);
        assert!(SearchFilters::default().is_empty());
        assert!(SearchFilters::default().validate().is_ok());
        assert!(!filtered().filters.is_empty());
        assert!(filtered().validate().is_ok());

        let cases: [(&str, Change); 6] = [
            ("min_bedrooms", |f| f.min_bedrooms = Some(0)),
            ("min_bedrooms", |f| f.min_bedrooms = Some(17)),
            ("min_beds", |f| f.min_beds = Some(0)),
            ("min_bathrooms", |f| f.min_bathrooms = Some(17)),
            ("unsupported amenity filter \"sauna\"", |f| {
                f.amenities = vec!["sauna".into()];
            }),
            ("at most 10 entries, got 11", |f| {
                f.amenities = vec!["wifi".into(); 11];
            }),
        ];
        for (expected, change) in cases {
            let mut params = base_params();
            change(&mut params.filters);
            let err = params.validate().unwrap_err().to_string();
            assert!(err.contains(expected), "{expected}: {err}");
        }
        let unknown = SearchFilters {
            amenities: vec!["sauna".into()],
            ..SearchFilters::default()
        };
        let err = unknown.validate().unwrap_err().to_string();
        assert!(
            err.contains("expected one of: wifi, air conditioning"),
            "{err}"
        );
    }

    #[test]
    fn amenity_ids_are_sorted_and_deduplicated() {
        let filters = filtered().filters;
        assert_eq!(filters.amenity_ids(), [4, 7]);
        assert_eq!(filters.amenity_id_list().as_deref(), Some("4,7"));
        assert_eq!(SearchFilters::default().amenity_id_list(), None);
    }

    #[test]
    fn filters_enter_the_query_pairs_with_airbnb_names() {
        let pairs = filtered().to_query_pairs();
        let get = |name: &str| -> Vec<&str> {
            pairs
                .iter()
                .filter(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
                .collect()
        };
        assert_eq!(get("min_bedrooms"), ["2"]);
        assert_eq!(get("min_beds"), ["3"]);
        assert_eq!(get("min_bathrooms"), ["1"]);
        assert_eq!(get("amenities[]"), ["4", "7"]);
        assert_eq!(get("guest_favorite"), ["true"]);
        assert_eq!(get("ib"), ["true"]);

        let plain = base_params().to_query_pairs();
        for name in [
            "min_bedrooms",
            "min_beds",
            "min_bathrooms",
            "amenities[]",
            "guest_favorite",
            "ib",
        ] {
            assert!(!plain.iter().any(|(key, _)| key == name), "{name}");
        }
    }

    #[test]
    fn filters_enter_the_cache_key() {
        assert_eq!(
            filtered().cache_key(),
            "paris:br=2:beds=3:ba=1:am=4,7:gf:ib"
        );
        let mut one = base_params();
        one.filters.guest_favorite = true;
        assert_eq!(one.cache_key(), "paris:gf");
        assert_ne!(one.cache_key(), base_params().cache_key());
        // The same amenities in another order and case are the same search.
        let mut a = base_params();
        a.filters.amenities = vec!["wifi".into(), "pool".into()];
        let mut b = base_params();
        b.filters.amenities = vec!["Pool".into(), "WIFI".into()];
        assert_eq!(a.cache_key(), b.cache_key());
    }

    #[test]
    fn stay_dates_are_checked_once_for_searches_and_quotes() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let (checkin, checkout) = check_stay_dates("2026-12-01", "2026-12-08", today).unwrap();
        assert_eq!(checkout.signed_duration_since(checkin).num_days(), 7);
        for (ci, co) in [
            ("2026-12-08", "2026-12-01"),
            ("2026-12-01", "2026-12-01"),
            ("2026-11-01", "2026-11-20"),
            ("01/12/2026", "2026-12-08"),
            ("2029-12-01", "2029-12-08"),
        ] {
            assert!(check_stay_dates(ci, co, today).is_err(), "{ci}..{co}");
        }
    }

    #[test]
    fn cache_key_location_only() {
        assert_eq!(base_params().cache_key(), "paris");
    }

    #[test]
    fn cache_key_normalises_location_case_and_whitespace() {
        let mut p = base_params();
        p.location = "  PARIS ".into();
        assert_eq!(p.cache_key(), "paris");
    }

    #[test]
    fn cache_key_escapes_the_separator_in_the_location() {
        let mut typed = base_params();
        typed.location = "Paris:a=2".into();
        let mut filtered = base_params();
        filtered.adults = Some(2);
        assert_ne!(typed.cache_key(), filtered.cache_key());
        assert_eq!(typed.cache_key(), "paris%3aa=2");
    }

    #[test]
    fn cache_key_includes_every_field() {
        let mut p = base_params();
        p.checkin = Some("2030-06-01".into());
        p.checkout = Some("2030-06-05".into());
        p.adults = Some(2);
        p.children = Some(1);
        p.infants = Some(1);
        p.pets = Some(1);
        p.min_price = Some(50);
        p.max_price = Some(200);
        p.property_type = Some("Entire home".into());
        p.cursor = Some("page2".into());
        assert_eq!(
            p.cache_key(),
            "paris:ci=2030-06-01:co=2030-06-05:a=2:ch=1:inf=1:p=1:min=50:max=200:pt=entire home:cur=page2"
        );
    }

    #[test]
    fn cache_key_differs_for_each_single_field() {
        let variants = [
            SearchParams {
                checkin: Some("2030-06-01".into()),
                checkout: Some("2030-06-05".into()),
                ..base_params()
            },
            SearchParams {
                adults: Some(2),
                ..base_params()
            },
            SearchParams {
                children: Some(1),
                ..base_params()
            },
            SearchParams {
                infants: Some(1),
                ..base_params()
            },
            SearchParams {
                pets: Some(1),
                ..base_params()
            },
            SearchParams {
                min_price: Some(50),
                ..base_params()
            },
            SearchParams {
                max_price: Some(200),
                ..base_params()
            },
            SearchParams {
                property_type: Some("Private room".into()),
                ..base_params()
            },
            SearchParams {
                cursor: Some("page2".into()),
                ..base_params()
            },
        ];
        let mut keys = std::collections::HashSet::new();
        assert!(keys.insert(base_params().cache_key()));
        for variant in &variants {
            assert!(
                keys.insert(variant.cache_key()),
                "cache key collision for {variant:?}"
            );
        }
    }

    #[test]
    fn canonical_property_type_accepts_airbnb_labels() {
        assert_eq!(canonical_property_type("Entire home"), Some("Entire home"));
        assert_eq!(
            canonical_property_type("entire home/apt"),
            Some("Entire home")
        );
        assert_eq!(canonical_property_type("Home"), Some("Entire home"));
        assert_eq!(
            canonical_property_type(" Private ROOM "),
            Some("Private room")
        );
        assert_eq!(canonical_property_type("Room"), Some("Private room"));
        assert_eq!(canonical_property_type("Hotel"), Some("Hotel room"));
        assert_eq!(canonical_property_type("Shared room"), None);
        assert_eq!(canonical_property_type("Castle"), None);
    }

    #[test]
    fn unsupported_property_type_is_rejected() {
        for rejected in ["Castle", "Shared room"] {
            let mut p = base_params();
            p.property_type = Some(rejected.into());
            let err = p.validate().unwrap_err();
            assert!(
                err.to_string()
                    .contains("Entire home, Private room, Hotel room"),
                "{rejected}: {err}"
            );
        }
    }

    #[test]
    fn every_documented_property_type_is_accepted() {
        for property_type in SUPPORTED_PROPERTY_TYPES {
            let mut p = base_params();
            p.property_type = Some(property_type.to_string());
            assert!(p.validate().is_ok(), "{property_type}");
        }
    }

    #[test]
    fn invalid_date_error_is_bounded_and_escaped() {
        let mut p = base_params();
        p.checkin = Some(format!("x\n{}", "y".repeat(5_000)));
        p.checkout = Some("2099-06-05".into());
        let msg = p.validate().unwrap_err().to_string();
        assert!(!msg.contains('\n'), "{msg}");
        assert!(msg.len() < 200, "{} bytes", msg.len());
    }

    #[test]
    fn overlong_location_is_rejected_without_echoing_it() {
        let mut p = base_params();
        p.location = "a".repeat(MAX_LOCATION_CHARS + 1);
        let msg = p.validate().unwrap_err().to_string();
        assert!(msg.contains("at most 200 characters"), "{msg}");
        assert!(!msg.contains(&p.location), "location echoed back: {msg}");

        p.location = "a".repeat(MAX_LOCATION_CHARS);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn location_with_control_characters_is_rejected() {
        for location in ["Paris\nINFO forged log line", "Paris\tFrance", "Paris\u{0}"] {
            let mut p = base_params();
            p.location = location.into();
            assert!(p.validate().is_err(), "{location:?} must be rejected");
        }
    }

    #[test]
    fn location_without_letters_or_digits_is_rejected() {
        for location in ["..", "../..", "?#", "%%"] {
            let mut p = base_params();
            p.location = location.into();
            assert!(p.validate().is_err(), "{location:?} must be rejected");
        }
    }

    #[test]
    fn real_world_locations_are_accepted() {
        for location in [
            "Paris, France",
            "São Paulo",
            "Saint-Martin/Sint Maarten",
            "L'Île-Rousse",
            "東京",
        ] {
            let mut p = base_params();
            p.location = location.into();
            assert!(p.validate().is_ok(), "{location:?} must be accepted");
        }
    }
}
