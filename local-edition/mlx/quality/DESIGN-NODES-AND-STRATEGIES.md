# Nodes and strategies: design for lane D (Q-193, Q-194, Q-195, Q-196)

Written 2026-09-27 by the lane D architect. The code was read at `a548c2ce0` (main, which carries the engine glance).
The installed app was walked read-only on 3.0.60 and 3.0.61, and the screenshots are in
`~/goose-screenshots/nodes-design/`. No product code was written; implementers cut from this document.

**Revised 2026-09-27 after an adversarial review** (verdict CONFIRMED-WITH-CORRECTION). Every item was
re-verified against the code at `d95aea476` before it was accepted; §13 lists each item, what changed, and the
evidence. The load rule, the loader, Tier A, the migration, the colours and the slice cut all changed. S1 (the
information architecture move) is already being implemented and is unchanged except that it no longer edits
`main.ts`.

---

## 0. What this document decides, in fifteen lines

1. **A new left-nav place called Nodes, placed first.** It holds everything a user hands work to, with two tabs, Nodes and Strategies. Swarm Settings leaves Providers. "Nodes" is the one naming decision surfaced to the owner, who said "Swarm settings" (§11.1).
2. **Providers keeps two tabs: LeanZero MLX and Cloud Providers.** LeanZero MLX gets four routed tabs: Engine · My Macs · Models · Sampling. A four-step setup strip on top makes the flow Macs → models → Run it → nodes visible.
3. **Physical resources are separate from what the user defines.**
   - Resources: your Macs and your signed-in cloud accounts.
   - A way to run a model: one of the placement planner's candidates (single on a Mac, or a tensor or pipeline split).
   - A node: a named definition, meaning a model plus one way (MLX) or a provider plus a model (cloud).
   - A strategy: roles mapped to nodes.
4. **Every node card shows at a glance:** its kind, where it runs, the model, split or not, live state, memory against budget, measured speed, and who uses it. All of it comes from the derivations that already exist (engineFigures, the glance, the fit rule, the speed store). There is no second derivation.
5. **A strategy maps roles to an ordered chain of nodes.** The roles are Chat, Planning, Build, Testing, Frontend and Backend. In each chain the 1st node is the primary, and the 2nd and later are fallbacks, each with a weight. Each role also carries two settings:
   - A *when* rule: failover, overflow, or share by weight.
   - An *if not loaded* rule: load it and wait, or use the next node meanwhile.

   The UI states each rule back as a sentence in plain words.
6. **The load rule is today's real rule: ONE MLX way serves this Mac's goose at a time, across all your Macs.** The router serves this Mac's chat from exactly one MLX engine (a remote-single route, else the split, else this Mac's single engine), and Run it stops every serving way before it starts another (Q-119). v1 adopts that rule instead of the "one way per Mac" the first draft assumed. To serve a node that is not running, the goosed-side loader:
   1. Refuses loudly while a swarm build holds the engine, or while a way it would stop belongs to a "Keep loaded" node.
   2. Stops **every** running way, but only once no agent **reply** (not a single model call) that was open before this demand still uses it, in any goose process on this Mac.
   3. Lets the one fit rule judge the result, then loads through the same start paths Run it uses, including Run it's discover-and-configure path for a split set up for another model.
   4. Records how long the load took.

   No clock decides anything (gate 5); every wait is on a reply ending or a record changing. Cloud nodes are always "loaded". Two ways at once is S10, behind a measurement.
7. **Chat routes through the existing `swarm` provider, and swarm.rs is not touched.** Two new model ids, `node:<id>` and `strategy:<id>`, route through the router. Delegates of a strategy session use its Build role, and only when the delegate's provider resolves to `swarm`.
8. **Swarm builds come in two tiers.**
   - Tier A: the build strategy is compiled into the existing `swarm` block, handed only to the spawned run through the `SWARM` env var. There is no engine change and the benchmark is untouched. It is narrow on purpose: a build reaches LeanZero MLX only through this Mac's single engine at `mlx_engine.port`, so split and remote-single nodes and a cloud Planning node are refused for builds with their reason, and LM Studio models loaded on the fleet always join the build.
   - Tier B: roles per phase and per task, plus swaps at phase boundaries. This is an engine change, gated on a measured sb-7 run at or above 0.4616.
9. **Slices, in order, with confidence** (re-cut; every file has exactly one owning slice, §9):

   | Slice | Scope | Confidence |
   |---|---|---|
   | S0 | Contract, store and the pure rules | high |
   | S1 | Information architecture move (in flight) | high |
   | S2 | Node cards, New node, Run it's Save as node | medium-high |
   | S3 | Chat and build routing | medium-high |
   | S4 | Chip, session start and turn line | medium |
   | S5 | Loader, holders and load measurements | **low-medium** |
   | S6 | Strategies and the Nodes page shell | medium |
   | S7 | Glance, nav chip and My Macs | medium-high |
   | S8 | Swarm runs register as engine holders | medium |
   | S9 | Swarm Tier B (gated) | **low** |
   | S10 | More than one MLX way at a time (gated) | **low** |
   | S11 | State harness and live check | medium-high |

---

## 1. The owner's ask, as requirements

| # | Requirement | His words |
|---|---|---|
| R1 | Nodes are a first-class place in the left nav, not a Providers tab (**the requirement he called mandatory**) | "Swarm settings in the left navigation", "it's buried in providers" |
| R2 | My Macs is a LeanZero MLX tab beside Engine | "my Macs should actually go under Leanzero MLX as a tab next to… Engine" |
| R3 | The UI implies the flow: connect Macs, run a model, make nodes, start sessions on them | "an easy flowing way to start connecting this, the UI should imply this" |
| R4 | A node shows what it is at a glance, not just a model dropdown | "it doesn't offer any information on what the node is at a glance" |
| R5 | A node has evolved with distributed: single or split, which Macs | "it has not evolved with the rest where we added distributed" |
| R6 | Nodes are virtual definitions, not physical Macs | "definitions which is not the same as the physical nodes" |
| R7 | LeanZero MLX node variants follow the engine's recommendation (single, distributed) | "variations that the engine can recommend for distributed" |
| R8 | Strategies: primary and secondary, when to use which, weights | "primary node, secondary node, and when to use one or the other as well as weights" |
| R9 | Roles: planning, execution, testing, frontend, backend | "when a node is better used for planning or … execution or testing or frontend or backend" |
| R10 | The MLX engine loads and unloads models to serve strategies | "Leanzero mlx engine also gets the ability to unload and load models based on the strategies" |
| R11 | A session starts on one of the available nodes, and the choice is always visible | "when you start a session you choose from your available nodes" |
| R12 | Cloud nodes are simple: a provider plus a model | "with Cloud it's simple, each node you create for openrouter with a model in mind" |

The first draft marked R11 "mandatory". The owner's "mandatory" was about moving Swarm settings into the nav (R1); R11 is met by an always-visible, always-named chip with "Any node (Auto)" as the default (§11.7).

The holes he admitted, and where this design fills them:

- What a node is when it is not running. The node state vocabulary is in §4.3.
- What happens when two roles need two models. That is the load rule in §6.4.
- How a task gets a role. §6.2 says which classifications exist today and which are new.
- How a chat uses a role. §6.2 and §7.1 cover it.
- What becomes of the existing pool and its Share column. It stays, editable, as "Your swarm pool" (§4.5, §5.3); weight has one meaning (§6.3).

---

## 2. What exists today (mapped at `a548c2ce0`, re-checked at `d95aea476`)

### 2.1 UI

Abbreviations used in this section: `S` = `ui/desktop/src`, `LZ` = `S/components/leanzero-swarm`.

