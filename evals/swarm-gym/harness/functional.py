"""Does the produced app ACTUALLY WORK? Deterministic, no brain, no key, no spec knowledge required.

This is the metric the swarm is being tuned against: a run is only a success if the artifact functions.
Task status, `complete_result{passed:true}` and any model self-report are explicitly NOT evidence — the
measured false-green rate on those is high enough that they are worse than no signal at all.

MEASURED (h1-treat-2): the engine reported passed+verified on an app whose EVERY command crashed
(`error: 'str' object has no attribute 'mkdir'`). Nothing in the engine caught it because no deterministic
gate ever ran a command — `--help` works fine on an app where nothing else does.

Deliberately spec-agnostic. It cannot know the right OUTPUT for a command (that needs the golden values in
the spec), so it does not try. It answers the weaker question that is still decisive in practice: does the
program run, and do the commands it advertises about itself execute without crashing?
"""

from __future__ import annotations

import os
import re
import subprocess
from typing import Dict, List, Optional

# Python/runtime exception text surfaced to a user. An honest USAGE error is not in this list on purpose:
# `app set` with no arguments SHOULD fail, and counting that as a defect would punish correct apps.
RUNTIME_MARKERS = (
    "traceback (most recent call last)",
    "object has no attribute",
    "unsupported operand type",
    "is not subscriptable",
    "not callable",
    "nonetype",
    "keyerror",
    "indexerror",
    "typeerror",
    "attributeerror",
    "valueerror: invalid literal",
    "no module named",
)
USAGE_MARKERS = (
    "the following arguments are required",
    "invalid choice",
    "unrecognized arguments",
    "error: argument",
    "usage:",
)


def _classify(out: str, err: str, code: Optional[int]) -> Optional[str]:
    low = f"{out}\n{err}".lower()
    if any(m in low for m in RUNTIME_MARKERS):
        return "runtime-error"
    if code == 0 and not any(u in low for u in USAGE_MARKERS):
        if any(l.strip().startswith(("error:", "fatal:")) for l in low.splitlines()):
            return "error-but-exit-0"
    return None


def entry_package(root: str) -> Optional[str]:
    """The runnable package, found the way the smoke gate finds it: a dir holding __main__.py."""
    try:
        names = sorted(os.listdir(root))
    except OSError:
        return None
    for d in names:
        if os.path.isdir(os.path.join(root, d)) and os.path.exists(
            os.path.join(root, d, "__main__.py")
        ):
            return d
    return None


def assess(root: str, timeout: int = 25) -> Dict[str, object]:
    pkg = entry_package(root)
    if not pkg:
        return {"root": root, "verdict": "no-entry", "measurable": False}

    def run(args: List[str]):
        try:
            p = subprocess.run(
                ["python3", "-m", pkg] + args,
                cwd=root, capture_output=True, text=True, timeout=timeout,
            )
            return p.stdout, p.stderr, p.returncode
        except subprocess.TimeoutExpired:
            return "", "TIMEOUT", None
        except OSError as exc:
            return "", f"spawn error: {exc}", None

    out, err, code = run(["--help"])
    if code != 0:
        return {"root": root, "entry": pkg, "verdict": "help-fails",
                "detail": (out + err).strip()[:160], "measurable": True,
                "commands": 0, "broken": 0}

    # argparse advertises its subcommands in the usage line as {a,b,c}.
    m = re.search(r"\{([^}]*)\}", out + err)
    subs: List[str] = []
    if m and "," in m.group(1) and " " not in m.group(1):
        subs = [x.strip() for x in m.group(1).split(",") if x.strip()]
    if not subs:
        # Runnable and answers --help, but exposes no discoverable command surface to probe.
        return {"root": root, "entry": pkg, "verdict": "unprobeable",
                "measurable": True, "commands": 0, "broken": 0}

    broken = []
    for sub in subs[:12]:
        o, e, c = run([sub])
        why = _classify(o, e, c)
        if why:
            first = (o + e).strip().splitlines()
            broken.append({"command": sub, "why": why,
                           "detail": first[0][:110] if first else ""})
    return {
        "root": root, "entry": pkg,
        "verdict": "functional" if not broken else "broken",
        "commands": len(subs), "broken": len(broken), "failures": broken,
        "measurable": True,
    }


def summarise(results: List[Dict[str, object]]) -> Dict[str, object]:
    """Corpus-level rate. `unprobeable` is counted SEPARATELY, never as a pass — an app we could not
    probe is not an app we showed to work, and folding it into the numerator is how a metric starts
    flattering itself."""
    probed = [r for r in results if r.get("verdict") in ("functional", "broken")]
    return {
        "apps": len(results),
        "probed": len(probed),
        "functional": sum(1 for r in probed if r["verdict"] == "functional"),
        "broken": sum(1 for r in probed if r["verdict"] == "broken"),
        "unprobeable": sum(1 for r in results if r.get("verdict") == "unprobeable"),
        "help_fails": sum(1 for r in results if r.get("verdict") == "help-fails"),
        "no_entry": sum(1 for r in results if r.get("verdict") == "no-entry"),
        "functional_rate": (
            round(sum(1 for r in probed if r["verdict"] == "functional") / len(probed), 3)
            if probed else None
        ),
    }
