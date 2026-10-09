# Forge 1.0 benchmark machinery — inventory for Forge 2.0 (read-only, 2026-10-09)

Scope: `evals/swarm-bench/forge/**` and the forge files under `evals/swarm-bench/bench/`, at HEAD 548f555a7.
Nothing in `~/Projects/goose` or `~/Projects/LeanZero-website` was edited.

CPU rule kept: no Playwright, scorer, pytest, cargo or npm build ran. The measurements below come from:
- one in-process `facts()` call for one seed (23 ms);
- small Python reads of the existing verdicts and observations (files of 12 MB or less) under
  `~/Library/Application Support/Goose/benchmark/runs/build/`;
- greps of the pinned kit cache and of the installed Forge CLI.

---

## 0. Measured numbers (the baseline every projection below scales from)

| quantity | value | source |
|---|---|---|
| scoring pack, 1 seed (scoring variant) | 349,206 bytes JSON: 229 issues (1,085 B/issue), 215 history entries, 38 live, 53 fields, 8 sprints; generated in 23 ms; node RSS 61 MB | `facts('0123456789abcdef',{scoring:true})` |
| scorer wall time per tree (3 scoring sites) | 351–436 s for working apps; 1,149 s and 1,256 s for two pathological apps; 17–24 s for empty or broken trees | `verdict.json` `scorer_seconds`, 22 forge runs |
| UI phase per site | 85–91 s (the graded recording, 21 segments), 460 s for one slow app | `bench-media/media-manifest.json` |
| proxied-call overhead (localhost HTTP + site handler + log) | 0.6–1.5 ms per call (rerun phase with no faults: 479 calls in 0.3 virtual s; 858 calls in 1.3 s) | `forge-observations.json` phase durations |
| `forge-observations.json` size | 1.8–11.9 MB typical; 57.6 MB worst (run 62d49af2); the bulk is `phases[*].calls[*].response` (full Jira bodies) and `ui` | same files |
| a strong app's call mix (Haiku 0.9661) | backfill 16 Jira + 579 KVS calls; live 72 invocations (41 trigger, 31 consumer deliveries), 91 Jira, 473 KVS | same |
| public input | 18,885 bytes (prompt + contract + starter) | `forge/public/*.md` |
| check registry | 66 rows: 63 tiered (L K T R S B U V A), 3 E; 7 criticals; 4 admission bands | `score_forge.py:2976` |
| scorer tests / kit tests | 117 tests in `test_score_forge.py` (1,773 lines); ~30 cases in `forge/kit/test` (1,860 lines) | grep |

---

## 1. Component inventory

How to read the last two columns:
- "Generic" is what a different Forge task, or a second product, could reuse with the same code.
- "Hard-wired" is code that only means something for Scope Ledger.

