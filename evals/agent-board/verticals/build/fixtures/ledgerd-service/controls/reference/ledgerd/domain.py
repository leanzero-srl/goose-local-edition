import re
from typing import Dict, List, Tuple

from .money import AmountError, to_minor
from .store import ACCOUNT_TYPES, DEBIT_POSITIVE

DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


class LedgerError(ValueError):
    pass


def check_date(text: str) -> str:
    if not DATE.match(text or ""):
        raise LedgerError(f"date must be YYYY-MM-DD, got {text!r}")
    year, month, day = (int(p) for p in text.split("-"))
    if not (1 <= month <= 12 and 1 <= day <= 31):
        raise LedgerError(f"date is not a real date: {text!r}")
    return text


def check_type(kind: str) -> str:
    if kind not in ACCOUNT_TYPES:
        raise LedgerError(f"account type must be one of {', '.join(ACCOUNT_TYPES)}, got {kind!r}")
    return kind


def parse_leg(text: str) -> Tuple[str, int]:
    if ":" not in text:
        raise LedgerError(f"leg must be ACCOUNT:AMOUNT, got {text!r}")
    name, _, amount = text.rpartition(":")
    if not name:
        raise LedgerError(f"leg must name an account, got {text!r}")
    try:
        return name, to_minor(amount)
    except AmountError as exc:
        raise LedgerError(str(exc))


def check_balanced(legs: List[Tuple[str, int]]) -> None:
    if len(legs) < 2:
        raise LedgerError("an entry needs at least two legs")
    total = sum(amount for _, amount in legs)
    if total != 0:
        raise LedgerError(f"entry legs must sum to zero, they sum to {total}")


def signed_balance(kind: str, debits: int, credits: int) -> int:
    return debits - credits if kind in DEBIT_POSITIVE else credits - debits
