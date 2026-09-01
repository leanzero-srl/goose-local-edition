# MLX engine campaign — LEDGER

## 2026-09-01 — LEANZERO LINK recon: the seams the build binds to (survives compaction)

Isolation (measured): personal tailscaled = /usr/local/bin/tailscaled on /var/run/tailscaled.socket
(Tailscale.app, carrying the live benchmark — DO NOT TOUCH); the sidecar uses the SEPARATE Homebrew
tailscaled 1.98.5 (/opt/homebrew/bin) with ~/.leanzero/tailscale/{state,sock} + userspace-networking
— different binary/socket/state, provably non-interfering. Homebrew tailscaled uses single-dash Go
flags (-socket, -statedir; -tun to be confirmed by the mesh agent).

Session mirroring seam: goose-server/src/session_event_bus.rs ALREADY is a subscribe-able bus —
broadcast::Sender<SessionEvent> cap 256 + 512 replay buffer + AtomicU64 seq; subscribe(last_id)
returns replay+receiver (ClientTooFarBehind on eviction). BUT per-session (AppState.session_buses
map), no process-wide bus, no session-created/archived event — the control service iterates the map
and adds lifecycle events. Payload = MessageEvent (routes/reply.rs:128). SSE precedent live at
GET /sessions/{id}/events (Last-Event-ID honored, comment-heartbeat). axum ws in both server crates;
/acp is the existing WS (token via ?token= query, origin-policy gated).

Idle/busy truth: AgentManager (execution/manager.rs) global singleton — is_session_busy(id),
list_active_session_ids(); caveat normal reply paths under-register, so the honest node-busy signal
is "any session bus has an active_request" (session_event_bus active_requests) ∪ ACP
active_prompt_runs. Cross-node liveness prior art = SwarmEngine::resident_processes (swarm_engine.rs)
+ live_fleet_slots. Dispatcher idle-guard seam: scheduler.rs DeviceAdmission.offer (:771, mid-run
join, append-only) + least_loaded_free_device (:1450); DispatchError::Transient re-steers to another
device (natural "peer busy" path). swarm-surgeon owns these edits.

Identity/token: jsonwebtoken 10.2 present (sign-only today; the Rust client only STORES the
worker-minted JWT, never verifies its own). Secret plumbing needs ZERO new code
(Config set_secret/get_secret + LEANZERO_LINK_TOKEN env override) — but the SPEC wants
~/.leanzero/identity.json as source of truth, honored. goosed secret = GOOSE_SERVER__SECRET_KEY;
desktop finds goosed on a random 127.0.0.1 port (gooseServe.ts findAvailablePort, no port file).

IN FLIGHT: worker (leanzero-link/worker/) + mesh sidecar crate (crates/leanzero-link/src/mesh.rs).
NEXT (sequenced): control module in the same crate (/v1/swarm/nodes|sessions|stream bound to the
mesh IP) after mesh lands; identity client after worker contract; dispatcher idle-guard via
swarm-surgeon; Link tab UI via panel-surgeon.

## 2026-08-31 ~20:45 — Restructure COMPLETE (pass C); one testing casualty owned and repaired

Pass C (0fc7a4dd9): the LeanZero Swarm three-tab view is live — LeanZero MLX (engine content
nested intact), Cloud Providers (62 cloud cards, zero local leaks, reused modal/acp wholesale),
and the RADICALLY SIMPLE nodes tab per Mihai's amendment (add node / provider per node / one
weight / remove — nothing else; machine-capped MLX adds discovered via lms ps ∪ LM Link prefixes;
remote nodes wear the amber "awaiting fleet routing" truth chip; cloud stays CLI-only). Namespace
renamed components/mlx → leanzero-swarm; /mlx-engine redirects. Queued #8/#9/#10 cleared (issue
links → leanzero repo; release-fork refuses <2.0.0 — proven both directions; main-process brand
from one resolver, which live-caught that this config uses active_provider not GOOSE_PROVIDER).
1286 tests; live end-to-end incl. byte-verified config round-trip and `swarm pool` re-parsing it.

**Casualty, owned:** the mlx-backend first delegation's live lifecycle test ran cancel-now-DELETES
against the REAL ~/.goose/models (16:46) and wiped the 9B + the 8-bit residue while its report
said "fixtures untouched" (true only of the two dirs it named). Charter law 7 added (destructive
live tests take a TEMPDIR models root, always); retroactive MISS graded in ROSTER; the 9B
re-downloaded the same evening. The 2026-08-31 late-afternoon entry's claim stands corrected here
— evidence annotated, never deleted.

