#!/usr/bin/env python3
"""G7: is any language silently inheriting another's tooling? Exit 0 (1 if a gap is found).

Mihai filed G7 — "detect hard coded logic in the swarm and make it generic ... this agent will be used
to produce script, software, apps etc." The 366 `.py` literals in swarm.rs are the SYMPTOM. The
MECHANISM is the wildcard: every `_ =>` and every omitted `TargetLang` variant is how a new stack
inherits the first one's tooling without anyone deciding it should.

MEASURED, and this is the argument for auditing arms instead of literals: a scan of language match
blocks found TWO gaps in the whole file, and BOTH were real defects —
  * `verify_recipe` was `TypeScript | Rust | _`, so a GO app, smoke-tested with `go build`/`go test`,
    had its fix worker told to verify with `python3 -m pytest`   (F102)
  * `overview_run_command` was `Python | Rust | TypeScript | _ => None`, so a Go app resolved to NO
    run command at all and nothing could probe its entry point   (F103)

BRACE-MATCHED, NOT WINDOWED. The first version of this scan read a fixed 16 lines after `match lang {`
and reported `contract_stub_spec` as missing four variants — it has all five, spread over 80 lines. A
false positive here sends someone to "fix" correct code, which is the F101 lesson: an instrument's
verdict can be right while its reason is wrong, so read the source before acting.
"""
from __future__ import annotations
import pathlib, re, sys

ALL = {"Python", "TypeScript", "Rust", "Go", "Other"}
SRC = pathlib.Path(__file__).resolve().parents[3] / "crates/goose-cli/src/commands/swarm.rs"


def audit(path: pathlib.Path) -> list[dict]:
    lines = path.read_text(errors="replace").splitlines()
    out = []
    for i, l in enumerate(lines):
        if not re.search(r"match\s+[^{]*\blang\b[^{]*\{|match\s+[^{]*TargetLang", l):
            continue
        depth, body = 0, []
        for j in range(i, min(i + 400, len(lines))):
            body.append(lines[j])
            depth += lines[j].count("{") - lines[j].count("}")
            if depth <= 0 and j > i:
                break
        b = "\n".join(body)
        if "TargetLang::" not in b:
            continue
        arms = set(re.findall(r"TargetLang::(\w+)\s*=>", b))
        out.append({"line": i + 1, "arms": sorted(arms), "missing": sorted(ALL - arms),
                    "wildcard": bool(re.search(r"^\s*_\s*=>", b, re.M))})
    return out


def main() -> int:
    if not SRC.is_file():
        print(f"swarm.rs not found at {SRC}")
        return 0
    rows = audit(SRC)
    gaps = [r for r in rows if r["wildcard"] or r["missing"]]
    print(f"{len(rows)} language match blocks in swarm.rs (brace-matched)")
    for r in rows:
        flag = "WILDCARD" if r["wildcard"] else ("MISSING " + ",".join(r["missing"]) if r["missing"] else "exhaustive")
        print(f"  L{r['line']:<6} {flag}")
    if gaps:
        print(f"\n{len(gaps)} GAP(S) — a language here inherits another's tooling by default. "
              f"Give it its own arm; an exhaustive match makes the next language fail to COMPILE "
              f"until someone states its answer.")
        return 1
    print("\nno gaps: every language states its own answer, and adding a sixth will not compile "
          "until it does too")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
