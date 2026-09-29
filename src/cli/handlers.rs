//! Small conversion helpers between CLI arg structs and domain params.
//!
//! Kept intentionally thin — the actual orchestration lives in
//! `crate::application::analytical_handlers` or inline in `cli::dispatch`.
//! These helpers only exist to keep the dispatcher readable.

use crate::cli::args::SearchArgs;
use crate::domain::search_params::SearchParams;

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
    }
}
