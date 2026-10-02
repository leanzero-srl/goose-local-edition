"""Serial real-browser controls for SB7.2's overview legibility, enabled with SB72_BROWSER_TESTS=1.

Same harness as test_sb71_visual.py (the reference app booted against a seeded vendor, the probe's
sb71-visual scenario), run with golden-sb72 and product_probe_sb72.mjs. Each control is one
sb72_controls.py defect; the full-scorer consequences are recorded in sb7.2/DESIGN.md.
"""
import os
import unittest

import sb72_controls
import test_sb71_visual


@unittest.skipUnless(os.environ.get('SB72_BROWSER_TESTS') == '1', 'opt-in real-browser SB7.2 controls')
class OverviewLegibilityControls(unittest.TestCase):
    golden = 'golden-sb72'
    probe = 'product_probe_sb72.mjs'
    collect = test_sb71_visual.VisualControls.collect

    def control(self, name):
        (patch,) = sb72_controls.CONTROLS[name]
        return self.collect(patch)

    def legibility(self, rows):
        return rows['q_overview_legibility']['parts']

    def test_reference_overview_is_framed_lit_and_reads_status(self):
        rows = self.collect()
        for name, row in rows.items():
            self.assertEqual(row['score'], 1, (name, row.get('detail')))
        parts = self.legibility(rows)
        self.assertTrue(parts['framing']['ok'] and parts['lit']['ok'] and parts['status']['ok'], parts)

    def test_unlit_field_fails_lighting_and_visible_admission(self):
        rows = self.control('unlit-field')
        self.assertFalse(self.legibility(rows)['lit']['ok'])
        self.assertEqual(rows['s_visible_surface']['score'], 0)

    def test_misframed_canvas_fails_framing_only(self):
        rows = self.control('misframed-canvas')
        self.assertFalse(self.legibility(rows)['framing']['ok'])
        self.assertLess(rows['q_overview_legibility']['score'], 1)
        self.assertEqual(rows['s_visible_surface']['score'], 1)

    def test_swapped_status_colours_misread_against_the_legend(self):
        rows = self.control('status-swapped')
        status = self.legibility(rows)['status']
        self.assertLess(status['accuracy'], .9, status)

    def test_low_contrast_button_is_a_text_defect_not_a_3d_one(self):
        rows = self.control('low-contrast-button')
        self.assertLess(rows['q_legible_presentation']['score'], 1)
        self.assertEqual(rows['q_overview_legibility']['score'], 1)
        self.assertTrue(all(f['ok'] for f in rows['q_legible_presentation']['parts']['framing']))


if __name__ == '__main__':
    unittest.main()
