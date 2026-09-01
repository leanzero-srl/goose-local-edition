# NOW — MLX in-house engine campaign (branch goose/mlx-inferencing)

## NEW CHAPTER (2026-09-01): LEANZERO LINK
Mihai's five-phase spec: passwordless Resend-OTP identity (~/.leanzero/identity.json), cross-WAN
mesh via Tailscale, idle-node execution guards, real-time session mirroring, companion-app-ready
/v1/swarm/{nodes,sessions,stream} APIs. MEASURED HEAD START: this machine is ALREADY on a live
tailnet (worksmacstudio 100.122.51.13 + Mihai-Macbook-2 100.83.119.44, direct connection carrying
the benchmark; Funnel enabled; gabee NOT on the tailnet yet). Architecture decisions taken (defaults,
Mihai can override): auth backend = self-hostable Cloudflare Worker in leanzero-link/worker/
(holds RESEND_API_KEY/AUDIENCE_ID/LINK_JWT_SECRET/TS_API_TOKEN — secrets NEVER in the desktop);
mesh v1 rides the EXISTING tailnet (detect via `tailscale status --json`), embedded userspace
tailscaled-as-sidecar (goose-sidecar pattern) is the end-user path. Mirroring v1 = WS pubsub of
session deltas + read-only mirror; full DB bidirectional sync is explicit v2. IN FLIGHT: worker
build (docs-verified, full local test harness) + recon (session event bus, goosed HTTP surface,
busy/idle truth sources, workspace ws/jwt deps). NEXT: Rust crates/leanzero-link control service
(/v1/swarm/nodes idle-guard + /v1/swarm/stream WS) bound to the mesh IP; dispatcher idle-guard
seam via swarm-surgeon; Link tab UI via panel-surgeon. Fleet benchmark still live — same
read-only discipline.

