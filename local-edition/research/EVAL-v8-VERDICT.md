# EVAL-v8 — VERDICT

The question this eval set out to answer: **can the v8 swarm stack make a weak local-model fleet
(3× qwopus3.6-27b-coder-mtp on LM Studio) deliver genuinely working multi-module apps** — closing the
4 "draw" failure classes the controlled A/B (`AB-CONTROLLED.md`) isolated: lone-node STALL, cross-module
CONTRACT DRIFT hidden by isolation-only tests, BUILT-BUT-UNWIRED entry points, and NO real end-to-end run.

## Headline

- **Greenfield: the v8 stack WORKS.** The hard multi-module max-detail apps (archetype A2) went **3-for-3 WINS**
  (ledger 8/6/8/8, log-DSL 8/8/8/8, state-machine 8/6/8/8), each VERIFIED by running the app, AST-review-clean,
  0 stub pollution — across three distinct draw classes (contract-drift cascade / no-dispatcher stall /
  state-graph). vs the same multi-module regime WITHOUT contracts: A1-2 FAIL 2/0/5/2 + A1-3 broken 3/5/5/4.
  The build does what it set out to: a weak 27B fleet ships working multi-module apps.
- **Amendments (feature-add to an existing app) are a SEPARATE, HARDER regime** — and where v8 was weakest.
  They surfaced two real failure modes, each then FIXED:
  - A3-1 (chaos-fern --svg): the architect RE-ARCHITECTED (new parallel modules, abandoned originals) -> the
    AST reviewer caught it; fix = strengthened architect amendment rule (commit f9e89b782).
  - A3-2 (byte-oracle --json): WRONG-PATH write (the worker wrote `cli.py` to the repo ROOT instead of
    `byte_oracle/cli.py`) -> --json dead while 135 tests + smoke passed FALSELY. Same f9e89b782 "EXACT existing
    path" rule targets it.
  - A3-3 (byte-oracle --count, on the fixed binary, NO per-run instruction): WIN 7/6/7/7 — edited the REAL
    cli.py in place, --count works via the real entry, no stray. **The fix VALIDATED: A3 trend 5/3/4/6 ->
    4/5/4/5 -> 7/6/7/7 as f9e89b782 landed.**

## Per-archetype results (scores: correctness / test-depth / quality / spec, 1-10, vs AB qwopus mean 5.8/5.6/7.6/5.6)

| Run | Archetype | App | Result | Score |
|-----|-----------|-----|--------|-------|
| A1-1 | hard app, 1-line spec | markdown->HTML CLI | WIN | 9/9/9/9 |
| A1-2 | hard app, 1-line spec | spreadsheet w/ formulas | FAIL (no contracts; detailer drift cascade, fixed) | 2/0/5/2 |
| A1-3 | hard app, 1-line spec | task scheduler | broken (no contracts; store unwired) | 3/5/5/4 |
| A2-1 | hard, max-detail | double-entry ledger | WIN | 8/6/8/8 |
| A2-2 | hard, max-detail | log-pipeline DSL (logfunnel class) | WIN | 8/8/8/8 |
| A2-3 | hard, max-detail | state-machine engine | WIN | 8/6/8/8 |
| A3-1 | amendment | chaos-fern: add --svg | PARTIAL/CUT (re-architected) | 5/3/4/6 |
| A3-2 | amendment | byte-oracle: add --json | PARTIAL/FAIL (wrong-path) | 4/5/4/5 |
| A3-3 | amendment | byte-oracle: add --count | WIN (f9e89b782 validated) | 7/6/7/7 |

## Which v8 features earned their place

- **CONTRACTS** (per-module signature stubs injected into worker prompts) — the difference between A2's wins and
  A1-2/A1-3's failures on multi-module apps. Prevents the cross-module contract drift the isolation tests miss.
- **SMOKE gate** (`pytest --collect-only` + `python3 -m <pkg> --help`) — catches the "verbal PASS, never ran"
  class; a deterministic end-to-end oracle.
- **AST REVIEWER** (model-free unwired-module detector) — caught real defects that running + tests + human
  review all missed: A1-3's unwired store, A3-1's abandoned originals, A3-2's stray root cli.py. The single
  highest-signal gate for the BUILT-BUT-UNWIRED class.
- **PRE-REVIEWER** (idle-node correctness check) — caught a live JSON-key spec drift mid-run on A3-2.
- **DETAILER owned-files fix + DONE_GATE + fan-out cap + amendment EXACT-path rule (f9e89b782)** — each closed a
  specific real failure observed in a run.

## What still fails / honest caveats

- **Amendments remain harder than greenfield.** f9e89b782 fixed the two observed failure modes, but the regime is
  intrinsically trickier (the model must respect an existing layout, not invent one). One more open item: the
  WIRE-FIX mis-applies on amendments — it tries to wire PRE-EXISTING intentional dead modules (byte-oracle's
  `detector.py`), flailing. Candidate fix: wire-fix only on modules newly-unwired THIS run (before/after diff).
- **FALSE-GREEN is real:** A3-2 passed 135 tests + the smoke gate while the feature was dead (tests imported the
  stray file; smoke only checks `--help`). Only RUNNING the actual feature exposed it. VERIFY-don't-trust held
  up across the whole eval — every "WIN" here was confirmed by running the app, not by a green suite.
- **A2 used max-detail specs while A1 used 1-line specs**, so spec detail also contributes to A2's wins; the
  cleanest isolation would be a contracts-OFF same-spec A2 run. But the failure MECHANISMS that sank A1-2/A1-3
  (drift, unwiring, stub pollution) were each observed PREVENTED on A2.
- **Playwright is environment-blocked in this sandbox** (browser launches but file://, localhost-HTTP and
  data: content all time out), so the A3-1 SVG was verified structurally + algorithmically, not by pixel.

## The pivot now underway: TECHNOLOGY-AGNOSTIC

The entire matrix above is Python. The swarm ENGINE (scheduler/dispatch/judge) was always language-agnostic, but
the architect prompt + every gate were Python-hardcoded. Per the user's direction ("it should not care about
python, allow multiple technologies"), the de-Python work has started: STEP 1 (6881ae6d9) makes the architect
language-aware via a `TargetLang` profile (Python/TypeScript/Rust/Go/Other) + `detect_language`, Python
byte-identical. Remaining: the worker prompt, integrate-verify, and the gates (smoke/contracts/done_gate
language-aware or skip-clean; the TypeScript AST import-graph reviewer is deferred as lowest-confidence).
Validation: language-named experiments (TS-1 = a TypeScript todo CLI) now exercise the agnostic path end-to-end
on this host (node v22 + cargo present).

## Bottom line

On a weak 27B local fleet, the v8 stack converts the multi-module DRAW class into working apps on greenfield
(A2 3/3), and — after the amendment fixes — on feature-adds too (A3-3). The deterministic gates (smoke, AST
reviewer, pre-review) repeatedly caught defects that tests and human review missed. The work is now generalizing
from Python to any technology.
