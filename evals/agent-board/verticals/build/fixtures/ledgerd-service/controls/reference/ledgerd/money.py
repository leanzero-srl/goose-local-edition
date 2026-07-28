from decimal import Decimal, InvalidOperation, ROUND_HALF_UP


class AmountError(ValueError):
    pass


def to_minor(text: str, places: int = 2) -> int:
    try:
        value = Decimal(text)
    except (InvalidOperation, TypeError):
        raise AmountError(f"not a valid decimal amount: {text!r}")
    return int(value.scaleb(places).to_integral_value(rounding=ROUND_HALF_UP))
