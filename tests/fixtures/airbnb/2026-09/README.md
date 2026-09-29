# Airbnb captures — 2026-09

Anonymized airbnb.com responses captured on 2026-09-28 (Lyon, France). They pin the
request/response shapes the GraphQL adapter was re-synchronized against, and let the
test suite detect drift the next time Airbnb changes its internal API.

| File | Operation | Source | Notes |
|---|---|---|---|
| `graphql/StaysSearch.request.json` | `StaysSearch` request body | airbnb.com web client | hash `0afc7d44…`; flexible-date search, page 2 (`cursor` = items_offset 18); sends both `placeId` and `query` |
| `graphql/StaysSearch.response.json` | `StaysSearch` response | airbnb.com web client | trimmed to `searchResults` (18 `StaySearchResult`, 2 pictures each), `paginationInfo.pageCursors` and `loggingMetadata.remarketingLoggingData`; no `nextPageCursor`, no `totalCount` |
| `graphql/StaysSearch.validation_error.json` | `StaysSearch` response | mcp-airbnb 0.2.0 with the stale hash `d4d95036…` | HTTP 200, `data: null`, `errors[0].extensions.classification = ValidationError` |
| `graphql/StaysPdpReviewsQuery.response.json` | `StaysPdpReviewsQuery` response | airbnb.com web client | hash `cfdc3ffb…`, listing 38817969, offset 0, 24 reviews, `metadata.reviewsCount = 490`, offset not echoed |
| `graphql/PdpAvailabilityCalendar.response.json` | `PdpAvailabilityCalendar` response | airbnb.com web client | hash `be60714e…`, 12 months from 2026-09 (365 days), every `price.localPriceFormatted` is `null`; `conditionRanges` removed |
| `graphql/StaysPdpSections.request.json` | `StaysPdpSections` request body (POST) | airbnb.com web client | hash `c47b106f…`; recorded only, mcp-airbnb still uses GET `80c7889b…` |
| `graphql/StaysPdpSections.apartment.response.json` | `StaysPdpSections` response | mcp-airbnb 0.2.0, hash `80c7889b…` | listing 38817969 (entire rental unit) |
| `graphql/StaysPdpSections.hotel.response.json` | `StaysPdpSections` response | mcp-airbnb 0.2.0, hash `80c7889b…` | listing 1257736932578886647 (hotel room) |

## Privacy

The raw captures contain third-party personal data (names, reviews, bios, photos) and
are **never committed**. `scripts/anonymize_fixtures.py` (standard library only,
deterministic) turns them into these files:

- people's names become `PersonN`;
- free text written about or by people (review comments, host "about" and responses,
  reviewer locations, host highlights) becomes `Placeholder text N.` (HTML tags are kept);
- image URLs become `https://example.com/fixture-image/N.jpg`;
- user and review ids become `100000N`, whether plain, relay-encoded (`DemandUser:N`,
  `User:N`) or the numeric `pdpContext.hostId`;
- session, share (`unique_share_id`) and trace ids become zeros
  (`00000000-0000-0000-0000-000000000000`, `fixture-trace-id`).

Host-authored public listing text is kept on purpose, because the parsers are tested
on it and it is already public on airbnb.com: listing and search titles, the SEO
description, house rules, most photo captions (`localizedCaption`), amenity labels and
other listing content. Listing ids, coordinates, ratings, prices and dates are kept as
captured.

`fixtures_are_anonymized` (`src/test_helpers.rs`) fails on any `a0.muscache.com` URL,
`/users/show/` link, non-zero UUID or `hostId` outside the anonymized range.

The script refuses to write a file in which a collected name or an `a0.muscache.com`
URL survives.

## Regenerating

```bash
python3 scripts/anonymize_fixtures.py --src <dir with raw captures> --dst tests/fixtures/airbnb/2026-09
```

## Using them

- Unit tests: `crate::test_helpers::fixture_json("graphql/<file>.json")`.
- Integration tests: `include_str!("fixtures/airbnb/2026-09/graphql/<file>.json")`.

## Known live issues (2026-09-29 smoke check)

No log line was emitted for any of these (default log level, no `Airbnb API changed for` line and no `falling back to HTML scraper` line in any of the smoke logs). They are observations on the returned results, and the calls keep working.

- StaysSearch page 2 (cursor from the first `search "Lyon, France"`) returned 18 listings, 5 of which were already on page 1 (expected 0). Re-running page 1 alone returned 17 of the same 18 ids in a different order, so upstream ranking is not stable between calls, and page 2 still shares 5 to 6 ids with either copy of page 1. The cursor is not ignored (13 ids are new). Deduplication in `collect_search_pages` absorbs the overlap.
- StaysSearch with `--property-type "Hotel room"` (`kgAndTags=Tag:9613`) returned 18 listings, 1 of which was on the unfiltered page 1. All 18 results are titled `Hotel in …`, so the filter works upstream. `property_type` was `None` for 16 of them because hotel cards carry the name in `title` and the `Hotel in <place>` line in `subtitle`; the shared `StaySearchResult` parser (P1b) handles that swap (`hotel_items_swap_title_and_subtitle`).
