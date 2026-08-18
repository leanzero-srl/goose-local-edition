# sb-6 scoring design — the instrument stops compressing

Target file: `evals/swarm-bench/bench/score_build.py` (extends the sb-5.3 registry idiomatically: `check()`/`product_check()` decorators, `g()` results, `Ctx` fields filled in `gather()`, env-gated evaluation, `SCORER_VERSION` bump). Everything below is deterministic — no LLM judges — and hermetic (raw WebGL/Canvas/CSS-3D only; the probe stays `product_probe.mjs` + playwright chromium with software GL).

## 0. Why sb-5 compresses (the diagnosis the math must answer)

Under sb-5.3, `score = 0.60·CORE + 0.15·J + 0.10·V + 0.05·P + 0.10·HARD`. Of that, **0.90 is saturable**: CORE/J/V/P are dominated by checks a strong model always lands (binary flips like `serves_page`, `api_content_type`, `v_filter`-presence, and fraction checks whose numerators a competent build maxes). Only HARD (0.10) is graded against measured optima. So Opus, Sonnet, and now a good local fleet all pile into 0.89–0.96: the instrument has ~0.10 of resolving power for the entire top half of the population.

The fix is arithmetic, not vibes: **move ≥ 0.40 of the total mass onto axes that cannot saturate by construction** (measured optima, differentials, calibrated rungs, a zero-defect gate), and remove every binary whose flip is worth more than one ladder rung.

---

## 1. Continuous partial credit: every binary becomes a ladder

Rule: **a check returns a measured fraction or a stated rung — never a bare bit.** Ladders below are stated per check. Rungs marked ⟨cal⟩ are *calibration-owned*: their cut points are set by the §2 procedure, not frozen by hand. Everything else is spec-traceable (the builder read the same number).

### Existing binaries → ladders (tier unchanged unless noted)

| Check | Ladder (top → bottom) |
|---|---|
| `server_runs` (A) | 1.0 healthy ≤ 5 s · 0.75 healthy ≤ 25 s · 0.4 port bound, `/api/health` non-200 · 0.15 process survives 5 s without binding · 0 crash at boot |
| `serves_page` (A) | 1.0 `GET /` 200 + all three assets (`styles.css`, `app.js`) 200 with correct content types · 0.7 page 200, one asset missing/mistyped · 0.4 page 200, no assets wired · 0.2 non-200 but `web/index.html` exists on disk · 0 |
| `total_field` (B) | 1.0 exact · 0.5 within ±2 (fencepost) · 0.25 within 20% · 0 |
| `ui_offline` (B) | 1.0 zero external refs AND page renders with non-localhost network blocked (browser-measured) · 0.5 zero external refs statically but blocked-network render fails · 0 |
| `vendor_cursor_paging` (C) | fraction of page transitions that used the documented cursor |
| `vendor_all_pages` (C) | pages fetched / pages required |
| `vendor_retry_secs`, `vendor_retry_date` (C) | 1.0 waited ∈ [Retry-After, 2×] · 0.6 retried too early but converged · 0.3 retried ignoring the header · 0 gave up |
| `vendor_cursor_expiry` (C) | 1.0 resumed per the documented restart protocol, zero rows lost · 0.7 restarted from scratch (correct, wasteful — the waste is charged once, in `request_efficiency`) · 0.3 skipped rows (data loss) · 0 |
| `client_all_payments` (C) | rows returned / EXPECTED_TOTAL |
| `client_total_count` (C) | same exactness ladder as `total_field` |
| `client_create_replay` (C) | 1.0 same id twice AND vendor-side count unchanged · 0.5 no duplicate but id not surfaced/different · 0 |
| `client_idempotency_key` (C) | fraction of create calls carrying the key |
| `client_integer_amounts` (C) | fraction of amounts integral **on the wire** (parse_float sentinel, existing pattern) |
| `concurrent_sync_safe` (D) | `max(0, 1 − |total − N| / N)` — full duplication (Δ=N) → 0 |
| `api_content_type` (D) | fraction of sampled responses with `application/json` |
| `client_timeouts` (D) | 1.0 sync survives the vendor's new hang-once trap within 35 s (behavioral) · 0.4 `timeout=` present in source only · 0 |
| `ui_error_actionable` (D) | 1.0 probe clicks the retry affordance with the API unblocked and the view **recovers** · 0.6 actionable text visible · 0.3 generic error text · 0 |
| `j_error_state` (J) | 1.0 visible + actionable + working retry · 0.6 visible + actionable · 0.3 any visible error indication · 0 |
| `j_empty_state` (J) | 1.0 empty text + Sync CTA works from empty db · 0.6 empty text · 0.3 not blank · 0 (phantom rows on empty db stay a hard 0) |
| `v_filter` (V) | 1.0 selecting `refunded` shows only refunded rows AND the readout updates AND `all` restores · 0.5 rows change plausibly · 0.2 control present but inert · 0 absent |
| `v_responsive_375` (V) | 1.0 no h-scroll AND ≥1 row rendered AND tap targets ≥ 40 px · 0.6 no h-scroll + rows rendered · 0.3 no h-scroll (empty page vacuous — see §5) · 0 |
| `ui_currency` → `v_currency_rendered` (V, browser truth) | 1.0 rendered summary matches the exact expected `€…` string computed from the fixture · 0.6 currency-formatted, wrong value · 0.3 raw minor units shown · 0 |

