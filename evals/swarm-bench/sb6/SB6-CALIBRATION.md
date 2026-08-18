# sb-6.0 Calibration Protocol — Bedrock baselines for the v3 (punishing) tier

Scope: this is the CALIBRATION half of the sb-6 program. It assumes a drafted `spec-build-v3.md` (harder API schema + raw-WebGL/Canvas/CSS-3D frontend) and a drafted sb-6 check set in `score_build.py` exist from the spec/scorer workstream. Everything below runs on the EXISTING pipeline — no new infrastructure except one fitting script.

---

## 0. Pipeline facts this protocol is built on (all verified in-repo)

- **Entrant runner:** `evals/swarm-bench/bench/run_build.py`. Cloud entrants run `goose run --provider aws_bedrock --model <id>` (line 65), engine binary `~/Projects/goose/target/release/goose` (override: `BENCH_GOOSE`).
- **Model ids** (the `MODELS` dict, run_build.py:31 and calibrate.py:38 — identical):
  - `opus-5` → `us.anthropic.claude-opus-5`
  - `sonnet-5` → `us.anthropic.claude-sonnet-5`
  - `haiku-4.5` → `us.anthropic.claude-haiku-4-5-20251001-v1:0`
- **Credentials:** `~/.config/agent-board/bedrock.env` (exists, mode 600, 179 bytes; keys `AWS_BEARER_TOKEN_BEDROCK`, `AWS_REGION`; rotated — `.prev` sibling from the same day). `load_env()` re-reads it on every `invoke()`, so mid-sweep rotation is picked up per-run. **Pre-flight each sweep:** confirm the token is fresh (a stale token fails the FIRST run with an auth error in `agent.tail` — abort, refresh, restart that rep).
- **Spec + tier injection:** `BENCH_SPEC=<abs path to spec-build-v3.md>` and `BENCH_PRODUCT=1` (same mechanism REGIME.env uses for v2). The prompt is the spec with `{DOCS_URL}/{BASE_URL}/{API_KEY}` substituted.
- **Per-check data:** each run writes `runs/<out>/<entrant>-r<rep>/verdict.json` with `score`, `scorer_version`, `tiers`, and `checks` — a list of `{check, tier, score ∈ [0,1]}` (60 entries under sb-5.3; sb-6 will have more). A tree copy + verdict is archived under `_sb4trees/` automatically.
- **calibrate.py is NOT the vehicle** — it drives the old single-file Meridian-client episode (`candidate_client.py`). It stays useful as a fast (1–3 min) loop for iterating individual vendor-API traps before paying for full build episodes, but all sb-6 baselines run through `run_build.py`.
- **Controls:** `bench/controls.py` proves the grader HIGH / LOW / ISOLATION by injecting defects into a copy of a known-good tree. sb-6 requires a v3 defect set (including at least two 3D-frontend defects) before any baseline number is believed.
- **Measured timing/variance under the current (v2) spec** — the basis for rep counts and cost:
  - Opus: 678 / 834 / 1170 s; scores 0.9800 / 0.9675 / 0.9755 (sd ≈ 0.005)
  - Sonnet: 410–517 s; scores 0.9203 / 0.9203 / 0.9692 (sd ≈ 0.023)
  - Haiku: 318–454 s; scores 0.7294 / 0.7305 / 0.7861, plus one archived catastrophic 0.0300 run (~1 in 13 cloud episodes)

**Engine freeze:** record `shasum -a 256 ~/Projects/goose/target/release/goose` before the first baseline run and verify it before every subsequent run of the same calibration. Do NOT rebuild mid-calibration (a live benchmark is also running on this machine — no cargo, per the freeze discipline). A changed engine binary voids the fit.

---

## 1. The exact procedure — models, reps, and why

### Rep counts

| Entrant | Reps | Role | Justification |
|---|---|---|---|
| `opus-5` | **6** | threshold-setter | Thresholds are per-check Opus **medians**. n=6 gives an even-n median (mean of the 3rd and 4th order statistics) — smoother in graded [0,1] space than an odd-n single order statistic, and robust to **2** arbitrary aberrant runs. For a check Opus passes with p=0.8, P(median lands on pass) ≈ 0.90 at n=6; pushing to n=8 buys ~3 points of that probability for +2 slowest-model runs — not worth it. Checks near p=0.5 have genuinely unstable medians at ANY affordable n; the fit handles those with the clamp + discrimination filter (§2), not with more reps. |
| `sonnet-5` | **4** | mid-scale anchor | Sets the "solid" band and the monotonicity gate; median of 4 is robust to 1 outlier. Sonnet's observed sd (0.023) is check-concentrated (its three v2 runs differ on a handful of checks, not diffusely), so 4 reps resolve which checks it reliably clears. |
| `haiku-4.5` | **3** | floor anchor + discrimination filter | Only needs to establish which checks Haiku fails/underperforms (the opus-vs-haiku gap is the per-check discrimination statistic). 3 reps + the catastrophe rule suffice. |

