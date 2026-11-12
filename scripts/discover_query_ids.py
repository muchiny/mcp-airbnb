#!/usr/bin/env python3
"""Read Airbnb's current persisted-query ids from its public web bundles.

Airbnb's GraphQL API is called by persisted-query id (`/api/v3/<Operation>/<id>`).
The ids rotate. When one does, the server answers "Airbnb API changed for
<operation>" and the fix is a new id under `scraper.graphql_hashes` in
config.yaml. This script finds the ids the airbnb.com web client ships today:

    python3 scripts/discover_query_ids.py --listing-id 12345678
    python3 scripts/discover_query_ids.py --listing-id 12345678 --all
    python3 -m unittest discover -s scripts -p 'test_discover_query_ids.py'   # offline tests

It downloads three public pages (home, one search, one listing) and the
JavaScript bundles they reference, and reads every
`name:'<Operation>',type:'query',operationId:'<64 hex>'` they contain. It then
compares the five operations the server uses with config.yaml and prints a
`graphql_hashes:` block to paste.

Limits, stated plainly:
- Only bundles referenced by the three pages are read. Airbnb loads some
  operations from chunks fetched later (reviews, calendar): those are reported
  as NOT FOUND, which means "not in these bundles", not "removed". Read them
  from the browser's network tab instead (the id is the last path segment of
  the `/api/v3/<Operation>/<id>` request).
- A new id often comes with new variables or a new response shape. The id
  alone is then not enough: run `airbnb status` after the change.

Polite by construction: pages are fetched one at a time, 2.5 s apart; bundles
come from the static CDN (`a0.muscache.com`) only, capped in number and size.
A redirect is followed only to the same https host. Nothing is written to
disk and nothing fetched is executed.

Exit status: 0 = every id found matches config.yaml, 1 = at least one id
differs, 2 = a page could not be fetched or no bundle was found.
"""

from __future__ import annotations

import argparse
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Only this origin and path prefix is ever downloaded as a bundle.
BUNDLE_PREFIX = "https://a0.muscache.com/airbnb/static/"
BUNDLE_URL = re.compile(r"https://a0\.muscache\.com/airbnb/static/[A-Za-z0-9_./-]+\.js")
OPERATION = re.compile(
    r"name:'([A-Za-z0-9_]{1,80})',type:'(?:query|mutation)',operationId:'([0-9a-f]{64})'"
)
CONFIG_ID = re.compile(r"^\s+([a-z_]+):\s*\"([0-9a-f]{64})\"\s*$")

# Operation name -> key under `scraper.graphql_hashes`.
WANTED = {
    "StaysSearch": "stays_search",
    "StaysPdpSections": "stays_pdp_sections",
    "StaysPdpReviewsQuery": "stays_pdp_reviews",
    "PdpAvailabilityCalendar": "pdp_availability_calendar",
    "StaysPdpBookItQuery": "stays_pdp_book_it",
}

USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
)
PAGE_PAUSE_SECS = 2.5
BUNDLE_PAUSE_SECS = 0.1
MAX_BUNDLES = 200
MAX_PAGE_BYTES = 8 * 1024 * 1024
MAX_BUNDLE_BYTES = 8 * 1024 * 1024
TIMEOUT_SECS = 30


def bundle_urls(html: str) -> list[str]:
    """Bundle URLs referenced by a page, in order, without duplicates."""
    seen: dict[str, None] = {}
    for url in BUNDLE_URL.findall(html):
        if url.startswith(BUNDLE_PREFIX) and ".." not in url:
            seen.setdefault(url, None)
    return list(seen)


def operations(javascript: str) -> dict[str, str]:
    """Operation name -> persisted-query id, for every operation in a bundle."""
    return dict(OPERATION.findall(javascript))


def configured_ids(config_text: str) -> dict[str, str]:
    """Ids under `graphql_hashes:` in a config.yaml, by key."""
    ids: dict[str, str] = {}
    inside = False
    for line in config_text.splitlines():
        if line.strip() == "graphql_hashes:":
            inside = True
            continue
        if inside:
            match = CONFIG_ID.match(line)
            if match:
                ids[match.group(1)] = match.group(2)
            elif line.strip() and not line.strip().startswith("#"):
                break
    return ids


def compare(found: dict[str, str], configured: dict[str, str]) -> list[tuple[str, str, str, str]]:
    """One `(operation, config key, status, id)` row per wanted operation."""
    rows = []
    for operation, key in WANTED.items():
        current = found.get(operation)
        if current is None:
            rows.append((operation, key, "NOT FOUND", configured.get(key, "")))
        elif configured.get(key) == current:
            rows.append((operation, key, "SAME", current))
        else:
            rows.append((operation, key, "CHANGED", current))
    return rows


def yaml_block(rows: list[tuple[str, str, str, str]]) -> str:
    """The `graphql_hashes:` block for the ids that were found."""
    lines = ["scraper:", "  graphql_hashes:"]
    lines += [f'    {key}: "{value}"' for _, key, status, value in rows if status != "NOT FOUND"]
    return "\n".join(lines)


