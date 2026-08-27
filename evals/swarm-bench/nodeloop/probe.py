#!/usr/bin/env python3
"""Does the release binary actually CONTAIN the engine changes I think I shipped? (BATCH.md §4 step 4)

WHY A STRING PROBE AND NOT `cargo build` SUCCEEDING. `cargo test` and `cargo clippy` do NOT rebuild
`target/release/goose`, and an empirical behaviour check against a stale binary silently exercises
the OLD code. A build that prints "Finished" is not proof the running binary carries the edit — the
sweep may have been holding an older file, the build may have gone to a different profile, or the
edit may sit behind a feature gate.

WHY LITERALS AND NOT FUNCTION NAMES. `strings` cannot see private Rust fn names — three known-present
private fns probe as ZERO — so "fn is_test_path not found" would be a FALSE alarm. Emitted string
literals (event names, field keys) DO survive into the binary. That distinction is what makes a zero
here a PROVEN negative rather than an observed one.

THE CONTROL THAT MAKES THIS WORTH RUNNING. A probe that only ever runs against the NEW binary cannot
tell "the literal is present" from "my grep matches anything". So this is designed to run TWICE:

    BEFORE the rebuild  -> NEW literals must be ABSENT, old literals PRESENT   (negative arm)
    AFTER  the rebuild  -> NEW literals must be PRESENT, old literals PRESENT  (positive arm)

Running only the second half is the same half-a-control mistake that has cost this campaign six
findings. The before-arm is UNRECOVERABLE once the rebuild lands, so it is taken first.

`--baseline` records the pre-rebuild reading to probe-baseline.json; `--verify` reads it back and
requires each NEW literal to have FLIPPED absent -> present. A literal that was already present
before the rebuild proves nothing about the rebuild and is reported as such.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GOOSE = Path("/Users/mihaiperdum/Projects/goose/target/release/goose")
BASELINE = HERE / "probe-baseline.json"

# Literals introduced by THIS batch. Each must flip absent -> present across the rebuild.
# PENDING literals — must flip absent -> present across the NEXT rebuild.
#
# ⚠️ A LITERAL GRADUATES OUT OF THIS SET THE MOMENT IT LANDS. Leaving a landed literal here makes
# every subsequent run report it "present before AND after — vacuous", which is true, useless, and
# trains the reader to skim past the one line that might say STILL ABSENT. Move it to
# POSITIVE_CONTROLS instead, where being present is exactly the job.
NEW = {
    "cannot be verified by anyone, including this gate": "F806 — the missing-counters shape finding (fail-open closed)",
}
# The 10:2x boundary's ONE change (hermetic scratch db per gate invocation) is LITERAL-LESS: it
# alters a filename format string whose distinctive part ("goose-spec-contract-") pre-exists.
# Per this probe's own docstring the honest move is to NOT run --verify and say so plainly
# (F767). The BEHAVIOURAL check, pre-registered: on the next unit, twin shadow verdicts must
# AGREE with the round gate that follows promotion — a twin verified 0 whose promoted tree
# re-fails the identical finding was the shared-db signature and must not recur.
# The five adopted upstream commits carry no probeable literal (8f1590b75's distinctive
# strings are all in #[cfg(test)] code — the F762 dead-code rule): their rebuild evidence is
# greengate's debug suite, which ran their own tests green at each pick.
# The 01:01 boundary's two chosen literals were BOTH unprobeable classes, found the hard way
# (probe read STILL ABSENT while the live-path key `subsplit` count=1 proved the build carried
# the edit — a false "rebuild did not carry" alarm from probe design, not from the build):
#   - "the shadow differs from its root outside its owned slots" sits in #[allow(dead_code)]
#     splice_functions: ZERO callers in release => the optimizer never codegens it, so NO
#     literal inside dead-until-wired code can ever evidence a rebuild. Its evidence is the
#     debug-build test suite (greengate runs the 5-way splice test).
#   - "SUBSPLIT:" is 9 bytes inside strip_prefix: short literals get constant-folded into
#     immediate comparisons and never reach .rodata. Prefer LONG live-path strings — json event
#     keys, format strings — and verify a candidate with bytes.count() BEFORE trusting absence.
# S3i2's landing IS verified: `subsplit` (the detail_completed event key, live path) reads
# count=1 in the 01:01 binary and the string existed nowhere before these commits.
# Literal-LESS in this batch, verified BEHAVIOURALLY on the first post-rebuild run: 4bb2b1ac7
# difficulty on task_dispatched (the bare words "difficulty"/"easy"/"hard" cannot attribute in a
# 236 MB binary — check = task_dispatched events carry the key, then A2 becomes a log lookup);
# a7a4e6ee7 S1i2 shard verify-before-promote (its new json keys "shard"/"promoted" are reused
# words — check = complete_fix_completed carrying BOTH shard and promoted on a sink_shard round,
# and complete_fix_wave reporting promoted counts).
# Literal-LESS changes in the same pending batch, each verified BEHAVIOURALLY on the first
# post-rebuild run instead (recorded so the gap is deliberate): A2 hard_device_key (private fn,
# invisible to strings; check = no hard task on a busy device while another idles), A3 idle-job
# spread/yield/notify (check = no review holding the last slot while ready work waits), B2 sink
# claim gate (check = no task_completed after sink completion on a replanned run), G1 derived
# grace (check = no straggler_aborted with round elapsed < 3x45s), K5 single-owned-file repair
# (check = the no-path write/edit error class vanishes for single-file workers), K7 tool_choice
# guard + F756 fix-cap 3000 (check = twins run past 1320s; agent_ok true appears).
# Deliberately NOT probed: `inconclusive_reasons` (C6 step 1). The baseline run found it ALREADY
# PRESENT — it has shipped on the `spec_contract` event since before this batch (swarm.rs:26503), and
# C6 deliberately REUSES the name on `complete_verify` (:26570) so both events read the same shape.
# Two different events, no duplicate key — checked, because a duplicate key inside one json! macro
# silently keeps only the last value. But a literal that was present before the rebuild can never
# evidence the rebuild, so probing it would be a vacuous pass dressed as a verification.
# C6 IS VERIFIED A DIFFERENT WAY: the first post-rebuild run's log must show `inconclusive_reasons`
# on a `complete_verify` event, not merely somewhere in the binary. Recorded here so the gap is
# deliberate and visible rather than silently dropped.

# Literals that exist in BOTH binaries. If one of these reads absent, the probe itself is broken
# (wrong path, wrong tool, stripped binary) and NO conclusion may be drawn from the NEW readings.
POSITIVE_CONTROLS = {
    # GRADUATED from NEW after the 2026-08-12 08:52 rebuild (watched flipping absent -> present):
    "skeleton written:": "S3 fill fan — landed 2026-08-12",
    "created file(s) from the winning twin": "F766 promote fix — landed 2026-08-12",
    # "subsplit" was here 2026-08-12 02:11-02:15 and is REMOVED: the binary's one occurrence
    # is the mangled SYMBOL extract_subsplit (which does prove compilation), invisible to this
    # probe's `strings` haystack — a control verified with a different instrument (bytes.count)
    # than the probe reads with is a guaranteed-blind control. THIRD PROBE RULE: graduate a
    # literal only after `strings <binary> | grep` — the probe's own instrument — finds it.
    # GRADUATED from NEW after the 2026-08-12 00:27 rebuild (watched flipping absent -> present):
    "GOOSE_SWARM_TESTGEN": "S7 — landed 2026-08-12",
    "tests/generated/test_gen_": "S7 — landed 2026-08-12",
    "no landable fenced test block": "S7 — landed 2026-08-12",
    # GRADUATED from NEW after the 2026-08-11 23:46 rebuild (watched flipping absent -> present):
    "complete-fix::cross-file": "S1i3 — landed 2026-08-11",
    # GRADUATED from NEW after the 2026-08-11 23:30 rebuild (watched flipping absent -> present):
    "must carry a UTC designator": "Q2b2 — landed 2026-08-11",
    # GRADUATED from NEW after the 2026-08-11 22:13 rebuild (watched flipping absent -> present):
    "GOOSE_SWARM_SINK_SHARD": "S1i1 — landed 2026-08-11",
    # GRADUATED from NEW after the 2026-08-11 17:49 rebuild (watched flipping absent -> present):
    "probed_post": "F751 — landed 2026-08-11",
    # GRADUATED after the 2026-08-11 20:36 rebuild (watched flipping):
    "CONTRACT UNAVAILABLE": "D1 — landed 2026-08-11",
    "NONE ON DISK YET": "D3 — landed 2026-08-11",
    "GOOSE_COMPACT_KEEP_TAIL": "K4 — landed 2026-08-11",
    "task_dispatched": "the most common event in any run log",
    # THE LINEAR-ENGINE CONTROLS. The eight literals that used to sit here — would_skip_ladder,
    # would_skip_ladder_prelift, plan_convergence, requested_best_of_n, distinct_draft_models,
    # straggler_deferred, "END your spec with ONE line", "The spec names these documents" — belonged
    # to the plan vote, the redraft ladder, the detail fan and the C7 prompt, all of which the
    # OPEN -> ASK -> RESEARCH -> SYNTHESIS -> REVIEW rewrite deleted. Several still grep in swarm.rs
    # inside code nothing calls any more, and that is exactly the trap this probe's own dead-code rule
    # names: unreachable code is never codegen'd, so the literal never reaches .rodata. Every one of
    # them reads ZERO in the 2026-08-27 15:18 binary, which would have tripped the BLIND gate on every
    # run and buried the one line that matters.
    # Each replacement below was verified with `strings target/release/goose | grep -F` — the probe's
    # OWN instrument, per the THIRD PROBE RULE — not by grepping the source, which is what would have
    # kept the dead ones.
    "slices_opened": "the OPEN phase always emits it — one per run, count=1 in the 15:18 binary",
    "clarify_proxy_armed": "the ASK phase's arming event",
    "review_findings": "REVIEW emits one per round, and REVIEW always runs at least once",
    "defects_rated": "the CRITICAL/MINOR rating that replaced all-or-nothing",
    "sink_id_pinned": "SYNTHESIS pins the sink id on every plan it wires",
    # Two prompt fragments, because event names alone would leave the probe blind to a rewrite that
    # kept the event schema and gutted the instructions. Both are long, live-path, and single-piece
    # (neither straddles a format! interpolation, which would split them across .rodata entries).
    "HOW MANY MACHINES EXIST IS IRRELEVANT": "the opener's fleet-independence rule",
    "Write the MODULE SPECIFICATION for your slice": "BRIEF_CONTRACT, spliced into every slice owner",
    # GRADUATED from NEW after the 2026-08-09 15:15 rebuild, where six were watched flipping absent
    # -> present; four of the six died with the ladder and the detail fan (see above) and only these
    # two survive. They earn their keep as controls: if either ever reads ABSENT the build is not the
    # one I think it is, and nothing below may be interpreted.
    "skipped_tests": "F711/C1 — landed 2026-08-09",
    "description_chars": "C12(b) — landed 2026-08-09",
    # GRADUATED 2026-08-10. All three read PRESENT in the 08:44 binary while still sitting in NEW —
    # the exact stale-pending state the NEW docstring warns about, caught only by reading the probe
    # instead of trusting the set. That reading also settled a live question: the `probe_post` ARM
    # exercised a gate that IS in the binary, so its 6 fix dispatches are a real mechanism readout.
    "GOOSE_SWARM_PROBE_ADVERTISED_POST": "F738 — the advertised-POST gate — landed 2026-08-10",
    "is not CHEAP on a repeat run": "F740 — the NotCheap finding — landed 2026-08-10",
    "it is the previous page's tag": "F740 — the per-request ETag remediation — landed 2026-08-10",
}

# Must never appear. Catches a grep that matches everything, or a probe run against the wrong file.
NEGATIVE_CONTROL = "goose_swarm_bogus_marker_that_must_not_exist"


def literals(path: Path) -> set[str]:
    out = subprocess.run(["strings", str(path)], capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"strings failed on {path}: {out.stderr.strip()}")
    return set(out.stdout.splitlines())


def present(hay: set[str], needle: str) -> bool:
    """Substring match — a literal is usually embedded in a larger constant, not on its own line."""
    return any(needle in line for line in hay)


def read(path: Path) -> dict:
    if not path.exists():
        sys.exit(f"binary missing: {path}")
    hay = literals(path)
    return {
        "mtime": os.path.getmtime(path),
        "size": os.path.getsize(path),
        "new": {k: present(hay, k) for k in NEW},
        "controls": {k: present(hay, k) for k in POSITIVE_CONTROLS},
        "negative": present(hay, NEGATIVE_CONTROL),
    }


def check_controls(r: dict) -> None:
    bad = [k for k, v in r["controls"].items() if not v]
    if bad:
        sys.exit(f"🔴 POSITIVE CONTROL FAILED: {bad} absent from the binary. The probe is BLIND — "
                 "no conclusion about the new literals is valid. Check the path/profile first.")
    if r["negative"]:
        sys.exit("🔴 NEGATIVE CONTROL FAILED: the bogus marker matched. The probe matches anything.")
    print(f"  controls: {len(r['controls'])} known-present literals found, bogus marker absent ✅")


def main() -> int:
    import time
    # AN EMPTY NEW SET PASSES BOTH ARMS VACUOUSLY. `--baseline` would report "all 0 literals absent"
    # and `--verify` would leave `ok` True having checked nothing, printing the same ✅ as a real
    # verification. That is `all([])` wearing a rebuild's clothes, and it is the one failure this
    # file cannot afford: its entire job is to make a rebuild's contents PROVABLE.
    if not NEW:
        sys.exit("🔴 NEW is empty — there is nothing to prove about this rebuild. Either register "
                 "the literal the edit introduces, or do not run the probe and say so plainly.")
    mode = "--verify" if "--verify" in sys.argv else "--baseline"
    r = read(GOOSE)
    stamp = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(r["mtime"]))
    print(f"binary {GOOSE}\n  built {stamp}  size {r['size']:,}")
    check_controls(r)

    if mode == "--baseline":
        already = [k for k, v in r["new"].items() if v]
        for k, v in r["new"].items():
            print(f"  {'PRESENT' if v else 'absent ':8s} {k:32s} {NEW[k]}")
        BASELINE.write_text(json.dumps(r, indent=2))
        if already:
            print(f"\n⚠️  {already} ALREADY PRESENT before the rebuild. These cannot serve as evidence "
                  "that the rebuild landed — the flip test is vacuous for them.")
        else:
            print(f"\n✅ BASELINE RECORDED: all {len(NEW)} new literals ABSENT pre-rebuild, so a "
                  "PRESENT reading after the rebuild is a real flip and not a pre-existing string.")
        return 0

    if not BASELINE.exists():
        sys.exit("🔴 no probe-baseline.json — the pre-rebuild arm was never taken. A PRESENT reading "
                 "now cannot be distinguished from a literal that was always there. Do NOT proceed.")
    before = json.loads(BASELINE.read_text())
    if before["mtime"] == r["mtime"]:
        sys.exit(f"🔴 BINARY NOT REBUILT — mtime unchanged ({stamp}). cargo said Finished but this "
                 "file is the same one the baseline read. Nothing was verified.")
    ok = True
    for k in NEW:
        was, now = before["new"][k], r["new"][k]
        if now and not was:
            print(f"  ✅ FLIPPED absent -> present   {k:32s} {NEW[k]}")
        elif now and was:
            print(f"  ⚠️  present before AND after   {k:32s} — vacuous, proves nothing")
        else:
            print(f"  🔴 STILL ABSENT               {k:32s} {NEW[k]}")
            ok = False
    print(f"\nbinary mtime moved {stamp} (was "
          f"{time.strftime('%H:%M:%S', time.localtime(before['mtime']))})")
    if not ok:
        print("🔴 THE REBUILD DID NOT CARRY THE EDIT. Do not start the sweep on this binary.")
        return 1
    print("✅ VERIFIED: every new literal flipped absent -> present, controls held.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
