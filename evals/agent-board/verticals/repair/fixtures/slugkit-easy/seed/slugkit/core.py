import re

_NON_ALNUM = re.compile(r"[^a-z0-9]+")


def slugify(text: str, max_len: int = 60) -> str:
    """Build a URL slug: lowercase, alphanumeric runs joined by single hyphens.

    A slug never starts or ends with a hyphen, and is never longer than max_len.
    """
    if max_len <= 0:
        raise ValueError("max_len must be positive")
    joined = _NON_ALNUM.sub("-", text.lower())
    trimmed = joined.strip("-")
    return trimmed[:max_len]
