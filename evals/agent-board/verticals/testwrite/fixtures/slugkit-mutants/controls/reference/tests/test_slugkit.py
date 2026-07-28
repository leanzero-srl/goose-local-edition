import pytest

from slugkit import slugify


def test_joins_words_with_single_hyphens():
    assert slugify("Hello World") == "hello-world"


def test_collapses_runs_of_punctuation():
    assert slugify("Hello,   World!!") == "hello-world"


def test_strips_leading_and_trailing_separators():
    assert slugify("  --Hello--  ") == "hello"


def test_truncates_to_max_len():
    assert len(slugify("Hello World Foo", 13)) <= 13
    assert slugify("Hello World Foo", 13) == "hello-world-f"


def test_truncation_never_leaves_a_trailing_hyphen():
    assert slugify("Hello World Foo", 12) == "hello-world"


def test_rejects_non_positive_max_len():
    with pytest.raises(ValueError):
        slugify("anything", 0)
