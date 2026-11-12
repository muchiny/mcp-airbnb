"""Offline tests of scripts/discover_query_ids.py (no network).

    python3 -m unittest discover -s scripts -p 'test_discover_query_ids.py'
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import discover_query_ids as d  # noqa: E402

ID_A = "a" * 64
ID_B = "b" * 64
ID_C = "c" * 64


class BundleUrls(unittest.TestCase):
    def test_keeps_static_bundles_once_and_in_order(self):
        html = (
            '<script src="https://a0.muscache.com/airbnb/static/packages/web/common/a.1f.js"></script>'
            '<link rel="preload" href="https://a0.muscache.com/airbnb/static/packages/web/en/b.2e.js">'
            '<script src="https://a0.muscache.com/airbnb/static/packages/web/common/a.1f.js"></script>'
        )
        self.assertEqual(
            d.bundle_urls(html),
            [
                "https://a0.muscache.com/airbnb/static/packages/web/common/a.1f.js",
                "https://a0.muscache.com/airbnb/static/packages/web/en/b.2e.js",
            ],
        )

    def test_ignores_other_hosts_paths_and_traversal(self):
        html = (
            '<script src="https://d0a7e.airbnb.com/tags.js"></script>'
            '<script src="https://evil.example/airbnb/static/x.js"></script>'
            '<script src="https://a0.muscache.com/other/x.js"></script>'
            '<script src="https://a0.muscache.com/airbnb/static/../../x.js"></script>'
            '<script src="https://a0.muscache.com.evil.example/airbnb/static/x.js"></script>'
        )
        self.assertEqual(d.bundle_urls(html), [])


class Operations(unittest.TestCase):
    def test_reads_queries_and_mutations(self):
        js = (
            f"e.default={{name:'StaysPdpSections',type:'query',operationId:'{ID_A}',productContext:'P'}};"
            f"e.default={{name:'SaveListing',type:'mutation',operationId:'{ID_B}'}};"
        )
        self.assertEqual(d.operations(js), {"StaysPdpSections": ID_A, "SaveListing": ID_B})

    def test_ignores_malformed_ids(self):
        js = "name:'StaysSearch',type:'query',operationId:'abc'" + f"name:'X',type:'other',operationId:'{ID_A}'"
        self.assertEqual(d.operations(js), {})


class ConfiguredIds(unittest.TestCase):
    CONFIG = f"""scraper:
  rate_limit_per_second: 0.5
  graphql_hashes:
    stays_search: "{ID_A}"
    # a comment
    stays_pdp_sections: "{ID_B}"

cache:
  max_entries: 500
  not_an_id: "{ID_C}"
"""

    def test_reads_only_the_graphql_hashes_block(self):
        self.assertEqual(
            d.configured_ids(self.CONFIG),
            {"stays_search": ID_A, "stays_pdp_sections": ID_B},
        )

    def test_a_config_without_the_block_has_no_ids(self):
        self.assertEqual(d.configured_ids("scraper:\n  locale: en\n"), {})

    def test_the_committed_config_lists_every_wanted_key(self):
        ids = d.configured_ids((d.REPO / "config.yaml").read_text(encoding="utf-8"))
        self.assertEqual(set(ids), set(d.WANTED.values()))


class Compare(unittest.TestCase):
    def test_same_changed_and_not_found(self):
        found = {"StaysSearch": ID_A, "StaysPdpSections": ID_C, "Unrelated": ID_B}
        configured = {"stays_search": ID_A, "stays_pdp_sections": ID_B, "stays_pdp_reviews": ID_B}
        rows = {operation: (key, status, value) for operation, key, status, value in d.compare(found, configured)}
        self.assertEqual(rows["StaysSearch"], ("stays_search", "SAME", ID_A))
        self.assertEqual(rows["StaysPdpSections"], ("stays_pdp_sections", "CHANGED", ID_C))
        self.assertEqual(rows["StaysPdpReviewsQuery"], ("stays_pdp_reviews", "NOT FOUND", ID_B))
        self.assertEqual(rows["StaysPdpBookItQuery"], ("stays_pdp_book_it", "NOT FOUND", ""))
        self.assertEqual(len(rows), len(d.WANTED))

    def test_yaml_block_lists_found_ids_only(self):
        rows = d.compare({"StaysSearch": ID_A}, {})
        self.assertEqual(
            d.yaml_block(rows),
            f'scraper:\n  graphql_hashes:\n    stays_search: "{ID_A}"',
        )


class Pages(unittest.TestCase):
    def test_pages_are_home_search_and_listing(self):
        self.assertEqual(
            d.page_urls("https://www.airbnb.com", "Paris, France", "42"),
            [
                "https://www.airbnb.com/",
                "https://www.airbnb.com/s/Paris-France/homes",
                "https://www.airbnb.com/rooms/42",
            ],
        )
        self.assertEqual(len(d.page_urls("https://www.airbnb.com", "Lyon", None)), 2)

    def test_bad_arguments_exit_2_without_network(self):
        self.assertEqual(d.main(["--base-url", "http://www.airbnb.com"]), 2)
        self.assertEqual(d.main(["--base-url", "https://www.airbnb.com/x"]), 2)
        self.assertEqual(d.main(["--listing-id", "12/../x"]), 2)
        self.assertEqual(d.main(["--listing-id", "042"]), 2)
        self.assertEqual(d.main(["--config", "/nonexistent/config.yaml"]), 2)


class Redirects(unittest.TestCase):
    def test_only_the_same_https_host_is_the_same_origin(self):
        page = "https://www.airbnb.com/rooms/42"
        self.assertTrue(d.same_origin(page, "https://www.airbnb.com/rooms/42?locale=en"))
        self.assertTrue(d.same_origin(page, "https://WWW.AIRBNB.COM/"))
        self.assertFalse(d.same_origin(page, "http://www.airbnb.com/rooms/42"))
        self.assertFalse(d.same_origin(page, "https://www.airbnb.fr/rooms/42"))
        self.assertFalse(d.same_origin(page, "https://www.airbnb.com.evil.example/"))
        self.assertFalse(d.same_origin(page, "https://www.airbnb.com:8443/"))
        self.assertFalse(d.same_origin(page, "ftp://www.airbnb.com/x"))
        self.assertFalse(d.same_origin(page, "/relative"))

    def test_a_redirect_to_another_host_is_refused(self):
        handler = d.SameOriginRedirects()
        request = d.urllib.request.Request("https://a0.muscache.com/airbnb/static/a.js")
        with self.assertRaises(d.urllib.error.URLError):
            handler.redirect_request(request, None, 302, "Found", {}, "https://evil.example/a.js")
        with self.assertRaises(d.urllib.error.URLError):
            handler.redirect_request(request, None, 302, "Found", {}, "http://a0.muscache.com/airbnb/static/a.js")
        followed = handler.redirect_request(
            request, None, 302, "Found", {}, "https://a0.muscache.com/airbnb/static/b.js"
        )
        self.assertEqual(followed.full_url, "https://a0.muscache.com/airbnb/static/b.js")

    def test_server_text_is_printed_without_control_characters(self):
        self.assertEqual(d.printable("ok \x1b[31mred\x07\nnext"), "ok ?[31mred??next")
        self.assertEqual(len(d.printable("x" * 1000)), 300)
        self.assertEqual(d.printable(ValueError("boom")), "boom")


if __name__ == "__main__":
    unittest.main()
