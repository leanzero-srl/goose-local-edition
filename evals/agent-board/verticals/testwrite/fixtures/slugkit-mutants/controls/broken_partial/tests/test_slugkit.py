from slugkit import slugify


def test_joins_words_with_single_hyphens():
    assert slugify("Hello World") == "hello-world"


def test_collapses_runs_of_punctuation():
    assert slugify("Hello,   World!!") == "hello-world"
