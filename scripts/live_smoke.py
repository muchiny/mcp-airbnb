#!/usr/bin/env python3
"""Opt-in live smoke test for mcp-airbnb. NOT run in CI.

Starts the `mcp-airbnb` binary, speaks MCP JSON-RPC over stdio, calls each
of the 18 tools once against the REAL Airbnb site and flags output that the
2026-09 audit found to be wrong: $0 prices, empty reviews for a listing that
has reviews, "Where you'll be" used as a location, occupancy that counts
past dates, the hard-coded 65% occupancy, duplicated labels, leaked base64
relay ids, bad ordinals, mixed currencies and unencoded resource URIs.

Polite by construction: one call at a time, --pause seconds between calls,
on top of the server's own rate limiter. Printed excerpts are capped at
120 characters and full tool output is never written to disk.

    cargo build --release --bin mcp-airbnb
    python3 scripts/live_smoke.py --live --location "Lyon, France"
    python3 -m unittest discover -s scripts -p 'test_live_smoke.py'   # offline tests

Exit status: 0 = no flags, 1 = flags raised, 2 = harness error or --live missing.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PROTOCOL_VERSION = "2025-06-18"
EXCERPT = 120

EXPECTED_TOOLS = (
    "airbnb_search",
    "airbnb_listing_details",
    "airbnb_reviews",
    "airbnb_price_calendar",
    "airbnb_host_profile",
    "airbnb_neighborhood_stats",
    "airbnb_occupancy_estimate",
    "airbnb_compare_listings",
    "airbnb_price_trends",
    "airbnb_gap_finder",
    "airbnb_revenue_estimate",
    "airbnb_listing_score",
    "airbnb_amenity_analysis",
    "airbnb_market_comparison",
    "airbnb_host_portfolio",
    "airbnb_review_sentiment",
    "airbnb_competitive_positioning",
    "airbnb_optimal_pricing",
)


@dataclass(frozen=True)
class Flag:
    code: str
    detail: str


@dataclass
class ToolRun:
    name: str
    is_error: bool
    text: str
    seconds: float
    flags: list[Flag] = field(default_factory=list)


class HarnessError(RuntimeError):
    """The server could not be driven: spawn failure, timeout, dead pipe, bad JSON."""


def excerpt(line: str) -> str:
    line = line.strip()
    return line if len(line) <= EXCERPT else line[: EXCERPT - 1] + "…"


# ---------------------------------------------------------------- detectors

_ZERO_PRICE = (
    re.compile(r"[$€£¥]\s?0(?:[.,]0+)?(?![\d.,])"),
    re.compile(r"\b[A-Z]{3}\s?0(?:[.,]0+)?(?![\d.,])"),
    re.compile(r"(?<![\d.,])0(?:[.,]0+)?\s?[$€£¥]"),
)
_PLACEHOLDER_LOCATION = re.compile(r"Where you[’']ll be")
_REVIEW_COUNT = re.compile(r"^Rating: [\d.]+ \((\d[\d,]*) reviews?\)", re.M)
_CALENDAR_ROW = re.compile(r"^\d{4}-\d{2}-\d{2}\s", re.M)
# "Days: N total" (audit) or "Nights considered: N (…)" (after P2, I7).
_OCCUPANCY_DAYS = re.compile(r"Days:\s*(\d+)\s+total|Nights considered:\s*(\d+)")
_DUPLICATED_LABEL = re.compile(r"\b([A-Z][a-z]+(?: [a-z]+)*): \1:")
_ORDINAL = re.compile(r"\b(\d+)th\b")
# base64 of "DemandUser:" and "StayListing:" (Airbnb relay ids)
_RELAY_ID = re.compile(r"\b(?:RGVtYW5kVXNlcj|U3RheUxpc3Rpbmc6)[A-Za-z0-9+/=]*")
_NIGHT_PRICE = re.compile(r"([$€£¥]|\b[A-Z]{3})\s?\d[\d.,]*/night")
_LISTING_ID = re.compile(r"\(ID: (\d+)\)")


def _per_line(text: str, code: str, predicate) -> list[Flag]:
    return [Flag(code, excerpt(line)) for line in text.splitlines() if predicate(line)]


def flag_zero_prices(text: str) -> list[Flag]:
    return _per_line(text, "zero-price", lambda line: any(p.search(line) for p in _ZERO_PRICE))


def flag_placeholder_location(text: str) -> list[Flag]:
    return _per_line(text, "placeholder-location", lambda line: bool(_PLACEHOLDER_LOCATION.search(line)))


def flag_default_occupancy(text: str) -> list[Flag]:
    return _per_line(
        text,
        "hard-coded-occupancy",
        lambda line: ("65.0%" in line and "occupancy" in line.lower()) or "avg: 65.0" in line,
    )


def flag_duplicated_labels(text: str) -> list[Flag]:
    return _per_line(text, "duplicated-label", lambda line: bool(_DUPLICATED_LABEL.search(line)))


def flag_bad_ordinals(text: str) -> list[Flag]:
    def wrong(line: str) -> bool:
        for match in _ORDINAL.finditer(line):
            n = int(match.group(1))
            if n % 10 in (1, 2, 3) and n % 100 not in (11, 12, 13):
                return True
        return False

    return _per_line(text, "bad-ordinal", wrong)


def flag_relay_ids(text: str) -> list[Flag]:
    return _per_line(text, "relay-id", lambda line: bool(_RELAY_ID.search(line)))


def flag_unknown_host(text: str) -> list[Flag]:
    return _per_line(text, "unknown-host", lambda line: "Unknown Host" in line)


def flag_no_reviews_analyzed(text: str) -> list[Flag]:
    return _per_line(text, "no-reviews-analyzed", lambda line: line.strip() == "Reviews Analyzed: 0")


def generic_flags(text: str) -> list[Flag]:
    flags: list[Flag] = []
    for detector in (
        flag_zero_prices,
        flag_placeholder_location,
        flag_default_occupancy,
        flag_duplicated_labels,
        flag_bad_ordinals,
        flag_relay_ids,
        flag_unknown_host,
        flag_no_reviews_analyzed,
    ):
        flags.extend(detector(text))
    return flags


def review_count(details_text: str) -> int | None:
    match = _REVIEW_COUNT.search(details_text)
    return int(match.group(1).replace(",", "")) if match else None


def review_entries(reviews_text: str) -> int:
    return sum(1 for line in reviews_text.splitlines() if line.startswith("**"))


def flag_empty_reviews(details_text: str, reviews_text: str) -> list[Flag]:
    count = review_count(details_text)
    if count and review_entries(reviews_text) == 0:
        return [Flag("empty-reviews", f"listing shows {count} reviews but airbnb_reviews returned none")]
    return []


def flag_past_dates_in_occupancy(calendar_text: str, occupancy_text: str) -> list[Flag]:
    rows = len(_CALENDAR_ROW.findall(calendar_text))
    past = sum(1 for line in calendar_text.splitlines() if "Past date" in line)
    match = _OCCUPANCY_DAYS.search(occupancy_text)
    considered = int(match.group(1) or match.group(2)) if match else None
    if past and considered is not None and considered >= rows:
        return [
            Flag(
                "occupancy-counts-past-dates",
                f"{past} of {rows} calendar days are past dates but occupancy uses {considered} days",
            )
        ]
    return []


def flag_mixed_currencies(search_text: str) -> list[Flag]:
    currencies = sorted({m.group(1) for m in _NIGHT_PRICE.finditer(search_text)})
    if len(currencies) > 1:
        return [Flag("mixed-currencies", "nightly prices in " + ", ".join(currencies))]
    return []


def flag_unencoded_uris(uris: list[str]) -> list[Flag]:
    return [Flag("unencoded-resource-uri", excerpt(uri)) for uri in uris if re.search(r"\s", uri)]


def listing_ids(search_text: str) -> list[str]:
    return _LISTING_ID.findall(search_text)


# ---------------------------------------------------------------- stdio client


class McpStdio:
    """Minimal newline-delimited JSON-RPC client for an MCP stdio server."""

    def __init__(self, argv, *, cwd=None, env=None, stderr_path=None):
        stderr = open(stderr_path, "wb") if stderr_path else subprocess.DEVNULL
        try:
            self._proc = subprocess.Popen(
                argv,
                cwd=cwd,
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=stderr,
            )
        except OSError as exc:
            raise HarnessError(f"cannot start {argv[0]}: {exc}") from exc
        finally:
            if stderr_path:
                stderr.close()
        self._next_id = 0
        self._lines: queue.Queue = queue.Queue()
        self._pump_thread = threading.Thread(target=self._pump, daemon=True)
        self._pump_thread.start()

    def _pump(self):
        for raw in self._proc.stdout:
            self._lines.put(raw)
        self._lines.put(None)

    def _send(self, message):
        try:
            self._proc.stdin.write((json.dumps(message) + "\n").encode("utf-8"))
            self._proc.stdin.flush()
        except OSError as exc:
            raise HarnessError(f"cannot write to the server: {exc}") from exc

    def notify(self, method, params=None):
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    def request(self, method, params=None, timeout=60.0):
        self._next_id += 1
        request_id = self._next_id
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}})
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise HarnessError(f"{method}: no response within {timeout:.0f} s")
            try:
                raw = self._lines.get(timeout=remaining)
            except queue.Empty:
                raise HarnessError(f"{method}: no response within {timeout:.0f} s") from None
            if raw is None:
                raise HarnessError(f"{method}: server closed stdout (exit code {self._proc.poll()})")
            try:
                message = json.loads(raw)
            except json.JSONDecodeError:
                raise HarnessError(f"{method}: non-JSON line on stdout: {raw[:EXCERPT]!r}") from None
            if message.get("id") != request_id:
                continue
            if "error" in message:
                return {"__jsonrpc_error__": message["error"]}
            return message.get("result", {})

    def initialize(self, timeout=60.0):
        result = self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "mcp-airbnb-live-smoke", "version": "1.0"},
            },
            timeout,
        )
        if "__jsonrpc_error__" in result:
            raise HarnessError(f"initialize failed: {result['__jsonrpc_error__']}")
        self.notify("notifications/initialized")
        return result

    def call_tool(self, name, arguments, timeout):
        started = time.monotonic()
        result = self.request("tools/call", {"name": name, "arguments": arguments}, timeout)
        seconds = time.monotonic() - started
        if "__jsonrpc_error__" in result:
            error = result["__jsonrpc_error__"]
            return True, f"JSON-RPC error {error.get('code')}: {error.get('message')}", seconds
        text = "\n".join(
            item.get("text", "") for item in result.get("content", []) if item.get("type") == "text"
        )
        return bool(result.get("isError")), text, seconds

    def close(self):
        try:
            if self._proc.stdin:
                self._proc.stdin.close()
        except OSError:
            pass
        try:
            self._proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            self._proc.wait(timeout=5)
        self._pump_thread.join(timeout=5)
        if self._proc.stdout:
            self._proc.stdout.close()


# ---------------------------------------------------------------- plan + run


def search_arguments(location: str, today: dt.date) -> dict:
    return {
        "location": location,
        "checkin": (today + dt.timedelta(days=30)).isoformat(),
        "checkout": (today + dt.timedelta(days=33)).isoformat(),
        "adults": 2,
    }


def plan_calls(location: str, second_location: str, ids: list[str], today: dt.date) -> list[tuple[str, dict]]:
    """Every tool except airbnb_search (called first to discover ids)."""
    listing = ids[0]
    dates = search_arguments(location, today)
    compare = {"ids": ids[:2]} if len(ids) >= 2 else {"location": location, "max_listings": 5}
    return [
        ("airbnb_listing_details", {"id": listing}),
        ("airbnb_reviews", {"id": listing}),
        ("airbnb_price_calendar", {"id": listing, "months": 1}),
        ("airbnb_host_profile", {"id": listing}),
        (
            "airbnb_neighborhood_stats",
            {"location": location, "checkin": dates["checkin"], "checkout": dates["checkout"]},
        ),
        ("airbnb_occupancy_estimate", {"id": listing, "months": 1}),
        ("airbnb_compare_listings", compare),
        ("airbnb_price_trends", {"id": listing, "months": 3}),
        ("airbnb_gap_finder", {"id": listing, "months": 1}),
        ("airbnb_revenue_estimate", {"id": listing, "location": location, "months": 3}),
        ("airbnb_listing_score", {"id": listing}),
        ("airbnb_amenity_analysis", {"id": listing, "location": location}),
        ("airbnb_market_comparison", {"locations": [location, second_location]}),
        ("airbnb_host_portfolio", {"id": listing}),
        ("airbnb_review_sentiment", {"id": listing, "max_pages": 1}),
        ("airbnb_competitive_positioning", {"id": listing, "location": location}),
        ("airbnb_optimal_pricing", {"id": listing, "location": location}),
    ]


def execute(client: McpStdio, name: str, arguments: dict, timeout: float) -> ToolRun:
    is_error, text, seconds = client.call_tool(name, arguments, timeout)
    run = ToolRun(name=name, is_error=is_error, text=text, seconds=seconds)
    if is_error:
        first = text.splitlines()[0] if text else "(no message)"
        run.flags.append(Flag("tool-error", excerpt(first)))
    elif not text.strip():
        run.flags.append(Flag("empty-output", "tool returned no text"))
    run.flags.extend(generic_flags(text))
    return run


def resolve_binary(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).resolve()
    release = REPO / "target" / "release" / "mcp-airbnb"
    if release.is_file():
        return release
    on_path = shutil.which("mcp-airbnb")
    if on_path:
        return Path(on_path)
    raise HarnessError("no mcp-airbnb binary: run `cargo build --release --bin mcp-airbnb` or pass --binary")


def report(runs: list[ToolRun], global_flags: list[Flag], stderr_log: Path, json_path: str | None) -> int:
    total = len(global_flags) + sum(len(r.flags) for r in runs)
    print(f"mcp-airbnb live smoke — {dt.date.today().isoformat()}")
    for run in runs:
        status = "FLAG" if run.flags else " OK "
        print(f"[{status}] {run.name:<32} {run.seconds:6.1f}s")
        for flag in run.flags:
            print(f"         {flag.code}: {flag.detail}")
    for flag in global_flags:
        print(f"[FLAG] {flag.code}: {flag.detail}")
    print(f"Summary: {len(runs)} tool calls, {total} flags. Server stderr: {stderr_log}")
    if json_path:
        payload = {
            "date": dt.date.today().isoformat(),
            "tools": [
                {
                    "name": r.name,
                    "is_error": r.is_error,
                    "seconds": round(r.seconds, 2),
                    "flags": [f.__dict__ for f in r.flags],
                }
                for r in runs
            ],
            "flags": [f.__dict__ for f in global_flags],
        }
        Path(json_path).write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8")
    return 1 if total else 0


def run_smoke(args) -> int:
    binary = resolve_binary(args.binary)
    stderr_log = Path(tempfile.gettempdir()) / f"mcp-airbnb-smoke-{os.getpid()}.log"
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "info")
    if args.config:
        env["AIRBNB_CONFIG"] = str(Path(args.config).resolve())
    client = McpStdio([str(binary)], cwd=REPO, env=env, stderr_path=stderr_log)
    runs: list[ToolRun] = []
    global_flags: list[Flag] = []
    try:
        client.initialize(args.timeout)
        listed = client.request("tools/list", {}, args.timeout).get("tools", [])
        names = {tool.get("name") for tool in listed}
        global_flags += [Flag("tool-missing", n) for n in sorted(set(EXPECTED_TOOLS) - names)]
        global_flags += [Flag("tool-not-exercised", n) for n in sorted(names - set(EXPECTED_TOOLS))]
        today = dt.date.today()
        runs.append(execute(client, "airbnb_search", search_arguments(args.location, today), args.timeout))
        found = listing_ids(runs[0].text)
        ids = ([args.listing_id] if args.listing_id else []) + [i for i in found if i != args.listing_id]
        if not ids:
            global_flags.append(Flag("no-listing-id", "search returned no listing id; pass --listing-id"))
            return report(runs, global_flags, stderr_log, args.json_report)
        for name, arguments in plan_calls(args.location, args.second_location, ids, today):
            time.sleep(args.pause)
            runs.append(execute(client, name, arguments, args.timeout))
        resources = client.request("resources/list", {}, args.timeout).get("resources", [])
        global_flags += flag_unencoded_uris([r.get("uri", "") for r in resources])
    except HarnessError as exc:
        print(f"harness error: {exc}\nserver stderr: {stderr_log}", file=sys.stderr)
        return 2
    finally:
        client.close()
    by_name = {run.name: run for run in runs}
    if "airbnb_listing_details" in by_name and "airbnb_reviews" in by_name:
        by_name["airbnb_reviews"].flags += flag_empty_reviews(
            by_name["airbnb_listing_details"].text, by_name["airbnb_reviews"].text
        )
    if "airbnb_price_calendar" in by_name and "airbnb_occupancy_estimate" in by_name:
        by_name["airbnb_occupancy_estimate"].flags += flag_past_dates_in_occupancy(
            by_name["airbnb_price_calendar"].text, by_name["airbnb_occupancy_estimate"].text
        )
    by_name["airbnb_search"].flags += flag_mixed_currencies(by_name["airbnb_search"].text)
    return report(runs, global_flags, stderr_log, args.json_report)


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description="Opt-in live smoke test of mcp-airbnb against real Airbnb.")
    parser.add_argument("--live", action="store_true", help="required: confirms you want real upstream traffic")
    parser.add_argument("--binary", help="path to mcp-airbnb (default: target/release/mcp-airbnb, then PATH)")
    parser.add_argument("--config", help="config.yaml to use (sets AIRBNB_CONFIG)")
    parser.add_argument("--location", default="Lyon, France")
    parser.add_argument("--second-location", default="Bordeaux, France")
    parser.add_argument("--listing-id", help="listing to analyse (default: first search result)")
    parser.add_argument("--pause", type=float, default=3.0, help="seconds between tool calls (default 3)")
    parser.add_argument("--timeout", type=float, default=240.0, help="seconds per call (default 240)")
    parser.add_argument("--json-report", help="write flags (no tool output) to this JSON file")
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    if not args.live:
        print("refusing to run: pass --live to send real requests to Airbnb (never run this in CI)", file=sys.stderr)
        return 2
    try:
        return run_smoke(args)
    except HarnessError as exc:
        print(f"harness error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
