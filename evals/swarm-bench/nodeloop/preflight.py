#!/usr/bin/env python3
"""Will the next boundary PASS? Answer it from source, before killing the run to find out.

`./loop.sh boundary` verifies MARKERS against the rebuilt BINARY — which is the right check and the
wrong moment. By the time it runs, the supervisor is dead, the engine is dead, and the rebuild is
spent. A marker that turns out to be a comment (three times now: `failed_task_finding`,
`is_code_deliverable`, `THE SPEC STATES ITS ENDPOINTS`) refuses a perfectly correct binary and sends
me hunting a defect that does not exist.

Every one of those failures is decidable from SOURCE, with the fleet running and nothing at stake.
That is what this does.

Four verdicts, and the middle two are the whole point:

  LITERAL  — the marker appears inside a string literal. It will be in the binary.
  DERIVED  — it never appears in source, but a `#[serde(rename_all = "snake_case")]` enum has the
             CamelCase variant, so the derive macro generates the literal at COMPILE time. Real, and
             invisible to any source grep. See the warning below.
  COMMENT  — it appears ONLY on comment lines. `strings` reads the binary's data section; the
             compiler strips comments long before that. This marker WILL report ABSENT on a correct
             build. It is the exact failure this file exists to prevent.
  ABSENT   — not in crates/ at all. Either the fix was never written, or it was reverted, or the
             marker has a typo. All three are worth knowing before a rebuild rather than after.

⚠ THE FIRST RUN OF THIS SCRIPT WAS WRONG ABOUT TWO OF FORTY-ONE MARKERS, and getting it wrong in
this direction is expensive: it told me to delete `task_split` (ABSENT) and `speculated` (COMMENT),
both of which `strings target/release/goose` finds in the shipped binary. `event.rs` carries
`#[serde(tag = "event", rename_all = "snake_case")]`, so variants `TaskSplit` and `Speculated`
become those exact strings in the binary's data section without the text ever existing in a .rs
file. A source grep cannot see a compile-time derive — the same class of blindness as grepping
swarm.rs alone, one layer deeper. Hence DERIVED, and hence the binary cross-check below.

The binary is consulted as a POSITIVE signal ONLY. It is whatever was built last, so a marker for a
fix committed after that build is legitimately missing from it, and its absence proves nothing.
Presence, however, is proof the marker is findable — which is the only thing this script claims.

Deliberately searches ALL of `crates/`, not swarm.rs. MARKERS' own instructions said swarm.rs for
months, and `WHAT THE SUPERVISOR ALREADY FOUND` lives in goose-swarm/src/scheduler.rs — grepping one
file returns 0 for it, which reads identically to the COMMENT case and would have had me delete a
good marker to "fix" the check.

Usage:
    python3 preflight.py            # exit 0 if every marker will survive the rebuild, 1 otherwise
"""
from __future__ import annotations

import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
CRATES = HERE.parents[2] / "crates"
MARKERS = HERE / "MARKERS"

# A line contributes a LITERAL hit only if the marker sits outside `//`. Rust has no marker-bearing
# block comments in this tree, and treating `/*` as a comment start would misread a `*/` inside a
# string. Narrow on purpose: a false COMMENT verdict costs a real marker, so when in doubt this
# leans toward LITERAL and lets the binary check (which is authoritative) have the last word.
def classify_line(line: str, marker: str) -> str | None:
    if marker not in line:
        return None
    stripped = line.lstrip()
    if stripped.startswith("//"):
        return "comment"
    # `code(); // marker` — the marker is after the comment opener on a code line.
    idx, cidx = line.index(marker), line.find("//")
    if cidx != -1 and cidx < idx:
        return "comment"
    return "literal"


def camel(marker: str) -> str:
    return "".join(p[:1].upper() + p[1:] for p in marker.split("_"))