**Load-bearing gap found by pass C:** LM Studio's :1234 HTTP surface now requires an API token →
the desktop fleet panel reads offline everywhere (discovery still works via lms ps IPC). Queued
#12: token setting + Authorization header. Also #13 pending Mihai's word: the nodes-tab weight
currently maps to SwarmDevice.weight (concurrency); routing share would be speed_weight.

## 2026-08-31 night — Goose Swarm 2.0.0: pass A shipped, parent updates severed, DMG published

Pass A (40fa6c5c2, panel-surgeon): edition now derives from the provider like the Rust resolver
(the blocker was defaultSettings.edition masquerading as an explicit choice); Benchmark restored;
rebrand "Goose Swarm" everywhere user-visible; nav = [New Chat, Skills, Memories, Benchmark,
Leanzero MLX, Settings]; Settings loses Models. UPDATE SEVERANCE: the parent's owner/repo was
define-baked into packaged builds at bundle time — repointed to leanzero-srl/goose-local-edition
with a network-log proof and a zero-parent-marker pin test across all four update-path files.
Version line broken to 2.0.0 (own line, above upstream 1.x forever).

DMG: signing identity minted on this machine (just setup-signing-identity; first release-fork run
failed loud on the missing cert), then `just release-fork 2.0.0` sealed
ui/desktop/out/make/Goose-2.0.0.dmg (211 MB, stable self-signed identity). **Release v2.0.0
PUBLISHED on leanzero-srl/goose-local-edition with latest-mac.yml + DMG + zip — the own update
feed is live; installed 1.41.x builds roll forward into Goose Swarm.**

Operational lesson banked to memory: the harness reaps backgrounded process trees ~60s after a
turn ends (this was every "mystery kill" incl. the 03:00 swarm run) — anything that must survive
launches via open/launchd or nohup+disown to ppid 1. The app now runs from the SEALED 2.0.0
bundle under launchd (ppid 1). Pass B (Projects tree) in flight; pass C queued.

## 2026-08-31 evening — Goose Swarm restructure recon (facts that size passes B and C)

- "Merge main for Benchmark" measured a NO-OP: 0 behind / 868 ahead; Benchmark was hidden by the
  renderer's edition setting defaulting 'standard' (never provider-derived) — also why the
  .local-edition CSS scope was absent. Pass A fixes derivation + rebrands Goose Swarm + severs
  parent auto-update (own 2.0.0 line).
- PROJECTS TREE (pass B) is mostly seams, not construction: SessionListItem already carries
  workingDir on every row; SQLite filtering by working_dir EXISTS server-side and is wired to the
  ACP request's cwd with filter-hash-bound cursors — the desktop's acpListSessions simply never
  sends cwd (one-line addition). createSession(workingDir) is already parameterized (inheritance
  is call-site choice). recent-dirs.json is an LRU, not a registry → projects get their own
  user-curated projects.json (same file-backed pattern). A project_id column is plumbed
  end-to-end but never written — left dormant; the PATH is the project key by design.
- THREE-TAB VIEW (pass C): SwarmSettingsSection already renders device rows with weights,
  supervisor-uniqueness, cloud add/rm through the swarm-cloud CLI IPC (invariant: the desktop
  never upserts CLOUD devices directly — mutate via CLI then re-read); NODE_PROVIDERS in the
  section is already the provider-dropdown option shape; the TS SwarmDeviceRow mirror lacks the
  Rust engine field ("mlx-sidecar") — must be added; provider credentials have a complete ACP
  surface (save/auth/delete + metadata-driven forms); cloud-vs-local is the name-fragment policy
  (no metadata field). Provider reassignment on an existing node = remove+recreate under the hood.

## 2026-08-31 late afternoon — HF browser round two backend (c19c28a7f, first mlx-backend delegation)

Owner's hardcoded-filters call-out proven correct beyond the complaint: the crawl found **80
archs** where the list had 37. Measured facts now on record:
- The filter vocab is CRAWLED, not hardcoded: HF's models-tags-by-type has NO arch bucket and only
  2 quants (measured, refused); a 15-request bounded crawl (10 pages by downloads + 5 by newest,
  named constants) yields 80 archs (config.model_type doubles as a tag on 100% of 708 carrying
  one — every vocab entry is server-side filterable), 7 bit-widths + 7 precision families, 175
  authors; 1h TTL cache, stale serves carry refreshError explicitly.
