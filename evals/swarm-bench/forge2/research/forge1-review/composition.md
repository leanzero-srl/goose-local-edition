# Forge 1.0 board — whole-score composition audit (read-only, 2026-10-09)

Scope: composition (inner, criticals, ROOT_BLOCKS, bands, excellence), the 7 zeros, cross-model identical partials,
harness-version effects, and the two "looks wrong" rankings. No scorer, browser, pytest or cargo was run. All
counterfactuals are pure-python `score_forge.compose_from_rows` over the board's stored rows
(scripts: `scratchpad/forge-review/py/{recompose,cf_comment,cf_range,cf_all}.py`).

## Baseline facts (checked, sound)

- Installed scorer == repo: `score_forge.py`, `forge_probe.mjs`, `forge_site.py`, `forge_oracle.py`,
  `forge-thresholds.json` byte-identical (`cmp`).
- **Composition reproduces 31/31 published scores exactly** from the stored `checksSummary` rows (vacuous / absent
  parts rebuilt from the detail text). So the board is what the current composition produces; no stale-composition or
  rounding defect.
- Probe, oracle and thresholds sha are identical across every local verdict (f42b73a0 / b6fa47bd / e5d7f98e).
  `score_forge.py` differs: three versions produced the local verdicts (106cfa6f: Gemini, Hy4, Grok, gpt-terra, Ling,
  Aion Mini; bf785df6: Nex, Nemotron, Qwen Flash, GLM Flash; 7ede60a3 = current: the rest). The intervening commits
  (b1a4a7e4b, d022c8c0d, 0be0267d6) touch the undeclared-resource charge, wrong-shaped manifests and M7; low impact.
- 150-call budget: applied to every local run (`verdict.agent.budget`; the top-level `budget` key is simply absent
  on rescored verdicts — not a missing cap).
