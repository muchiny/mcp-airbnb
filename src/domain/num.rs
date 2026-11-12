//! Checked conversions of collection sizes.
//!
//! The analytics report counts as `u32` and divide by them as `f64`. An `as`
//! cast would truncate or round without a trace, so these helpers saturate
//! instead. The bound is `u32::MAX`: the listings, reviews and calendar days
//! of one tool call number a few thousand at most, so it is never reached.

/// `count` as a `u32`, saturating at `u32::MAX`.
pub(crate) fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// `count` as an `f64`: exact up to `u32::MAX`, saturating beyond.
pub(crate) fn count_f64(count: usize) -> f64 {
    f64::from(count_u32(count))
}

/// An amount as text: no decimals for a whole amount, two otherwise
/// (`1345`, `621.30`), so a price never prints as `621.3`.
pub(crate) fn amount_text(amount: f64) -> String {
    if amount.fract() == 0.0 {
        format!("{amount:.0}")
    } else {
        format!("{amount:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_print_whole_or_with_two_decimals() {
        assert_eq!(amount_text(1345.0), "1345");
        assert_eq!(amount_text(621.3), "621.30");
        assert_eq!(amount_text(188.61), "188.61");
        assert_eq!(amount_text(0.5), "0.50");
    }

    #[test]
    fn counts_convert_exactly_within_u32() {
        assert_eq!(count_u32(0), 0);
        assert_eq!(count_u32(366), 366);
        assert_eq!(count_f64(0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(count_f64(366).to_bits(), 366.0_f64.to_bits());
    }

    #[test]
    fn counts_saturate_instead_of_wrapping() {
        assert_eq!(count_u32(usize::MAX), u32::MAX);
        assert_eq!(
            count_f64(usize::MAX).to_bits(),
            f64::from(u32::MAX).to_bits()
        );
    }
}
