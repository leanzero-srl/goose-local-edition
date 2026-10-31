"""b_money_rendered grades each rendered cell against the payment it shows, never a fixed page.

Observations are the saved `load` probe emits of two real sb-7.2 runs (MiMo split, 2026-10-30):
8985a86d renders newest-first (pay_12287..12276), a16c45bf ascending (pay_00000..00011).
"""
import unittest
from pathlib import Path

import fixtures_v3
import score_sb7 as s

RUN_8985A86D = {
    "seed": "d693ea1f071edb3b",
    "rows": [("pay_12287", "$224.00"), ("pay_12286", "¥313"), ("pay_12285", "KWD 502.000"),
             ("pay_12284", "€14.10"), ("pay_12283", "€51.40"), ("pay_12282", "€38.50"),
             ("pay_12281", "€7.08"), ("pay_12280", "€1,920.00"), ("pay_12279", "$1,600.00"),
             ("pay_12278", "$4.02"), ("pay_12277", "$78.30"), ("pay_12276", "KWD 2,530.000")],
}
RUN_A16C45BF = {
    "seed": "d0cfabb8f4370603",
    "rows": [("pay_00000", "$76.50"), ("pay_00001", "$33.40"), ("pay_00002", "KWD 241.000"),
             ("pay_00003", "KWD 1,290.000"), ("pay_00004", "KWD 64.900"), ("pay_00005", "¥6"),
             ("pay_00006", "€760.00"), ("pay_00007", "¥541"), ("pay_00008", "€623.00"),
             ("pay_00009", "$192.00"), ("pay_00010", "$424.00"), ("pay_00011", "¥69")],
}

CHECK = next(fn for name, _tier, fn in s.SB7_CHECKS if name == "b_money_rendered")
_PACKS = {}


def run_check(seed, rows, with_ids=True):
    if seed not in _PACKS:
        _PACKS[seed] = fixtures_v3.build(seed)
    c = s.Ctx(Path("/nonexistent"))
    c.pack = _PACKS[seed]
    c.payments = {"total": s.N_FROZEN}
    load = {"amountTexts": [t for _id, t in rows]}
    if with_ids:
        load["rowAmounts"] = [{"id": pid, "amountText": t} for pid, t in rows]
    c.probes = {"load": load}
    return CHECK(c)