- Browse rows carry sizeBytesEstimate from safetensors dtype counts — 0.003% off the true sum on
  3 verified repos; exact list-mode bytes DON'T exist (usedStorage → 400 with proof; siblings
  sizes null); the N+1 per-row pattern was refused per the pagination mandate.
- HF resolve URLs honor Range through BOTH redirect classes (non-LFS 307→206, LFS 302→CDN 206
  with Content-Range total == tree size); resume is append-proven byte-for-byte by live test;
  pause keeps .part; CANCEL NOW DELETES the partial repo dir (the owner's stuck-row complaint).
- Completeness follows model.safetensors.index.json shards + config.json; his fixture dirs were
  pre-first-shard cancels (never the masquerade case — that needed one finished shard, now pinned).
- modelsList carries diskAvailableBytes/diskTotalBytes (statvfs, df-verified to the KiB).
- Wire additions: browseFilters, modelCard (README capped 1 MiB with truncation flag),
  downloadPause, downloadResume (works on untracked disk residue), progress state "paused" +
  restartedFiles. Per-endpoint request budgets stated in the report.
UI round two in flight (type-ahead comboboxes, fullscreen model card, lifecycle buttons, disk bar,
narrow-width reactivity).

## 2026-08-31 afternoon — Mihai's first hands-on: three UI corrections, all shipped same-day

Backend for the round (6d4b5d7bc): per-model sampling profiles with live-config migration proven
against the verbatim config (presence 1.2 lands in the mounted model's profile and its argv);
paginated HF browse with ALL four filters server-side-honest (filter=AND proven with a negative
control; quant/arch match tags — name-only quants are honestly under-included, never wrong pages);
cursor guarded against token exfiltration. Rapid-MLX has no serve-time context-length flag, so
context_limit stays goose-side bookkeeping (matches the night's earlier finding).

He used the window himself; his confusions are the spec. (1) Sampling was buried below the fold on
the Engine tab and absent from the Models tab where he looked → Sampling is now a third top-level
tab with a per-model shortcut from every model row (9b826b4cb, live-verified 18 checks). (2) The
chat window's multi-provider model selector coexisted with the MLX window and read as "all these
providers" → chat now rides the MLX engine alone when the mlxEngine capability is present: the
selector UI is replaced by a state chip showing the SERVED model (click → MLX window), session
provider/model auto-syncs to the running engine, and the legacy selector stays in code behind the
capability gate (in flight). Backend: status now carries served_model_id (d906d90a8) because the
served alias, not the HF dir id, is what chat requests must name. (3) The mount card always
offered green "Mount" even with a model mounted → button matrix now tells the truth: Mounted
(disabled, green) / Switch model / Mount / mounting-spinner, picker follows the mounted model
without overriding explicit user picks (in flight, same commit as 2).

## 2026-08-31 ~03:00 — SWARM E2E THROUGH THE SIDECAR: PROVEN (artifacts + event log)

Rerun on the fixed binary (3d31c31e0) with the repaired single-key config,
GOOSE_SWARM_PIN_DEVICE=workhorse-mlx: the swarm's own pre-warm MOUNTED the engine (uvx fork pin,
8090 listener observed), config loaded clean (no config_parse_error), planner-keep guard held the
alias, pool was exactly [workhorse-mlx], every dispatch and judge look ran with provider omlx —
and the swarm WROTE WORKING CODE through the sidecar: src/slugify.py + src/test_slugify.py in the
run workspace, 4/4 tests passing under an independent pytest. Phases reached:
open→synthesis→review→execute→judge (look 7 on integrate-verify).

Caveat, stated plainly: the final COMPLETE-phase banner was lost — an external kill-all stopped
the session's background tasks at seq ~165 (run + engine tree died mid-judging; not a divergence,
not a crash; the log's last writes are 12s before the check). The substance is proven by the
artifacts + log; a clean-banner rerun is one command:
`GOOSE_SWARM_PIN_DEVICE=workhorse-mlx goose swarm run "<task>"`.

**Config note for Mihai:** ~/.config/goose/config.yaml on the workhorse now carries a `swarm:`
block (sidecar device workhorse-mlx + planner_model = the 9B alias + allow_model_load) and the
repaired `mlx_engine:` block. Any swarm run launched FROM THIS MACHINE will keep that planner
(the keep-guard sees the device carrying it). The MacBook's fleet config is untouched. Backups of
every config state are in the session scratchpad.

## 2026-08-31 ~02:30 — First sidecar-only swarm micro-run DIVERGED; stopped, different fix dispatched

Per follow-the-test-kill-on-divergence: the run was not retried. Observed (stdout + ground truth):
the surgeon's viability trace rested on a premise I failed to flag — `lms ps` WORKS here (CLI IPC,
tokenless), so fleet discovery imported the three 27B devices; every task went to them and 401'd
instantly (0.0s "claimed done but never wrote"); the engine correctly finished STILL RED and
refused success. The sidecar device: never pre-warmed (no mount, no listener on 8090), never
dispatched. Poisoned metric spotted: instant-failing devices ranked "fastest" (15ms = the speed of
a 401). Also suspicious: integrate-verify showed ✓ at 0.0s while carrying the auth error in its
report body. All four findings handed to the swarm-surgeon with the run's event log for a
diagnose-and-fix pass (pre-warm mount ordering is the step-C defect; speed-ranking and ✓-on-401
are its call to fix-or-report under mild-not-deterministic).

## 2026-08-31 ~02:00 — Swarm wiring complete (step C); micro-run's LM-planner leg blocked on fleet token

Step C (a2aec6fd4, swarm-surgeon): SidecarEngine registered in the Engines registry behind the
"mlx-sidecar" device tag; dispatcher routes sidecar models to the omlx provider (map the brief
presumed, surgeon built minimally); sidecar devices merge additively from config (never discovered,
never condemned by a not-yet-mounted engine's None probe); per-engine re-warm; OMLX_HOST exported
at registry construction; TRACE VERDICT YES walked in both directions; 704 tests + 8 gates +
workspace clippy green; ratchet held at 45,096. Planner remains lmstudio-pinned (pre-warm +
servability) by explicit choice.

**Blocker found for the full mixed-pool micro-run:** the fleet's LM Studio HTTP API (:1234) now
requires Bearer auth; no token exists on this machine (`lms ps` works — CLI IPC — but the openai
provider path 401s; the earlier probe that looked like a token was `logIncomingTokens: false`).
The token lives with the MacBook setup. Morning item for Mihai: provide LM_API_TOKEN (or run the
mixed micro-run from the MacBook). Sidecar-only run viability question put to the surgeon.

## 2026-08-31 ~01:00 — LIVE DESKTOP VERIFICATION COMPLETE (the whole loop, in the running app)

Driven over CDP (ENABLE_PLAYWRIGHT + playwright connect_over_cdp; screenshots in the session
scratchpad). Verified END TO END in the RUNNING app, not by tests: nav shows MLX Engine
(capability-gated) → view live (status card polls real ACP) → model picker lists the 9B from
~/.goose/models → Mount → MOUNTING → **RUNNING** (solid green; hermes chip; context 262,144; pid;
19.1 GB free bar) → presence penalty 1.2 → Save → exact "Restart required" banner → Remount →
running again, banner gone, field persisted, **engine process cmdline carries
--default-presence-penalty 1.2 (ps-proven)** → Models tab: models_dir row (~/.goose/models, Edit)
→ HF search returns real MLX repos → Download (Irfanuruchi/SmolLM2-135M-Instruct-MLX-4bit) → live
progress bar → DONE chip at 76 MB/76 MB, listed with size → Delete via custom dialog → gone from
list AND disk → Unmount → STOPPED, 8090 freed, zero orphan processes → Settings: ZERO "Local
Inference" occurrences (old window fully removed). Fleet gate ALLOW before/after everything.

**Three defects the live run caught that no test had** (all fixed + pinned by new tests/commits):
1. goosed's PATH grab-bag broke the spawn TWICE — first the mcp-hermit shim (cold python
   bootstrap ate the 180s budget), then the desktop-bundled ui/desktop/src/bin/uvx wrapper
   (nested bash chain, never served). Fix: the engine subprocess gets a FIXED standard PATH;
   missing uvx fails loudly by name. (2a249bafc + follow-up)
2. The gate's ALLOW message rendered under a red "Mount blocked" banner — status now carries
   gate_verdict; block=red, warn=amber, allow=no banner; 3 UI tests pin it.
3. First-run friction the tests can't see: onboarding guard (needs GOOSE_PROVIDER — set to omlx
   with GOOSE_MODEL qwen3.5-9b-4bit in config.yaml, backed up first) and the telemetry consent
   modal (declined). Both are one-time.