**Total: 13 runs per sweep.**

### Catastrophic-run rule

A run is *catastrophic* iff `agent.timed_out`, or `agent.exit != 0`, or `score < 0.30` with a produce-level root cause (no tree / server never started). One in ~13 cloud episodes did this under v2 (Haiku, 0.0300, re-run scored 0.7861).

- Re-run that rep ONCE (`--only-rep <k>`); use the re-run in the fit; **record** the failed run in the sweep log (the catastrophe rate is itself data).
- If the re-run is ALSO catastrophic, keep it in the fit — twice in a row is the model's behavior under v3, not infra noise.
- Never delete the failed run's directory or archive; the tree archive is the forensic record.

### Commands (one command per rep — resumable; a crash loses one rep, not the sweep)

```bash
cd ~/Projects/goose/evals/swarm-bench

# Pre-flight (every sweep)
test -s ~/.config/agent-board/bedrock.env || { echo "no bedrock.env"; exit 1; }
shasum -a 256 ~/Projects/goose/target/release/goose | tee runs/calib-v3/ENGINE.sha
python3 bench/controls.py --out runs/controls-sb6        # HIGH/LOW/ISOLATION must pass first

# Environment for every baseline run
export BENCH_PRODUCT=1
export BENCH_SPEC="$PWD/spec-build-v3.md"

# Opus: 6 reps (threshold-setter). --port 8990 avoids the live sweep's 8850 range.
# --timeout 2700: v3 is harder; v2 Opus already hit 1170s, and a timeout scored as a
# catastrophe from a too-tight limit would poison the fit.
for r in 0 1 2 3 4 5; do
  python3 bench/run_build.py --entrant opus-5 --reps 6 --only-rep $r \
      --timeout 2700 --port 8990 --out runs/calib-v3 2>&1 | tee -a runs/calib-v3/sweep.log
done

# Sonnet: 4 reps
for r in 0 1 2 3; do
  python3 bench/run_build.py --entrant sonnet-5 --reps 4 --only-rep $r \
      --timeout 2700 --port 8990 --out runs/calib-v3 2>&1 | tee -a runs/calib-v3/sweep.log
done

# Haiku: 3 reps
for r in 0 1 2; do
  python3 bench/run_build.py --entrant haiku-4.5 --reps 3 --only-rep $r \
      --timeout 2700 --port 8990 --out runs/calib-v3 2>&1 | tee -a runs/calib-v3/sweep.log
done
```

Run serially (the vendor mock + headless Chromium probe are light, but the machine hosts a live benchmark — serial keeps the interference bounded and the port range single). For unattended execution wrap each loop in `caffeinate -is nohup ... &` per the established unattended-loop practice, one loop at a time.

**Usage capture (new, required):** goose does not emit token usage into the verdict. After each run, harvest whatever usage the goose session log carries for that episode into `runs/calib-v3/usage-<entrant>-r<rep>.json` (even a stdout-tail grep); if nothing is available, record wall-clock + turn count as the proxy. The §4 cost model gets re-based on the first sweep's real numbers.

---

## 2. Threshold fitting — the quantile rule

For each check `c`, collect the 6 Opus scores `s₁..s₆`, the 4 Sonnet scores, the 3 Haiku scores. Let `opus_med(c)` = mean of the 3rd and 4th Opus order statistics; similarly `sonnet_med`, `haiku_med`.

**The rule: full credit at the Opus median.**

```
t_c        = clamp(opus_med(c), 0.50, 1.00)
rescaled   = min(1.0, raw_score / t_c)
```

- A run scoring at/above the Opus median gets full credit on `c`; below it, proportional credit. By construction, **~half of Opus runs lose partial credit on every calibrated check** — that is the compression-breaker the operator asked for. Opus stops pinning 0.95+; the ceiling becomes "match Opus's median per-check performance across the whole sheet," which even Opus does not do.
- Wait — note the direction: since `t_c ≤ 1`, `s/t_c ≥ s`, i.e. rescaling by itself is *lenient* for scores near t_c. The punishing effect comes from pairing it with the sb-6 check set being genuinely harder (checks whose raw Opus median lands at 0.6–0.9, not 1.0). The rule's job is **placement of the full-credit bar at observed-Opus, not at theoretical-perfect** — so the scoreboard resolves differences *between* strong models instead of saturating. Both halves (harder checks + Opus-median bar) are required; neither alone fixes the compression.