class MoneyRenderedTests(unittest.TestCase):
    def test_8985a86d_newest_first_is_12_of_12(self):
        r = run_check(RUN_8985A86D["seed"], RUN_8985A86D["rows"])
        self.assertEqual(r["score"], 1.0, r["detail"])
        self.assertEqual(r["parts"]["ok_frac"], 1.0)
        self.assertEqual(r["parts"]["jpy_kwd"], [1.0, 1.0])

        self.assertIn("12 of 12 cells matched by row id; 0 without a pack id", r["detail"])

    def test_cells_without_row_ids_score_0_and_are_named(self):
        r = run_check(RUN_8985A86D["seed"], RUN_8985A86D["rows"], with_ids=False)
        self.assertEqual(r["score"], 0.0, r["detail"])
        self.assertEqual(r["parts"]["without_pack_id"], 12)
        self.assertIn("0 of 12 cells matched by row id; 12 without a pack id (scored 0)", r["detail"])

    def test_a16c45bf_ascending_still_12_of_12(self):
        r = run_check(RUN_A16C45BF["seed"], RUN_A16C45BF["rows"])
        self.assertEqual(r["score"], 1.0, r["detail"])

    def test_planted_digit_errors_fail_by_id_and_with_made_up_ids(self):
        """Review 2026-10-31: each of these passed 12/12 against the whole-pack fallback."""
        for idx, text in ((10, "$7.83"), (3, "€141.00"), (1, "¥312")):
            r = self._planted(idx, text, True)
            self.assertAlmostEqual(r["parts"]["ok_frac"], 11 / 12, places=3, msg=text)
            self.assertLess(r["score"], 1.0, text)
        jpy = self._planted(1, "¥312", True)
        self.assertEqual(jpy["parts"]["jpy_kwd"][0], 0.0)
        rows = [(f"row-{i}", t) for i, (_pid, t) in enumerate(RUN_8985A86D["rows"])]
        rows[10] = ("row-10", "$7.83")
        made_up = run_check(RUN_8985A86D["seed"], rows)
        self.assertEqual(made_up["score"], 0.0, made_up["detail"])
        self.assertEqual(made_up["parts"]["without_pack_id"], 12)

    def _planted(self, idx, text, with_ids):
        rows = list(RUN_8985A86D["rows"])
        rows[idx] = (rows[idx][0], text)
        return run_check(RUN_8985A86D["seed"], rows, with_ids=with_ids)

    def test_planted_wrong_exponents_fail_with_and_without_ids(self):
        for with_ids in (True, False):
            jpy = self._planted(1, "¥313.00", with_ids)
            self.assertEqual(jpy["parts"]["jpy_kwd"][0], 0.0, jpy["detail"])
            self.assertLess(jpy["score"], 1.0)
            kwd = self._planted(2, "KWD 5,020.00", with_ids)
            self.assertEqual(kwd["parts"]["jpy_kwd"][1], 0.0, kwd["detail"])
            self.assertLess(kwd["score"], 1.0)

    def test_joined_cell_with_another_payments_amount_fails(self):
        # $1,600.00 is a real USD pack amount (pay_12279) but not pay_12287's $224.00
        r = self._planted(0, "$1,600.00", True)
        self.assertAlmostEqual(r["parts"]["ok_frac"], 11 / 12, places=3)

    def test_joined_cell_in_the_wrong_currency_fails(self):
        r = self._planted(0, "€224.00", True)
        self.assertAlmostEqual(r["parts"]["ok_frac"], 11 / 12, places=3)

    def test_misaligned_row_ids_score_0(self):
        rows = RUN_8985A86D["rows"]
        c_rows = [{"id": "pay_00000", "amountText": "elsewhere"}] + [
            {"id": pid, "amountText": t} for pid, t in rows]
        if RUN_8985A86D["seed"] not in _PACKS:
            _PACKS[RUN_8985A86D["seed"]] = fixtures_v3.build(RUN_8985A86D["seed"])
        c = s.Ctx(Path("/nonexistent"))
        c.pack = _PACKS[RUN_8985A86D["seed"]]
        c.payments = {"total": s.N_FROZEN}
        c.probes = {"load": {"amountTexts": [t for _i, t in rows], "rowAmounts": c_rows}}
        r = CHECK(c)
        self.assertEqual(r["score"], 0.0)
        self.assertEqual(r["parts"]["without_pack_id"], 12)


    def test_cross_currency_sum_still_zeroes_correct_newest_first_cells(self):
        """The F16 leg is untouched by the join: correct cells plus a summary that sums EUR and
        JPY minors into one number is still wrong money (0.0, the severity input too)."""
        seed, rows = RUN_8985A86D["seed"], RUN_8985A86D["rows"]
        if seed not in _PACKS:
            _PACKS[seed] = fixtures_v3.build(seed)
        c = s.Ctx(Path("/nonexistent"))
        c.pack = _PACKS[seed]
        c.payments = {"total": s.N_FROZEN}
        c.probes = {"load": {"amountTexts": [t for _i, t in rows],
                             "rowAmounts": [{"id": i, "amountText": t} for i, t in rows]}}
        c.summary = {"by_currency": [{"currency": "EUR", "count": 1, "total_minor": 100},
                                     {"currency": "JPY", "count": 1, "total_minor": 7}],
                     "grand_total": 107}
        r = CHECK(c)
        self.assertEqual(r["score"], 0.0)
        self.assertEqual(r["parts"]["cross_currency_sum"], ["grand_total"])
        self.assertEqual(r["parts"]["ok_frac"], 1.0)

if __name__ == "__main__":
    unittest.main()
