# Fixtures captured 2026-11-12

In November 2026 Airbnb moved the listing content of `StaysPdpSections` from
the `sections` array to a listing entity, `data.node.pdpPresentation` (query
id `1e38cb0e…`). The `sections` array is still present; most sections are
empty placeholders. These files pin that shape for
`src/adapters/graphql/parsers/pdp_node.rs`.

| File | Listing | What it keeps |
|---|---|---|
| `StaysPdpSections.villa.response.json` | 45490342 (entire villa, Superhost, guest favorite) | full entity: title, description, 14 amenity groups with a "Not included" group, rules with structured details, category ratings, review topics, host passport data |
| `StaysPdpSections.apartment.response.json` | 38817969 (entire rental unit) | `location.locationName` (a neighbourhood), host `about`, free-text host rules |
| `StaysPdpBookItQuery.response.json` | none (synthetic) | the booking panel of a bookable 7-night stay, for `src/adapters/graphql/parsers/quote.rs`: `structuredDisplayPrice` (total as primary line), both `priceDetails` breakdowns, `availability`, payment tip and announcement. Shape as captured for query id `b4e68e23…`; every amount, date and message is made up ($1,400 = 7 nights x $200.00) |
| `StaysPdpSections.hotel.response.json` | 1257736932578886647 (hotel room) | `pdpType: HOTEL`, `title: null`, `location.subtitle: null`, empty overview items, a business host without response rate |

## How they were made

`scripts/trim_pdp_entity.py` builds each file from a raw capture kept outside
the repository. It works by allow-list: only the fields the parsers read are
copied.

```bash
python3 scripts/trim_pdp_entity.py --kind villa --month 2026-11 < /var/tmp/<raw capture>.json
```

The capture is piped in and the script takes no file path: it writes
`tests/fixtures/airbnb/<month>/StaysPdpSections.<kind>.response.json` and
nothing else.

- Listing titles, descriptions, host names, host bios, host highlights,
  free-text rules, Airbnb-generated highlights and every photo URL are
  placeholders. The villa's host highlight `Speaks English and French` is
  synthetic (it exercises the language parser).
- Host ids are base64 `DemandUser:100000N`.
- Kept as captured: listing ids, coordinates, place names, amenity labels,
  ratings, counts, category ratings and review topic labels. These are public
  listing data, and the parsers are tested on them.