### New T-tier (3D) ladders — all read the `viz` probe scenario (§7 contract)

| Check | Ladder |
|---|---|
| `t_context_real` | 1.0 WebGL/WebGL2 on a canvas whose **visible** rect ≥ 200×150 px, ≥1 program linked, ≥1 draw call inside the observation window · 0.6 CSS-3D: computed `matrix3d` non-flat under `perspective` · 0.3 canvas drawing without 3D evidence · 0 |
| `t_scene_content` | statistics of the compositor screenshot clipped to the canvas: 1.0 non-background coverage ∈ [0.15, 0.85] ⟨cal⟩ AND ≥8 distinct 16-level-quantized colors · 0.6 coverage ∈ [0.05, 0.95] AND ≥4 colors · 0.3 any non-uniform pixels · 0 uniform |
| `t_data_bound` | differential: signature = (coverage, color count, draw calls) on the FULL instance vs the EMPTY instance (the harness already boots one for `j_empty_state`). 1.0 differ on ≥2 axes AND full coverage ≥ 0.10 · 0.5 differ on 1 axis · 0 identical — a decorative spinning cube scores 0 |
| `t_interaction` | 1.0 drag AND wheel each produce (changed-pixel fraction − no-input baseline) ≥ 0.02 ⟨cal⟩, change bounded < 0.98 · 0.5 one of the two · 0 |
| `t_animation` | fps ladder counting only frames that issued ≥1 draw call: 1.0 ≥45 · 0.75 ≥30 · 0.5 ≥15 · 0.25 ≥5 · 0 (interaction-driven static scenes are measured during the drag window instead) |
| `t_no_lib` | 1.0 no vendored three.js/babylon signature in served JS AND `t_context_real` > 0 · 0 signature found · 0 no 3D at all (gated — see §5) |

Determinism note (the `_ladder` doctrine already in the file): screenshots are taken under software GL, quantized to 16 levels/channel, statistics bucketed to 0.05, and every ⟨cal⟩ cut point must sit ≥ 3 buckets away from observed reference variance before it freezes. The sb-6 **control build** must exercise T fully so probe breakage trips the existing controls HIGH gate instead of silently zeroing a tier.

---

## 2. Difficulty calibration: the equation, the knobs, the bands

**Principle: raw measurements are never calibrated; only three tier-level knobs are.** Per-check calibration with 5 reps/model overfits instantly; three targets support at most three free parameters.

Each check emits a raw `x_i ∈ [0,1]` from fixed spec-traceable measurement. The scorer applies:

- fraction checks: `s_i = x_i^{γ_{t(i)}}` — applied **per check, before the tier mean**, so by Jensen's inequality inconsistency is punished (mean of powers < power of mean for γ>1); consistency is the axis the campaign already identified as real (F821).
- value ladders (P tier): rung bounds `b'_q = b_q / k_P` — spec budgets stay printed in details; `k_P` tightens the rungs.

Knobs: `γ_core` (A,B,C,D,J,V), `γ_hard` (T, HARD), `k_P`.

