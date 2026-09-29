#!/usr/bin/env python3
"""Anonymize and trim raw Airbnb captures into committed test fixtures.

Standard library only. Deterministic: the same input always produces the same
output, so re-running the script yields an empty git diff.

Usage:
    python3 scripts/anonymize_fixtures.py \
        --src /var/tmp/mcp-airbnb-audit/fixtures \
        --dst tests/fixtures/airbnb/2026-09

What it does, per file listed in FILES:
  * collects the names of real people (reviewers, hosts, co-hosts) and replaces
    every word-boundary occurrence of them, in every string, with "PersonN";
  * replaces free text written about or by people (review comments, host "about"
    and responses, reviewer locations, host highlights, `caption` and `Html`
    fields) with placeholder text, keeping HTML tags so the structure stays
    realistic; host-authored public listing text (titles, SEO description,
    house rules, `localizedCaption`) is kept on purpose;
  * replaces every a0.muscache.com image URL with https://example.com/fixture-image/N.jpg;
  * replaces user ids (plain, relay-encoded or numeric hostId), review ids,
    profile paths, session, share and trace ids;
  * trims the StaysSearch response to the parts the parsers read;
  * fails (exit 1) if any collected name or muscache URL survives.
"""

import argparse
import base64
import json
import re
import sys
from pathlib import Path

# source file name -> destination path relative to --dst
FILES = {
    "web_StaysSearch_request.json": "graphql/StaysSearch.request.json",
    "web_StaysSearch_response.json": "graphql/StaysSearch.response.json",
    "gql_01_StaysSearch.json": "graphql/StaysSearch.validation_error.json",
    "web_StaysPdpReviewsQuery_response.json": "graphql/StaysPdpReviewsQuery.response.json",
    "web_PdpAvailabilityCalendar_response.json": "graphql/PdpAvailabilityCalendar.response.json",
    "web_StaysPdpSections_request.json": "graphql/StaysPdpSections.request.json",
    "gql_03_StaysPdpSections.json": "graphql/StaysPdpSections.apartment.response.json",
    "gql_02_StaysPdpSections.json": "graphql/StaysPdpSections.hotel.response.json",
}

# (__typename, key) pairs whose string value is a real person's name
NAME_FIELDS = {
    ("ReviewUser", "firstName"),
    ("ReviewUser", "hostName"),
    ("PassportCardData", "name"),
    ("UserData", "name"),
}

# keys whose string value is free text written by a person
FREE_TEXT_KEYS = {"comments", "commentV2", "about", "response", "caption", "localizedReviewerLocation"}
# (__typename, key) pairs holding free text written by the host
FREE_TEXT_TYPED = {("ReadMoreHtml", "htmlText"), ("Html", "htmlText"), ("ImageMetadata", "caption")}

ID_KEYS_BY_TYPE = {
    ("ReviewUser", "id"),
    ("ReviewUser", "contextualUserId"),
    ("PdpReviewForP3", "id"),
}
# keys whose plain numeric string value is a real user id (e.g. pdpContext.hostId)
NUMERIC_USER_ID_KEYS = {"hostId"}
UUID_KEYS = {"federatedSearchSessionId", "federatedSearchId", "loggingCorrelationId", "legacyLoggingSectionId"}
ZERO_UUID = "00000000-0000-0000-0000-000000000000"
MUSCACHE = re.compile(r"https://a0\.muscache\.com/[^\s\"'<>]*")
PROFILE_PATH = re.compile(r"^/users/(profile|show)/\d+$")
SHARE_ID = re.compile(r"unique_share_id=[0-9a-fA-F-]+")
HTML_TAG = re.compile(r"(<[^>]*>)")


