#!/usr/bin/env bash
# THE GREEN GATE — every check the project's own rules require, before a release build.
#
# WHY THIS IS A SCRIPT AND NOT A LINE IN A CHECKLIST. Twice on 2026-08-09 an engine edit reached the
# brink of a rebuild having passed only `cargo check`:
#
#   F714  cargo fmt --check FAILED after four engine edits. AGENTS.md: "Never: Skip cargo fmt".
#         cargo check says nothing whatsoever about formatting.
#   F718  an edit landed BETWEEN an existing #[test] attribute and its function. The orphaned
#         attribute duplicated onto the new test and the original became dead code that NO LONGER
#         RAN. Only clippy saw it (duplicate_macro_attributes + dead_code).
#
# Both were "the required check was not in the procedure", so the procedure is now executable.
#
# ⚠️ THE TEST TOTAL IS NOT A SAFETY NET. F718 measured 473 passing in BOTH the broken and the fixed
# state: the duplicate attribute registered one test twice while disabling another, so a lost test
# and an added test CANCEL EXACTLY. A count that matches expectation is not evidence that nothing was
# lost. That is why clippy is a HARD FAILURE here and not advisory — it is the only check that saw it.
set -uo pipefail

REPO=/Users/mihaiperdum/Projects/goose
cd "$REPO" || exit 2
# shellcheck disable=SC1091
source bin/activate-hermit >/dev/null 2>&1

CRATES=(-p goose-cli -p goose-swarm)
fail=0
note() { printf '%s %s\n' "$1" "$2"; }

echo "=== GREEN GATE ==="

if cargo fmt "${CRATES[@]}" --check >/tmp/gg_fmt 2>&1; then
  note "✅" "fmt      clean"
else
  note "🔴" "fmt      DIFFS PRESENT — AGENTS.md forbids skipping cargo fmt (F714)"
  head -20 /tmp/gg_fmt
  fail=1
fi

# --all-targets so TEST code is linted too: the F718 defect lived in a #[cfg(test)] block and is
# invisible without it.
cargo clippy "${CRATES[@]}" --all-targets >/tmp/gg_clippy 2>&1
clippy_n=$(grep -cE '^(warning|error)' /tmp/gg_clippy | head -1 | tr -dc '0-9')
clippy_n=${clippy_n:-0}
if [ "$clippy_n" -eq 0 ]; then
  note "✅" "clippy   0 warnings"
else
  note "🔴" "clippy   $clippy_n warning(s)/error(s) — THE ONLY CHECK THAT CAUGHT F718"
  grep -E '^(warning|error)' -A 6 /tmp/gg_clippy | head -30
  fail=1
fi

# F787b FULLY closed: the feature-unstable snapshot, the 4 CryptoProvider jwt tests, and the
# acp fork-order expectations (F803 — the cache-safe assembly tail-appends turn-context; the
# tests encoded upstream's order) are all fixed, so the ENTIRE core crate gates every hold.
if cargo test "${CRATES[@]}" >/tmp/gg_test 2>&1 && cargo test -p goose >>/tmp/gg_test 2>&1; then
  note "✅" "tests    $(grep -hoE '[0-9]+ passed' /tmp/gg_test | awk '{s+=$1} END{print s}') passed across $(grep -c 'test result: ok' /tmp/gg_test) suites"
else
  note "🔴" "tests    FAILED"
  grep -E '^(test result: FAILED|---- .* stdout|failures:)' -A 4 /tmp/gg_test | head -30
  fail=1
fi

# NOT a pass/fail line — a REMINDER that the number above cannot be read as coverage.
echo "   ⚠ the total above is NOT evidence no test was lost (F718: 473 both broken and fixed)."
echo "     After adding tests, confirm the neighbours still RUN by name, not by total."

if [ "$fail" -ne 0 ]; then
  echo "🔴 GREEN GATE FAILED — do NOT build a release binary on this tree."
  exit 1
fi
echo "✅ GREEN GATE PASSED — safe to cargo build --release."
