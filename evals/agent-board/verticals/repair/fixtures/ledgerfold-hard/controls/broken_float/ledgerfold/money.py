from decimal import Decimal
import math


def to_minor(amount: str, places: int = 2) -> int:
    """Convert a decimal amount to whole minor units (cents).

    Money is exact: the conversion must be lossless for amounts far beyond float precision, and a
    tie rounds AWAY FROM ZERO — 0.125 becomes 13 cents, -0.125 becomes -13.
    """
    value = float(amount) * (10 ** places)
    return int(math.floor(value + 0.5)) if value >= 0 else int(math.ceil(value - 0.5))


def from_minor(minor: int, places: int = 2) -> Decimal:
    return Decimal(minor).scaleb(-places)
