from decimal import Decimal

import pytest

from ledgerfold import allocate, from_minor, invoice_total, split_evenly, to_minor


def test_converts_a_plain_amount():
    assert to_minor("1.23") == 123


def test_ties_round_away_from_zero():
    assert to_minor("0.125") == 13
    assert to_minor("-0.125") == -13


def test_large_amounts_stay_exact():
    assert to_minor("99999999999999.99") == 9999999999999999


def test_round_trips_through_minor_units():
    assert from_minor(to_minor("40.50")) == Decimal("40.50")


def test_split_sums_back_to_the_total():
    for total in (100, 101, 7, 0, -101):
        assert sum(split_evenly(total, 3)) == total


def test_split_hands_the_remainder_to_the_earliest_parts():
    assert split_evenly(101, 3) == [34, 34, 33]


def test_split_rejects_a_nonsense_division():
    with pytest.raises(ValueError):
        split_evenly(100, 0)


def test_invoice_totals_its_lines():
    lines = [{"amount": "10.00"}, {"amount": "5.55"}]
    assert invoice_total(lines) == 1555


def test_allocation_never_loses_a_cent():
    lines = [{"amount": "10.00"}, {"amount": "0.01"}]
    assert sum(allocate(lines, 3)) == 1001
