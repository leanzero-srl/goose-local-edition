from ledgerfold import invoice_total, split_evenly, to_minor


def test_converts_a_plain_amount():
    assert to_minor("1.23") == 123


def test_split_sums_back():
    assert sum(split_evenly(101, 3)) == 101


def test_invoice_totals_its_lines():
    assert invoice_total([{"amount": "10.00"}, {"amount": "5.55"}]) == 1555
