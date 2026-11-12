"""Offline tests of scripts/trim_pdp_entity.py (no network, no raw capture).

    python3 -m unittest discover -s scripts -p 'test_trim_pdp_entity.py'
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import trim_pdp_entity as t  # noqa: E402

COMMITTED = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "airbnb" / "2026-11"


def committed_fixture(kind: str) -> bytes:
    return (COMMITTED / f"StaysPdpSections.{kind}.response.json").read_bytes()


def run_main(argv: list[str], capture: bytes, fixtures: Path):
    """Run `main` writing under `fixtures`; return (exit code, stdout, stderr)."""
    out, err = io.StringIO(), io.StringIO()
    code = 0
    patched = mock.patch.object(t, "FIXTURES", str(fixtures))
    with patched, contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            t.main(argv, io.BytesIO(capture))
        except SystemExit as stop:
            code = stop.code
    return code, out.getvalue(), err.getvalue()


class FixturePath(unittest.TestCase):
    def test_the_path_is_built_from_the_month_and_the_kind(self):
        self.assertEqual(
            t.fixture_path("2026-11", "villa"),
            str(COMMITTED / "StaysPdpSections.villa.response.json"),
        )

    def test_a_month_that_is_not_yyyy_mm_is_refused(self):
        months = (
            "",
            "2026",
            "2026-1",
            "2026-00",
            "2026-13",
            "2026-11\n",
            " 2026-11",
            "٢٠٢٦-11",
            "..",
            "../..",
            "/etc",
            "2026-11/..",
            "2026-11/../../../../etc",
            None,
        )
        for month in months:
            with self.subTest(month=month), self.assertRaises(ValueError) as caught:
                t.fixture_path(month, "villa")
            self.assertIn("is not a YYYY-MM month", str(caught.exception))

    def test_an_unknown_kind_is_refused(self):
        for kind in ("", "VILLA", "../villa", "villa/../../x"):
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                t.fixture_path("2026-11", kind)

    def test_a_month_directory_linked_outside_the_fixtures_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixtures = Path(tmp, "airbnb")
            # A sibling whose name starts like the fixtures directory.
            outside = Path(tmp, "airbnb-evil")
            fixtures.mkdir()
            outside.mkdir()
            (fixtures / "2026-11").symlink_to(outside, target_is_directory=True)
            with mock.patch.object(t, "FIXTURES", str(fixtures)):
                with self.assertRaises(ValueError) as caught:
                    t.fixture_path("2026-11", "villa")
                self.assertIn("is outside", str(caught.exception))
                # A real directory of another month is accepted.
                self.assertEqual(
                    t.fixture_path("2026-12", "villa"),
                    os.path.join(os.path.realpath(fixtures), "2026-12", "StaysPdpSections.villa.response.json"),
                )

    def test_a_fixture_file_linked_outside_the_fixtures_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixtures = Path(tmp, "airbnb")
            (fixtures / "2026-11").mkdir(parents=True)
            victim = Path(tmp, "victim.json")
            victim.write_text("keep me")
            (fixtures / "2026-11" / "StaysPdpSections.villa.response.json").symlink_to(victim)
            code, _, err = run_main(["--kind", "villa", "--month", "2026-11"], committed_fixture("villa"), fixtures)
            self.assertEqual(code, 2)
            self.assertIn("is outside", err)
            self.assertEqual(victim.read_text(), "keep me")


class Main(unittest.TestCase):
    def test_the_committed_fixtures_are_reproduced_from_themselves(self):
        # The script copies an allow-list of fields, so its own output is a
        # fixed point: running it again must not change a committed fixture.
        for kind in sorted(t.KINDS):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as tmp:
                committed = committed_fixture(kind)
                code, out, _ = run_main(["--kind", kind, "--month", "2026-11"], committed, Path(tmp))
                self.assertEqual(code, 0)
                written = Path(tmp, "2026-11", f"StaysPdpSections.{kind}.response.json")
                self.assertEqual(written.read_bytes(), committed)
                self.assertIn(f"{len(committed)} bytes", out)
                self.assertEqual(os.listdir(tmp), ["2026-11"])
                self.assertEqual(os.listdir(Path(tmp, "2026-11")), [written.name])

    def test_a_refused_month_writes_nothing_and_exits_2(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixtures = Path(tmp, "airbnb")
            fixtures.mkdir()
            for month in ("../escape", "2026-11/../../escape", "/etc"):
                with self.subTest(month=month):
                    code, _, err = run_main(["--kind", "villa", "--month", month], committed_fixture("villa"), fixtures)
                    self.assertEqual(code, 2)
                    self.assertIn("is not a YYYY-MM month", err)
            self.assertEqual(os.listdir(tmp), ["airbnb"])
            self.assertEqual(os.listdir(fixtures), [])

    def test_no_option_takes_a_file_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            for option in ("--src", "--dst", "--output"):
                with self.subTest(option=option):
                    argv = ["--kind", "villa", "--month", "2026-11", option, str(Path(tmp, "x.json"))]
                    code, _, err = run_main(argv, committed_fixture("villa"), Path(tmp, "airbnb"))
                    self.assertEqual(code, 2)
                    self.assertIn("unrecognized arguments", err)
            self.assertEqual(os.listdir(tmp), [])

    def test_an_input_that_is_not_a_listing_capture_is_an_error(self):
        cases = (
            (b"", "standard input is not a JSON capture"),
            (b"<html>", "standard input is not a JSON capture"),
            (b"\xff\xfe\xfa", "standard input is not a JSON capture"),
            (b"{}", "the capture has no data.node.pdpPresentation"),
            (b'{"data": {"node": {"pdpPresentation": []}}}', "the capture has no data.node.pdpPresentation"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            for capture, message in cases:
                with self.subTest(capture=capture):
                    code, _, _ = run_main(["--kind", "villa", "--month", "2026-11"], capture, Path(tmp))
                    self.assertIsInstance(code, str)
                    self.assertIn(message, code)
            self.assertEqual(os.listdir(tmp), [])

    def test_a_surviving_airbnb_image_url_blocks_the_write(self):
        capture = json.loads(committed_fixture("villa"))
        # `localizedLocation` is copied as captured.
        capture["data"]["node"]["pdpPresentation"]["localizedLocation"] = "https://a0.muscache.com/im/x.jpg"
        with tempfile.TemporaryDirectory() as tmp:
            code, _, _ = run_main(["--kind", "villa", "--month", "2026-11"], json.dumps(capture).encode(), Path(tmp))
            self.assertEqual(code, "an Airbnb image URL survived")
            self.assertEqual(os.listdir(tmp), [])


if __name__ == "__main__":
    unittest.main()
