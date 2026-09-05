# NOW — MLX in-house engine campaign (branch goose/mlx-inferencing)

## 2026-09-05 — THE NAME IS GOOSE SWARM · RAPID-MLX DRAWN TO UPSTREAM HEAD · BRANCH → MAIN
- Owner: the product is officially **Goose Swarm**. The 2026-09-02 Flock rename (92e2aa4c5, 91f2d6315) is reversed on every
  rendered label, the provider display name ("Goose Swarm" in the model selector), the sidebar wordmark, the hub title, the CLI
  verb (`goose swarm`, no alias) and all 16 i18n catalogs. Internal ids never moved, so nothing in config/runs/IPC changed.
  Proof: typecheck clean, i18n:check green (1643 messages × 15 locales), 204 files / 1728 tests green, dev gates 8/8.
- Rapid-MLX: fork main fast-forwarded to raullenchai/Rapid-MLX main (0 fork-only commits; 89 upstream commits past our
  v0.13.1 pin, version 0.13.4), pushed with tags; pin tag **v0.13.4-lz.1** on the head. Consumed surface re-verified at the
  new head: `/v1/status` carries num_running + num_waiting (routes/health.py:304), `serve` flags --port/--served-model-name/
  --enable-prefix-cache/--max-concurrent-requests/--resident-model-idle-ttl present. Live mount through the manager on the new
  pin: running after 13.3 s, context_window 262144, parser hermes, active_requests 0, unmount clean.
- An engine upgrade now REACHES existing installs: `EngineSettings::migrate_launcher` (engine.rs) — a persisted spawn_command
  equal to a SUPERSEDED default follows the shipped launcher on load (tracing warn with from → to, persisted once; an
  owner-edited launcher never matches). Before it every existing config, this machine's included, ran @v0.13.1 forever.
- Handoff queue item landed: ipcMain `restart-app` quits through `will-quit` (lease cleanup → mesh daemon + engine teardown)
  and relaunches only when the quit really happens; a refused restart (live-run dialog) arms nothing.
- Hygiene: ui/desktop/src/bin/tailscale{,d} (37 MB) and ui/desktop/dist (143 files) were TRACKED despite the 09-02
  .gitignore — untracked now (`just fetch-tailscale` / the build regenerate them).
- Found in the tree, not branch work: an overlay of origin/local-edition's docs + Cargo.lock/Justfile/.gitignore (27 files,
  byte-identical to that branch's tip d8202bc89) — stashed as stash@{0}. origin/local-edition carries 825 commits (122 on the
  engine, incl. the r6h golden 0.4616) that are on neither this branch nor main; merging that line is a separate job.