**Exemptions (all recorded in the thresholds file with a reason):**

1. **Binary checks** — every baseline score across all 13 runs is exactly 0 or 1: leave binary (a quantile on a two-point distribution is noise).
2. **HARD-block checks** — gates, never rescaled.
3. **Non-discriminating checks** — `opus_med(c) − haiku_med(c) < 0.05`: keep raw and FLAG for the spec workstream (a check Haiku matches Opus on is measuring nothing at the top; it either moves to Tier A floor duty or gets deepened).
4. **Dead checks** — zero for all 13 runs. An observed zero licenses NOTHING (the negative-proof rule): before acting, prove the check can fire at all by running the scorer against the golden control tree (`bench/controls.py` HIGH property, extended with a v3 golden tree). Check fires on golden → the models genuinely all fail it → it stays (it is headroom). Check cannot fire even on golden → grader defect → fix and re-sweep.

**Fitting script** (`bench/fit_thresholds.py`, ~60 lines, to be created at implementation time — algorithm):

```python
# read runs/calib-v3/{opus-5,sonnet-5,haiku-4.5}-r*/verdict.json
# assert all scorer_version == "sb-6.0-rcN" and identical N; refuse otherwise
# per check: medians as above; classify binary/hard/dead/non-discriminating/calibrated
# emit runs/calib-v3/thresholds-sb6.json:
# { "fitted_from": {"scorer": "sb-6.0-rc2", "engine_sha": ..., "date": ...,
#     "model_ids": {...}},                       # Bedrock aliases can re-point; pin the fit context
#   "checks": { "<name>": {"t": 0.83, "class": "calibrated",
#                "opus": [...], "sonnet_med": ..., "haiku_med": ...}, ... } }
# print the acceptance-gate report (below)
```

The scorer consumes the file at load time and **refuses to score if the file's sha256 does not match the constant baked next to `SCORER_VERSION`** — a threshold file must shadow like a gate, not drift like a note.

---

## 3. The iteration loop → freeze

```
draft spec-build-v3.md + sb-6.0-rc1 checks (raw thresholds = 1.0)
   │
   ▼
[SMOKE] 1 Opus run  ──fail (episode broken / probe crashes / check unreachable)──► fix, rc bump
   │ pass
   ▼
[CONTROLS] bench/controls.py with a v3 golden tree + v3 defect set (incl. ≥2 3D defects)
   │ HIGH + LOW + ISOLATION pass
   ▼
[BASELINE SWEEP] 6 + 4 + 3 runs (§1)
   │
   ▼
[FIT] fit_thresholds.py → thresholds-sb6.json + gate report
   │
   ▼
[ACCEPTANCE GATES] (on rescaled totals)
   G1 ordering:      mean(Opus) > mean(Sonnet) > mean(Haiku), each gap ≥ 0.04
   G2 headroom:      mean(Opus) ∈ [0.80, 0.90]   — above 0.92 = spec still too easy → deepen
   G3 resolution:    ≥ 1/3 of graded checks show opus_med − haiku_med ≥ 0.15
   G4 no dead grader: every check fires on the golden tree
   G5 free-check cap: ≤ 10% of checks score 1.0 for ALL runs of ALL three models
   │ any gate fails ──► amend spec/checks, bump to rc(N+1), FULL re-sweep
   │                    (any check/weight/fixture change voids comparability — the
   │                     scorer's own version rule; no partial re-runs)
   ▼ all pass
[FREEZE]  SCORER_VERSION = "sb-6.0"
          thresholds-sb6.json committed; sha256 baked into score_build.py
          spec-build-v3.md frozen; ENGINE.sha recorded
          the 13 baseline verdicts committed to the ledger as the sb-6.0 anchor rows
          REGIME.env flips BENCH_SPEC to spec-build-v3.md when the local fleet enters
```

Budget **2–3 iterations**. The first sweep nearly always fails G2 or G3 — that is the loop working, not failing.

---

## 4. Time and token cost per full calibration sweep

**Wall clock (serial):** under v3 (harder), Opus est. 20–30 min/run → 6 runs ≈ 2.5 h; Sonnet 10–15 min → 4 runs ≈ 50 min; Haiku 8–12 min → 3 runs ≈ 30 min; + scoring/probe overhead ≈ 30 min. **≈ 4–5 h per sweep**; a 3-iteration calibration ≈ 1.5–2 elapsed days interleaved with the live benchmark.

