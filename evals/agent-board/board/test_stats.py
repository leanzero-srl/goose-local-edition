"""Pin the maths without a fleet.

Every number the board prints comes through these functions, so they are tested against properties
rather than golden values — a property survives a formula change, a magic constant does not.
"""

from __future__ import annotations

import math

import pytest

import card
import drift


@pytest.mark.parametrize("n", [1, 3, 5, 10, 30, 100])
def test_wilson_interval_stays_inside_zero_and_one(n):
    """The reason Wilson is used at all: the normal approximation escapes [0,1] at small n."""
    for passes in range(n + 1):
        _, lo, hi = card.wilson(passes, n)
        assert 0.0 <= lo <= hi <= 1.0


@pytest.mark.parametrize("n", [1, 3, 5, 10, 30, 100])
def test_the_interval_contains_the_point_estimate(n):
    for passes in range(n + 1):
        p, lo, hi = card.wilson(passes, n)
        assert lo <= p <= hi


def test_intervals_narrow_as_evidence_accumulates():
    widths = [card.wilson(n // 2, n)[2] - card.wilson(n // 2, n)[1] for n in (4, 8, 16, 64, 256)]
    assert widths == sorted(widths, reverse=True)


def test_a_perfect_score_still_carries_uncertainty():
    """5/5 is not proof of 100%. An interval that collapses at the boundary is the lie this
    benchmark exists to avoid."""
    _, lo, hi = card.wilson(5, 5)
    assert lo < 1.0
    assert hi == pytest.approx(1.0)


def test_mde_shrinks_as_reps_grow():
    values = [drift.mde(0.5, n) for n in (1, 3, 5, 15, 50)]
    assert values == sorted(values, reverse=True)


def test_mde_at_a_pinned_rate_does_not_claim_infinite_resolution():
    """p=0 or p=1 has no observed variance. Reporting MDE 0 would claim the instrument can separate
    anything — from a sample that has simply never seen the other outcome."""
    assert drift.mde(0.0, 5) > 0
    assert drift.mde(1.0, 5) == pytest.approx(drift.mde(0.0, 5))
    assert drift.mde(1.0, 5) >= drift.mde(0.5, 5)


def test_mde_is_capped_at_a_hundred_points():
    assert drift.mde(0.5, 1) <= 100.0


def test_reps_needed_actually_reaches_the_band_it_promises():
    for target in (10.0, 15.0, 25.0):
        n = drift.reps_needed(0.5, target)
        assert drift.mde(0.5, n) <= target + 1e-9


def test_reps_needed_grows_as_the_band_tightens():
    assert drift.reps_needed(0.5, 5.0) > drift.reps_needed(0.5, 25.0)


def _row(label, passes, n, secs=10.0):
    return {"label": label, "score": 1.0, "wall_secs": secs, "rep": 0,
            "probe": {"tampered": False}, "provider": None, "crashed": False, "timed_out": False}


def test_overlapping_entrants_share_a_rank_and_are_never_ordered():
    episodes = []
    for i in range(5):
        episodes.append(dict(_row("a", 5, 5), score=1.0, rep=i))
        episodes.append(dict(_row("b", 4, 5), score=1.0 if i < 4 else 0.0, rep=i))
    rows = card.summarise(episodes)
    assert {r["label"]: r["rank"] for r in rows} == {"a": 1, "b": 1}


def test_a_clear_separation_does_produce_distinct_ranks():
    episodes = []
    for i in range(30):
        episodes.append(dict(_row("strong", 0, 0), score=1.0, rep=i))
        episodes.append(dict(_row("weak", 0, 0), score=0.0, rep=i))
    rows = card.summarise(episodes)
    ranks = {r["label"]: r["rank"] for r in rows}
    assert ranks["strong"] == 1
    assert ranks["weak"] > 1


def test_a_crashed_episode_counts_against_the_entrant():
    """Crashes stay in the denominator. Dropping them is dropping the finding."""
    episodes = [dict(_row("x", 0, 0), score=1.0, rep=0),
                dict(_row("x", 0, 0), score=0.0, rep=1, crashed=True)]
    rows = card.summarise(episodes)
    assert rows[0]["n"] == 2
    assert rows[0]["passes"] == 1
    assert rows[0]["crashed"] == 1


def test_wilson_of_an_empty_sample_is_maximally_uncertain():
    p, lo, hi = card.wilson(0, 0)
    assert (p, lo, hi) == (0.0, 0.0, 1.0)
    assert not math.isnan(p)