class Anonymizer:
    def __init__(self):
        self.names = {}  # real name -> placeholder
        self.images = {}  # real url -> placeholder
        self.ids = {}  # real id -> placeholder
        self.text_counter = 0

    # ---- pass 1: collect names -------------------------------------------------
    def collect(self, value, typename=None):
        if isinstance(value, dict):
            t = value.get("__typename", typename)
            for k, v in value.items():
                if isinstance(v, str) and (t, k) in NAME_FIELDS and len(v.strip()) >= 2:
                    self.names.setdefault(v.strip(), f"Person{len(self.names) + 1}")
                else:
                    self.collect(v, t)
        elif isinstance(value, list):
            for item in value:
                self.collect(item, typename)

    # ---- pass 2: transform -----------------------------------------------------
    def placeholder_text(self, original):
        self.text_counter += 1
        n = self.text_counter
        parts = HTML_TAG.split(original)
        kept = [p if HTML_TAG.fullmatch(p) or not p.strip() else f"Placeholder text {n}." for p in parts]
        return "".join(kept)

    def image(self, url):
        return self.images.setdefault(url, f"https://example.com/fixture-image/{len(self.images) + 1}.jpg")

    def fake_id(self, real):
        return self.ids.setdefault(real, str(1_000_000 + len(self.ids) + 1))

    def replace_names(self, s):
        for real, fake in self.names.items():
            s = re.sub(rf"\b{re.escape(real)}\b", fake, s)
        return s

    def relay_user_id(self, s):
        try:
            decoded = base64.b64decode(s, validate=True).decode("utf-8")
        except Exception:  # noqa: BLE001 - any decoding failure means "not a relay id"
            return None
        m = re.fullmatch(r"(DemandUser|User):(\d+)", decoded)
        if not m:
            return None
        return base64.b64encode(f"{m.group(1)}:{self.fake_id(m.group(2))}".encode()).decode()

    def string(self, typename, key, s):
        if (typename, key) in NAME_FIELDS:
            return self.names.get(s.strip(), s)
        if key in FREE_TEXT_KEYS or (typename, key) in FREE_TEXT_TYPED:
            return self.placeholder_text(s)
        if typename == "HostHighlight" and key == "title" and not s.startswith("Speaks "):
            return "Placeholder host highlight."
        if (typename, key) in ID_KEYS_BY_TYPE:
            return self.fake_id(s)
        if key in NUMERIC_USER_ID_KEYS and s.isdigit():
            return self.fake_id(s)
        if s.isdigit() and s in self.ids:
            # the same real id stored under another key (e.g. logging payloads)
            return self.ids[s]
        if key in UUID_KEYS:
            return ZERO_UUID
        if key == "traceId":
            return "fixture-trace-id"
        if key == "p3ImpressionId":
            return "p3_0_fixture"
        if PROFILE_PATH.match(s):
            return "/users/profile/0"
        relay = self.relay_user_id(s) if key in ("userId", "id") else None
        if relay is not None:
            return relay
        s = SHARE_ID.sub(f"unique_share_id={ZERO_UUID}", s)
        s = MUSCACHE.sub(lambda m: self.image(m.group(0)), s)
        return self.replace_names(s)

    def transform(self, value, typename=None, key=None):
        if isinstance(value, dict):
            t = value.get("__typename", typename)
            out = {k: self.transform(v, t, k) for k, v in value.items()}
            if value.get("filterName") in UUID_KEYS:
                out["filterValues"] = [ZERO_UUID]
            return out
        if isinstance(value, list):
            return [self.transform(item, typename, key) for item in value]
        if isinstance(value, str):
            return self.string(typename, key, value)
        return value

    def leaks(self, text):
        found = [real for real in self.names if re.search(rf"\b{re.escape(real)}\b", text)]
        if "a0.muscache.com" in text:
            found.append("a0.muscache.com")
        return found


def trim(dst_rel, data):
    """Keep only what the parsers read, to keep fixtures small and reviewable."""
    if dst_rel == "graphql/StaysSearch.response.json":
        results = data["data"]["presentation"]["staysSearch"]["results"]
        for item in results["searchResults"]:
            if isinstance(item.get("contextualPictures"), list):
                item["contextualPictures"] = item["contextualPictures"][:2]
        logging = results.get("loggingMetadata") or {}
        return {
            "data": {
                "presentation": {
                    "__typename": data["data"]["presentation"].get("__typename"),
                    "staysSearch": {
                        "__typename": data["data"]["presentation"]["staysSearch"].get("__typename"),
                        "results": {
                            "__typename": results.get("__typename"),
                            "searchResults": results["searchResults"],
                            "paginationInfo": results["paginationInfo"],
                            "loggingMetadata": {
                                "__typename": logging.get("__typename"),
                                "remarketingLoggingData": logging.get("remarketingLoggingData"),
                            },
                        },
                    },
                }
            },
            "extensions": data.get("extensions", {}),
        }
    if dst_rel == "graphql/PdpAvailabilityCalendar.response.json":
        for month in data["data"]["merlin"]["pdpAvailabilityCalendar"]["calendarMonths"]:
            month.pop("conditionRanges", None)
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--src", required=True, type=Path)
    parser.add_argument("--dst", required=True, type=Path)
    args = parser.parse_args()

    loaded = {}
    anonymizer = Anonymizer()
    for src_name in FILES:
        path = args.src / src_name
        if not path.is_file():
            print(f"missing raw capture: {path}", file=sys.stderr)
            return 1
        loaded[src_name] = json.loads(path.read_text(encoding="utf-8"))
        anonymizer.collect(loaded[src_name])

    status = 0
    for src_name, dst_rel in FILES.items():
        try:
            trimmed = trim(dst_rel, loaded[src_name])
        except (KeyError, TypeError) as e:
            print(f"unexpected capture shape in {src_name}: missing or invalid key {e}", file=sys.stderr)
            return 1
        out = anonymizer.transform(trimmed)
        text = json.dumps(out, indent=1, ensure_ascii=False) + "\n"
        leaked = anonymizer.leaks(text)
        if leaked:
            print(f"LEAK in {dst_rel}: {len(leaked)} personal value(s) survived", file=sys.stderr)
            status = 1
            continue
        target = args.dst / dst_rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        print(f"wrote {target} ({len(text)} bytes)")
    print(f"replaced {len(anonymizer.names)} names, {len(anonymizer.images)} image URLs, {len(anonymizer.ids)} ids")
    return status


if __name__ == "__main__":
    sys.exit(main())
