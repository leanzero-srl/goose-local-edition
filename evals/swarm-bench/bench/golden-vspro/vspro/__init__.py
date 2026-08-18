"""VendorSync Pro — the sb-6 reference implementation.

This is the hand-written correct app the freeze gate scores (red-team F7/G6): every
non-calibration-owned check must pass against it before any threshold is trusted. It is the
instrument's ruler, so it follows spec-build-v3 (as amended by the SB6-PACKAGE contradiction
ledger and the binding red-team amendments) to the letter.
"""

__version__ = "1.0.0"

STATUSES = ("settled", "pending", "refunded", "failed")
CURRENCIES = ("EUR", "USD", "JPY", "KWD")
CURRENCY_EXPONENT = {"EUR": 2, "USD": 2, "JPY": 0, "KWD": 3}