Instrument note (honesty): my driver wrongly expected the progress row to vanish on completion —
the UI keeps it with a DONE chip, which is better; driver assumption, not a defect.

Residual polish item: deleting a model leaves an empty publisher dir shell in models_dir.

Append-only, newest first, READ BACK at the start of every session. Every entry: what happened,
what it changed, what it means next time. Machine twin: `experiments.jsonl` (one row per real
experiment/event; a failed or inconclusive row carries a `void_reason` string, never a bare boolean).

`experiments.jsonl` row schema:
`{"ts", "experiment", "engine", "config": {...}, "result": {...}, "verdict": "pass|fail|inconclusive", "void_reason": "<only when not pass>", "commit"}`

---

## 2026-08-30/31 night — BAKE-OFF VERDICT: Rapid-MLX (evidence in experiments.jsonl, 8 scored runs)

**Protocol:** identical instrument (bench/swarm_bench.py) per engine, serial and hermetic, one
engine at a time on ports 8090/8091, memory-gated, fleet snapshot ALLOW before/after every run.
Model: freshly downloaded mlx-community/Qwen3.5-9B-MLX-4bit served from ~/.goose/models (both
engines took the arbitrary models_dir directly — no fork patch needed). rapid-mlx 0.13.1 (brew
core), omlx 0.6.4 (their tap).