**Tokens (estimate — confidence MEDIUM; goose doesn't surface usage in verdicts, which is why §1 adds usage capture; re-base after sweep 1):** an agentic build episode of this length runs ~30–60 turns with context growing to ~30–60k tokens; billed volume is dominated by cache reads.

| Per run | Cache reads | Writes+uncached in | Output | List-price equivalent* |
|---|---|---|---|---|
| Opus 5 ($5/$25/MTok; reads $0.50, writes $6.25) | 2–4 M | 150–400 k | 40–80 k | **$3–6** |
| Sonnet 5 ($3/$15 — intro $2/$10 to 2026-08-31; reads $0.30) | 1–2 M | 100–250 k | 30–60 k | **$1–2.5** |
| Haiku 4.5 ($1/$5) | 0.5–1 M | 80–200 k | 20–50 k | **$0.4–0.8** |

**Per 13-run sweep: ≈ 20–30 M billed-equivalent tokens, ≈ $25–50 list-equivalent. Full 3-iteration calibration: ≈ $75–150.**

\* First-party list rates as the reference; Bedrock is partner-priced separately. The current bearer-token arrangement has made these runs free-in-practice for this campaign — the estimate exists for budgeting, rate-limit headroom, and the day the arrangement changes. Practical constraint that HAS bitten: token freshness (the env file rotates) — pre-flight check in §1.

---

## 5. Drift protection

**Re-calibration triggers (each names its response):**

| Trigger | Response |
|---|---|
| ANY change to a check, weight, fixture, spec text, probe logic (`product_probe.mjs`), or vendor mock | `SCORER_VERSION` bump + FULL re-sweep + refit. Already the codebase's own law ("bump on ANY change… a verdict carrying a different version is not comparable") — the thresholds file inherits it. |
| Playwright / headless-Chromium version bump | Offline re-score of the archived golden tree (no model runs): the frozen verdict must reproduce within ±0.005 total, else the V/J checks drifted → treat as a probe change (row above). |
| Bedrock alias drift (`us.anthropic.claude-opus-5` is a floating alias; a snapshot re-point silently changes the threshold-setter) | `thresholds-sb6.json` pins fit date + exact model ids. Quarterly, or on any announced model update, run a **3-rep Opus probe**: if its mean drifts > 0.05 from the frozen anchor mean, refit thresholds on a fresh 6-rep Opus sweep → **sb-6.1** (thresholds-only bump; checks unchanged). |
| Engine binary change (`ENGINE.sha` mismatch) | Entrant-harness change → anchor rows void → full re-sweep under the new binary before any new sb-6 entry is scored. |
| Threshold-file tamper | Structural, not procedural: the scorer refuses on sha256 mismatch (§2). |

**Version-bump rules:** `sb-6.0` = frozen checks + frozen thresholds. Thresholds-only refit (alias drift, quarterly probe) = `sb-6.1`, `sb-6.2`, … Check/weight/spec change = `sb-6.x → sb-7-rc1` and the full loop in §3 runs again. Rc versions (`sb-6.0-rcN`) never appear on any published board.

**Comparability vs sb-5.x:** none, and none is claimed. Different spec, different app, different check set — an sb-6 number and an sb-5 number NEVER share a table, a chart, or a sentence implying magnitude comparison. The bridge is ordinal only: entrants appearing on both boards (opus-5, sonnet-5, haiku-4.5, local-single, swarm-Nnode) get a rank-correlation note (expected to preserve opus > sonnet > 3-node > haiku > 1-node; a rank inversion is a finding, not an embarrassment). sb-5 entries and their `_sb4trees` archives stay frozen as the historical record; the local fleet re-enters under sb-6 with the same rep discipline as the cloud anchors (n=3 minimum, same catastrophe rule).

---

## Confidence statement

- **High:** pipeline mechanics, model ids, env/credential handling, result paths, commands — all verified against the repo and existing run artifacts; the rep-count variance basis is measured, not assumed.
- **Medium:** token/cost figures (no usage telemetry in verdicts yet — the protocol's usage-capture step exists precisely to replace this estimate with measurement after sweep 1).
- **Medium:** acceptance-gate bands (G2's [0.80, 0.90], G3's 1/3-at-0.15). They encode the operator's intent (break the top-end compression) but the first baseline sweep is what shows whether v3 lands them; the loop is built to adjust the SPEC to the bands, not the bands to the spec.
- **Known subtlety flagged honestly:** the quantile rule alone is score-inflating (dividing by t ≤ 1); the punishment comes from the harder v3 check set placing Opus medians well below 1.0. If the spec workstream under-delivers on difficulty, G2 catches it (Opus mean > 0.92 → iterate) — the gate, not anyone's memory, enforces the intent.