**Model score under knobs θ = (γ_core, γ_hard, k_P):**

```
S_m(θ) = 0.88 · [ Σ_{t∈{A..D,J,V}} w_t · mean_i x_i^γcore
                + w_P · mean_i ladder_{kP}(v_i)
                + Σ_{t∈{T,HARD}} w_t · mean_i x_i^γhard ]
       + 0.12 · E_m            (E: §3; expected ≈ 0 for today's references)
```

**Fit** (design-time, deterministic grid search — never a runtime curve):

```
θ* = argmin_θ  Σ_{m ∈ {opus, sonnet, haiku}} ( median_reps S_m(θ) − mid(band_m) )²
     over γcore ∈ {1.0, 1.1, …, 2.5}, γhard ∈ {1.0, …, 3.0}, kP ∈ {1, 1.5, 2, 3}
```

Run via the existing Bedrock pipeline (`calibrate.py` models table, `goose run --provider aws_bedrock`, token at `~/.config/agent-board/bedrock.env`), extended to build the full sb-6 spec and score with `BENCH_SB6=1`; ≥5 reps per model, **medians** (robust to one timeout dud). θ* freezes as constants; `SCORER_VERSION = "sb-6.0"`; verdicts archived under `runs/calibration-sb6/`.

**Acceptance criteria (all must hold, or the SPEC iterates — never the knobs beyond their caps):**
1. ordering: `median(Opus) − median(Sonnet) ≥ 0.06` and `median(Sonnet) − median(Haiku) ≥ 0.10`
2. per-model IQR ≤ 0.06 (else the instrument is noisy — fix probes, don't widen bands)
3. every model inside its band. All above → the task is too easy: deepen the task (the file's own doctrine). All below → too hard.
4. γ capped at 4.0 hard: a band unreachable inside the cap is a task-design defect, not a curve-fitting job. This is the anti-Goodhart clause.

**Proposed bands and rationale** (I adjusted the suggested Opus band downward; here is why):

| Model | Band | Rationale |
|---|---|---|
| Opus | **0.72 – 0.80** | The 0.12 EXCELLENCE slice is gate-locked (§3) and today's references will not pass a zero-defect gate, so the non-E ceiling is 0.88. An Opus band of 0.78–0.85 would force Opus's inner mean to 0.89–0.97 — which contradicts a punishing T/HARD. 0.72–0.80 leaves 0.08 of non-E margin plus the whole 0.12 E slice above Opus: future models stay measurable. |
| Sonnet | **0.60 – 0.70** | ≥ 0.06 gap to Opus's floor; the gap is carried mostly by T+HARD (feasibility below). |
| Haiku | **0.40 – 0.52** | "Kinda works scores about half" — the file's own design rule. Haiku builds a running app; the floor must say so. |
| (projection) local 27b fleet | ~0.35 – 0.50 | Today's 0.8911 population lands where the operator's actual experiments need resolution. |

Band widths (~0.08–0.12) ≥ 3× observed rep noise (n3 arm spread 0.003; single-node σ ≈ 0.02–0.03), so bands cannot overlap under run-to-run variance.

**Feasibility rehearsal** (illustrative pre-calibration medians — the fit sets the real ones):

- Opus: CORE .94, J .90, V .88, P 1.0, T .70, HARD .62 → inner = .376+.108+.070+.060+.098+.124 = .836 → **0.736** ✓
- Sonnet: CORE .88, J .78, V .75, P .95, T .45, HARD .45 → inner = .716 → **0.630** ✓
- Haiku: CORE .72, J .55, V .50, P .85, T .15, HARD .25 → inner = .516 → **0.454** ✓

Note where the Opus–Sonnet gap lives: 0.88·(0.14·0.25 + 0.20·0.17) ≈ **0.061 of the ~0.11 total gap comes from T+HARD alone** — the spread now lives on the hard axes, which is the entire point.

---

## 3. The EXCELLENCE band (E): the last 0.12, gate-locked

```
score = 0.88 · inner + 0.12 · E,      E = G · mean(E-checks)
```

`G ∈ {0,1}` — the **zero-defect gate**, all conditions simultaneously:
- every J check == 1.0 (all four journeys perfect in a real browser)
- zero console errors across ALL probe scenarios (load, sync, error, empty, viz)
- `v_responsive_375 == 1.0` and `v_dates_readable == 1.0`
- every P check at its top rung
- `t_data_bound == 1.0` (the 3D scene provably encodes the data)

E-checks (each a ladder, only evaluated when G could plausibly hold — they cost extra probe time):
- `e_fps_under_data`: ≥45 fps sustained on the full fixture with ≥1 draw call/frame
- `e_load_p95_100`: `GET /api/payments?limit=100` p95 within budget **while a sync is running** (8 concurrent clients)
- `e_interaction_latency`: pointer-drag → first pixel change ≤ 100 ms (rAF timestamps)
- `e_hard_mastery`: HARD tier mean ≥ 0.90

Arithmetic consequence: **no build reaches > 0.88 without the gate**, and the gate requires the 3D tier + perf budgets + a zero-defect frontend together — exactly the operator's top-end definition. Today's references land 0.45–0.80; the band above 0.88 stays empty until something earns it, which is what "100 is not meant to be reachable" demands.

---

## 4. Compound checks: gate × min composition

**Math.** `compound = (Π gates ∈ {0,1}) · min(components)`. `min`, not product: product double-punishes correlated partials; `min` states "the weakest property bounds the whole," and each component is itself continuous so the gradient survives.

**Anti-stacking rule (the reason compounds exist):** a measurement feeds **at most one credit-carrying check**. When a compound absorbs components, their standalone entries become weight-0 diagnostics (reported in `parts`, excluded from tier means). Asserted disjoint at import.

Membership:

| Compound | Tier | Gates | Components (each a §1 ladder) | Absorbs |
|---|---|---|---|---|
| `h_sync_discipline` | HARD | sync2 ran AND vendor requests observed | min(second-sync cheapness ladder, idempotence, update-propagation fraction) | `resync_idempotent`, `second_sync_cost`, `update_propagation` |
| `h_durability` | HARD | rows before SIGKILL > 0 | min(restart persistence fraction, concurrent-sync distance, atomic-upsert ladder) | `restart_persistence`, `concurrent_sync_safe`, `store_atomic_upsert` |
| `t_viz_truth` | T | visible canvas ≥ 200×150; no vendored 3D lib | min(`t_scene_content`, `t_data_bound`, `t_interaction`) | those three as credit-carriers |
| `j_first_use` | J | renderedRowCount > 0 | min(time-to-first-data ladder, rendered==claimed reconciliation, console-clean) | `j_loads_data`, `j_console_clean` |
| `c_api_depth` | C | ≥1 row observed on the wire | min(deep-schema exactness fraction, boundary-offset pagination, validation matrix) | `local_pagination`, `input_validation`, wire-shape half of `row_integrity` |

This is the mechanism that stops partial implementations stacking: under sb-5, "syncs but duplicates" + "cheap but stale" + "propagates but slowly" each banked separate credit; under sb-6 the trio is one number bounded by its worst member.

---

## 5. Anti-gaming invariants: every new check names its vacuous-pass counter

Doctrine already in the file ("absent input must score zero, never full marks"), now systematic:

| Check | Vacuous pass attempted | Counter |
|---|---|---|
| `t_context_real` | 1-px / offscreen canvas; context created, never drawn | visible-rect ∩ viewport ≥ 200×150 AND ≥1 draw call inside the observation window |
| `t_scene_content` | solid full-canvas fill; stretched static image | coverage **upper** bound 0.95; distinct-color floor; background color sampled from the page body, not assumed |
| `t_data_bound` | decorative cube identical with/without data | empty-vs-full differential required — identical signatures score 0 |
| `t_data_bound` (2nd order) | random-noise renderer "differs" from everything | **self-consistency gate**: two screenshots of the SAME instance must agree (within-instance variance small) before between-instance difference earns credit |
| `t_data_bound` (3rd order) | fabricated marks on the empty db | empty-instance viz showing full-instance-level coverage → 0 (phantom-data rule, mirrors `j_empty_state`) |
| `t_interaction` | perpetual full-canvas animation makes any diff "change" | credit = interaction delta **minus** same-Δt no-input baseline ≥ 0.02 |
| `t_animation` | empty rAF loop — fps without content | only frames with ≥1 draw call count |
| `t_no_lib` | no 3D at all → "no library" trivially true | credit gated on `t_context_real > 0` |
| `j_first_use` | blank page has no console errors | renderedRowCount > 0 gate before console-clean credit |
| `v_filter` | control present but inert | credit requires the row set to change AND restore; presence alone caps at 0.2 |
| `v_responsive_375` | empty page never scrolls horizontally | ≥1 rendered row required at 375 px |
| `p_*` under load | fast because empty or erroring | latency credit multiplied by a correctness gate (rows == expected **on the same responses**) |
| `e_fps_under_data` | static scene reports "60 fps" | draw-calls-per-frame requirement |
| `h_sync_discipline` | no second sync = infinitely cheap | ran-gate; absence scores 0 (the `second_sync_cost` precedent, kept) |
| `c_api_depth` | endpoint absent → no wrong fields observed | schema credit computed only over observed rows; zero rows = 0 |

---

## 6. sb-6 tier/weight table and the compression-proof arithmetic

Inner weights (sum 1.0, scaled by 0.88), within-core split shifted off the saturating floor (A drops from .25 to .15 of core — every reference saturates A, so saturation buys less):

| Slice | Inner w | Absolute | Character |
|---|---|---|---|
| CORE (A .15 · B .30 · C .30 · D .25 within) | 0.40 | **0.3520** | saturable, γ_core-tempered |
| J journeys | 0.12 | **0.1056** | ladders + `j_first_use` compound |
| V visual | 0.08 | **0.0704** | ladders, browser truth |
| P performance | 0.06 | **0.0528** | k_P-stretched value ladders |
| T three-d | 0.14 | **0.1232** | differential-graded, γ_hard |
| HARD | 0.20 | **0.1760** | measured optima + compounds, γ_hard |
| E excellence | — | **0.1200** | gate-locked (§3) |
| **Total** | | **1.0000** | |

**Max-score decomposition.** Saturable mass (CORE+J+V+P) = 0.88·0.66 = **0.5808**. Non-saturable mass (T+HARD+E) = **0.4192**, of which 0.12 sits behind a zero-defect gate and 0.2992 is graded against measured optima, calibrated rungs, and empty-vs-full differentials that have no flip-to-1 shortcut.

**Why compression cannot recur:**
1. sb-5's saturable mass was 0.90; sb-6's is 0.58 — a build that perfects everything perfectible by saturation stops at **0.5808 + its T/HARD earnings**, and at reference-level T/HARD (≈0.65) that is ≈ 0.88·(0.66 + 0.34·0.65) = **0.771**. The top half of the scale belongs to the hard axes by arithmetic.
2. γ_core > 1 applies per check before the mean, so even inside the saturable mass, one weak ladder rung drags more than it used to — per-rep saturation now requires top rungs across ~45 ladders simultaneously.
3. No remaining binary is worth more than one rung of one ladder.
4. **Weight-floor invariant, enforced at import** (a gate, not a memory — the four-errors-one-shape lesson):

```python
assert TIER_WEIGHT_SB6["T"] + TIER_WEIGHT_SB6["HARD"] >= 0.34, "hard-axis floor"
assert E_WEIGHT >= 0.10, "excellence slice floor"
```
Any future edit that re-compresses the instrument must delete an assert to do it — the diff says so out loud.

---

## 7. Drop-in Python skeleton

Appends to `score_build.py`, same env-gating pattern as `BENCH_PRODUCT`/`BENCH_AMEND`. The `viz` probe scenario contract (for `product_probe.mjs`): the probe injects a GL wrapper via `addInitScript` (counts `drawArrays`/`drawElements`, linked programs, `DEPTH_TEST`, canvas `getContext` kinds), screenshots the canvas rect via compositor (no `preserveDrawingBuffer` dependence), and emits:

```json
{"canvas": {"w":0,"h":0,"visibleW":0,"visibleH":0},
 "gl": {"kind":"webgl2|webgl|css3d|canvas2d|none","drawCalls":0,"programs":0,"depthTest":false},
 "shots": {"coverage":0.0,"colors":0,"repeatDelta":0.0},
 "interaction": {"dragDelta":0.0,"wheelDelta":0.0,"baselineDelta":0.0},
 "fps": {"frames":0,"drawFrames":0,"seconds":2.0},
 "lib": {"threeDetected":false}}
```

```python
# ── sb-6: punishing tier — env-gated, sb-5 path stays byte-identical ─────────────────────────
SB6 = bool(os.environ.get("BENCH_SB6"))
SCORER_VERSION = "sb-6.0" if SB6 else SCORER_VERSION

# Inner weights (× 0.88) + the gate-locked E slice. The asserts are the compression-proof:
# re-compressing the instrument requires deleting one, which no diff does quietly.
TIER_WEIGHT_SB6 = {"A": 0.06, "B": 0.12, "C": 0.12, "D": 0.10,   # CORE 0.40, split 15/30/30/25
                   "J": 0.12, "V": 0.08, "P": 0.06, "T": 0.14, "HARD": 0.20}
E_WEIGHT = 0.12
assert abs(sum(TIER_WEIGHT_SB6.values()) - 1.0) < 1e-9
assert TIER_WEIGHT_SB6["T"] + TIER_WEIGHT_SB6["HARD"] >= 0.34, "hard-axis weight floor"
assert E_WEIGHT >= 0.10, "excellence slice floor"

# Calibration knobs — frozen by bench/calibrate_sb6.py (grid fit, §2), NEVER hand-edited.
# γ applies per check BEFORE the tier mean: inconsistency is punished (Jensen).
GAMMA = {"core": 1.0, "hard": 1.0}   # placeholder until the Bedrock fit lands
K_P = 1.0
GAMMA_CAP = 4.0
assert GAMMA["core"] <= GAMMA_CAP and GAMMA["hard"] <= GAMMA_CAP

SB6_CHECKS: List[tuple] = []
DIAGNOSTIC: set = set()   # absorbed-by-compound checks: reported, weight zero


def sb6_check(name: str, tier: str) -> Callable:
    def deco(fn):
        SB6_CHECKS.append((name, tier, fn))
        return fn
    return deco


def compound(components: Dict[str, float], gates: Dict[str, bool]) -> tuple:
    """gate × min. Gates are binary preconditions (absence scores 0 — the vacuous rule);
    components are ladders; the weakest bounds the whole so partials stop stacking."""
    if not all(gates.values()):
        failed = [k for k, v in gates.items() if not v]
        return 0.0, {"gate_failed": failed, **components}
    return min(components.values()), {**{f"gate:{k}": True for k in gates}, **components}


def _viz(c: Ctx) -> Dict:
    return c.probe_viz if isinstance(getattr(c, "probe_viz", None), dict) else {}


# ── T tier ────────────────────────────────────────────────────────────────────────────────────

@sb6_check("t_context_real", "T")
def t_context_real(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    gl, cv = p.get("gl") or {}, p.get("canvas") or {}
    visible = (cv.get("visibleW") or 0) >= 200 and (cv.get("visibleH") or 0) >= 150
    if gl.get("kind") in ("webgl", "webgl2") and visible and (gl.get("drawCalls") or 0) > 0:
        s = 1.0
    elif gl.get("kind") == "css3d":
        s = 0.6
    elif gl.get("kind") == "canvas2d" and visible and (gl.get("drawCalls") or 0) > 0:
        s = 0.3
    else:
        s = 0.0
    return g(s, f"{gl.get('kind') or 'none'} on {cv.get('visibleW')}x{cv.get('visibleH')} "
                f"visible px, {gl.get('drawCalls', 0)} draw calls",
             "no real 3D surface — an offscreen or never-drawn context earns nothing",
             parts={"visible": visible, "kind": gl.get("kind"),
                    "draw_calls": gl.get("drawCalls", 0)})


@sb6_check("t_viz_truth", "T")
def t_viz_truth(c: Ctx):
    """COMPOUND: scene content AND data-binding AND interaction, bounded by the weakest.
    The data-binding differential is the anti-decoration mechanism: a spinning cube looks
    identical over an empty db and scores 0; a noise renderer fails the self-consistency
    gate (repeatDelta must be small BEFORE between-instance difference earns credit)."""
    full, empty = _viz(c), getattr(c, "probe_viz_empty", {}) or {}
    if _pe(full):
        return g(0, f"PROBE UNAVAILABLE: {_pe(full)}", "harness failure, not app evidence")
    shots, eshots = full.get("shots") or {}, empty.get("shots") or {}
    cov, colors = shots.get("coverage") or 0.0, shots.get("colors") or 0
    scene = (1.0 if 0.15 <= cov <= 0.85 and colors >= 8 else
             0.6 if 0.05 <= cov <= 0.95 and colors >= 4 else
             0.3 if cov > 0 else 0.0)
    self_consistent = (shots.get("repeatDelta") or 1.0) <= 0.05
    axes = sum([abs(cov - (eshots.get("coverage") or 0.0)) >= 0.10,
                abs(colors - (eshots.get("colors") or 0)) >= 3,
                abs((full.get("gl") or {}).get("drawCalls", 0)
                    - (empty.get("gl") or {}).get("drawCalls", 0)) >= 3])
    bound = (1.0 if axes >= 2 and cov >= 0.10 else 0.5 if axes == 1 else 0.0)
    it = full.get("interaction") or {}
    base = it.get("baselineDelta") or 0.0
    drag = (it.get("dragDelta") or 0.0) - base >= 0.02
    wheel = (it.get("wheelDelta") or 0.0) - base >= 0.02
    inter = 1.0 if drag and wheel else 0.5 if drag or wheel else 0.0
    score, parts = compound(
        {"scene": scene, "data_bound": bound, "interaction": inter},
        {"canvas_visible": t_context_real(c)["score"] > 0,
         "no_vendored_lib": not (full.get("lib") or {}).get("threeDetected"),
         "self_consistent": self_consistent})
    return g(score, f"scene={scene} bound={bound} (axes {axes}/3) inter={inter} "
                    f"cov={cov:.2f} colors={colors}",
             "the 3D element is decoration, noise, or inert — not a data surface",
             parts=parts)


@sb6_check("t_animation", "T")
def t_animation(c: Ctx):
    p = _viz(c)
    if _pe(p):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    f = p.get("fps") or {}
    secs = f.get("seconds") or 0
    fps = (f.get("drawFrames") or 0) / secs if secs else 0.0   # draw-frames only: empty rAF earns 0
    return g(_ladder(-fps, [(1.0, -45), (0.75, -30), (0.5, -15), (0.25, -5)]),
             f"{fps:.0f} draw-fps over {secs}s ({f.get('frames', 0)} raw frames)",
             "a scene that cannot animate at interactive rates fails the product bar")


# ── HARD compounds (absorbed members demote to diagnostics) ──────────────────────────────────
DIAGNOSTIC |= {"resync_idempotent", "second_sync_cost", "update_propagation",
               "restart_persistence", "concurrent_sync_safe", "store_atomic_upsert"}


@sb6_check("h_sync_discipline", "HARD")
def h_sync_discipline(c: Ctx):
    ran = isinstance(c.sync2, dict) and any(k in c.sync2 for k in ("inserted", "total", "fetched"))
    cheap = (1.0 if c.sync2_reqs and c.sync2_304 == c.sync2_reqs else
             0.7 if c.sync2_reqs and c.sync2_cond == c.sync2_reqs else
             0.4 if c.sync2_304 and c.sync2_cond else 0.0)
    idem = (1.0 if c.sync2.get("inserted") == 0 and c.sync2.get("total") == EXPECTED_TOTAL
            else 0.5 if c.sync2.get("total") == EXPECTED_TOTAL else 0.0)
    prop = min(c.update_seen / c.update_changed, 1.0) if c.update_changed else 0.0
    score, parts = compound({"cheap": cheap, "idempotent": idem, "propagates": prop},
                            {"sync2_ran": ran, "vendor_reqs_seen": c.sync2_reqs > 0})
    return g(score, f"cheap={cheap} idem={idem} prop={prop:.2f}",
             "sync must be cheap AND idempotent AND propagate updates — together, not severally",
             parts=parts)


# ── evaluate() extension ──────────────────────────────────────────────────────────────────────

def _gamma(x: float, tier: str) -> float:
    return max(0.0, min(1.0, x)) ** (GAMMA["hard"] if tier in ("T", "HARD") else GAMMA["core"])


def excellence(rows: List[Dict], c: Ctx) -> tuple:
    by = {r["check"]: r for r in rows}
    gate = (all(by[n]["score"] == 1.0 for n in
                ("j_first_use", "j_sync_journey", "j_error_state", "j_empty_state") if n in by)
            and by.get("v_responsive_375", {}).get("score") == 1.0
            and by.get("v_dates_readable", {}).get("score") == 1.0
            and by.get("t_viz_truth", {}).get("score", 0) >= 1.0
            and all(by[n]["score"] == 1.0 for n in
                    ("p_list_latency", "p_page_interactive", "p_sync_wall") if n in by))
    e_rows = [r for r in rows if r["tier"] == "E"]
    e_mean = sum(r["score"] for r in e_rows) / len(e_rows) if e_rows else 0.0
    return (1.0 if gate else 0.0), e_mean


def evaluate_sb6(c: Ctx, rows: List[Dict]) -> Dict:
    """Called from evaluate() when SB6: rows already carries CORE+product+sb6 check results."""
    tiers, inner = {}, 0.0
    for tier, w in TIER_WEIGHT_SB6.items():
        sub = [r for r in rows if r["tier"] == tier and r["check"] not in DIAGNOSTIC]
        mean = sum(_gamma(r["score"], tier) for r in sub) / len(sub) if sub else 0.0
        tiers[tier] = {"mean": round(mean, 4), "checks": len(sub), "weight": w}
        inner += mean * w
    gate, e_mean = excellence(rows, c)
    score = 0.88 * inner + E_WEIGHT * gate * e_mean
    tiers["E"] = {"mean": round(gate * e_mean, 4), "gate": bool(gate), "weight": E_WEIGHT}
    return {"score": round(score, 4), "scorer_version": SCORER_VERSION,
            "inner": round(inner, 4), "excellence_gate": bool(gate), "tiers": tiers,
            "checks": rows, "root_causes": attribute_root_causes(rows),
            "excellent": gate and score >= 0.90, "solid": score >= 0.62}
```

**Calibration fit sketch** (`bench/calibrate_sb6.py`, reuses `calibrate.py` env/model plumbing):

```python
BANDS = {"opus-5": (0.72, 0.80), "sonnet-5": (0.60, 0.70), "haiku-4.5": (0.40, 0.52)}

def rescore(verdict, gc, gh, kp):        # re-applies knobs to ARCHIVED raw x_i — no re-runs
    ...

def fit(verdicts_by_model):
    best = None
    for gc in [1.0 + 0.1 * i for i in range(16)]:
        for gh in [1.0 + 0.1 * i for i in range(21)]:
            for kp in (1.0, 1.5, 2.0, 3.0):
                med = {m: median(rescore(v, gc, gh, kp) for v in vs)
                       for m, vs in verdicts_by_model.items()}
                if not (med["opus-5"] - med["sonnet-5"] >= 0.06
                        and med["sonnet-5"] - med["haiku-4.5"] >= 0.10):
                    continue
                loss = sum((med[m] - sum(BANDS[m]) / 2) ** 2 for m in med)
                if best is None or loss < best[0]:
                    best = (loss, gc, gh, kp, med)
    return best   # None ⇒ bands unreachable inside caps ⇒ iterate the SPEC, not the knobs
```

## 8. What `gather()` / the probe must add (interface only — sibling work)

`Ctx` fields: `probe_viz`, `probe_viz_empty` (same empty-instance boot the empty-state probe already does — reused, not duplicated), plus the under-load perf fields for E. Probe: one new `viz` scenario per §7's JSON contract (GL wrapper via `addInitScript`, compositor-clipped screenshots quantized to 16 levels/channel, drag+wheel with a no-input baseline window, 2 s draw-frame counter). The sb-6 control build must include a real data-bound WebGL scene so the controls' HIGH gate catches probe breakage — the reference zeroing is the harness refusing to be trusted, the existing pattern.

**Confidence.** High: tier/weight arithmetic, compound math, calibration equation, anti-gaming counters — these are extensions of mechanisms already proven in this file. Lower: the exact ⟨cal⟩ pixel-statistic cut points (coverage bands, delta thresholds) — software-GL rendering variance across the fleet's machines is the one empirical unknown, and it is exactly what the ≥3-bucket-margin rule and the calibration reps exist to measure before anything freezes.