def page_urls(base_url: str, location: str, listing_id: str | None) -> list[str]:
    """The public pages whose bundles are read."""
    slug = re.sub(r"[^A-Za-z0-9]+", "-", location).strip("-")
    urls = [f"{base_url}/", f"{base_url}/s/{slug}/homes"]
    if listing_id:
        urls.append(f"{base_url}/rooms/{listing_id}")
    return urls


def same_origin(url: str, other: str) -> bool:
    """Whether two URLs are both https and on the same host and port."""
    a, b = urllib.parse.urlsplit(url), urllib.parse.urlsplit(other)
    return a.scheme == b.scheme == "https" and a.netloc.lower() == b.netloc.lower() != ""


class SameOriginRedirects(urllib.request.HTTPRedirectHandler):
    """Follow a redirect only to the same https host: a page or a bundle is
    never read from another host than the one that was asked."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not same_origin(req.full_url, newurl):
            raise urllib.error.URLError("redirect to another host or scheme refused")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


OPENER = urllib.request.build_opener(SameOriginRedirects)


def printable(text: object, limit: int = 300) -> str:
    """`text` without control characters, cut at `limit`: server-supplied
    text (an error reason, a redirect target) must not reach the terminal raw."""
    cleaned = "".join(c if c.isprintable() else "?" for c in str(text))
    return cleaned[:limit]


def fetch(url: str, limit: int) -> str:
    """GET `url` as text, reading at most `limit` bytes."""
    request = urllib.request.Request(  # noqa: S310 - https URLs built from constants
        url, headers={"User-Agent": USER_AGENT, "Accept-Language": "en"}
    )
    with OPENER.open(request, timeout=TIMEOUT_SECS) as response:  # noqa: S310
        return response.read(limit).decode("utf-8", errors="replace")


def discover(pages: list[str], log) -> dict[str, str]:
    """Every operation id in the bundles the pages reference."""
    bundles: dict[str, None] = {}
    for index, page in enumerate(pages):
        if index:
            time.sleep(PAGE_PAUSE_SECS)
        html = fetch(page, MAX_PAGE_BYTES)
        found = bundle_urls(html)
        log(f"{page}: {len(found)} bundles")
        for url in found:
            bundles.setdefault(url, None)
    if not bundles:
        raise RuntimeError("the pages reference no bundle (bot challenge, or the page layout changed)")
    found_ids: dict[str, str] = {}
    for url in list(bundles)[:MAX_BUNDLES]:
        try:
            found_ids.update(operations(fetch(url, MAX_BUNDLE_BYTES)))
        except (urllib.error.URLError, TimeoutError) as error:
            log(f"skipped {url}: {printable(error)}")
        time.sleep(BUNDLE_PAUSE_SECS)
    log(f"{min(len(bundles), MAX_BUNDLES)} bundles read, {len(found_ids)} operations found")
    return found_ids


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--listing-id", help="a public listing id; needed for the listing operations")
    parser.add_argument("--location", default="Paris, France", help="location of the search page")
    parser.add_argument("--base-url", default="https://www.airbnb.com", help="Airbnb origin (https)")
    parser.add_argument("--config", type=Path, default=REPO / "config.yaml", help="config.yaml to compare with")
    parser.add_argument("--all", action="store_true", help="also list every other operation found")
    args = parser.parse_args(argv)

    if not args.base_url.startswith("https://") or args.base_url.count("/") != 2:
        print("--base-url must be an https origin, such as https://www.airbnb.com", file=sys.stderr)
        return 2
    if args.listing_id and not re.fullmatch(r"[1-9][0-9]{0,24}", args.listing_id):
        print("--listing-id must be digits only, without a leading zero", file=sys.stderr)
        return 2
    try:
        configured = configured_ids(args.config.read_text(encoding="utf-8"))
    except OSError as error:
        print(f"cannot read {args.config}: {printable(error)}", file=sys.stderr)
        return 2

    def log(message: str) -> None:
        print(message, file=sys.stderr)

    try:
        found = discover(page_urls(args.base_url, args.location, args.listing_id), log)
    except (urllib.error.URLError, TimeoutError, RuntimeError) as error:
        print(f"discovery failed: {printable(error)}", file=sys.stderr)
        return 2

    rows = compare(found, configured)
    for operation, key, status, value in rows:
        print(f"{status:<9} {operation:<24} {key:<26} {value}")
    if any(status == "NOT FOUND" for _, _, status, _ in rows):
        print(
            "\nNOT FOUND = not in the bundles of these pages (Airbnb loads some operations later)."
            "\nRead those ids from the browser's network tab: /api/v3/<Operation>/<id>."
            + ("" if args.listing_id else "\nPass --listing-id to read the listing page too.")
        )
    print("\n" + yaml_block(rows))
    if args.all:
        print("\nOther operations found:")
        for operation in sorted(set(found) - set(WANTED)):
            print(f"  {operation}: {found[operation]}")
    return 1 if any(status == "CHANGED" for _, _, status, _ in rows) else 0


if __name__ == "__main__":
    sys.exit(main())
