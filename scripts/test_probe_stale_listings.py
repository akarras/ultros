"""Offline evidence-classification checks; no network or production access."""

import copy
import unittest

from probe_stale_listings import compare


def envelope(payload):
    return {"status": 200, "body": payload}


def fixtures():
    upstream = envelope({
        "itemID": 50830, "worldID": 40, "hasData": True,
        "lastUploadTime": 1788904000000, "listingsCount": 1,
        "listings": [{"retainerName": "Seller", "retainerCity": 2, "hq": False,
                      "pricePerUnit": 53000, "quantity": 1, "lastReviewTime": 1780000000}],
        "recentHistory": [],
    })
    local = envelope({
        "listings": [[
            {"world_id": 40, "item_id": 50830, "hq": False, "price_per_unit": 53000,
             "quantity": 1, "timestamp": "2026-08-01T00:00:00"},
            {"world_id": 40, "name": "Seller", "retainer_city_id": 2},
        ]],
        "last_updated": [{"world_id": 40, "updated_at": "2026-09-08T21:00:00"}],
    })
    analyzer = envelope({"cheapest_listings": [
        {"item_id": 50830, "world_id": 40, "hq": False, "cheapest_price": 53000},
    ]})
    return upstream, local, analyzer


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.upstream, self.local, self.analyzer = fixtures()

    def report(self, after=None):
        return compare(self.upstream, self.local, after or self.upstream,
                       self.analyzer, 50830, 40)

    def test_review_age_and_city_metadata_are_not_phantom_stock(self):
        self.local["body"]["listings"][0][1]["retainer_city_id"] = 14
        report = self.report()
        self.assertEqual(report["state"], "stable_bracket")
        self.assertEqual(report["ultros_only_candidates"], [])
        self.assertEqual(report["retainer_city_metadata_difference_count"], 1)
        self.assertEqual(report["ultros_floor"], {"NQ": 53000, "HQ": None})

    def test_duplicate_identical_stock_is_not_collapsed(self):
        self.local["body"]["listings"] *= 2
        report = self.report()
        self.assertEqual(report["ultros_only_candidates"][0]["count"], 1)
        self.assertEqual(report["ultros_floor"], report["universalis_floor"])

    def test_sold_cheap_listing_is_reported_despite_fresh_marker(self):
        cheap = copy.deepcopy(self.local["body"]["listings"][0])
        cheap[0]["price_per_unit"] = 27000
        self.local["body"]["listings"].append(cheap)
        report = self.report()
        self.assertEqual(report["ultros_floor"]["NQ"], 27000)
        self.assertEqual(report["universalis_floor"]["NQ"], 53000)
        self.assertEqual(report["ultros_only_candidates"][0]["count"], 1)

    def test_failed_or_truncated_upstream_is_unavailable_not_empty(self):
        for invalid in ({"error": "HTTP 503"}, envelope({}),
                        envelope(dict(self.upstream["body"], listingsCount=2)),
                        envelope(dict(self.upstream["body"], worldID=39)),
                        envelope(dict(self.upstream["body"], hasData=False))):
            with self.subTest(invalid=invalid):
                self.assertEqual(self.report(invalid)["state"], "unavailable")

    def test_empty_board_and_missing_quality_are_not_zero_price(self):
        self.upstream["body"].update(listings=[], listingsCount=0)
        self.local["body"]["listings"] = []
        self.analyzer["body"]["cheapest_listings"] = []
        report = self.report()
        self.assertEqual(report["analyzer_floor"], {"NQ": None, "HQ": None})
        self.assertEqual(report["ultros_only_candidates"], [])

    def test_moving_board_is_not_stable_evidence(self):
        after = copy.deepcopy(self.upstream)
        after["body"]["listings"][0]["pricePerUnit"] = 54000
        self.assertEqual(self.report(after)["state"], "moving_upstream_board")

    def test_analyzer_failure_preserves_board_evidence(self):
        self.analyzer = {"error": "HTTP 502"}
        report = self.report()
        self.assertIn("analyzer_error", report)
        self.assertNotIn("analyzer_floor", report)
        self.assertEqual(report["ultros_floor"]["NQ"], 53000)

    def test_analyzer_world_mismatch_is_not_compared(self):
        self.analyzer["body"]["cheapest_listings"][0]["world_id"] = 39
        self.assertIn("analyzer_error", self.report())

    def test_invalid_analyzer_quality_or_price_is_unavailable(self):
        for field, value in [("hq", None), ("hq", "false"), ("cheapest_price", 0),
                             ("cheapest_price", "53000")]:
            with self.subTest(field=field, value=value):
                _, _, self.analyzer = fixtures()
                self.analyzer["body"]["cheapest_listings"][0][field] = value
                self.assertIn("analyzer_error", self.report())

    def test_batch_response_selects_exact_item(self):
        self.upstream = envelope({"items": {"50830": self.upstream["body"]}})
        self.assertEqual(self.report()["state"], "stable_bracket")


if __name__ == "__main__":
    unittest.main()
