#!/usr/bin/env python3
"""Build a committed test fixture from a raw 2026-11 `StaysPdpSections` capture.

Standard library only, deterministic. Unlike `anonymize_fixtures.py`, which
redacts a full capture, this script works by allow-list: the output is built
from scratch with the fields the parsers read (`data.node.pdpPresentation`
and a few metadata fields). Everything written by or about a person (listing
title, description, host name, bio, host highlights, free-text rules, photo
URLs, Airbnb-generated highlights, which may quote host names) is replaced by
a placeholder. Public listing data is kept as captured: ids, coordinates,
place names, amenity labels, ratings, counts, review topic labels.

Usage (the raw capture is read from standard input and stays outside the
repository):
    python3 scripts/trim_pdp_entity.py --kind villa --month 2026-11 \
        < /var/tmp/mcp-airbnb-2026-11/pdp_dated_45490342_2027-06-12.json

It writes `tests/fixtures/airbnb/<month>/StaysPdpSections.<kind>.response.json`
and nothing else. No file path is taken from the command line: the shell opens
the capture, and the output path is built from the month and the kind, then
checked to lie inside the fixtures directory.
"""

import argparse
import base64
import json
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.realpath(__file__)))
# The only directory this script writes to.
FIXTURES = os.path.join(REPO, "tests", "fixtures", "airbnb")
MONTH = re.compile(r"[0-9]{4}-(0[1-9]|1[0-2])")

# kind -> (listing name, host name, numeric host id, synthetic host highlight)
KINDS = {
    "villa": ("Sample villa - 3 bedrooms", "Host A", "1000001", "Speaks English and French"),
    "apartment": ("Sample apartment - city center", "Host C", "1000003", "My work: Example"),
    "hotel": ("Sample hotel, classic room", "Business Host B", "1000002", None),
}
IMAGE = "https://example.com/fixture-image/{}.jpg"


def fixture_path(month, kind):
    """Path of the `kind` fixture of `month`, refused unless it lies inside
    `tests/fixtures/airbnb/`.

    `month` is the only free text that enters the path. It must be a YYYY-MM
    month, and the canonical path (symlinks resolved) must stay under the
    fixtures directory, prefix followed by a separator so that a sibling such
    as `airbnb-evil/` does not pass.
    """
    if not isinstance(month, str) or not MONTH.fullmatch(month):
        raise ValueError(f"--month {month!r} is not a YYYY-MM month")
    if kind not in KINDS:
        raise ValueError(f"--kind {kind!r} is not one of {', '.join(sorted(KINDS))}")
    base_dir = os.path.realpath(FIXTURES)
    resolved = os.path.realpath(os.path.join(base_dir, month, f"StaysPdpSections.{kind}.response.json"))
    prefix = base_dir if base_dir.endswith(os.sep) else base_dir + os.sep
    if not resolved.startswith(prefix):
        raise ValueError(f"the fixture path {resolved} is outside {base_dir}")
    return resolved


def get(node, *path):
    for key in path:
        if not isinstance(node, dict):
            return None
        node = node.get(key)
    return node


def pick(node, keys):
    return {key: node.get(key) for key in keys} if isinstance(node, dict) else None


def ugc(text):
    return {"__typename": "UGCText", "localizedString": text}


def amenity_groups(groups):
    return [
        {
            "title": group.get("title"),
            "amenities": [pick(item, ("available", "title")) for item in group.get("amenities") or []],
        }
        for group in groups or []
    ]


def rule_item(item):
    description = item.get("description")
    if isinstance(description, dict) and "text" in description:
        kept = {"__typename": "LocalizedText", "text": description["text"]}
    elif description is None:
        kept = None
    else:
        kept = {"__typename": "PdpUGCText", "content": ugc("Host rule text redacted.")}
    return {"type": item.get("type"), "title": item.get("title"), "description": kept}


