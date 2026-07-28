import pytest

from shiftlog import Interval, Schedule, merge_busy


def test_rejects_a_genuinely_overlapping_shift():
    s = Schedule()
    s.add(9, 12)
    with pytest.raises(ValueError):
        s.add(11, 14)


def test_allows_shifts_that_only_touch():
    s = Schedule()
    s.add(9, 12)
    s.add(12, 15)
    assert s.total_hours() == 6


def test_merge_joins_touching_blocks():
    assert merge_busy([Interval(9, 12), Interval(12, 15)]) == [Interval(9, 15)]


def test_merge_joins_overlapping_blocks():
    assert merge_busy([Interval(9, 13), Interval(11, 15)]) == [Interval(9, 15)]


def test_merge_keeps_blocks_with_a_gap_separate():
    assert merge_busy([Interval(9, 12), Interval(13, 15)]) == [Interval(9, 12), Interval(13, 15)]


def test_merge_of_nothing_is_nothing():
    assert merge_busy([]) == []


def test_interval_must_have_positive_length():
    with pytest.raises(ValueError):
        Interval(5, 5)