**Decided it:** sustained-load stability at N=8, the swarm's exact profile. Successive N=8 runs,
same night, same host: rapid TTFT mean 8.0 → 7.6 → 7.3 s (improving); omlx 7.7 → 13.1 → 11.7 s
with p95 hitting 20 s and aggregate falling 49 → 30/33 tps — degradation with session age that did
NOT recover (probe cold-TTFT stayed 2.5 s vs 1.15 s fresh). The third omlx run was run specifically
to test whether run 2's decay was transient; it reproduced.

**Also for rapid:** working hybrid-aware prefix cache (hit TTFT −26% with fidelity held; omlx's
cache showed zero hit benefit on the DeltaNet hybrid — safe but inert); RSS 4.4 vs 6.2–6.5 GB;
per-model auto-config (hermes parser + qwen3 reasoning for dense qwen3.5, with documented dense-vs-
MoE hybrid distinctions); `--watchdog-ppid` (parent-death watchdog made for sidecar supervision);
presence/frequency penalty already upstream; goose precedent (author-tested via Ollama provider).

**Ties:** tool fidelity 1.0 with ZERO errors and ZERO malformed calls for BOTH engines across all
runs — the June hybrid footgun is dead in both current versions (rapid hit-fidelity 1.0, omlx
hit-fidelity 1.0). The 9B is emphatically NOT tool-inept; Mihai's fallback-model permission unused.

**Fairness notes (both directions):** omlx defaults thinking ON (every tool call pays a reasoning
preamble; 2x wall) — corrected via `chat_template_kwargs: {enable_thinking: false}` and RE-scored
before the verdict (its N=1 then beat rapid: TTFT 1.22 s); the degradation verdict rests on
no-think runs only. rapid ran with `--enable-prefix-cache` as omlx ran with its SSD cache dir —
headline features on for both. Instrument defects found and fixed mid-bake (decode_tps
divide-by-near-zero → req_tps; RSS sampler silent-zero → loud failure); the affected first-row
metrics are voided in-ledger, never edited.

**Next:** fork raullenchai/Rapid-MLX under leanzero; oMLX card filled in TRUMP-CARDS.md.

## 2026-08-31 — Fork live, pinned launch proven, goose↔engine E2E proven with zero code changes

**Fork:** github.com/leanzero-srl/Rapid-MLX created via API (keychain token auths as leanzero-srl),
cloned to ~/Projects/Rapid-MLX with `upstream` → raullenchai/Rapid-MLX (tags fetched; v0.13.1
present = the brew version we benched). Update runbook: `git fetch upstream && git merge
upstream/main && git tag leanzero-vX.Y.Z && git push origin main --tags`, then bump the pin in
goose. **Pinned launch proven:** `uvx --from git+https://github.com/leanzero-srl/Rapid-MLX@v0.13.1
rapid-mlx --version` → 0.13.1 (resolves, builds, cached).

**E2E:** release goose (Aug 25 build), `--provider omlx --model qwen3.5-9b-4bit`,
OMLX_HOST→127.0.0.1:8090 (rapid-mlx): agent called `write` (hello.py verified on disk) then
`shell` (python3 import+run), reported exact output "Hello, goose". The in-tree omlx declarative
provider IS wire-compatible with rapid-mlx — provider layer needs zero new code for basic agent use.

