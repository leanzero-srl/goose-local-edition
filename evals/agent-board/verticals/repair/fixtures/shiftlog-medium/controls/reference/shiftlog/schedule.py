from typing import List

from .intervals import Interval, overlaps


class Schedule:
    """A set of work shifts that may not conflict."""

    def __init__(self) -> None:
        self._shifts: List[Interval] = []

    def add(self, start: int, end: int) -> Interval:
        shift = Interval(start, end)
        for existing in self._shifts:
            if overlaps(shift, existing):
                raise ValueError(
                    f"shift {start}-{end} conflicts with {existing.start}-{existing.end}"
                )
        self._shifts.append(shift)
        self._shifts.sort(key=lambda i: i.start)
        return shift

    @property
    def shifts(self) -> List[Interval]:
        return list(self._shifts)

    def total_hours(self) -> int:
        return sum(i.end - i.start for i in self._shifts)


def merge_busy(intervals: List[Interval]) -> List[Interval]:
    """Collapse spans into the fewest covering blocks.

    Spans that merely touch — one ending exactly where the next begins — are one continuous
    block of busy time, not two.
    """
    if not intervals:
        return []
    ordered = sorted(intervals, key=lambda i: i.start)
    merged = [ordered[0]]
    for span in ordered[1:]:
        last = merged[-1]
        if overlaps(last, span) or last.end == span.start:
            merged[-1] = Interval(last.start, max(last.end, span.end))
        else:
            merged.append(span)
    return merged