**Routing.**
- The app uses a HashRouter (`S/App.tsx:853`). The Providers route is `leanzero-swarm` (`App.tsx:832`), `/mlx-engine` redirects to it (`:834`), and `LeanZeroSwarmRoute` passes `?tab=` through (`:315-319`).
- Main sends deep links over `set-view` IPC, which `App.tsx` `handleSetView` (`:627-634`) turns into `/leanzero-swarm?tab=<section>`: `?tab=mlx` (`S/main.ts:2191`) and `?tab=link` (`main.ts:2381`, the Link tray's "open").
- `ComposerReadiness.tsx:178` defines `ENGINE_ROUTE='/leanzero-swarm?tab=mlx'`.

**Providers shell: `LZ/LeanZeroSwarmView.tsx` (97 lines).**
- Its tabs are mlx, link (only when the Link feature is on), cloud and swarm (`:52-57`), drawn with `Segmented` (`:71`).
- The active tab is local state seeded from the URL and never written back (`:47-50`).
- "Providers" and "LeanZero MLX" are hardcoded English (`:53`, `:66`).
- The docblock at `:32-34` is stale: it claims a lever panel lives in Settings.

**Swarm Settings: `LZ/SwarmNodesSection.tsx` (571 lines).** It is a `DataTable` with four columns and a Remove row action:
- **Node:** a StatusDot plus the Mac name (`:356`).
- **Provider:** a ghost button that reassigns (`:376`).
- **Model:** `NodeModelCell`, a Combobox of this Mac's models (`LZ/NodeModelCell.tsx:42-152`).
- **Share:** `WeightStepper`, which writes `speed_weight` (`:254-283`).

It reads and upserts the whole `swarm` config key (`:130-182`); it is the **one UI writer of `swarm`**. Add node lives in `LZ/AddNodeDialog.tsx:202`. Cloud nodes are added through IPC `swarm-cloud`, which runs the CLI (`S/main.ts:5656`).

**My Macs: `LZ/MyMacs.tsx` (505 lines).**
- Each `MacCard` (`:336`) shows memory, disk, models, chip, the let-others-use switches and Details.
- Its data comes from `LZ/useMacs.tsx` `MacsProvider` (`:251`), which polls Link and each Mac's status every 5 s. `useMacs()` **throws** outside a provider (`useMacs.tsx:184`); the provider is mounted only by `LeanZeroSwarmView` and by `WithMacs` (which mounts its own when none is above). Nothing outside Providers can read Mac data today.
- It is rendered only inside `LeanZeroLinkSection.tsx:806`, the Link tab.

**LeanZero MLX: `LZ/MlxEngineView.tsx` (3,109 lines).**
- Tabs are engine, models and sampling (`:2396`), with hardcoded labels (`:3003-3013`) and no routing.
- `EngineSection` (`:830`) holds `MlxStateTile` (`:1059`), the model picker, `MemoryBar` and `PlacementCard` (`:1189`), which is **Run it**.

**Run it: `LZ/PlacementCard.tsx` (1,834 lines).**
- The goal control offers Chat, Long documents and Many requests.
- Its ways are this Mac, "Run on <Mac>" (remote single) and "Run across your Macs" (split) (`waysOf` `:740`).
- `usePlacementPlans` (`:686`) is hard-wired to `mlxPlacementPlan('chat')` and turns a failed read into an empty map (`:695-697`), a silent substitution.
- **Run is a switch across all Macs.** `servingWays` (`:874-915`) lists every serving way — this Mac's single, a peer's remote single, the split — and `switchTo` (`:1263`) stops each before starting ("Q-119: Run for Flash while the Studio served the 27B"). `stopForSwitch` (`:1236`) follows each stop to its end.
- `startWay` (`:1189`) starts a split through `startSplitFor` (`:1128`) whenever the saved split is set up for another model: it discovers the candidate's Macs for this model, builds the config (`splitConfigFor`, `splitPlan`, `cleanConfig` in `LZ/mlxDistributed.ts:427-540`), provisions a Mac's Python when one is missing, and starts.

**Number helpers.**
- `engineFigures()` and `promptProgress()` (`LZ/engineFigures.ts:40`, `:76`), shared by the tile and the glance.
- In the tile: `figures()` `:696`, `measuredOf()` `:689` and `compact()` `:375` (`MlxStateTile.tsx`).
- In `LZ/mlxLiveStats.ts`: `formatRate` `:409` and `compactTokens` `:402`.

**Engine glance.**
- Its single source is `buildEngineGlance` in main (`S/utils/engineGlance.ts:248`, called at `main.ts:2272`), pushed on `engine-glance` (`GlancePush` `:141`, `EngineGlance` `:73` with `nodes: GlanceNode[]`). Renderers report their sessions to main through `glanceStore.ts:103` → `engine-glance-sessions`.
- It is drawn by `S/components/engineGlance/EngineGlanceCard.tsx:445` and `EngineGlanceInApp.tsx`: the dock card sits in the sidebar at `NavigationPanel.tsx:159`.
- The rules for when it shows are in `S/utils/engineGlanceRules.ts`.

**Left nav.**
- The panel is `S/components/Layout/NavigationPanel.tsx:90`. Its items live in `S/hooks/useNavigationItems.ts:37-42`: MCPs, Skills, Memories, Providers.
- Item labels are `navItemMessages` (`:54-72`).
- `/leanzero-swarm` shows only when `isLocal || mlxEngine` (`NavigationPanel.tsx:99-104`).

**Chat model chip.**
- The chip is `S/components/settings/models/bottom_bar/ModelsBottomBar.tsx:192`, and its label comes from `deriveChatServedBy` (`S/components/chatServedBy/chatServedBy.ts:680`). The existing turn line is `chatServedBy/turnLine.ts`.
- Its menu offers "LeanZero MLX" (`:469`), "Change model", which opens `SwitchModelModal`, links, and model settings.
- `SwitchModelModal` lists `swarm/swarm` and `swarm/swarm-build` (`SwitchModelModal.tsx:551-568`) plus cloud providers. **No individual node or LeanZero MLX model is ever offered.**
- A pick is written per session with `acpSetSessionProviderModel`, or to the global defaults with `acpSaveDefaults` (`S/components/ModelAndProviderContext.tsx:110-175`).

**Primitives** (`S/components/lz/index.ts`): PageHeader, Segmented, Button, Chip, StatusDot, DataTable, EmptyState, KeyValue, Panel, Checkbox, Disclosure and Combobox. `LZ/studio.tsx` adds ToneBanner, WeightStepper, StudioSwitch, StudioSelect (custom) and `nodeHue`. The available dialogs are `ConfirmationModal`, `OverlayDialog` and `BaseModal`. `ENGINE_PHASES` and `PHASE_HEX` are in `S/components/lz/tokens.ts:84`.

**i18n.** Strings are declared with `defineMessages` next to the code, and the catalogs are `S/i18n/messages/*.json`. `pnpm i18n:extract` writes en.json, and `pnpm i18n:check` runs as part of `lint:check`.

### 2.2 Backend

**What a node is.** A node is a `SwarmDevice` inside the `swarm` config block (`crates/goose-cli/src/commands/swarm.rs:274-316`). Its fields:
- `id`, `model_id`, `enabled`
- `weight`: concurrency
- `instances`
- `host`: display only
- `provider`: cloud family
- `speed_weight`: routing share
- `supervision`
- `engine`: `None` is LM Studio; `mlx-sidecar` is LeanZero MLX

Pool fields: `planner_model` (`:330`), `planner_also_works`, `speed_weights` (a substring map), `supervision_pool`.

**The block has two readers.**
- The CLI's typed `SwarmConfig`. Its `save_config` (`:1499`) **re-serializes the typed struct**, so any unknown field in `swarm` is dropped the first time the CLI saves, for example on `goose swarm cloud add`.
- The chat router's `PoolConfig` / `PoolDevice` (`crates/goose/src/providers/swarm_router.rs:50-90`). It ignores `speed_weight`, `supervision` and `planner_model`.

**Chat routing.** The `swarm` provider (`crates/goose/src/providers/swarm.rs`) has two routes: `swarm-build` spawns `goose swarm run` (`route_for` `:62`, spawn `:717`), and **every other model name** is routed chat (`route_chat`, which receives the model config, so the model name reaches the router). For each turn the router:
1. Probes every device.
2. Keeps one node per MLX engine (`one_node_per_engine` `:1051`).
3. Prefers the conversation's sticky node, keyed on the system prompt plus the first user message (`:884`).
4. Otherwise takes the node with the most free slots, with ties going to the higher `weight` (`:972`, test `ties_go_to_the_heavier_node` `:1730`).
5. Otherwise queues on every servable node.

MLX nodes get capacity `MAX_CONCURRENT_REQUESTS` (`:194`). The chat context window comes from `get_context_limit` (`providers/swarm.rs:625`), which asks `pool_context_window()` with **no model argument**.

**One MLX engine serves this Mac's goose chat at a time.** `probe_mlx` (`swarm_router.rs:467`) answers in precedence order:
1. A live remote-single route refuses this Mac's own engine: `sidecar_routed_away` (`:231`) says "this Mac's MLX chat is served from <peer> (remote single) — stop it to use this Mac's own engine". The route record is **one** record, `mlx-remote-route.json` (`mlx_remote.rs`, `RouteRecord`): one remote route at a time.
2. Else a live split (`distributed_target`, `:693`, from `mlx_distributed_owner`) is probed in place of this Mac's single engine.
3. Else this Mac's single engine at `mlx_engine.port`.

Together with Run it's switch, that is the product's real rule: one MLX way at a time for this Mac's goose, whichever Macs it uses.

**One goosed per app instance, shared by every desktop window** (Q-257, 2026-09-28; before it every window ran its own goosed, and a second goosed was refused the LeanZero Link mesh the first held, so window 2 could not load the split). `gooseServeLeaseRegistry.ts` `liveLocal`/`acquireLocal` hands every window the live local goosed and starts one only when none is live; each ACP connection is its own agent inside it, and the loader reaches any live one (`nodes_loader.rs` `LiveAgents`). Several goose PROCESSES on one Mac still exist — a `goose swarm run` child, a CLI `goose serve`, another app build — so the rest of this paragraph holds for them. The router's leases, the serving registry and any in-process queue are **per process**. `mlx_serving.rs`'s header says so: work "that leaves through neither door — a `goose swarm run` child process, another app on the port — is not listed". Cross-process facts today are files: the route record, the distributed owner record (`mlx_distributed_owner.rs`), and the Mac-wide load lock (`goose-sidecar/src/machine.rs`, a `flock` with proof-of-gone that ignores `GOOSE_PATH_ROOT`).

**A lease is one model call, not one reply.** `leased()` (`swarm_router.rs:1001`) registers MLX leases only (`LmStudio | Cloud => None`), with the session id available from `session_context::current_session_id()` (`:1031`). `LeasedStream` (`:1374`) holds the lease "for exactly the life of the stream": one completion. An agent reply in a tool loop is many completions. A reply is scoped per ACP prompt in `acp/server.rs` `on_prompt` (`:2564`); each completion re-scopes the session id in `agents/reply_parts.rs:313`. A delegate runs its own session inside the parent's tool call (`agents/subagent_handler.rs:187`).

**The route-load contract.** `RouteLoad` (`swarm_router.rs:1263`): a turn that finds no node while a route loads waits on the route's own state; a failed load "ends the turn at once with the route's words". Installed from `acp/server.rs:967`.

**Swarm build routing.**
- OPEN and SYNTHESIS always use `planner_model` (`swarm.rs:19836`, `:20087`).
- The research fan steals work across the fleet.
- BUILD and INTEGRATE go through `Scheduler::pick_device` (`crates/goose-swarm/src/scheduler.rs:1069-1190`). It filters on `enabled && !supervision && in_flight < weight` and ranks by load, then speed weight. `TaskSpec.preferred_model` (`crates/goose-swarm/src/dag.rs:24`) is only a tie-break.
- The judge uses `aux_model_for_call`, a name heuristic (`fleet_order.rs:422`: "27b" | "dense" | "coder").
- The REPAIR fix target uses `rank_fix_target`.
- No per-role, per-phase or frontend/backend node choice exists.

**How a build reaches models (what bounds Tier A).**
- **LeanZero MLX only at `mlx_engine.port`.** `engines_for_run` (`swarm_engine.rs:1355`) creates the run's **own** `MlxEngineManager`, and `SidecarEngine::new` (`:1128`) points at `http://127.0.0.1:<mlx_engine.port>`. The split listens on the first free port above it (8091, `mlx_distributed_discover.rs:1267`) and the remote single is a relay in goosed; a build reaches neither.
- **The build can mount the engine itself.** `SidecarEngine::ensure_loaded` (`:1255`) mounts `mlx_engine.model_id` under the device's alias when `allow_model_load` is on (`prewarm_pool`, `:1401`): a second loader racing goosed's, and it mounts the configured model whatever the device names.
- **LM Studio residents always join.** `reconcile_pool_with_fleet` (`fleet_order.rs:299-390`) builds the pool from `lms ps` with `enabled: true` hard-set (`:365`); only config-declared sidecar and cloud devices honour `enabled` (`merge_sidecar_devices`, `swarm_engine.rs:1733`; the cloud merge, `swarm.rs:27178`).
- **A cloud planner is replaced while LM Studio has anything loaded.** The planner keep (`swarm.rs` ~27005) keeps `planner_model` only when a device in the discovered-plus-sidecar pool carries it; cloud devices are merged later (`:27178`), so a cloud `planner_model` is overwritten by the LM Studio pick.

**The MLX engine.**
- It runs one model per engine (`crates/goose-sidecar/src/engine.rs:1170-1178`: "Any already-running engine is shut down first").
- Status carries the load phases `waitingForLoad | makingRoom | starting | loading | warming`, together with resident and weights bytes (`:755`), `active_requests`, memory, and `machine_load` (the Mac's load lock).
- The one fit rule is `crates/goose-sidecar/src/fit.rs`. It computes the budget as `min(available − RAM×0.093, GPU ceiling)`, charges the other engines' resident bytes, and credits `freed_by_switch_bytes` (the doc says: "Run replaces the way that serves now, it never adds a second one").
- The placement planner (`crates/goose-sidecar/src/placement/planner.rs:1032`) returns, for each model and goal, candidates of the form `PlacementKey{kind: single|tensor|pipeline, nodes, link}` (`placement/store.rs:42`); this Mac is the relative key `local`. Each candidate carries fit, speed (measured or estimated), `Action` and `Outcome`, and the plan carries `Badge` and `badge_after_stopping`.
- The serving intent (`crates/goose/src/providers/mlx_serving_intent.rs`) records `Single | RemoteSingle | Split` for restore at launch.
- The speed store (`mlx-speed-measurements.jsonl`) holds prefill, decode and ttft per placement. **Nothing measures or stores how long a model takes to load.**

**Subagents.** `summon` resolves a delegate's provider (params, recipe, `GOOSE_SUBAGENT_PROVIDER`, then the parent's) and model (params, recipe, `GOOSE_SUBAGENT_MODEL`, then the parent's) in `crates/goose/src/agents/platform_extensions/summon.rs:1588-1680`. The provider can differ from the parent's. The upstream lead/worker provider was deleted (upstream `c88e9ce10`).

**Config env override.** `Config::get_param` reads the environment variable named by the uppercased key **first** (`crates/goose/src/config/base.rs:733`). The CLI's `load_config` reads `swarm` through it (`swarm.rs:1446`). An env var `SWARM` on a child process therefore replaces the block for that child alone. §7.2 relies on this.

### 2.3 Defects found while mapping

These are filed as findings for the loop. Each is removed by a slice below.

| # | Defect | Evidence | Removed by |
|---|---|---|---|
| D1 | The Share stepper changes build routing but **not chat**. The chat router ignores `speed_weight` and tie-breaks on `weight`, which the build engine uses to mean concurrency | `swarm_router.rs:972`, `scheduler.rs:1069` | S3: Auto's tie-break reads the pool's Share (`speed_weight`); strategy weights are read only by `share` |
| D2 | The `supervision` **doc comments** claim the node "takes the judge, review and synthesis calls". It does not: it only leaves the build pool and prefers idle jobs. No UI surfaces the flag today | `swarm.rs:303-306` (doc comment), `settings/swarm/golden.ts:36-37` (doc comment), `fleet_order.rs` | S2 corrects the TS doc comment; the Rust doc comment rides S9 (the only slice that edits `swarm.rs`). The Planning role replaces the idea (Tier B makes it real) |
| D3 | Dead second door: `S/components/settings/swarm/SwarmSettingsSection.tsx` (1,072 lines) still reads and writes `swarm` and is unrouted | `SettingsView.tsx:89-91` | S1: deleted |
| D4 | Providers and MLX tabs are not in the URL. A deep link cannot open Models or Sampling, and Back loses the tab | `LeanZeroSwarmView.tsx:47-50`, `MlxEngineView.tsx:2412` | S1 |
| D5 | Hardcoded English: "Providers", "LeanZero MLX", the MLX tab labels and the Nodes empty state | see §2.1 | S1, S2 |
| D6 | The planner and judge choice is a model-name heuristic ("27b", "dense", "coder"), a hard-coded bit that gate 10 names | `fleet_order.rs:422` | S9: the Planning role replaces it |
| D7 | The router's `CLOUD_REGISTRY` copies 4 of the **14** `CLOUD_DEFS` rows and works only because the other 10 are identities; a new row with a differing registry name would misroute silently (`unwrap_or(family)`) | `swarm_router.rs:96-109` vs `swarm/cloud.rs:25-124` | S3: a parity test in `cloud.rs`'s test module asserts every row maps identically through the router |
| D8 | Unknown fields in `swarm` are silently dropped by any CLI save | `swarm.rs:1499` | S0: node definitions live in their own key |
| D9 | Five pollers of the same engine status | UI map §2.1 | S2: node cards subscribe to the existing stores and add no poller |
| D10 | `usePlacementPlans` turns a failed plan read into "no plans" | `PlacementCard.tsx:695-697` | S2: a failed read is a state carrying the error's words |

---

## 3. Research: how comparable products present this

| Product | What it does | Take | Avoid |
|---|---|---|---|
| OpenRouter (model-routing, model-fallbacks, presets) | An ordered `models` fallback list, the response names the model that actually served it, and a named **preset** bundles model, fallbacks and parameters | Name the bundle. Always show **which node served the turn** | Auto Router's opaque per-prompt choice |
| LiteLLM Router (routing, tag_routing) | A model group of deployments, with `weight` for load share, `order` for failover tiers, **tags** for routing, and cooldowns | Keep **weight (share) separate from order (failover)**. Tags map onto roles | Exposing a strategy enum. Cooldowns in seconds, which gate 5 forbids |
| Cline, Roo, Kilo, Cursor (modes) | Plan/Act (or Architect/Code) each have their own model. Kilo's per-mode "profiles"; Roo's "sticky" last-used model; Cursor's missing per-mode default is a user complaint | A role picks its node automatically, and the mapping is explicit | Roo's implicit "last used wins" |
| Aider (architect/editor, weak model) | An architect model plans, an editor model edits, and a weak model does chores. Each slot defaults to the main model | Unset roles inherit, and the UI says what they inherit | — |
| Continue (model roles) | Each model lists the roles it can serve, and the Models panel has one dropdown per role | **One row per role** as the strategy editor | A flat list of roles with no "when" |
| Claude Code (sub-agents, model-config) | A subagent `model` of `inherit` or an alias, a documented resolution order, `opusplan` (plan on one model, execute on another), and an ordered `fallbackModel` | Show the **resolution order** ("from the strategy", "same as Build", "fallback") | — |
| LM Studio (JIT, TTL, auto-evict, LM Link) | Load on request, an idle TTL, auto-evict down to one JIT model, `--estimate-only` before loading, and remote devices listed with the device name | Show a memory estimate before a load. Show the device name on every entry | Silent single-model eviction, which would thrash a strategy. A GUI-only "Load anyway" |
| Ollama (keep_alive, MAX_LOADED_MODELS) | `ollama ps` shows NAME, SIZE, PROCESSOR, CONTEXT and UNTIL. When memory is short, requests queue and idle models are unloaded | Swap policy: **queue, then stop only idle ways** | Timers (keep_alive) deciding residency, which gate 5 forbids |
| exo | Topology-aware auto-parallel, a placement preview before creating an instance, memory-weighted layer split, and a memory delta per device | **Preview before load** (our Run it already does this). Show memory per Mac for the split | Placement hidden behind "just run it" |
| Copilot custom agents, OpenCode, Zed profiles | `model` can be an ordered priority list. Hand-off buttons (Plan → Implement). A per-agent model override | The ordered chain. Later: a "Plan → Build" hand-off in chat | — |
| Vercel AI Gateway | `modelAttempts` records every model tried, with its error and status | A per-turn "tried" line when a fallback happened | — |

What carries into the design:

- The word "strategy" stays because it is the owner's word, with the subtitle "a named plan for which node does what".
- The role names follow what users already know: Chat, Planning, Build, Testing, Frontend, Backend.
- The chain is shown numbered (1st, 2nd, 3rd). Weight shows as a share only inside a role that uses "Share".
- Every turn says which node served it, and why whenever that was not the 1st node.

---

## 4. The model

### 4.1 Four layers

```
RESOURCES           WAYS (the planner's candidates)       NODES (user definitions)        USES
─────────           ───────────────────────────────       ────────────────────────        ────
Mihai Macbook  ─┐   single on Mihai Macbook               "27B · both Macs"               a chat session
Work's Studio  ─┼─▶ single on Work's Mac Studio      ─▶   "Flash · Studio"           ─▶   a strategy's roles ─▶ swarm builds
OpenRouter key ─┘   split across both (tensor|pipeline)   "Claude Sonnet · cloud"         a chat's delegates
                    (cloud: always the provider)
```

- **Resource.** A Mac signed in to LeanZero Link (`useMacs`), or a cloud provider with a key set (Cloud Providers). Resources are physical. You connect them; you do not define them.
- **Way.** How one model runs on those resources: `PlacementKey{kind, nodes, link}`. The planner enumerates ways, judges fit and speed, and recommends one (Best, or Best you can start now). Ways are never stored on their own. A node stores the way it chose.
- **Node.** A named definition: a model plus one way for MLX, a provider plus a model for cloud or endpoint. It holds its own settings. Many nodes may name different ways and models; **one MLX way serves this Mac's goose at a time** (§6.4), so two MLX nodes with different ways are never serving together in v1.
- **Strategy.** Roles mapped to ordered chains of nodes, with when-rules and weights.
- **Use.** A chat session picks a node, a strategy or "Any node (Auto)". A strategy session's delegates use its Build role. Swarm builds use the strategy set "For swarm builds", or "Your swarm pool".

### 4.2 Storage: one new config key, owned by goosed

The `swarm` block cannot carry the new fields (D8), and a second free-form upsert door is how D3 happened. So:

- The new key `nodes` is read and written **only** through new ACP methods in goosed. The desktop never upserts it raw.
- **`nodes` never writes `swarm`.** The pool keeps its one UI writer, `SwarmNodesSection` (§5.3). A node that came from the pool reads its model and provider **through** from `swarm` rather than copying them, so the two can never diverge.
- The Rust types are the source. The TS mirror is pinned to the same `nodes.fixture.json` in both test suites, the way `model_identity.fixture.json` pins `node_names_model` and `nodeNamesModel` (Q-128).

```ts
// ui/desktop/src/components/nodes/model.ts  (mirror of crates/goose/src/nodes/mod.rs)
type Role = 'chat' | 'planning' | 'build' | 'testing' | 'frontend' | 'backend';

type NodePlacement =
  | { kind: 'single' | 'tensor' | 'pipeline'; macs: string[]; link?: string } // = PlacementKey; 'local' = this Mac
  | { kind: 'follows' };        // legacy MLX device: serves whatever this Mac's engine serves (Q-128 behaviour)

interface NodeDef {
  id: string;                   // stable slug, unique; equals the swarm device id when it came from the pool
  name: string;                 // unique display name (Q-154: never two "mihai")
  kind: 'mlx' | 'cloud' | 'endpoint';
  model?: string;               // MLX: the model id as modelsList names it; cloud: the provider's model id.
                                // Absent when poolDevice is set: read through from swarm.devices[poolDevice].model_id
  placement?: NodePlacement;    // MLX only
  goal?: 'chat' | 'longDocuments' | 'manyRequests'; // the goal the way was chosen for (MLX)
  provider?: string;            // cloud/endpoint; absent when poolDevice is set (read through from the device)
  keepLoaded?: boolean;         // MLX: the loader never stops this node's way for another demand
  poolDevice?: string;          // set for nodes adopted from the pool: the swarm device this node reads through
  origin: 'pool' | 'user' | 'runIt';
}

interface RoleEntry {
  chain: { node: string; weight: number }[];    // 1st = primary; weight is used only when when === 'share'
  when: 'failover' | 'overflow' | 'share';
  ifNotLoaded: 'load' | 'useNext';              // MLX only; cloud is always loaded
}

interface Strategy {
  id: string;
  name: string;
  note?: string;                                  // the owner's own words for this strategy, shown under its name
  roles: Partial<Record<Role, RoleEntry>>;        // unset roles inherit (§6.1)
}

interface NodesConfig {                           // config key `nodes`
  version: 1;
  defs: NodeDef[];
  strategies: Strategy[];
  declined: string[];                             // pool device ids the user removed as nodes: never re-adopted (§4.5)
  forNewChats: { kind: 'auto' } | { kind: 'node' | 'strategy'; id: string };
  forBuilds: { kind: 'pool' } | { kind: 'strategy'; id: string }; // 'pool' = today's swarm block, untouched
}
```

`nodes/read` returns the defs **resolved**: a `poolDevice` node carries the device's current model and provider, flagged `modelFrom: 'pool'`, and a node whose device has left the pool is shown as "no longer in your swarm pool" (never dropped silently).

**ACP (all defined in S0; S0 owns every DTO in `crates/goose-sdk-types/src/custom_requests.rs` and every dispatch function in `acp/server/custom_dispatch.rs`, so no later slice touches either file):**

| Method | Answers | Body lives in |
|---|---|---|
| `nodes/read`, `nodes/write`, `nodes/removeNode`, `nodes/removeStrategy` | the config, validated writes | S0 `nodes/acp.rs` |
| `nodes/buildEligibility {strategy}` | `Ok` or the named reasons a strategy cannot drive a build (§7.2) | S0 `nodes/project.rs` |
| `nodes/residency` | per node: serving / loading / waiting / not running / refused-last-time, from engine truth plus the installed loader's state | S0 `nodes/residency.rs` + the loader seam |
| `nodes/loadHistory {node}` | the measured loads of that node's model, way and Macs | S0 reads `goose-sidecar/src/placement/loads.rs` |
| `nodes/servedLast {session}` | the last served-turn record of a session: `{node, role, rank, reason, tried: [{node, reason}]}` | S0 `nodes/served.rs` (written by S3's router) |
| `nodes/ensureServing {node}` | `Ready`, `Wait(reason)` or `Refused(reason)` | the installed loader (S5); with none installed, a named refusal: "loading nodes is not available in this goose process; start <node> in Run it" |

**The loader seam.** S0's `nodes/seam.rs` defines a `NodeLoader` trait with an install point, the same shape as `RouteLoad` / `install_route_load`: `ensure_serving`, `note_lease(session, way)` and `note_child(child_session, parent_session)`, and `in_progress()`. With nothing installed the answers are the named refusal above, and the two notes do nothing (with no loader, nothing batches). S5 implements the trait and installs it. That is how S0 compiles without S5 and no file has two owners.

**Validation in `nodes/write`.** The UI shows each refusal verbatim.

- Ids and names are unique.
- Every chain names an existing node, and has at least one entry.
- Weights are integers of 1 or more.
- `placement.macs` names Macs that the roster knows. The key `local` is always this Mac (the node config lives on this Mac and is never shared). An unknown Link node id is **kept** and shown as "not connected", never dropped.
- Removing a node is refused, with the names, while a strategy uses it or while `forNewChats` names it. The dialog knows both from the config it opened on and offers each as a box before anything is tried ("Also remove it from Everyday", "Start new chats on Any node (Auto) instead"); Remove stays disabled, with "Tick the box above to remove it" beside it, until each is ticked. A refusal a box answers is never shown as a red "Not removed"; only a refusal no box answers is, in the engine's words, after the attempt (Q-259).
- Removing a node that live sessions are set to is allowed only with the count acknowledged ("3 chats are set to this node. Their next message will say it was removed"). Those sessions' next turn ends with `nodes.removedNode` naming the node and offering the chip; it never falls to Auto (gate 1).
- Removing a `poolDevice` node adds its device id to `declined`; the pool itself is untouched.
- A write that changes `forNewChats` also writes the global defaults (`GOOSE_PROVIDER=swarm`, `GOOSE_MODEL=node:<id>|strategy:<id>|swarm`) in the same call, so there is one door for "new chats start on".
- `forBuilds = strategy` is refused with `nodes/buildEligibility`'s reasons (§7.2).

### 4.3 Node state: one derivation, never guessed

`nodeGlance(def, facts)` lives in `ui/desktop/src/components/nodes/nodeGlance.ts`. Its facts come from stores that already exist, read **only on the Nodes page**, which mounts `WithMacs` exactly as Providers does (the same poller, on a different route; the two are never mounted together):

- `useMacs`: roster, memory and models per Mac.
- The main-pushed engine glance and `chatServedBy`: what runs, phase, activity, load.
- `usePlacementPlans(goal)`: fit and speed for the node's model and goal. S2 moves it out of `PlacementCard.tsx`, gives it the goal, and makes a failed read a state with its words (D10).
- The speed store, via `measuredOf`.
- `nodes/residency` and `nodes/loadHistory`.

Surfaces outside the Nodes page (the nav chip, the chip menu) never call `nodeGlance`; they read the coarse, goosed-side `nodes/residency` or the app-wide glance, so they need no Mac or plan data (§5.1, §8.5).

It returns one state. Each state has one colour, its words, and the action it offers. The colour table is §4.6.

| State | When (from facts) | Chip | Line under the name | Card action |
|---|---|---|---|---|
| `serving` | the way serving this Mac's goose is this node's way and model (model identity via `nodeNamesModel`) | green "Serving" | the glance's stage: "Writing · 11.2 tok/s", "Reading prompt · 40.7K · 25%" or "Idle" | Stop |
| `loading` | the loader or Run it is starting this node's way | amber, dark ink, "Loading" | `EngineLoad.phase` in the tile's words: "Waiting for another load on Work's Mac Studio", "Making room", "Starting", "Loading · 12.4 of 31.0 GB", "Warming up" | — |
| `ready` | not running, nothing else is serving, and the plan says this way fits and is startable (`Action` not `Unavailable`) | slate, white ink, "Not loaded" | "Starts in about 48 s · median of 3 loads", or "First start not measured yet" | Start |
| `displaced` | not running because **another way is serving this Mac's goose** (any Mac, any way) | slate "Not loaded" plus a slate-outline chip "Another way is running" | "Work's Mac Studio is serving Flash (node Flash · Studio)" | Start (the confirm names what stops) |
| `needsStep` | the plan's `Badge.NeedsBothMacs.needs`, the model missing on a Mac, a Mac's engine not built, or peer permission off | orange, dark ink, "Needs a step" | the planner's own words: "Copy the model to Work's Mac Studio first", "Allow this Mac to run part of a split on Work's Mac Studio" | Open Run it |
| `cantRun` | `TooBig`, a Mac not connected, the engine failed with its own words, or fit Block even after stopping | red "Can't run" | "Too big: short 1.6 GB on Work's Mac Studio", "Work's Mac Studio is not connected to LeanZero Link", or the engine's `last_error` | Details |
| `heldByBuild` | a swarm build holds this Mac's engine (§6.4 holders) | red "Held by a build" | "A swarm build is using Flash · this Mac; it frees when the build ends" | — |
| `follows` (legacy) | `placement.kind === 'follows'` | ink-outline chip (2px solid border, no fill tint) "Follows this Mac" | "Serves whatever this Mac's engine runs: Qwen3.8-27B · split across 2 Macs" | Pin a way |
| cloud `ready` | the provider is configured and the last call succeeded (or no call yet) | green "Ready" | "Always available · billed by OpenRouter" | — |
| cloud `keyMissing` | the provider is not configured | red "Key missing" | "Set up OpenRouter under Providers › Cloud Providers" | Set up |
| cloud `failing` | the last call failed | red "Last call failed" | the provider's error, verbatim | Details |

A saved split set up for another model is **not** a needs-step: Run it's discover-and-configure path handles it today (`startSplitFor`), and the loader carries the same path (§6.4). Only provisioning a Mac's engine, copying a model and permissions stay a step.

Every figure goes through `engineFigures()`, `measuredOf()`, `formatRate()` or `formatGb()`. A new `figureText` is never written; there are already two (D-list §2.1).

Memory "fit" on a card is the fit rule's own numbers from the plan's candidate for this way (`fit.nodes[]`: need against budget per Mac). While the node is serving, it is the live peak against budget, the same numbers the glance's `GlanceNode.peakGb` and `budgetGb` show.

### 4.4 Naming

A new MLX node's name defaults to `<model short name> · <where>`, where `<where>` is "this Mac", "<Mac name>" or "both Macs" / "<n> Macs". Examples: "27B Atlassian · both Macs", "Flash · Work's Mac Studio". A cloud or endpoint node's name defaults to `<model id as the provider lists it> · <provider>`, for example "anthropic/claude-sonnet-4.5 · OpenRouter"; its card carries the short name ("claude-sonnet-4.5") on the model line under the name (Q-439, owner demo 2026-09-28: two providers' "deepseek-v4.1-flash" are different models). Names are editable and unique.

### 4.5 Adoption of today's pool, on every `nodes/read`

This is a pure function, `adopt(swarm_block, nodes_key)`. It is idempotent, it never writes `swarm`, and it never re-adopts a device id listed in `declined`. Each case, and the node it produces:

- **An MLX device** (`engine: mlx-sidecar`): a node with `placement: {kind:'follows'}`, `poolDevice` = the device id, named `This Mac's engine` — after what it follows, never after one Mac: it serves whatever this Mac's engine runs, split across several Macs included (Q-303, 2026-09-28; it was `<Mac name> engine`, and a pool node still carrying that exact generated name is renamed on read; a typed name is kept). A second MLX device on the same Mac would collide with that name (Q-154), so names are deduplicated at adoption: the first keeps `This Mac's engine`, each further one is `This Mac's engine · <device model short name>`, and if that still collides, `· <device id>`.
- **A cloud device:** a cloud node with `poolDevice` = the device id; its model and provider read through from the device.
- **An LM Studio device:** nothing. LM Studio is not offered in this edition (owner rule 2026-09-05). Such devices stay in `swarm` untouched, and the Nodes page shows one line: "2 LM Studio devices in your swarm config are not shown here".
- **A swarm device with no node and not declined** (for example one added by the CLI): adopted by the same rules on the next read.

Adoption writes the adopted defs to `nodes` on the first successful `nodes/write`, not on read, so a read never writes config.

`forNewChats` starts as `auto`. `forBuilds` starts as `pool`. Behaviour is therefore **byte-identical** until the user chooses otherwise.

### 4.6 One palette, three disjoint families

The first draft reused hues across kinds, states and roles (MLX and Build both blue; Cloud, cloud Ready and Planning all violet; Endpoint and Testing teal; Serving and Backend green; needs-step and Frontend orange). The palette is now split into three families that never share a hue, defined once in `components/nodes/hues.ts`:

| Family | Members and fills |
|---|---|
| **Kind** (identity, never a colour) | solid ink `#111827`, white text, plus an icon that carries the kind: MLX (Cpu), Cloud (Cloud), Endpoint (Plug). The where chip is an ink-outline chip (2px solid border) with the Mac icon |
| **State** (one colour per meaning) | green `#15803D` serving and cloud ready (can take work now) · amber `#F59E0B` with dark ink, loading · slate `#475569` not loaded · slate outline, "Another way is running" · orange `#EA580C` with dark ink, needs a step · red `#DC2626` can't run, held by a build, key missing, last call failed |
| **Role** | Chat pink `#DB2777` · Planning purple `#9333EA` · Build indigo `#4F46E5` · Testing sky `#0284C7` · Frontend teal `#0D9488` · Backend brown `#92400E` |

The setup strip (§8.6) uses green for done, blue `#2563EB` for Next (blue is no longer a role) and slate for later steps. The state harness (§10.2) computes CIEDE2000 between every pair of fills that can appear on one card or one strategy row and refuses a pair under 15; the Chat pink against the can't-run red is the pair most likely to need re-picking, and S2 re-picks from the same family if the harness refuses it.

---

## 5. Information architecture and routes

### 5.1 Left nav

`useNavigationItems.ts` `NAV_ITEMS` becomes, in order (S1):

1. **Nodes**: route `/nodes`, lucide `Network` icon.
2. MCPs
3. Skills
4. Memories
5. Providers

The Nodes entry is gated like Providers: `isLocal || mlxEngine`. Its label is `navigation.itemNodes` = "Nodes".

It carries a small solid chip (S7), which shows only while something is worth saying, read from the app-wide engine glance (main-pushed, present on every screen; no `MacsProvider`, no plan read):
- "Loading" (amber) while the glance shows a load phase.
- "Failed" (red) while the glance shows the engine failed.

The first draft's "1 can't run" count needed every node's plan on every screen, which only exists inside a `MacsProvider`; it is dropped rather than bought with a new app-wide poller. A permanent "ready" count is not shown, because it would be noise (Q-8).

### 5.2 Routes

| Route | Shows | Replaces |
|---|---|---|
| `#/nodes` and `#/nodes?tab=nodes` | Nodes page, Nodes tab | Providers › Swarm Settings |
| `#/nodes?tab=strategies` | Strategies tab | new |
| `#/nodes?tab=nodes&node=<id>` | scrolls to and outlines that card (solid 2px ring) | new |
| `#/nodes?tab=strategies&strategy=<id>` | opens that strategy's editor | new |
| `#/leanzero-swarm?tab=mlx&mlx=engine\|macs\|models\|sampling` | LeanZero MLX with that inner tab | today's `?tab=mlx` (the default stays engine) |
| `#/leanzero-swarm?tab=cloud` | Cloud Providers | unchanged |
| `#/leanzero-swarm?tab=swarm` | **redirect** to `#/nodes` | old deep links |
| `#/leanzero-swarm?tab=link` | **redirect** to `?tab=mlx&mlx=macs` | `main.ts:2381` keeps sending `'link'`; `handleSetView` builds `?tab=link` and this redirect lands it. **main.ts is not edited** (changing it to `'macs'` would build `?tab=macs`, which is no route) |
| `#/harness/nodes?state=<id>&theme=<l\|d>` | the state harness (§10.2), registered only when goose runs with `GOOSE_UI_HARNESS=1` | new, test-only (S11) |

Every tab click writes the URL with `setSearchParams(..., { replace: true })`. That removes D4 and makes Back work.

### 5.3 Where each existing component lands

| Component | Today | After |
|---|---|---|
| `LeanZeroSwarmView.tsx` | Providers, 4 tabs | Providers, 2 tabs (LeanZero MLX, Cloud Providers), with an i18n title and subtitle (S1) |
| `MlxEngineView.tsx` tab strip | Engine · Models · Sampling, unrouted | Engine · **My Macs** · Models · Sampling, routed via `mlx=`, plus the setup strip (§8.6) above it (S1) |
| `LeanZeroLinkSection.tsx` (sign-in cards plus `MyMacs`) | Providers › My Macs | LeanZero MLX › My Macs (signed out: the Link sign-in cards; signed in: MyMacs plus LinkRoutesPanel) (S1) |
| `MyMacs.tsx` | inside the Link tab | same component. Each Mac card gains "Nodes on this Mac" (S7) |
| `DistributedLinkPeers.tsx` | used by `DistributedSetup` | unchanged |
| `SwarmNodesSection.tsx`, `NodeModelCell.tsx`, `WeightStepper`, `AddNodeDialog.tsx` | Providers › Swarm Settings | **kept, unchanged, as "Your swarm pool"**: S1 hosts it under Nodes › Nodes; S6's page shell keeps it as a section under the node cards, shown in full while "Swarm builds use" is "Your swarm pool" and collapsed (a Disclosure) otherwise. It stays the one UI writer of `swarm`; its Share stepper keeps meaning routing share, now in both routers (D1) |
| `settings/swarm/SwarmSettingsSection.tsx` | dead, unrouted | **deleted** (S1) |
| `CloudProvidersSection.tsx` | Providers › Cloud | its line "swarm nodes cannot yet" is reworded (S2): "Make a node from any configured provider under Nodes" |
| `PlacementCard.tsx` (Run it) | Engine tab | unchanged position. S2 extracts `PlacementCandidates` (the ways list) and `usePlacementPlans` for reuse and adds "Save as node". Its switch choreography stays in TS in v1, pinned to the loader's by a shared fixture (§6.4 step 9) |
| `EngineGlanceCard.tsx` | sidebar dock, float, desktop | S7 adds one line naming the node the serving way belongs to |
| `ModelsBottomBar.tsx`, `SwitchModelModal.tsx` | chip, then a modal with no nodes | S4: the chip menu lists strategies and nodes (§8.5). The modal loses the two swarm rows (they move to the menu) and keeps cloud and endpoint models |

### 5.4 What Providers keeps

Providers keeps where models come from:
- **LeanZero MLX:** your Macs, the engine, models and sampling.
- **Cloud Providers:** credentials and endpoints.

Its new subtitle: "Where your models run: the LeanZero MLX engine on your Macs, and the cloud providers you've signed in to. Turn them into nodes under Nodes."

---

## 6. Strategies

### 6.1 Roles, and what each means in words

Each role has one line of plain words, shown in the editor and on the strategy card. Every role's colour is a solid hue from the role family (§4.6), and the harness checks its contrast and its distance from every state colour.

| Role | Words the user reads | Hue | Inherits when unset |
|---|---|---|---|
| Chat | "Your turns in a chat: answers, edits, tool calls" | pink `#DB2777` | Build |
| Planning | "Reading the request, asking questions, researching and writing the plan" | purple `#9333EA` | Chat |
| Build | "Writing the code for each task" | indigo `#4F46E5` | Chat |
| Testing | "Running and checking the result, and fixing what the check finds" | sky `#0284C7` | Build |
| Frontend | "Build tasks that write the user interface (pages, components, styles)" | teal `#0D9488` | Build |
| Backend | "Build tasks that write the server side (APIs, services, data)" | brown `#92400E` | Build |

The resolution order is shown in the UI, as Claude Code does. An unset role row reads "Same as Build" in full-strength text with the Build chip; it is never greyed out.

Validation:
- A strategy must set at least one role.
- Inheritance cannot cycle. Chat and Build inherit from each other only when both are unset, which validation refuses.

### 6.2 How work gets a role: honest about what exists

| Role | Swarm today | Chat today | New (and in which slice) |
|---|---|---|---|
| Chat | — | every turn of a session (the `swarm` model routes it to a pool node) | S3: a `strategy:<id>` session's turns resolve to the Chat chain |
| Planning | OPEN and SYNTHESIS use the single `planner_model` pin. The research fan steals across the whole fleet. The judge uses a name heuristic (D6) | none | Tier A (S3): `planner_model` := Planning's 1st node, local-single MLX only (§7.2). Tier B (S9): OPEN, ASK, the research fan, SYNTHESIS and judge looks resolve through the Planning chain |
| Build | BUILD tasks go through `pick_device` over the enabled pool | a delegate (`summon`) inherits the session's model | S3: a strategy session's delegates get `strategy:<id>@build` when their provider resolves to `swarm`. Tier A: the pool := Build's chain with its weights, plus any LM Studio residents |
| Testing | the INTEGRATE sink and REPAIR's `rank_fix_target` exist as separate choices with no role | none | Tier B only: the sink lanes and REPAIR fix lanes resolve through the Testing chain |
| Frontend / Backend | **no classification exists** | none | Tier B only: a BUILD task's role is **declared by synthesis in the plan** (a plan fact, gate 2), carried on the task spec. A task with no declaration is Build and is counted in a loud `role_undeclared{task}` event that tick.py prints. No file-extension table (gate 10: a list sized for today's languages is a typed absolute) |

Chat's honest limit: a chat has no planning phase today. Planning, Testing, Frontend and Backend affect **swarm builds only** (Tier B). Chat uses the Chat role, and its delegates use Build. The strategy editor says so on those rows: "Used by swarm builds".

### 6.3 One meaning of weight; the three when-rules

A weight lives on a chain entry inside a role, and it is read only by the `share` rule. That ends D1's two meanings for strategies. For "Any node (Auto)", S3 makes the chat router's tie-break read the pool's Share (`speed_weight`, added to `PoolDevice`), the same value the build scheduler routes by; with equal or absent Shares the order stays today's (the heavier `weight`, then the first). Concurrency (`weight`) is the node's capacity: MLX `MAX_CONCURRENT_REQUESTS`, cloud `instances`. It is never shown as a user lever.

In the build pool projection (Tier A), the Build role's weights become each device's `speed_weight`.

| When-rule | UI label | Plain-words sentence the UI renders (example) | Router behaviour |
|---|---|---|---|
| `failover` (default) | "Use the next one only when the one before can't run" | "Chat runs on 27B · both Macs. If it can't run, on Claude Sonnet · OpenRouter." | 1st if servable (or loadable per `ifNotLoaded`), else 2nd, and so on. A busy 1st **queues** the turn on the 1st. A 1st whose load **fails** counts as can't run for this turn (§6.4 step 11) |
| `overflow` | "Use the next one when the one before is busy" | "Build runs on 27B · both Macs; when it is busy, the extra tasks go to Claude Sonnet." | 1st with a free slot, else the next with a free slot, else queue on all |
| `share` | "Share the work by weight" | "Build is shared: 27B · both Macs 2 parts, Claude Sonnet 1 part." | Smooth weighted round-robin over servable nodes (deterministic and testable, never random). Chat shares **per conversation, not per turn**: stickiness is kept, because moving a conversation between MLX nodes throws away its prompt cache (the split's 102K cached prefix in the walk). Swarm tasks share per task |

Because one MLX way serves at a time (§6.4), `share` and `overflow` across **two different MLX ways** would swap on every alternation. `nodes/write` refuses a `share` or `overflow` chain with two MLX entries whose ways differ ("Sharing between 27B · both Macs and Flash · this Mac would stop one to load the other on every turn"). Sharing MLX with cloud, or cloud with cloud, is allowed.

`ifNotLoaded` applies to MLX entries:

| Value | UI label | Behaviour |
|---|---|---|
| `load` (default) | "Load it and wait" | The turn waits while the loader makes the node servable (§6.4) |
| `useNext` | "Use the next one meanwhile" | The next chain entry that is servable now takes the turn, and no load is started. The turn line says why |

`ifServingOther` (Q-428, 2026-09-28) applies to MLX entries under `ifNotLoaded: load`. The owner: "the strategy should have the option hopefully to avoid interrupting a node doing its thing."

| Value | UI label | Behaviour |
|---|---|---|
| `useNext` | "Use the next node" | While the Mac serves another node for other chats, the next chain entry takes the turn and nothing is stopped. The turn line says so (`nodes.fellBackServingOther`: "Chat is on {node} ({rank}): {mac} is serving {serving} for chat \"…\""). |
| `wait` | "Wait" | The turn waits for the other node's running replies, then until each chat resting on it is closed or moves to another node, and then it loads. The waiting demand holds nobody behind it: the other chat's next reply is not made to wait behind it, and no other demand queues behind it. |
| `takeOver` | "Take it over" | The node loads as soon as the other node's running replies end. This is the behaviour before Q-428. |

**"Serving another node"** is the loader's own fact, never a clock (gate 5). It holds when a way the switch would stop has either:
- a reply running on it, in any goose process on this Mac (the holders of step 7); or
- a chat of this goosed whose last reply leased that way and which is still open in a connected window.

The chat's last way is `Holds.last_way`. It is kept after the reply ends, and a helper's lease outside a reply never moves it. A way nobody uses is switched under every setting. A running reply is never cut under any setting. Only running replies are published by other goose processes, so an idle chat of another process is not seen; its next reply is.

**Migration.** A role entry saved before Q-428 carries no field and reads `takeOver`, so stored strategies keep today's behaviour. `takeOver` is written back as absent, so they round-trip byte-identical. A new strategy from the editor gets `useNext` when its chain has a next node, and `wait` otherwise. Routes with no strategy to carry a setting keep `takeOver`:
- a `node:` chat;
- a chat's own node set made from the chip (Q-359);
- the one-turn "Answer on {next}" pick;
- a card's Start.

Q-432: the helpers around a reply (the fact check, the memory review, the title and the tool labels) never demand a load. They run on the chat's node when it serves, on a later chain entry that needs no load, or on the node this Mac serves now. Otherwise they are skipped with a logged reason (`background_work::never_switches`, `swarm_router::helper_plan`).

A chain whose entries are all exhausted is a **loud refusal** that names every entry and its reason, exactly the router's "no node can serve this turn" contract. It never falls to "any node", because that would be a silent substitution (gate 1). The user-configured chain is the only fallback, and every step down it is announced.

### 6.4 The load rule: what the MLX engine does physically

The facts it rests on, all in code today (§2.2):
- One engine per Mac serves one model.
- **One MLX way serves this Mac's goose at a time, across all Macs**: a remote-single route refuses this Mac's engine (`sidecar_routed_away`), a split is probed in place of the single, there is one route record, and Run it stops every serving way before it starts another (Q-119).
- The one fit rule judges memory, crediting what a switch frees.
- Loads on one Mac are serialised by the Mac-wide load lock (`machine.rs`).
- Every window of the app shares ONE goosed (Q-257; it was one per window until 2026-09-28). Other goose processes on the Mac remain — a `goose swarm run` child has its own engine manager and can mount the engine itself, and a CLI `goose serve` or another app build is a separate goosed — so the cross-process holders below stay load-bearing; what no longer crosses a process boundary is two windows of one app.

v1 keeps all of these. More than one way at a time (a second route, removing `sidecar_routed_away`, a switch that stops only the Macs it needs, two models on one Mac) is S10, behind a measurement.

**Holders: the cross-process record (S5, `crates/goose-sidecar/src/holders.rs`).** It follows `machine.rs`'s rules: Mac-wide path beside the load lock (ignores `GOOSE_PATH_ROOT`, because the resource is the Mac), one file per process held under a `flock` so a dead holder frees itself, and a recorded holder displaced only on proof it is gone. Two kinds:
- **goosed:** its open replies, each `{session, root_session, way}` — the way the reply's last lease used (from the router through the seam's `note_lease`), and `root_session` from `note_child` (summon registers a delegate's parent).
- **swarm run:** the way its sidecar engine serves, for the life of the run (S8).

The file also carries the **swap claim**: the process that holds it is the Mac's loader for that swap. Another process's loader that wants to swap while the claim is held waits for it (the one-loader election, per swap).

**The loader** lives in goosed (`crates/goose/src/acp/server/nodes_loader.rs`, installed through S0's seam next to `install_route_load` at `acp/server.rs:967`). It runs for chat turns now and for the swarm in Tier B. `ensure_serving(node, demand)` answers `Ready`, `Wait(reason)` or `Refused(reason)`:

1. **Served already?** The way serving this Mac's goose is this node's way and model (`node_names_model`). Answer `Ready`.
2. **Cloud or endpoint?** Answer `Ready`. The provider's own errors are the only refusals.
3. **Held by a build?** A live swarm-run holder exists. Answer `Refused("a swarm build is using <way>; it frees when the build ends")`. The loader never stops an engine under a running build.
4. **Stop set.** **Every** way serving this Mac's goose now (this Mac's single, a peer's remote single, the split), computed exactly as `servingWays` computes it.
   - If any of them belongs to a node marked `keepLoaded`: `Refused("<node> is kept loaded on <Mac>")`.
5. **Needs a step?** The planner's `Action::Unavailable`, the model missing on a Mac, a Mac whose engine must be built (provisioning), or peer permission off. Answer `Refused("needs a step: <planner's or split plan's words>")`. The loader never copies models, provisions or changes permissions; those stay one click in Run it.
   - **A split set up for another model is not a step.** The loader carries Run it's path: discover the candidate's Macs for this model, build the config (`split_config.rs`, a port of `splitConfigFor` / `splitPlan` / `cleanConfig` pinned to the TS by a shared fixture), and start. A split-plan blocker is the refusal, in its own words; a plan that needs provisioning refuses as a step.
6. **Fit.** The fit rule with `freed_by_switch_bytes` = what the stop set frees, measured.
   - If it gives Block: `Refused(<the verdict's message>)`. The desktop shows the verdict and Make room exactly as Run it does.
7. **Replies in flight?** Any **reply** open in any goose process on this Mac (the holders), other than this demand's own root, whose way is in the stop set; or engine `active_requests` above what the holders explain (another client on the port). The answer is `Wait("<way> is answering <n>; loading <node> when it finishes")`.
   - The demand joins one FIFO (one way at a time means one queue). The entry is a guard owned by the demanding reply: a cancelled turn drops it and it leaves the queue before any stop.
   - It is woken **by a reply ending**: in this process, by the reply guard's drop; in another process, by that process's holder file changing, observed on the lifecycle status read the engine view already performs (an observation cadence, never a decision: no seconds value decides anything, gate 5).
8. **Batching per reply (the thrash guard, progress-based).** The unit is an **agent reply**, not a model call: the reply guard is taken in `on_prompt` for the whole turn, so a session in a tool loop keeps its way for every completion of that reply and never swaps mid-reply. Replies on the running way that opened **before** the demand are served first. A reply that opens **after** a queued swap demand and wants the running way waits behind the swap. So two chats alternating between two models cost one swap per **reply** alternation, and none starves.
   - **Delegates.** A demand from a delegate of an open reply is that reply's own (`root_session`): the parent's hold yields to its child, because the parent is blocked inside the tool call and waiting on it would deadlock. When the child's reply ends, the parent's next completion demands its own way back. That is two swaps per delegate call when Chat and Build are different MLX ways, and strategyFit says so before it happens (§8.4).
9. **Start.** The loader takes the swap claim, then goes through the **same core functions** the ACP handlers call:
   - `MlxEngineManager::mount` for a single here
   - the remote-single start for a peer
   - `distributedStart` for a split (after step 5's discover and config when needed)
   - the stop halves: `unmount`, the route withdraw, `distributedStop`, each followed to its end as `stopForSwitch` does

   Stop-first ordering is PlacementCard's: its `servingWays` order, each stop followed until the way lets go, and a switch to the split waiting for the peer's unmount to settle. It is pinned by a shared fixture of (serving ways, target way) → (stop list, start) that the Rust suite and a TS test of PlacementCard's exported `servingWays` both run. **Run it keeps its TS choreography in v1** (§11.9); the fixture keeps the two identical, and the swap claim plus the Mac load lock serialise them. A Run it click during a loader swap is the person's intent: the loader's start then fails as superseded, and step 11 applies.
10. **Record.** When the way answers `/v1/models`, a row is appended to `mlx-load-measurements.jsonl` (`goose-sidecar/src/placement/loads.rs`): `{model, placement, macs, weights_bytes, phases_ms{starting, loading, warming}, total_ms, file_cache_warm: bool, outcome}`. It is written at the three ready paths themselves (the manager's mount-ready, the remote single's ready, the split's ready), so Run it's loads are measured too, not only the loader's.
    - The UI shows the median of measured loads of the same model, way and Macs: "about 48 s · median of 3 loads", or "First load not measured yet".
    - **No estimate is shown in place of a measurement** (gate 1).
    - The measured time is **displayed, never used to decide** which node loads (gate 5).
11. **A failed load.** The stop set is already stopped; the loader does **not** restore it (that would be a second load nobody asked for, and it can fail too). The outcome is recorded (`outcome: failed`, with the engine's words). The demanding turn follows its role's rule: under `failover` with a next entry, the next entry serves and the turn line names the failure ("27B · both Macs failed to load: <words>"); with no next entry, the turn ends with the load's own words, the `RouteLoad` contract. The sessions whose way was stopped get the displaced notice with the failure named.
12. **A cancelled turn.** Before the swap claim is taken, cancelling removes the demand and nothing happens. After the stops have begun, the swap runs to its end (a half-stopped Mac is worse than either way) and is recorded `outcome: cancelledAfterStop`; the target stays loaded.
13. **Serving intent.** A load by the loader writes the serving intent like any owner start, so a relaunch restores the last way the strategy used.

**What the user sees.** The strings are in §8.7:
- the composer line on the waiting chat
- the displaced chat's notice ("27B was stopped for Flash · Studio in chat 'Kickoff notes'; your next message loads it back")
- the Nodes card in `loading`, `displaced` or `heldByBuild`
- the glance's existing loading stage, now naming the node

Cloud nodes never enter the loader's stop sets.

---

## 7. Where strategies take effect

### 7.1 Chat sessions (S3 + S4 + S5; **no swarm.rs change**)

**Model ids on the existing `swarm` provider.** `route_for` already sends every name other than `swarm-build` to routed chat, and `route_chat` receives the model config, so the new ids are parsed in the router:

| Model id | Meaning |
|---|---|
| `swarm` | "Any node (Auto)", today's pool routing; the tie-break reads the pool's Share (D1) |
| `swarm-build` | unchanged |
| `swarm-build:strategy:<id>` | a build driven by that strategy (Tier A, §7.2) |
| `node:<id>` | exactly that node |
| `strategy:<id>` | that strategy's Chat chain |
| `strategy:<id>@<role>` | that strategy's role chain. Used for delegates |

**The router** (`swarm_router.rs`) turns a route into its candidate nodes:
- It builds them from node definitions (§4.2), not from `swarm.devices`.
- It reuses the same probes, `one_node_per_engine`, stickiness, slots and queueing. Because one MLX way serves at a time, a chain's MLX entries other than the serving way's are "not loaded", never "servable elsewhere".
- It applies the when-rule.
- It calls the loader (through S0's seam) for not-servable MLX entries whose `ifNotLoaded` is `load`, and notes every MLX lease's way against the session (`note_lease`).
- **For every lease — MLX, LM Studio or cloud — it writes a served-turn record** `{node, role, rank_in_chain, reason_if_not_first, tried}` through S0's `nodes/served.rs`: an in-process ring keyed by session, plus the session's last record persisted in its `extension_data` (key `nodes.served`) so the chip survives a reload. `chatServedBy` reads it through `nodes/servedLast` at the end of each turn (an event, not a poller). Before S5, a not-loaded MLX node is the seam's named refusal; NoNodeNotice then offers **Start <node>**, which opens Run it pre-selected on that way.

**Context window.** `get_context_limit` passes the model name to the router (`route_context_window(model)` replaces the argument-less `pool_context_window()`):
- `node:<id>`: that node's window.
- `strategy:<id>`: the smallest window in the Chat chain, so goose compacts before the smallest node's wall.
- `swarm`: unchanged. This is the same rule the router already applies to the pool.

**Delegates.** In `summon.rs` `resolve_model_config`, when the parent session's model is `strategy:<id>`, **the resolved provider is `swarm`**, and nothing overrides the model (params, recipe, `GOOSE_SUBAGENT_MODEL`), the delegate gets `strategy:<id>@build`. When `GOOSE_SUBAGENT_PROVIDER` or a recipe picks another provider, the model is left exactly as today (a `strategy:` id means nothing to another provider). summon also calls the seam's `note_child(child, parent)` so the loader treats the delegate as its parent's own demand (§6.4 step 8).

**Session start (R11).**
- `forNewChats` is written through `nodes/write`, which writes the global defaults in the same call (§4.2).
- The chip menu changes it for one session through the existing `acpSetSessionProviderModel`.
- The ModelsBottomBar auto-sync (`:214-246`) that follows the engine's served model for `omlx` sessions stays as it is. It does not apply to `node:` or `strategy:` sessions, because their served node is the router's lease.

**Cloud.** The router's `CLOUD_REGISTRY` stays in the goose crate; `cloud_registry_name` becomes `pub`, and a test in `cloud.rs`'s test module asserts that every one of the 14 `CLOUD_DEFS` rows maps through it to its own `registry` (D7). The engine's table is not moved (it is engine-adjacent, and moving it buys nothing a parity test does not).

### 7.2 Swarm builds

**Tier A: no engine change.** A pure one-door function, `project(swarm_block, strategy, mlx_engine) -> Result<swarm_block, Vec<Reason>>`, lives in `crates/goose/src/nodes/project.rs` (S0), with `build_eligibility` as its refusal half. What a build can reach decides what it accepts:

- **MLX entries: only the local single on this Mac's engine**, whose model is the engine's configured `mlx_engine.model_id` (the only model the build's own `SidecarEngine` mounts, under the device's alias). A split node is refused ("a swarm build reaches LeanZero MLX only through this Mac's single engine; 27B · both Macs is a split"), a remote-single node is refused, and a local single of another model is refused ("swarm builds on this Mac's engine run <configured model>; choose it in Run it first").
- **Planning's 1st** must be such a local single. A cloud Planning node is refused ("the engine replaces a cloud planner with a model LM Studio has loaded"), because the planner keep (`swarm.rs` ~27005) overwrites a cloud `planner_model` whenever LM Studio has anything resident, and Tier A cannot see LM Studio when it projects.
- **Build's chain** becomes the devices: each entry a device with `enabled: true` and `speed_weight` = the entry weight (under `share`; otherwise the 1st gets the maximum weight and the others weight 1). Other **sidecar and cloud** devices get `enabled: false`, which `merge_sidecar_devices` and the cloud merge honour.
- **LM Studio residents always join.** `reconcile_pool_with_fleet` rebuilds the local pool from `lms ps` with `enabled: true`; `enabled: false` does nothing to them. The strategy's "For swarm builds" note and the run header say so: "LM Studio models loaded on your fleet also join this build".
- Every other field of the block is kept byte-for-byte.

The `swarm-build:strategy:<id>` spawn (the goose crate's `providers/swarm.rs`, `:717`) sets `SWARM=<projected block JSON>` **on the child only**. That spawn is the desktop's only door to `goose swarm run` outside the Benchmark view. The Benchmark view spawns `run_build.py` (`ui/desktop/src/main.ts:3978`), and Agent Work spawns `goose swarm agent run` (`main.ts:6967`); neither goes through `project`. The global config is never written, so **the Benchmark view and `bench_dispatch.mjs` see the untouched block. The control arm stays the control arm.**

Tier A is honest about what it cannot express. The run header and the strategy's "For swarm builds" note print it:
- Testing, Frontend and Backend "take effect when the engine learns roles; this build uses Build for every task".
- Planning's 2nd entry is not used ("the engine's own planner fallback applies").
- LM Studio residents join.

Proof without a run: `project(block, golden_identity_strategy) == block`, byte-identical, as a unit test. Under `SWARM=`, the spawned run's `levers_resolved` echoes the **projected** block (the env override is what the run reads, which is the point) while the operator's global block is diffed unchanged, and the first `pool_resolved` names the projected devices plus any LM Studio residents. The CLI has no dry-run flag, so a live check reads that first event and stops the run.

**Tier B (S9): an engine change. It goes through swarm-surgeon and scheduler-surgeon, and is gated.**

The changes:
- **Role-aware dispatch.** `pick_device` filters devices by the task's role chain, with the chain's when-rule. The task role comes from the phase (planning, testing) or from synthesis's declaration on the task (frontend, backend, §6.2).
- The Planning chain replaces the `planner_model` pin and the name heuristic D6.
- **Phase-boundary loads.** Between phases, the engine asks goosed's loader to make the next phase's nodes servable, and it swaps only at those boundaries. The build's own `SidecarEngine` then stops mounting on its own when a goosed loader is present (the second-loader race, §2.2).
- The build reaches the split and remote singles through a goosed-served relay, so Tier B is not bounded by `mlx_engine.port`.

Gates this must respect:
- Gate 5: no seconds anywhere; waits are loader events.
- Gate 10: weights are user ratios, roles are plan facts, and no new numeric const.
- Gate 1: an unserved role is a loud `role_unserved{role, tried:[node: reason]}` event that tick.py prints, never "any device".
- Gate 6: roles never add tasks; they only route.
- Gate 9: the step's value is measured.

**Measurement it needs (the landing condition).** Each arm is run from the Benchmark view (gate 3):
1. **Identity arm.** Strategy "Golden", whose projection equals the golden pool. Its `pool_resolved` and `levers_resolved` must equal the r6h golden's, and its score must be at or above 0.4616.
2. **Role arm.** Planning on the 27B split, Build on per-Mac singles. Graded per phase on wall minutes and score against 0.4616.

It lands only with gate 8's trace. Status for the ledger row: `SCHEDULED waits on: S5 loader LANDED + S8 LANDED + the identity-arm sb-7 run`.

### 7.3 The engine glance and the tray

The glance stays **one engine view** (it answers "what is the engine doing"). S7 adds only the node name to it. Because one MLX way serves this Mac's goose at a time, the existing single view already covers every state v1 can reach; the glance follows `chatServedBy` as today.

A multi-way glance (one row per serving way) is deliberately not added until S10 makes two concurrent ways possible.

---

## 8. Screens

Common rules for every screen:
- Studio primitives only.
- No native `<select>`, `alert`, `confirm` or `prompt`.
- Chips are solid fills (or solid 2px outlines) with ink that passes 4.5:1 in both themes, from the three families of §4.6 (checked by the harness).
- No left accent rails. Emphasis comes from solid chips, weight and full borders.
- Every string goes through `defineMessages`, with key prefixes `nodes.*`, `strategies.*` and `mlxSetup.*`.

The numbers in the wireframes are illustrative, except the 27B split figures, which were read from the 3.0.60 and 3.0.61 walk. Every real figure comes from the derivations named in §4.3.

Widths:
- **460** (the app's narrow window): one column. Tables become stacked cards.
- **1000:** two columns.
- **Full (≥ 1400):** three columns of cards. The editor goes side by side with the "On your Macs" panel.

Both themes use the same hues. In dark mode, chip fills step to the 400-level of each hue with dark ink wherever white ink would fail contrast.

### 8.1 Left nav (1000, light)

```
┌────────────────────────────┐
│ ▣ Goose Swarm              │
│ ⌬ Nodes          [Loading] │  ← chip only while the glance shows a load or a failure
│ ⏻ MCPs                     │
│ ⚡ Skills                   │
│ ◍ Memories                 │
│ ✕ Providers                │
│ ACTIVE NOW (1) …           │
│ …                          │
│ [engine glance dock card]  │
│ ⚙ Settings                 │
└────────────────────────────┘
```

Strings:
- `navigation.itemNodes` "Nodes"
- `nodes.navLoading` "Loading"
- `nodes.navFailed` "Failed"

### 8.2 Nodes page, Nodes tab

**At 1000, with two Macs, a split serving, and one cloud node:**

```
Nodes                                                          [ Nodes | Strategies ]   [+ New node]
A node is a model you can hand work to, on your Macs or in the cloud. Chats start on a node or a
strategy; swarm builds use a strategy or your swarm pool.

New chats start on:  [● Everyday (strategy) ▾]            Swarm builds use:  [Your swarm pool ▾]

ON YOUR MACS · LeanZero MLX                                      Manage Macs and models →
┌────────────────────────────────────────────┐  ┌────────────────────────────────────────────┐
│ [■ MLX] [□ Split · 2 Macs]      [Serving]  │  │ [■ MLX] [□ This Mac]        [Not loaded]  │
│ 27B Atlassian · both Macs                  │  │ Flash · this Mac   [Another way is running]│
│ Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx│  │ rapid-mlx/Qwen3.8-Flash-Next-4bit         │
│ Writing · 11.2 tok/s · Chat · Kickoff notes│  │ Starts in about 48 s · median of 3 loads  │
│ ─────────────────────────────────────────  │  │ ─────────────────────────────────────────  │
│ 10.4–11.6  tok/s writing  81–227 reading   │  │ 38–44  tok/s writing   (measured)         │
│ 262K context                               │  │ 131K context                              │
│ Mihai Macbook   38.6 of 61.8 GB  ██████░░░ │  │ Mihai Macbook  needs 18.2 of 61.8 GB  ██░ │
│ Work's Studio   38.6 of 66.2 GB  █████░░░░ │  │                                           │
│ Used by [Chat 1st · Everyday] [Planning 1st│  │ Used by [Chat 1st · Quick]                │
│ · Everyday] [Build 1st · Everyday]         │  │ Starting it stops 27B · both Macs          │
│ [Stop]  [Edit]  [⋯]                         │  │ [Start]  [Edit]  [⋯]                        │
└────────────────────────────────────────────┘  └────────────────────────────────────────────┘

IN THE CLOUD                                                     Manage cloud providers →
┌────────────────────────────────────────────┐
│ [■ Cloud] [OpenRouter]            [Ready]  │
│ Claude Sonnet · OpenRouter                 │
│ <the provider's model id the user picked>  │   ← never a default
│ Always available · billed by OpenRouter    │
│ Used by [Chat 2nd · Everyday] [Build 2nd · │
│ Everyday]                                  │
│ [Edit]  [⋯]                                 │
└────────────────────────────────────────────┘

YOUR SWARM POOL · used by swarm builds while "Swarm builds use" is "Your swarm pool"      [▾]
(today's pool table, unchanged: Node · Provider · Model · Share · Remove, and Add node)
```

**Card anatomy:**

1. **Chip row.** The kind chip (solid ink with its icon: MLX, Cloud or Endpoint), the where chip ("This Mac", "<Mac name>", or "Split · N Macs", an ink outline with the Mac icon), and the state chip on the right in its state colour.
2. **Name**, bold.
3. **Model id**, mono. For a pool node: the device's model, with "from your swarm pool" and an "Edit in your swarm pool" link instead of an Edit of the model.
4. **Live or next line.**
5. **Figures row:** `engineFigures` / `measuredOf`, with "(measured)" or "(estimated)" exactly as Run it labels them.
6. **Memory**, one bar per Mac. The bar is a solid fill against its budget, with the number in text.
7. **Used by:** role chips in role hues. Each chip links to its strategy.
8. **The displacement line** (when starting it would stop something; with one way at a time, that is whenever another way serves).
9. **Actions.**

The `⋯` menu is a Radix dropdown: Keep loaded (checkbox), Duplicate, Show in Run it, Remove.

**States of this page:**

- **Empty (no nodes, Link signed out):** an EmptyState reading "No nodes yet". Body: "Connect your Macs and run a model, or add a cloud model." Buttons: [Set up your Macs] (to `?tab=mlx&mlx=macs`) and [+ New node].
- **One Mac:** the where chip reads "This Mac". Split ways do not appear in New node. Under the MLX group: "Add another Mac to run models too big for this one →" (to My Macs).
- **Two Macs, split running:** as drawn above. The displacement line appears on every other MLX card.
- **Swap in progress:** the target card is `loading` ("Loading · 12.4 of 31.0 GB"). The source card is `serving` with a solid amber strip at the top: "Stopping for Flash · this Mac after this answer". A strip is a full-width top band, not a left rail.
- **A build holds the engine:** every MLX card that is not the build's way shows `heldByBuild`.
- **Node unavailable:** a red "Can't run" chip with the reason line. The card stays at full strength and is never greyed.
- **LM Studio devices present:** a one-line notice at the foot of the page (§4.5).

**At 460:** one column. The "New chats start on / Swarm builds use" selectors stack. Figures wrap to two rows. The memory bars keep full width.

**Strings (en):**

| Key | Text |
|---|---|
| `nodes.title` | "Nodes" |
| `nodes.subtitle` | "A node is a model you can hand work to, on your Macs or in the cloud. Chats start on a node or a strategy; swarm builds use a strategy or your swarm pool." |
| `nodes.tabNodes` / `nodes.tabStrategies` | "Nodes" / "Strategies" |
| `nodes.new` | "New node" |
| `nodes.forNewChats` | "New chats start on:" |
| `nodes.forBuilds` | "Swarm builds use:" |
| `nodes.auto` | "Any node (Auto)" |
| `nodes.pool` | "Your swarm pool" |
| `nodes.poolSection` | "Your swarm pool · used by swarm builds while \"Swarm builds use\" is \"Your swarm pool\"" |
| `nodes.fromPool` | "from your swarm pool" |
| `nodes.editInPool` | "Edit in your swarm pool" |
| `nodes.leftPool` | "No longer in your swarm pool" |
| `nodes.groupMacs` | "On your Macs · LeanZero MLX" |
| `nodes.groupCloud` | "In the cloud" |
| `nodes.manageMacs` | "Manage Macs and models" |
| `nodes.manageCloud` | "Manage cloud providers" |
| `nodes.kindMlx` / `kindCloud` / `kindEndpoint` | "MLX" / "Cloud" / "Endpoint" |
| `nodes.whereThisMac` | "This Mac" |
| `nodes.whereSplit` | "Split · {count} Macs" |
| `nodes.stateServing` | "Serving" |
| `nodes.stateLoading` | "Loading" |
| `nodes.stateNotLoaded` | "Not loaded" |
| `nodes.stateOtherWay` | "Another way is running" |
| `nodes.stateNeedsStep` | "Needs a step" |
| `nodes.stateCantRun` | "Can't run" |
| `nodes.stateHeldByBuild` | "Held by a build" |
| `nodes.stateFollows` | "Follows this Mac" |
| `nodes.stateReady` | "Ready" |
| `nodes.stateKeyMissing` | "Key missing" |
| `nodes.stateLastCallFailed` | "Last call failed" |
| `nodes.startsIn` | "Starts in about {duration} · median of {count, plural, one {# load} other {# loads}}" |
| `nodes.firstStart` | "First start not measured yet" |
| `nodes.displaces` | "Starting it stops {names}" |
| `nodes.stoppingFor` | "Stopping for {node} after this answer" |
| `nodes.heldByBuildLine` | "A swarm build is using {node}; it frees when the build ends" |
| `nodes.follows` | "Serves whatever this Mac's engine runs: {model}" |
| `nodes.pinWay` | "Pin a way" |
| `nodes.cloudAlways` | "Always available · billed by {provider}" |
| `nodes.usedBy` | "Used by" |
| `nodes.roleIn` | "{role} {rank} · {strategy}" |
| `nodes.memNeed` | "needs {need} of {budget} GB" |
| `nodes.memPeak` | "{peak} of {budget} GB" |
| `nodes.actionStart` / `actionStop` / `actionEdit` | "Start" / "Stop" / "Edit" |
| `nodes.keepLoaded` | "Keep loaded" |
| `nodes.duplicate` | "Duplicate" |
| `nodes.showInRunIt` | "Show in Run it" |
| `nodes.remove` | "Remove" |
| `nodes.removeAlsoFromStrategies` | "Also remove it from {strategies}" (a box on open when a strategy names the node; Q-259) |
| `nodes.removeNewChatsAuto` | "Start new chats on Any node (Auto) instead" (a box on open when new chats start on the node) |
| `nodes.removeLiveChats` | "Remove it anyway: {count, plural, one {# chat is} other {# chats are}} set to this node" (a box the moment the engine counts them; its description: "Its/Their next message will say the node was removed and ask you to pick another.") |
| `nodes.removeBlocked` | "{count, plural, one {Tick the box above to remove it} other {Tick the # boxes above to remove it}}" (beside the disabled Remove) |
| `nodes.emptyTitle` | "No nodes yet" |
| `nodes.emptyBody` | "Connect your Macs and run a model, or add a cloud model." |
| `nodes.setUpMacs` | "Set up your Macs" |
| `nodes.oneMacHint` | "Add another Mac to run models too big for this one" |
| `nodes.lmStudioHidden` | "{count, plural, one {# LM Studio device} other {# LM Studio devices}} in your swarm config {count, plural, one {is} other {are}} not shown here" |

### 8.3 New node dialog (OverlayDialog, a stepper)

```
New node                                                        1 Kind › 2 Model › 3 Way › 4 Name
────────────────────────────────────────────────────────────────────────────────────────────────
Step 1  Where should it run?
  ┌──────────────────────────┐ ┌──────────────────────────┐ ┌──────────────────────────┐
  │ [MLX] On your Macs       │ │ [Cloud] A cloud model    │ │ [Endpoint] Your endpoint │
  │ LeanZero MLX engine,     │ │ OpenRouter, Claude,      │ │ Any OpenAI-compatible    │
  │ one Mac or split         │ │ Gemini… (2 set up)       │ │ server you added         │
  └──────────────────────────┘ └──────────────────────────┘ └──────────────────────────┘

Step 2 (MLX)  Which model?        [Filter models…]           (custom Combobox; badges from the planner)
  Qwen3.8-27B-Atlassian-Q8-mlx      31 GB   [Needs both Macs]   on: Mihai Macbook, Work's Mac Studio
  Qwen3.8-Flash-Next-4bit           18 GB   [Fits this Mac]     on: both
  Get more models →  (Models tab)

Step 3 (MLX)  How should it run?  Goal: [ Chat | Long documents | Many requests ]
  ● Best  Split across both Macs · pipeline      11.2 tok/s writing (measured)   262K context
    Mihai Macbook 38.6 of 61.8 GB · Work's Mac Studio 38.6 of 66.2 GB
  ○ Work's Mac Studio alone                       Does not fit: short 1.6 GB
  ○ Both Macs · tensor                           Slower: 7.9 vs 11.2 tok/s · faster reading long prompts
  (the extracted PlacementCandidates list, identical to Run it's; unavailable ways are selectable
   only when their outcome is not DoesNotFit, and carry their "needs" line)

Step 4  Name it
  Name  [27B Atlassian · both Macs            ]
  [ ] Keep loaded: never stop it for another node
                                                           [Back]  [Create node]  [Create and start]
```

The first draft's "Add to your swarm pool" checkbox is removed: it would have been a second writer of `swarm`. The pool is edited only in its own section (§5.3).

The cloud path runs Step 2 (Cloud) → the provider, from the **configured** providers only, plus the button "Set up another provider →" → the model, from the provider's own model list (existing `acpListProviderDetails` / model fetch) → Name. It is the New node dialog's own step, written through `nodes/write`; `AddNodeDialog` stays the pool's own Add.

States:
- No models: "No models on your Macs yet. Get one in Models →".
- No configured providers: the Cloud tile says "None set up · Set up".
- One Mac: Step 3 shows only single ways.
- A plan that failed to measure: the plan's `notes` are shown verbatim, and there is no guessed row.
- A plan read that failed: the error's words, and a Retry.
- At 460: the stepper collapses to "Step 2 of 4 · Model", and the tiles stack.

Strings, all under `nodes.new*`: "New node", "Where should it run?", "On your Macs", "LeanZero MLX engine, one Mac or split", "A cloud model", "An endpoint", "Which model?", "How should it run?", "Name it", "Keep loaded: never stop it for another node", "Create node", "Create and start", "No models on your Macs yet.", "Get one in Models", "None set up", "Set up another provider". The planner's candidate strings are reused from `placementCard.*`, not copied.

### 8.4 Strategies tab and the strategy editor

**List at 1000.** "Everyday" uses one MLX way for every MLX role, so it never swaps. "Quick" is the example of a strategy that does.

```
Strategies                                                                  [+ New strategy]
A strategy says which node does what: its roles, the order to try nodes in, and when to use the next one.

┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ Everyday                                 [New chats start here]                              │
│ "Big model thinks and builds; the cloud takes the overflow."   ← the owner's own note         │
│ [Chat] 27B · both Macs → Claude Sonnet                                                       │
│ [Planning] 27B · both Macs                                                                   │
│ [Build] 27B · both Macs ⇆ Claude Sonnet  (share 2:1)                                         │
│ [Testing] same as Build   [Frontend] same as Build   [Backend] same as Build                 │
│ On your Macs: one way (27B · both Macs), no swaps                                            │
│ Swarm builds: can't use it — 27B · both Macs is a split; builds reach LeanZero MLX only      │
│ through this Mac's single engine                                                              │
│ [Edit] [Use for new chats] [⋯ Duplicate · Remove]                                            │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ Quick                                                                                        │
│ [Chat] Flash · this Mac → Claude Sonnet     [Build] 27B · both Macs                          │
│ On your Macs: swaps (Flash · this Mac ⇄ 27B · both Macs · about 48 s / 1 min 40 s, measured) │
│ ⚠ Each delegate call swaps twice: to 27B · both Macs and back to Flash · this Mac            │
│ [Edit] [Use for new chats] [⋯]                                                               │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Editor (full width; at 1000 it is a large OverlayDialog; at 460 a full-screen sheet):**

```
Strategy  [Everyday                        ]   Note (your words)  [Big model thinks and builds; …]

ROLE        WHAT IT IS                                   NODES IN ORDER                           WHEN TO USE THE NEXT        IF NOT LOADED
[Chat]      Your turns in a chat…                        1 [27B · both Macs ▾]  2 [Claude Sonnet ▾]  (•) can't run ( ) busy ( ) share   (•) Load it and wait ( ) Use the next meanwhile
            ↳ "Chat runs on 27B · both Macs. If it can't run, on Claude Sonnet · OpenRouter. If 27B isn't loaded, it loads and your turn waits (about 1 min 40 s)."
[Planning]  Reading the request, asking questions…       1 [27B · both Macs ▾]  + add               …
            Used by swarm builds.
[Build]     Writing the code for each task               1 [27B · both Macs ▾] ×2   2 [Claude Sonnet ▾] ×1   ( ) ( ) (•) share
            ↳ "Build is shared: 27B · both Macs 2 parts, Claude Sonnet 1 part."
[Testing]   Running and checking the result…             Same as Build   [Set its own nodes]
[Frontend]  Build tasks that write the user interface…   Same as Build   [Set its own nodes]
[Backend]   Build tasks that write the server side…      Same as Build   [Set its own nodes]

ON YOUR MACS  (what this strategy asks of your Macs)
  One way at a time serves your chats: 27B · both Macs (Mihai Macbook shard 1/2, Work's Mac Studio shard 2/2)
  Cloud               Claude Sonnet · OpenRouter                               → always available
  Swarm builds        can't use this strategy: 27B · both Macs is a split
                                                                                  [Cancel]  [Save strategy]
```

Every node picker is the custom `Combobox`, and each option shows its state chip. The weight "×2" is a `WeightStepper`, shown only under `share`. Role chips use the role hues. At 460 each role becomes a stacked card (role chip, words, chain, when, if-not-loaded, then the sentence).

The "On your Macs" panel is computed in `components/nodes/strategyFit.ts` from the node definitions, the plan fit (`fit.nodes`) and the load history. It is pure and tested. **v1 rule: any two MLX entries in the strategy whose ways differ conflict, whichever Macs they use**, because one MLX way serves this Mac's goose at a time. Each conflicting pair gets a "swaps" row with both measured load times, or "not measured yet". When Chat's 1st and Build's 1st are different MLX ways and Build's `ifNotLoaded` is `load`, the panel adds the delegate warning (`strategies.delegateSwaps`). The swarm-builds line comes from `nodes/buildEligibility`.

Editor states:
- A new strategy has one Chat row set to the current `forNewChats` node; Build is unset and therefore "Same as Chat", so a new strategy never swaps by default.
- A role whose chain names a `cantRun` node shows that chip inline, and saving is allowed.
- A chain naming a removed node cannot happen; validation refuses it.
- A `share` or `overflow` chain with two different MLX ways is refused with §6.3's sentence.
- "Use for swarm builds" is shown only when `nodes/buildEligibility` answers Ok; otherwise the reasons are shown in its place.

Strings (`strategies.*`):

| Key | Text |
|---|---|
| title | "Strategies" |
| subtitle | "A strategy says which node does what: its roles, the order to try nodes in, and when to use the next one." |
| new | "New strategy" |
| name | "Strategy" |
| note | "Note (your words)" |
| colRole | "Role" |
| colWhat | "What it is" |
| colNodes | "Nodes in order" |
| colWhen | "When to use the next" |
| colIfNotLoaded | "If not loaded" |
| whenFailover | "can't run" |
| whenOverflow | "busy" |
| whenShare | "share" |
| loadWait | "Load it and wait" |
| useNext | "Use the next meanwhile" |
| colIfServingOther | "If its Mac is serving another node" (Q-428; values "Use the next node" · "Wait" · "Take it over", each with its sentence under it) |
| sameAs | "Same as {role}" |
| setOwn | "Set its own nodes" |
| usedByBuilds | "Used by swarm builds." |
| addNode | "Add a node" |
| onYourMacs | "On your Macs" |
| oneWay | "One way at a time serves your chats: {node}" |
| noSwaps | "one way ({node}), no swaps" |
| fits | "fits" |
| swaps | "swaps: {a} / {b}" |
| swapWarn | "{roleA} and {roleB} need different ways. Each switch between them stops one and loads the other." |
| delegateSwaps | "Each delegate call swaps twice: to {build} and back to {chat}" |
| shareTwoWays | "Sharing between {a} and {b} would stop one to load the other on every turn" |
| cloudAlways | "always available" |
| useForChats | "Use for new chats" |
| useForBuilds | "Use for swarm builds" |
| badgeChats | "New chats start here" |
| badgeBuilds | "Swarm builds use this" |
| buildsCantUse | "Swarm builds can't use this strategy: {reasons}" |
| buildsSplit | "{node} is a split; swarm builds reach LeanZero MLX only through this Mac's single engine" |
| buildsRemote | "{node} runs on {mac}; swarm builds reach LeanZero MLX only through this Mac's single engine" |
| buildsOtherModel | "Swarm builds on this Mac's engine run {model}; choose {node}'s model in Run it first" |
| buildsCloudPlanner | "Planning on {node}: the engine replaces a cloud planner with a model LM Studio has loaded" |
| buildsLmStudioJoin | "LM Studio models loaded on your fleet also join this build" |
| save | "Save strategy" |

The six role names and their six description lines from §6.1 are also in this namespace. The sentence builder `sentenceFor(role, entry, facts)` (in `strategySentence.ts`) composes its sentences from ICU templates, one per when-rule × ifNotLoaded, never by string concatenation.

### 8.5 The model chip and session start

**Chip label** (from the served-turn record, `nodes/servedLast`, read at the end of each turn; before a session's first turn, from its model id and `nodes/residency`):
- `node:` sessions: "<node name> · <state>".
- `strategy:` sessions: "<strategy> · <node that served the last turn>".
- `swarm` sessions: "Any node · <node>".

The phase dot keeps today's rule.

**Chip menu** (Radix dropdown, as today). Its states come from `nodes/residency` and `nodes/loadHistory`, read when the menu opens (an event), so it needs no Mac or plan data and works on every screen:

```
Run this chat on
  STRATEGIES
  ✓ Everyday            Chat → 27B · both Macs        [Serving]
    Quick               Chat → Flash · this Mac       [Not loaded]
  NODES
    27B · both Macs                                   [Serving]
    Flash · this Mac    starts in about 48 s          [Not loaded]
    Claude Sonnet · OpenRouter                        [Ready]
    Any node (Auto)
  ─────────
  Manage nodes…                         LeanZero MLX engine…
  Other models and providers…           (opens SwitchModelModal: cloud and endpoints)
```

Picking an entry sets the **session's** model (`acpSetSessionProviderModel`). The next turn shows the loader line if a load is needed.

A **new chat** opens with `forNewChats`, which defaults to "Any node (Auto)". The chip always names what the chat runs on, so the choice is always visible and one click away; a forced pick on every new chat would add a step the owner did not ask for (§11.7). When `forNewChats` is `auto` and the pool has no servable node, the existing NoNodeNotice shows, with a new first action: [Choose a node].

**Turn line** (under the assistant message, only when something is worth saying), from the served-turn record of the turn just served:
- "on Claude Sonnet · 2nd for Chat: 27B · both Macs can't run (Work's Mac Studio is not connected)"
- "on Claude Sonnet · 2nd for Chat: 27B · both Macs failed to load: <words>"
- "on 27B · both Macs · loaded for this turn in 1 min 38 s"

After a reload, only the session's last record is kept (it persists in the session); earlier turn lines are not reconstructed, and nothing pretends they were.

### 8.6 LeanZero MLX with the setup strip (Providers › LeanZero MLX)

```
Providers                                                       [ LeanZero MLX | Cloud Providers ]
Where your models run: the LeanZero MLX engine on your Macs, and the cloud providers you've signed in to.
Turn them into nodes under Nodes.

 (1 Your Macs · 2 connected ✓) ─▶ (2 Models · 2 on your Macs ✓) ─▶ (3 Run it · 27B split running ✓) ─▶ (4 Nodes · 3 nodes ✓)
 Engine   My Macs   Models 2   Sampling                [Split across 2 Macs · over Thunderbolt]   Powered by Rapid-MLX
```

The four steps are solid chips:
- done: solid green, white ink, with a ✓
- the next step to take: solid blue, white ink, "Next"
- later steps: solid slate, white ink

Each chip is a button to its tab, or to `#/nodes`. When all four are done the strip stays as compact navigation. At 460 it collapses to "Step 3 of 4 · Run it › Next". Example states:
- Signed out: step 1 is "Connect your other Macs" in blue. With only one Mac it still counts as done, reading "1 Mac".
- No model: step 2 is blue.
- Nothing running: step 3 is "Run a model" in blue.
- No node for the running way: step 4 is "Save as a node" in blue.

My Macs tab: today's MyMacs component. In S7 each Mac card gains "Nodes on this Mac" chips (links), and when there is only one Mac, an "Add another Mac" card: "Sign in to LeanZero Link with the same account on your other Mac. It appears here, and you can run models too big for one Mac across both."

Run it (S2): each way row gains "Save as node". After a Run it start succeeds, a single line appears: "Save this way as a node so chats and builds can pick it" [Save as node].

Strings (`mlxSetup.*`):

| Key | Text |
|---|---|
| step1 | "Your Macs" |
| step1Done | "{count, plural, one {# Mac} other {# connected}}" |
| step1Next | "Connect your other Macs" |
| step2 | "Models" |
| step2Done | "{count} on your Macs" |
| step2Next | "Get a model" |
| step3 | "Run it" |
| step3Done | "{model} running" |
| step3Next | "Run a model" |
| step4 | "Nodes" |
| step4Done | "{count} nodes" |
| step4Next | "Save as a node" |
| next | "Next" |
| compact | "Step {n} of 4 · {label}" |

Also: `providers.title` "Providers", `providers.subtitle` (text above), `providers.tabMlx` "LeanZero MLX", and `mlxEngine.tabEngine` / `tabMacs` / `tabModels` / `tabSampling`.

### 8.7 Load, swap and fallback strings (composer, cards, glance)

| Key | Text | Where |
|---|---|---|
| `nodes.turnLoading` | "Loading {node} for this chat: {phase}" | composer readiness line |
| `nodes.turnWaiting` | "Waiting for {way} to finish {count, plural, one {# reply} other {# replies}}, then loading {node} ({duration})" | composer |
| `nodes.turnFirstLoad` | "First load of {node}, not measured yet" | composer |
| `nodes.displacedNotice` | "{node} was stopped for {other} in chat \"{chat}\". Your next message loads it back ({duration})." | displaced chat, above the composer. Actions: [Keep {node} loaded] [Use {next} instead] |
| `nodes.displacedFailed` | "{node} was stopped for {other}, which failed to load: {words}. Your next message loads {node} back." | displaced chat |
| `nodes.refusedKept` | "Can't load {node}: {kept} is kept loaded on {mac}." | composer |
| `nodes.refusedBuild` | "Can't load {node}: a swarm build is using {way}. It frees when the build ends." | composer |
| `nodes.refusedFit` | "Can't load {node} on {mac}: {verdict}" | composer; action [Make room] (the existing flow) |
| `nodes.loadFailed` | "{node} failed to load: {words}" | composer (no next entry) and turn line (failover) |
| `nodes.fellBack` | "{role} is on {node} ({rank}): {primary} can't run: {reason}" | turn line. Action [Retry {primary}] |
| `nodes.needsStepRefusal` | "{node} needs a step first: {needs}" | NoNodeNotice. Action [Open Run it] |
| `nodes.removedNode` | "{node} was removed. Pick another node from the chip." | composer of a session set to a removed node |
| `nodes.loaderAbsent` | "Loading nodes is not available here; start {node} in Run it." | NoNodeNotice before S5, and in processes with no loader |
| `nodes.glanceNode` | "Node · {name}" | glance card, under the mode line |

---

## 9. Slices

Every slice runs the full gate in `goose-feature-dev`: fmt, clippy `-D warnings`, cargo test for the crate, tsc, eslint, `i18n:check`, vitest.

**Rules that hold across all slices:**
- **Every file has exactly one owning slice.** No file appears in two Owns lists below. S1 is in flight and owns its files until it merges; each S1 file a later slice needs is handed to exactly one later slice, named in that slice's Owns list ("handed from S1"). No slice other than S1 edits a file S1 owns before S1 merges.
- The contract is closed in S0: every ACP DTO (`custom_requests.rs`), every dispatch function (`custom_dispatch.rs`) and every TS client function (`ui/desktop/src/acp/nodes.ts`) for this whole design is written in S0. Later slices fill module files they own, reached through S0's seam, and never reopen those three files.
- `ui/desktop/src/i18n/messages/en.json` is regenerated, never hand-merged. After merging a slice, run `pnpm i18n:extract` on main and commit the result.
- A slice never edits `crates/goose-cli/src/commands/swarm.rs` or `crates/goose-swarm/*`, except S9. `crates/goose-cli/src/commands/swarm_engine.rs` is S8's alone, and `crates/goose-cli/src/commands/swarm/cloud.rs` is S3's alone (test module only).
- "Depends on" names **data or compile** dependencies. A slice may be cut in a worktree before its dependencies merge when only its live proof needs them; the table says which.

### 9.0 The slice table

| Slice | Owns | Depends on | Confidence |
|---|---|---|---|
| S0 | the `nodes` module (types, store, adopt, validate, resolve, project, eligibility, seam, served, residency, ACP bodies, fixture); `crates/goose/src/lib.rs` (the `pub mod nodes;` line); `acp/server/custom_dispatch.rs`; `goose-sdk-types/src/custom_requests.rs`; `goose-sidecar/src/placement/loads.rs` + `placement/mod.rs`; `ui/desktop/src/acp/nodes.ts`; `components/nodes/model.ts`, `resolve.ts` + tests | — | high |
| S1 | as dispatched (routes, nav items, Providers shell, MLX tab strip, NodesView hosting SwarmNodesSection, MlxSetupStrip, SwarmSettingsSection deleted); **not** `main.ts` | — (in flight) | high |
| S2 | `components/nodes/` NodeCard, NodesTab, nodeGlance (+ fixtures, tests), NewNodeDialog, hues.ts; `leanzero-swarm/PlacementCard.tsx`, new `PlacementCandidates.tsx`, new `usePlacementPlans.ts`; `CloudProvidersSection.tsx`; `settings/swarm/golden.ts` (D2 doc comment) | S0 | medium-high |
| S3 | `providers/swarm_router.rs`, `providers/swarm.rs`, `agents/platform_extensions/summon.rs`, `goose-cli/src/commands/swarm/cloud.rs` (test module only) | S0 | medium-high |
| S4 | `ModelsBottomBar.tsx`, new `bottom_bar/NodesChipMenu.tsx`, `SwitchModelModal.tsx`, `chatServedBy/chatServedBy.ts`, `useChatServedBy.ts`, `turnLine.ts`, `noNodeNotice/NoNodeNotice.tsx` | S0; S3 for live proof (the served record) | medium |
| S5 | `acp/server.rs` (mod line, loader install, `on_prompt` reply guard); new `acp/server/nodes_loader.rs` + `nodes_loader/{switch,split_config,holds}.rs` + their two fixtures; `acp/server/mlx_remote_single.rs`, `acp/server/mlx_distributed.rs` (core extraction, ready-path load rows); `goose-sidecar/src/holders.rs` + `goose-sidecar/src/lib.rs`; `goose-sidecar/src/engine.rs` (mount-ready load row); `harness/r5.mjs`; new `leanzero-swarm/placementSwitch.fixture.test.ts`, `splitConfig.fixture.test.ts` | S0; S3 for live proof (leases and delegates feed the holds) | **low-medium** |
| S6 | `components/nodes/NodesView.tsx` (handed from S1), StrategiesTab, StrategyCard, StrategyEditor, strategyFit, strategySentence, UseSelectors | S0, S2, S1 merged | medium |
| S7 | `ui/desktop/src/main.ts` (the glance's node name), `utils/engineGlance.ts`, `components/engineGlance/glanceStore.ts`, `EngineGlanceCard.tsx`, `Layout/NavigationPanel.tsx` (the nav chip; handed from S1 if S1's gating edit touches it), `leanzero-swarm/MyMacs.tsx` + new `MyMacs.test.tsx` | S0, S1 merged | medium-high |
| S8 | `goose-cli/src/commands/swarm_engine.rs` | S5 (holders.rs) | medium |
| S9 | `goose-cli/src/commands/swarm.rs`, `goose-swarm/src/scheduler.rs` (and the `dag.rs` task-role field) | S3, S5, S8 + the measured run | **low** |
| S10 | to be cut when unblocked: the route record, `sidecar_routed_away`, Run it's switch, `MlxEngineManager` | the interference measurement | **low** |
| S11 | `App.tsx` (the harness route; handed from S1), `components/nodes/__states__/states.ts`, new `NodesHarness.tsx`, `harness/nodes-states.mjs`, `harness/nodes-livecheck.mjs` | S2, S4, S6, S7, S1 merged | medium-high |

**Parallel plan** (at most three surgeons at once, memory `be-mindful-of-usage`):
- **Now:** S0 (S1 keeps going). Nothing else can start before S0's contract exists.
- **When S0 merges:** S2, S3 and S5 in parallel (file-disjoint; S5's J3/J4 wait for S3). S7 takes the first free slot once S1 has merged.
- **Then:** S4 after S3; S6 after S2 (and S1 merged); S8 after S5.
- **Then:** S11 after S2, S4, S6 and S7.
- **Then:** S9 and S10 on their measurements.

### S0: Contract, store and the pure rules

**Order:** first. **Confidence: HIGH.** Pure types, adoption, validation, resolution and projection, with shared fixtures, plus three thin readers (loads, served, residency). The risks are fixture drift between TS and Rust, which the shared fixture refuses, and `residency` reading the three cross-process records the router already reads (it calls the same readers; it adds none).

**Owns:**
- `crates/goose/src/lib.rs`: the `pub mod nodes;` line
- `crates/goose/src/nodes/mod.rs`: types, read/write of key `nodes`, `adopt` (declined list, name dedupe), validation (§4.2), the `forNewChats` defaults write
- `crates/goose/src/nodes/resolve.rs`: pure chain resolution, `sentence_facts`
- `crates/goose/src/nodes/project.rs`: `project()` and `build_eligibility()` (§7.2)
- `crates/goose/src/nodes/seam.rs`: the `NodeLoader` trait, install point and the named absence
- `crates/goose/src/nodes/served.rs`: the per-session served-turn ring and the persisted last record
- `crates/goose/src/nodes/residency.rs`: per-node residency from engine truth plus `in_progress()`
- `crates/goose/src/nodes/acp.rs`: every handler body
- `crates/goose/src/nodes/nodes.fixture.json`
- `crates/goose/src/acp/server/custom_dispatch.rs`: one dispatch function per `nodes/*` method
- `crates/goose-sdk-types/src/custom_requests.rs`: every DTO of §4.2's table
- `crates/goose-sidecar/src/placement/loads.rs` (the store: row type, `append`, `median_for`, absent → `None`) and its `placement/mod.rs` line
- `ui/desktop/src/acp/nodes.ts`
- `ui/desktop/src/components/nodes/model.ts`, `resolve.ts` and their tests

**Tests:**
- serde round-trip of every fixture case
- adoption of: MLX device → `follows` with `poolDevice`; two MLX devices on one Mac → deduplicated names; cloud device → read-through cloud node; LM Studio → skipped plus a count; a declined id → not re-adopted; idempotence; an unknown Mac kept; `local` accepted as this Mac
- a validation refusal per rule, including removal while `forNewChats` names the node and a `share` chain with two different MLX ways
- `resolve` for each when-rule × {1st servable, busy, can't run, not loaded + load/useNext, load failed} × {chain length 1, 2, 3}, with the TS and Rust suites running the **same** fixture
- `project(block, golden_identity) == block`, byte-identical; Build chain with weights → `speed_weight`s; Planning 1st → `planner_model`; each refusal of §7.2 (split, remote, other model, cloud planner) carries its reason
- `loads.rs`: append, median, absent → `None` (an absent file means no load has been measured, which is true until S5's writers land)
- the seam with nothing installed answers the named refusal
- `nodes/write` of `forNewChats` writes both defaults

**Must not break:** `swarm` is never written; `forNewChats=auto` / `forBuilds=pool` keep today's behaviour; a read never writes config.

### S1: Information architecture move (Q-193, Q-194)

**Order:** in flight, parallel with S0. **Confidence: HIGH.** Unchanged from the first draft **except that it does not edit `main.ts`**: `main.ts:2381` keeps sending `'link'`, `handleSetView` builds `?tab=link`, and S1's redirect lands it on My Macs. Changing the line to `'macs'` would have built `?tab=macs`, which is no route.

**Owns:**
- `ui/desktop/src/hooks/useNavigationItems.ts`
- `ui/desktop/src/App.tsx`: routes and redirects (handed to S11 after merge)
- `ui/desktop/src/utils/navigationUtils.ts`
- `ui/desktop/src/components/leanzero-swarm/LeanZeroSwarmView.tsx`
- `MlxEngineView.tsx`: **tab strip region only**, plus the routed `mlx=` param
- new `ui/desktop/src/components/nodes/NodesView.tsx`, hosting `SwarmNodesSection` unchanged (handed to S6 after merge)
- new `components/leanzero-swarm/MlxSetupStrip.tsx`
- delete `components/settings/swarm/SwarmSettingsSection.tsx` and its imports (keep `golden.ts`, which `SwarmNodesSection` uses)

**Tests:**
- `NavigationPanel.test.tsx`: Nodes first, gated like Providers
- route tests for every row of §5.2, including the two redirects, the `set-view` `'link'` path landing on My Macs, and `ENGINE_ROUTE`
- `LeanZeroSwarmView.test.tsx`: two tabs, i18n title
- `MlxEngineView` tab test: 4 tabs, URL written on click, Back restores
- `MlxSetupStrip` renders each step state
- the existing `SwarmNodesSection.test.tsx` passes unchanged

**CDP live:** click Nodes in the nav, see the pool table. Open `#/leanzero-swarm?tab=link`, land on My Macs. The Link tray's "open" lands on My Macs. Back restores the tab.

**Must not break:** `ComposerReadiness` `ENGINE_ROUTE`, main's `set-view` deep links, the glance's click-to-Engine, and `split-start.mjs` (it drives `#/leanzero-swarm` Run it; update its selector if the tab strip changes).

### S2: Node cards, New node and Run it's Save as node (Q-195, Q-196 R6/R7)

**Order:** after S0. **Confidence: MEDIUM-HIGH.** The derivation joins five stores, and a subtle bug could show "Not loaded" on a serving split when the model-identity rule mismatches (the Q-128 class). The fixture includes that exact case: 27B split served under its HF id against an alias, and a remote single serving while this Mac's engine is idle (`displaced`, never `serving`). Extracting the candidates list and the plans hook from a 1,834-line card must keep PlacementCard's tests green, **unchanged**. The palette's CIEDE2000 check may force one re-pick.

**Owns:**
- `components/nodes/NodeCard.tsx`, `NodesTab.tsx` (the card grid, groups, empty and one-Mac states)
- `components/nodes/nodeGlance.ts` (+ `nodeGlance.fixtures.ts`, tests)
- `components/nodes/NewNodeDialog.tsx` (MLX and cloud paths, written through `nodes/write`)
- `components/nodes/hues.ts` (the three families of §4.6)
- `components/leanzero-swarm/PlacementCard.tsx`: extraction of `PlacementCandidates.tsx` and `usePlacementPlans.ts` (with the goal parameter and the failed-read state, D10), "Save as node" per way, and the post-start line
- `components/leanzero-swarm/PlacementCandidates.tsx`, `usePlacementPlans.ts` (new)
- `CloudProvidersSection.tsx`: the one reworded line
- `components/settings/swarm/golden.ts`: the `supervision` doc comment corrected to what the flag does (D2)

`SwarmNodesSection.tsx`, `NodeModelCell.tsx`, `AddNodeDialog.tsx` and `WeightStepper` are **not** touched: they stay the pool's editor.

**Tests:**
- `nodeGlance` for every §4.3 state, from fixture facts, including `displaced` by a way on another Mac and `heldByBuild`
- `NodeCard` renders each state's chip, line and action, the pool node's read-through model with "Edit in your swarm pool", and the Studio-clean assertions (no `select`, no `border-l-*`, no opacity below 1 on text)
- `NewNodeDialog`: MLX path with one Mac and with two Macs; cloud path with none configured and with one; an unmeasured plan shows its notes; a failed plan read shows its words
- `usePlacementPlans` for each goal, and a failed read is a state, not an empty map
- `PlacementCard.test.tsx` and `MlxEngineView.test.tsx` green, unchanged; Save as node writes one def through `nodes/write`

**CDP live:** S2's cards become reachable when S6 wires NodesView; until then they are proven in the unit tests and, after S11, in the harness. Run it's Save as node is proven live here: save the serving split, and `nodes/read` returns the def with its way.

**Must not break:** one derivation (engineFigures, fit rule), no new poller (the Nodes page mounts `WithMacs` exactly as Providers does), Q-128 identity, Q-154 unique names, Run it's switch unchanged.

### S3: Chat and build routing (Q-196 R11)

**Order:** after S0, parallel with S2 and S5. **Confidence: MEDIUM-HIGH.** The router has a fake-probe test harness (`NodeProbe` trait) that makes every branch testable. The subtle parts are stickiness under `share`, the smallest-window rule, and the served record on every lease kind. The summon change touches every delegate, so it is covered by tests that a non-strategy parent and a non-`swarm` provider are byte-identical. The Tier A spawn rides the proven env override.

**Owns:**
- `crates/goose/src/providers/swarm_router.rs`: parsing `node:`, `strategy:`, `@role` from the model name; candidates from definitions; when-rules; weighted round-robin per conversation; the seam calls (`ensure_serving`, `note_lease`); the served record on every lease; `route_context_window(model)`; Auto's tie-break on the pool's Share with `speed_weight` added to `PoolDevice` (D1); `cloud_registry_name` made `pub`
- `crates/goose/src/providers/swarm.rs`: `get_context_limit` passes the model name; `route_for` maps `swarm-build:strategy:<id>` to Build; that spawn sets `SWARM=<project() output>` on the child only
- `crates/goose/src/agents/platform_extensions/summon.rs`: delegates → `@build` when the provider resolves to `swarm`; `note_child`
- `crates/goose-cli/src/commands/swarm/cloud.rs`: **test module only**, the D7 parity test over all 14 rows

**Tests** (router unit tests on fake probes):
- each when-rule; a chain exhausted → loud error naming each entry; a failed load under failover → the next entry with the failure named
- sticky kept under `share`; `overflow` queues on all when every entry is busy
- a not-loaded node with no loader installed → the named refusal carrying the node
- window = the node's for `node:`, the smallest in the chain for `strategy:`; `swarm` unchanged
- a served record is written for an MLX lease, an LM Studio lease and a cloud lease
- Auto: ties go to the larger Share; with equal or absent Shares the order is today's (`ties_go_to_the_heavier_node` kept for that case); every other existing router test passes
- summon: a strategy parent on `swarm` gives `@build`; `GOOSE_SUBAGENT_MODEL` still wins; `GOOSE_SUBAGENT_PROVIDER=anthropic` leaves the model as today; a non-strategy parent is unchanged
- the D7 parity test fails when a `CLOUD_DEFS` row's registry differs from the router's answer
- a spawn test asserts the child env carries `SWARM`, the parent process env does not, and the global config is unchanged

**CDP/E2E:** J5 (§10.4).

**Must not break:**
- gate 3: the Benchmark view and Agent Work spawns never call `project`
- gate 5: no clock in any wait
- the router's existing queue-on-all semantics and the remote-route precedence
- Q-18 (the context window saved on sessions)
- Q-128 follow rules for `swarm`

### S4: The chip, session start and the turn line (R11)

**Order:** after S0; its live proof after S3. **Confidence: MEDIUM.**
- `chatServedBy` is a large derivation with many consumers (composer, glance, tray title). Adding the served record must not disturb its readiness logic.
- The auto-sync at `ModelsBottomBar.tsx:214-246` must stay out of `node:` and `strategy:` sessions, or it would silently rewrite the session model (the gate 1 class).

**Owns:**
- `ModelsBottomBar.tsx`: label and the menu mount
- new `settings/models/bottom_bar/NodesChipMenu.tsx`: the menu of §8.5, fed by `nodes/read`, `nodes/residency` and `nodes/loadHistory` on open
- `SwitchModelModal.tsx`: drops the two swarm rows
- `components/chatServedBy/chatServedBy.ts`, `useChatServedBy.ts`: the served record, read at turn end through `nodes/servedLast`
- `components/chatServedBy/turnLine.ts`: the fallback, failed-load and loaded-in lines
- `components/noNodeNotice/NoNodeNotice.tsx`: Start <node>, Choose a node, the displaced notice, the removed-node line, the loader-absent line

**Tests:**
- `ModelsBottomBar.test.tsx`: the menu lists strategies and nodes with states; picking one sets the session model; a new chat opens on `forNewChats`
- the auto-sync does not fire for `node:` or `strategy:`
- `chatServedBy` over a fallback record gives the turn line; over a failed-load record, the failure line
- `NoNodeNotice` covers not-loaded, needs-step, displaced, displaced-with-failure, removed and loader-absent
- `SwitchModelModal.test.tsx` updated

**CDP live:** the chip's node equals the card that says Serving (livecheck, S11).

**Must not break:** Q-12 (the chip names model and Mac), Q-147 (live session rows), and the composer readiness bar's "only when something needs the user" rule.

### S5: The loader, holders and load measurements (R10)

**Order:** after S0, parallel with S2 and S3; J3 and J4 after S3 merges. **Confidence: LOW-MEDIUM**, flagged plainly:
- Stopping and starting ways from goosed is where races live. `r5.mjs` exists because Run-it switch races were real, and v1 now has two actors (Run it in the renderer, the loader in goosed) serialised by the swap claim and the load lock rather than one choreography.
- Holders across processes: each window's goosed and each swarm run must be seen; a stale or torn record must be named, never guessed (the route record's `Stale` / `Unreadable` discipline). The file-change wake for another process's reply is observed on a lifecycle read, so a wait can outlast the reply by one observation.
- The per-reply batching must be proven under two chats **in tool loops** and under a delegate, or it will thrash, starve or deadlock.
- The split's discover-and-config port must match `splitConfigFor` / `splitPlan` / `cleanConfig` case for case.
- A split start involves Link, the ranks' own budget re-checks (fit.rs notes a 0.5–0.6% drift refusal) and the Mac load lock.

The mitigations:
- The two shared fixtures (the switch, the split config), run by both suites.
- The loader refuses anything that needs a step rather than attempting it, and refuses outright under a build.
- `r5.mjs` extended with J4 before this ships.

**Owns:**
- `crates/goose/src/acp/server.rs`: the `mod nodes_loader;` line, installing the loader through the seam beside `install_route_load` (`:967`), and the reply guard in `on_prompt` (`:2564`) that opens and closes the session's reply in the holders record
- new `crates/goose/src/acp/server/nodes_loader.rs`: `ensure_serving` (§6.4 steps 1–13), the FIFO of reply-owned guards
- new `nodes_loader/switch.rs` (stop set, stop-first order), `nodes_loader/split_config.rs` (the port), `nodes_loader/holds.rs` (reply holds, `note_lease`, `note_child`), plus `switch.fixture.json` and `split_config.fixture.json`
- `crates/goose/src/acp/server/mlx_remote_single.rs`, `mlx_distributed.rs`: the minimal `pub(crate)` core-function extraction so the loader calls the same cores as the handlers, and the load row at their ready paths
- new `crates/goose-sidecar/src/holders.rs` and its `lib.rs` line: the Mac-wide holder records and the swap claim, with `machine.rs`'s liveness and proof-of-gone rules
- `crates/goose-sidecar/src/engine.rs`: the load row at the manager's mount-ready path
- `local-edition/mlx/quality/harness/r5.mjs`: J4
- new `ui/desktop/src/components/leanzero-swarm/placementSwitch.fixture.test.ts` (runs `switch.fixture.json` against PlacementCard's exported `servingWays`) and `splitConfig.fixture.test.ts` (runs `split_config.fixture.json` against `mlxDistributed.ts`); neither edits a source file

**Tests:**
- Rust: stop-set computation over the switch fixture, including a remote single on the Studio stopped for a single here, and a split stopped for a remote single
- a build holder → the refusal; keepLoaded refusal; fit Block refusal with the verdict text; a needs-step refusal carries the planner words; a split for another model → discover and config, not a refusal; a provisioning need → a step
- replies: an open reply in another process (a fixture holder file) → Wait; its end → woken; a reply's second and third completions never trigger a swap; a delegate's demand is its parent's own and does not deadlock; a reply opened after a queued demand waits behind it; a cancelled demand leaves the queue before any stop; a cancel after the stops began completes and records `cancelledAfterStop`
- a failed start → no restore, `outcome: failed` recorded, the words returned
- two loaders in two processes → one holds the swap claim, the other waits on it
- `holders.rs`: a dead pid is stale and ignored, a torn file is unreadable and refuses, proof-of-gone before displacement
- load rows written at all three ready paths
- **Live:** J3 and J4 on both Macs with no double mount, no orphan engine (`clean.sh` list empty after), and every swap recorded; `split-start.mjs` still passes

**Must not break:**
- gate 4: stops go through the existing unmount and stop paths, never killpg
- gate 5: every wait is on a reply ending or a record changing
- the Mac load lock, the route record's precedence, the serving intent restore, Make room, and Run it's switch

### S6: Strategies and the Nodes page shell (R8, R9)

**Order:** after S2 (it uses NodeCard's state chips and `nodeGlance` in the pickers) and after S1 merges (NodesView is handed from S1). **Confidence: MEDIUM.** The editor is dense. The risk is words and layout at 460, not logic. `strategyFit` is pure and simple in v1 (any two different MLX ways conflict).

**Owns:**
- `components/nodes/NodesView.tsx` (handed from S1): the tab strip, `NodesTab` (S2), the "Your swarm pool" section rendering `SwarmNodesSection` unchanged (open while `forBuilds` is `pool`, a Disclosure otherwise), and `StrategiesTab`
- `components/nodes/UseSelectors.tsx`: "New chats start on" and "Swarm builds use", both written through `nodes/write` with refusals shown verbatim
- `components/nodes/StrategiesTab.tsx`, `StrategyCard.tsx`, `StrategyEditor.tsx`, `strategyFit.ts`, `strategySentence.ts`

**Tests:**
- `sentenceFor` for every when × ifNotLoaded × chain length, compared against exact expected strings
- `strategyFit`: one Mac; a split plus a single on another Mac → swaps (the one-way rule, not a shared Mac); Chat and Build on different MLX ways → the delegate warning; one MLX way plus cloud → no swaps; cloud-only
- the editor's validation refusals rendered, including a `share` across two MLX ways
- "Use for swarm builds" hidden and its reasons shown for a split strategy; shown for an eligible one
- inheritance display, and a new strategy's Build reading "Same as Chat"
- the pool section renders `SwarmNodesSection` and its Share stepper still writes `speed_weight`

**CDP live:** Nodes page on the installed build: cards and pool section present; each card's state equals engine truth (`mlxEngineActivity`, `mlxDistributedStatus`, remote single status); the serving card's figures equal the Engine tile's to the digit.

### S7: The glance, the nav chip and My Macs (R3)

**Order:** after S0 and after S1 merges. **Confidence: MEDIUM-HIGH.** Additive lines on existing surfaces; the new part is the node name travelling from a renderer to main inside the existing sessions report.

**Owns:**
- `ui/desktop/src/components/engineGlance/glanceStore.ts`: the renderer's sessions report gains the serving node's name, read from `nodes/residency` when a changed glance arrives (an event; no new poller)
- `ui/desktop/src/main.ts`: `publishEngineGlance` (`:2270-2290`) passes the reported name into `buildEngineGlance`'s options (the call at `:2272`)
- `ui/desktop/src/utils/engineGlance.ts`: an optional `nodeName` in the options and on `EngineGlance`
- `components/engineGlance/EngineGlanceCard.tsx`: the one line
- `components/Layout/NavigationPanel.tsx` (handed from S1 if S1's gating edit touches it): the Nodes item's Loading / Failed chip, from the glance
- `components/leanzero-swarm/MyMacs.tsx`: "Nodes on this Mac" and the "Add another Mac" card; new `MyMacs.test.tsx` (MyMacs has none today)

**Tests:** `engineGlance.test.ts` (nodeName from the report; absent otherwise, never guessed), the `EngineGlanceCard` line, the nav chip for loading, failed and neither, `MyMacs.test.tsx`.

### S8: Swarm runs register as engine holders

**Order:** after S5 (it uses `holders.rs`). **Confidence: MEDIUM.** It is a write-only side effect in the engine's process plus one refusal on a mount that would stop someone else's work. For the golden (LM Studio only), `engines_for_run` returns before any sidecar is registered, so nothing is written and nothing is refused: byte-identical, proven by the development gates and a run of `goose swarm gate` on an archived tree.

**Owns:** `crates/goose-cli/src/commands/swarm_engine.rs`:
- `engines_for_run` registers a swarm-run holder for the life of the run when it registers a sidecar engine, naming the way its `mlx_engine.port` engine serves
- `SidecarEngine::ensure_loaded` reads the holders before mounting: when a goosed holds this Mac's engine with an open reply or a `keepLoaded` node, the mount is refused with the holder's words through the existing `MountFailure` path (the device leaves the pool by name, `sidecar-device-excluded` class), instead of racing goosed's loader

**Tests:** a registered holder is withdrawn at run end and on a crash (the flock drops); `ensure_loaded` with a goosed reply open → a named `MountFailure`; with no holder → today's behaviour; the development gates stay green.

### S9: Swarm Tier B, roles in the engine (gated)

**Order:** after S3, S5 and S8, plus a measured run. **Confidence: LOW.** It is an engine change on the guarded path, the frontend/backend declaration is new plan content, and phase-boundary swaps interact with the research fan's work-stealing.

**Owns:** `crates/goose-cli/src/commands/swarm.rs` (swarm-surgeon; includes D2's Rust doc comment) and `crates/goose-swarm/src/scheduler.rs` plus the task-role field in `dag.rs` (scheduler-surgeon), per §7.2.

**Lands only with:**
- the identity arm reproducing the golden's `pool_resolved` and `levers_resolved`, with score at or above 0.4616
- the role arm graded per phase
- gate 8 traces

**Ledger status:** `SCHEDULED waits on: S5 LANDED + S8 LANDED + identity-arm sb-7 run`.

### S10: More than one MLX way at a time (gated)

**Confidence: LOW.** v1's one-way rule is the product's real rule. Lifting it means, together: a route record that holds more than one remote route, removing the `sidecar_routed_away` exclusion, a Run it switch and a loader stop set that stop only the Macs the target needs, the router choosing among concurrent MLX engines, and, for two models on one Mac, `MlxEngineManager` supervising more than one engine (ports, the load lock, the tray and the glance's one-engine assumption).

**Waits on:** a measurement of how much a second engine on the same GPU slows the first (decode tok/s of A while B serves, on the M4 Max and the M3 Ultra), and of what a second concurrent remote route costs the relay. Two ways share the Metal working set (Q-11, Q-106), and nothing measures the interference today. Its file ownership is cut when the measurement lands.

**Ledger status:** `SCHEDULED waits on: the co-residency interference measurement`.

### S11: The state harness and the live check

**Order:** after S2, S4, S6 and S7, and after S1 merges (App.tsx is handed from S1). **Confidence: MEDIUM-HIGH.** It renders existing components from fixtures and reads the live app read-only.

**Owns:**
- `ui/desktop/src/App.tsx`: the `#/harness/nodes` route, registered only under `GOOSE_UI_HARNESS=1`
- `components/nodes/__states__/states.ts`, new `components/nodes/NodesHarness.tsx`
- `local-edition/mlx/quality/harness/nodes-states.mjs`, `nodes-livecheck.mjs`

**Tests:** §10.2 and §10.3.

---

## 10. Test plan

The owner asked for "quite a lot of testing". Four layers follow. Each one refuses something the layer below it cannot see.

### 10.1 Unit

**Rust:**
- `nodes/*`: fixture round-trip, adopt, validate, resolve, project and eligibility, served, residency, loads store
- the loader: stop sets, holders, reply holds and delegates, the swap claim, failures and cancels
- `swarm_router`: every route and when-rule on fake probes, the served record per lease kind, Auto's Share tie-break
- `summon`: delegate role, provider guard
- `cloud.rs`: the D7 parity test
- `swarm_engine.rs`: the holder and the refused mount

**TS (vitest):**
- the model and resolve mirror on the same fixture; the switch and split-config fixtures against PlacementCard and `mlxDistributed.ts`
- `nodeGlance` for every state, `strategyFit`, `strategySentence`, `usePlacementPlans`
- `MlxSetupStrip`, `NodeCard`, `NewNodeDialog`, `StrategyEditor`, `UseSelectors`
- the chip menu, `chatServedBy` with served records, `NoNodeNotice` states, routes and redirects, nav and its chip
- `EngineGlanceCard` node line, `MyMacs`

Every surface test carries the Studio-clean assertions (§S2).

### 10.2 State harness

The engine glance proved the value of state fixtures (`engineGlance.fixtures.ts`). This harness extends that idea to screenshots.

**Fixture file:** `components/nodes/__states__/states.ts` enumerates every state named in this document:
- empty and signed out
- one Mac
- two Macs idle
- split serving (writing, reading)
- single serving on this Mac
- remote single serving on the Studio (every other MLX card `displaced`)
- a swap in progress (stopping the source; loading the target in each load phase)
- a build holding the engine
- a node that can't run (too big; Mac not connected; engine failed)
- needs a step
- displaced
- legacy follows, and a pool node that left the pool
- cloud ready, key missing, and failing
- a strategy with primary down → secondary, and with primary failed to load
- a strategy that swaps, with the delegate warning
- a strategy refused for builds (split; cloud planner)

**Where it renders:** `#/harness/nodes?state=<id>&theme=<l|d>`, registered only when goose runs with `GOOSE_UI_HARNESS=1`. That route renders the NodeCard grid, the strategy card, the editor, the chip menu (open) and the composer line for the state, fed by fixture facts through the same components.

**The driver:** `local-edition/mlx/quality/harness/nodes-states.mjs` runs the **packaged** binary on an isolated `GOOSE_PATH_ROOT` with `--remote-debugging-port` (memory `frontend-check-packaged-binary`). For each state × {light, dark} × {460, 1000, 1600} it:
1. Takes a screenshot to `~/goose-screenshots/nodes-states/<state>-<theme>-<w>.png`.
2. Asserts:
   - no horizontal overflow (`scrollWidth <= clientWidth`)
   - no `select`, and no `dialog` opened by `window.confirm`
   - no element with a left border wider than 1px that differs from its other borders (the rail ban)
   - every chip's ink-to-fill contrast is at least 4.5, computed from the computed styles
   - every pair of chip fills on one card or one strategy row is at least 15 apart in CIEDE2000 (§4.6)
   - no text with opacity below 1
   - every visible string exists in en.json (no raw ids)

The mlx-ux-critic then **reads** the screenshots. The assertions are tripwires; the reader is the gate (gate 7's law applied to UI).

### 10.3 Live walk over CDP (installed build, read-only)

`local-edition/mlx/quality/harness/nodes-livecheck.mjs` follows the `livecheck.mjs` pattern. It never clicks anything that changes state, and it navigates back to the chat it found.

It checks:
- the nav shows Nodes first, and its chip agrees with the glance
- Providers has two tabs, and LeanZero MLX has four with My Macs second
- every node card's state agrees with engine truth (`mlxEngineActivity`, `mlxDistributedStatus`, `remoteSingleStatus`), and at most one MLX card is `serving`
- the serving card's figures equal the Engine tile's
- the chip's node equals the Serving card and the session's `nodes/servedLast`
- the glance's node line equals it too
- the redirects land where they should, including the Link tray's "open"

It prints one JSON line and exits 3 on any contradiction. `r1.mjs` calls it every minute during an E2E, exactly as it calls `liveCheck`.

### 10.4 End-to-end journeys (on the owner's two Macs)

Each journey restores state afterwards with `split-start.mjs`. They are run by the quality loop's E2E procedure, and their results go into `E2E-RUNS.md`.

1. **J1 (the flow).**
   1. My Macs shows both Macs.
   2. Models.
   3. Run it: 27B split.
   4. Save as node.
   5. New chat: the chip shows the node.
   6. A turn answers.
   7. The card, chip and glance all name the node.
2. **J2 (primary down, then secondary).**
   1. Strategy "Everyday": Chat is 27B split 1st and a cloud node 2nd, with failover.
   2. Chat.
   3. Stop the split from Engine.
   4. The next turn goes to the 2nd; the turn line reads `nodes.fellBack`, and `nodes/servedLast` names the cloud node (the cloud lease is recorded).
   5. Start the node from the notice.
   6. The next turn is back on the 1st.
   7. Variant: make the 27B's load fail (a Mac disconnected mid-start); the turn goes to the 2nd with `nodes.loadFailed` in the turn line, and nothing is restored.
3. **J3 (a swap, across windows).**
   1. Strategy "Quick": Chat is Flash single on this Mac, Build is the 27B split.
   2. A chat that delegates. Observe the loader's Wait line, then Loading, then the swap to the 27B for the delegate and back to Flash for the parent's next completion (two swaps, as strategyFit warned).
   3. With a second window's chat mid-reply on the running way, a demand in the first window waits until that reply ends, and never stops the engine under it; the Wait line names the second window's chat. Since Q-257 both windows share ONE goosed, so this proves the IN-PROCESS wait (the loader's own open replies); the CROSS-PROCESS holders files are proven by J6 (a `goose swarm run` child holds the engine), and the cross-process wait by a chat mid-reply on a CLI `goose serve` beside the app.
   4. `mlx-load-measurements.jsonl` gains a row per load, including a Run it start.
   5. The displaced chat shows its notice.
   6. `clean.sh` lists no orphan engine.
4. **J4 (two chats in tool loops: the thrash and starvation probe).** Run by the extended `r5.mjs`. Two chats on two nodes with different ways, each asked for work that takes several tool steps per reply, send in turn for 20 replies. Check:
   - swaps equal **reply** alternations, never completion count; no swap happens mid-reply
   - no reply is starved
   - no double mount; the Mac load lock is always held by one process; the swap claim by one loader
   - a cancelled turn mid-queue leaves no swap behind
5. **J5 (Tier A, with LM Studio resident).** With one LM Studio model loaded on the fleet: a swarm build from chat on a strategy whose Planning and Build are "Flash · this Mac" (the engine's configured model) with Build shared with a cloud node. Check: the run's `pool_resolved` names the Build chain **plus the LM Studio resident** and the header says it joined; `planner_model` is Planning's node; `levers_resolved` echoes the projected block while the global config diffs unchanged; "Everyday" (split) shows its builds refusal and cannot be chosen; the benchmark's `bench_dispatch.mjs 9897 sb-7 1` path still runs the untouched pool.
6. **J6 (a build holds the engine).** Start a swarm build on the Flash single; from a chat on "27B · both Macs", the next turn is refused with `nodes.refusedBuild`, the card shows `heldByBuild`, and nothing is stopped; after the build ends, the same turn loads the 27B.

---

## 11. Open questions (the work proceeds on each recommendation)

1. **The nav name: the one naming decision surfaced to the owner.** He said "Swarm settings". The options are "Nodes", "Swarm" and "Swarm Settings". **Recommended: "Nodes"**, and this is put to him as a decision rather than assumed: it is the word he uses for the thing inside it, "Swarm Settings" reads as levers, and "Swarm" is the app's name. The move into the nav is his mandatory requirement either way; only the label is open.
2. **The nav position.** **Recommended: first, above MCPs.** A session starts from it (R11).
3. **Auto-load by default.** **Recommended: `ifNotLoaded = load` by default**, with every wait and swap announced and "Keep loaded" one click away. The cost is shown before it happens: a new strategy's Build is "Same as Chat", so it never swaps by default; a strategy whose Chat and Build are different MLX ways carries the delegate warning (two swaps per delegate call), and a `share` across two MLX ways is refused. The "Everyday" example uses one MLX way for every MLX role. Pool nodes (`follows`) never trigger loads.
4. **Should "Use for swarm builds" change what the Benchmark view runs?** **Recommended: no.** The benchmark keeps the untouched `swarm` pool (control arm, gate 3). Strategies reach builds only through the per-spawn `SWARM` env.
5. **More than one MLX way at a time.** **Recommended: not in v1.** One way at a time is the rule the product already enforces; swaps are honest and measured. S10 comes after the interference measurement.
6. **Per-node sampling overrides.** **Recommended: no.** The model's profile in Sampling stays the one source, and a node shows which profile it uses, with a link. A second place to set temperature would be a second door.
7. **Keep "Any node (Auto)" as the default for new chats?** **Recommended: keep it.** The owner's "mandatory" was about Swarm settings moving into the nav, not about forcing a node pick. Auto is the migration default (byte-identical behaviour for everyone who never makes a node), the chip always names what the chat runs on, and choosing is one click. It is last in the chip menu's node list.
8. **Chat roles beyond Chat and delegates.** **Recommended: none in v1.** Planning, Testing, Frontend and Backend act on swarm builds (Tier B), and the editor says so on those rows. A chat-level Plan → Build hand-off button (the Copilot pattern) is a later, separately measured feature.
9. **Run it on the loader.** **Recommended: not in this plan.** In v1 Run it keeps its own switch, pinned to the loader's by the shared fixture and serialised with it by the swap claim and the load lock. Converging Run it onto `nodes/ensureServing` would make PlacementCard depend on the least certain slice (S5); it becomes its own slice once J3 and J4 have passed on the loader, owning `PlacementCard.tsx` after S2.

---

## 12. Invariants this design touches, and how each is kept

| Invariant | Kept by |
|---|---|
| Gate 1: no silent substitution | A chain exhausted is a loud refusal naming every entry. A failed load is named and never quietly restored. "Not measured yet" replaces any guessed load time. Unknown Macs are kept and shown. A removed node's sessions are told, never moved to Auto. A failed plan read is a state. Tier A prints what it cannot express, including the LM Studio residents that join. The loader's absence is a named refusal |
| Gate 2: specific text | The role and task text reaching models is unchanged. Tier B's role comes from synthesis's own declaration and adds no prompt template |
| Gate 3: the benchmark launch | Strategies never touch the global `swarm` block. Per-spawn env only |
| Gate 4: reaping | The loader stops through the existing unmount and stop paths. Holders use `machine.rs`'s proof-of-gone before displacing anyone |
| Gate 5: no time inputs | Every loader wait is a reply ending or a record changing. Load durations are displayed, never decisions |
| Gate 6: one door | Node definitions and strategies go through `nodes/write` only; `swarm` keeps its one UI writer. The pool projection is `project()` only. Run it and the loader share one stop-first rule through one fixture |
| Gate 10: no absolutes | Weights are user ratios. Tier B roles are plan facts, not a file-extension table. No new numeric const |
| Q-128 model identity | Every node-to-way match uses `node_names_model` / `nodeNamesModel` |
| Q-154 unique names | Adoption deduplicates, validation refuses duplicates |
| The one-way rule | The loader's stop set, strategyFit's conflict rule and the card states all use "one MLX way serves this Mac's goose at a time" |
| The one fit rule | The loader, the strategy view and the cards read `fit.rs` verdicts. None recomputes them |
| One glance | Cards and the glance share `engineFigures`. The nav chip and chip menu read the glance and `nodes/residency`; no new app-wide poller |
| Owner UI rules | Three disjoint colour families, the harness assertions in §10.2 plus the critic's read |

---

## 13. Review corrections (2026-09-27)

The review's verdict was CONFIRMED-WITH-CORRECTION. Each item was re-verified in the code at `d95aea476` before it was accepted.

| # | Item | Verdict | Evidence re-checked | What changed |
|---|---|---|---|---|
| 1 | The load rule is one way at a time for this Mac's goose across all Macs, not one per Mac | ACCEPTED | `PlacementCard.tsx:874-915` `servingWays` lists local, peer and split; `switchTo` (`:1263`) stops each (Q-119 comment at `:1266`); `swarm_router.rs:467-469` refuses this Mac's engine while a remote route is up; `distributed_target` (`:693`) probes the split in place of the single; `RouteRecord` is one record | v1 adopts the real rule (§0.6, §2.2, §6.4). Stop set = every serving way. strategyFit: any two different MLX ways conflict. A `share`/`overflow` across two MLX ways is refused. `displaced` redefined. Lifting the rule is S10 |
| 2 | The loader cannot see holders outside its process | ACCEPTED | `mlx_serving.rs:17-18`; each window's goosed (`main.ts:2361`, `:2367`); `swarm_engine.rs:1355` creates its own `MlxEngineManager`; `ensure_loaded` (`:1255`) mounts under `allow_model_load` | Mac-wide holder records with a swap claim in `goose-sidecar/src/holders.rs` (the `machine.rs` pattern), written by every goosed (S5) and every swarm run (S8). The loader refuses under a build; one loader per swap by the claim; a build's own mount refuses when a goosed holds the engine |
| 3 | The batching unit is a call, not a reply | ACCEPTED | `LeasedStream` (`swarm_router.rs:1374`) holds the lease for one stream; `current_session_id()` at `:1031`; completions re-scoped at `reply_parts.rs:313`; the reply is `on_prompt` (`acp/server.rs:2564`) | Batching is per reply: a guard in `on_prompt` holds the way for the whole reply; delegates are their parent's own demand (`note_child`, no deadlock). J4 now uses chats in tool loops and counts reply alternations |
| 4 | Tier A cannot express what §7.2 and J5 claimed | ACCEPTED | `SidecarEngine::new` (`swarm_engine.rs:1128`) at `mlx_engine.port`; split at 8091 (`mlx_distributed_discover.rs:1267`); `reconcile_pool_with_fleet` hard-sets `enabled: true` (`fleet_order.rs:365`); the planner keep (`swarm.rs` ~27005) before the cloud merge (`:27178`) | `project()` refuses split, remote-single, other-model singles and a cloud Planning node, each with its reason; MLX entries are the local single of the engine's configured model; LM Studio residents are stated as joining. "Everyday" is shown as refused for builds. J5 runs with an LM Studio resident |
| 5 | The loader would remove Run it's split-for-another-model path | ACCEPTED | `startWay` (`PlacementCard.tsx:1189-1206`) calls `startSplitFor` (`:1128`) when `setupMatches` is false: discover, config, provision, start | The loader carries discover-and-config (`split_config.rs`, a port pinned by a shared fixture to `mlxDistributed.ts`). A split for another model is no longer `needsStep`. Provisioning, copying and permissions stay manual steps |
| 6 | Removed nodes come back; adopted names collide | ACCEPTED | §4.5 as written re-adopted any device with no node; every MLX device was named `<Mac name> engine` | `declined` list in `nodes`; adoption deduplicates names (model short name, then device id) |
| 7 | The pool becomes uneditable, and a second writer of `swarm` appears; D1 stays for Auto | ACCEPTED | S2 deleted `NodeModelCell` and the Share stepper while `forBuilds=pool` stayed the default; §8.3's "Add to your swarm pool" wrote `swarm` against S0's rule; `ties_go_to_the_heavier_node` (`swarm_router.rs:1730`) | The pool table stays, unchanged, as "Your swarm pool" (its one writer). The checkbox is removed. Pool nodes read their model and provider through from `swarm` (`poolDevice`). S3 makes Auto's tie-break read the pool's Share |
| 8 | No transport carries lease information to the UI | ACCEPTED | `leased()` registers MLX only (`LmStudio \| Cloud => None`) and the entry dies with the lease | A served-turn record for every lease kind (S0 `nodes/served.rs`, written by S3's router, persisted as the session's last record), read by S4 through `nodes/servedLast` at turn end. S0 owns every DTO, so `custom_requests.rs` has one owner; `mlx_serving.rs` is not touched by any slice |
| 9 | "No new poller" was false for the nav chip and chip menu; plans were hard-wired to chat | ACCEPTED | `useMacs.tsx:184` throws outside a provider; providers only in `LeanZeroSwarmView` and `WithMacs`; `usePlacementPlans` calls `mlxPlacementPlan('chat')` (`:690`) | The nav chip reads the app-wide glance (Loading / Failed; the "can't run" count is dropped). The chip menu reads goosed-side `nodes/residency` on open. `nodeGlance` runs only on the Nodes page under `WithMacs`. S2 gives `usePlacementPlans` a goal and a failed-read state (D10) |
| 10 | A failed load, a cancelled turn and some removals had no stated outcome | ACCEPTED | `RouteLoad` (`swarm_router.rs:1256-1268`): a failed load ends the turn with the route's words | §6.4 steps 11–12: no restore; failover continues to the next entry with the failure named, else the turn ends with the load's words; a cancel before the claim leaves nothing, after the stops completes and is recorded. Removal refuses on `forNewChats`, counts live sessions, and those sessions are told |
| 11 | Ownership and order | ACCEPTED | `buildEngineGlance` called at `main.ts:2272`; `cloud.rs` has 14 `CLOUD_DEFS` rows; the load row needs `engine.rs`; S2/S6 read S5 data; NodesView had four editors | Re-cut in §9.0: every file has one owner. main.ts is S7's. D7 is a test in `cloud.rs`'s test module (S3), no table move. `engine.rs` is S5's. S0 owns the loads reader, so S2's "not measured yet" is honest before S5; the loader is reached through S0's seam. NodesView is S6's; the selectors are S6's `UseSelectors.tsx` |
| 12 | Five smaller defects | ACCEPTED (all five) | `handleSetView` (`App.tsx:627-634`) builds `?tab=<section>`; the live line is `main.ts:2381`; `resolve_provider` (`summon.rs:1655-1680`) honours `GOOSE_SUBAGENT_PROVIDER`; `PlacementKey` uses `local`; `levers_resolved` reads the env-overridden block | S1 leaves main.ts alone (the `?tab=link` redirect covers it). Delegates get `@build` only when the provider resolves to `swarm`. Validation accepts `local` as this Mac. The claim about `levers_resolved` is corrected (it echoes the projected block). Tier B roles are declared by synthesis, not read from a file-extension table |
| 13 | UI colours collide | ACCEPTED | §4.3, §6.1 and §8.2 reused blue, violet, teal, green and orange across kinds, states and roles | §4.6: three disjoint families (kinds are ink with icons, states one colour per meaning, roles their own six hues), and a CIEDE2000 floor in the harness |
| §11.1 | Renaming the owner's "Swarm settings" to "Nodes" | ACCEPTED | — | "Nodes" kept, recorded as the one naming decision surfaced to him |
| §11.3 | Load-by-default plus delegates to Build means two swaps per delegate in "Everyday" | ACCEPTED | the draft's "Everyday" had Chat on the split and Build on Flash · this Mac | The example is fixed (one MLX way for every MLX role), a new strategy's Build is "Same as Chat", the delegate warning is shown, and two-way MLX sharing is refused |
| §11.7 | "Mandatory" read as forcing a node pick | ACCEPTED | the owner's "mandatory" was about moving Swarm settings into the nav | Auto stays the default, with the reason stated; R1 carries "mandatory", R11 does not |
| D2 | A doc comment, not a user-facing tooltip | ACCEPTED (reworded) | `swarm.rs:303-306` and `golden.ts:36-37` are doc comments; no UI surfaces `supervision` | D2 now says so; S2 corrects the TS comment and S9 the Rust one |

**Kept as the reviewer found them correct:** D8; the env override (`base.rs:733`); Tier A leaves the benchmark untouched; D3 deletable; one model per engine; an unknown context window is safe; `replace: true` keeps Back working; no seconds decide anything; S4's auto-sync risk does not exist as a defect (the guard stays in S4's tests as a regression check).

**Where this revision is least certain, stated plainly:**
- S5 stays low-medium. Two actors (Run it and the loader) are serialised rather than unified, a cross-process wake is observed on a lifecycle read, and the per-reply hold with delegate yielding is new logic with a deadlock shape if `root_session` is ever wrong. J3 and J4 are the proof; S5 does not ship without them.
- The palette's CIEDE2000 floor has not been computed yet; Chat pink against the can't-run red is the pair most likely to fail it.
- S8 touches the engine's process. It is byte-identical for the LM-Studio-only golden by construction, but that claim is proven by the development gates and a gate run, not by reasoning alone.