def derived_by_serde(marker: str, texts: dict) -> str | None:
    """A snake_case marker that a `rename_all = "snake_case"` enum generates from a CamelCase variant.

    Narrow by construction: the marker must BE snake_case, the file must carry the rename attribute,
    and the CamelCase form must appear as a variant head (start of line, followed by `{`, `(`, or
    `,`) rather than anywhere in the text. A bare mention in a doc comment does not qualify — that is
    how `Speculated` appears at scheduler.rs:1212, and crediting it there would make this check as
    blind as the one it replaces.
    """
    # Single-word variants rename too (`Speculated` -> `speculated`), and requiring an underscore
    # made this miss one of the two markers it was written for — it survived only because the stale
    # binary happened to carry it. A verdict must not depend on that.
    if not re.fullmatch(r"[a-z0-9]+(_[a-z0-9]+)*", marker):
        return None
    cc = camel(marker)
    for p, t in texts.items():
        if 'rename_all = "snake_case"' not in t:
            continue
        for i, line in enumerate(t.splitlines(), 1):
            if re.match(rf"\s*{re.escape(cc)}\s*[{{(,]", line):
                return f"{p}:{i} (variant {cc})"
    return None


def read_markers() -> list[str]:
    out = []
    for raw in MARKERS.read_text().splitlines():
        s = raw.strip()
        if s and not s.startswith("#"):
            out.append(s)
    return out


def main() -> int:
    if not MARKERS.is_file():
        print("no MARKERS file — nothing to pre-flight")
        return 0
    markers = read_markers()
    sources = [p for p in CRATES.rglob("*.rs") if "/target/" not in str(p)]
    texts = {p: p.read_text(errors="replace") for p in sources}
    rel = {p: str(p.relative_to(CRATES)) for p in sources}

    # POSITIVE-ONLY cross-check. See the module docstring: this binary predates any fix committed
    # since it was built, so absence here is not evidence and is never read as such.
    binary = CRATES.parent / "target" / "release" / "goose"
    in_binary: set[str] = set()
    if binary.is_file():
        import subprocess
        try:
            blob = subprocess.run(["strings", str(binary)], capture_output=True, text=True,
                                  timeout=180).stdout
            in_binary = {m for m in markers if m in blob}
        except Exception as exc:
            print(f"(binary cross-check unavailable: {exc})")

    print(f"=== BOUNDARY PRE-FLIGHT — {len(markers)} marker(s) against {len(sources)} .rs files ===")
    print("does the rebuild already carry every fix, and is every marker findable in the binary?\n")

    bad = []
    for m in markers:
        hits = {"literal": [], "comment": []}
        for p, t in texts.items():
            if m not in t:
                continue
            for i, line in enumerate(t.splitlines(), 1):
                k = classify_line(line, m)
                if k:
                    hits[k].append(f"{p.relative_to(CRATES)}:{i}")
        if hits["literal"]:
            where = hits["literal"][0]
            extra = f" (+{len(hits['literal']) - 1} more)" if len(hits["literal"]) > 1 else ""
            print(f"  LITERAL  {m[:44]:<46} {where}{extra}")
            continue
        # Only now consider a compile-time derive, so a real literal always wins the verdict.
        der = derived_by_serde(m, {rel[p]: t for p, t in texts.items()})
        if der:
            seen = " [confirmed in binary]" if m in in_binary else ""
            print(f"  DERIVED  {m[:44]:<46} {der}{seen}")
        elif m in in_binary:
            # No source form found, yet the shipped binary has it. Whatever generates it, it is
            # findable — which is all a marker has to be. Reporting this as a failure would be an
            # instrument overruling the artefact it is a proxy for.
            print(f"  LITERAL  {m[:44]:<46} (no source form; present in the built binary)")
        elif hits["comment"]:
            bad.append((m, "COMMENT", hits["comment"][0]))
            print(f"  COMMENT  {m[:44]:<46} {hits['comment'][0]}  <-- WILL REPORT ABSENT")
        else:
            bad.append((m, "ABSENT", "-"))
            print(f"  ABSENT   {m[:44]:<46} not in crates/ at all")

    print()
    if bad:
        print(f"{len(bad)} marker(s) would FAIL the boundary. Fix them NOW, with the fleet still up:")
        for m, why, where in bad:
            if why == "COMMENT":
                print(f"  · {m!r} is only a comment ({where}). Either point it at the real literal the "
                      f"fix introduced, or drop it and verify that fix with a unit test instead — "
                      f"asserting behaviour beats asserting presence (F62, F71).")
            else:
                print(f"  · {m!r} is nowhere in crates/. The fix is missing, reverted, or the marker "
                      f"is a typo. Do NOT rebuild until you know which.")
        return 1
    print("every marker is a real string literal — the boundary will not refuse the rebuild for a "
          "phantom. That is the cheap half; it does not promise the fixes WORK.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
