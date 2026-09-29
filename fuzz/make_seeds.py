#!/usr/bin/env python3
"""Regenerate the committed fuzz seeds in fuzz/seeds/<target>/.

Sources, and nothing else:
  * tests/fixtures/airbnb/2026-09/**/*.json (anonymized in P1a, safe to commit)
  * the synthetic inputs defined in this file

Each fixture is classified by its JSON shape (not by its file name), shrunk
so libFuzzer can mutate it quickly (lists cut to a few items, long strings
cut, UI-only keys dropped) and, for the HTML parsers, wrapped in the
<script data-deferred-state> page shape Airbnb serves. The output is
deterministic, so --check can tell whether fuzz/seeds/ is stale.

    python3 fuzz/make_seeds.py          # rewrite fuzz/seeds/
    python3 fuzz/make_seeds.py --check  # exit 1 if fuzz/seeds/ is stale
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "tests" / "fixtures" / "airbnb" / "2026-09"
SEEDS = REPO / "fuzz" / "seeds"

TARGETS = (
    "fuzz_search_parser",
    "fuzz_detail_parser",
    "fuzz_calendar_parser",
    "fuzz_review_parser",
    "fuzz_graphql_search",
    "fuzz_graphql_detail",
    "fuzz_graphql_review",
    "fuzz_graphql_host",
    "fuzz_host_profile_html",
    "fuzz_api_key",
    "fuzz_calendar_analytics",
    "fuzz_calendar_model",
    "fuzz_input_validation",
)
GRAPHQL_JSON_TARGETS = (
    "fuzz_graphql_search",
    "fuzz_graphql_detail",
    "fuzz_graphql_review",
    "fuzz_graphql_host",
)

# Large keys that no parser reads (UI screens, filter panels, SEO blocks).
# Keep `sbuiData.sectionConfiguration`: P1b's PDP helpers read room counts
# and the "Type in Place" location from `sectionConfiguration.root.sections`.
DROP_KEYS = frozenset(
    {
        "screens",
        "screensV2",
        "flows",
        "filters",
        "searchInput",
        "loggingMetadata",
        "seo",
    }
)
MAX_STR = 200


def dig(value, *path):
    for key in path:
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    return value


def classify(doc):
    """Return the payload kind of an Airbnb GraphQL response, or None."""
    if isinstance(doc, dict) and isinstance(doc.get("errors"), list) and doc["errors"]:
        return "error"
    if dig(doc, "data", "presentation", "staysSearch") is not None:
        return "search"
    if dig(doc, "data", "presentation", "stayProductDetailPage", "sections") is not None:
        return "pdp"
    if dig(doc, "data", "presentation", "stayProductDetailPage", "reviews") is not None:
        return "reviews"
    if dig(doc, "data", "merlin", "pdpAvailabilityCalendar") is not None:
        return "calendar"
    return None


def shrink(value, max_list, keep_full=frozenset()):
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            if key in DROP_KEYS:
                continue
            if key in keep_full and isinstance(item, list):
                out[key] = [shrink(x, max_list, keep_full) for x in item]
            else:
                out[key] = shrink(item, max_list, keep_full)
        return out
    if isinstance(value, list):
        return [shrink(x, max_list, keep_full) for x in value[:max_list]]
    if isinstance(value, str) and len(value) > MAX_STR:
        return value[:MAX_STR]
    return value


def dump(doc) -> bytes:
    return json.dumps(doc, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def deferred_state_page(operation, doc, wrapper) -> bytes:
    payload = json.dumps(
        {wrapper: [[f"{operation}:{{}}", doc]]},
        ensure_ascii=False,
        separators=(",", ":"),
    ).replace("</", "<\\/")
    return (
        '<!doctype html><html><head><meta charset="utf-8"></head><body>'
        '<script id="data-deferred-state-0" data-deferred-state="true" '
        f'type="application/json">{payload}</script></body></html>'
    ).encode("utf-8")


def fixture_seeds():
    """Yield (target, file name, bytes) for every recognised fixture."""
    for path in sorted(FIXTURES.rglob("*.json")):
        try:
            doc = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise SystemExit(f"error: {path} is not valid JSON: {exc}") from exc
        kind = classify(doc)
        # graphql/X.json and p1b/X.json must not overwrite each other's seeds.
        stem = f"{path.parent.name}.{path.stem}"
        if kind == "search":
            small = shrink(doc, 3)
            yield "fuzz_graphql_search", f"{stem}.json", dump(small)
            yield "fuzz_search_parser", f"{stem}.niobe.html", deferred_state_page(
                "StaysSearch", small, "niobeClientData"
            )
            yield "fuzz_search_parser", f"{stem}.minimal.html", deferred_state_page(
                "StaysSearch", small, "niobeMinimalClientData"
            )
        elif kind == "pdp":
            small = shrink(doc, 2, keep_full=frozenset({"sections"}))
            page = deferred_state_page("StaysPdpSections", small, "niobeClientData")
            yield "fuzz_graphql_detail", f"{stem}.json", dump(small)
            yield "fuzz_graphql_host", f"{stem}.json", dump(small)
            for target in (
                "fuzz_detail_parser",
                "fuzz_host_profile_html",
                "fuzz_review_parser",
                "fuzz_calendar_parser",
            ):
                yield target, f"{stem}.html", page
        elif kind == "reviews":
            small = shrink(doc, 3)
            yield "fuzz_graphql_review", f"{stem}.json", dump(small)
            yield "fuzz_review_parser", f"{stem}.html", deferred_state_page(
                "StaysPdpReviewsQuery", small, "niobeClientData"
            )
        elif kind == "calendar":
            small = shrink(doc, 7)
            yield "fuzz_calendar_parser", f"{stem}.json", dump(small)
            yield "fuzz_calendar_analytics", f"{stem}.json", dump(small)
        elif kind == "error":
            for target in (*GRAPHQL_JSON_TARGETS, "fuzz_calendar_parser"):
                yield target, f"{stem}.json", dump(doc)


def calendar_day(date, price, available, reason=None):
    return {
        "date": date,
        "price": price,
        "available": available,
        "min_nights": 1,
        "max_nights": 365,
        "closed_to_arrival": None,
        "closed_to_departure": None,
        "unavailability_reason": reason,
    }


def price_calendar(days):
    return {
        "listing_id": "12345",
        "currency": "USD",
        "days": days,
        "average_price": None,
        "occupancy_rate": None,
        "min_price": None,
        "max_price": None,
    }


SYNTHETIC_CALENDAR_RESPONSE = {
    "data": {
        "merlin": {
            "pdpAvailabilityCalendar": {
                "calendarMonths": [
                    {
                        "month": 10,
                        "year": 2027,
                        "days": [
                            {
                                "calendarDate": "2027-10-01",
                                "available": True,
                                "minNights": 2,
                                "maxNights": 365,
                                "availableForCheckin": True,
                                "availableForCheckout": True,
                                "bookable": True,
                                "price": {"localPriceFormatted": "$120"},
                            },
                            {
                                "calendarDate": "2027-10-02",
                                "available": False,
                                "minNights": 2,
                                "maxNights": 365,
                                "availableForCheckin": False,
                                "availableForCheckout": True,
                                "bookable": False,
                                "price": {"localPriceFormatted": None},
                            },
                            {
                                "calendarDate": "2027-10-03",
                                "available": True,
                                "minNights": 1,
                                "maxNights": 365,
                                "availableForCheckin": True,
                                "availableForCheckout": True,
                                "bookable": True,
                                "price": {"localPriceFormatted": "€95"},
                            },
                        ],
                    }
                ]
            }
        }
    }
}

SYNTHETIC = {
    "fuzz_api_key": {
        "homepage.html": (
            b"<!doctype html><html><head><script>window.__config = "
            b'{"api_config":{"key":"0123456789abcdefghijklmnopqrstuv"}}'
            b"</script></head><body></body></html>"
        ),
        "truncated.txt": b'{"api_config":{"key":"',
        "empty-key.txt": b'{"api_config":{"key":""}}',
        "multibyte.txt": '{"api_config":{"key":"clé-ü"}}'.encode("utf-8"),
    },
    "fuzz_calendar_parser": {"synthetic-priced.json": dump(SYNTHETIC_CALENDAR_RESPONSE)},
    "fuzz_calendar_analytics": {"synthetic-priced.json": dump(SYNTHETIC_CALENDAR_RESPONSE)},
    "fuzz_calendar_model": {
        "mixed.json": dump(
            price_calendar(
                [
                    calendar_day("2026-09-01", None, False, "PastDate"),
                    calendar_day("2027-10-01", 120.0, True),
                    calendar_day("2027-10-02", None, False, "Booked"),
                    calendar_day("2027-10-03", None, False, "BlockedByHost"),
                    calendar_day("2027-10-04", 95.5, True),
                ]
            )
        ),
        "non-ascii-dates.json": dump(
            price_calendar(
                [
                    calendar_day("２０２７-10-01", 120.0, False),
                    calendar_day("2027", None, True),
                    calendar_day("é", 80.0, True),
                    calendar_day("", None, False),
                ]
            )
        ),
        "empty.json": dump(price_calendar([])),
    },
    "fuzz_input_validation": {
        "listing-id.txt": b"12345",
        "search.txt": b"Lyon, France\n2027-03-01\n2027-03-04\n2",
        "inverted-dates.txt": b"Lyon\n2027-03-04\n2027-03-01\n0",
        "unicode.txt": "Zürich\n２０２７-03-01\n2027-03-04\n1".encode("utf-8"),
    },
}


def all_seeds():
    seeds = {}
    for target, name, data in fixture_seeds():
        seeds[(target, name)] = data
    for target, files in SYNTHETIC.items():
        for name, data in files.items():
            seeds[(target, name)] = data
    return seeds


def write(root, seeds):
    for target in TARGETS:
        directory = root / target
        if directory.exists():
            shutil.rmtree(directory)
        directory.mkdir(parents=True)
    for (target, name), data in sorted(seeds.items()):
        (root / target / name).write_bytes(data)


def snapshot(root):
    if not root.is_dir():
        return {}
    return {
        p.relative_to(root).as_posix(): p.read_bytes()
        for p in sorted(root.rglob("*"))
        if p.is_file() and p.relative_to(root).parts[0] in TARGETS
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description="Regenerate fuzz/seeds/ from the anonymized fixtures.")
    parser.add_argument("--check", action="store_true", help="exit 1 if fuzz/seeds/ is stale")
    args = parser.parse_args(argv)
    if not FIXTURES.is_dir():
        print(
            f"error: {FIXTURES.relative_to(REPO)} is missing; P1a commits the anonymized fixtures there",
            file=sys.stderr,
        )
        return 2
    seeds = all_seeds()
    missing = [t for t in TARGETS if not any(key[0] == t for key in seeds)]
    if missing:
        found = ", ".join(sorted(p.name for p in FIXTURES.rglob("*.json")))
        print(f"error: no seed for {missing}; fixtures found: {found}", file=sys.stderr)
        return 2
    if args.check:
        with tempfile.TemporaryDirectory() as tmp:
            write(Path(tmp), seeds)
            fresh = snapshot(Path(tmp))
        committed = snapshot(SEEDS)
        changed = (set(fresh) ^ set(committed)) | {
            k for k in set(fresh) & set(committed) if fresh[k] != committed[k]
        }
        if changed:
            print(
                "fuzz/seeds/ is stale; run python3 fuzz/make_seeds.py. Differences:\n  "
                + "\n  ".join(sorted(changed)),
                file=sys.stderr,
            )
            return 1
        print(f"fuzz/seeds/ is up to date ({len(fresh)} files)")
        return 0
    write(SEEDS, seeds)
    total = sum(len(v) for v in seeds.values())
    print(f"wrote {len(seeds)} seeds ({total // 1024} KiB) to {SEEDS.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
