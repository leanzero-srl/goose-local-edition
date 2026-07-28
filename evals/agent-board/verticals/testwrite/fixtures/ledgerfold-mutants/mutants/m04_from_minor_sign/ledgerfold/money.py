from decimal import Decimal, ROUND_HALF_UP


def to_minor(amount: str, places: int = 2) -> int:
    """Convert a decimal amount to whole minor units (cents).

    Money is exact: the conversion must be lossless for amounts far beyond float precision, and a
    tie rounds AWAY FROM ZERO — 0.125 becomes 13 cents, -0.125 becomes -13.
    """
    scaled = Decimal(amount).scaleb(places)
    return int(scaled.to_integral_value(rounding=ROUND_HALF_UP))


def from_minor(minor: int, places: int = 2) -> Decimal:
    return Decimal(minor).scaleb(places)
