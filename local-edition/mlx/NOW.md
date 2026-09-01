# NOW — MLX in-house engine campaign (branch goose/mlx-inferencing)

## REVIEW + FIX CAMPAIGN (2026-09-02) — IN FLIGHT, fully autonomous ("yes do it all")
PROGRESS (latest, 2026-09-02 ~02:00 EEST; full per-item detail in the session scratchpad review-findings-index.md):
DONE+COMMITTED: worker (ffc2e38a0..94fa06376 + 5ab48a8c2 sweeper; LIVE, lockout gone, nodeSecret contract); swarm.rs pass 1
(a7ffb0516 524d104c0 743c4d511 9d5958f19 7f74d3730; tracer: S-H1 premise fails on this fleet — LM Studio /v1/models answers 401 to
the unauthenticated probe → EVERY servability consumer is "unproven" here → LM_API_TOKEN probe = wave-2 item 1); goose-sidecar
(fcab044c0 71189f66b e09790ad0; tracer: breaker trips at the 4TH death not 3rd; lsof -ti :PORT listed CLIENTS → old reclaim could
have SIGTERMed goosed itself, fixed w/ LISTEN filter); goose link layer (7a745914f..94fd2ccdf: key LEANZERO_LINK_ALLOW_REMOTE_EXECUTION
default OFF, receiver's mode, validated cwd, both busy doors, no email token, status DTO remoteExecutionAllowed/Wired,
mlxControlWired, meshBinaries); desktop logic (03b291c84..6512c99e7); design system "LeanZero Studio" (d64068fb6..77dd6f690,
ui/desktop/DESIGN.md); stage-1 polish (575a141a3..cdcc6588c, screenshots-passE); remake: shell/nav/projects/landing
(8ccdbd9b9..a98366653), session chrome (9ace453ee 1b0ede59d 2f13f54ba), hub+Link+nodes (e610a589d..249052a76), benchmark+settings
(efafa4700..785defe08). IN FLIGHT: leanzero-link crate fixes (+ local_sessions→Result trait change), C2 goose-serve injection
(link_serve.rs), Q1 MLX busy signal, wave-2 swarm pass (token probe, planner unregistered arm, canonical name w/ engine kind,
dev-gates §4), tick.py new events, remake: MLX engine/models (+StudioSelect reuse, cloud.tsx), swarm run panel (+2 deferred
behavior edits), chrome leftovers (+ui/button|tabs|input solid disabled, lz Button iconOnly, AppLayout frame), confirm-on-close
dialog. AFTER ALL LAND: lz/Segmented additive extension (role/title/describedby) then swap benchmark/settings pickers; promote
leanzero-swarm/studio.tsx controls into lz/; `pnpm i18n:extract` + validate ONCE; rebuild goosed (`just release-binary`) →
`pnpm run package` (tailscaled bundled via fetch-tailscale) → launch packaged app on CDP 9897 → screenshots-passF light+dark
(scratchpad/cdp-shot.mjs; hub tabs are now role=radio/data-value) → ONE REAL Link connect with receipts (identity.json + node-id +
"tailscaled ready" + "control service listening" + <host>-<6hex> node on Headscale + /execute 202). NOTE: `just run-ui` dev mode
is BROKEN (Vite renderer dev server closes during dep-scan) — package+CDP is the verification path.
Branch review vs main: 6 read-only reviewers (Link Rust / Link worker / fallback-hunter / works-prover / swarm engine / desktop UI)
→ 6 adversarial refuters re-measuring from primary sources. 34 findings → 30 survived, 4 killed, 9 reviewer-proposed FIXES caught
wrong. Full index with every refuter verdict + corrected spec: session scratchpad `review-findings-index.md` (copy the essentials
here if it is gone). TOP TRUTHS (all refuter-confirmed): (1) the worker's IP rate-limit keyed on a header Funnel never sets → GLOBAL
sign-in lockout was LIVE — FIXED+DEPLOYED (worker commits ffc2e38a0..94fa06376, Funnel X-Forwarded-For measured per-client); (2) remote
execute + mlx proxy are DEAD in the shipped app — injection only in `goosed agent`, desktop runs `goose serve` → 501 "not wired";
(3) the app's own connect path (connect_inner) has NEVER run against Headscale (no ~/.leanzero/node-id; all mocks omit loginServer) —
my earlier "proven e2e" was the hand-driven tailscaled chain; (4) no built app contains tailscaled; (5) idle guard never fires (busy
read from a map only orchestrator subagents fill); (6) remote exec default-on, Auto mode even for Approve users, unvalidated cwd;
(7) node_token derivable from the email → replaced by a worker-issued per-account nodeSecret (contract shipped in the worker);
(8) swarm: mixed LM+MLX pool silently moves a sidecar planner to LM Studio; an LM probe hiccup empties the LM half of every fan once a
sidecar serves; sidecar fabricates Running on an occupied port; SIGKILL orphans the uv grandchild (killpg on its OWN group is
gate-consistent); (9) desktop: Cmd+W on a session window KILLS a live swarm run; dead-lane demotion inert by default + blind to MLX.
FIX WAVE 1 (parallel, one agent per disjoint file set, TRACE in every commit): worker DONE; crates/leanzero-link (crate agent);
goose link.rs + remote_executor + both-door busy registration (C1); swarm.rs/swarm_engine.rs (swarm-surgeon); goose-sidecar
(mlx-backend); desktop main/useFleet/csp/useSwarmRun (panel-surgeon). WAVE 2 after: C2 = wire executor/mlx/delta under `goose serve`
(brief: scratchpad wave2-injection-brief.md — mlx control is a one-liner; executor+delta need goose-crate impls over on_prompt's reply
path), fix-tracers on the engine commits, rebuild+repackage (tailscaled now bundled via `just fetch-tailscale`), then ONE REAL app
connect with receipts (identity.json + node-id + "tailscaled ready"/"control service listening" + a <host>-<6hex> node on Headscale
+ /execute 202 not 501).
UI: owner wants "a really nice and welcome remake" (visual only, functionality untouched, leanzero.net = light accent reference).
Stage-1 polish (enforce the .local-edition token doctrine) running; design system "LeanZero Studio" (Inter via @fontsource, slate
surfaces, ONE accent #1d4ed8, violet secondary for the reasoning channel, node ramp for nodes only, lz/* primitives + ui/desktop/
DESIGN.md) being built; then per-surface remake agents (spec: scratchpad ui-remake-spec.md — nav/projects, landing, session chrome,
LeanZero Swarm hub + models DataTable, Link section, swarm run panel as mission control, benchmark/settings harmonized), each verified
in the RUNNING app with light+dark screenshots into local-edition/mlx/screenshots-passF/.

