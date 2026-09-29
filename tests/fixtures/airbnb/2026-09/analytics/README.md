# P2 analytics fixture (captured 2026-09-28)

A live Airbnb response used by the analytics-correctness phase (P2).
`tests/analytics_fixture_test.rs` parses it and pins the reference date to
2026-09-28, so its expectations never drift with the clock.

| File | Source capture | What it keeps |
|---|---|---|
| `pdp_availability_calendar_2026-09-28.json` | `PdpAvailabilityCalendar` (`gql_04`), 3 months (September to November 2026), 91 days | Every day verbatim: `calendarDate`, `available`, `availableForCheckin`/`availableForCheckout`, `bookable`, `minNights`/`maxNights`, `price.localPriceFormatted: null` (no nightly price is published) |

## Anonymization

- Listing id → `12345678` (the `listingId` of each month).
- Dropped: the top-level `extensions` object (request trace id) and each month's `conditionRanges` (not used by analytics).

The payload holds no names, URLs, host ids or other personal data. Dates,
availability flags and stay limits are kept as captured.