## CURRENT CHAPTER (2026-08-31 evening): the Goose Swarm restructure
PASS E (in flight, ~22:30, Mihai reviewing live): (1) sessions are SESSIONS not chats incl.
default title; (2) stale swarm board GATED — a new session never shows a previous run's
planning/ETA (heartbeat-fresh or session-owned only; read-only toward .swarm); (3) honest Nodes
strip on blank sessions from configured devices (mlx violet + cloud chips, no fake occupancy);
(4) LM Studio session-UI rows behind settings 'showLmStudioFleet' DEFAULT FALSE — disabled never
deleted; (5) LIVE FLEET BENCHMARK RUNNING — zero LM Studio contact, renderer-only niced rebuild,
no cargo; (6) extensions icon removed from session bar; (7) no pricing UI on omlx sessions;
(8) "Make recipe from this conversation" removed from the name menu; (9) AI auto-naming of
sessions surfaces in the header (investigate existing backend naming first — report which world);
(10) bigger rename hitbox. Publishing still HELD.
PASS D (in flight, ~21:15, amended ~21:30): Mihai — "New chat button removed. Sessions are
started from project only." Nav loses New Chat; every other new-chat affordance removed or
redirected; home route becomes a project-directed landing; per-project "+ New session" is the
ONLY creation path. AMENDED IN: the Models area splits into [Hugging Face | Downloaded]
second-level tabs (downloaded was buried below the browser; folder row + disk bar move to
Downloaded; progress rows visible on both), and the legacy "Swarm LeanZero" lever tab is REMOVED
from Settings (section stays in code; the simple nodes tab is the only swarm surface now).
Publishing HELD until Mihai calls the restructure complete (2.0.0 + 2.0.1 already on the feed).
AMENDMENT (Mihai, ~20:00): the Swarm Settings tab is RADICALLY SIMPLE — Add node / provider per
node / one weight per node, NOTHING else ("I don't want all of those levers… That is it.
Seriously."); the legacy lever section stays in Settings untouched until the golden-formula strip.
MLX nodes auto-cap to the swarm's discovered MACHINES (LM Link surface): 5-machine swarm = 5
addable LeanZero MLX nodes; cloud unlimited. Remote-machine MLX nodes addable but wear a solid
"awaiting fleet routing" chip until per-node engine endpoints ship (never fake reachability).
Mihai's five-phase UI reorg: (1) "merge main for Benchmark" — MEASURED no-op, we contain all of
main (0 behind / 868 ahead); Benchmark was hidden because the renderer's edition is a stored
setting defaulting 'standard' (never provider-derived like edition.rs) — same root cause as the
.local-edition CSS scope being absent. Pass A (in flight, panel-surgeon): edition derives from
provider (fragments incl. mlx), rebrand "Goose Local Edition"→"Goose Swarm", nav declutter to
[New Chat, Skills, Memories, Benchmark, Leanzero MLX, Settings], drop Settings>Models. (2) Recon
in flight (Explore): session working-dir metadata + swarm-settings/provider-credential plumbing.
(3) Pending passes: B = ChatGPT-style Projects tree (projects as local paths, sessions grouped
under them, new-session inherits the project's cwd); C = "LeanZero Swarm" three-tab view (LeanZero
MLX | Cloud Providers | Swarm Settings with node creation + per-node provider [mlx-or-cloud] +
weight), plus components/mlx → leanzero-swarm namespace rename. (4) UPDATE SEVERANCE (Mihai):
the app must never check the parent goose for updates again + own versioning — folded into Pass A;
version line breaks to 2.0.0 (above upstream 1.x forever; installed 1.41.x updates forward once
our feed publishes). (5) After Pass A: build the FULL DMG via `just release-fork 2.0.0` (Mihai:
"build the full DMG to check"), again at the end of the restructure.

**Read this FIRST after any compaction, before anything else in this dir.** Budget ~2k tokens.
Rule: when the thread changes, this file changes IN THE SAME COMMIT.

## The mission (Mihai, 2026-08-30)
Add an in-house, open-source MLX inference engine that goose itself supervises — **NEXT TO LM Studio,
never replacing it**. It supersedes only the built-in `goose-local-inference` FFI path. Primary
workload: **the swarm** — N concurrent long-context tool-calling agents. Plus: a starter desktop UI
window (engine control: mount/unmount, sampling incl. presence penalty, context length; HF browser:
MLX-only search + download into a configurable `models_dir`). Full plan:
`~/.claude/plans/i-want-you-to-enumerated-hejlsberg.md`.

## Hard constraints (Mihai's words, do not relax)
- Tests use **qwen3.5-9b 4-bit ONLY, freshly downloaded**. Mounting the fleet's 27B is FORBIDDEN for now.
  Amendment (Mihai, later that night): if the 9B itself proves inept at tool calls, downloading a
  similarly-sized tool-call-strong replacement is sanctioned. Status: NOT needed — the 9B scored 100%
  tool fidelity over 78 requests on rapid-mlx 0.13.1; any later fidelity gap on the same model is an
  ENGINE differentiator, not a model defect.
- The running LM Studio fleet must not be disturbed: memory gate before ANY mount (`gates.py`),
  `lms ps` snapshot/verify around every session, sidecar never on ports 1234/11434.
- One configurable `models_dir` (default `~/.goose/models`) for downloads AND mounts — not tied to LM Studio.

## State at last write (2026-08-30)
- Branch created off local-edition @ 5d238baa5 (pulled 779 commits first; agentic structure now in-repo).
- Finalists: **Rapid-MLX** (raullenchai/Rapid-MLX — tool parsers, decode speed, goose precedent) vs
  **oMLX** (jundot/omlx — continuous batching, prefix-sharing paged KV + SSD tier, memory guard).
  MTPLX + mlx-serve dropped → see TRUMP-CARDS.md. Verdict comes from the swarm-shaped bake-off, NOT web claims.
- **Inherited hazard (docs/EXPERIMENTS.md spike, 2026-06-25): on Gated-DeltaNet hybrids (all our qwens),
  a prefix-cache HIT can silently break tool-calling (omlx #825, mlx-lm #980). LM Studio was chosen over
  raw mlx-lm/omlx for exactly this. The bake-off scores this as a first-class dimension.**

## Restart, in order
1. `cat local-edition/mlx/NOW.md && cat local-edition/mlx/LEDGER.md | head -60`
2. `git log --oneline -10` on branch goose/mlx-inferencing
3. `python3 local-edition/mlx/gates.py probe` (memory state) and `~/.lmstudio/bin/lms ps` (fleet state)
4. Continue from "The one decision waiting" below.

## Operating mode — quality bar (Mihai, 2026-08-30, verbatim intent)
"Don't leave TODOs and fake implementations, don't do shitty fallbacks, never do horrible
deterministic stuff that no one cares about — find solutions to problems, that is your job, and
implement them correctly. That means putting in the effort." Applies to every artifact on this
branch: sidecar, UI, fork patches, bench harness. A missing input is a LOUD named absence, never a
quiet substitute; a mechanism measures and nudges, never gates for gating's sake.

## Operating mode
**UNATTENDED (Mihai, 2026-08-30 ~23:00, going to sleep): full autonomy for the night.** Decide and
act on best knowledge; no questions; maintain house rules + agentic discipline; commit every step;
keep this file and the ledger current so restrictions and thread survive compaction. The approved
plan is the authorization for all of it — bake-off, fork, integration, UI.

## The one decision waiting — MORNING STATE (2026-08-31 ~03:00)
EVERYTHING GREEN except two named holds. Done tonight: bake-off (8 runs, verdict Rapid-MLX),
fork live (leanzero-srl/Rapid-MLX @v0.13.1, uvx pin proven), goose-sidecar crate (supervisor +
gates), 11 ACP methods, desktop MLX Engine window verified in the RUNNING app (mount/sampling/
HF-download/delete, two spawn defects + gate-banner bug found live and fixed), SwarmEngine
boundary steps A/B/C in swarm.rs (surgeon; per-engine partition; TRACE YES), and the SWARM E2E
through the sidecar — real artifacts, 4/4 tests (LEDGER ~03:00; final banner lost to an external
kill-all of background tasks — rerun is one command).
HOLDS, both with reasons in LEDGER: (1) local-inference default-feature flip — the flag still
gates live desktop surfaces (onboarding/dictation/ModelSettingsPanel); needs Mihai's call on those
surfaces' fate. (2) Mixed-pool (LM planner + sidecar worker) run — blocked on the fleet's
LM_API_TOKEN, which lives on the MacBook.
MORNING one-liners: clean-banner swarm rerun (command in LEDGER); `just run-ui` to reopen the
desktop (app was killed with the task sweep); provide LM token for the mixed run.
Branch pushed to origin through the latest commit.

## Superseded waiting-item (kept for the record)
Bake-off verdict, pending oMLX numbers. State (2026-08-30 ~23:00): rapid-mlx 0.13.1 DONE — 3
concordant runs in experiments.jsonl (fidelity 1.0 at N=1/4/8, TTFT ~1.7/3.8/7.5s, aggregate up to
~50 tps, no hybrid footgun, RSS max 4.4 GB, serves ~/.goose/models path directly, hermes parser
auto-selected). rapid stopped cleanly (SIGTERM per-pid), fleet gate ALLOW throughout. oMLX 0.6.4
serving on :8091 from --model-dir ~/.goose/models (id Qwen3.5-9B-MLX-4bit), bench running 2x.
Next after verdict: LEDGER entry + fork the winner (leanzero GitHub, upstream remote) + build/test
crates/goose-sidecar (authored+committed, build held during scored runs) + swarm.rs EngineAdapter
via the swarm-surgeon agent + desktop window. Bench instrument fixed twice (req_tps replaces
decode_tps; RSS sampler fails loud) — first experiments.jsonl row's decode_tps is void.
