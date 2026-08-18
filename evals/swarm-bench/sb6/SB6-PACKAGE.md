# sb-6 — THE HARD TIER: unified design package

Four artifacts, one frozen vocabulary. Everything here is deterministic (no LLM judges), hermetic (raw WebGL, stdlib backend, zero CDN), and runs on the existing harness (`vendor_service.py` mock + `product_probe.mjs`/playwright + `score_build.py` + `run_build.py` Bedrock entrants).

---
---

# ARTIFACT 1 — SB6-DESIGN.md

## 1. What sb-6 is

sb-6 replaces the sb-5.x "VendorSync" build task with **VendorSync Pro** (`vspro`): a harder API contract (webhook push with HMAC signatures, optimistic concurrency via If-Match/412/428, partial-failure batch creates, 4 currencies with minor-unit exponents 0/2/3, Europe/Berlin day-bucketing across a DST transition), a genuinely hard frontend (an interactive **raw-WebGL 3D bar chart** with a fully frozen scene/camera/interaction contract and a mandatory `window.vsdbg` instrumentation API), and a scorer whose top half is arithmetically owned by non-saturable axes. The instrument stays deterministic end to end: the probe recomputes every expected pixel analytically from the frozen contract and compares it to the real framebuffer — no golden images, no judges.

## 2. Why — the punishing-separation argument (from the data)

The 19,509-verdict mine (93 unique builds + 3 cloud entrants) proved the compression is structural, not anecdotal:

- **42.8 of 100 pts pass for ≥90% of serious builds** (25/60 checks); at ≥80% it is 70.3 pts. 32/60 checks are binary in practice (≤2 distinct values across 93 builds). 44/60 checks sit at mean 1.0 for the top cohort.
- **The entire top-band loss budget is 2.76 pts across 5 checks, four of which are broken or noise**: `v_status_distinct` is fixture-capped at 0.5 (every fixture payment is "settled"), `j_sync_journey.view_refreshed` fails for 16/17 builds *including both cloud models* (probe artifact), and the rest are source greps.
- **The Opus–Sonnet gap (0.63 pts) is 100% regex greps** — 57/60 checks identical. The leaderboard above 0.95 is ordered by whether a model wrote a literal `timeout=`.
- **The local↔cloud gap is one defect fanned into ~8 checks** (partial sync 100/247 ≈ 7.9 of the 11.1-pt gap). The moment the fleet lands full sync it sits beside Sonnet — the exact compression Mihai observed at 0.8911.
- **The P tier is dead** (0.24–0.74 ms measured vs 150 ms budget — 200–600× slack), and `request_efficiency` has a live vacuous pass (13/31 serious builds score 1.0 with 3–5 requests, five of which synced 0 rows).

The fix is arithmetic: sb-5's saturable mass is 0.90 of the score; sb-6's is **0.58**. The remaining **0.42 lives on axes with no flip-to-1 shortcut**: the T (3D) tier graded against analytically recomputed pixels (0.1232 absolute), HARD compounds graded against measured optima (0.1760), and a gate-locked EXCELLENCE slice (0.12) that no build reaches without a zero-defect frontend + 3D truth + perf under load simultaneously. A build that perfects everything saturable stops at ≈0.77 with reference-level T/HARD. Feasibility rehearsal (pre-calibration): Opus ≈ 0.74, Sonnet ≈ 0.63, Haiku ≈ 0.45, current local fleet projected 0.35–0.50 — the population lands exactly where the operator's experiments need resolution, with 0.20+ of headroom above Opus for future models.

Every measured sb-5 defect is answered by name in this package:

| sb-5 defect (measured) | sb-6 answer |
|---|---|
| `v_status_distinct` fixture ceiling (30% of top loss) | Fixture interleaves 4 statuses so default page 1 always carries ≥3; check regrades on ≥3 distinct styled pairs |
| `j_sync_journey.view_refreshed` probe artifact (27% of top loss) | Journey re-based: probe boots the app on a **half-seeded db**, so the sync click observably changes the rendered row count |
| Grep-decided top order (`client_timeouts`, `ui_currency`, `ui_polish`) | Converted to behavior: vendor `--stall` trap (sync must return within bound), rendered amount-cell parsing (JPY/KWD exponent traps), rendered-style checks. 0.4 residual source-only credit on timeouts |
| `request_efficiency` vacuous clamp | 1.0 **iff** reqs == OPTIMAL_REQUESTS (mock-exported constant) **and** completeness == 1.0; fewer-than-optimal scores 0 unless completeness is 1.0 and all vendor traps passed |
| `second_sync_cost` dead 0.4 rung | Continuous `0.5·cond/reqs + 0.5·304/reqs`, as the "cheap" component of the `h_sync_discipline` min-compound |
| `summary_accuracy` dead ±10% band | Per-currency bucket credit: k of 4 (count, total_minor) pairs exact → k/4; cross-currency sums are forbidden by spec |
| Dead P tier | Re-founded on interaction/render/under-load budgets (tooltip 150 ms, optimistic paint 100 ms, first 3D frame 3 s, drag frame count, `/api/payments` p95 **while 8 clients sync**) — real at any row count because they measure the render/contention path, not idle SQLite |
| Partial-sync fan-out reads as breadth | ROOT_BLOCKS extended to attribute `sync_completeness < 1.0`, not only == 0 |
| One defect = many checks stacking partial credit | gate×min compounds (`h_sync_discipline`, `h_durability`, `j_first_use`, `c_api_depth`); absorbed members demote to weight-0 diagnostics |

## 3. Contradiction ledger — which side won, and why

