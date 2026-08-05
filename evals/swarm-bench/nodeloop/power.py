#!/usr/bin/env python3
"""CAN GOAL ONE BE ANSWERED AT ALL? The effect size a 5-pair sign test would need. Exit 0.

L142: check whether the design can reach the bar BEFORE spending the night on it. Four n3 cells are
collected and the n1 arm has not started, so this is the last moment where the answer is still cheap.

WHAT THE PROTOCOL REQUIRES. `PREREGISTERED.md` fixes a one-sided SIGN TEST over matched pairs, and
F260 fixes the arithmetic: with n pairs the smallest attainable p is 0.5**n, so n=5 can only clear
0.05 by going **5 for 5**. There is no partial credit — four pairs favouring 3 nodes gives 6/32 =
0.1875 and fails. So the question "can this work?" reduces to a single quantity:

    q = P(a random 3-node cell outscores a random 1-node cell)     P(5 of 5) = q**5

WHY THIS IS NOT PESSIMISM. It is the same reasoning that retracted last session's headline: three
runs of an IDENTICAL config scored 44.2 / 86.7 / 90.0%, a 46-point spread, so every n=1 conclusion
was uninterpretable. The replicate spread is a property of the fleet, it is measurable from the cells
already on disk, and it sets the bar the node effect has to clear. Reporting that bar is a result
whichever side of it the truth lands on.

⚠ THIS MEASURES THE n3 SPREAD ONLY. The n1 arm has not run, so its spread is ASSUMED equal — stated
as an assumption everywhere it is used, never buried. If n1 turns out noisier the bar rises; if the
1-node arm is simply much worse, the bar is met easily and this file will have been a cheap check.

Usage:
    python3 power.py              the bar, from the cells currently on disk
    python3 power.py --self-test  controls in both directions
"""
from __future__ import annotations

import statistics
import sys
from math import comb, erf, sqrt

import sweep

MIN_REPS = 5                  # the sweep's target; the curve's n
ALPHA = 0.05


def phi(z: float) -> float:
    """Standard normal CDF."""
    return 0.5 * (1 + erf(z / sqrt(2)))


def phi_inv(p: float) -> float:
    """Inverse normal CDF by bisection — no scipy in this venv, and precision here is irrelevant."""
    lo, hi = -8.0, 8.0
    for _ in range(200):
        mid = (lo + hi) / 2
        if phi(mid) < p:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def p_all_favour(q: float, n: int) -> float:
    """P(all n pairs favour 3 nodes | each does with probability q)."""
    return q ** n


def min_q_for_power(n: int, power: float) -> float:
    """The per-pair win rate needed for an n-pair sign test to reach `power` chance of n-for-n."""
    return power ** (1.0 / n)


def gap_for_q(q: float, sd: float) -> float:
    """Mean difference implying win-rate q, for two independent draws of equal spread.

    P(X > Y) = Phi((mu_x - mu_y) / (sd * sqrt(2))) for independent X, Y of the same sd.
    """
    return phi_inv(q) * sd * sqrt(2)


def max_losses(n: int) -> int:
    """How many pairs may favour 1 node and still leave p < 0.05. Negative means n is too small."""
    for losses in range(n, -1, -1):
        wins = n - losses
        p = sum(comb(n, k) for k in range(wins, n + 1)) / 2 ** n
        if p < ALPHA:
            return losses
    return -1


def power_at(n: int, q: float) -> float:
    """P(the sign test clears ALPHA | each pair favours 3 nodes with probability q)."""
    allowed = max_losses(n)
    if allowed < 0:
        return 0.0
    return sum(comb(n, k) * q ** (n - k) * (1 - q) ** k for k in range(0, allowed + 1))


def q_for_power(n: int, power: float) -> float:
    """The per-pair win rate needed for an n-pair sign test to reach `power`. Bisection."""
    if max_losses(n) < 0:
        return 1.0
    lo, hi = 0.5, 1.0
    for _ in range(200):
        mid = (lo + hi) / 2
        if power_at(n, mid) < power:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def n3_cells() -> list[dict]:
    for r in sweep.read_results():
        if r.get("arm") == "baseline" and r.get("nodes") == 3 and sweep.is_real_unit(r) \
                and r.get("score") is not None:
            yield r


