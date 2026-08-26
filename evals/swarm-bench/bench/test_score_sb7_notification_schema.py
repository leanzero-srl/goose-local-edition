import tempfile
import unittest
from pathlib import Path

from bench.score_sb7 import Ctx, SB7_CHECKS, _record_notifier_notifications


def notification_multiset_check(ctx: Ctx):
    check_fn = next(
        fn for name, _tier, fn in SB7_CHECKS if name == "r_notification_multiset"
    )
    return check_fn(ctx)


class NotificationMultisetSchemaTests(unittest.TestCase):
    def make_ctx(self) -> Ctx:
        ctx = Ctx(Path(tempfile.mkdtemp()))
        ctx.events = [{"type": "draft.submitted"}]
        return ctx

    def test_no_http_response_is_probe_unavailable(self):
        result = notification_multiset_check(self.make_ctx())

        self.assertTrue(result["unavailable"])
        self.assertIn("no HTTP response", result["detail"])

    def test_reachable_wrong_schema_is_product_zero(self):
        ctx = self.make_ctx()
        _record_notifier_notifications(
            ctx, 200, {"notifications": [], "limit": 200, "offset": 0}
        )

        result = notification_multiset_check(ctx)

        self.assertEqual(result["score"], 0.0)
        self.assertNotIn("unavailable", result)
        self.assertIn("expected an object containing a data array", result["detail"])
        self.assertIn("response schema is wrong", result["consequence"])

    def test_reachable_http_error_is_product_zero(self):
        ctx = self.make_ctx()
        _record_notifier_notifications(ctx, 404, {"error": "not found"})

        result = notification_multiset_check(ctx)

        self.assertEqual(result["score"], 0.0)
        self.assertNotIn("unavailable", result)
        self.assertIn("HTTP 404", result["detail"])

    def test_documented_data_array_scores_normally(self):
        ctx = self.make_ctx()
        _record_notifier_notifications(
            ctx, 200, {"data": [{"kind": "draft.submitted"}]}
        )

        result = notification_multiset_check(ctx)

        self.assertEqual(result["score"], 1.0)
        self.assertNotIn("unavailable", result)

    def test_transport_drop_does_not_erase_prior_reachable_evidence(self):
        ctx = self.make_ctx()
        _record_notifier_notifications(
            ctx, 200, {"notifications": [], "limit": 200, "offset": 0}
        )
        _record_notifier_notifications(ctx, None, None)

        result = notification_multiset_check(ctx)

        self.assertEqual(result["score"], 0.0)
        self.assertNotIn("unavailable", result)


if __name__ == "__main__":
    unittest.main()