## REVIEW + FIX CAMPAIGN (2026-09-02) — COMPLETE; EVERYTHING PROVEN LIVE ON THE PACKAGED APP (12:37 build)
FINAL LIVE CHECK 12:38-12:44 (works-prover, receipts quoted in the session scratchpad): theme System follows the OS via nativeTheme (IPC theme-set → {dark:true}, class local-edition dark; Light/Dark/System all correct); sidebar = full-width Theme row above an unclipped Settings row; Link Connect → Connected 100.64.0.3, BOTH log lines now in the serve log ('leanzero-link tailscaled ready' pid 52459; 'control service listening on loopback' 127.0.0.1:41226), Headscale node 9 worksmacstudio-lan-6a972f online; MLX Mount → running pid 52701 :8090 (no stray); *** LEAK FIX PROVEN: AppleScript quit → serve log 'stop requested … SIGTERM' → teardown mesh ('1 mesh daemon(s) stopped per-pid … identity and state dir kept') → teardown engine ('SIGTERM, grace, proven group kill, port released') → exit 143; tailscaled/uv/python GONE, socket gone, :8090 free, identity.json kept 0600; relaunch → Connect + Mount with ZERO refusals. Personal Tailscale untouched (3 reads). Click-to-expand: code in the asar (Clipped.tsx/RevealDialog), on-screen proof needs a live run (next benchmark). App LEFT OPEN, CONNECTED (100.64.0.3) and MOUNTED (pid 53086).
USER-REPORTED FIXES (Mihai, post-remake): Memories/Skills selection invisible (bg-background-accent never existed) → 46ebc237e; theme switch System|Light|Dark default System → 7fdfd5806 + df61a1a20 (nativeTheme); cut-off Settings label → 7f90b01fb + DESIGN.md rule 'a button's own label never truncates' (0b74e2c34); click-to-expand for every clipped prompt/brief/row in the run panel → ebd84d9dc..ef2d79e63 (Clipped + RevealDialog, 24 tests); 'Building' in sessions = the live-pass fixture (removed). Mihai: 'the design is very nice, I love it… reminds me of my website'.
REMAINING QUEUE (small, no on-screen change): ipcMain 'restart-app' → app.exit(0) skips will-quit → an IN-APP RESTART still leaks daemons (route through the lease cleanup); LMSTUDIO_API_KEY set nowhere on the workhorse (its LM Studio needs a token) — set it or add a main-side secret reader; fleet-card offline text lacks the reason; deriveFleet laneless digest keys a sidecar digest to the LM row; studio.tsx → lz promotion; FLEET/nodes/peers → DataTable; meshPollFailures/lastPollError DTO; AgentManager registration ticket; admission-cap engine lever; goosed-agent GoosedRemoteExecutor writes no provider/extensions on fresh sessions; MacBook tick.py rows for the 5 new events; two benign zod eval CSP violations; prettier pass on the pre-existing unformatted files (never main.ts wholesale).
PROVEN 2026-09-02 ~10:15 (live pass, packaged app, receipts quoted in the session scratchpad): Mihai signed in with the emailed code (worker's FIRST auth_verified + node_secret_minted + headscale_user_created + headscale_join_key_minted), ~/.leanzero/identity.json 0600 + node-id 6a972f, Headscale ONE user acct-d0afafb47308cf6e + ONE node worksmacstudio-lan-6a972f online 100.64.0.2 ephemeral, status Connected w/ remoteExecutionWired+mlxControlWired=true, remoteExecutionAllowed=false, meshBinaries found at Resources/bin, /nodes lists self, self-targeted remoteExecute → accepted + Busy→Idle (never 501; the 403 gate lives on the PEER route — unreachable with 0 peers by design), mlx proxy round-trips, personal Tailscale byte-identical. Visuals: 20 surfaces light+dark = ONE accent hue each, no rail/tint/native select (screenshots-passF). Two literal-receipt misses fixed/explained: the leanzero_link INFO log lines were filtered → c91da892a adds leanzero_link=info; the 403 needs a linked peer. Dead-as-configured branches (not defects): LM Studio fleet card (SHOW_LMSTUDIO_PROVIDER=false + toggle off; its offline text lacks the reason → follow-up), swarm-provider context limit + recipe wizard (active_provider omlx). Mihai's post-remake reports fixed: Memories/Skills selection was WHITE-ON-WHITE (class `bg-background-accent` never existed) → 46ebc237e; sidebar theme switch System|Light|Dark default System → 7fdfd5806; 'Building' in every goose session = the live-pass FIXTURE (removed). Mihai: 'the design is very nice, I love it… reminds me of my website… polish it a bit'.
REMAINING QUEUE (no on-screen change / decisions): LMSTUDIO_API_KEY is set NOWHERE on the workhorse while its LM Studio requires a token (every LM call 401s; MLX-only config unaffected) — set it or add a main-side secret reader (renderer secrets are masked); deriveFleet laneless digest keys a sidecar digest to the LM row; promote studio.tsx controls into lz/; FLEET/nodes/peers → DataTable; meshPollFailures/lastPollError DTO; AgentManager registration ticket; admission-cap engine lever; fleet-card offline reason; MacBook-side tick.py rows for the 5 new events; the goosed-agent GoosedRemoteExecutor writes no provider/extension data on fresh sessions. Node adoption after an app restart: an orphaned engine reads stopped+stray → Unmount then Mount (S-H3 refuse design).
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
(efafa4700..785defe08). ALSO LANDED since: MLX engine/models remake (3713e606c..3314a86d5, StudioSelect choosers, cloud.tsx), swarm run panel mission control (d0acc8581..95ee1b8fb incl. the two behavior edits 4117853ca: useFleetCorroboration swap + sourceMissing band), confirm-on-close dialog (a80209a40..805d6a00d: mouse close/quit on a live run now asks; IPC confirm-close-run/-reply), bench-scorer charter fix 5a791375c (tick.py lives ONLY on the MacBook — MacBook-side item: print the 5 new events). ALSO LANDED: leanzero-link crate fixes (14 commits b7f1c3425..5938a2bf7; tests 79→114; nodeSecret token, allow default OFF gates /execute+/mlx, DaemonExited loud, listener-pid readiness proof, Origin→403, key via file:, local_sessions→Result/503), chrome leftovers (c5743a61c..bb767a6e6: app-wide solid disabled state in ui/button|tabs|input, Button iconOnly+destructive+forwardRef), leftovers-2 (21614dfa8..f7ed93434). VERIFIED: worker works-prover WORKS on all 7 claims live (no live /verify ever succeeded yet → first real desktop sign-in after the rebuild is the remaining traversal). TRACERS: sidecar 3/3 concur (e09790ad0 breaker trips at the 4TH death), swarm 2 concur w/ corrections (S-H2 = 2→8 slots; S-H1 premise: LM Studio /v1/models answers 401 to the unauthenticated probe → LM_API_TOKEN in the probe is wave-2 item 1), desktop 4 concur + ***U-M3 (949d3fa6e) REFUTED — REGRESSION: index.html static CSP meta intersects; live localhost:1234 config went OFFLINE (cspSafe rewrite deleted) → correction agent running (restore normalization; LAN via main-process probe); U-M8 scorer token fiction on sb-7 (graded-sb7-db) → same agent***, link layer 6 concur + 2 partial (409 only in composition w/ opt-in; AgentBusyGuard lacks disarm → successor-token clobber race → fix agent running). *** SESSION LIMIT HIT ~03:00 2026-09-02 (resets 06:50 local): TEN agents cut mid-step — C2 goose-serve injection, Q1 MLX busy signal, wave-2 swarm pass (mid item 3), swarm event-log timestamps/sub-component polish, lz-extensions (Segmented/DataTable slots + adoption), U-M3 CSP-regression fix (main.ts refs removed, tests pending), AgentBusyGuard race fix (edited, gates pending), leftovers-3 (a heredoc half-write suspected — verify its file), crate key-file guard + wedged-daemon demotion, roster grading. RESUME = one SendMessage per agent id (ids + per-agent stop points in the session scratchpad review-findings-index.md, section 'SESSION LIMIT HIT'); a one-shot cron at 06:53 fires the resume. Then: tracers on the new commits, lz promotion + table adoption, ONE i18n extract, `just release-binary`, `pnpm run package`, screenshots-passF, the real Link connect w/ receipts. LANDED after the cut: C2 goose-serve injection DONE (59074c710..04abe8a9a — /execute answers 202 under goose serve; link_serve.rs); U-M3 CSP correction (987889548..62d12d110 — probes + wizard chat moved into MAIN over IPC fleet-probe/fleet-chat; packaged renderer is file:// with a static meta CSP); race fix 70f7718c7; Q1 MLX busy signal (527eb821a..ed409244f, /v1/status num_running+num_waiting; --max-concurrent-requests 8 is a HARD 503 admission cap → swarm transient classification sent to the wave-2 swarm agent); crate follow-ups (16730333a key-file guard, 4a0e912a8 wedged-daemon look-count=5); swarm timestamps+polish (8156093d7 + 18 commits, residue 0); leftovers-3 (caaac7221..26832ab02). ALSO LANDED: lz-extensions + adoption (4d901286a..0787de9fc: Segmented slots/modes, DataTable rowProps/rowTestId/renderSubRow, benchmark pickers + settings tabs on the primitive, syntax palette, toast icons), tiny ban fix (b8072447d, 594cca9d2), i18n extract (541652f43) + 27 ids × 15 locales (9291aa9c2) → i18n:check FULLY GREEN, DESIGN.md font-weight wording (0f490e33d). HAZARD CAUGHT+REVERTED: `cargo remove hmac` gc'd the ROOT Cargo.toml (opentelemetry-http + the load-bearing icu pins) uncommitted — restored; memory updated. Warm-up `cargo build --release -p goose-cli` done 09:13 (2m25s). STILL RUNNING: wave-2 swarm pass (token probe etc.) — the ONLY gate before `just release-binary` → `pnpm run package` → the live pass (brief: scratchpad/live-pass-brief.md; the Link sign-in needs Mihai to type the emailed code — hand-off step). QUEUED follow-ups (no on-screen change / consumption): promote studio.tsx controls into lz/; FLEET/nodes/peers → DataTable; freeBusy: exclude mlxNodes from span attribution (tracer, medium); DTO meshPollFailures/lastPollError; admission-cap engine lever; AgentManager registration-ticket refactor; roster grading (one pass at the end). Earlier text: a tracer over the 5 newest commits. HELD to the end: roster grading; admission-cap engine lever. THEN: lz promotion + table adoption → ONE i18n extract + prettier → just release-binary → pnpm run package → screenshots-passF (light+dark; hub tabs role=radio) + the LIVE-PASS checklist (fleet ONLINE on localhost:1234, no renderer request to :1234, real context limit, wizard reply) → the REAL Link connect with receipts. IN FLIGHT (pre-cut, now done): C2 goose-serve injection (+2 link.rs compile one-liners for the crate's trait change) (+ local_sessions→Result trait change), C2 goose-serve injection
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

## 2026-09-02 — the product is GOOSE FLOCK (rename, 92e2aa4c5 + follow-up)

Community feedback: geese come in a FLOCK, not a swarm. Every string a person READS now says
Flock; every identifier a machine reads is unchanged, deliberately.

  DISPLAY (renamed)  window title / tray "Goose Flock"; sidebar brand "LeanZero Flock"; nav row
                     "Goose Flock" carrying icons/Goose (NavItem.icon widened from LucideIcon to
                     React.ComponentType<{className?}>); hub heading "LeanZero Flock" with the
                     "Flock Settings" tab; provider display name "LeanZero Flock" in the model
                     selector; all 16 i18n catalogs; README prose; `goose flock` CLI.
  IDS (unchanged)    `swarm` config key, `.swarm/` run dir, `swarm` provider id, GOOSE_SWARM_* env,
                     IPC channels, crate/file/module/route names, i18n message KEYS, test ids, and
                     every model-facing prompt string (golden-formula surface).
  COMPAT             `goose swarm` is a clap visible_alias of `goose flock` — bench_dispatch,
                     run_build, the skills, main.ts's swarm-cloud IPC and the swarm provider's own
                     `cmd.arg("swarm")` all keep working untouched.
  PROVEN LIVE        packaged build relaunched on CDP 9897: title "Goose Flock", brand "LeanZero
                     Flock", nav [Skills, Memories, Benchmark, Goose Flock] with the goose glyph
                     (clipPath clip0_2096_5193), hub h1 "LeanZero Flock", tabs incl. "Flock
                     Settings", and ZERO occurrences of /swarm/i in the rendered text of either
                     screen. `goose flock --help` and `goose swarm --help` resolve to the same
                     command. 1728/1728 desktop tests, clippy clean, development_gates 8/8.
  IF IT NEEDS UNDOING  the display/id split is documented at the top of ui/desktop/src/branding.ts.

## 2026-09-02 — HANDOFF: continuing this on ANOTHER MACHINE

Everything below travels in git. Branch `goose/mlx-inferencing`, in sync with origin. The last six
commits are the Flock rename and four rounds of the brand mark; before them, the MLX/Link work.

### Get to a working checkout

```bash
git clone git@github.com:leanzero-srl/goose-local-edition.git goose
cd goose && git checkout goose/mlx-inferencing
source bin/activate-hermit          # toolchain: node 24, pnpm 10.30, cargo, just, protoc, python 3.10, uv
cargo build -p goose-cli            # ~3 min cold; proves the Rust side
cd ui/desktop && pnpm install && pnpm run typecheck && pnpm test    # expect 204 files / 1728 tests green
```

If `pnpm` is only available through corepack, scripts that shell out to a bare `pnpm` (like
`pnpm run package`) need a shim first: `printf '#!/bin/sh\nexec corepack pnpm@10.30.0 "$@"\n' >
<dir>/pnpm && chmod +x <dir>/pnpm` with `<dir>` first on PATH.

### The verify loop (compiling is NOT evidence — look at the running app)

```bash
just fetch-tailscale                # 35 MB of mesh binaries, gitignored, per-machine
cd ui/desktop && pnpm run package
open -n out/Goose-darwin-arm64/Goose.app --args --remote-debugging-port=9897
node scripts/cdp-probe.mjs --eval "document.title"
node scripts/cdp-probe.mjs --shot /tmp/nav.png --clip 0,40,250,130 --scale 5
```

`scripts/cdp-probe.mjs` is the tool for that rule; it takes `--eval`, `--shot`, `--clip`, `--scale`.
Quit the app with `osascript -e 'tell application "Goose" to quit'` — that path runs the lease
cleanup and the supervised teardown of tailscaled + the MLX engine. The in-app restart does not.

### Changing the brand mark

`ui/desktop/src/components/icons/leanzeroMark.tsx` is the ONLY geometry. After editing it run
`node ui/desktop/scripts/build-brand-icons.mjs` — it regenerates icon.icns, the PNGs, the Linux SVG
and both menu-bar templates FROM that file (needs Google Chrome as the rasteriser, macOS `sips` and
`iconutil`). Never hand-edit `src/images/icon.*`. The rules that four owner rounds paid for are in
`ui/desktop/DESIGN.md` › The brand mark — in particular: the goose is the upstream product's own
path, and the pair MERGES (no mask, no halo gap between them).

### What does NOT travel, and what to do about it

- **The LeanZero Link control plane is LAN-bound to this Mac Studio.** Self-hosted Headscale on
  127.0.0.1:8790 behind a Tailscale Funnel, launchd jobs `com.leanzero.headscale` and
  `com.leanzero.link-auth`, secrets in `~/.leanzero/` (api-key.txt, link-worker.env). None of it is
  in git and none of it should be. On another machine the app can still build and run; Link sign-in
  will talk to THIS machine's control plane over the funnel URL, so it keeps working as long as this
  Mac Studio is up. Moving the control plane is its own job, not a checkout step.
- **The model fleet.** LM Studio / the MLX engine and `~/.goose/models` are per-machine. Note
  `LMSTUDIO_API_KEY` is set nowhere on this workhorse, so its LM Studio answers 401 to every probe.
- **`~/.config/goose/config.yaml`** carries the `swarm` key (the node pool) — per-machine, not in
  git. Recreate with `goose swarm pool`.
- **The loop-state instruments** (`tick.py`, `bench_dispatch.mjs`, `first_tick_r1.sh`) live only on
  the MacBook at `~/goose-builds/loop-state/`; the sync is one-way MacBook -> workhorse.
- **Fetched/derived artifacts** are gitignored now, so `git status` is quiet on a fresh clone:
  `ui/desktop/src/bin/tailscale{,d}`, `/main.js`, `/ui/desktop/dist/`, the screenshots-pass* dirs.
- **Notarization** still needs Mihai's Apple Developer ID certificate in the login keychain;
  `just release-notarized <version>` is wired and waiting on it.

### Open queue (nothing here blocks the branch)

- ipcMain `restart-app` calls `app.exit(0)` and skips the lease cleanup, so an in-app restart still
  leaks tailscaled + the MLX engine. Quitting normally is safe. Route it through the same teardown.
- `LMSTUDIO_API_KEY` unset on the workhorse (above).
- Fleet-card offline text lacks the reason; deriveFleet keys a laneless sidecar digest to the LM row;
  `studio.tsx` primitives want promoting into `lz/`; FLEET/nodes/peers want the DataTable;
  meshPollFailures/lastPollError are not in the status DTO; two benign zod eval CSP violations.
- `icon.ico` (Windows) is still the old goose art — deliberate, Windows is not a target.