## MULTI-TENANT MESH — DONE & LIVE (2026-09-01, commit ff9c53b59)
"Everyone connects to their OWN swarm." Moved OFF Tailscale hosted control (personal tailnet, no
isolation) ONTO **self-hosted Headscale** — each account = one Headscale user = the isolation unit.
PROVEN END TO END before commit (scripts in ~/.leanzero/hs-iso-test/):
- Node join works against a ROOT url in ~2-3s. The earlier funnel /headscale HANG was the path-strip
  breaking the noise /ts2021 protocol. FIX: Headscale mounted at the funnel **ROOT** `/` on :443
  (`tailscale funnel --https=443 http://127.0.0.1:8790`) — root mount does NOT strip, so the control
  protocol survives; the more-specific MCP sub-paths (/docproc,/websearch,/lmstudio,/leanzero-link)
  still win their matches → MCPs UNDISTURBED. `/headscale` path removed (redundant).
- Isolation is NOT free: Headscale default = allow-all. Static policy `{src:["*"],dst:["autogroup:
  self:*"]}` (ONE policy for ALL accounts) gives automatic per-account isolation. VERIFIED: 2 accounts,
  3 nodes — same-account nodes see each other, cross-account do NOT. Full-chain rerun (JWT→worker→key→
  join→netmap) HOLDS.
- Worker (leanzero-link/worker): new src/lib/headscale.ts mints per-account ephemeral preauth keys
  against the Headscale REST API (every shape measured live: preauthkey `user`=NUMERIC id; policy
  get/put self-heals isolation before any mint; username acct-<sha256(email)[:16]>, PII-free); join-key
  response now carries `loginServer`. config.ts meshProvider headscale>tailscale>none. 10 new tests, 85 green.
- Rust: JoinKeyResult.login_server (Option, serde default); manager.connect_inner overrides
  mesh_config.login_server with the worker's value so key+control-server travel together. cargo test/clippy green.
- PERSISTENCE: Headscale under launchd **com.leanzero.headscale** (KeepAlive; binary ~/.leanzero/bin/
  headscale v0.29.3; config ~/.leanzero/headscale/config.yaml; DB sqlite; API key ~/.leanzero/headscale/
  api-key.txt 0600). Worker launchd plist PATH fixed to /usr/local/bin (brew node 25.9.0 is BROKEN —
  missing libllhttp — it was flapping the worker; /usr/local/bin/node v24.15.0 works). Both services up.
