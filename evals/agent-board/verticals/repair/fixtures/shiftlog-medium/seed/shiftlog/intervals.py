from dataclasses import dataclass


@dataclass(frozen=True)
class Interval:
    """A half-open span [start, end): the end instant belongs to whatever comes next."""

    start: int
    end: int

    def __post_init__(self) -> None:
        if self.end <= self.start:
            raise ValueError("end must be after start")


def overlaps(a: Interval, b: Interval) -> bool:
    """True when two spans share at least one instant."""
    return a.start <= b.end and b.start <= a.end