**Correction to the 2026-08-30 research-verdict entry:** upstream Rapid-MLX issue/PR numbers run
past #2770, so the research doc's "#2330/#2360" citations were plausible after all — my "garbled
citations" judgment was itself too broad. The failure-mode CONTENT still tested false on current
versions (no cache-degradation on rapid in 3 runs; the decay showed on omlx instead).

**Sidecar crate:** crates/goose-sidecar green — 4 real-child lifecycle tests + 6 memory-gate tests,
clippy clean. GGUF-vs-MLX finding: goose-local-inference's download machinery is single-file
GGUF-shaped; MLX needs repo-snapshot downloads → built fresh in goose-sidecar::hf (in flight).

## 2026-08-31 — Backend + boundary landed by two agents; penalties proven to bite

**ACP backend (c7be09fd0):** goose-sidecar grew hf.rs (MLX HF search — measured `filter=mlx` is the
working filter, `library=mlx` returns unfiltered transformers repos — repo-tree pagination, snapshot
downloads with .part/rename/resume/cancel and exact byte accounting proven by a LIVE 21 MB download
test) and engine.rs (MlxEngineManager state machine: stopped/mounting/running/failed, gate-Block
refuses before any load, restart_required = persisted-vs-mounted argv diff). 11 ACP methods under
_goose/unstable/mlxEngine/* registered + capability advertised; settings persist under config key
`mlx_engine`; all clippy-clean. Not yet exercised: a full real mount through the ACP handlers —
that is the running-desktop-app verification, still owed.

**SwarmEngine boundary step A (9812ac069, swarm-surgeon):** trait + LmStudioEngine, seven functions
verbatim, 404 swarm tests + 8 development gates + workspace clippy green, size ratchet tightened.
Surgeon's finds: the /api/v0/models prober never returned loaded_context_length (plan overstated
the parity gap), and swarm_engine.rs sat outside the development-gates file glob → step B item.

**Sampling penalties (live A/B on rapid-mlx 0.13.1, temp 0):** frequency_penalty=2.0 collapses a
repeat-blue probe from 30 repetitions to 4 with early stop — the per-request OpenAI-params →
sampler path IS plumbed. presence_penalty=1.9 did not flip the greedy argmax on the same probe
(flat penalty < the dominant token's logit margin — expected math, same plumbing). UI ships both
knobs; the settings path additionally uses the server-side --default-* flags at mount.

## 2026-08-30 — Campaign opened; finalists chosen from verified research; hybrid footgun inherited

**Did:** Verified Mihai's engine research against the live web (all three engines real; citations
garbled; framing correct). Dropped MTPLX (MTP-quant lock-in vs our model reuse) and mlx-serve (Zig,
new-model lag) → TRUMP-CARDS.md. Finalists Rapid-MLX vs oMLX, to be decided by a swarm-shaped
bake-off (1/4/8 concurrent tool-calling streams, qwen3.5-9b-4bit downloaded fresh, memory-gated).
Two Explore agents mapped the repo (provider layer, swarm coupling surface, subprocess idioms); one
adversarial Plan agent validated the integration design and killed two wrong turns (per-device
endpoint assumption — doesn't exist; cloud-device path — strips local extras).

**Learned, and it changed the design:** `local-edition/docs/EXPERIMENTS.md` (2026-06-25 spike)
records that raw mlx-lm/omlx prefix-cache HITs silently broke tool-calling on Gated-DeltaNet hybrids
(omlx #825, mlx-lm #980) and that this was THE reason LM Studio won last time. Consequence: the
bake-off scores hybrid-prefix-cache-vs-tool-calling as a first-class dimension — it directly attacks
oMLX's headline strength and must be re-verified on current engine versions, not assumed fixed.

**Gates stood up the same day:** `gates.py` (memory-mount, port-safety, fleet-untouched) with
BLOCK+ALLOW self-tests in `gates_selftest.py`, on the acting path before any mount.

MESH WIRE SHAPE (control service + UI bind to this; 8048946c2):
MeshStatus{self_ip?, self_hostname?, backend_state: NoState|InUseOtherUser|NeedsLogin|NeedsMachineAuth|Stopped|Starting|Running|Other(s), online, peers[]}; MeshPeer{hostname, ip?, online, last_seen?(RFC3339, None while connected)}. Daemon: brew tailscaled 1.98.5, --tun=userspace-networking --statedir ~/.leanzero/tailscale --socket ~/.leanzero/tailscale/tailscaled.sock --no-logs-no-support; CLI --socket is GLOBAL before subcommand; up takes --auth-key --hostname --accept-routes=false --login-server --timeout --advertise-tags --reset.