- Env: ~/.leanzero/link-worker.env adds HEADSCALE_API_URL(127.0.0.1:8790)/HEADSCALE_API_KEY/
  HEADSCALE_LOGIN_SERVER(https://worksmacstudio.tailfc4700.ts.net).
BOTH prior "remaining for other users" items now DONE:
- RESEND DOMAIN: verified domain `leanzero.atlascrafted.com` (sending enabled) already existed on the
  account. LEANZERO_MAIL_FROM switched to "LeanZero Link <link@leanzero.atlascrafted.com>" (env file needs
  QUOTES — value has spaces/<>, bash sources it). PROVEN: direct Resend send to a non-owner (gabriela@
  leanzero.net) → HTTP 200; worker request-code → otp_issued (handler 502s loud if Resend refuses, so
  ok:true is real). OTP now reaches any address.
- TAILSCALED BUNDLE: binaries ship in ui/desktop/src/bin (gitignored, like the goose binary) via forge
  extraResource; ui/desktop/scripts/fetch-tailscale.sh populates them (go build @pinned v1.98.5 if go
  present, else copy from a system install — host arch only); `just fetch-tailscale` recipe signs them
  ad-hoc + verifies execute, hooked into `copy-binary`. gooseServe.ts buildGooseServeEnv sets LEANZERO_
  TAILSCALED/LEANZERO_TAILSCALE_CLI to the bundled paths WHEN PRESENT (discovery.rs env override wins;
  absent → falls through to PATH/known — an explicit env override still wins). PROVEN: the bundled
  src/bin/tailscaled+tailscale joined Headscale in 3s via a worker key, exact mesh.rs invocation. Cross-arch
  (intel/universal) bundle needs Go (TS_GOARCH) — NOT auto-hooked into copy-binary-intel to avoid a silent
  wrong-arch ship. Windows tailscaled.exe not yet bundled (mac DMG is the primary target).
Personal Tailscale + benchmark fleet verified UNTOUCHED throughout. Brew node 25.9.0 on the Mac is BROKEN
(missing libllhttp) — ui vitest can't run here; the bundle is proven empirically instead. `brew reinstall node`.

## NEW CHAPTER (2026-09-01): LEANZERO LINK
## MIHAI'S DECISIONS 2026-09-01 (post-core): build #2 and #3; #1 is his to deploy
- #1 WORKER DEPLOY: Mihai's — the runbook is given (Cloudflare+Resend+Tailscale accounts → 5 secrets
  RESEND_API_KEY/RESEND_AUDIENCE_ID/LINK_JWT_SECRET/TS_API_TOKEN/TS_TAILNET + LEANZERO_MAIL_FROM;
  he runs `wrangler login`, then I can run kv-create + secret-put + deploy if he pastes values;
  he gives the deployed URL → I bake it as LEANZERO_LINK_WORKER_URL default + rebuild). Until then
  everything connected-state is unit-only.
- #2 SWARM-ENGINE GRAFT (#22): SHELVED by Mihai after the design (2026-09-01). Build-fan proved
  UNSAFE (mesh moves events not files; swarm completes on LOCAL files → silent-break). Safe slice
  (advisory-review fan) adds less than the delegation UI already gives. Engine untouched. Full
  distributed-builds would need a file-return subsystem = separate project, not queued.