| # | Contradiction | Winner | Why |
|---|---|---|---|
| R1 | **Scene semantics**: hard-spec's day×status bucket chart (`#viz3d`, buckets from `/api/buckets`) vs probe-3d's per-payment 5×5 grid (`#scene`, 800×480) | **hard-spec** | The bucket scene ties the 3D view to the DST/Berlin data-correctness trap (wrong bucketing = visibly wrong bars = pixel checks fail), and its frozen contract is the more complete spec text (tooltip, click→filter, fallback table, vsdbg). probe-3d's math is generic — its verified pipeline re-parameterizes onto any pinned camera. Probe constants updated: canvas `#viz3d`, colors incl. `failed #DC2626`, side ×0.62, clear `#0F172A`, camera yaw/pitch/distance. |
| R2 | **Canvas size**: probe-3d pins 800×480; hard-spec requires responsive full-width canvas | **hard-spec**, probe adapts | The probe measures `getBoundingClientRect()` at the fixed 1280×800 viewport and computes aspect from the measured rect (deterministic per app). The 800×480 backing assert becomes `backing == round(rect × DPR)` with DPR forced to 1. |
| R3 | **Idle animation**: probe-3d's 6°/s idle orbit + prefers-reduced-motion contract vs hard-spec's static, drag-driven scene | **hard-spec** | Determinism (pixel sampling against a static scene needs no motion freeze) and product taste (an idle-spinning finance chart is decoration; the operator's design rules demand intentional UI). Motion capability is measured instead during the **scripted 2-s drag**: `vsdbg.frames()` delta ≥ 24 (calibration-owned rung), gated on instrumented draw calls advancing. The `viz-motion` scenario is dropped; reduced-motion emulation is kept purely as a determinism lever in the probe context. |
| R4 | **App instrumentation**: hard-spec's graded `window.vsdbg` vs probe-3d's "never trust app internals" | **both, layered** | Pixels are the truth anchor (probe-side analytic math vs readPixels, with the screenshot cross-check as hardening); vsdbg is an *additional graded surface* whose claims are cross-checked against both. vsdbg that contradicts the pixels scores 0 on `t_vsdbg_truth` — "an instrumentation layer that reports a scene the canvas does not show scores as broken, not as clever." probe-3d's `#camera-readout`/`#picked-payment` DOM readouts are dropped; `vsdbg.camera()`/`vsdbg.pick()` carry that truth, and the user-visible pick effect is `#status-filter` changing plus the table refreshing. |
| R5 | **Fallback**: probe-3d's glKill no-WebGL scenario vs hard-spec's user-facing `#viz-toggle` 2D table | **merged** | The 2D table (`#viz-fallback`) exists as a product feature (graded via toggle in the `viz` scenario: cells must equal `/api/buckets`); one added spec sentence makes it the automatic no-WebGL degradation path, graded by the `viz-fallback` scenario (glKill init script: no throw, table + notice visible, main table alive). Cheap for the builder, doubly testable. |
| R6 | **Tier naming/weights**: probe-3d's G tier {J .10, V .07, P .04, G .09} vs scorer-finesse's T tier inside {A .06 … T .14, HARD .20} ×0.88 + E .12 | **scorer-finesse** | It is the dedicated scoring workstream; its arithmetic (saturable mass 0.90→0.58, hard-axis floor asserts) answers the compression diagnosis directly. probe-3d's G checks become T-tier checks under those weights. |
| R7 | **Calibration mechanism**: scorer-finesse's three tier-level knobs (γ_core, γ_hard, k_P; grid fit) vs bedrock-calibration's per-check Opus-median thresholds (s′ = min(1, s/t_c)) | **scorer-finesse** | Decisive overfit argument: three band targets support at most three free parameters; per-check quantiles at n=6 are noise for any check near p=0.5 (bedrock's own admission), and bedrock itself flags the quantile rule as score-*inflating* in isolation. Per-check Opus/Sonnet/Haiku medians survive as **classification diagnostics** (binary / non-discriminating / dead — feeding spec iteration, never rescaling), inside the same sha256-pinned artifact bedrock designed. |
| R8 | **Opus acceptance band**: bedrock G2 [0.80, 0.90] vs scorer-finesse [0.72, 0.80] | **scorer-finesse** | The E slice is gate-locked, so the non-excellence ceiling is 0.88; an Opus band of 0.80–0.90 would force Opus's inner mean toward 0.91–1.0, contradicting a punishing T/HARD. Bedrock marked its band medium-confidence and pre-dated the E-gate design. G2 becomes: Opus median ∈ [0.72, 0.80]; > 0.84 ⇒ spec too easy, deepen. Ordering gaps take scorer-finesse's stricter ≥0.06 (Opus–Sonnet) / ≥0.10 (Sonnet–Haiku). Bedrock's G3/G4/G5 are kept unchanged (per-check discrimination, golden-tree fire proof, ≤10% universal-1.0 cap). |
| R9 | **second_sync scoring**: saturation-data's continuous `0.5·cond/reqs + 0.5·304/reqs` vs scorer-finesse's rung ladder inside `h_sync_discipline` | **both** | The continuous formula (evidence: the 0.4 rung fired zero times in 31 builds; a 30/33-conditional build scored the same 0.0 as a 0/9 build) becomes the "cheap" **component**; the compound structure (gate × min with idempotence and propagation) stays. Each source wins its half. |
| R10 | **P-tier fix**: saturation-data's "only workload scale (≥25k rows) can help" vs hard-spec's 1,553-row fixture | **hard-spec's fixture, saturation-data's principle** | 25k rows would blow the 90-s sync budget through the mock's rate-limit traps and stretch agent episodes past affordable calibration. The principle — measurements must land in the budget's dynamic range — is honored by *changing what P measures*: contended API latency (8 concurrent clients during a live sync) and frontend interaction latencies, which have real dynamic range at any row count. If calibration still shows >20× slack on any P rung, the fixture scales in sb-7; the G5/dead-check gate will say so. |
| R11 | **T-tier compounds**: scorer-finesse's `t_viz_truth` gate×min vs probe-3d's per-check ladders | **probe-3d's per-check structure** | The analytic checks are already individually gated against every named vacuous pass (visible-rect + draw-call gates, mono-wash cap, phantom-empty rule), and the data-binding *differential* (`t_data_bound` empty-vs-full) is strictly weaker than the analytic binding check (tops must match colors at positions computed *from the data*) — so the differential is superseded, and its phantom-data half moves into the empty-instance guard inside `t_scene_binding`. Compounds remain where partial-stacking was actually measured: HARD, J, C. |
| R12 | **Entrant pipeline**: task framing said `calibrate.py`; bedrock-calibration verified it drives the *old* single-file Meridian-client loop | **bedrock-calibration** | All sb-6 baselines run through `bench/run_build.py` (`goose run --provider aws_bedrock --model <id>`, token at `~/.config/agent-board/bedrock.env`, `BENCH_SPEC` + `BENCH_PRODUCT=1`). `calibrate.py` stays as the fast pre-flight loop for iterating individual vendor-API traps. |
| R13 | **j_sync_journey in the E gate** (scorer-finesse gates E on all-J-perfect; saturation-data proved `view_refreshed` unpassable) | **both, sequenced** | The journey is re-based first (half-seeded db, R-table above); only then may it gate E. An E gate containing a probe-broken check would lock the slice shut for reasons that are harness, not app — the exact validity failure sb-5's top band suffered. |

## 4. The frozen vocabulary (single source of truth — spec, probe, and scorer must all match this table verbatim)

**App**: Python package `vspro` (`vspro/meridian.py`, `store.py`, `api.py`, `web/`, `__main__.py`); run `python -m vspro --db PATH --port N`. Frontend files: `web/index.html`, `web/styles.css`, `web/app.js`, `web/viz.js`; combined ≤ **150 KB**.

**Endpoints**: `GET /api/health`, `GET /api/payments`, `GET /api/payments/<id>`, `GET /api/summary`, `GET /api/buckets`, `POST /api/sync`, `POST /api/payments/<id>/note`, `POST /api/payments/batch`, `POST /api/webhooks/meridian`.

**DOM ids/classes**: `#app-header`, `#summary`, `.cur-total[data-currency]`, `#last-sync`, `#sync-now`, `#status-filter`, `#currency-filter`, `#prev`, `#next`, `#notice`, `#viz3d`, `#viz-tooltip`, `#viz-toggle`, `#viz-fallback`, `#viz-empty`, `#viz-error`.

**Statuses (frozen order, j = 0..3)**: `settled #16A34A (22,163,74)`, `pending #F59E0B (245,158,11)`, `refunded #8B5CF6 (139,92,246)`, `failed #DC2626 (220,38,38)`. Side faces = round(0.62 × top). Clear color `#0F172A (15,23,42)`. Currencies: `EUR`(2), `USD`(2), `JPY`(0), `KWD`(3).

**Scene**: bar center `x_i = (i − (D−1)/2)·1.5` (i = day index, oldest = 0), `z_j = (j − 1.5)·1.5`; footprint 1.0×1.0, base y=0, `h = count·0.25`; zero-count cells draw nothing and are unpickable.

**Camera**: orbit; defaults `yaw 35°, pitch 27°, distance 30`; target `T = (0,3,0)`; `fovY 50°`, near 0.1, far 200; clamps pitch [5,85], distance [10,90]. Drag `yaw −= 0.35·Δx`, `pitch += 0.35·Δy` (clamped); wheel `distance ·= exp(0.0012·ΔY)` (clamped); dblclick resets. Projection basis and NDC formulas exactly as spec §5.

**Instrumentation**: `window.vsdbg = { version: 3, scene(), camera(), setCamera(), project(), pick(), frames() }`.

**Fixture**: 1,553 payments, 14 Europe/Berlin days spanning **2026-03-29** (DST), 4 statuses interleaved so the default-sort first page always shows ≥3, 4 currencies. `fixtures.py` exports `EXPECTED_TOTAL`, per-currency `EXPECTED_BY_CURRENCY`, `EXPECTED_BUCKETS`, and `OPTIMAL_REQUESTS` (computed from page size + trap overhead — never hand-written in a check).

**Probe**: scenarios `viz`, `viz-fallback` added to the existing `load|sync|error|empty`; env `BENCH_VIZ_BUCKETS` (expected buckets JSON); launch args (viz scenarios only) `--use-angle=swiftshader --enable-unsafe-swiftshader --force-color-profile=srgb --force-device-scale-factor=1 --js-flags=--random-seed=1357`; viewport 1280×800; `reducedMotion:'reduce'`; pixel tolerance ±8.

**Scorer**: `SCORER_VERSION = "sb-6.0"` (`sb-6.0-rcN` during calibration; rc never on a board). Tiers A B C D J V P **T** HARD (inner, ×0.88) + gate-locked **E** (0.12). Env gate `BENCH_SB6=1`.

## 5. Scoring architecture (summary; full skeleton in Artifact 4)

- **Inner weights** {A .06, B .12, C .12, D .10, J .12, V .08, P .06, T .14, HARD .20} × 0.88, plus E = 0.12 · gate · mean(E-checks). Import-time asserts: `T + HARD ≥ 0.34` and `E ≥ 0.10` — re-compressing the instrument requires deleting an assert, which no diff does quietly.
- **Every binary becomes a ladder** (full table in the scorer skeleton); no remaining binary is worth more than one rung.
- **Compounds** = gate × min(components); absorbed members become weight-0 diagnostics (disjointness asserted at import).
- **Calibration knobs**: `γ_core` (A–D, J, V), `γ_hard` (T, HARD) applied per check *before* the tier mean (Jensen punishes inconsistency — the axis F821 identified as real), `k_P` tightening P/value ladders. Grid-fit against Bedrock medians; γ hard-capped at 4.0 — a band unreachable inside the cap is a task-design defect, and the SPEC iterates, never the knobs.
- **E gate** (all simultaneously): all four J journeys == 1.0 (with `j_sync_journey` re-based per R13), zero console errors across every scenario incl. viz, `v_responsive_375` == 1.0, `v_dates_readable` == 1.0, all P at top rung, `t_scene_binding` == 1.0. E-checks: `e_frames_under_drag`, `e_under_load_latency`, `e_optimistic_paint`, `e_hard_mastery`.
- **Anti-gaming**: every new check ships its vacuous-pass counter (visible-rect+draw-call gates, mono-wash cap, phantom-empty-db zero, no-input-baseline subtraction for interaction deltas, draw-frames-only fps, completeness-gated efficiency and latency, 150 KB budget structurally blocking vendored 3D libs).

## 6. Calibration plan (merged protocol)

**Pipeline** (verified in-repo): `bench/run_build.py`, model ids `us.anthropic.claude-opus-5` / `us.anthropic.claude-sonnet-5` / `us.anthropic.claude-haiku-4-5-20251001-v1:0`; token `~/.config/agent-board/bedrock.env` (re-read per run; pre-flight freshness check each sweep); `BENCH_SPEC=$PWD/spec-build-v3.md BENCH_PRODUCT=1 BENCH_SB6=1`; `--timeout 2700 --port 8990`; serial; engine sha recorded before run 1 and verified before every run (no cargo — a live benchmark shares this machine); usage capture per run (wall-clock + turn count as proxy until goose emits tokens).

**Reps**: Opus **6** (even-n median, survives 2 aberrant runs), Sonnet **4**, Haiku **3** — 13 runs/sweep, justified by measured v2 variance (Opus sd ≈ .005, Sonnet ≈ .023, Haiku ≈ .026 with ~1-in-13 catastrophes). Catastrophic run (timeout / exit≠0 / <0.30 with produce-level root cause) → re-run once, record the failure; a second catastrophe stays in the fit. ~4–5 h wall and ~$25–50 list-equivalent per sweep; budget 2–3 iterations.

**Loop**: draft spec+rc checks → 1 Opus smoke → **controls first** (`bench/controls.py` with a v3 golden tree + defect set including ≥2 3D defects — mono-wash and readout-only-drag are the seeded lows; probe-3d's validation already proved the discriminating case: a sign-flipped camera scored tops 1/13, picks 0/5) → 13-run sweep → grid-fit knobs → gates:

- **G1** ordering: median(Opus) − median(Sonnet) ≥ 0.06; median(Sonnet) − median(Haiku) ≥ 0.10; per-model IQR ≤ 0.06.
- **G2** headroom: median(Opus) ∈ [0.72, 0.80]; > 0.84 ⇒ deepen the spec.
- **G3** resolution: ≥ ⅓ of graded checks show opus_med − haiku_med ≥ 0.15.
- **G4** no dead grader: every check fires on the golden tree (an observed zero licenses nothing — the negative-proof rule).
- **G5** free-check cap: ≤ 10% of checks are 1.0 for all 13 runs.

Any failure → amend the SPEC/checks, rc-bump, **full** re-sweep. All pass → **freeze**: `SCORER_VERSION="sb-6.0"`; `calib-sb6.json` (knobs + per-check medians + classifications + fit context: engine sha, model ids, date) committed with its **sha256 baked into score_build.py — the scorer refuses to score on mismatch**. Calibration-owned rungs frozen from the reference + cloud distribution: drag-frame floor, T pixel-fraction thresholds, k_P rung bounds, side-face achievable fraction.

**Drift protection** (each trigger names its response): any check/weight/fixture/spec/probe/mock change → version bump + full re-sweep; playwright/Chromium bump → offline golden-tree re-score must reproduce ±0.005; Bedrock alias drift → quarterly 3-rep Opus probe, drift > 0.05 → thresholds-only refit as sb-6.1; engine sha mismatch → anchors void; artifact tamper → structural refusal.

## 7. Migration & comparability policy vs sb-5.x

- **No numeric comparability, and none claimed.** An sb-6 number and an sb-5 number never share a table, chart, or magnitude sentence. Separate boards.
- The bridge is **ordinal only**: entrants on both boards (opus-5, sonnet-5, haiku-4.5, local single, swarm-N) get a rank-correlation note; expected order opus > sonnet > 3-node > haiku > 1-node — an inversion is a finding, not an embarrassment.
- sb-5 verdicts and `_sb4trees` archives stay frozen as the historical record; ROOT_BLOCKS partial-sync attribution is the one sb-5-lineage change and rides the sb-6 version.
- The local fleet re-enters under sb-6 with the same rep discipline as the cloud anchors (n ≥ 3, same catastrophe rule); `REGIME.env` flips `BENCH_SPEC` to `spec-build-v3.md` only after freeze.
- `BENCH_SB6` gating keeps the sb-5.3 path byte-identical until the flip.

## 8. Confidence statement (carried forward honestly, per-source)

**High**: SwiftShader determinism, preserveDrawingBuffer instrumentation, CSS↔backing mapping, the analytic project/occlude/sample/pick/drag pipeline (bit-identical reruns, discriminating on a real bug class); tier/weight/compound/knob arithmetic; pipeline mechanics and commands. **Calibration-owned, not hand-frozen**: the camera framing constants (fovY 50 / distance 30 / T=(0,3,0) frame the 14×4 scene by arithmetic — validate once against the reference implementation before freezing), the 24-frames/2s drag floor under SwiftShader, T pixel-fraction cut points, the side-face achievable fraction (antialiased-edge misses measured at 1/12, deterministic). **Known gap**: the scenario glue composes verified pieces but has not run against a full v3 app, because none exists — building the reference implementation and the two seeded low-controls is the mandatory next step before any threshold is trusted. **Medium**: token/cost figures (usage capture replaces them after sweep 1) and the exact acceptance bands (the loop adjusts the spec to the bands, never the reverse).

---
---

# ARTIFACT 2 — spec-build-v3.md (final draft)

# Build `vspro` — VendorSync Pro

An operations product that syncs payments from the Meridian API, keeps them consistent through
vendor-pushed webhooks and concurrent edits, and gives a finance team a live view of the money —
including an interactive 3D chart of payment activity.

The Meridian API v2 documentation is at `{DOCS_URL}`. Read it before you start — every behaviour
you must handle is documented there, and several of them (rate limits, expired cursors, version
conflicts, webhook signatures) will defeat a client that did not read. Base URL `{BASE_URL}`,
API key `{API_KEY}`.

Work in the current directory. Python 3, standard library only for the backend — no pip installs
(`sqlite3`, `zoneinfo`, `hashlib`, `hmac` are all in the standard library). The frontend ships
ZERO external code — no CDN, no npm, no vendored libraries of any kind. Everything must work
fully offline.

---

## What to build

### 1. `vspro/meridian.py` — the vendor client

```python
class MeridianClient:
    def __init__(self, base_url: str, api_key: str) -> None
    def fetch_all_payments(self) -> list[dict]        # every payment, oldest first by instant
    def get_payment(self, payment_id: str) -> dict    # single resource; includes "version"
    def total_count(self) -> int                      # how many payments exist in the collection
    def create_payment(self, value_minor: int, currency: str, counterparty: dict,
                       occurred_at: str, idempotency_key: str) -> str
    def create_batch(self, items: list[dict]) -> list[dict]   # per-item results, input order kept
    def update_payment(self, payment_id: str, fields: dict, version: int) -> dict
    def register_webhook(self, url: str) -> dict      # {"id": ..., "secret": ...}
```

- `create_payment` returns the payment id and is safe to call more than once with the same key.
- `update_payment` sends the documented `If-Match` header built from `version`. When the vendor
  answers `412 Precondition Failed`, the client recovers as the docs prescribe: re-fetch the
  resource, re-apply `fields` on the fresh version, retry ONCE with the new `If-Match`. A second
  412 is surfaced to the caller as a conflict error. It never writes without `If-Match` — the
  vendor answers `428 Precondition Required` if you try, and that response is a bug in your
  client, not a retry case.
- `create_batch` submits up to 20 create operations in one request. The vendor applies them
  independently and reports per-item outcomes; one failed item must NOT discard, retry, or
  re-submit the items that succeeded.
- `register_webhook` is idempotent by URL: registering a URL the vendor already knows returns
  the SAME id and secret. The vendor verifies the URL with a challenge handshake during the
  call — your server must already be listening when you register.
- Pagination, `Retry-After` in both documented forms, `410 cursor_expired` restart, `ETag` /
  `If-None-Match` conditional requests: all exactly as documented, all mandatory.

### 2. `vspro/store.py` — local persistence

```python
class Store:
    def __init__(self, path: str) -> None
    def upsert_many(self, payments: list[dict]) -> tuple[int, int]   # (inserted, updated)
    def query(self, limit: int, offset: int, status: str | None = None,
              currency: str | None = None, sort: str = "created_at") -> tuple[list[dict], int]
    def get(self, payment_id: str) -> dict | None
    def apply_event(self, event: dict) -> str    # "applied" | "duplicate" | "stale"
    def buckets(self) -> list[dict]              # day x status counts, Europe/Berlin days
    def count(self) -> int
    def last_sync(self) -> str | None            # RFC3339 UTC, or None if never synced
    def set_last_sync(self, when: str) -> None
```

Persist to SQLite at the given path. A payment already present is updated, never duplicated —
syncing twice must not change the count. `query` returns `(rows, total)` where `total` counts the
rows matching the FILTERS, not the whole table.

`apply_event` is the webhook consumer. It must be idempotent and ordered:

- an event id already processed → `"duplicate"`, state untouched;
- an event whose payment `version` is not greater than the stored version → `"stale"`, state
  untouched — the vendor does not guarantee delivery order and an old event must never overwrite
  a newer row;
- otherwise the payment row is updated and the event id recorded → `"applied"`.

`buckets` returns one cell per (day, status) pair covering every calendar day the data spans, in
the **Europe/Berlin** timezone. The day a payment belongs to is the Berlin calendar date of its
`created_at` INSTANT — not of its raw string, and not the UTC date. The fixture spans the
2026-03-29 DST transition on purpose; UTC-day bucketing produces measurably wrong counts.

### 3. `vspro/api.py` — the HTTP backend

`serve(port: int, store: Store, client: MeridianClient)` starts a JSON API on `127.0.0.1:port`:

| Method | Path | Response |
|---|---|---|
| `GET` | `/api/health` | see shape below |
| `GET` | `/api/payments?limit=<int>&offset=<int>&status=<s>&currency=<c>&sort=<k>` | `{"data": [...], "total": <int>, "limit": <int>, "offset": <int>}` |
| `GET` | `/api/payments/<id>` | the payment, or 404 envelope |
| `GET` | `/api/summary` | see shape below |
| `GET` | `/api/buckets` | see shape below |
| `POST` | `/api/sync` | `{"fetched": <int>, "inserted": <int>, "updated": <int>, "total": <int>}` |
| `POST` | `/api/payments/<id>/note` | `{"id": <str>, "note": <str>, "version": <int>}` |
| `POST` | `/api/payments/batch` | `{"results": [...], "succeeded": <int>, "failed": <int>}` |
| `POST` | `/api/webhooks/meridian` | vendor-facing; see section 4 |

**Health.**

```json
{"status": "ok", "payments": <int>, "last_sync": <str or null>,
 "webhook": {"registered": <bool>, "received": <int>, "applied": <int>,
             "ignored": <int>, "rejected": <int>}}
```

The four webhook counters are live evidence: `received` counts every POST that reached the
endpoint (valid or not), `applied` / `ignored` / `rejected` follow the definitions in section 4.

**Payments.** `limit` defaults to 50 and is capped at 200. `offset` defaults to 0. `data` items
carry exactly the keys `id`, `amount_minor`, `currency`, `created_at`, `settled_at`, `status`,
`version`, `note`, `counterparty_name`, `country` — the vendor's nested `counterparty` object is
flattened into the last two. `status` filters to one of `settled`, `pending`, `refunded`,
`failed`; `currency` filters to one of `EUR`, `USD`, `JPY`, `KWD`; the two combine. `sort` is one
of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`; default `created_at` (ascending
by INSTANT). `total` always reflects the active filters. An unknown `status`, `currency` or
`sort` value is a validation error, not an empty result.

**Summary.**

```json
{"count": <int>, "last_sync": <str or null>, "oldest": <str or null>, "newest": <str or null>,
 "by_currency": [{"currency": "EUR", "count": <int>, "total_minor": <int>}, ...]}
```

`by_currency` is sorted by currency code ascending and contains one entry per currency present.
There is NO cross-currency total anywhere in the response — summing minor units across currencies
is meaningless and forbidden. `oldest` / `newest` are `created_at` of the earliest and latest
payments as RFC3339 **UTC**.

**Buckets.**

```json
{"timezone": "Europe/Berlin",
 "days": ["2026-03-23", "..."],
 "statuses": ["settled", "pending", "refunded", "failed"],
 "cells": [{"day": "2026-03-23", "status": "settled", "count": <int>}, ...]}
```

`days` is every calendar day from the first to the last, ascending, no gaps. `cells` contains
one entry for EVERY (day, status) pair — `days x statuses`, count 0 included — ordered day-major,
statuses in the frozen order above.

**Note.** `POST /api/payments/<id>/note` with body `{"note": <str>}` (1–280 chars) writes the
note through to the vendor with `update_payment` — full optimistic-concurrency dance included —
then persists the returned resource locally and responds with the new `version`. If the conflict
cannot be resolved (a second 412), respond `409` with the error envelope, code `"conflict"`, and
leave the local row unchanged.

**Batch.** `POST /api/payments/batch` with body:

```json
{"items": [{"amount": {"value_minor": <int>, "currency": <str>},
            "counterparty": {"name": <str>, "country": <str>},
            "occurred_at": <rfc3339>, "idempotency_key": <str>}, ...]}
```

Validate shape locally first (1–20 items; `value_minor` a positive integer; `currency` in the
supported set; `country` exactly two uppercase letters; `name` 1–80 chars; `occurred_at`
RFC3339; `idempotency_key` non-empty). Shape-valid batches are forwarded via `create_batch`;
the vendor may still fail individual items on business rules (the docs name the per-payment
amount limit). Respond 200 with per-item results in input order:

```json
{"results": [{"index": 0, "status": "created", "id": "pay_..."},
             {"index": 1, "status": "error",
              "error": {"code": "amount_over_limit", "message": "..."}}],
 "succeeded": <int>, "failed": <int>}
```

Partial failure is a NORMAL outcome: succeeded items stay succeeded, failed items report their
own error, and nothing is retried with a fresh key.

**Error envelope.** Every error this API returns uses ONE structured envelope:

```json
{"error": {"code": "<snake_case>", "message": "<human sentence>",
           "field_errors": [{"path": "items[2].amount.value_minor", "code": "not_an_integer"}]}}
```

`field_errors` appears only on validation failures (HTTP 400) and uses dot paths with `[index]`
for arrays. Frozen `code` vocabulary for field errors: `required`, `not_an_integer`,
`not_positive`, `unsupported`, `too_long`, `bad_format`. Envelope codes: `bad_request`,
`not_found`, `conflict`, `bad_signature`, `vendor_unavailable`. An unknown path is 404 with the
envelope, code `"not_found"`. A bad `limit`/`offset` — non-numeric or negative — is 400 with a
`field_errors` entry naming the parameter. Every response is JSON except the static frontend
assets.

### 4. Webhooks — the vendor calls YOU

On startup, AFTER the server is bound and listening, the app registers
`http://127.0.0.1:<port>/api/webhooks/meridian` with the vendor via `register_webhook`.
Registration triggers the documented challenge handshake: the vendor POSTs
`{"type": "webhook.verify", "challenge": "<hex>"}` to the URL and the endpoint must answer
`200` with `{"challenge": "<the same hex>"}` — this request is unsigned, because the secret does
not exist until registration completes.

Every subsequent delivery is a signed event:

```json
{"id": "evt_00c4", "type": "payment.updated", "created_at": "<rfc3339 UTC>",
 "data": { <the full payment object, including "version"> }}
```

with header `Meridian-Signature: t=<unix seconds>,v1=<hex>` where
`v1 = HMAC_SHA256(secret, "<t>.<raw request body>")` — the raw bytes, not a re-serialization.

The endpoint must, deterministically:

- verify the signature FIRST; missing or wrong → `401` with the envelope, code
  `"bad_signature"`, state untouched, `rejected` +1;
- pass valid events to `Store.apply_event`: `"applied"` → `applied` +1, `"duplicate"` or
  `"stale"` → `ignored` +1; respond `200 {"received": true}` in all three cases;
- count every arrival in `received`, valid or not;
- answer within 3 seconds and never trigger a sync or any vendor call from inside the handler.

The vendor WILL deliver duplicates, WILL deliver events out of order, and WILL (once) deliver a
forged signature. The four health counters are the ledger of how the app handled all of it.

### 5. `vspro/web/` — the frontend

A single page, served by the backend at `GET /`. Plain HTML/CSS/JS, no build step, no CDN, no
external code of any kind — it must work offline. This page is what the finance team uses every
day. Build it as a product, not as a debug view over the API.

Ship it as FOUR files, each owned and written separately: `web/index.html` (structure only),
`web/styles.css` (all styling), `web/app.js` (page behavior: table, filters, sync, notes), and
`web/viz.js` (the 3D engine, nothing else). The backend serves all four with correct content
types; the page references them with relative paths. Combined size of the four files: at most
**150 KB** uncompressed — hand-written code fits in a tenth of that; the budget exists so that a
vendored library cannot.

The page shows, top to bottom: a branded header bar (`#app-header`) carrying the app name; the
summary; the 3D visualization panel; the payments table.

**Summary** (`#summary`). One element per currency present, class `cur-total`, attribute
`data-currency`, showing the payment count and the total formatted in that currency. Never a
combined cross-currency figure. The last-sync time (`#last-sync`) reads human, or `Never synced`
when there is none. A **Sync now** button (`#sync-now`) calls `POST /api/sync`, shows a visible
in-flight state (`data-state="syncing"`, control disabled), and refreshes every view on
completion.

**Table.** Columns Date, Amount, Status, Counterparty, Note. Server-driven through the
documented `limit`/`offset`/`status`/`currency`/`sort` parameters — the page never fetches the
whole collection to paginate in memory, and never renders all rows in one scroll when more than
50 exist.

- **Pagination:** **Prev**/**Next** buttons (`#prev`, `#next`) and a `showing X–Y of TOTAL`
  readout, where TOTAL is the filtered total.
- **Sorting:** the Date and Amount column headers are clickable and toggle ascending/descending,
  reflected in `aria-sort` on the header cell and driven through the API's `sort` parameter.
- **Filters:** a status filter (`#status-filter`) and a currency filter (`#currency-filter`),
  each a custom dropdown (never a native `<select>`), each actually changing the rows AND the
  TOTAL readout.
- **Status badges:** `settled` `#16A34A`, `pending` `#F59E0B`, `refunded` `#8B5CF6`, `failed`
  `#DC2626` — the same four hex values the 3D chart uses. Distinct in computed color, not only
  in text.
- **Notes, optimistically:** each row's Note cell is editable through a custom inline editor
  (never `prompt()`). On confirm the new value paints IMMEDIATELY — before the network responds
  — with the row in `data-state="saving"`; success moves it to `data-state="saved"`; a `409`
  reverts the cell to the previous value and shows a non-blocking notice in `#notice`
  (`role="status"`), never an `alert()`.

**The 3D visualization.** An interactive 3D bar chart of payment activity: one bar per
(day, status) bucket from `GET /api/buckets`, rendered with **raw WebGL** — a `<canvas
id="viz3d">` with a `webgl` or `webgl2` context, created with `{antialias: false, alpha: false}`.
No three.js, no library, no exceptions; the asset budget enforces it. The harness browser
provides WebGL; section *2D fallback* below defines what happens when a browser does not.
Every contract below is FROZEN — the grader recomputes this math independently and compares it
to your API, your pixels, and your picking.

*Scene contract.* Right-handed world, +Y up, units are world units.

- Day index `i` (0 = oldest day) → bar center `x_i = (i − (D−1)/2) · 1.5` where `D` is the day
  count. Status index `j` (frozen order `settled`=0, `pending`=1, `refunded`=2, `failed`=3) →
  bar center `z_j = (j − 1.5) · 1.5`.
- Each bar is an axis-aligned box: footprint 1.0 x 1.0 centered at `(x_i, z_j)`, base at `y = 0`,
  height `h = count · 0.25`. A zero-count cell draws NO geometry and is never pickable.
- Flat, unlit colors. Top face: EXACTLY the status hex above. Side faces: the same color with
  each channel multiplied by 0.62 and rounded. Background clear color: `#0F172A`. Depth testing
  on — near bars occlude far bars.
- The scene is STATIC between inputs: no idle animation. Frames are drawn on load, on input,
  and on data change.

*Camera contract.* An orbit camera, angles in degrees:

```
θ = yaw · π/180        φ = pitch · π/180        T = (0, 3, 0)
eye = T + distance · ( cos φ · sin θ,  sin φ,  cos φ · cos θ )
f = normalize(T − eye)    r = normalize(f x (0,1,0))    u = r x f
```

For a world point `p`: `q = p − eye`, `xc = q·r`, `yc = q·u`, `zc = q·f`. Points with
`zc ≤ 0.1` do not project. With `fovY = 50°`, `k = 1 / tan(fovY/2)`,
`aspect = Wcss / Hcss` of the canvas:

```
ndcx = (k / aspect) · xc / zc        ndcy = k · yc / zc
sx = (ndcx + 1) / 2 · Wcss           sy = (1 − ndcy) / 2 · Hcss
```

`sx, sy` are CSS pixels relative to the canvas's top-left. GL near/far planes: 0.1 / 200. The
canvas backing store is sized `clientWidth x devicePixelRatio` (likewise height); the rendered
image must agree with this projection at any DPR. Defaults: `yaw = 35`, `pitch = 27`,
`distance = 30`. Clamps: pitch `[5, 85]`, distance `[10, 90]`; yaw unbounded.

*Interaction contract.*

- Pointer drag on the canvas, per move event with CSS-pixel deltas `Δx, Δy`:
  `yaw ← yaw − 0.35·Δx`, `pitch ← clamp(pitch + 0.35·Δy, 5, 85)`.
- Wheel: `distance ← clamp(distance · exp(0.0012 · deltaY), 10, 90)`.
- Double-click: reset to the defaults.
- Hover: within 150 ms of the pointer resting on a bar, a tooltip `#viz-tooltip` appears near
  the cursor with the text `<count> <status> · <day>` — the day human-readable, e.g.
  `12 settled · 29 Mar 2026`. Off a bar, the tooltip hides.
- Click on a bar: sets `#status-filter` to that bar's status and the table refreshes to match.

*Picking* is geometric truth: the bar whose rendered surface is nearest the camera at that CSS
pixel — a partially occluded bar loses to the bar in front of it, exactly as the depth buffer
says.

*2D fallback.* A toggle button `#viz-toggle` (with `aria-pressed`) swaps the canvas for a real
`<table id="viz-fallback">` of the same buckets — one row per day, columns Day, Settled,
Pending, Refunded, Failed, Total, each count cell carrying `data-day` and `data-status` — and
back. The two views always agree because both read `/api/buckets`. If `getContext('webgl')`
(and `webgl2`) returns null — a machine without WebGL — the page must not throw: it shows the
same 2D table automatically, with a visible notice that 3D is unavailable, and every other part
of the page keeps working.

*Instrumentation contract* — REQUIRED and graded. The page exposes `window.vsdbg`:

```js
window.vsdbg = {
  version: 3,                                    // the literal number 3
  scene(),      // {days: [...], statuses: [...], bars: [{key, i, j, count, x, z, h}, ...]}
                //   key = "<YYYY-MM-DD>|<status>"; zero-count cells omitted
  camera(),     // {yaw, pitch, distance} — live values, degrees
  setCamera({yaw, pitch, distance}),             // applies clamps, renders
  project(x, y, z),  // [sx, sy] CSS px per the camera contract, or null if zc <= 0.1
  pick(sx, sy),      // bar key or null, occlusion-correct
  frames(),          // total frames drawn since load, monotonically increasing
};
```

`vsdbg` must tell the truth: `scene()` agrees with `/api/buckets`, `project()` agrees with the
pixels actually on the canvas, `pick()` agrees with what a user's hover hits. The grader
cross-checks all three against screenshots — an instrumentation layer that reports a scene the
canvas does not show scores as broken, not as clever.

**Dates.** Every timestamp a user sees is rendered human-readable in the user's locale — e.g.
`1 Mar 2026, 14:00`. A raw ISO-8601 string with an offset must never appear in the rendered
page. This covers the Date column, the tooltip, and the last-sync time alike.

**Money.** Amounts render in each row's OWN currency with that currency's minor-unit exponent:
`EUR` and `USD` have 2 decimals, `JPY` has 0, `KWD` has 3. `129900 EUR → €1,299.00`;
`129900 JPY → ¥129,900`; `129900 KWD → KWD 129.900`. A yen amount with two decimals, or a dinar
truncated to two, is wrong money, and money is the product.

**States.** The page handles, visibly and distinctly: **loading**, **empty** (no payments yet —
with a call to sync), and **error** (backend unreachable or erroring — with text a user can act
on). The viz panel additionally owns its own states: `#viz-empty` when every bucket is zero,
`#viz-error` when `/api/buckets` fails. Never a blank panel, never a spinner that never
resolves.

**Responsive.** At a viewport 375 px wide the page lays out cleanly with no horizontal scroll;
the canvas shrinks to full width (min height 240 px) and stays interactive.

**Design.** The page has an intentional visual design: a real palette with strong solid accent
colors, a clear typographic hierarchy, and a branded header bar carrying the app name. Never use
faded pastel washes — pick saturated, solid colors over tints. Never decorate cards or rows with
a left accent line or rail. Never use browser-native controls where custom styling is expected —
no default `<select>`, no `alert()`/`confirm()`/`prompt()` dialogs.

### 6. `vspro/__main__.py` — the entry point

`python -m vspro --db PATH --port N` starts the backend serving the API and the page, then —
after the server is listening — registers the webhook with the vendor. It must not crash when
the database file does not yet exist, and must start (serving whatever is already local) even if
the vendor is briefly unreachable at boot.

### 7. Performance budgets

Measured against the vendor's 1,553-row fixture (14 Berlin days spanning the 2026-03-29 DST
switch, 4 statuses, 4 currencies), on the machine the build runs on:

- First data rows rendered within **2 seconds** of page load.
- The 3D canvas shows its first non-background frame within **3 seconds** of page load.
- `GET /api/payments` at `limit=50` answers in under **150 ms** at p95 — including while a sync
  is running with 8 concurrent readers.
- `GET /api/buckets` answers in under **200 ms** at p95.
- `GET /api/summary` answers in under **150 ms** at p95.
- `POST /api/sync` completes the full fixture within **90 seconds**, documented waits included.
- During a scripted 2-second drag, `vsdbg.frames()` advances by at least **24**, and a camera
  change is visible on the canvas within **250 ms** of the input that caused it.
- The hover tooltip appears within **150 ms**.
- An optimistic note edit paints the new value within **100 ms** of confirm — before the network
  responds.
- `index.html` + `styles.css` + `app.js` + `viz.js` total at most **150 KB** uncompressed.

---

## Rules

- Amounts are integers in minor units everywhere, end to end. Never floats. Rendering respects
  each currency's minor-unit exponent (EUR 2, USD 2, JPY 0, KWD 3).
- Never sum amounts across currencies — not in the API, not in the UI, not in a tooltip.
- Sorting, comparing and bucketing times happens on the INSTANT, not on the string. Buckets use
  the Europe/Berlin calendar day of the instant.
- Every write to a vendor resource carries `If-Match`. A 412 means someone got there first:
  re-fetch, re-apply, retry once. Never blind-write, never retry a create with a fresh
  idempotency key.
- Webhook deliveries are untrusted input until the signature verifies against the RAW request
  body. Duplicates and stale events are normal traffic, silently ignored, and counted.
- The tool runs repeatedly against the same database; a second sync must be cheap (conditional
  requests) and must not duplicate rows or regress webhook-applied versions.
- The frontend ships zero external code and works fully offline. The backend is standard library
  only.
- Include a README.md with the exact commands to install nothing, run the server, and sync.

---
---

# ARTIFACT 3 — probe extension (drop into `bench/product_probe.mjs`)

Integration notes: (1) line 45's whitelist gains `'viz', 'viz-fallback'`; (2) the launch call adds `VIZ_LAUNCH_ARGS` and the `glInstrument`/`glKill` init scripts only when `scenario.startsWith('viz')`; (3) `_product_probe` in the scorer gains an `env=` passthrough for `BENCH_VIZ_BUCKETS`; (4) `saveShot('viz')` / `saveShot('viz-fallback')` keep the quality-screenshot contract. All page-side functions are self-contained (the file's existing style). The math and sampling pipeline below is the empirically verified probe-3d machinery, re-parameterized to the frozen v3 contract (canvas `#viz3d`, 4 statuses, bucket grid, yaw/pitch camera with target (0,3,0), fovY 50, side ×0.62, clear #0F172A, measured-rect aspect).

```js
// ── sb-6 viz: pinned scene contract (must mirror spec-build-v3 §5 verbatim) ──────────────────
const VIZ = {
  fovYDeg: 50, near: 0.1, far: 200,
  yaw0: 35, pitch0: 27, dist0: 30, target: [0, 3, 0],
  dragDegPerPx: 0.35, wheelK: 0.0012,
  pitchMin: 5, pitchMax: 85, distMin: 10, distMax: 90,
  cellPitch: 1.5, half: 0.5, hPerCount: 0.25,
  bg: [15, 23, 42],                                     // #0F172A
  statusOrder: ['settled', 'pending', 'refunded', 'failed'],
  status: { settled: [22, 163, 74], pending: [245, 158, 11],
            refunded: [139, 92, 246], failed: [220, 38, 38] },
  sideFactor: 0.62, tol: 8,
};
const VIZ_LAUNCH_ARGS = [
  '--use-angle=swiftshader', '--enable-unsafe-swiftshader',   // deterministic software GL (verified)
  '--force-color-profile=srgb', '--force-device-scale-factor=1',
  '--js-flags=--random-seed=1357',
];
const sideColor = (c) => c.map((v) => Math.round(v * VIZ.sideFactor));

// ── vector / projection math (verified pipeline, spec-basis form) ────────────────────────────
const deg = (d) => (d * Math.PI) / 180;
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (a) => { const l = Math.hypot(...a); return [a[0] / l, a[1] / l, a[2] / l]; };

function cameraEye(yaw, pitch, dist) {
  const t = deg(yaw), p = deg(pitch), T = VIZ.target;
  return [T[0] + dist * Math.cos(p) * Math.sin(t),
          T[1] + dist * Math.sin(p),
          T[2] + dist * Math.cos(p) * Math.cos(t)];
}
function cameraBasis(eye) {
  const f = norm(sub(VIZ.target, eye));
  const r = norm(cross(f, [0, 1, 0]));
  const u = cross(r, f);
  return { f, r, u };
}
// world point → CSS px inside the canvas (per the spec's exact formulas), or null if zc <= 0.1
function projectPt(eye, basis, W, H, p) {
  const q = sub(p, eye);
  const xc = dot(q, basis.r), yc = dot(q, basis.u), zc = dot(q, basis.f);
  if (zc <= VIZ.near) return null;
  const k = 1 / Math.tan(deg(VIZ.fovYDeg) / 2), aspect = W / H;
  return { x: ((k / aspect) * (xc / zc) + 1) / 2 * W,
           y: (1 - k * (yc / zc)) / 2 * H };
}
function rayAABB(o, d, mn, mx) {                        // slab test: entry distance, or null
  let t0 = -Infinity, t1 = Infinity;
  for (let k = 0; k < 3; k++) {
    if (Math.abs(d[k]) < 1e-9) { if (o[k] < mn[k] || o[k] > mx[k]) return null; continue; }
    let a = (mn[k] - o[k]) / d[k], b = (mx[k] - o[k]) / d[k];
    if (a > b) [a, b] = [b, a];
    t0 = Math.max(t0, a); t1 = Math.min(t1, b);
  }
  return t0 <= t1 && t1 > 0 ? Math.max(t0, 0) : null;
}
// Bars from the expected /api/buckets payload (BENCH_VIZ_BUCKETS): zero-count cells draw nothing.
function barGeom(buckets) {
  const D = buckets.days.length, bars = [];
  for (const cell of buckets.cells) {
    if (!cell.count) continue;
    const i = buckets.days.indexOf(cell.day);
    const j = VIZ.statusOrder.indexOf(cell.status);
    const x = (i - (D - 1) / 2) * VIZ.cellPitch;
    const z = (j - 1.5) * VIZ.cellPitch;
    const h = cell.count * VIZ.hPerCount;
    bars.push({ key: `${cell.day}|${cell.status}`, day: cell.day, status: cell.status,
                count: cell.count, i, j, x, z, h,
                mn: [x - VIZ.half, 0, z - VIZ.half], mx: [x + VIZ.half, h, z + VIZ.half] });
  }
  return bars;
}
function firstHit(eye, P, bars) {                       // nearest box on the forward ray eye→P→∞
  const d = norm(sub(P, eye));
  let best = null;
  bars.forEach((b, i) => {
    const t = rayAABB(eye, d, b.mn, b.mx);
    if (t != null && (best === null || t < best.t)) best = { index: i, t };
  });
  return best;
}
// Every sample with its analytic expectation. Occlusion-filtered, deterministic (verified).
function buildVizSamples(buckets, yaw, pitch, dist, W, H) {
  const bars = barGeom(buckets);
  const eye = cameraEye(yaw, pitch, dist), basis = cameraBasis(eye);
  const proj = (p) => projectPt(eye, basis, W, H, p);
  const inCanvas = (p) => p && p.x >= 2 && p.x <= W - 3 && p.y >= 2 && p.y <= H - 3;
  const tops = [], above = [], sides = [], occludedTops = [];
  bars.forEach((b, i) => {
    const T = [b.x, b.h, b.z], pT = proj(T);
    if (inCanvas(pT)) {
      const hit = firstHit(eye, T, bars), dT = Math.hypot(...sub(T, eye));
      if (hit && hit.index !== i && hit.t < dT - 1e-6)
        occludedTops.push({ i, key: b.key, css: pT, occluder: hit.index });
      else
        tops.push({ i, key: b.key, status: b.status, cx: pT.x, cy: pT.y,
                    expect: VIZ.status[b.status], kind: 'top' });
    }
    const Q = [b.x, b.h + 0.6, b.z], pQ = proj(Q);       // just above the top: must be sky
    if (inCanvas(pQ) && !firstHit(eye, Q, bars))
      above.push({ i, cx: pQ.x, cy: pQ.y, expect: VIZ.bg, kind: 'above' });
    const S = [b.x, 0.6 * b.h, b.z], pS = proj(S);       // inside the column: its own surface
    const hS = inCanvas(pS) && firstHit(eye, S, bars);
    if (pS && hS && hS.index === i)
      sides.push({ i, cx: pS.x, cy: pS.y,
                   expectAny: [sideColor(VIZ.status[b.status]), VIZ.status[b.status]], kind: 'side' });
  });
  const sky = [];                                        // background over the grid gaps
  const D = buckets.days.length;
  const maxH = Math.max(0.25, ...bars.map((b) => b.h));
  for (const [gx, gz] of [[-0.3, -0.35], [0.3, -0.35], [-0.3, 0.35], [0.3, 0.35], [0, 0]]) {
    const G = [gx * D * VIZ.cellPitch, maxH + 2.0, gz * 4 * VIZ.cellPitch];
    const p = proj(G);
    if (inCanvas(p) && !firstHit(eye, G, bars)) sky.push({ cx: p.x, cy: p.y, expect: VIZ.bg, kind: 'sky' });
  }
  const corners = [[3, 3], [W - 4, 3], [3, H - 4], [W - 4, H - 4]]
    .map(([cx, cy]) => ({ cx, cy, expect: VIZ.bg, kind: 'corner' }));
  const grid = [];                                       // blind coverage grid, no expectation
  for (let ix = 0; ix < 6; ix++) for (let iy = 0; iy < 4; iy++)
    grid.push({ cx: (ix + 0.5) * W / 6, cy: (iy + 0.5) * H / 4, kind: 'grid' });
  return { bars, eye, tops, above, sides, sky, corners, grid, occludedTops };
}

// ── page-side init scripts (VERIFIED mechanisms) ─────────────────────────────────────────────
// viz: force preserveDrawingBuffer (readPixels-after-composite trap), record context
// acquisitions, count draw calls, watch for context loss.
function glInstrument() {
  window.__probeViz = { contexts: [], drawCalls: 0, contextLost: 0 };
  const wrap = (proto, offscreen) => {
    const orig = proto.getContext;
    proto.getContext = function (type, attrs) {
      if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
        attrs = Object.assign({}, attrs || {}, { preserveDrawingBuffer: true });
        const gl = orig.call(this, type, attrs);
        if (gl && !gl.__probeSeen) {
          gl.__probeSeen = true;
          window.__probeViz.contexts.push({ type, offscreen, canvasId: this.id || null });
          for (const fn of ['drawArrays', 'drawElements', 'drawArraysInstanced', 'drawElementsInstanced'])
            if (typeof gl[fn] === 'function') {
              const d = gl[fn].bind(gl);
              gl[fn] = (...a) => { window.__probeViz.drawCalls++; return d(...a); };
            }
          if (this.addEventListener)
            this.addEventListener('webglcontextlost', () => window.__probeViz.contextLost++);
        }
        return gl;
      }
      return orig.call(this, type, attrs);
    };
  };
  wrap(HTMLCanvasElement.prototype, false);
  if (typeof OffscreenCanvas !== 'undefined') wrap(OffscreenCanvas.prototype, true);
}
// viz-fallback: WebGL unavailable, canvas-2D alive (verified: leaves 2D contexts working).
function glKill() {
  const kill = (proto) => {
    const orig = proto.getContext;
    proto.getContext = function (type, ...rest) {
      if (/webgl/i.test(String(type))) return null;
      return orig.call(this, type, ...rest);
    };
  };
  kill(HTMLCanvasElement.prototype);
  if (typeof OffscreenCanvas !== 'undefined') kill(OffscreenCanvas.prototype);
}

// ── page-side sampler: one readPixels, indexed lookups; CSS→backing + y-flip verified ────────
function pageSampleScene(arg) {
  const canvas = document.getElementById('viz3d');
  if (!canvas) return { found: false };
  const rect = canvas.getBoundingClientRect();
  const gl = canvas.getContext('webgl2') || canvas.getContext('webgl') ||
             canvas.getContext('experimental-webgl');
  if (!gl) return { found: true, glReadable: false, rect: { w: rect.width, h: rect.height } };
  const W = canvas.width, H = canvas.height;
  const px = new Uint8Array(W * H * 4);
  gl.readPixels(0, 0, W, H, gl.RGBA, gl.UNSIGNED_BYTE, px);
  const at = (cx, cy) => {
    const bx = Math.min(W - 1, Math.max(0, Math.round(cx * (W / rect.width))));
    const by = Math.min(H - 1, Math.max(0, H - 1 - Math.round(cy * (H / rect.height))));
    const o = (by * W + bx) * 4;
    return [px[o], px[o + 1], px[o + 2]];
  };
  const near = (a, b, t) => Math.abs(a[0] - b[0]) <= t && Math.abs(a[1] - b[1]) <= t && Math.abs(a[2] - b[2]) <= t;
  const out = [];
  for (const s of arg.samples) {
    const got = at(s.cx, s.cy);
    let ok = null;
    if (s.expect) ok = near(got, s.expect, arg.tol);
    else if (s.expectAny) ok = s.expectAny.some((e) => near(got, e, arg.tol));
    out.push({ kind: s.kind, i: s.i, key: s.key, got, ok });
  }
  return { found: true, glReadable: true,
           rect: { left: rect.left, top: rect.top, w: rect.width, h: rect.height },
           backing: { w: W, h: H }, dpr: window.devicePixelRatio,
           instrument: window.__probeViz || null, samples: out };
}

// ── page-side helpers ────────────────────────────────────────────────────────────────────────
function pageVizReady() {
  const v = window.__probeViz || { drawCalls: 0, contexts: [] };
  const c = document.getElementById('viz3d');
  const d = window.vsdbg;
  return { canvas: !!c, drawCalls: v.drawCalls, contexts: v.contexts.length,
           vsdbg: !!d, vsdbgVersion: d ? d.version : null };
}
function pageVsdbgSnapshot(arg) {           // scene + camera + frames + spot projections/picks
  const d = window.vsdbg;
  if (!d) return null;
  const safe = (fn) => { try { return fn(); } catch (e) { return { __err: String(e).slice(0, 80) } } };
  return {
    version: d.version,
    camera: safe(() => d.camera()),
    frames: safe(() => d.frames()),
    scene: safe(() => { const s = d.scene(); return { days: s.days, statuses: s.statuses,
      bars: s.bars.map((b) => ({ key: b.key, count: b.count, x: b.x, z: b.z, h: b.h })) }; }),
    projections: (arg.points || []).map((p) => safe(() => d.project(p[0], p[1], p[2]))),
    picks: (arg.picks || []).map((q) => safe(() => d.pick(q[0], q[1]))),
  };
}
function pageStatusFilterValue() {
  const el = document.getElementById('status-filter');
  if (!el) return null;
  return (el.getAttribute('data-value') || el.getAttribute('aria-label') || el.textContent || '')
    .trim().toLowerCase().slice(0, 40);
}
function pageTooltipState() {
  const el = document.getElementById('viz-tooltip');
  if (!el || !el.getClientRects().length) return { visible: false };
  const cs = getComputedStyle(el);
  if (cs.visibility === 'hidden' || cs.display === 'none' || +cs.opacity === 0) return { visible: false };
  return { visible: true, text: (el.textContent || '').trim().slice(0, 80) };
}
function pageFallbackTable() {              // #viz-fallback cell values, keyed day|status
  const t = document.getElementById('viz-fallback');
  if (!t || !t.getClientRects().length) return { visible: false };
  const cells = {};
  for (const td of t.querySelectorAll('[data-day][data-status]'))
    cells[`${td.getAttribute('data-day')}|${td.getAttribute('data-status')}`] =
      parseInt((td.textContent || '').replace(/[^\d-]/g, ''), 10);
  return { visible: true, cells };
}
function pageVizPanelState() {
  const vis = (id) => { const e = document.getElementById(id);
    return !!(e && e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden'); };
  return { vizEmpty: vis('viz-empty'), vizError: vis('viz-error'), canvas: vis('viz3d'),
           toggle: vis('viz-toggle'), fallback: vis('viz-fallback') };
}
function pageVizNoWebgl() {                 // viz-fallback scenario analysis
  const state = pageVizPanelState();
  let notice = null;
  const re = /2d|fallback|unavailable|not\s+supported|webgl/i;
  const visEl = (el) => !!(el.getClientRects && el.getClientRects().length) &&
                        getComputedStyle(el).visibility !== 'hidden';
  for (const el of document.querySelectorAll('body *')) {
    if (el.children.length > 0 || el.closest('script,style,td,th')) continue;
    if (!visEl(el)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 200 && re.test(t)) { notice = t.slice(0, 120); break; }
  }
  return { ...state, notice };
}

// ── scenario blocks (append to main()'s dispatch; whitelist gains 'viz','viz-fallback') ─────
  } else if (scenario === 'viz') {
    const buckets = JSON.parse(process.env.BENCH_VIZ_BUCKETS ||
      '{"days":[],"statuses":["settled","pending","refunded","failed"],"cells":[]}');
    const emptyRun = buckets.cells.every((c) => !c.count);
    const navigationError = await safeGoto(20000);
    if (navigationError) { emit({ navigationError, consoleErrors: consoleErrors() }); return; }
    await waitIdle(10000);

    // readiness: canvas + >=1 draw call (empty run: canvas may legitimately never draw)
    let ready = { canvas: false, drawCalls: 0, vsdbg: false };
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      ready = await page.evaluate(pageVizReady).catch(() => ready);
      if (ready.canvas && (emptyRun || ready.drawCalls > 0)) break;
      await sleep(150);
    }
    await sleep(300);
    const panel = await page.evaluate(pageVizPanelState).catch(() => ({}));

    if (emptyRun) {                          // phantom-data guard: empty db ⇒ #viz-empty, no bars
      const probe = await page.evaluate(pageSampleScene,
        { samples: Array.from({ length: 24 }, (_, k) =>
            ({ cx: (k % 6 + 0.5) * 100, cy: (Math.floor(k / 6) + 0.5) * 80, kind: 'grid' })),
          tol: VIZ.tol }).catch(() => null);
      const dbg = await page.evaluate(pageVsdbgSnapshot, { points: [], picks: [] }).catch(() => null);
      await saveShot('viz');
      emit({ emptyRun: true, ready, panel,
             vsdbgBarCount: dbg && dbg.scene && dbg.scene.bars ? dbg.scene.bars.length : null,
             gridGot: probe && probe.samples ? probe.samples.map((s) => s.got) : null,
             consoleErrors: consoleErrors() });
      return;
    }

    // aspect from the MEASURED rect (R2): compute expectations after reading it
    const pre = await page.evaluate(pageSampleScene, { samples: [], tol: VIZ.tol });
    if (!pre.found || !pre.glReadable) {
      await saveShot('viz');
      emit({ ready, panel, glReadable: !!pre.glReadable, canvasFound: !!pre.found,
             consoleErrors: consoleErrors() });
      return;
    }
    const W = pre.rect.w, H = pre.rect.h, rect = pre.rect;
    const backingOk = Math.abs(pre.backing.w - Math.round(W * pre.dpr)) <= 1 &&
                      Math.abs(pre.backing.h - Math.round(H * pre.dpr)) <= 1;

    const S = buildVizSamples(buckets, VIZ.yaw0, VIZ.pitch0, VIZ.dist0, W, H);
    const all = [...S.tops, ...S.above, ...S.sides, ...S.sky, ...S.corners, ...S.grid];
    const r1 = await page.evaluate(pageSampleScene, { samples: all, tol: VIZ.tol });
    const cnt = (k) => { const a = (r1.samples || []).filter((s) => s.kind === k);
                        return { ok: a.filter((s) => s.ok).length, total: a.length }; };
    const gridGot = (r1.samples || []).filter((s) => s.kind === 'grid').map((s) => s.got);
    const isBg = (c) => c.every((v, ix) => Math.abs(v - VIZ.bg[ix]) <= VIZ.tol);
    const nonBg = gridGot.filter((c) => !isBg(c));
    const topGot = (r1.samples || []).filter((s) => s.kind === 'top' && s.ok).map((s) => s.got.join(','));

    // vsdbg truth: scene ≡ buckets, project ≡ probe math, camera ≡ defaults
    const spotBars = S.bars.filter((_, k) => k % Math.max(1, Math.ceil(S.bars.length / 6)) === 0).slice(0, 6);
    const dbg1 = await page.evaluate(pageVsdbgSnapshot,
      { points: spotBars.map((b) => [b.x, b.h, b.z]),
        picks: S.tops.slice(0, 4).map((t) => [t.cx, t.cy]) }).catch(() => null);
    let vsdbgProjErr = null, vsdbgSceneOk = null, vsdbgPickOk = null;
    if (dbg1 && dbg1.scene && Array.isArray(dbg1.scene.bars)) {
      const want = new Map(S.bars.map((b) => [b.key, b]));
      const claims = dbg1.scene.bars;
      const matched = claims.filter((c) => { const w = want.get(c.key);
        return w && c.count === w.count && Math.abs(c.x - w.x) < 1e-6 &&
               Math.abs(c.z - w.z) < 1e-6 && Math.abs(c.h - w.h) < 1e-6; }).length;
      vsdbgSceneOk = { matched, claimed: claims.length, expected: want.size };
      const eye = cameraEye(VIZ.yaw0, VIZ.pitch0, VIZ.dist0), basis = cameraBasis(eye);
      const errs = spotBars.map((b, ix) => {
        const got = dbg1.projections[ix], exp = projectPt(eye, basis, W, H, [b.x, b.h, b.z]);
        return Array.isArray(got) && exp ? Math.hypot(got[0] - exp.x, got[1] - exp.y) : 999;
      });
      vsdbgProjErr = +Math.max(...errs).toFixed(2);
      vsdbgPickOk = { ok: dbg1.picks.filter((k, ix) => k === S.tops[ix].key).length,
                      total: dbg1.picks.length };
    }

    // tooltip: hover a visible top; poll for #viz-tooltip within 1.2s, record latency
    let tooltip = { shown: false };
    if (S.tops.length) {
      const t0bar = S.tops[0];
      await page.mouse.move(rect.left + t0bar.cx, rect.top + t0bar.cy);
      const tStart = Date.now();
      while (Date.now() - tStart < 1200) {
        const ts = await page.evaluate(pageTooltipState).catch(() => ({ visible: false }));
        if (ts.visible) { tooltip = { shown: true, ms: Date.now() - tStart, text: ts.text,
                                      wantPrefix: `${S.bars[t0bar.i].count} ${t0bar.status}` }; break; }
        await sleep(40);
      }
      await page.mouse.move(rect.left + 2, rect.top + H + 40);   // off-canvas: must hide
      await sleep(250);
      const ts2 = await page.evaluate(pageTooltipState).catch(() => ({ visible: false }));
      tooltip.hides = !ts2.visible;
    }

    // picking: click bar tops → #status-filter changes; depth pair on an occluded top whose
    // occluder has a DIFFERENT status (fixture guarantees at least one such pair)
    const stride = Math.max(1, Math.ceil(S.tops.length / 5));
    const targets = S.tops.filter((_, k) => k % stride === 0).slice(0, 5);
    const picks = [];
    for (const t of targets) {
      await page.mouse.click(rect.left + t.cx, rect.top + t.cy);
      await sleep(120);
      const got = await page.evaluate(pageStatusFilterValue).catch(() => null);
      picks.push({ want: t.status, got, ok: !!got && got.includes(t.status) });
    }
    let depthPair = { available: false, ok: null };
    const dp = S.occludedTops.find((o) =>
      S.bars[o.occluder].status !== S.bars.find((b) => b.key === o.key).status);
    if (dp) {
      await page.mouse.click(rect.left + dp.css.x, rect.top + dp.css.y);
      await sleep(120);
      const got = await page.evaluate(pageStatusFilterValue).catch(() => null);
      depthPair = { available: true, want: S.bars[dp.occluder].status, got,
                    ok: !!got && got.includes(S.bars[dp.occluder].status) };
    }

    // 2D toggle: #viz-toggle → #viz-fallback cells must equal expected buckets
    let toggle = { present: false };
    if (panel.toggle) {
      await page.click('#viz-toggle').catch(() => {});
      await sleep(300);
      const ft = await page.evaluate(pageFallbackTable).catch(() => ({ visible: false }));
      if (ft.visible) {
        const nonZero = buckets.cells.filter((c) => c.count > 0);
        const okCells = nonZero.filter((c) => ft.cells[`${c.day}|${c.status}`] === c.count).length;
        toggle = { present: true, visible: true, okCells, totalCells: nonZero.length };
      } else toggle = { present: true, visible: false };
      await page.click('#viz-toggle').catch(() => {});
      await sleep(300);
    }

    // camera: wheel → reprojection at new distance; drag LAST (2s, frames counted);
    // sign-flip diagnostic; dblclick reset restores baseline (verified reversibility anchor)
    const cx0 = rect.left + W / 2, cy0 = rect.top + H / 2;
    await page.mouse.move(cx0, cy0);
    await page.mouse.wheel(0, 400);                       // dist 30·exp(0.48) ≈ 48.5
    await sleep(250);
    const distT = Math.min(VIZ.distMax, VIZ.dist0 * Math.exp(VIZ.wheelK * 400));
    const Sw = buildVizSamples(buckets, VIZ.yaw0, VIZ.pitch0, distT, W, H);
    const rw = await page.evaluate(pageSampleScene, { samples: Sw.tops, tol: VIZ.tol })
      .catch(() => ({ samples: [] }));
    await page.mouse.dblclick(cx0, cy0);                  // reset before the drag
    await sleep(250);

    const dbgF0 = await page.evaluate(pageVsdbgSnapshot, { points: [], picks: [] }).catch(() => null);
    const draws0 = (await page.evaluate(pageVizReady).catch(() => ({ drawCalls: 0 }))).drawCalls;
    const tDrag = Date.now();
    await page.mouse.move(cx0, cy0); await page.mouse.down();
    for (let step = 1; step <= 20; step++) {              // scripted 2s drag, +120px total
      await page.mouse.move(cx0 + step * 6, cy0, { steps: 1 });
      await sleep(95);
    }
    await page.mouse.up();
    const dragWallMs = Date.now() - tDrag;
    await sleep(250);
    const dbgF1 = await page.evaluate(pageVsdbgSnapshot, { points: [], picks: [] }).catch(() => null);
    const draws1 = (await page.evaluate(pageVizReady).catch(() => ({ drawCalls: 0 }))).drawCalls;

    const yawT = VIZ.yaw0 - 120 * VIZ.dragDegPerPx;       // spec: yaw ← yaw − 0.35·Δx
    const S2 = buildVizSamples(buckets, yawT, VIZ.pitch0, VIZ.dist0, W, H);
    const S2f = buildVizSamples(buckets, VIZ.yaw0 + 120 * VIZ.dragDegPerPx, VIZ.pitch0, VIZ.dist0, W, H);
    const r2 = await page.evaluate(pageSampleScene,
      { samples: [...S2.tops, ...S2f.tops.map((s) => ({ ...s, kind: 'topflip' }))], tol: VIZ.tol })
      .catch(() => ({ samples: [] }));
    const camAfter = dbgF1 && dbgF1.camera && !dbgF1.camera.__err ? dbgF1.camera : null;

    await page.mouse.dblclick(cx0, cy0);                  // reset → baseline must restore
    await sleep(250);
    const r3 = await page.evaluate(pageSampleScene, { samples: S.tops, tol: VIZ.tol })
      .catch(() => ({ samples: [] }));

    const f = (arr) => { const a = arr.filter((s) => s.ok).length; return { ok: a, total: arr.length }; };
    await saveShot('viz');
    emit({
      emptyRun: false, ready, panel, backingOk, canvasRect: { w: W, h: H },
      gl: r1.instrument || null, glReadable: true,
      contextCheck: { gridNonBg: nonBg.length, gridTotal: gridGot.length,
                      gridDistinct: new Set(nonBg.map((c) => c.join(','))).size,
                      corners: cnt('corner') },
      binding: { tops: cnt('top'), above: cnt('above'), sides: cnt('side'), sky: cnt('sky'),
                 occludedTops: S.occludedTops.length,
                 statusesExpected: new Set(S.bars.map((b) => b.status)).size,
                 distinctTopColors: new Set(topGot).size },
      vsdbg: { present: !!dbg1, version: dbg1 ? dbg1.version : null,
               sceneOk: vsdbgSceneOk, projMaxErrPx: vsdbgProjErr, picks: vsdbgPickOk },
      tooltip,
      picking: { picks, correct: picks.filter((p) => p.ok).length, total: picks.length, depthPair },
      toggle,
      wheel: { proj: f(rw.samples || []), expectedDist: +distT.toFixed(2) },
      drag: { proj: f((r2.samples || []).filter((s) => s.kind === 'top')),
              projFlipped: f((r2.samples || []).filter((s) => s.kind === 'topflip')),
              cameraAfter: camAfter, expectedYaw: +yawT.toFixed(1),
              framesDelta: dbgF0 && dbgF1 && Number.isFinite(dbgF1.frames) && Number.isFinite(dbgF0.frames)
                ? dbgF1.frames - dbgF0.frames : null,
              drawCallsDelta: draws1 - draws0, wallMs: dragWallMs,
              reset: f(r3.samples || []) },
      consoleErrors: consoleErrors(),
    });

  } else if (scenario === 'viz-fallback') {
    // context created with addInitScript(glKill): WebGL null, canvas-2D alive.
    const navigationError = await safeGoto(20000);
    if (!navigationError) { await waitIdle(8000); await sleep(800); }
    const fb = await page.evaluate(pageVizNoWebgl).catch(() => ({ canvas: false }));
    const ft = await page.evaluate(pageFallbackTable).catch(() => ({ visible: false }));
    const buckets = JSON.parse(process.env.BENCH_VIZ_BUCKETS || '{"cells":[]}');
    const nonZero = (buckets.cells || []).filter((c) => c.count > 0);
    const okCells = ft.visible
      ? nonZero.filter((c) => ft.cells[`${c.day}|${c.status}`] === c.count).length : 0;
    const snap = await page.evaluate(pageViewSnapshot).catch(() => null);   // existing helper
    await saveShot('viz-fallback');
    emit({ navigationError, ...fb, fallbackTable: { visible: !!ft.visible, okCells,
           totalCells: nonZero.length },
           renderedRowCount: snap ? snap.rowCount : 0,
           consoleErrors: consoleErrors() });
  }
```

---

# ARTIFACT 4 — scorer skeleton (drop-in for `bench/score_build.py`, env-gated `BENCH_SB6`)

Covers: weights + asserts, calibration knobs, T-tier checks reading the Artifact-3 emit schema, HARD compounds (incl. the new webhook/conflict checks), the re-founded efficiency formulas, the new B/C/V checks for the v3 API surface, the E gate, `evaluate_sb6`, and the fit sketch. Existing sb-5.3 checks keep running where still valid; the `DIAGNOSTIC` set demotes absorbed ones. `g()`, `_pe()`, `_ladder()`, `Ctx`, `check`/`product_check` are the file's existing primitives.

```python
# ══ sb-6: THE HARD TIER — env-gated; the sb-5.3 path stays byte-identical ═══════════════════
SB6 = bool(os.environ.get("BENCH_SB6"))
if SB6:
    SCORER_VERSION = "sb-6.0-rc1"          # rc until the Bedrock fit freezes; never on a board

# Inner weights (× 0.88) + the gate-locked E slice. The asserts ARE the compression-proof:
# re-compressing the instrument requires deleting one, which no diff does quietly.
TIER_WEIGHT_SB6 = {"A": 0.06, "B": 0.12, "C": 0.12, "D": 0.10,     # CORE 0.40 split 15/30/30/25
                   "J": 0.12, "V": 0.08, "P": 0.06, "T": 0.14, "HARD": 0.20}
E_WEIGHT = 0.12
assert abs(sum(TIER_WEIGHT_SB6.values()) - 1.0) < 1e-9
assert TIER_WEIGHT_SB6["T"] + TIER_WEIGHT_SB6["HARD"] >= 0.34, "hard-axis weight floor"
assert E_WEIGHT >= 0.10, "excellence slice floor"

# Calibration knobs — frozen by bench/fit_sb6.py from the Bedrock sweep (grid fit vs bands
# Opus .72-.80 / Sonnet .60-.70 / Haiku .40-.52), NEVER hand-edited. gamma applies per check
# BEFORE the tier mean: Jensen punishes inconsistency. Capped: an unreachable band is a
# task-design defect — iterate the SPEC, not the knobs.
GAMMA = {"core": 1.0, "hard": 1.0}         # placeholders until the fit lands
K_P = 1.0
GAMMA_CAP = 4.0
assert GAMMA["core"] <= GAMMA_CAP and GAMMA["hard"] <= GAMMA_CAP

# calib-sb6.json (knobs + per-check Opus/Sonnet/Haiku medians + classifications + fit context)
# is sha256-pinned: a tampered or missing artifact refuses to score, never silently defaults.
CALIB_SHA256 = "TBD-AT-FREEZE"

# Absorbed-by-compound checks: still computed and reported in `parts`, weight zero.
DIAGNOSTIC = {"resync_idempotent", "second_sync_cost", "update_propagation",
              "restart_persistence", "concurrent_sync_safe", "store_atomic_upsert",
              "j_loads_data", "j_console_clean", "local_pagination", "input_validation"}


def compound(components: Dict[str, float], gates: Dict[str, bool]) -> tuple:
    """gate × min. Gates are binary preconditions (absence scores 0 — the vacuous rule);
    the weakest component bounds the whole, so one defect stops stacking partial credit."""
    if not all(gates.values()):
        return 0.0, {"gate_failed": [k for k, v in gates.items() if not v], **components}
    return min(components.values()), {**{f"gate:{k}": True for k in gates}, **components}


def _viz(c: Ctx) -> Dict:
    return c.probe_viz if isinstance(getattr(c, "probe_viz", None), dict) else {}


def _frac(d: Optional[Dict]) -> float:
    d = d or {}
    return (d.get("ok", 0) / d["total"]) if d.get("total") else 0.0


# ── T TIER: the 3D contract, graded from analytically recomputed pixels ─────────────────────

@product_check("t_context_real", "T")
def t_context_real(c: Ctx):
    """A real WebGL context on #viz3d that really drew. Counters: offscreen/1px canvas
    (backing must match rect×DPR), acquire-and-clear (draw* counted, clear is not),
    full-screen wash (corners must equal #0F172A ±8), canvas-2D fake (context kind)."""
    p = _viz(c)
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    gl = p.get("gl") or {}
    ctxs = gl.get("contexts") or []
    ck = p.get("contextCheck") or {}
    corners = ck.get("corners") or {}
    parts = {
        "webgl_on_viz3d": any(x.get("canvasId") == "viz3d" for x in ctxs),
        "backing_matches_rect": bool(p.get("backingOk")),
        "draw_calls": (gl.get("drawCalls") or 0) >= 1,
        "coverage": (ck.get("gridNonBg") or 0) >= 3 and (ck.get("gridDistinct") or 0) >= 2,
        "corners_bg": corners.get("ok") == corners.get("total") and (corners.get("total") or 0) > 0,
    }
    s = (0.25 * parts["webgl_on_viz3d"] + 0.15 * parts["backing_matches_rect"]
         + 0.15 * parts["draw_calls"] + 0.25 * parts["coverage"] + 0.20 * parts["corners_bg"])
    return g(s, f"ctx={len(ctxs)} draws={gl.get('drawCalls')} "
                f"nonBg={ck.get('gridNonBg')}/{ck.get('gridTotal')} corners={corners.get('ok')}",
             "no real 3D surface — the panel is decoration or absent", parts=parts)


@product_check("t_scene_binding", "T")
def t_scene_binding(c: Ctx):
    """Every visible bar's top pixel shows its status color at the ANALYTICALLY projected
    position (position encodes height, so wrong heights fail directly); above-top must be
    background (brackets height); mid-column must be the bar's own surface (0.75 achievable-
    fraction ladder absorbs antialiased-edge misses, measured 1/12 deterministic); sky/gaps
    must be background. Mono-wash caps at 0.25; a full-coverage viz on the EMPTY db is
    phantom data and zeroes the check."""
    p = _viz(c)
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    pe_ = getattr(c, "probe_viz_empty", {}) or {}
    if pe_.get("emptyRun") and (pe_.get("vsdbgBarCount") or 0) > 0:
        return g(0, f"empty db but vsdbg claims {pe_.get('vsdbgBarCount')} bars",
                 "phantom data on an empty database")
    b = p.get("binding") or {}
    score = (0.5 * _frac(b.get("tops")) + 0.25 * _frac(b.get("above"))
             + 0.15 * min(_frac(b.get("sides")) / 0.75, 1.0) + 0.10 * _frac(b.get("sky")))
    if (b.get("statusesExpected") or 0) >= 3 and (b.get("distinctTopColors") or 0) < 3:
        score = min(score, 0.25)           # mono-wash guard (fixture guarantees >=3 statuses)
    return g(score, f"tops {_frac(b.get('tops')):.2f} above {_frac(b.get('above')):.2f} "
                    f"sides {_frac(b.get('sides')):.2f} sky {_frac(b.get('sky')):.2f} "
                    f"colors={b.get('distinctTopColors')} occl={b.get('occludedTops')}",
             "the scene does not encode the data — bars misplaced, mis-colored, or mis-heighted")


@product_check("t_camera", "T")
def t_camera(c: Ctx):
    """Wheel → reprojection at the computed distance; 2s drag → reprojection at the computed
    yaw (sign-flip half-credited: finesse, not a cliff); dblclick reset restores the baseline
    (the honesty anchor — noise cannot pass it); vsdbg.camera() agrees post-drag."""
    p = _viz(c)
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    d, w = p.get("drag") or {}, p.get("wheel") or {}
    proj, projf = _frac(d.get("proj")), _frac(d.get("projFlipped"))
    proj_credit = proj if proj >= projf else 0.5 * projf
    cam = d.get("cameraAfter") or {}
    cam_ok = cam.get("yaw") is not None and abs(cam["yaw"] - (d.get("expectedYaw") or 0)) <= 1.5
    score = (0.20 * _frac(w.get("proj")) + 0.35 * proj_credit
             + 0.10 * cam_ok + 0.35 * _frac(d.get("reset")))
    return g(score, f"wheel {_frac(w.get('proj')):.2f} drag {proj:.2f} (flip {projf:.2f}) "
                    f"cam yaw={cam.get('yaw')} want {d.get('expectedYaw')} "
                    f"reset {_frac(d.get('reset')):.2f}",
             "the orbit control does not implement the documented camera")


@product_check("t_picking", "T")
def t_picking(c: Ctx):
    """Click a bar → #status-filter takes that status and the table refreshes. Depth pair:
    clicking the projected top of an analytically occluded bar must select the OCCLUDER's
    status (the fixture guarantees a cross-status occlusion pair). Counters: always-same-
    status fails 4/5 targets; 2D-nearest picking without depth fails the pair."""
    p = _viz(c)
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    k = p.get("picking") or {}
    frac = (k.get("correct", 0) / k["total"]) if k.get("total") else 0.0
    dp = k.get("depthPair") or {}
    depth = (1.0 if dp.get("ok") else 0.0) if dp.get("available") else frac
    return g(0.65 * frac + 0.35 * depth,
             f"{k.get('correct')}/{k.get('total')} picks, depthPair="
             f"{'ok' if dp.get('ok') else dp.get('got')}",
             "clicking a bar does not drive the product — the 3D view is not wired to the data")


@product_check("t_vsdbg_truth", "T")
def t_vsdbg_truth(c: Ctx):
    """vsdbg must TELL THE TRUTH: scene() ≡ expected buckets (key/count/x/z/h exact),
    project() ≡ the probe's own math (≤2px), pick() occlusion-correct, version == 3.
    An instrumentation layer contradicting the pixels scores broken, not clever — the pixel
    checks above are the anchor that makes faking this pointless."""
    p = _viz(c)
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    v = p.get("vsdbg") or {}
    if not v.get("present"):
        return g(0, "window.vsdbg absent", "the graded instrumentation API is missing")
    sc = v.get("sceneOk") or {}
    scene = (sc.get("matched", 0) / sc["expected"]) if sc.get("expected") else 0.0
    proj = 1.0 if (v.get("projMaxErrPx") is not None and v["projMaxErrPx"] <= 2.0) else 0.0
    picks = _frac(v.get("picks"))
    ver = 1.0 if v.get("version") == 3 else 0.0
    return g(0.1 * ver + 0.4 * scene + 0.3 * proj + 0.2 * picks,
             f"scene {sc.get('matched')}/{sc.get('expected')} projErr={v.get('projMaxErrPx')}px "
             f"picks {v.get('picks')}",
             "vsdbg reports a scene the canvas does not show")


@product_check("t_tooltip", "T")
def t_tooltip(c: Ctx):
    p = _viz(c)
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    t = p.get("tooltip") or {}
    if not t.get("shown"):
        return g(0, "tooltip never appeared", "hover tells the user nothing")
    text_ok = bool(t.get("text", "").startswith(t.get("wantPrefix", "\x00")))
    latency = _ladder(t.get("ms"), [(1.0, 150 * K_P), (0.6, 400 * K_P), (0.3, 1200)])
    hides = 1.0 if t.get("hides") else 0.0
    return g(0.5 * text_ok + 0.3 * latency + 0.2 * hides,
             f"'{t.get('text')}' in {t.get('ms')}ms (want '{t.get('wantPrefix')}…') hides={t.get('hides')}",
             "the tooltip is absent, slow, or wrong about the data")


@product_check("t_fallback", "T")
def t_fallback(c: Ctx):
    """Two halves: (a) viz scenario — #viz-toggle swaps to #viz-fallback whose cells equal
    /api/buckets; (b) viz-fallback scenario (WebGL killed) — no crash, table auto-shown,
    notice visible, main table alive. A never-uses-WebGL app passes (b) but zeroes
    t_context_real, so there is no vacuous win."""
    p, q = _viz(c), getattr(c, "probe_viz_fallback", {}) or {}
    if _pe(p) and _pe(q):
        return g(0, f"PROBE UNAVAILABLE: {_pe(p) or _pe(q)}", "harness failure, not app evidence")
    tg = p.get("toggle") or {}
    tg_frac = (tg.get("okCells", 0) / tg["totalCells"]) if tg.get("totalCells") else 0.0
    toggle = tg_frac if tg.get("visible") else (0.1 if tg.get("present") else 0.0)
    errs = (q.get("consoleErrors") or {}).get("count", 99)
    ft = q.get("fallbackTable") or {}
    auto = (ft.get("okCells", 0) / ft["totalCells"]) if ft.get("totalCells") and ft.get("visible") else 0.0
    grace = (0.4 * (errs == 0) + 0.35 * auto + 0.15 * bool(q.get("notice"))
             + 0.10 * ((q.get("renderedRowCount") or 0) > 0))
    return g(0.5 * toggle + 0.5 * grace,
             f"toggle cells {tg.get('okCells')}/{tg.get('totalCells')}; no-webgl errs={errs} "
             f"auto {ft.get('okCells')}/{ft.get('totalCells')} notice={bool(q.get('notice'))}",
             "a machine without WebGL gets a crash or a blank panel")


# ── HARD: compounds + the new API-contract mechanisms ───────────────────────────────────────

@product_check("h_sync_discipline", "HARD")
def h_sync_discipline(c: Ctx):
    """gate × min(cheap, idempotent, propagates). 'cheap' is the CONTINUOUS second-sync
    formula (R9): 0.5·cond/reqs + 0.5·304/reqs — the dead 0.4 rung is gone, a 30/33-
    conditional build no longer scores like a 0/9 build. Absence of a second sync scores 0."""
    ran = isinstance(c.sync2, dict) and any(k in c.sync2 for k in ("inserted", "total", "fetched"))
    cheap = (0.5 * (c.sync2_cond / c.sync2_reqs) + 0.5 * (c.sync2_304 / c.sync2_reqs)) \
        if getattr(c, "sync2_reqs", 0) else 0.0
    idem = (1.0 if c.sync2.get("inserted") == 0 and c.sync2.get("total") == EXPECTED_TOTAL
            else 0.5 if c.sync2.get("total") == EXPECTED_TOTAL else 0.0) if ran else 0.0
    prop = min(c.update_seen / c.update_changed, 1.0) if getattr(c, "update_changed", 0) else 0.0
    score, parts = compound({"cheap": round(cheap, 3), "idempotent": idem, "propagates": round(prop, 3)},
                            {"sync2_ran": ran, "vendor_reqs_seen": getattr(c, "sync2_reqs", 0) > 0})
    return g(score, f"cheap={cheap:.2f} idem={idem} prop={prop:.2f}",
             "sync must be cheap AND idempotent AND propagate updates — together, not severally",
             parts=parts)


@product_check("h_durability", "HARD")
def h_durability(c: Ctx):
    persist = min(getattr(c, "rows_after_restart", 0) / EXPECTED_TOTAL, 1.0)
    conc = max(0.0, 1 - abs(getattr(c, "concurrent_total", 0) - EXPECTED_TOTAL) / EXPECTED_TOTAL)
    atomic = getattr(c, "atomic_upsert_score", 0.0)
    score, parts = compound({"persists": round(persist, 3), "concurrent": round(conc, 3),
                             "atomic": atomic},
                            {"had_rows_before_kill": getattr(c, "rows_before_kill", 0) > 0})
    return g(score, f"persist={persist:.2f} conc={conc:.2f} atomic={atomic}",
             "data that does not survive a restart or a race is not stored", parts=parts)


@product_check("h_webhook_ledger", "HARD")
def h_webhook_ledger(c: Ctx):
    """The mock scripts a delivery sequence (dupes, out-of-order, one forged signature) and
    freezes the expected counter quad. /api/health's webhook counters must match EXACTLY,
    the forged event must be 401 bad_signature with state untouched, and the stale event
    must not have regressed the row version. Gate: registration handshake completed."""
    w = (getattr(c, "health2", None) or {}).get("webhook") or {}
    exp = vendor_service.EXPECTED_WEBHOOK_COUNTERS      # frozen by the delivery script
    quad = ["received", "applied", "ignored", "rejected"]
    exact = sum(w.get(k) == exp[k] for k in quad) / 4
    forged = 1.0 if getattr(c, "forged_event_status", None) == 401 else 0.0
    stale = 1.0 if getattr(c, "stale_row_version_ok", False) else 0.0
    score, parts = compound({"counters": exact, "forged_rejected": forged, "stale_ignored": stale},
                            {"registered": bool(w.get("registered"))})
    return g(score, f"counters {[w.get(k) for k in quad]} want {[exp[k] for k in quad]} "
                    f"forged={getattr(c, 'forged_event_status', None)}",
             "the webhook ledger is the proof the app handled untrusted push traffic", parts=parts)


@product_check("h_conflict_dance", "HARD")
def h_conflict_dance(c: Ctx):
    """Mock injects one 412 on the note write (the one-shot trigger pattern, like v2's 429/410).
    Grade from the vendor trace: If-Match on every write (428 never triggered), exactly one
    refetch+retry on the 412, note landed with the fresh version; a scripted second-412 case
    must yield local 409 conflict envelope with the row unchanged."""
    t = getattr(c, "conflict_trace", {}) or {}
    parts = {"if_match_always": t.get("writes_without_if_match", 1) == 0,
             "recovered_412": bool(t.get("retried_once_with_fresh_version")),
             "no_extra_retries": t.get("retries_after_412", 9) <= 1,
             "second_412_is_409": t.get("second_412_local_status") == 409 and
                                  bool(t.get("second_412_row_unchanged"))}
    s = 0.25 * parts["if_match_always"] + 0.35 * parts["recovered_412"] \
        + 0.15 * parts["no_extra_retries"] + 0.25 * parts["second_412_is_409"]
    return g(s, f"{t}", "optimistic concurrency done wrong silently loses someone's edit",
             parts=parts)


@product_check("request_efficiency", "HARD")
def request_efficiency_v3(c: Ctx):
    """Vacuous pass CLOSED (measured: 13/31 serious builds scored 1.0 with 3-5 requests,
    five of which synced 0 rows). OPTIMAL_REQUESTS is exported by the mock, never hand-
    written. Fewer-than-optimal requests score 0 unless completeness is 1.0 AND all three
    vendor traps passed — the one honest under-optimum build in the corpus failed a trap."""
    from fixtures import OPTIMAL_REQUESTS
    reqs, complete = getattr(c, "sync1_reqs", 0), getattr(c, "sync_completeness_score", 0.0)
    traps_ok = all(getattr(c, k, 0.0) >= 1.0 for k in
                   ("trap_retry_after", "trap_cursor_expiry", "trap_stall"))
    if reqs < OPTIMAL_REQUESTS:
        s = (OPTIMAL_REQUESTS / max(reqs, 1)) * 0 if not (complete >= 1.0 and traps_ok) else 1.0
        s = 1.0 if (complete >= 1.0 and traps_ok) else 0.0
    elif reqs == OPTIMAL_REQUESTS and complete >= 1.0:
        s = 1.0
    elif reqs <= OPTIMAL_REQUESTS + 2:
        s = 0.75 * complete
    elif reqs <= OPTIMAL_REQUESTS + 7:
        s = 0.5 * complete
    else:
        s = (OPTIMAL_REQUESTS / reqs) * complete
    return g(s, f"{reqs} vendor requests (optimum {OPTIMAL_REQUESTS}), completeness {complete:.2f}",
             "fetching nothing is not efficiency — credit requires the data actually arrived")


# ── new/changed B, C, V checks for the v3 surface (representative set) ───────────────────────

@check("b_buckets_dst", "B")
def b_buckets_dst(c: Ctx):
    """Fraction of /api/buckets cells exactly equal to the fixture's Berlin-day bucketing.
    The 2026-03-29 DST day is the discriminator: UTC-day bucketing gets those cells wrong
    while matching most others — the fraction resolves exactly that."""
    from fixtures import EXPECTED_BUCKETS
    got = {(x.get("day"), x.get("status")): x.get("count") for x in (getattr(c, "buckets", None) or {}).get("cells", [])}
    want = {(x["day"], x["status"]): x["count"] for x in EXPECTED_BUCKETS["cells"]}
    if not got:
        return g(0, "no cells returned", "the buckets endpoint is the 3D chart's data source")
    exact = sum(got.get(k) == v for k, v in want.items()) / len(want)
    shape = 1.0 if len(got) == len(want) else 0.5 if got else 0.0
    return g(0.8 * exact + 0.2 * shape,
             f"{sum(got.get(k) == v for k, v in want.items())}/{len(want)} cells exact",
             "Berlin-day bucketing across the DST switch is the data-correctness trap")


@check("b_summary_currency", "B")
def b_summary_currency(c: Ctx):
    """Replaces summary_accuracy (its ±10% band never fired in 93 builds): k of 4 per-currency
    (count, total_minor) pairs exact → k/4 real resolution. Any cross-currency total anywhere
    in the response caps the check at 0.25 — summing across currencies is forbidden."""
    from fixtures import EXPECTED_BY_CURRENCY
    s = getattr(c, "summary", None) or {}
    by = {x.get("currency"): x for x in s.get("by_currency", [])}
    k = sum(1 for cur, exp in EXPECTED_BY_CURRENCY.items()
            if by.get(cur, {}).get("count") == exp["count"]
            and by.get(cur, {}).get("total_minor") == exp["total_minor"])
    score = k / len(EXPECTED_BY_CURRENCY)
    if any(key in s for key in ("total_minor", "total", "grand_total")):
        score = min(score, 0.25)
    return g(score, f"{k}/{len(EXPECTED_BY_CURRENCY)} currency buckets exact",
             "money summed across currencies is wrong money")


@check("c_batch_partial", "C")
def c_batch_partial(c: Ctx):
    """Mock injects one amount_over_limit item into the probe's batch. Grade: per-item results
    in input order, succeeded items created exactly once, the failed item reports its own
    error code, and the trace shows NO retry of the failed item (and no fresh-key retry)."""
    r = getattr(c, "batch_result", {}) or {}
    t = getattr(c, "batch_trace", {}) or {}
    parts = {"order_kept": bool(r.get("order_kept")),
             "succeeded_once": t.get("duplicate_creates", 9) == 0,
             "failed_reported": r.get("failed_error_code") == "amount_over_limit",
             "no_retry": t.get("retries_of_failed_item", 9) == 0 and t.get("fresh_key_retries", 9) == 0}
    s = sum(parts.values()) / 4
    return g(s, f"{parts}", "partial failure is a normal outcome, not a rollback or a retry storm",
             parts=parts)


@check("b_error_envelope", "B")
def b_error_envelope(c: Ctx):
    """Probe fires: bad limit, unknown path, invalid batch item. Fraction of responses using
    the frozen envelope (error.code from the frozen vocabulary, field_errors with dot paths
    on the 400s). Computed only over responses actually received — absent endpoint = 0."""
    cases = getattr(c, "envelope_cases", []) or []
    if not cases:
        return g(0, "no error responses observed", "an API that cannot say what went wrong")
    ok = sum(1 for x in cases if x.get("envelope_ok")) / len(cases)
    paths = sum(1 for x in cases if x.get("expects_field_errors") and x.get("field_paths_ok")) \
        / max(1, sum(1 for x in cases if x.get("expects_field_errors")))
    return g(0.6 * ok + 0.4 * paths, f"envelope {ok:.2f}, field paths {paths:.2f} over {len(cases)}",
             "structured errors are the contract's error half")


@product_check("v_currency_rendered", "V")
def v_currency_rendered(c: Ctx):
    """Browser truth replaces the Intl.NumberFormat grep (which decided the Opus-Sonnet
    order): the probe harvests amount-cell texts; fraction rendered with the row's OWN
    minor-unit exponent. JPY-with-decimals and truncated-KWD are the traps."""
    cells = getattr(c, "amount_cells", []) or []      # [{currency, minor, text}] from the probe
    if not cells:
        return g(0, "no amount cells rendered", "money is the product")
    ok = sum(1 for x in cells if x.get("format_ok")) / len(cells)
    jpy = [x for x in cells if x.get("currency") == "JPY"]
    kwd = [x for x in cells if x.get("currency") == "KWD"]
    trap = ((all(x.get("format_ok") for x in jpy) if jpy else 0) +
            (all(x.get("format_ok") for x in kwd) if kwd else 0)) / 2
    return g(0.6 * ok + 0.4 * trap, f"{ok:.2f} of {len(cells)} cells; JPY/KWD traps {trap:.2f}",
             "a yen with two decimals is wrong money")


# ── EXCELLENCE: the last 0.12, gate-locked ──────────────────────────────────────────────────

def excellence(rows: List[Dict], c: Ctx) -> tuple:
    """G ∈ {0,1}: zero-defect frontend + 3D truth + perf, simultaneously. j_sync_journey may
    gate only because sb-6 re-bases it on a half-seeded db (view_refreshed is now observable —
    under sb-5 both cloud models failed it as a probe artifact, R13)."""
    by = {r["check"]: r for r in rows}
    gate = (all(by[n]["score"] == 1.0 for n in
                ("j_first_use", "j_sync_journey", "j_error_state", "j_empty_state") if n in by)
            and (getattr(c, "console_errors_all_scenarios", 1) == 0)
            and by.get("v_responsive_375", {}).get("score") == 1.0
            and by.get("v_dates_readable", {}).get("score") == 1.0
            and by.get("t_scene_binding", {}).get("score", 0) >= 1.0
            and all(by[n]["score"] == 1.0 for n in
                    ("p_list_latency", "p_page_interactive", "p_sync_wall") if n in by))
    e_rows = [r for r in rows if r["tier"] == "E"]
    e_mean = sum(r["score"] for r in e_rows) / len(e_rows) if e_rows else 0.0
    return (1.0 if gate else 0.0), e_mean


@product_check("e_frames_under_drag", "E")
def e_frames_under_drag(c: Ctx):
    p = _viz(c)
    d = (p.get("drag") or {})
    fr, wall = d.get("framesDelta"), (d.get("wallMs") or 2000) / 1000
    if fr is None or (d.get("drawCallsDelta") or 0) <= 0:
        return g(0, "frames not measurable or no draw calls", "an empty rAF loop earns nothing")
    fps = fr / wall
    # 24-per-2s spec floor; exact rungs CALIBRATION-OWNED (SwiftShader across machines)
    return g(_ladder(-fps, [(1.0, -24 * K_P), (0.75, -18), (0.5, -12), (0.25, -6)]),
             f"{fr} frames / {wall:.1f}s = {fps:.0f}fps ({d.get('drawCallsDelta')} draw calls)",
             "the 3D view must stay interactive under input")


@product_check("e_under_load_latency", "E")
def e_under_load_latency(c: Ctx):
    """/api/payments p95 with 8 concurrent readers DURING a live sync — contention gives the
    latency budget real dynamic range at 1,553 rows (R10: the P-tier fix is what is measured,
    not just how much). Latency credit is gated on the responses being CORRECT (rows==expected
    on the same responses) — fast-because-empty is the named counter."""
    m = getattr(c, "under_load", {}) or {}
    if not m.get("correct"):
        return g(0, f"p95={m.get('p95_ms')}ms but responses wrong/empty under load",
                 "fast wrong answers are not performance")
    return g(_ladder(m.get("p95_ms"), [(1.0, 150 / K_P), (0.75, 300), (0.5, 600), (0.25, 1200)]),
             f"p95={m.get('p95_ms')}ms with 8 readers during sync", "the product under real load")


@product_check("e_optimistic_paint", "E")
def e_optimistic_paint(c: Ctx):
    ms = getattr(c, "optimistic_paint_ms", None)
    if ms is None:
        return g(0, "optimistic edit not observed", "the note editor never painted before the network")
    return g(_ladder(ms, [(1.0, 100), (0.6, 250), (0.3, 800)]), f"painted in {ms}ms",
             "optimistic UI is the difference between an app and a form")


@product_check("e_hard_mastery", "E")
def e_hard_mastery(c: Ctx):
    rows = getattr(c, "_hard_rows", [])
    mean = sum(r["score"] for r in rows) / len(rows) if rows else 0.0
    return g(1.0 if mean >= 0.90 else mean / 0.90 * 0.5, f"HARD mean {mean:.3f}",
             "excellence includes the mechanisms, not only the surface")


# ── evaluation ──────────────────────────────────────────────────────────────────────────────

def _gamma(x: float, tier: str) -> float:
    return max(0.0, min(1.0, x)) ** (GAMMA["hard"] if tier in ("T", "HARD") else GAMMA["core"])


def evaluate_sb6(c: Ctx, rows: List[Dict]) -> Dict:
    """rows = CORE + product + sb-6 results. DIAGNOSTIC members are reported, weight 0.
    ROOT_BLOCKS additionally attributes sync_completeness < 1.0 (not only == 0), so one
    partial-sync defect stops reading as eight independent failures."""
    tiers, inner = {}, 0.0
    for tier, w in TIER_WEIGHT_SB6.items():
        sub = [r for r in rows if r["tier"] == tier and r["check"] not in DIAGNOSTIC]
        mean = sum(_gamma(r["score"], tier) for r in sub) / len(sub) if sub else 0.0
        tiers[tier] = {"mean": round(mean, 4), "checks": len(sub), "weight": w}
        inner += mean * w
    c._hard_rows = [r for r in rows if r["tier"] == "HARD"]
    gate, e_mean = excellence(rows, c)
    score = 0.88 * inner + E_WEIGHT * gate * e_mean
    tiers["E"] = {"mean": round(gate * e_mean, 4), "gate": bool(gate), "weight": E_WEIGHT}
    return {"score": round(score, 4), "scorer_version": SCORER_VERSION,
            "inner": round(inner, 4), "excellence_gate": bool(gate), "tiers": tiers,
            "checks": rows, "root_causes": attribute_root_causes(rows),
            "excellent": bool(gate) and score >= 0.88, "solid": score >= 0.55}


# gather() additions (interface): boot the app; run sync; then
#   env = {"BENCH_VIZ_BUCKETS": json.dumps(fixtures.EXPECTED_BUCKETS)}
#   c.probe_viz          = _product_probe("viz", base, env=env)
#   c.probe_viz_fallback = _product_probe("viz-fallback", base, env=env)
#   c.probe_viz_empty    = _product_probe("viz", empty_base,
#                              env={"BENCH_VIZ_BUCKETS": json.dumps(fixtures.EMPTY_BUCKETS)})
#   (the empty instance is the same boot j_empty_state already does — reused, not duplicated)
# plus: conflict_trace / batch_trace / envelope_cases from the vendor mock's trace,
# health2 after the scripted webhook deliveries, under_load from perf_probe with 8 clients,
# amount_cells harvested by the existing view-snapshot probe (add amountTexts alongside
# dateTexts), and the half-seeded-db boot for the re-based j_sync_journey.


# ── bench/fit_sb6.py sketch (re-applies knobs to ARCHIVED raw x_i — no re-runs) ─────────────
BANDS = {"opus-5": (0.72, 0.80), "sonnet-5": (0.60, 0.70), "haiku-4.5": (0.40, 0.52)}

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
    return best   # None ⇒ bands unreachable inside caps ⇒ iterate the SPEC, never the knobs
# On freeze: write calib-sb6.json {knobs, per-check medians + classifications
# (binary/non-discriminating/dead — G3/G4/G5 diagnostics, NOT rescalers), fit context:
# engine sha, exact model ids, date}; bake its sha256 into CALIB_SHA256; SCORER_VERSION="sb-6.0".
```

---

## Implementation order (so the package lands without trusting anything unproven)

1. **Vendor mock extensions** (`vendor_service.py`): 1,553-row fixture with `EXPECTED_BUCKETS` / `EXPECTED_BY_CURRENCY` / `OPTIMAL_REQUESTS` / `EXPECTED_WEBHOOK_COUNTERS` exports; one-shot 412 trigger; scripted webhook delivery (handshake, dupes, out-of-order, one forged); batch endpoint with the amount-limit business rule; `--stall` trap.
2. **Reference implementation** of the full spec (grow probe-3d's validated `bars.html` into `vspro`) — this validates the camera framing constants (the one flagged arithmetic risk) before anything freezes.
3. **Probe integration** (Artifact 3) + the two seeded low-controls (mono-wash, readout-only drag) + v3 defect set for `controls.py` (≥2 3D defects).
4. **Scorer integration** (Artifact 4) behind `BENCH_SB6`; sb-5.3 path byte-identical.
5. **Calibration loop** (§6 of the design doc): smoke → controls → 13-run sweep → fit → gates → freeze `sb-6.0`.
6. **Fleet entry**: `REGIME.env` flips `BENCH_SPEC`; local arms re-enter with n ≥ 3 and the same catastrophe rule; sb-5 boards frozen as history.