"""Offline tests for scripts/live_smoke.py (no network, no Rust build).

    python3 -m unittest discover -s scripts -p 'test_live_smoke.py' -v
"""

from __future__ import annotations

import datetime as dt
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import live_smoke as ls  # noqa: E402

FAKE_SERVER = textwrap.dedent(
    """
    import json, sys
    for line in sys.stdin:
        msg = json.loads(line)
        if "id" not in msg:
            continue
        method = msg["method"]
        if method == "initialize":
            result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                      "serverInfo": {"name": "fake", "version": "0"}}
        elif method == "tools/list":
            result = {"tools": [{"name": "airbnb_search", "inputSchema": {"type": "object"}}]}
        elif method == "tools/call":
            if msg["params"]["name"] == "hang":
                continue
            result = {"content": [{"type": "text", "text": "Price: $0/night"}], "isError": False}
        else:
            result = {}
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/message", "params": {}}) + "\\n")
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}) + "\\n")
        sys.stdout.flush()
    """
)


class DetectorTests(unittest.TestCase):
    def test_zero_prices_are_flagged_in_all_three_spellings(self):
        text = "Overall: avg $0, min $0, max $0\n2026-10          0 $\nPrice: USD0/night"
        self.assertEqual([f.code for f in ls.flag_zero_prices(text)], ["zero-price"] * 3)

    def test_real_prices_and_zero_percentages_are_not_flagged(self):
        text = "Price: $60.37/night\n€108/night\n2026-10        100 $\nOccupancy: 0.0%\nPrice: $10/night"
        self.assertEqual(ls.flag_zero_prices(text), [])

    def test_placeholder_location_is_flagged_with_both_apostrophes(self):
        self.assertEqual(len(ls.flag_placeholder_location("Location: Where you’ll be\nWhere you'll be")), 2)
        self.assertEqual(ls.flag_placeholder_location("Location: Lyon, France"), [])

    def test_empty_reviews_are_flagged_only_when_the_listing_has_reviews(self):
        details = "# Place\nRating: 4.93 (490 reviews)\n"
        self.assertEqual([f.code for f in ls.flag_empty_reviews(details, "")], ["empty-reviews"])
        one_review = "**Guest A** (2026-08-01) - 5.0*\nGreat stay\n"
        self.assertEqual(ls.flag_empty_reviews(details, one_review), [])
        self.assertEqual(ls.flag_empty_reviews("# New place\n", ""), [])

    def test_occupancy_that_counts_past_days_is_flagged(self):
        calendar = (
            "Price calendar for listing 1 (USD)\n"
            "2026-09-01          -  No (Past date)          1\n"
            "2026-09-02          -  No (Past date)          1\n"
            "2026-09-29          -        Yes          1\n"
        )
        # Audit-time wording, then P2's wording of OccupancyEstimate's Display.
        for bad, good in (
            ("Days: 3 total, 2 occupied, 1 available", "Days: 1 total, 0 occupied, 1 available"),
            ("Nights considered: 3 (2 unavailable, 1 open)", "Nights considered: 1 (0 unavailable, 1 open)"),
        ):
            flagged = ls.flag_past_dates_in_occupancy(calendar, bad)
            self.assertEqual([f.code for f in flagged], ["occupancy-counts-past-dates"], bad)
            self.assertEqual(ls.flag_past_dates_in_occupancy(calendar, good), [], good)

    def test_hard_coded_occupancy_is_flagged(self):
        self.assertEqual(len(ls.flag_default_occupancy("Projected occupancy: 65.0%")), 1)
        self.assertEqual(len(ls.flag_default_occupancy("Occupancy: 33.0 (avg: 65.0)")), 1)
        self.assertEqual(ls.flag_default_occupancy("Projected occupancy: 71.4%"), [])

    def test_bad_ordinals_are_flagged_but_teens_are_not(self):
        self.assertEqual(len(ls.flag_bad_ordinals("33th percentile")), 1)
        self.assertEqual(len(ls.flag_bad_ordinals("21th percentile")), 1)
        self.assertEqual(ls.flag_bad_ordinals("11th, 12th, 13th and 4th"), [])

    def test_duplicated_labels_and_relay_ids_are_flagged(self):
        self.assertEqual(len(ls.flag_duplicated_labels("Response rate: Response rate: 100%")), 1)
        self.assertEqual(ls.flag_duplicated_labels("Response rate: 100%"), [])
        self.assertEqual(len(ls.flag_relay_ids("ID: RGVtYW5kVXNlcjoxMjM0")), 1)
        self.assertEqual(ls.flag_relay_ids("ID: 1000001"), [])

    def test_mixed_currencies_in_one_search_are_flagged(self):
        mixed = "1. **A** (ID: 1)\n   Lyon\n   $60.37/night\n2. **B** (ID: 2)\n   Lyon\n   €65/night"
        self.assertEqual([f.code for f in ls.flag_mixed_currencies(mixed)], ["mixed-currencies"])
        self.assertEqual(ls.flag_mixed_currencies("$60/night\n$70/night"), [])
        self.assertEqual(ls.listing_ids(mixed), ["1", "2"])

    def test_resource_uris_with_whitespace_are_flagged(self):
        uris = ["airbnb://search/Lyon, France", "airbnb://search/Lyon%2C%20France", "airbnb://listing/1"]
        self.assertEqual([f.detail for f in ls.flag_unencoded_uris(uris)], ["airbnb://search/Lyon, France"])

    def test_plan_exercises_every_tool_with_future_dates(self):
        today = dt.date(2026, 9, 28)
        planned = {"airbnb_search"} | {
            name for name, _ in ls.plan_calls("Lyon, France", "Bordeaux, France", ["1", "2"], today)
        }
        self.assertEqual(planned, set(ls.EXPECTED_TOOLS))
        search = ls.search_arguments("Lyon, France", today)
        self.assertGreater(dt.date.fromisoformat(search["checkin"]), today)
        self.assertLess(dt.date.fromisoformat(search["checkin"]), dt.date.fromisoformat(search["checkout"]))

    def test_main_refuses_to_run_without_live(self):
        self.assertEqual(ls.main([]), 2)


class StdioHarnessTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        server = Path(self._tmp.name) / "fake_server.py"
        server.write_text(FAKE_SERVER, encoding="utf-8")
        self.client = ls.McpStdio([sys.executable, str(server)])

    def tearDown(self):
        self.client.close()
        self._tmp.cleanup()

    def test_initialize_list_and_call_round_trip(self):
        info = self.client.initialize(timeout=10)
        self.assertEqual(info["serverInfo"]["name"], "fake")
        tools = self.client.request("tools/list", {}, timeout=10)["tools"]
        self.assertEqual([t["name"] for t in tools], ["airbnb_search"])
        is_error, text, _ = self.client.call_tool("airbnb_search", {"location": "x"}, timeout=10)
        self.assertFalse(is_error)
        self.assertEqual(text, "Price: $0/night")

    def test_silent_server_times_out_instead_of_hanging(self):
        self.client.initialize(timeout=10)
        with self.assertRaises(ls.HarnessError):
            self.client.call_tool("hang", {}, timeout=1)

    def test_dead_server_is_a_harness_error(self):
        dead = ls.McpStdio([sys.executable, "-c", "import sys; sys.exit(3)"])
        try:
            with self.assertRaises(ls.HarnessError):
                dead.initialize(timeout=10)
        finally:
            dead.close()


if __name__ == "__main__":
    unittest.main()