- #3 SMALL SEMANTICS: YES. #13 weight = ROUTING SHARE (speed_weight), not concurrency — nodes-tab
  stepper should write speed_weights (UI-side, panel-surgeon, independent of swarm.rs). #17 arbitrary configured cloud
  providers as nodes — its OWN pass now (#22 shelved, no longer folds in): first VERIFY whether the
  dispatcher already routes an arbitrary cloud provider name (is_cloud + create(provider_name)); if
  so it's a UI+config change (panel-surgeon), else a small swarm-surgeon CLOUD_DEFS change. After #13.
IN FLIGHT: node-picker UI (panel-surgeon, ui) + #22 design (swarm-surgeon, read-only). NEXT: review
#22 design → authorize build (+#17 swarm.rs); panel-surgeon for #13 + #17 UI after node-picker lands.

## LEANZERO LINK: FULLY LIVE (2026-09-01) — every layer proven end-to-end
Worker self-hosted on the Mac (launchd com.leanzero.link-auth, KeepAlive) + Tailscale Funnel path
/leanzero-link on :443 (Mihai's MCP funnels untouched; backup ~/.leanzero/funnel-config.backup.json).
Public: https://worksmacstudio.tailfc4700.ts.net/leanzero-link (TS strips the prefix). health
mail=true mesh=true audience=false. PROVEN LIVE: OTP request→Resend (test-mode delivers only to the
Resend owner zerobarat1@gmail.com); join-key→untagged ephemeral Tailscale key (after fixes: TS_NODE_TAG
empty=untagged, AND Tailscale rejects '.'/'@' in key descriptions → email sanitized to [A-Za-z0-9_-],
c07bc0380). goosed points at the live worker via launchctl setenv LEANZERO_LINK_WORKER_URL (+ baked
as DEFAULT_WORKER_BASE_URL in worker_client.rs for reboot-permanence; a niced goosed rebuild to bundle
that was kicked — repackage to finish baking; until then the env holds it).
TEST: app → LeanZero Swarm → LeanZero Link → email zerobarat1@gmail.com → code to that inbox → verify →
connect. Secrets in ~/.leanzero/link-worker.env (0600). Audience id unset (sync skips; add RESEND_
AUDIENCE_ID to enable). To use any From address, verify a Resend domain + change LEANZERO_MAIL_FROM.
REMAINING (all Mihai-side / optional): #17 arbitrary cloud providers as nodes (own pass) + #23 add-dialog
label; verify a Resend domain for non-owner emails; repackage to bake the URL into the bundle. #22 SHELVED.

## LEANZERO LINK — BUILDING THE DEEPER HALF (Mihai said continue, 2026-09-01)
Both decisions → BUILD. Sequential chain (all share the goose-server↔link seam, so no parallel):
  Agent 1 (IN FLIGHT, link-backend): full per-message mirroring — goose-server process-wide delta
    tap injected into GoosedSwarmStateSource; completes P4. Read-only broadcast, low risk.
  Agent 2 (NEXT, link-backend): cross-node REMOTE-EXECUTE endpoint on the control service + a
    goose-server-backed LocalExecutor (run a prompt+cwd through the real agent, stream results).
    NEW security surface = execute-on-your-own-devices; trust = tailnet membership + node_token
    bearer (same-account only) — enforce + document LOUDLY. This is the acting path the idle guard needs.
  Agent 3 (AFTER 2, swarm-surgeon): dispatcher idle-guard — swarm routes eligible tasks to Idle
    mesh PEERS via the execute endpoint (helpers exist: link.rs fetch_local_swarm_nodes /
    node_token_from_email; port 41226; peers[].status Idle|Busy|Offline); all-busy → local fallback/queue.
  Then panel-surgeon: surface remote-run + the queued staleness gate (#20).
Backend contracts all banked in LEDGER (worker/mesh/control/manager/goosed). Fleet benchmark still live.

Mihai's five-phase spec: passwordless Resend-OTP identity (~/.leanzero/identity.json), cross-WAN
mesh via Tailscale, idle-node execution guards, real-time session mirroring, companion-app-ready
/v1/swarm/{nodes,sessions,stream} APIs. MEASURED HEAD START: this machine is ALREADY on a live
tailnet (worksmacstudio 100.122.51.13 + Mihai-Macbook-2 100.83.119.44, direct connection carrying
the benchmark; Funnel enabled; gabee NOT on the tailnet yet). Architecture decisions taken (defaults,
Mihai can override): auth backend = self-hostable Cloudflare Worker in leanzero-link/worker/
(holds RESEND_API_KEY/AUDIENCE_ID/LINK_JWT_SECRET/TS_API_TOKEN — secrets NEVER in the desktop);
CORRECTION (Mihai, 2026-09-01): DO NOT ride the existing tailnet — that is Mihai's PERSONAL
Tailscale (and LM Link is LM Studio's, separate again); Link must be fully independent so stopping
either leaves Link intact. Mesh = goose OWNS an EMBEDDED headless userspace tailscaled sidecar
(goose-sidecar supervisor pattern) with its OWN state dir ~/.leanzero/tailscale/, its own socket,
--tun=userspace-networking (no root, no system TUN, invisible to the system `tailscale` CLI),
joined to the ACCOUNT's isolated tailnet via the worker's ephemeral key. Two independent Tailscale
worlds on one machine; the personal-tailnet detection is NOT a dependency (at most a later
duplicate-install advisory). Mirroring v1 = WS pubsub of
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