def build(raw, kind):
    listing_name, host_name, host_id, host_highlight = KINDS[kind]
    node = get(raw, "data", "node")
    pres = get(node, "pdpPresentation")
    if not isinstance(pres, dict):
        sys.exit("the capture has no data.node.pdpPresentation")
    info = pres.get("hostInfo") or {}
    passport = info.get("passportData") or {}
    quality = pres.get("quality") or {}
    descriptions = pres.get("descriptions") or {}
    place = pres.get("location") or {}
    counter = iter(range(1, 1000))

    def image():
        return {"image": {"uri": IMAGE.format(next(counter))}}

    def description(key):
        if descriptions.get(key) is None:
            return None
        return ugc("Sample description, first line.<br />Second line with <b>markup</b>.")

    highlights = [{"text": {"content": ugc(host_highlight)}}] if host_highlight else []
    highlights.append({"text": {"content": ugc("")}})
    presentation = {
        "__typename": pres.get("__typename"),
        "pdpType": pres.get("pdpType"),
        "title": {"content": ugc(listing_name)} if pres.get("title") else None,
        "descriptions": {
            "longDescriptionHtml": description("longDescriptionHtml"),
            "shortDescriptionHtml": (
                {"content": description("shortDescriptionHtml")}
                if descriptions.get("shortDescriptionHtml")
                else None
            ),
        },
        "amenities": {
            "previewAmenitiesGroups": amenity_groups(get(pres, "amenities", "previewAmenitiesGroups")),
            "seeAllAmenitiesGroups": amenity_groups(get(pres, "amenities", "seeAllAmenitiesGroups")),
        },
        "location": pick(place, ("subtitle", "locationName", "latitude", "longitude")),
        "localizedLocation": pres.get("localizedLocation"),
        "hostInfo": {
            "passportData": {
                "name": host_name,
                "userId": base64.b64encode(f"DemandUser:{host_id}".encode()).decode(),
                "isSuperhost": passport.get("isSuperhost"),
                "isVerified": passport.get("isVerified"),
                "profilePictureUrl": IMAGE.format("host"),
                "ratingCount": passport.get("ratingCount"),
                "ratingAverage": passport.get("ratingAverage"),
                "timeAsHost": pick(passport.get("timeAsHost"), ("years", "months")),
            },
            "highlights": highlights,
            "responseRateText": info.get("responseRateText"),
            "responseTimeText": info.get("responseTimeText"),
            "about": ugc("Host bio redacted.") if info.get("about") else None,
            "overview": {"title": {"text": f"Hosted by {host_name}"}},
        },
        "quality": {
            "listingRatingStats": {
                "overallRatingStats": pick(
                    get(quality, "listingRatingStats", "overallRatingStats"),
                    ("ratingAverage", "ratingCount"),
                )
            },
            "isGuestFavorite": quality.get("isGuestFavorite"),
            "categoryRatings": [
                pick(entry, ("categoryType", "localizedRating", "label"))
                for entry in quality.get("categoryRatings") or []
            ],
            "reviewTags": [
                pick(tag, ("name", "count", "localizedName")) for tag in quality.get("reviewTags") or []
            ],
        },
        "overview": pick(pres.get("overview"), ("title", "items")),
        "rules": {
            "groupItems": [
                {
                    "type": group.get("type"),
                    "title": group.get("title"),
                    "items": [rule_item(item) for item in group.get("items") or []],
                }
                for group in get(pres, "rules", "groupItems") or []
            ]
        },
        "heroMedia": {"edges": [{"node": image()} for _ in (get(pres, "heroMedia", "edges") or [])[:3]]},
        "mediaTour": {
            "stops": [
                {"name": stop.get("name"), "items": [image() for _ in (stop.get("items") or [])[:2]]}
                for stop in (get(pres, "mediaTour", "stops") or [])[:2]
            ]
        },
        "highlights": [
            {
                "headline": {"localizedContent": f"Sample highlight {index}"},
                "body": {"localizedContent": f"Sample highlight {index} body"},
            }
            for index, _ in enumerate(pres.get("highlights") or [], start=1)
        ],
        "sharingConfig": {"propertyType": get(pres, "sharingConfig", "propertyType")},
        "personCapacity": pres.get("personCapacity"),
    }

    old_root = get(raw, "data", "presentation", "stayProductDetailPage", "sections") or {}
    metadata = old_root.get("metadata") or {}
    sharing = dict(pick(metadata.get("sharingConfig"), ("title", "propertyType", "personCapacity", "reviewCount", "starRating")) or {})
    if kind == "hotel" and isinstance(sharing.get("title"), str):
        # "<hotel name> · ★4.44 · Hotel in Lyon": the first part is the business name.
        sharing["title"] = " · ".join(["Sample hotel"] + sharing["title"].split(" · ")[1:])
    sections = []
    for section in old_root.get("sections") or []:
        kind_name = section.get("sectionComponentType")
        body = section.get("section")
        if kind_name == "SBUI_SENTINEL":
            continue
        if kind_name == "BOOK_IT_SIDEBAR" and isinstance(body, dict):
            body = pick(body, ("__typename", "maxGuestCapacity", "structuredDisplayPrice"))
        elif kind_name == "PHOTO_TOUR_SCROLLABLE" and isinstance(body, dict):
            body = {
                "__typename": body.get("__typename"),
                "mediaItems": [{"baseUrl": IMAGE.format(next(counter))} for _ in (body.get("mediaItems") or [])[:2]],
            }
        elif isinstance(body, dict):
            # Since 2026-11 the other sections are empty placeholders.
            body = {"__typename": body.get("__typename")}
        sections.append({"sectionComponentType": kind_name, "section": body})

    return {
        "data": {
            "node": {
                "__typename": node.get("__typename"),
                "propertyType": node.get("propertyType"),
                "spaceType": node.get("spaceType"),
                "personCapacity": node.get("personCapacity"),
                "description": {"name": ugc(listing_name)},
                "listingRatingStats": {
                    "overallRatingStats": pick(
                        get(node, "listingRatingStats", "overallRatingStats"), ("ratingAverage", "ratingCount")
                    )
                },
                "location": {"coordinate": pick(get(node, "location", "coordinate"), ("latitude", "longitude"))},
                "pdpPresentation": presentation,
            },
            "presentation": {
                "stayProductDetailPage": {
                    "sections": {
                        "metadata": {
                            "pdpType": metadata.get("pdpType"),
                            "sharingConfig": sharing,
                            "loggingContext": {
                                "eventDataLogging": pick(
                                    get(metadata, "loggingContext", "eventDataLogging"),
                                    ("listingId", "listingLat", "listingLng", "roomType", "personCapacity"),
                                )
                            },
                        },
                        "sections": sections,
                    }
                }
            },
        }
    }


def main(argv=None, stdin=None):
    stdin = sys.stdin.buffer if stdin is None else stdin
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--kind", required=True, choices=sorted(KINDS))
    parser.add_argument(
        "--month", required=True, help="fixture directory under tests/fixtures/airbnb/, as YYYY-MM (2026-11)"
    )
    args = parser.parse_args(argv)
    try:
        target = fixture_path(args.month, args.kind)
    except ValueError as error:
        parser.error(str(error))
    try:
        raw = json.loads(stdin.read())
    except ValueError as error:
        sys.exit(f"standard input is not a JSON capture: {error}")
    fixture = build(raw, args.kind)
    text = json.dumps(fixture, indent=1, ensure_ascii=True) + "\n"
    if "muscache" in text:
        sys.exit("an Airbnb image URL survived")
    os.makedirs(os.path.dirname(target), exist_ok=True)
    with open(target, "w", encoding="ascii", newline="\n") as out:
        out.write(text)
    print(f"{os.path.relpath(target, REPO)}: {len(text)} bytes")


if __name__ == "__main__":
    main()
