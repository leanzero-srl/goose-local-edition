from slugkit import slugify


def test_joins_words_with_single_hyphen():
    assert slugify("Hello World") == "hello-world"


def test_collapses_punctuation_and_runs():
    assert slugify("Hello,   World!!") == "hello-world"


def test_strips_leading_and_trailing_separators():
    assert slugify("  --Hello--  ") == "hello"


def test_truncates_mid_word():
    assert slugify("Hello World Foo", 13) == "hello-world-f"


def test_max_len_never_ends_with_hyphen():
    assert slugify("Hello World Foo", 12) == "hello-world"


def test_rejects_non_positive_max_len():
    import pytest

    with pytest.raises(ValueError):
        slugify("anything", 0)
