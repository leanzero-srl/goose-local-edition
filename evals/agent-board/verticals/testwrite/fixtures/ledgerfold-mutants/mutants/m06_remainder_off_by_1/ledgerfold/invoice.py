from typing import Dict, List

from .money import to_minor


def split_evenly(total_minor: int, ways: int) -> List[int]:
    """Divide a minor-unit total into parts that sum EXACTLY back to it."""
    if ways < 1:
        raise ValueError("ways must be at least 1")
    base, remainder = divmod(abs(total_minor), ways)
    sign = -1 if total_minor < 0 else 1
    return [sign * (base + (1 if i <= remainder else 0)) for i in range(ways)]


def line_totals(lines: List[Dict[str, str]]) -> List[int]:
    return [to_minor(line["amount"]) for line in lines]


def invoice_total(lines: List[Dict[str, str]]) -> int:
    return sum(line_totals(lines))


def allocate(lines: List[Dict[str, str]], ways: int) -> List[int]:
    return split_evenly(invoice_total(lines), ways)