def report() -> int:
    cells = list(n3_cells())
    scores = [c["score"] for c in cells]
    walls = [c["wall_secs"] for c in cells if c.get("wall_secs")]
    print(f"cells read: {len(cells)}   (3-node baseline, real, scored)")
    for c in sorted(cells, key=lambda c: c.get("rep", -1)):
        print(f"    r{c.get('rep')}  score {c['score']:.4f}   wall {c.get('wall_secs', 0):.0f}s")
    if len(scores) < 2:
        print("  fewer than 2 scored cells — no spread to measure yet")
        return 0

    s_mean, s_sd = statistics.mean(scores), statistics.stdev(scores)
    print(f"\n  SCORE  mean {s_mean:.4f}   sd {s_sd:.4f}   range {min(scores):.4f}-{max(scores):.4f}"
          f"   ({(max(scores)-min(scores))/s_mean:.0%} of the mean)")
    if len(walls) >= 2:
        w_mean, w_sd = statistics.mean(walls), statistics.stdev(walls)
        print(f"  WALL   mean {w_mean:.0f}s  sd {w_sd:.0f}s  range {min(walls):.0f}-{max(walls):.0f}"
              f"   ({(max(walls)-min(walls))/w_mean:.0%} of the mean)")

    print(f"\n  THE SIGN TEST'S ARITHMETIC (F260): with {MIN_REPS} pairs, p = 0.5**{MIN_REPS} = "
          f"{0.5 ** MIN_REPS:.4f} ONLY at {MIN_REPS}-for-{MIN_REPS}.")
    print(f"    {MIN_REPS - 1} of {MIN_REPS} gives p = "
          f"{sum(comb(MIN_REPS, k) for k in range(MIN_REPS - 1, MIN_REPS + 1)) / 2 ** MIN_REPS:.4f}"
          f" and FAILS. There is no partial credit.")

    print(f"\n  THE BAR, assuming the 1-node arm's spread matches the 3-node arm's (sd {s_sd:.4f}):")
    print(f"    {'chance of a clean sweep':>24s} {'per-pair win rate q':>20s} {'score gap needed':>17s}")
    for power in (0.50, 0.80, 0.95):
        q = min_q_for_power(MIN_REPS, power)
        gap = gap_for_q(q, s_sd)
        print(f"    {power:>23.0%} {q:>20.3f} {gap:>17.4f}"
              f"   ({gap / s_mean:>4.0%} of the 3-node mean)")

    # THE DESIGN CHOICE. `MIN_REPS` is mine to set, and the sign test's tolerance is NOT linear in n:
    # below n=8 a single crossing kills the result outright, at n=8 the test survives one. That
    # discontinuity moves the required effect size more than another rep of the same design does.
    print(f"\n  HOW THE BAR MOVES WITH THE NUMBER OF PAIRS (score sd {s_sd:.4f}):")
    print(f"    {'pairs':>5s} {'min p':>7s} {'losses OK':>10s} {'q for 50% power':>16s} {'gap needed':>11s}")
    for n in range(4, 13):
        allowed = max_losses(n)
        q = q_for_power(n, 0.50)
        gap = gap_for_q(q, s_sd)
        note = "  <- first n that survives a crossing" if allowed == 1 and max_losses(n - 1) == 0 else ""
        print(f"    {n:>5} {0.5 ** n:>7.4f} {allowed:>10} {q:>16.3f} {gap:>11.4f}{note}")

    print("\n  READ IT BOTH WAYS, because it cuts both:")
    q50 = min_q_for_power(MIN_REPS, 0.50)
    print(f"    · a coin-flip chance of even REACHING significance needs the 3-node arm to win "
          f"{q50:.0%} of pairs")
    print(f"    · that is a score gap of {gap_for_q(q50, s_sd):.3f} — the 1-node arm scoring "
          f"about {s_mean - gap_for_q(q50, s_sd):.3f} against the 3-node {s_mean:.3f}")
    print("    · IF the 1-node arm is simply much worse than that, the bar is met easily and this")
    print("      file cost nothing. IF the arms are close, five pairs CANNOT settle it and the")
    print("      honest report is the bar, not a null (L133).")
    print(f"\n  ⚠ n = {len(scores)} cells. An sd from {len(scores)} points is itself very uncertain;")
    print("    this sizes the question, it does not answer it. The n1 spread is ASSUMED, not measured.")
    return 0


def self_test() -> int:
    """A power calculation that cannot say 'hopeless' is not a power calculation."""
    assert abs(phi(0.0) - 0.5) < 1e-9 and phi(-8) < 1e-9 and phi(8) > 1 - 1e-9
    assert abs(phi_inv(0.5)) < 1e-6, "the median must invert to 0"
    assert abs(phi(phi_inv(0.87)) - 0.87) < 1e-6, "phi_inv must actually invert phi"

    # F260's arithmetic, asserted rather than trusted: 5-for-5 clears, 4-of-5 does not.
    assert abs(p_all_favour(0.5, 5) - 0.03125) < 1e-12
    assert sum(comb(5, k) for k in range(4, 6)) / 32 > ALPHA, "4 of 5 must FAIL — no partial credit"

    # Monotonic in the obvious directions, or the table below it is meaningless.
    assert min_q_for_power(5, 0.95) > min_q_for_power(5, 0.50), "more power needs a higher win rate"
    assert gap_for_q(0.87, 0.10) > gap_for_q(0.87, 0.05), "a noisier arm needs a BIGGER gap"
    assert gap_for_q(0.5, 0.1) == 0.0 or abs(gap_for_q(0.5, 0.1)) < 1e-6, \
        "a 50% win rate must need NO gap — the null must cost nothing"
    # ...and it must be able to report a HOPELESS bar, not just a reachable one.
    assert gap_for_q(min_q_for_power(5, 0.95), 0.5) > 1.0, \
        "with a huge spread the required gap must exceed the whole 0-1 score range"
    # The tolerance table is the actionable half, so its edges are asserted, not eyeballed.
    assert max_losses(5) == 0, "at 5 pairs a single crossing must kill the result"
    assert max_losses(7) == 0, "at 7 pairs 6-of-7 = 0.0625 still FAILS"
    assert max_losses(8) == 1, "8 is the first n where the sign test survives one crossing"
    assert max_losses(4) == 0 and 0.5 ** 4 > ALPHA or max_losses(4) == -1, \
        "at 4 pairs even a clean sweep gives 0.0625 — the test cannot pass at all"
    assert power_at(5, 1.0) == 1.0 and power_at(5, 0.5) == 0.5 ** 5, "power must bracket correctly"
    assert q_for_power(8, 0.50) < q_for_power(5, 0.50), \
        "surviving a crossing must LOWER the per-pair win rate needed — that is the whole point"
    assert power_at(4, 0.99) == 0.0, "an n that cannot reach ALPHA must score ZERO power, not a high one"

    print("self-test OK — phi inverts, 4-of-5 fails, a noisier arm raises the bar, hopeless is sayable")
    return 0


def main(argv: list[str]) -> int:
    return self_test() if "--self-test" in argv else report()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
