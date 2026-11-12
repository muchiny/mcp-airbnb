//! Small conversion helpers between CLI arg structs and domain params.
//!
//! Kept intentionally thin — the actual orchestration lives in
//! `crate::application::analytical_handlers` or inline in `cli::dispatch`.
//! These helpers only exist to keep the dispatcher readable.

use crate::application::analytical_handlers::StayFilter;
use crate::cli::args::{SearchArgs, StayArgs};
use crate::domain::search_params::{SearchFilters, SearchParams};

/// Convert `SearchArgs` from clap into the domain's `SearchParams`.
pub fn search_args_to_params(args: SearchArgs) -> SearchParams {
    SearchParams {
        location: args.location,
        checkin: args.checkin,
        checkout: args.checkout,
        adults: args.adults,
        children: args.children,
        infants: args.infants,
        pets: args.pets,
        min_price: args.min_price,
        max_price: args.max_price,
        property_type: args.property_type,
        cursor: args.cursor,
        filters: SearchFilters {
            min_bedrooms: args.min_bedrooms,
            min_beds: args.min_beds,
            min_bathrooms: args.min_bathrooms,
            amenities: args.amenities,
            guest_favorite: args.guest_favorite,
            instant_book: args.instant_book,
        },
    }
}

/// Convert the shared stay arguments into the application's `StayFilter`.
pub fn stay_args_to_filter(args: StayArgs) -> StayFilter {
    StayFilter {
        checkin: args.checkin,
        checkout: args.checkout,
        adults: args.adults,
        children: None,
        property_type: args.property_type,
        filters: SearchFilters {
            min_bedrooms: args.min_bedrooms,
            ..SearchFilters::default()
        },
    }
}
