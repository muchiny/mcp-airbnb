# P1b fixture excerpts (captured 2026-09-28)

Excerpts of live Airbnb responses used by the parser-accuracy phase (P1b).
Section and field shapes are verbatim; only personal or identifying values
were replaced. The files are ASCII (non-ASCII characters are `\uXXXX` escapes).

| File | Source capture | What it keeps |
|---|---|---|
| `pdp_hotel.json` | `StaysPdpSections`, hotel listing 1257736932578886647 (`gql_02`) | `pdpType: HOTEL`, no `MEET_YOUR_HOST`, `LOCATION_PDP` with `subtitle: null` and heading title, SBUI overview "Hotel in ...", booking sidebar without price |
| `pdp_apartment.json` | `StaysPdpSections`, listing 38817969 (`gql_03`) | `MEET_YOUR_HOST` with base64 `userId`, `hostDetails` labels, SBUI overview counts and host id, preview vs see-all amenities with `available: false` items |
| `stays_search.json` | web `StaysSearch` response, items 0, 1 and 11 | `primaryLine` with `qualifier: "total"`, the discounted `orderedComponents` variant, "N nights x $P" breakdowns, `HOSTINFO` "Business host"/"Individual host", hotel title/subtitle swap |
| `calendar.json` | `PdpAvailabilityCalendar` (`gql_04`), 3 days of Sept and 3 of Oct 2026 | `calendarDate`, explicit `available`, `price.localPriceFormatted: null` |

## Anonymization

- Host name → `Host A`; co-host → `Cohost B`; host bio → `Host bio redacted.`; occupation highlight → `My work: Example`.
- Host ids → base64 `DemandUser:1000001` and `User:1000002`; the SBUI numeric host id → `1000001`.
- Listing and hotel names, descriptions, the hotel address and business details → generic placeholders.
- Photo URLs → `https://example.com/fixture-image/anon/<name>.jpeg` (no `a0.muscache.com` URL is committed, as in P1a's `graphql/` fixtures).
- Dropped: reviewer data, share ids, logging correlation ids, badge icon URLs.

Kept as captured: listing ids, coordinates, amenity labels, ratings, review counts, prices and dates. These are public listing data, and the parsers are tested on them.