- Inner score has NO root dedup: by design (DESIGN §8.1 plain tier means; ROOT_BLOCKS is "attribution + multiplier
  dedup", §8.3). Band-4 `band_defects` prices shadows once, roots only from inside the band (score_forge.py:3244-3252),
  as DESIGN §8.5 says. The critical multiplier suppresses a critical dep only under a non-vacuous critical root
  (3296-3303), as DESIGN says. These match the design; the defects below are where the design's own intent is missed.

## CONFIRMED DEFECTS, ranked by board movement

### D1 (largest). `b_comment_exactly_once` fires the "duplicate side effect" critical on apps that posted NO duplicate

score_forge.py:2241-2252:
```python
bad = [... for s in scen if s.get('commentsAdded') != 1 or s.get('successFlags') != 1]
if bad: return g(0, f'not exactly once: ...', CRITICAL_CHECKS['b_comment_exactly_once'])
```
The critical's consequence is "duplicate side effect on a customer's Jira" (CRITICAL_CHECKS, DESIGN §8.3) and the
severity selftest (4) states the intent "a duplicate comment scores below a missing comment" (3429). But the predicate
charges (a) ZERO comments and (b) a missing success flag exactly like a duplicate, with a 0.6 cliff, plus a band-4
defect. The selftest only exercises the *vacuous* missing path, so the measured-zero inversion passes it.

Stored rows — 5 entries fired this critical; only ONE posted a duplicate:

| entry | stored detail | duplicate? |
|---|---|---|
| gpt-terra | `doubleClick@813: 2 comment(s), 0 success flag(s)` | YES (correct charge) |
| Qwen3.8 Omni Flash | `doubleClick@725: 0 comment(s), 1 success flag(s)` ×3 sprints | no — posted nothing, showed success (dedupe after the first post) |
| Sonnet 5.5 | `doubleClick@931: 0 comment(s), 0 success flag(s)`, `@1167` same | no — posted nothing |
| Hy4 | `post@930: 1 comment(s), 0 success flag(s)` … every scenario 1 comment | no — exactly one comment each time; flag missing (its `notify()` calls `showFlag` without `id`, ui/sprint-ledger.js:180) — already charged in `u_comment_flow` (2424-2437 checks `post.successFlags == 1`) |
| GLM-5.3 FlashX | seed 133ce75a: `post@749: 0 comment(s)`; comments `SEC-449 404` | no — see D1b |

D1b (root shadow): GLM FlashX's zero on seed 133ce75a is CAUSED by its permission leak. forge-observations.json: the
probe selected `changeId 85610` (SEC-449) in sprint 749; `b_no_permission_leak`'s own detail lists
`sprint-action-749…: SEC-449` and `data-change-id 85610`. The probe picks the first table row whose issue is not
comment-forbidden (forge_probe.mjs:641, 767), i.e. it picked the leaked hidden issue; the POST as the viewer got 404;
the app showed an ERROR flag (contract-correct for a failure). One defect (the leak) -> two critical multipliers
(0.6 × 0.6 = 0.36). `ROOT_BLOCKS['b_no_permission_leak']` (3006) = CHANGE_LIST_ROWS, which has `u_comment_flow` but
not `b_comment_exactly_once`. On the other two seeds the row is 1.0 (per-seed 0.305 / 0.535 / 0.510).

Published run pages state "duplicate side effect on a customer's Jira" for four apps that made none.

Counterfactual (critical fires only when some scenario added >= 2 comments; row stays 0 and stays a band-4 defect):
Qwen Omni Flash 0.5529 -> 0.8951, Sonnet 0.5508 -> 0.8649, Hy4 0.4796 -> 0.6890, GLM FlashX 0.2963 -> 0.4939
(if the row also passes when nothing duplicated: 0.9390 / 0.8958 / 0.6898 / 0.5045). gpt-terra unchanged. The
0.80 -> 0.55 cliff on the board is mostly this one predicate.

Refutation attempted: contract §5 "One click, or a double click, posts exactly one comment" — zero is "not exactly
one", so the ROW failing on Sonnet/Qwen Omni/GLM FlashX is defensible. The CRITICAL is not: its stated consequence and
selftest (4) both say a missing comment is milder than a duplicate. Hy4 cannot be defended at all (exactly one comment
every time). Also: the probe runs `doubleClick` on the same selection immediately after `post` (forge_probe.mjs:776)
without re-selecting, so an app that clears the selection or blocks re-posting the same change (an anti-duplicate
design) scores the duplicate critical; Sonnet's cause is unverified (tree not local).

Fix: in `b_comment_exactly_once`, fire the critical only on `commentsAdded >= 2`; grade zero comments / missing flag
in `u_comment_flow` (add the doubleClick's `commentsAdded == 1` as a sub-step, it already has the flag); select the
post target from the oracle's viewer-visible issues (or add `b_comment_exactly_once` to
`ROOT_BLOCKS['b_no_permission_leak']`); add a selftest case "measured zero comments (non-vacuous) scores above a
duplicate". Drop the stale "(the 429-once fault included)" (2251) and the video caption's "comment post through the
429 retry" (forge_probe.mjs:844) — the comment 429 was removed (forge/site/fixtures.cjs:631).

### D2. A two-attribute index range is charged as "KVS entity unused" (band 2, 0.699) — Max Prime, Ember, Muse

score_forge.py:998-1008 `_sprint_indexes` requires `len(rng) == 1`; `k_entity_declared` (1012) and
`s_entity_index_used` (2037) both read it; M7 (1265-1276) fails the same finding again in `k_manifest_semantics` with
no `graded_by` (against the file's own "PRICED ONCE" rule, 1176-1181); `deploy_readiness` (1442) then says "would not
deploy" while band 1 is passed.

Max Prime's manifest: `indexes: - name: by-sprint, partition: [sprintId], range: [at, changeId]`. Its KVS log:
44+3+3 `entity/query` on `scope-change` index `by-sprint` — it reads its ledger through the index every time; the
scorer reports "0/90 ledger reads are entity queries on the sprint index". Ember (`[atMs, changeIdNum]`) and Muse
(`[atMs, changeKey]`) chose the same shape — it is exactly the contract's default order (at, then changelog id).
The pinned real Forge manifest schema (`forge-kit/…/@forge/manifest/out/schema/manifest-schema.json`,
storage.entities.indexes.range) is `{"type":"array","items":{"type":"string"},"minItems":1}` — no maxItems; real
`forge lint` passed (l_deployable 1.0); the kit emulator sorts by every range attribute (kit/lib/kvs.cjs:183-186).
Only the docs prose cited in 0be0267d6 says "one attribute".

One root charges: k_entity_declared 0, s_entity_index_used 0, M7, the excellence gate (K row), and the band-2 cap.
Counterfactual (range accepted): Max Prime 0.6953 -> 0.9594 (rank 9 -> 5), Ember 0.4576 -> 0.4782, Muse 0.1312 ->
0.1355. If instead Forge really refuses it at deploy, the consistent charge is band 1 (would not deploy): Max Prime
0.4765, Ember 0.2696. **Either way 0.6953 corresponds to neither world** — the scorer itself says "would not deploy"
and grades "deployable, KVS entity unused".

Fix: decide the fact (one live `forge deploy` on wolfaenpak with a 2-attribute range settles it). If accepted: drop
`len(rng) == 1` and the M7 range clause. If refused: make it an l_deployable / band-1 fact and give M7 `graded_by`
so it is priced once. Confidence: high that the current charge is wrong; medium on the direction.

### D3 (small). Reconcile economy is a 1-call cliff at the oracle optimum; it orders the top 4

forge-thresholds.json `economy_rungs [[1.0,1.0],[0.75,2.0],…]`: the golden is 0.933x (14 calls vs optimum 15), so
the fitted top rung was clamped to exactly 1.0x. 16 calls (1.07x) -> 0.75. Stored: Sol 14 -> 1.0, Opus 14 -> 1.0,
Sol-pro 18 -> 0.75, Haiku 16 -> 0.75. That 0.25 row = 0.010 final, which is the whole gap between #2 (0.9767) and
#3/#4 (0.9664/0.9661); all four have inner 1.0, so the top-5 order is decided only by the E rows. The event row was
made continuous (§17.7 F1); the reconcile row was not. Fix: `min(1, top/ratio)` like `e_event_economy`
(approximated from the run-seed call counts in the stored detail, since the row is a 3-site mean: Haiku -> 0.9736,
Sol-pro -> 0.9697; they move up but stay below the first two). The identical 0.75 ×6 is this rung,
not a broken check.

### D4 (score-neutral). Criticals compound on apps with no functions/modules; one manifest root fires three criticals

When `l_bundles_load` / `l_deployable` are vacuous (no function / no module), nothing suppresses `r_backfill_complete`
and `u_widget_loads` (they have no precondition, DESIGN §8.2), so empty starters carry ×0.36 (Aion ×2, DeepSeek Pro,
Nemotron) and GLM Flash ×0.216. Solar: an undeclared `widget` resource turns every UI row into a charged 0
(`candidate_section_fault`, 3062-3080), so `b_no_permission_leak`, `b_comment_exactly_once` and `u_widget_loads`
each fire 0.6 from that one root (×0.0778 overall). Inner is ~0 for all of them, so the board does not move beyond
0.01; the multiplier stack is mis-attributed. Fix: a section fault proven from the manifest suppresses UI-surface
criticals as `root:manifest`; `l_deployable` failing / no functions should be a ROOT_BLOCKS root like l_bundles_load.

## The seven zeros (Q3) — all real per Forge's own rules; three never built anything

| entry | evidence (forge-observations.json / tree) | verdict |
|---|---|---|
| Aion 3.5 (dd4c377c) | manifest = the starter (`app:` only); lint `document required properties are 'modules' or 'connectModules'`; 23 of 150 calls; session ended on a text-only turn: "I'll now explore the manifest schema for the specific module types I need." | real zero; harness stop on narration |
| Aion 3.5 Mini (fc04cfc5) | starter untouched; 1 call, 85 output tokens, 3.2 s; last words "I'll start by exploring the workspace and reading the contract files." | real zero; measures one narrated turn |
| DeepSeek Pro (e5202647) | starter manifest; 108 calls, 10.2M input tokens spent on `tools/inspect*.py`; final turn emitted tool calls as TEXT (`<invoke name="write">`, engine-console.log:11385, 11407), file never written (.forge-dev/allq.py absent) | real zero; ended by a malformed tool-call turn |
| Nemotron (cf9fd2b1) | modules as maps; lint `trigger must be array`, `consumer must be array`…; wrote into `./Support/Goose/…` (quoted-path bug); budget 151 | real |
| GLM-5.3 Flash (e2f96d7d) | YAML invalid: `modules:` children at col 4, `  resources:` at col 2 (line 86) — "All mapping items must start at the same column"; real @forge/lint crashed reading it | real |
| Solar Mini 4 (0.0016) | 24 SchemaValidator errors (`trigger events property 0 must be string`, missing `filter`…) | real |
| Nex (0.0081) | manifest key `functions:` (not `function:`); lint `action references undefined function module with key 'getSprintScopeAction'` ×11 | real |

No zero comes from scorer strictness beyond real Forge lint. Harness note (not scorer): a single-model run ends at the
first assistant turn with no tool call; 3.0.108 continues only length-cut replies. Aion, Aion Mini and DeepSeek Pro
used 23 / 1 / 108 of 150 calls. Their zeros measure the stop rule as much as the model.

## Harness versions (Q5)

No goose version or build sha is recorded in the run tree, verdict.json or the board's `runMeta` — the effect cannot
be audited from stored rows (fix: stamp `goose --version` + git sha into verdict and runMeta). From dates only:
Opus (10-04) scored 0.9767 on the pre-3.0.106 harness, so the old harness did not cap Anthropic models; Sonnet's
gap is D1 plus real rows. Gemini 3.8 Flash ran 10-06 14:21Z, ~40 min after the 3.0.106 commit whose motivating bug was
Gemini thought signatures; unknown build; it used all 150 calls (no early empty-STOP), so it cannot be attributed.

## Rankings (Q6)

- Haiku 0.9661 vs Sonnet 0.5508: Sonnet pre-severity 0.918; ×0.6 from D1 (double click posted 0, no duplicate) ->
  0.551; band 4 has two defects (comment, LLM root `k_llm_model_current` vacuous with `u_llm_explain` priced once) ->
  0.869 cap. Without D1: 0.8649. So ~0.31 of the 0.415 gap is composition; ~0.10 is real (no LLM call at all, modal
  close 0/3 on one seed, comment flow 0.8). Different harness builds too (10-04 vs 10-09).
- Qwen Flash 0.7961 vs Max Prime 0.6953: earned 0.943 vs 0.927 — nearly equal. Qwen Flash is held at 0.799 by
  `v_dark_mode` 13/17 (dark widget painted `[18,18,18]`, not a `--ds-surface*` token — a stated contract §7 rule);
  Max Prime is held at 0.699 by D2. With D2 resolved as "accepted", Max Prime 0.9594 ranks 5th.

## Other things checked and found sound

- `capped_final` / `published_score` pull and truncation; band ceilings per DESIGN §8.5; critical severity transforms
  per §8.3 (r_backfill partial factors 0.7049 Ember, 0.7714 Gemini, 0.6814 Qwen 27B check out).
- u_widget_loads is independent of ledger content (any non-empty metric text), so it is not a shadow of r_backfill
  on Gemini/Qwen 27B/Muse; DeepSeek Flash's widget/action invoked nothing (`b_invoke_contract` vacuous) — real.
- GLM-5.3 Prime's l_deployable (19 SchemaValidator errors, `attributes property sprintId 'string' must be object`)
  is real Forge lint; ×0.6 is nearly redundant with the 0.499 cap (0.4921 -> 0.4736).
- `e_event_economy` 0.39-0.45 is a real measurement (85-117 calls); Qwen Flash reached 1.0 (36 calls, 1.29x), so the
  optimum is reachable; the contract only says "few Jira requests" (KVS metadata caching is not stated). Uniform cost.
- `k_rovo_skill` 0.2857 ×3 = SKILL.md missing from `source.dir` (5 of 7 conditions depend on it; GLM Prime's
  `skills/sprint-scope-analyst/` is empty) — real. `u_widget_chart` 0.75 ×2 = 3/4 charts — k/n coincidence.
- `u_widget_live` 0.75 ×8 (same failed sub-step `shows_new_numbers` on 8 apps) — looks like a shared signature;
  handed to the live-widget auditor.
- Worst-of-3 per-row merge composes a score below every single site (GLM FlashX 0.2963 vs 0.305/0.535/0.510; Ember
  0.4576 vs 0.50/0.54/0.58) — intended by DESIGN §8.7 F3, but worth stating on the board.