| component | LOC | what it does | generic vs hard-wired | known defects |
|---|---:|---|---|---|
| `kit/lib/runtime.cjs` + `runner.cjs` + `shim.cjs` | 163 + 57 + 53 | Bundles each handler file with esbuild the way `forge deploy` does, resolving only from the pristine kit modules. Runs every invocation in a fresh Node process: Atlassian's sha-pinned wrapper, under a deny-default `sandbox-exec` fence (bundle reads only, no exec, network only to the per-invocation proxy port). Shifts `Date` by the virtual-clock offset. Kills on timeout by pid. | ~100% generic | The timeout is a REAL-time `setTimeout(timeoutSec*1000)`. Only `Date` is shifted; timers are not virtualised, so a wait inside an invocation is real time (contract §8 says so). This is the load-sensitivity `forge_controls --quiet-load` works around. |
| `kit/lib/emulator.cjs` | 436 | The I2 API: `build`, `invoke`, `deliverProductEvent`/`deliverNext`, `drainQueues`, `runScheduled`, `invokeAction` (Rovo), `invokeResolver`. Queue push/stats/cancel. The documented async-event retry: `InvocationError.retryAfter` clamped to 1..900 s; a throw or timeout redelivered at 1, 2, 4, 8, then every 15 min inside 24 h. Module timeouts (25/55/55–900 s). | ~80% generic | Hard-wired: only `avi:jira:updated:issue` is ever emitted (`triggerEvent`, l.318–333); `contextAri` is a Jira site. Defect C. The queue drain is strictly sequential (l.262–316). NEW N1: stale trigger clock. NEW N10: `CYCLIC_INVOCATION_LIMIT = 1000` has no receipt. |
| `kit/lib/proxy.cjs` | 249 | The platform proxy the wrapper talks to. Routes `fpp/provider/{app,user,none}/remote/{jira,confluence,bitbucket,stargate}`, KVS, LLM, Realtime GraphQL, egress allowlist (502, no internet) and logs. Attributes every call to its invocation through a per-invocation token. Anything unmodelled answers 501 plus `harness_missing`. | ~75% generic | Product is `jira` only: Confluence and Bitbucket answer 404 "no product installed" (l.40–43). Stargate understands only queue publish/stats/cancel; `/graphql` (e.g. `webTrigger.getUrl`) goes to 501, then HELD. NEW N9: every full response body is kept in the log (`entry.response`). |
| `kit/lib/kvs.cjs` | 290 | KVS, secrets, custom entities with partition/range indexes, filters, cursors, batch (25 keys), transactions (≤ 25 ops, `check` + `conditions`), TTL, `FAIL_IF_EXISTS`, `returnValue`, documented size/depth/key limits. | ~95% generic | Defect E: it accepts multi-attribute `range`, applies range conditions to `range[0]` only, and sorts by all attributes. NEW N5: a transaction condition on a missing entity always answers 409 (l.231–239), so create-if-absent `NOT_EXISTS` idioms are refused; the codes are harness choices (header l.12–16). Every query scans all entities and sorts (O(E log E) per page). |
| `kit/lib/bridge-host.cjs` + `bridge-page.cjs` + `csp.cjs` + `tokens.cjs` | 374 + 182 + 21 + 38 | Custom UI host. Serves `resources[].path` under a random prefix with the CSP Atlassian's own `@forge/csp` computes, and answers `@forge/bridge` 7.1.0 ops: `getContext`, `invoke`, `fetchProduct`, theming from pinned `@atlaskit/tokens`, router, flags, modal, the dashboards widget edit API with the host's Save, and realtime subscribe. | ~85% generic: works for any module with a `resource` | Hard-wired: `contextFor` special-cases `dashboards:widget` only. Defect A: flags are drawn inside the app page (`bridge-page.cjs:39–44`). `fetchProduct` reaches Jira only. NEW N6: `getContext` carries no `permissions`. |
| `kit/bin/forge-dev.cjs` | 396 | The entrant's offline CLI: `invoke`, `events`, `scheduled`, `serve`, `llm`, `realtime`, `kvs`, `users`, `reset`. State lives in `.forge-dev/state.json` under a pid lock. | ~75% generic | `events` replays the site's Jira issue-update script; `--sprint` and `users`/comment-forbidden output are task-shaped. |
| `kit/bin/lint.cjs` | 101 | `npm run lint`. Runs `@forge/lint` 6.3.0 in `client-side` mode (16 linters) plus the stages of `@forge/manifest` 13.6.0 FullValidationProcessor, with the local OpenAPI copies (jira, jsw, jsm, conf, confv2, bb). | 100% generic | Defect E, root cause (section 3, N0): real `forge lint` also runs a SERVER-SIDE linter that this file can never run offline. |
| `site/site.cjs` | 330 | Mock Jira HTTP server: OpenAPI matching (outside the OpenAPI → 404; in it but unhandled → 501 + `harness_missing`, verdict held); OAuth2 scope check per operation; scripted faults matched by WHO calls (consumer-of-change, scheduled-run, resolver-read); per-issue write limit; control surface for the emulator and probe. | ~70% generic | Hard-wired: `WRITE_OPS = {comment POST}` and `UI_MODULE_TYPES = {jira:sprintAction, dashboards:widget}` (l.26–28). The fault scopes are Scope-Ledger flavoured. NEW N4: faults are not scoped to an invocation. |
| `site/state.cjs` | 111 | Mutable site state: issues, histories, comments, the virtual clock (`now = base + real elapsed + skipped`), and the live delivery plan. Live changes are applied in creation order whatever the delivery order. | ~50% | `applyItem` supports only Sprint, the estimate fields, status, labels, summary and priority; anything else throws. Sprints, boards, board config and permissions are static (`hiddenFrom` and `commentForbiddenFor` come from the pack). |
| `site/fixtures.cjs` | 661 | `facts(seed) → pack`: 2 projects, 3 boards (2 scrum + 1 kanban), 8 sprints with fixed roles, 229–245 issues, sprint-change pre-history, a 36–44-update live script with duplicates, permuted pairs and drops, faults, visibility, and the live-UI slot. Pure xorshift128+. | ~10% generic (the RNG, id-class discipline, delivery-plan concept) | NEW N3, which blocks scale: (1) `summary()` has 20 × 28 × 11 = 6,160 distinct summaries and loops forever once they run out; (2) issue ids (base 13,000–40,000, span 4N) collide with changelog ids (from 50,000) once N > ~2,500, and the oracle then refuses the pack; (3) O(N·E) loops (the estimate step's `issues.filter(… events.some …)`, `counted()`). |
| `site/jql.cjs` | 429 | JQL subset: tokenizer, parser, three-valued evaluator; `openSprints()` etc.; relative dates on the virtual clock. Valid-but-unmodelled JQL → 501. | ~95% generic (Jira) | `WAS`/`CHANGED` and many fields are not modelled (they go to 501, then HELD). |
| `site/rest/platform.cjs`, `agile.cjs`, `render.cjs` | 293 + 192 + 96 | Jira REST v3: field, user, project, issue, changelog (incl. bulkfetch), search/jql (token paging), approximate-count, comments (ADF-only), permissions/check, mypermissions. Agile/Software boards, sprints and issue lists in both paging styles. Response shapes as measured on Jira Cloud. | ~80% generic (Jira read side) | No issue create/edit/transition/delete, entity properties, links or workflows. 31 direct `c.state.pack.*` reads (22 in `agile.cjs`, 9 in `platform.cjs`), so the world cannot change mid-run. Every search page re-runs filter + sort over all issues. |
| `site/llm.cjs` | 156 | Forge LLM: `list()` returns the 8 models of the docs page; the documented `temperature`/`top_p` rules; a scripted responder (clean, digits, refusal, malformed, 500); streaming NDJSON with a split line. | Model list and validation generic; the script is task-specific (`report_scope`, change ids) | — |
| `site/realtime.cjs` | 171 | Realtime broker: channels, global vs context channels, signed tokens and claims, `replaySeconds`, publish rejected without a frontend context. | ~100% generic; `PRODUCT_CONTEXT` already includes `content`/`space` | Defect D lives in the contract, not here. |
| `site/openapi.cjs`, `limits.cjs`, `rng.cjs` | 64 + 48 + 66 | Operation lookup plus the OAuth2 scope rule from the shipped OpenAPI; page caps with receipts; the seeded RNG. | Generic | NEW N2: `openapi.cjs` loads only `jira.json` and `jsw.json`, and `alternatives()` keeps only `scheme === 'OAuth2'`. Confluence specs use `oAuthDefinitions`, so every Confluence call would be a 401. `score_forge.openapi_ops` has the same filter. |
| `bench/score_forge.py` | 3,773 | The registry with preconditions (vacuity G5), unavailable/absent semantics, composition `(0.88·inner + 0.12·gate·e_mean) × crit_mult`, severity transforms, ROOT_BLOCKS dedup, 4 admission bands (band 4 graded −0.03/defect), `BAND_PULL`, 3-site merge (worst per row, mean per E), calibration fitting and sha pin, severity selftest, reference gate, deploy-readiness rules (M1–M13, P1–P5, R1–R12), CLI refusals, `gather`. | About 50%: composition, bands, selftest, calibration, CLI, paging-walk grader, Retry-After verdict, scope evidence, PNG/contrast, most deploy rules. About 50% Scope Ledger: the 66 check bodies, band rosters, ROOT_BLOCKS, NUMBER_ROWS, ACTION_KEY, REPORT_TOOL, EXPLAIN_SCRIPT. | Defects B, E, F, G, H (section 2). C is also charged here by `k_consumer_shape` (the resolver form counts as the wrong shape). Rows scan all KVS rows per expected change (O(changes × rows) per phase). |
| `bench/forge_probe.mjs` | 990 | Drives I2 through the §8.7 sequence: lint ×2, build, backfill, live, heal, rerun, Rovo, then the UI in Playwright (widget per scrum board, edit/Save, light/dark, live step, sprint action per active sprint, explain ×5, resolver-500 step, not-started). Records video, screenshots and the contact sheet, and writes `forge-observations.json`. | ~55% generic (launch, normalisation, surface capture, DOM/token reads, recording); ~45% task script | Defect A (swallowed clicks, l.776/931). Defect B's scenario (double click on the same row 1.3 s after a post). `KIND` hard-codes the two UI module types (l.155–156). The UI phase is linear in scrum boards × active sprints. The site runs in-process (NEW N8). |
| `bench/forge_oracle.py` | 591 | Every expected number: membership at start and now, committed/added/removed/creep (Decimal half-up), visible and hidden changes per person, action JSON, leak terms, re-estimated sprints, the economy optimum (scoring-site paging aware). | ~5% generic | Cost is O(active sprints × issues × entries). `estimate_of` does `e in self.live` (list scans), which is infeasible at 10–50× (section 4.4). |
| `bench/forge_controls.py` | 263 | Mutant and idle controls: applies a patch to a copy of the golden, rebuilds, scores serially, and checks exactly the declared rows are lost (± ROOT_BLOCKS), the critical fires, and `max_final` holds. The empty starter and a one-function app must score ≤ 0.05. `--quiet-load` / `--yield-to` exist because real-time rows are load-sensitive. | ~90% generic (needs a new golden and mutants) | — |
| `bench/forge_kit.py` | 244 | Materialises the kit: `npm ci` from the committed lockfiles (app-modules, lint-modules), fetches the wrapper and loader by sha, copies the schema, writes `KIT.json`, flocks, `repin`. | 100% generic | — |
| `bench/forge_site.py` | 134 | The vendor interface for `run_build`: starts the dev site with a fresh `dev_seed`, writes the trace header, reaps by pid. | 100% generic | — |
| `bench/isolated_tiers.py` | 76 | The tier table (`FORGE10` = family forge, vendor `forge_site`, fenced network, kit, effort medium, own scoring site). | Generic: add a row | — |
| `run_build.py` forge paths | ~40 of 1,280 | Kit clone into the workdir, `FORGE_KIT` / `FORGE_SITE_URL`, input-manifest exclusion, stopping the dev site and handing over to `score_forge.gather`, `--forge`. | Generic plus one flag | — |
| `release_manifest.py` forge payload | ~25 of 140 | The forge payload closure. | Hard-codes `isolated_tiers.FORGE10` and `FORGE_TREES` | Needs a per-era generalisation. |
| `bench_isolation.py` fence/relay | ~150 of 448 | Localhost-only sandbox for the entrant plus a provider-only CONNECT relay with a two-sided preflight. | 100% generic | — |
| `forge-thresholds.json` | 44 | Calibrated rungs: economy fitted to the golden worst of 5, event top 1.46, contrast, chart px, blank share. Sha pinned in `CALIB_SHA256`. | Mechanism generic; values per golden | Defect F (the reconcile rung is a step). |
| `bench/golden-forge` | 958 src + 982 UI + 1,426 own dev bed + 161 manifest | Scope Ledger reference app at 1.000. It also carries WP3's second, independent mock (`dev/site.cjs`, `dev/runtime.cjs`). | Task-specific, but ideal as a brownfield v1 (axis 6) | — |
| `bench/golden-forge-alt` | 840 src + 821 UI | An independent second implementation (different endpoints, asApp + permissions/check, `stream()`, global channels). Proves coverage. | Task-specific | — |
| `forge/mutants` | 31 patches (745 lines) + 31 expect files + `build_mutants.py` 458 + `check_on_bed.py` 52 | One-defect mutants generated by anchor-asserting string edits. | Generator mechanism generic; mutants task-specific | — |
| `forge/public`, `forge/starter` | 18.9 KB; 5 files | Contract and empty starter with kit pins. | Contract task-specific; starter generic | Defect D (`FORGE-CONTRACT.md:97–99`). |
| `test_score_forge.py` | 1,773 | 117 tests, including the check ↔ contract map (§14 "measured is stated"). | Mechanism generic | — |

Rough reuse for a different Forge task:
- kit: 85–90%;
- site server and Jira handlers: about 70%;
- scorer machinery: about 50%;
- probe: about 55%;
- oracle, fixtures, public text and mutants: under 10%.

---

## 2. §17.8 accuracy defects mapped to components

| # | defect | component(s) | anchor | note for 2.0 |
|---|---|---|---|---|
| A | Flags drawn inside the app page, never dismissed, covering the app's buttons; the probe swallows the failed click | Custom UI host (kit) + probe | `kit/lib/bridge-page.cjs:39–44` (`:host{position:fixed;…z-index:2147483647}`, no `pointer-events:none`); `bench/forge_probe.mjs:776, 931` (`.catch(()=>{})`) | Host-fidelity class: draw flags outside the surface, or as `pointer-events:none`; record click failures as harness evidence |
| B | `b_comment_exactly_once` (critical) fires on 0 comments, a missing flag or a 404 | Scorer check + probe scenario | `score_forge.py:2241–2252`; probe double click on the same row (`forge_probe.mjs:775–782`) | Critical rows must test only their named consequence |
| C | Consumer declared in the schema's `resolver:{function,method}` form is never invoked | Emulator, and scorer K row | `kit/lib/emulator.cjs:128, 283` (`consumer.function`); also `score_forge.py:968` `k_consumer_shape.function_shape` requires `function` and no `resolver`, so the same app is charged twice | Every schema `oneOf` arm of every module needs an invocation path; verify whether K's preference for `function:` is intended |
| D | Contract says a global publish reaches "every subscription"; the emulator (per the docs) pairs `publishGlobal` only with `subscribeGlobal` | Public contract | `forge/public/FORGE-CONTRACT.md:97–99` vs `site/realtime.cjs:122–123` | Text fix, which means a new era |
| E | A two-attribute range index: Forge refuses it at lint and deploy; kit lint accepts it, the emulator serves it, and the scorer then says "entity unused" | Kit lint (client-side only) + KVS emulator + scorer | `kit/bin/lint.cjs` (mode client-side), `kit/lib/kvs.cjs:134–140, 184`, `score_forge.py:1006` (`_sprint_indexes` needs `len(rng)==1`), and `deploy_findings` M7 (`score_forge.py:1267`), which already says `would_fail` but is priced in `k_manifest_semantics` instead of `l_deployable` | Root cause is NOT a pin lag (N0 below) |
| F | `e_reconcile_economy` steps 1.0 → 0.75 at optimum + 1 call | Scorer + thresholds | `score_forge.py:2851–2865`, `forge-thresholds.json` `economy_rungs` | Make it continuous, like `e_event_economy` |
| G | Manifest-fault rows still pay criticals, and `k_manifest_semantics` re-charges a vacuous root | Scorer composition | `score_forge.py:3076–3077` (`candidate_section_fault` → `g(0.0)` for every row, criticals included); `_row_score`/`_charged` (`:1162–1173`) ignore vacuous rows, so the finding is priced again | Price each root once |
| H | `u_widget_live` is a mean of 4 conditions; polling scores 0.5 instead of DESIGN's 0 | Scorer check | `score_forge.py:2574–2578` | Grade the outcome; polling = 0 |

---

## 3. New findings not in §17.8 (each with its evidence)

**N0 — Defect E is a server-side rule. No kit pin can fix it.**
- The installed CLI (`/usr/local/lib/node_modules/@forge/cli`, version 14.1.0) bundles exactly the kit's `@forge/manifest` 13.6.0, `@forge/lint` 6.3.0 and `@forge/cli-shared` 9.7.0.
- No file in the CLI's `@forge/*` packages contains "range attribute".
- The CLI's lint has a `ServerSideLinter` (`@forge/lint/out/lint/linters/server-side-linter/server-side-linter.js`). It uploads only the interpolated `manifest.yml` and calls `getAppPreDeploymentCheck`.
- `cli-shared/out/graphql/gql/graphql.d.ts:2894` types the rules as `'MAJOR_VERSION_RULE' | 'MANIFEST_INVALID_RULE' | 'SYSTEM_USER_CHANGE_RULE'`.
- So the receipt's `MANIFEST_INVALID_RULE` is a server-side pre-deployment check. The kit's `linter.mode: 'client-side'` (forced by the no-internet design D4) can never see it.

**N1 — Stale trigger clock. Measured, latent today, breaking once limits are virtual.**
- What happens: `deliverNext()` / `deliverProductEvent()` (`emulator.cjs:335–345`) make the site apply the change. `state.applyThrough` → `advanceTo(created)` jumps the site clock forward. The trigger is then invoked with the emulator's OLD `clockOffset`; it is synced only after the invocation (`emulator.cjs:248`).
- Consequences: the trigger child's `Date.now()` lags the event's `changelog.created` by minutes (the change looks like it comes from the future), and `t1 − t0` includes the jump.
- Measured on the Haiku 0.9661 tree: 41 trigger invocations, virtual duration median 348 s, max 914 s; 29 of 41 exceed 25 s. Each actually ran in well under a second.
- Under virtual-time limits (axis 3) a correct app would be timed out.
- Fix: sync the clock before invoking, or have the site return its new `now` in the delivery.

**N2 — Confluence is blocked at the scope layer before any handler exists.**
- `site/openapi.cjs:15` loads only `jira.json` and `jsw.json`.
- `openapi.cjs:36` keeps only `scheme === 'OAuth2'`. `confv2.json` and `conf.json` use `scheme: 'oAuthDefinitions'` (215 of 218 v2 ops carry it).
- `score_forge.py:1084` repeats the filter.
- v2 paths are relative to `https://{domain}/wiki/api/v2`; v1 paths (where page restrictions live) carry `/wiki/rest/api/...`.

**N3 — Fixture scale blockers** (detail in section 1, `fixtures.cjs` row):
- the 6,160-summary vocabulary ends in an infinite loop;
- issue/changelog id bands collide above about 2,500 issues;
- the generator has O(N·E) and O(L·E) loops.

**N4 — Faults are not scoped to an invocation.**
- `site.cjs:63–98`: `consumer-of-change` arms on `signal('consumer-start', originChange)`, then fires on the first Jira request of ANY consumer.
- `x-forge-origin-change` is forwarded and parsed into `caller.originChange`, but `inScope` never compares it.
- `resolver-read` is armed globally too.
- Fine at concurrency 1. Mis-attributed at concurrency > 1.

**N5 — Condition on a missing entity is refused.**
- In `kvs.cjs:231–239` a transaction condition on a missing entity always answers 409 `CONDITIONAL_CHECK_FAILED`, even for `NOT_EXISTS`-style create-if-absent.
- The real semantics are undocumented (the file header calls the codes harness names).

**N6 — App-managed permissions are unusable in the emulator.**
- `@forge/api` 8.2.0 `permissions.hasPermission/hasScope/canFetchFrom` read `getAppContext().permissions` (`api/permissions.js:150–156`) and throw `ApiNotReadyError` when it is absent.
- `emulator.cjs:174–177 appContext()` never sets `permissions`.
- The bridge's `checkPermissions` reads them from `view.getContext()`, and `bridge-host.cjs contextFor` sets none.

**N7 — Webtriggers hold every verdict today.**
- `webTrigger.getUrl()` is a GraphQL mutation (`createWebTriggerUrl`) sent with `__requestAtlassianAsApp('/graphql')` (`@forge/api/out/webTrigger.js`).
- The proxy's `stargate()` answers it 501, so any app using it gets a HELD verdict.
- The request and response shapes are typed in `webTrigger.d.ts` (`WebTriggerRequest`/`WebTriggerResponse`).

**N8 — The site shares one event loop with everything else during scoring.**
- The scoring site runs IN the probe's Node process (`forge_probe.mjs:245–247`), together with the emulator proxy, KVS and the Playwright driver.
- A slow site handler stalls the UI's real-time deadlines: `waitMeaningful` 15 s, `LIVE_SETTLE_MS` 15 s, `settle` ≈ 10 s.

**N9 — The evidence keeps full Jira bodies.**
- `proxy.cjs:79–80` keeps `entry.response` (the full parsed body) for every product call.
- `proxy.cjs` already computes a compact `entry.page` summary (token, isLast, startAt, total, ids). The scorer's paging grader reads `response` (`score_forge.py:1807–1874`), but only for fields `page` already holds.

**N10 — An absolute with no receipt.** `CYCLIC_INVOCATION_LIMIT = 1000` (`emulator.cjs:40`) has none (gate 10). Continuation chains in 2.0 would make it load-bearing.

**N11 — Concurrency is asymmetric today.**
- UI-initiated resolver invocations already run concurrently: `bridge-host.answer` has no lock, and each invoke is its own sandboxed process.
- Triggers and consumers are strictly serial.

**N12 — Module-type lists are hard-coded** in three places:
- `site.cjs:28` `UI_MODULE_TYPES`;
- `forge_probe.mjs:155–156` `KIND` (a new module's resolver calls get `kind: null` and fall out of kind filters);
- `bridge-host.cjs:143` (only the widget gets a host-built extension).

---

## 4. Per-axis analysis for Forge 2.0

### 4.1 A second mock product: Confluence Cloud (REST v2 pages create/update/version, storage format or ADF, page permissions)

What 1.0 already provides:
- The site skeleton is product-agnostic:
  - OpenAPI-driven 404 / 501 / `harness_missing` policy;
  - fault engine;
  - control surface;
  - virtual clock;
  - trace.
- `proxy.cjs` already routes `remote/confluence`.
- The Custom UI host's `fetchProduct` already passes `product`.
- Realtime contexts know `content` and `space`.
- The kit ships `confv2.json` and `conf.json`. Lint's PermissionLinter already maps `requestConfluence` routes to scopes offline (`LOCAL_CONF_V2_SWAGGER`).
- Scope evidence, ADF structural checking (`adf_text`), the paging-walk grader and the whole composition carry over.

Reuse ≈ 45%. The infrastructure is reused; the product layer is new.

New work:
1. `site/openapi.cjs` becomes an OpenAPI registry per product:
   - load `confv2.json` with its `/wiki/api/v2` server prefix and `conf.json` (v1, for `/wiki/rest/api/content/{id}/restriction*`);
   - accept the `oAuthDefinitions` scheme;
   - same change in `score_forge.openapi_ops`.
2. `kit/lib/proxy.cjs`:
   - route `confluence` to the Confluence handler table instead of 404;
   - the page summariser learns Confluence's `results` + `_links.next`.
3. New `site/rest/confluence-v2.cjs`, `site/rest/confluence-v1.cjs` and a `render-confluence.cjs`:
   - pages: create, get, update, delete, versions;
   - spaces;
   - children / ancestors;
   - body representations (`storage`, `atlas_doc_format`, `wiki` on write; `body-format` on read);
   - cursor paging with RELATIVE `_links.next` and a `Link` header;
   - version conflicts;
   - restrictions, plus space permissions combined with page restrictions for asUser reads.
4. `site/state.cjs` gains a content store: page tree, version history with author/message, bodies per representation, labels, restrictions.
5. A Confluence fixture generator (new file) with its own seeded traps:
   - restricted subtree;
   - stale version;
   - storage-only bodies;
   - deep pagination.
6. Emulator events `avi:confluence:created:page` / `updated:page`. `triggerEvent` must become event-type-generic.
7. A new oracle and checks. The scorer machinery is reused.

Fidelity risks (where a guess could unfairly fail a correct app):
- Storage format is XHTML with `ac:`/`ri:` macros. A strict parser that rejects input Confluence accepts would zero honest apps.
- Storage ↔ ADF conversion on read is lossy and Atlassian-owned.
- The 409 shape and message for a stale `version.number` are unmeasured.
- Restriction inheritance differs by operation: view restrictions inherit from ancestors, edit restrictions do not. This needs a receipt.
- The `body-format` default (v2 GET returns no body unless asked).
- Relative cursor links.

Mitigation:
- Measure each rule on wolfaenpak (the sanctioned test tenant) and record receipts the way `limits.cjs` does.
- Grade within the representation the app wrote.
- Accept every documented reading.

Confidence: MEDIUM.

### 4.2 Consumer concurrency > 1 with real interleaving, plus KVS transactions and conditional writes

What 1.0 already provides:
- The KVS API surface is modelled, and its wire shape matches `@forge/kvs` 2.0.7's transaction builder: transactions with `set`/`delete`/`check` + `conditions` (≤ 25 ops, `DUPLICATE_KEY`), `FAIL_IF_EXISTS`, `returnValue`, batch `successfulKeys`/`failedKeys`.
- Every invocation is its own sandboxed process. The proxy attributes concurrent calls by token, so physical concurrency already works (N11 shows the UI path does it today).
- `PushEvent.concurrency {key, limit}` is passed through by `queue.push`.
- The scorer already distinguishes concurrent refusals from early retries in a 429 window (`_refused_in_window`).

Reuse ≈ 55%.

New work:
1. `emulator.cjs drainQueues` becomes a scheduler:
   - up to N deliveries in flight;
   - honour `concurrency.key`/`limit`;
   - per-delivery clocks, because a global `advance()` from one retry jumps time for all.
2. Deterministic interleaving: make `proxy.cjs` the choke point.
   - Every proxied request parks until a seeded scheduler grants it.
   - A step proceeds when every in-flight invocation is parked, finished, or waiting on a virtual timer. That needs the in-child timer agent from 4.3.
   - Score K schedule seeds and keep the worst row, like the 3-site rule. Scoring time grows ×K.
   - The sandbox guarantees the proxy is the only I/O channel, which is what makes this feasible.
3. `site.cjs` faults are matched by invocation (`x-forge-origin-change` / invocation id), fixing N4.
4. `kvs.cjs`:
   - decide and receipt condition-on-missing (N5);
   - decide and receipt transaction conflicts between concurrent writers;
   - decide index-read consistency. Today it is strongly consistent, which is lenient, never unfair.
5. Scorer/oracle: lost-update and duplicate-side-effect rows over race-prone fixtures. Final-state invariants reuse today's outcome checks (`t_no_double_count`, `r_heal_dropped`).

Fidelity risks:
- the real parallelism per queue, and whether `concurrency.limit` is exact;
- KVS conflict and condition semantics and their codes (N5 refuses idioms the platform may accept);
- CPU-bound app code between proxy calls is invisible to the scheduler. Guard it with a real-time watchdog that marks the verdict unavailable, never an app zero.

Confidence:
- MEDIUM-LOW for deterministic interleaving (new engineering, no precedent in this repo);
- HIGH that the KVS surface is already right.

### 4.3 Invocation time limits in VIRTUAL time, with work continued across invocations through the queue

What 1.0 already provides:
- `TIMEOUTS` with the documented limits and `timeoutFor` per module type;
- the async-event retry semantics (`InvocationError` with `retryAfter`, redelivery schedule, 24 h retention);
- `delayInSeconds` (`readyAt`);
- the site's virtual clock (`advance`, `advanceTo`) and the runner's `Date` shift;
- `r_completes_in_timeout`;
- the continuation pattern itself works today: an app can push its cursor and is redelivered.

Reuse ≈ 50%.

New work:
1. `runner.cjs`: an in-child virtual-time agent.
   - Patch `setTimeout`/`setInterval`/`timers/promises` + `Date`.
   - Over an IPC fd, report "idle until virtual T" so the emulator advances time instantly.
2. `proxy.cjs`: a deterministic latency model per endpoint class (scaled by page size) charged to the invocation's virtual budget, with receipts measured on wolfaenpak.
3. `emulator.cjs`:
   - enforce module timeouts on virtual elapsed time (kill by pid, gate 4);
   - keep a generous real-time watchdog that yields `unavailable`;
   - fix N1.
4. The contract states the time model, under the "every harness deviation is stated" rule.
5. Fixtures large enough that no single invocation can finish (couples to 4.4).
6. Scorer rows: continuation completeness (nothing lost or duplicated across slices) and idempotent restart after a mid-slice kill.

Side benefit: it removes 1.0's real-time load sensitivity (`--quiet-load`).

Fidelity risks:
- The latency model decides who finishes, so it must be calibrated with generous margins and stated.
- Virtualising timers can break code that relies on real timers inside the wrapper or `fetch`/undici. Test against the pinned wrapper.
- Not counting CPU time is lenient, never unfair.

Confidence: MEDIUM.

### 4.4 A 10–50× larger seeded site (tens of thousands of issues, dozens of boards and sprints)

Per-request cost and memory, read from the code:

| path | cost per request today | at 12k–50k issues |
|---|---|---|
| `GET/POST /search/jql` page | `allIssues()` array copy + filter + full sort of all hits; RE-RUN for every page (`platform.cjs:31–42, 276–290`) | O(N log N) per page, estimated 0.03–0.4 s; a fielded walk at 100/page is 120–500 pages, so roughly 4–200 s of site CPU per walk (estimate) |
| `changelog/bulkfetch` page | collects all histories of ≤ 1,000 ids, sorts with `Date.parse` inside the comparator, `grouped.find` O(page × groups) | Fine per page; redundant per page |
| agile sprint/board issue lists | filter over all issues + sort per page (`agile.cjs:25–40`) | O(N log N) per page × dozens of sprints |
| KVS entity query | scans every entity record of every entity, then sorts (`kvs.cjs:171–201`); cursor `findIndex` O(E) | O(E log E) per page |
| proxy log | full response bodies retained (N9) | Observations ×10–×50: about 20–600 MB typical; 0.6–2.9 GB for today's worst app. Python `json.loads` needs 5–8× that in RAM |
| site memory | pack ~1.1 KB/issue JSON + ~0.35 KB/history entry; `clone()` of fields per issue | Pack about 3.5 MB (10×), 17 MB (50× ≈ 12k), 70 MB (50k); heap ~150–400 MB per site, in the probe process (N8) |
| per proxied call | measured 0.6–1.5 ms | Grows with the cost of the page handler above |

Generator, oracle and probe at scale:
- **`fixtures.cjs`** is blocked (N3):
  - the infinite loop beyond 6,160 summaries;
  - id collisions above ~2,500 issues;
  - O(N·E) loops that make one pack minutes-to-tens-of-minutes at 50× (an estimate from loop structure; at 229 issues it is 23 ms).
- **`forge_oracle.py`** is O(A·N·E) plus O(E·L) per `estimate_of`:
  - today ≈ 0.35 M membership-scan steps plus ~2 M dict compares;
  - at 10× (A ≈ 10, N 2.4k, E 2.5k, L 380), roughly 10⁸–10⁹ Python operations: minutes to an hour per Oracle, with 6 Oracles per tree;
  - at 50×, 10¹⁰–10¹¹: hours to days. **Infeasible without per-issue indexing**; indexed, it is seconds.
- **The probe UI phase** is linear in scrum boards × active sprints (`probeUi` loops every board and every active sprint, light and dark). Today 2 boards and 3 sprints take ~90 s per site. With dozens of each, estimate 15–20 min per site, ×3 sites. The probe must sample boards and sprints by seed.
- **Real-time invocation timeouts**: a correct backfill making 10–50× more calls, at today's per-page site cost, exceeds the 55 s default from harness overhead alone. That is unfair unless 4.3 lands or the per-page cost drops.

Reuse ≈ 40%. Every component is touched, but its architecture holds.

New work (blast radius):
- `fixtures.cjs` rewritten for N boards/sprints:
  - a large vocabulary or numbered summaries;
  - re-banded id classes;
  - indexed simulation.
- `platform.cjs`/`agile.cjs`: hit lists cached per (JQL, caller, state version) under the page token that already binds the JQL hash.
- `kvs.cjs`: per-entity index maps.
- `proxy.cjs` + probe: keep `page` summaries, drop bodies past a ratio, or spill to NDJSON.
- `score_forge.py`: paging grader reads `page`; `rows_for` indexed by change id.
- `forge_oracle.py`: entries indexed per issue.
- `forge_probe.mjs`: seeded sampling of boards and sprints; the site moved out of process.

Feasibility after those fixes: an estimate of 10–30 min of scoring per tree at 50× with 3 sites (not measured).

Fidelity risks:
- harness slowness turned into app timeouts or UI deadlines (N8);
- documented page caps become load-bearing (ids-only 5,000, fielded 100, bulkfetch 1,000 issues / 10 fields), and some are "measured" receipts from one site on one day.

Confidence: HIGH on the diagnosis; MEDIUM on delivery without regressions.

### 4.5 Scheduled mid-run world changes (sprint closes, issues move boards/projects, estimation field changes on a board, an issue deleted, a permission revoked mid-session)

What 1.0 already provides:
- the virtual clock;
- the site-owned delivery plan (slots, duplicates, drops);
- the live-UI slot pattern (changes held back and delivered at a chosen moment with a surface open);
- the two-state oracle (`include_live_ui`), a seed of a timeline oracle;
- the probe's phase structure (backfill / live / heal / rerun) as checkpoints.

Reuse ≈ 35%.

New work:
1. `site/state.cjs` owns a mutable world:
   - sprints with state transitions (close: `completeDate`, open issues to the next sprint or backlog with the changelog Jira writes);
   - board configuration (the estimation field);
   - permissions (`hiddenFrom` mutable mid-session);
   - issue deletion (404, search omission, bulk `issueErrors`);
   - issue moves (new key, project change, old-key redirect).
   - Route the 31 direct `pack.*` reads in `rest/*.cjs` through it, and extend `applyItem` beyond its 6 field kinds.
2. Pack: a `world` timeline with virtual timestamps.
3. `emulator.cjs`: emit `avi:jira:deleted:issue`, sprint started/closed events and move updates. Today `triggerEvent` hard-codes `avi:jira:updated:issue`.
4. Probe: interleave world events with phases and with open surfaces (a permission revoked while the sprint action is open).
5. Oracle: computed at each checkpoint. The contract must decide, for example, whether a deleted issue's past change stays in the ledger.

Fidelity risks (high):
- what Jira does when a sprint completes (which changelog items, one event per moved issue or not);
- project moves (key change items, event types);
- whether a board estimation change is retroactive;
- permission propagation timing;
- event payload shapes.

Each needs a wolfaenpak receipt before it is graded. Where the docs are silent, grade outcomes and accept every reading.

Confidence: MEDIUM-LOW.

### 4.6 A BROWNFIELD start (inherit a v1 app plus pre-populated KVS data in an old schema, migrate live)

What 1.0 already provides:
- `bench/golden-forge` is a complete, 1.000-scoring v1. Its entities `scope-change` (ranged by float `at`) and `sprint-issue` (two indexes) give a realistic old schema.
- `createEmulator({devState})` already loads a KVS dump and pending queue items (`emulator.cjs:166–169`), so seeding old-schema data and v1-shaped pending events is supported.
- forge-dev persists the same state.
- Old data can be produced by running v1 through backfill and live on the scoring site with today's probe code, or written deterministically from the pack.

Reuse ≈ 50%.

New work:
1. `emulator.cjs upgrade(appDir)`: rebuild and swap bundles and manifest while keeping KVS, queues (old-payload events still pending), realtime subscriptions and widget configs. In-flight v1 invocations finish on v1.
2. `kvs.cjs` models schema-change semantics:
   - today an index change applies instantly and consistently, and reads of an entity no longer declared throw `INVALID_ENTITY_TYPE`;
   - Forge's real behaviour (asynchronous re-index, refused changes at deploy) must be measured.
3. The starter becomes the v1 tree.
4. A new idle control: "v1 unchanged" must sit at or below a stated floor, replacing the empty-starter ≤ 0.05 rule.
5. Migration rows: every v1 row is present in v2 shape; nothing is double-migrated under concurrent events; migration resumes across invocations under 4.3's limits.
6. Mutants against a v2 golden.

Fidelity risks:
- entity schema change and re-index behaviour, and which changes deploy refuses (the server-side rules again, see 4.8);
- deploy cut-over semantics.

Confidence: MEDIUM.

### 4.7 New module types

Shared reuse:
- The Custom UI host serves any module with a `resource`, with the real CSP, theming, invoke and router.
- `invokeResolver` resolves `module.resolver.function` and `edit.resolver.function` generically.
- Client-side lint validates all 211 module types of schema 13.6.0.
- The spike already served a `spike-issue-panel` (`forge-dev.test.cjs:90`).
- Overall reuse ≈ 50%: about 75% for UI-hosted modules, about 25% for modules the PRODUCT calls back into.

| module | new work (file → mechanism) | fidelity risk |
|---|---|---|
| `jira:adminPage`, `jira:projectSettingsPage` | Context shapes in `bridge-host.contextFor`; generalise `UI_MODULE_TYPES` (`site.cjs`) and `KIND` (`forge_probe.mjs`); probe flows; `urlFor` already knows `projectSettingsDetails` | LOW |
| issue panel / glance (`jira:issuePanel`, `jira:issueContext`) | Issue context `{issue:{id,key,type}, project}`; panel actions; same generalisation | LOW–MEDIUM |
| webtrigger | Emulator HTTP front door: request → `WebTriggerRequest {method, body, path, headers, queryParameters}`, response `{statusCode, statusText, body, headers}` (typed in `@forge/api` 8.2.0). Proxy answers the `createWebTriggerUrl` GraphQL mutation (N7). Then `secretKeyConfig` | MEDIUM (`secretKeyConfig` and HMAC semantics unconfirmed per RESEARCH) |
| app-managed permissions / "app roles" | Put the granted set `{scopes, external}` into `_meta.appContext.permissions` (`emulator.cjs`) and into `getContext` (`bridge-host.cjs`), N6. An admin-grant timeline couples to 4.5. "App roles" has no source in this repo and needs research before design | HIGH: how the platform fills `appContext.permissions` and when it changes is unmeasured; only the SDK's reading side is known |
| workflow validator / condition / post-function | The site gains a workflow engine (statuses, transitions, `POST /issue/{id}/transitions`; today `WRITE_OPS` is comment-only) and a SITE→EMULATOR callback that does not exist today. Expression-based variants need a Jira-expression evaluator (why RESEARCH §6 D was held back) | HIGH. The current variant set per module must be re-read from the docs; not fetched here |
| `jira:customField` with a value function | The site calls the app's value function when issues are read or searched (callback, batching, caching, partial failure); the field appears in `/field`, JQL and search | HIGH (when Jira invokes, batch shape, caching are unmeasured) |
| Confluence macro / `confluence:contentAction` | Needs 4.1 plus the macro context and config protocol and the content-action modal | MEDIUM–HIGH |

Confidence: MEDIUM.

### 4.8 Real-Forge lint fidelity

What 1.0 already provides:
- `lint.cjs` reproduces the CLIENT half of `forge lint` exactly: the same package versions the 14.1.0 CLI bundles (N0).
- `deploy_findings` M7 already encodes the multi-range rule as `would_fail` (`score_forge.py:1267`).
- `l_deployable` is critical and puts a 0.499 band on lint errors.

Reuse ≈ 70%.

New work:
1. A server-side rule corpus, for example `forge/kit/lint/server-rules.cjs`.
   - Each rule is an offline predicate over the interpolated manifest. That is all the server-side linter uploads.
   - Each carries a wolfaenpak receipt: CLI version, requestId, exact message, as the `lz-range-probe` one does.
   - Run it as a lint stage, so entrants see it in `npm run lint` and `l_deployable` charges it.
   - Move M7's pricing from `k_manifest_semantics` to `l_deployable`.
2. A corpus builder: run real `forge lint` and `forge deploy --no-verify` on wolfaenpak throwaway apps over a mutation catalogue (index shapes, attribute types, counts, names, module keys). Add a regression test that the offline corpus agrees with every recorded server verdict.
3. A drift sentinel: compare the kit's lint-modules versions with the newest CLI's bundled `@forge/*` on each re-pin.
4. Optional, report-only: a post-score server replay that HOLDS a verdict when offline and real lint disagree. That keeps the score hermetic while gate 1 never silently trusts the offline half.

Fidelity risks:
- Server rules change without a CLI release (they are server-driven), so the corpus goes stale silently unless re-validated on a schedule.
- An unmeasured, docs-inferred rule would over-fail a deployable app. Admit only measured rules.

Confidence:
- HIGH on the mechanism, which the CLI source proves;
- MEDIUM on keeping the corpus current.

---

## 5. Shared mechanisms (one build serves several axes)

1. **Deterministic discrete-event core.** An in-child virtual-time agent (timers + `Date` over IPC) plus a proxy-gated, seeded scheduler and per-invocation virtual clocks.
   - Serves 4.2, 4.3, 4.5 and the timeout side of 4.4.
   - It also removes the stale-clock defect (N1) and 1.0's real-time load sensitivity.
2. **A mutable world with a timeline in `site/state.cjs`.**
   - Serves 4.5, 4.6 and the Confluence content store (4.1).
   - Prerequisite: route the 31 direct `pack.*` reads through state.
3. **A product-plural site.** OpenAPI registry per product, both scope schemes, handler tables per product, event-type-generic `triggerEvent`, site→emulator callbacks. Serves 4.1 and 4.7.
4. **An evidence diet and indexing.**
   - Page summaries in place of bodies (the proxy already computes them).
   - Per-issue indexed oracle; per-change-id indexed scorer rows.
   - The site out of the probe's process.
   - Serves 4.4 and, indirectly, every axis (3 sites × K schedules multiplies today's 6–7 min).
5. **Measured-rule corpora with receipts.**
   - Lint server rules (4.8).
   - Platform semantics the emulator currently invents: KVS conflicts and conditions (4.2), Jira world-change behaviour (4.5), schema-change behaviour (4.6), module callbacks (4.7).
   - Wolfaenpak is the sanctioned bed for all of them.
6. **Generalised module-type registries** in place of the three hard-coded lists (N12). Serves 4.7.

---

## 6. What I did not verify, and where I am least sure

- Per-invocation process cost (spawn + `sandbox-exec` + wrapper load) is not in the observations. The normaliser drops `ms`. I only know the backend phases fit in about 35–50 s per site.
- Every at-scale timing in 4.4 is a code-derived estimate, except the pack-size scaling (from one measured pack) and the per-call overhead (measured).
- Things I did not read the docs for:
  - the current variant set of the Forge workflow modules;
  - custom-field value-function invocation semantics;
  - "app roles".
  These are flagged where used.
- Whether `k_consumer_shape`'s preference for the `function:` consumer form is intended design or a second charge of defect C.
- The website (`validateForgeAdmission`, `sync-forge-public.py`) mirrors the composition. A Forge 2.0 era needs the same rules there; not read here.
