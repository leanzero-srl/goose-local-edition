# Nodes and strategies: design for lane D (Q-193, Q-194, Q-195, Q-196)

Written 2026-09-27 by the lane D architect. The code was read at `a548c2ce0` (main, which carries the engine glance).
The installed app was walked read-only on 3.0.60 and 3.0.61, and the screenshots are in
`~/goose-screenshots/nodes-design/`. No product code was written; implementers cut from this document.

---

## 0. What this document decides, in fifteen lines

1. **A new left-nav place called Nodes, placed first.** It holds everything a user hands work to, with two tabs, Nodes and Strategies. Swarm Settings leaves Providers.
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
6. **The load rule.** The Macs keep today's product rule: one way running per Mac, and a split owns every Mac it uses. To serve a node that is not running, the new goosed-side loader works through these steps:
   1. It stops the ways that hold that node's Macs, but only once they have no turn in flight and no turn queued that arrived before this demand.
   2. The one fit rule judges the result.
   3. The loader loads through the same start paths Run it uses.
   4. It records how long the load took.

   A node marked "Keep loaded" is never stopped for another demand. No clock decides anything (gate 5); every wait is event-driven. Cloud nodes are always "loaded".
7. **Chat routes through the existing `swarm` provider, and swarm.rs is not touched.** Two new model ids, `node:<id>` and `strategy:<id>`, route through the router. Delegates of a strategy session use its Build role.
8. **Swarm builds come in two tiers.**
   - Tier A: the build strategy is compiled into the existing `swarm` block, which is handed only to the spawned run through the `SWARM` env var. There is no engine change, and the benchmark is untouched.
   - Tier B: roles per phase and per task, plus swaps at phase boundaries. This is an engine change, gated on a measured sb-7 run at or above 0.4616.
9. **Slices, in order, with confidence:**

   | Slice | Scope | Confidence |
   |---|---|---|
   | S0 | Contract and store | high |
   | S1 | Information architecture move | high |
   | S2 | Node cards and the New node dialog | medium-high |
   | S3 | Chat routing | medium-high |
   | S4 | Chip and session start | medium |
   | S5 | Loader and load measurements | **low-medium** |
   | S6 | Strategies UI | medium |
   | S7 | Glance, My Macs and Run it links | high |
   | S8 | Swarm Tier A | medium-high |
   | S9 | Swarm Tier B (gated) | **low** |
   | S10 | Two models resident on one Mac (gated) | **low** |

---

## 1. The owner's ask, as requirements

| # | Requirement | His words |
|---|---|---|
| R1 | Nodes are a first-class place in the left nav, not a Providers tab | "Swarm settings in the left navigation", "it's buried in providers" |
| R2 | My Macs is a LeanZero MLX tab beside Engine | "my Macs should actually go under Leanzero MLX as a tab next to… Engine" |
| R3 | The UI implies the flow: connect Macs, run a model, make nodes, start sessions on them | "an easy flowing way to start connecting this, the UI should imply this" |
| R4 | A node shows what it is at a glance, not just a model dropdown | "it doesn't offer any information on what the node is at a glance" |
| R5 | A node has evolved with distributed: single or split, which Macs | "it has not evolved with the rest where we added distributed" |
| R6 | Nodes are virtual definitions, not physical Macs | "definitions which is not the same as the physical nodes" |
| R7 | LeanZero MLX node variants follow the engine's recommendation (single, distributed) | "variations that the engine can recommend for distributed" |
| R8 | Strategies: primary and secondary, when to use which, weights | "primary node, secondary node, and when to use one or the other as well as weights" |
| R9 | Roles: planning, execution, testing, frontend, backend | "when a node is better used for planning or … execution or testing or frontend or backend" |
| R10 | The MLX engine loads and unloads models to serve strategies | "Leanzero mlx engine also gets the ability to unload and load models based on the strategies" |
| R11 | A session starts by choosing from the available nodes (mandatory) | "when you start a session you choose from your available nodes" |
| R12 | Cloud nodes are simple: a provider plus a model | "with Cloud it's simple, each node you create for openrouter with a model in mind" |

The holes he admitted, and where this design fills them:

- What a node is when it is not running. The node state vocabulary is in §4.3.
- What happens when two roles need two models on one Mac. That is the load rule in §6.4.
- How a task gets a role. §6.2 says which classifications exist today and which are new.
- How a chat uses a role. §6.2 and §7.1 cover it.
- What becomes of the existing pool and its Share column. That is the migration in §4.5 and the one meaning of weight in §6.3.

---

## 2. What exists today (mapped at `a548c2ce0`)

### 2.1 UI

Abbreviations used in this section: `S` = `ui/desktop/src`, `LZ` = `S/components/leanzero-swarm`.

**Routing.**
- The app uses a HashRouter (`S/App.tsx:853`). The Providers route is `leanzero-swarm` (`App.tsx:832`), `/mlx-engine` redirects to it (`:834`), and `LeanZeroSwarmRoute` passes `?tab=` through (`:315-319`).
- Main sends deep links over `set-view` IPC to `?tab=mlx` (`S/main.ts:2191`) and `?tab=link` (`main.ts:2369`).
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

It reads and upserts the whole `swarm` config key (`:130-182`). Add node lives in `LZ/AddNodeDialog.tsx:202`. Cloud nodes are added through IPC `swarm-cloud`, which runs the CLI (`S/main.ts:5656`).

**My Macs: `LZ/MyMacs.tsx` (505 lines).**
- Each `MacCard` (`:336`) shows memory, disk, models, chip, the let-others-use switches and Details.
- Its data comes from `LZ/useMacs.tsx` `MacsProvider` (`:251`), which polls Link and each Mac's status every 5 s.
- It is rendered only inside `LeanZeroLinkSection.tsx:806`, the Link tab.

**LeanZero MLX: `LZ/MlxEngineView.tsx` (3,109 lines).**
- Tabs are engine, models and sampling (`:2396`), with hardcoded labels (`:3003-3013`) and no routing.
- `EngineSection` (`:830`) holds `MlxStateTile` (`:1059`), the model picker, `MemoryBar` and `PlacementCard` (`:1189`), which is **Run it**.

**Run it: `LZ/PlacementCard.tsx` (1,834 lines).**
- The goal control offers Chat, Long documents and Many requests.
- Its ways are this Mac, "Run on <Mac>" (remote single) and "Run across your Macs" (split) (`waysOf` `:740`).
- Run goes through `run(way)` `:1249` → `startWay` `:1189` / `startSplitFor` `:1128`, and stop-first switching goes through `mlxEngineUnmount` `:1227` and `mlxDistributedStop` `:1236`.

**Number helpers.**
- `engineFigures()` and `promptProgress()` (`LZ/engineFigures.ts:40`, `:76`), shared by the tile and the glance.
- In the tile: `figures()` `:696`, `measuredOf()` `:689` and `compact()` `:375` (`MlxStateTile.tsx`).
- In `LZ/mlxLiveStats.ts`: `formatRate` `:409` and `compactTokens` `:402`.

**Engine glance.**
- Its single source is `buildEngineGlance` in main, pushed on `engine-glance` (`S/utils/engineGlance.ts`, `GlancePush` `:141`, `EngineGlance` `:73` with `nodes: GlanceNode[]`).
- It is drawn by `S/components/engineGlance/EngineGlanceCard.tsx:445` and `EngineGlanceInApp.tsx`: the dock card sits in the sidebar at `NavigationPanel.tsx:159`.
- The rules for when it shows are in `S/utils/engineGlanceRules.ts`.

**Left nav.**
- The panel is `S/components/Layout/NavigationPanel.tsx:90`. Its items live in `S/hooks/useNavigationItems.ts:37-42`: MCPs, Skills, Memories, Providers.
- Item labels are `navItemMessages` (`:54-72`).
- `/leanzero-swarm` shows only when `isLocal || mlxEngine` (`NavigationPanel.tsx:99-104`).

**Chat model chip.**
- The chip is `S/components/settings/models/bottom_bar/ModelsBottomBar.tsx:192`, and its label comes from `deriveChatServedBy` (`S/components/chatServedBy/chatServedBy.ts:680`).
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

**Chat routing.** The `swarm` provider (`crates/goose/src/providers/swarm.rs`) has two models: `swarm` (routed chat) and `swarm-build`, which spawns `goose swarm run` (`route_for` `:61`, spawn `:717`). For each turn the router:
1. Probes every device.
2. Keeps one node per MLX engine (`one_node_per_engine` `:1050`).
3. Prefers the conversation's sticky node, keyed on the system prompt plus the first user message (`:884`).
4. Otherwise takes the node with the most free slots, with ties going to the higher `weight` (`:972`).
5. Otherwise queues on every servable node.

MLX nodes get capacity `MAX_CONCURRENT_REQUESTS` (`:194`).

**Swarm build routing.**
- OPEN and SYNTHESIS always use `planner_model` (`swarm.rs:19836`, `:20087`).
- The research fan steals work across the fleet.
- BUILD and INTEGRATE go through `Scheduler::pick_device` (`crates/goose-swarm/src/scheduler.rs:1069-1190`). It filters on `enabled && !supervision && in_flight < weight` and ranks by load, then speed weight. `TaskSpec.preferred_model` (`crates/goose-swarm/src/dag.rs:24`) is only a tie-break.
- The judge uses `aux_model_for_call`, a name heuristic (`fleet_order.rs:422`: "27b" | "dense" | "coder").
- The REPAIR fix target uses `rank_fix_target`.
- No per-role, per-phase or frontend/backend node choice exists.

**The MLX engine.**
- It runs one model per engine (`crates/goose-sidecar/src/engine.rs:1170-1178`: "Any already-running engine is shut down first").
- Status carries the load phases `waitingForLoad | makingRoom | starting | loading | warming`, together with resident and weights bytes (`:755`), `active_requests`, memory, and `machine_load` (the Mac's load lock).
- The one fit rule is `crates/goose-sidecar/src/fit.rs`. It computes the budget as `min(available − RAM×0.093, GPU ceiling)`, charges the other engines' resident bytes, and credits `freed_by_switch_bytes` (the doc says: "Run replaces the way that serves now, it never adds a second one").
- The placement planner (`crates/goose-sidecar/src/placement/planner.rs:1032`) returns, for each model and goal, candidates of the form `PlacementKey{kind: single|tensor|pipeline, nodes, link}` (`placement/store.rs:42`). Each candidate carries fit, speed (measured or estimated), `Action` and `Outcome`, and the plan carries `Badge` and `badge_after_stopping`.
- The serving intent (`crates/goose/src/providers/mlx_serving_intent.rs`) records `Single | RemoteSingle | Split` for restore at launch.
- The speed store (`mlx-speed-measurements.jsonl`) holds prefill, decode and ttft per placement. **Nothing measures or stores how long a model takes to load.**

**Subagents.** `summon` resolves a delegate's model in this order: its params, then the recipe, then `GOOSE_SUBAGENT_MODEL`, then the parent session's model (`crates/goose/src/agents/platform_extensions/summon.rs:1595-1680`). The upstream lead/worker provider was deleted (upstream `c88e9ce10`).

**Config env override.** `Config::get_param` reads the environment variable named by the uppercased key **first** (`crates/goose/src/config/base.rs:733`). The CLI's `load_config` reads `swarm` through it (`swarm.rs:1446`). An env var `SWARM` on a child process therefore replaces the block for that child alone. §7.2 relies on this.

### 2.3 Defects found while mapping

These are filed as findings for the loop. Each is removed by a slice below.

| # | Defect | Evidence | Removed by |
|---|---|---|---|
| D1 | The Share stepper changes build routing but **not chat**. The chat router ignores `speed_weight` and tie-breaks on `weight`, which the build engine uses to mean concurrency | `swarm_router.rs:972`, `scheduler.rs:1069` | S3: a strategy weight means one thing in both routers |
| D2 | The `supervision` flag's tooltip says the node "takes the judge, review and synthesis calls". It does not: it only leaves the build pool and prefers idle jobs | `swarm.rs:302-306`, `golden.ts:36`, `fleet_order.rs` | S2: hidden from the UI; the Planning role replaces the idea (Tier B makes it real) |
| D3 | Dead second door: `S/components/settings/swarm/SwarmSettingsSection.tsx` (1,072 lines) still reads and writes `swarm` and is unrouted | `SettingsView.tsx:89-91` | S1: deleted |
| D4 | Providers and MLX tabs are not in the URL. A deep link cannot open Models or Sampling, and Back loses the tab | `LeanZeroSwarmView.tsx:47-50`, `MlxEngineView.tsx:2412` | S1 |
| D5 | Hardcoded English: "Providers", "LeanZero MLX", the MLX tab labels and the Nodes empty state | see §2.1 | S1, S2 |
| D6 | The planner and judge choice is a model-name heuristic ("27b", "dense", "coder"), a hard-coded bit that gate 10 names | `fleet_order.rs:422` | S9: the Planning role replaces it |
| D7 | The router's `CLOUD_REGISTRY` copies 4 of the 10 `CLOUD_DEFS` rows and works only because the rest are identities | `swarm_router.rs:95-100` vs `swarm/cloud.rs:25-95` | S3: one table, read by both |
| D8 | Unknown fields in `swarm` are silently dropped by any CLI save | `swarm.rs:1499` | S0: node definitions live in their own key |
| D9 | Five pollers of the same engine status | UI map §2.1 | S2: node cards subscribe to the existing stores and add no poller |

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
- **Node.** A named definition: a model plus one way for MLX, a provider plus a model for cloud or endpoint. It holds its own settings. Many nodes may share the same Macs. That is allowed, and the loader serves them one way at a time per Mac (§6.4).
- **Strategy.** Roles mapped to ordered chains of nodes, with when-rules and weights.
- **Use.** A chat session picks a node, a strategy or "Any node (Auto)". A strategy session's delegates use its Build role. Swarm builds use the strategy set "For swarm builds".

### 4.2 Storage: one new config key, owned by goosed

The `swarm` block cannot carry the new fields (D8), and a second free-form upsert door is how D3 happened. So:

- The new key `nodes` is read and written **only** through new ACP methods in goosed. The desktop never upserts it raw.
- The Rust types are the source. The TS mirror is pinned to the same `nodes.fixture.json` in both test suites, the way `model_identity.fixture.json` pins `node_names_model` and `nodeNamesModel` (Q-128).

```ts
// ui/desktop/src/components/nodes/model.ts  (mirror of crates/goose/src/nodes/mod.rs)
type Role = 'chat' | 'planning' | 'build' | 'testing' | 'frontend' | 'backend';

type NodePlacement =
  | { kind: 'single' | 'tensor' | 'pipeline'; macs: string[]; link?: string } // = PlacementKey
  | { kind: 'follows' };        // legacy MLX device: serves whatever this Mac's engine serves (Q-128 behaviour)

interface NodeDef {
  id: string;                   // stable slug, unique; equals the swarm device id when in the build pool
  name: string;                 // unique display name (Q-154: never two "mihai")
  kind: 'mlx' | 'cloud' | 'endpoint';
  model: string;                // MLX: the model id as modelsList names it; cloud: the provider's model id
  placement?: NodePlacement;    // MLX only
  goal?: 'chat' | 'longDocuments' | 'manyRequests'; // the goal the way was chosen for (MLX)
  provider?: string;            // cloud/endpoint: registry name ('openrouter', 'anthropic', a user endpoint id)
  keepLoaded?: boolean;         // MLX: the loader never stops this node's way for another demand
  origin: 'migrated' | 'user' | 'runIt';
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
  forNewChats: { kind: 'auto' } | { kind: 'node' | 'strategy'; id: string };
  forBuilds: { kind: 'pool' } | { kind: 'strategy'; id: string }; // 'pool' = today's swarm.devices, untouched
}
```

**ACP (S0):** `_goose/unstable/nodes/read`, `nodes/write` (validates, then writes), `nodes/removeNode`, `nodes/removeStrategy`.

S5 adds `nodes/ensureServing`, `nodes/residency` and `nodes/loadHistory`. The DTOs live in `crates/goose-sdk-types/src/custom_requests.rs`.

**Validation in `nodes/write`.** The UI shows each refusal verbatim.

- Ids and names are unique.
- Every chain names an existing node, and has at least one entry.
- Weights are integers of 1 or more.
- `placement.macs` names Macs that the roster knows. An unknown Mac is **kept** and shown as "not connected", never dropped.
- Removing a node that a strategy uses is refused with the strategy names. The UI offers "Remove from those strategies too".

### 4.3 Node state: one derivation, never guessed

`nodeGlance(def, facts)` lives in `ui/desktop/src/components/nodes/nodeGlance.ts`. Its facts come from stores that already exist:

- `useMacs`: roster, memory and models per Mac.
- The main-pushed engine glance and `chatServedBy`: what runs, phase, activity, load.
- `usePlacementPlans`: fit and speed for the node's model and goal.
- The speed store, via `measuredOf`.
- S5's `nodes/residency` and `nodes/loadHistory`.

It returns one state. Each state has its own colour, its words, and the action it offers.

| State | When (from facts) | Chip, solid fill | Line under the name | Card action |
|---|---|---|---|---|
| `serving` | the way running on those Macs is this node's way and model (model identity via `nodeNamesModel`) | green (`phase-serving`) "Serving" | the glance's stage: "Writing · 11.2 tok/s", "Reading prompt · 40.7K · 25%" or "Idle" | Stop |
| `loading` | the loader or Run it is starting this node's way | amber, dark ink, "Loading" | `EngineLoad.phase` in the tile's words: "Waiting for another load on Work's Mac Studio", "Making room", "Starting", "Loading · 12.4 of 31.0 GB", "Warming up" | — |
| `ready` | not running; the plan says this way fits and is startable (`Action` not `Unavailable`, `setup_matches`) | slate, white ink, "Not loaded" | "Starts in about 48 s · median of 3 loads", or "First start not measured yet" | Start |
| `displaced` | not running because another way holds one of its Macs | slate "Not loaded" plus a second chip "Macs busy" | "Work's Mac Studio is serving Flash (node Flash · Studio)" | Start (the confirm names what stops) |
| `needsStep` | the plan's `Badge.NeedsBothMacs.needs`, the model missing on a Mac, or the split set up for another model | orange, dark ink, "Needs a step" | the planner's own words: "Copy the model to Work's Mac Studio first", "Allow this Mac to run part of a split on Work's Mac Studio" | Open Run it |
| `cantRun` | `TooBig`, a Mac not connected, the engine failed with its own words, or fit Block even after stopping | red "Can't run" | "Too big: short 1.6 GB on Work's Mac Studio", "Work's Mac Studio is not connected to LeanZero Link", or the engine's `last_error` | Details |
| `follows` (legacy) | `placement.kind === 'follows'` | blue outline chip (solid border, no tint) "Follows this Mac" | "Serves whatever this Mac's engine runs: Qwen3.8-27B · split across 2 Macs" | Pin a way |
| cloud `ready` | the provider is configured and the last call succeeded (or no call yet) | violet "Ready" | "Always available · billed by OpenRouter" | — |
| cloud `keyMissing` | the provider is not configured | red "Key missing" | "Set up OpenRouter under Providers › Cloud Providers" | Set up |
| cloud `failing` | the last call failed | red "Last call failed" | the provider's error, verbatim | Details |

Every figure goes through `engineFigures()`, `measuredOf()`, `formatRate()` or `formatGb()`. A new `figureText` is never written; there are already two (D-list §2.1).

Memory "fit" on a card is the fit rule's own numbers from the plan's candidate for this way (`fit.nodes[]`: need against budget per Mac). While the node is serving, it is the live peak against budget, the same numbers the glance's `GlanceNode.peakGb` and `budgetGb` show.

### 4.4 Naming

A new MLX node's name defaults to `<model short name> · <where>`, where `<where>` is "this Mac", "<Mac name>" or "both Macs" / "<n> Macs". Examples: "27B Atlassian · both Macs", "Flash · Work's Mac Studio". A cloud node's name defaults to `<model short name> · <provider>`, for example "Claude Sonnet · OpenRouter". Names are editable and unique.

### 4.5 Migration of today's pool, on the first `nodes/read`

This is a pure function, `migrate(swarm_block, nodes_key)`. It is idempotent and it never writes `swarm`. Each case, and the node it produces:

- **An MLX device** (`engine: mlx-sidecar`): a node with `placement: {kind:'follows'}`, named `<Mac name> engine`. Its id is the same as the device's.
- **A cloud device:** a cloud node with `provider` = the family and `model` = `model_id`. Its id is the same.
- **An LM Studio device:** nothing. LM Studio is not offered in this edition (owner rule 2026-09-05). Such devices stay in `swarm` untouched, and the Nodes page shows one line: "2 LM Studio devices in your swarm config are not shown here".
- **A swarm device with no node** (for example one added by the CLI): it is adopted by the same rules on the next read.

`forNewChats` starts as `auto`. `forBuilds` starts as `pool`. Behaviour is therefore **byte-identical** until the user chooses otherwise.

---

## 5. Information architecture and routes

### 5.1 Left nav

`useNavigationItems.ts` `NAV_ITEMS` becomes, in order:

1. **Nodes**: route `/nodes`, lucide `Network` icon.
2. MCPs
3. Skills
4. Memories
5. Providers

The Nodes entry is gated like Providers: `isLocal || mlxEngine`. Its label is `navigation.itemNodes` = "Nodes". It carries a small solid count chip, which shows only while something is loading or failed:
- "Loading" (amber)
- "1 can't run" (red)

A permanent "ready" count is not shown, because it would be noise (Q-8).

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
| `#/leanzero-swarm?tab=link` | **redirect** to `?tab=mlx&mlx=macs` | `main.ts:2369` also changes to send `macs` |
| `#/harness/nodes?state=<id>&theme=<l\|d>` | the state harness (§10.2), registered only when goose runs with `GOOSE_UI_HARNESS=1` | new, test-only |

Every tab click writes the URL with `setSearchParams(..., { replace: true })`. That removes D4 and makes Back work.

### 5.3 Where each existing component lands

| Component | Today | After |
|---|---|---|
| `LeanZeroSwarmView.tsx` | Providers, 4 tabs | Providers, 2 tabs (LeanZero MLX, Cloud Providers), with an i18n title and subtitle |
| `MlxEngineView.tsx` tab strip | Engine · Models · Sampling, unrouted | Engine · **My Macs** · Models · Sampling, routed via `mlx=`, plus the setup strip (§8.6) above it |
| `LeanZeroLinkSection.tsx` (sign-in cards plus `MyMacs`) | Providers › My Macs | LeanZero MLX › My Macs (signed out: the Link sign-in cards; signed in: MyMacs plus LinkRoutesPanel) |
| `MyMacs.tsx` | inside the Link tab | same component. Each Mac card gains "Nodes on this Mac" (S7) |
| `DistributedLinkPeers.tsx` | used by `DistributedSetup` | unchanged |
| `SwarmNodesSection.tsx`, `NodeModelCell.tsx`, `AddNodeDialog.tsx` | Providers › Swarm Settings | **S1** hosts `SwarmNodesSection` unchanged under Nodes › Nodes, so there is value on day one. **S2** replaces it with node cards and the New node dialog, deletes `NodeModelCell` (a node's model belongs to its definition), and makes `AddNodeDialog`'s cloud path the New node dialog's cloud step |
| `settings/swarm/SwarmSettingsSection.tsx` | dead, unrouted | **deleted** (S1) |
| `CloudProvidersSection.tsx` | Providers › Cloud | unchanged. Its line "swarm nodes cannot yet" is reworded in S2: "Make a node from any configured provider under Nodes" |
| `PlacementCard.tsx` (Run it) | Engine tab | unchanged position. S2 extracts `PlacementCandidates` (the ways list) for reuse; S7 adds "Save as node" |
| `EngineGlanceCard.tsx` | sidebar dock, float, desktop | S7 adds one line naming the node the running way belongs to |
| `ModelsBottomBar.tsx`, `SwitchModelModal.tsx` | chip, then a modal with no nodes | S4: the chip menu lists strategies and nodes (§8.5). The modal loses the two swarm rows (they move to the menu) and keeps cloud and endpoint models |

### 5.4 What Providers keeps

Providers keeps where models come from:
- **LeanZero MLX:** your Macs, the engine, models and sampling.
- **Cloud Providers:** credentials and endpoints.

Its new subtitle: "Where your models run: the LeanZero MLX engine on your Macs, and the cloud providers you've signed in to. Turn them into nodes under Nodes."

---

## 6. Strategies

### 6.1 Roles, and what each means in words

Each role has one line of plain words, shown in the editor and on the strategy card. Every role's colour is a solid hue defined once in `components/nodes/roleHue.ts`, and the harness checks its contrast.

| Role | Words the user reads | Hue | Inherits when unset |
|---|---|---|---|
| Chat | "Your turns in a chat: answers, edits, tool calls" | pink `#DB2777` | Build |
| Planning | "Reading the request, asking questions, researching and writing the plan" | violet `#7C3AED` | Chat |
| Build | "Writing the code for each task" | blue `#2563EB` | Chat |
| Testing | "Running and checking the result, and fixing what the check finds" | teal `#0D9488` | Build |
| Frontend | "Build tasks that write the user interface (pages, components, styles)" | orange `#EA580C` | Build |
| Backend | "Build tasks that write the server side (APIs, services, data)" | green `#16A34A` | Build |

The resolution order is shown in the UI, as Claude Code does. An unset role row reads "Same as Build" in full-strength text with the Build chip; it is never greyed out.

Validation:
- A strategy must set at least one role.
- Inheritance cannot cycle. Chat and Build inherit from each other only when both are unset, which validation refuses.

### 6.2 How work gets a role: honest about what exists

| Role | Swarm today | Chat today | New (and in which slice) |
|---|---|---|---|
| Chat | — | every turn of a session (the `swarm` model routes it to a pool node) | S3: a `strategy:<id>` session's turns resolve to the Chat chain |
| Planning | OPEN and SYNTHESIS use the single `planner_model` pin. The research fan steals across the whole fleet. The judge uses a name heuristic (D6) | none | Tier A (S8): `planner_model` := Planning's 1st node. Tier B (S9): OPEN, ASK, the research fan, SYNTHESIS and judge looks resolve through the Planning chain |
| Build | BUILD tasks go through `pick_device` over the enabled pool | a delegate (`summon`) inherits the session's model | S3: a strategy session's delegates get `strategy:<id>@build`. Tier A: the pool := Build's chain with its weights |
| Testing | the INTEGRATE sink and REPAIR's `rank_fix_target` exist as separate choices with no role | none | Tier B only: the sink lanes and REPAIR fix lanes resolve through the Testing chain |
| Frontend / Backend | **no classification exists** | none | Tier B only: a BUILD task's role is derived from **its own `owned_files`**, a fact in the plan (gate 2). Frontend if every owned file is a UI file, backend if none is, Build if mixed. The extension list is a named table with a test, not a scattered literal. A task with no Frontend/Backend node falls to Build, which is the inheritance above |

Chat's honest limit: a chat has no planning phase today. Planning, Testing, Frontend and Backend affect **swarm builds only** (Tier B). Chat uses the Chat role, and its delegates use Build. The strategy editor says so on those rows: "Used by swarm builds".

### 6.3 One meaning of weight; the three when-rules

A weight lives on a chain entry inside a role, and it is read only by the `share` rule. There is no global node weight. That ends D1's two meanings.

In the build pool projection (Tier A), the Build role's weights become each device's `speed_weight` (routing share). Concurrency (`weight`) stays the node's measured capacity: MLX `MAX_CONCURRENT_REQUESTS`, cloud `instances`. It is never shown as a user lever.

| When-rule | UI label | Plain-words sentence the UI renders (example) | Router behaviour |
|---|---|---|---|
| `failover` (default) | "Use the next one only when the one before can't run" | "Chat runs on 27B · both Macs. If it can't run, on Claude Sonnet · OpenRouter." | 1st if servable (or loadable per `ifNotLoaded`), else 2nd, and so on. A busy 1st **queues** the turn on the 1st |
| `overflow` | "Use the next one when the one before is busy" | "Build runs on Flash · Studio; when Flash is busy, the extra tasks go to Claude Sonnet." | 1st with a free slot, else the next with a free slot, else queue on all |
| `share` | "Share the work by weight" | "Build is shared: Flash · Studio 2 parts, 27B · both Macs 1 part." | Smooth weighted round-robin over servable nodes (deterministic and testable, never random). Chat shares **per conversation, not per turn**: stickiness is kept, because moving a conversation between MLX nodes throws away its prompt cache (the split's 102K cached prefix in the walk). Swarm tasks share per task |

`ifNotLoaded` applies to MLX entries:

| Value | UI label | Behaviour |
|---|---|---|
| `load` (default) | "Load it and wait" | The turn waits while the loader makes the node servable (§6.4) |
| `useNext` | "Use the next one meanwhile" | The next chain entry that is servable now takes the turn, and no load is started. The turn line says why |

A chain whose entries are all exhausted is a **loud refusal** that names every entry and its reason, exactly the router's "no node can serve this turn" contract. It never falls to "any node", because that would be a silent substitution (gate 1). The user-configured chain is the only fallback, and every step down it is announced.

### 6.4 The load rule: what the MLX engine does physically

The facts it rests on, all in code today:
- One engine per Mac serves one model.
- A split owns every Mac it uses ("The split owns this Mac").
- Run replaces the running way and never adds a second one.
- The one fit rule judges memory, crediting what a switch frees.
- Loads on one Mac are serialised by the Mac's load lock (`machine_load`).

v1 keeps all of these. Co-residency (two models on one Mac at once) is S10, behind a measurement.

The **loader** lives in `crates/goose/src/nodes/loader.rs`, in goosed. It runs for chat turns now and for the swarm in Tier B. `ensure_serving(node, demand)` answers `Ready`, `Wait(reason)` or `Refused(reason)`:

1. **Served already?** The way running on the node's Macs is this node's way and model (`node_names_model`). Answer `Ready`.
2. **Cloud or endpoint?** Answer `Ready`. The provider's own errors are the only refusals.
3. **Needs a step?** The planner's `Action::Unavailable`, `StartSplit{setup_matches:false}`, the model missing on a Mac, or peer permission off. Answer `Refused("needs a step: <planner's words>")`. The loader never copies models, provisions or changes permissions on its own; those stay one click in Run it.
4. **Stop set.** Every running way that uses any of the node's Macs: this Mac's single, a peer's remote single, or the split.
   - If any of them belongs to a node marked `keepLoaded`: `Refused("<node> is kept loaded on <Mac>")`.
5. **Fit.** The fit rule with `freed_by_switch_bytes` = what the stop set frees, measured.
   - If it gives Block: `Refused(<the verdict's message>)`. The desktop shows the verdict and Make room exactly as Run it does.
6. **In flight?** A way in the stop set has a turn in flight (the router's leases, or `active_requests > 0` for other clients). The answer is `Wait("<way> is answering <n>; loading <node> when it finishes")`.
   - The demand joins a FIFO queue keyed by the stop set.
   - It is woken **by the lease release or by the next status read showing 0 active**. Both are events, not a clock (gate 5).
7. **Batching (the thrash guard, progress-based).** Turns for the *running* way that were already queued **before** this demand are served first. A turn for the running way that arrives **after** a swap demand is queued behind the swap. So two chats alternating between two models on one Mac cost one swap per alternation and never starve. The strategy view warns about this before it happens (§8.4).
8. **Start.** The loader goes through the **same core functions** the ACP handlers call:
   - `MlxEngineManager::mount` for a single here
   - the remote-single start for a peer
   - `distributedStart` for a split
   - the stop halves: `unmount`, remote stop, `distributedStop`

   Stop-first ordering is ported from `PlacementCard`'s switch. It is pinned by a shared fixture of (running ways, target way) → (stop list, start) that both the TS and the Rust suites run. After S5, PlacementCard calls `nodes/ensureServing` for "Use this", so there is **one** choreography.
9. **Record.** When the way answers `/v1/models`, the loader appends to `mlx-load-measurements.jsonl` (the new goose-sidecar `placement/loads.rs`): `{model, placement, macs, weights_bytes, phases_ms{starting, loading, warming}, total_ms, file_cache_warm: bool, outcome}`.
   - The UI shows the median of measured loads of the same model, way and Macs: "about 48 s · median of 3 loads", or "First load not measured yet".
   - **No estimate is shown in place of a measurement** (gate 1).
   - The measured time is **displayed, never used to decide** which node loads (gate 5).
10. **Serving intent.** A load by the loader writes the serving intent like any owner start, so a relaunch restores the last way the strategy used.

**What the user sees.** The strings are in §8.7:
- the composer line on the waiting chat
- the displaced chat's notice ("27B was stopped for Flash · Studio in chat 'Kickoff notes'; your next message loads it back")
- the Nodes card in `loading`
- the glance's existing loading stage, now naming the node

Cloud nodes never enter the loader's stop sets.

---

## 7. Where strategies take effect

### 7.1 Chat sessions (S3 + S4 + S5; **no swarm.rs change**)

**Model ids on the existing `swarm` provider** (`crates/goose/src/providers/swarm.rs` `route_for`):

| Model id | Meaning |
|---|---|
| `swarm` | "Any node (Auto)", today's pool routing, unchanged |
| `swarm-build` | unchanged |
| `node:<id>` | exactly that node |
| `strategy:<id>` | that strategy's Chat chain |
| `strategy:<id>@<role>` | that strategy's role chain. Used for delegates |

**The router** (`swarm_router.rs`) turns a route into its candidate nodes:
- It builds them from node definitions (§4.2), not from `swarm.devices`.
- It reuses the same probes, `one_node_per_engine`, stickiness, slots and queueing.
- It applies the when-rule.
- It calls the loader for not-servable MLX entries whose `ifNotLoaded` is `load`.

Every lease carries `{node, role, rank_in_chain, reason_if_not_first}`. `chatServedBy` reads it, so the chip and the turn line name the node that served. Before S5, a not-loaded MLX node is a structured refusal `node_not_loaded{node}`. NoNodeNotice then offers **Start <node>**, which opens Run it pre-selected on that way.

**Context window.**
- `node:<id>`: that node's window.
- `strategy:<id>`: the smallest window in the Chat chain, so goose compacts before the smallest node's wall.
- `swarm`: unchanged. This is the same rule the router already applies to the pool.

**Delegates.** In `summon.rs` `resolve_model_config`, when the parent session's model is `strategy:<id>` and nothing overrides it, the delegate gets `strategy:<id>@build`. This is one clearly bounded change. The existing precedence (params > recipe > `GOOSE_SUBAGENT_MODEL`) is unchanged.

**Session start (R11).**
- `forNewChats` writes the global defaults (`GOOSE_PROVIDER=swarm`, `GOOSE_MODEL=node:<id>|strategy:<id>|swarm`) through the existing `acpSaveDefaults`.
- The chip menu changes it for one session through the existing `acpSetSessionProviderModel`.
- The ModelsBottomBar auto-sync (`:214-246`) that follows the engine's served model for `omlx` sessions stays as it is. It does not apply to `node:` or `strategy:` sessions, because their served node is the router's lease.

**Cloud.** `CLOUD_REGISTRY` is replaced by one shared table read from `CLOUD_DEFS`' data, which moves into a small shared module that both crates read (D7).

### 7.2 Swarm builds

**Tier A (S8): no engine change.** A pure one-door function, `project(swarm_block, strategy) -> swarm_block`, lives in `crates/goose/src/nodes/project.rs`:

- `planner_model` := the Planning role's 1st node's served model id.
- `devices` := the Build role's chain as devices. Each device has `enabled: true` and `speed_weight` = the entry weight (under `share`; otherwise the 1st gets the maximum weight and the others weight 1). Other devices get `enabled: false`.
- Every other field of the block is kept byte-for-byte.

The `swarm-build` spawn (the goose crate's `providers/swarm.rs`, `:717`) sets `SWARM=<projected block JSON>` **on the child only**. That spawn is the desktop's only door to `goose swarm run` outside the Benchmark view. The Benchmark view spawns `run_build.py` (`ui/desktop/src/main.ts:3978`), and Agent Work spawns `goose swarm agent run` (`main.ts:6967`); neither goes through `project`. The global config is never written, so **the Benchmark view and `bench_dispatch.mjs` see the untouched block. The control arm stays the control arm.**

Tier A is honest about what it cannot express. The run header and the strategy's "For swarm builds" note print it:
- Testing, Frontend and Backend "take effect when the engine learns roles; this build uses Build for every task".
- Planning's 2nd entry is not used ("the engine's own planner fallback applies").
- A strategy whose MLX nodes would need a swap mid-build is refused for builds: "This strategy swaps models on Work's Mac Studio; swarm builds can use it once the engine loads per phase".

Proof without a run: `project(block, golden_identity_strategy) == block`, byte-identical, as a unit test. Under `SWARM=`, the spawned run's first `pool_resolved` event names the projected devices. The CLI has no dry-run flag, so a live check reads that first event and stops the run.

**Tier B (S9): an engine change. It goes through swarm-surgeon and scheduler-surgeon, and is gated.**

The changes:
- **Role-aware dispatch.** `pick_device` filters devices by the task's role chain, with the chain's when-rule. The task role comes from the phase (planning, testing) or from `owned_files` (frontend, backend).
- The Planning chain replaces the `planner_model` pin and the name heuristic D6.
- **Phase-boundary loads.** Between phases, the engine asks goosed's loader to make the next phase's nodes servable, and it swaps only at those boundaries.

Gates this must respect:
- Gate 5: no seconds anywhere; waits are loader events.
- Gate 10: weights are user ratios, the extension table is named and tested, and no new numeric const.
- Gate 1: an unserved role is a loud `role_unserved{role, tried:[node: reason]}` event that tick.py prints, never "any device".
- Gate 6: roles never add tasks; they only route.
- Gate 9: the step's value is measured.

**Measurement it needs (the landing condition).** Each arm is run from the Benchmark view (gate 3):
1. **Identity arm.** Strategy "Golden", whose projection equals the golden pool. Its `pool_resolved` and `levers_resolved` must equal the r6h golden's, and its score must be at or above 0.4616.
2. **Role arm.** Planning on the 27B split, Build on per-Mac singles. Graded per phase on wall minutes and score against 0.4616.

It lands only with gate 8's trace. Status for the ledger row: `SCHEDULED waits on: S5 loader LANDED + the identity-arm sb-7 run`.

### 7.3 The engine glance and the tray

The glance stays **one engine view** (it answers "what is the engine doing"). S7 adds only the node name to it. When strategies spread work across two ways (for example a chat on the Studio single while a split is stopped), the glance follows `chatServedBy` as today.

A multi-way glance (one row per running way) is deliberately not added until S10 makes two concurrent ways on one Mac possible. Before that, at most one way per Mac runs, and the existing `GlanceNode[]` per Mac already covers it.

---

## 8. Screens

Common rules for every screen:
- Studio primitives only.
- No native `<select>`, `alert`, `confirm` or `prompt`.
- Chips are solid fills with ink that passes 4.5:1 in both themes (checked by the harness).
- No left accent rails. Emphasis comes from solid chips, weight and full borders.
- Every string goes through `defineMessages`, with key prefixes `nodes.*`, `strategies.*` and `mlxSetup.*`.

The numbers in the wireframes are illustrative, except the 27B split figures, which were read from the 3.0.60 and 3.0.61 walk. Every real figure comes from the derivations named in §4.3.

Widths:
- **460** (the app's narrow window): one column. Tables become stacked cards.
- **1000:** two columns.
- **Full (≥ 1400):** three columns of cards. The editor goes side by side with the "On your Macs" panel.

Both themes use the same solid hues. In dark mode, chip fills step to the 400-level of each hue with dark ink wherever white ink would fail contrast.

### 8.1 Left nav (1000, light)

```
┌────────────────────────────┐
│ ▣ Goose Swarm              │
│ ⌬ Nodes          [Loading] │  ← chip only while something loads / can't run
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
- `nodes.navCantRun` "{count, plural, one {# can't run} other {# can't run}}"

### 8.2 Nodes page, Nodes tab

**At 1000, with two Macs, a split serving, and one cloud node:**

```
Nodes                                                          [ Nodes | Strategies ]   [+ New node]
A node is a model you can hand work to, on your Macs or in the cloud. Chats start on a node or a
strategy; swarm builds use a strategy.

New chats start on:  [● Everyday (strategy) ▾]            Swarm builds use:  [Your swarm pool ▾]

ON YOUR MACS · LeanZero MLX                                      Manage Macs and models →
┌────────────────────────────────────────────┐  ┌────────────────────────────────────────────┐
│ [MLX] [Split · 2 Macs]          [Serving]  │  │ [MLX] [This Mac]            [Not loaded]  │
│ 27B Atlassian · both Macs                  │  │ Flash · this Mac                          │
│ Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx│  │ rapid-mlx/Qwen3.8-Flash-Next-4bit         │
│ Writing · 11.2 tok/s · Chat · Kickoff notes│  │ Starts in about 48 s · median of 3 loads  │
│ ─────────────────────────────────────────  │  │ ─────────────────────────────────────────  │
│ 10.4–11.6  tok/s writing  81–227 reading   │  │ 38–44  tok/s writing   (measured)         │
│ 262K context                               │  │ 131K context                              │
│ Mihai Macbook   38.6 of 61.8 GB  ██████░░░ │  │ Mihai Macbook  needs 18.2 of 61.8 GB  ██░ │
│ Work's Studio   38.6 of 66.2 GB  █████░░░░ │  │                                           │
│ Used by [Chat 1st · Everyday] [Planning 1st│  │ Used by [Build 1st · Everyday]            │
│ · Everyday]                                │  │ Starting it stops 27B · both Macs          │
│ [Stop]  [Edit]  [⋯]                         │  │ [Start]  [Edit]  [⋯]                        │
└────────────────────────────────────────────┘  └────────────────────────────────────────────┘

IN THE CLOUD                                                     Manage cloud providers →
┌────────────────────────────────────────────┐
│ [Cloud] [OpenRouter]              [Ready]  │
│ Claude Sonnet · OpenRouter                 │
│ <the provider's model id the user picked>  │   ← never a default
│ Always available · billed by OpenRouter    │
│ Used by [Chat 2nd · Everyday]              │
│ [Edit]  [⋯]                                 │
└────────────────────────────────────────────┘
```

**Card anatomy:**

1. **Chip row.** The kind chip (MLX solid blue, Cloud solid violet, Endpoint solid teal), the where chip ("This Mac", "<Mac name>", or "Split · N Macs" in solid indigo), and the state chip on the right.
2. **Name**, bold.
3. **Model id**, mono.
4. **Live or next line.**
5. **Figures row:** `engineFigures` / `measuredOf`, with "(measured)" or "(estimated)" exactly as Run it labels them.
6. **Memory**, one bar per Mac. The bar is a solid fill against its budget, with the number in text.
7. **Used by:** role chips in role hues. Each chip links to its strategy.
8. **The displacement line** (when starting it would stop something).
9. **Actions.**

The `⋯` menu is a Radix dropdown: Keep loaded (checkbox), Duplicate, Show in Run it, Remove.

**States of this page:**

- **Empty (no nodes, Link signed out):** an EmptyState reading "No nodes yet". Body: "Connect your Macs and run a model, or add a cloud model." Buttons: [Set up your Macs] (to `?tab=mlx&mlx=macs`) and [+ New node].
- **One Mac:** the where chip reads "This Mac". Split ways do not appear in New node. Under the MLX group: "Add another Mac to run models too big for this one →" (to My Macs).
- **Two Macs, split running:** as drawn above. The displacement line appears on every card whose Macs are held by the split.
- **Swap in progress:** the target card is `loading` ("Loading · 12.4 of 31.0 GB"). The source card is `serving` with a solid amber strip at the top: "Stopping for Flash · this Mac after this answer". A strip is a full-width top band, not a left rail.
- **Node unavailable:** a red "Can't run" chip with the reason line. The card stays at full strength and is never greyed.
- **LM Studio devices present:** a one-line notice at the foot of the page (§4.5).

**At 460:** one column. The "New chats start on / Swarm builds use" selectors stack. Figures wrap to two rows. The memory bars keep full width.

**Strings (en):**

| Key | Text |
|---|---|
| `nodes.title` | "Nodes" |
| `nodes.subtitle` | "A node is a model you can hand work to, on your Macs or in the cloud. Chats start on a node or a strategy; swarm builds use a strategy." |
| `nodes.tabNodes` / `nodes.tabStrategies` | "Nodes" / "Strategies" |
| `nodes.new` | "New node" |
| `nodes.forNewChats` | "New chats start on:" |
| `nodes.forBuilds` | "Swarm builds use:" |
| `nodes.auto` | "Any node (Auto)" |
| `nodes.pool` | "Your swarm pool" |
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
| `nodes.stateMacsBusy` | "Macs busy" |
| `nodes.stateNeedsStep` | "Needs a step" |
| `nodes.stateCantRun` | "Can't run" |
| `nodes.stateFollows` | "Follows this Mac" |
| `nodes.stateReady` | "Ready" |
| `nodes.stateKeyMissing` | "Key missing" |
| `nodes.stateLastCallFailed` | "Last call failed" |
| `nodes.startsIn` | "Starts in about {duration} · median of {count, plural, one {# load} other {# loads}}" |
| `nodes.firstStart` | "First start not measured yet" |
| `nodes.displaces` | "Starting it stops {names}" |
| `nodes.stoppingFor` | "Stopping for {node} after this answer" |
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
  [ ] Add to your swarm pool   (shown only while "Swarm builds use" is "Your swarm pool")
                                                           [Back]  [Create node]  [Create and start]
```

The cloud path runs Step 2 (Cloud) → the provider, from the **configured** providers only, plus the button "Set up another provider →" → the model, from the provider's own model list (existing `acpListProviderDetails` / model fetch) → Name.

States:
- No models: "No models on your Macs yet. Get one in Models →".
- No configured providers: the Cloud tile says "None set up · Set up".
- One Mac: Step 3 shows only single ways.
- A plan that failed to measure: the plan's `notes` are shown verbatim, and there is no guessed row.
- At 460: the stepper collapses to "Step 2 of 4 · Model", and the tiles stack.

Strings, all under `nodes.new*`: "New node", "Where should it run?", "On your Macs", "LeanZero MLX engine, one Mac or split", "A cloud model", "An endpoint", "Which model?", "How should it run?", "Name it", "Keep loaded: never stop it for another node", "Add to your swarm pool", "Create node", "Create and start", "No models on your Macs yet.", "Get one in Models", "None set up", "Set up another provider". The planner's candidate strings are reused from `placementCard.*`, not copied.

### 8.4 Strategies tab and the strategy editor

**List at 1000:**

```
Strategies                                                                  [+ New strategy]
A strategy says which node does what: its roles, the order to try nodes in, and when to use the next one.

┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ Everyday                                 [New chats start here] [Swarm builds use this]      │
│ "Big model thinks, fast model types."                     ← the owner's own note              │
│ [Chat] 27B · both Macs → Claude Sonnet                                                       │
│ [Planning] 27B · both Macs                                                                   │
│ [Build] Flash · this Mac ⇆ Claude Sonnet  (share 2:1)                                        │
│ [Testing] same as Build   [Frontend] same as Build   [Backend] same as Build                 │
│ On your Macs: swaps on Mihai Macbook (27B ⇄ Flash · about 1 min 40 s each, measured)         │
│ [Edit] [Use for new chats] [Use for swarm builds] [⋯ Duplicate · Remove]                    │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Editor (full width; at 1000 it is a large OverlayDialog; at 460 a full-screen sheet):**

```
Strategy  [Everyday                        ]   Note (your words)  [Big model thinks, fast model types.]

ROLE        WHAT IT IS                                   NODES IN ORDER                           WHEN TO USE THE NEXT        IF NOT LOADED
[Chat]      Your turns in a chat…                        1 [27B · both Macs ▾]  2 [Claude Sonnet ▾]  (•) can't run ( ) busy ( ) share   (•) Load it and wait ( ) Use the next meanwhile
            ↳ "Chat runs on 27B · both Macs. If it can't run, on Claude Sonnet · OpenRouter. If 27B isn't loaded, it loads and your turn waits (about 1 min 40 s)."
[Planning]  Reading the request, asking questions…       1 [27B · both Macs ▾]  + add               …
            Used by swarm builds.
[Build]     Writing the code for each task               1 [Flash · this Mac ▾] ×2   2 [Claude Sonnet ▾] ×1   ( ) ( ) (•) share
            ↳ "Build is shared: Flash · this Mac 2 parts, Claude Sonnet 1 part."
[Testing]   Running and checking the result…             Same as Build   [Set its own nodes]
[Frontend]  Build tasks that write the user interface…   Same as Build   [Set its own nodes]
[Backend]   Build tasks that write the server side…      Same as Build   [Set its own nodes]

ON YOUR MACS  (what this strategy asks of each Mac)
  Mihai Macbook       27B · both Macs (shard 1/2)   ⇆   Flash · this Mac       → swaps: about 1 min 40 s / 48 s (measured)
  Work's Mac Studio   27B · both Macs (shard 2/2)                              → fits
  Cloud               Claude Sonnet · OpenRouter                               → always available
  ⚠ Chat and Build need different models on Mihai Macbook. Each switch between them stops one and loads the other.
                                                                                  [Cancel]  [Save strategy]
```

Every node picker is the custom `Combobox`, and each option shows its state chip. The weight "×2" is a `WeightStepper`, shown only under `share`. Role chips use the role hues. At 460 each role becomes a stacked card (role chip, words, chain, when, if-not-loaded, then the sentence).

The "On your Macs" panel is computed in `components/nodes/strategyFit.ts` from the node definitions, the plan fit (`fit.nodes`) and the load history. It is pure and tested. v1 rule: two nodes conflict on a Mac when their ways both use that Mac and are not the same way. Each conflicting pair gets a "swaps" row with both measured load times, or "not measured yet".

Editor states:
- A new strategy has one Chat row set to the current `forNewChats` node.
- A role whose chain names a `cantRun` node shows that chip inline, and saving is allowed.
- A chain naming a removed node cannot happen; validation refuses it.
- "Use for swarm builds" on a strategy that swaps is refused before Tier B, with the §7.2 sentence.

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
| sameAs | "Same as {role}" |
| setOwn | "Set its own nodes" |
| usedByBuilds | "Used by swarm builds." |
| addNode | "Add a node" |
| onYourMacs | "On your Macs" |
| fits | "fits" |
| swaps | "swaps: {a} / {b}" |
| swapWarn | "{roleA} and {roleB} need different models on {mac}. Each switch between them stops one and loads the other." |
| cloudAlways | "always available" |
| useForChats | "Use for new chats" |
| useForBuilds | "Use for swarm builds" |
| badgeChats | "New chats start here" |
| badgeBuilds | "Swarm builds use this" |
| buildsRefusedSwaps | "This strategy swaps models on {mac}; swarm builds can use it once the engine loads models per phase." |
| save | "Save strategy" |

The six role names and their six description lines from §6.1 are also in this namespace. The sentence builder `sentenceFor(role, entry, facts)` (in `strategySentence.ts`) composes its sentences from ICU templates, one per when-rule × ifNotLoaded, never by string concatenation.

### 8.5 The model chip and session start

**Chip label:**
- `node:` sessions: "<node name> · <state>".
- `strategy:` sessions: "<strategy> · <node that served the last turn>".
- `swarm` sessions: "Any node · <node>".

The phase dot keeps today's rule.

**Chip menu** (Radix dropdown, as today):

```
Run this chat on
  STRATEGIES
  ✓ Everyday            Chat → 27B · both Macs        [Serving]
    Cheap and fast      Chat → Flash · this Mac       [Not loaded]
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

A **new chat** opens with `forNewChats`. The owner calls this choice mandatory, and it is always visible: the chip is never empty. When `forNewChats` is `auto` and the pool has no servable node, the existing NoNodeNotice shows, with a new first action: [Choose a node].

**Turn line** (under the assistant message, only when something is worth saying):
- "on Claude Sonnet · 2nd for Chat: 27B · both Macs can't run (Work's Mac Studio is not connected)"
- "on 27B · both Macs · loaded for this turn in 1 min 38 s"

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

Run it (S7): each way row gains "Save as node". After a Run it start succeeds, a single line appears: "Save this way as a node so chats and builds can pick it" [Save as node].

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
| `nodes.turnWaiting` | "Waiting for {way} to finish {count, plural, one {# answer} other {# answers}} on {mac}, then loading {node} ({duration})" | composer |
| `nodes.turnFirstLoad` | "First load of {node}, not measured yet" | composer |
| `nodes.displacedNotice` | "{node} was stopped for {other} in chat \"{chat}\". Your next message loads it back ({duration})." | displaced chat, above the composer. Actions: [Keep {node} loaded] [Use {next} instead] |
| `nodes.refusedKept` | "Can't load {node}: {kept} is kept loaded on {mac}." | composer |
| `nodes.refusedFit` | "Can't load {node} on {mac}: {verdict}" | composer; action [Make room] (the existing flow) |
| `nodes.fellBack` | "{role} is on {node} ({rank}): {primary} can't run: {reason}" | turn line. Action [Retry {primary}] |
| `nodes.needsStepRefusal` | "{node} needs a step first: {needs}" | NoNodeNotice. Action [Open Run it] |
| `nodes.glanceNode` | "Node · {name}" | glance card, under the mode line |

---

## 9. Slices

Every slice runs the full gate in `goose-feature-dev`: fmt, clippy `-D warnings`, cargo test for the crate, tsc, eslint, `i18n:check`, vitest.

**Rules that hold across all slices:**
- Slices are file-disjoint so they can run in parallel worktrees.
- `ui/desktop/src/i18n/messages/en.json` is regenerated, never hand-merged. After merging a slice, run `pnpm i18n:extract` on main and commit the result.
- `custom_requests.rs` is owned by S0, then by S5. No other slice touches it.
- A slice never edits `crates/goose-cli/src/commands/swarm.rs` or `crates/goose-swarm/*`, except S9.

### S0: Contract and store

**Order:** first. **Confidence: HIGH.** It is pure types, a migration and validation. The only risk is fixture drift between TS and Rust, which the shared fixture refuses.

**Owns:**
- `crates/goose/src/nodes/mod.rs`: types, read/write of key `nodes`, migrate, validate
- `crates/goose/src/nodes/resolve.rs`: pure chain resolution, `sentence_facts`
- `crates/goose/src/nodes/loader.rs`: a **stub** returning `Refused(NotLoaded)`, handed to S5
- `crates/goose/src/nodes/nodes.fixture.json`
- `crates/goose/src/acp/server/nodes.rs` and the dispatch registration
- the DTOs in `crates/goose-sdk-types/src/custom_requests.rs`
- `ui/desktop/src/acp/nodes.ts`
- `ui/desktop/src/components/nodes/model.ts`, `resolve.ts` and their tests

**Tests:**
- serde round-trip of every fixture case
- migration of: MLX device → `follows`; cloud device → cloud node; LM Studio → skipped plus a count; idempotence; an unknown Mac kept
- a validation refusal per rule
- `resolve` for each when-rule × {1st servable, busy, can't run, not loaded + load/useNext} × {chain length 1, 2, 3}, with the TS and Rust suites running the **same** fixture

**Must not break:** the `swarm` block is never written, and `forNewChats=auto` / `forBuilds=pool` keep today's behaviour.

### S1: Information architecture move (Q-193, Q-194)

**Order:** parallel with S0. **Confidence: HIGH.** It moves components and routes; the risk is deep links, and the tests list every one.

**Owns:**
- `ui/desktop/src/hooks/useNavigationItems.ts`
- `ui/desktop/src/App.tsx`: routes and redirects
- `ui/desktop/src/utils/navigationUtils.ts`
- `ui/desktop/src/components/leanzero-swarm/LeanZeroSwarmView.tsx`
- `MlxEngineView.tsx`: **tab strip region only**, plus the routed `mlx=` param
- new `ui/desktop/src/components/nodes/NodesView.tsx`, hosting `SwarmNodesSection` unchanged
- new `components/leanzero-swarm/MlxSetupStrip.tsx`
- `main.ts`: the one `'link'` → `'macs'` line
- delete `components/settings/swarm/SwarmSettingsSection.tsx` and its imports (keep `golden.ts`, which `SwarmNodesSection` uses)

**Tests:**
- `NavigationPanel.test.tsx`: Nodes first, gated like Providers
- route tests for every row of §5.2, including the two redirects and `ENGINE_ROUTE`
- `LeanZeroSwarmView.test.tsx`: two tabs, i18n title
- `MlxEngineView` tab test: 4 tabs, URL written on click, Back restores
- `MlxSetupStrip` renders each step state
- the existing `SwarmNodesSection.test.tsx` passes unchanged

**CDP live:** click Nodes in the nav, see the pool table. Open `#/leanzero-swarm?tab=link`, land on My Macs. Back restores the tab.

**Must not break:** `ComposerReadiness` `ENGINE_ROUTE`, main's `set-view` deep links, the glance's click-to-Engine, and `split-start.mjs` (it drives `#/leanzero-swarm` Run it; update its selector if the tab strip changes).

### S2: Node cards and the New node dialog (Q-195, Q-196 R6/R7)

**Order:** after S0 and S1. **Confidence: MEDIUM-HIGH.** The derivation joins five stores, and a subtle bug could show "Not loaded" on a serving split when the model-identity rule mismatches (the Q-128 class). The fixture includes that exact case: 27B split served under its HF id against an alias. Extracting the candidates list from a 1,834-line card must keep PlacementCard's tests green, **unchanged**.

**Owns:**
- `components/nodes/NodeCard.tsx`
- `nodeGlance.ts` (+ `nodeGlance.fixtures.ts`)
- `NewNodeDialog.tsx`
- `roleHue.ts`
- `NodesView.tsx` (replaces the table with cards)
- `components/leanzero-swarm/PlacementCard.tsx`: **extraction only**, into `PlacementCandidates.tsx`, with Run it re-using it
- delete `NodeModelCell.tsx`
- `AddNodeDialog.tsx`: its cloud path folds into the New node dialog
- `CloudProvidersSection.tsx`: the one reworded line
- `golden.ts`: the supervision tooltip goes (D2)

**Tests:**
- `nodeGlance` for every §4.3 state, from fixture facts
- `NodeCard` renders each state's chip, line and action, plus the Studio-clean assertions (no `select`, no `border-l-*`, no opacity below 1 on text)
- `NewNodeDialog`: MLX path with one Mac and with two Macs; cloud path with none configured and with one; an unmeasured plan shows its notes
- `PlacementCard.test.tsx` and `MlxEngineView.test.tsx` green, unchanged

**CDP live** (installed build, read-only): each node card's state equals the engine truth (`mlxEngineActivity`, `mlxDistributedStatus`), and the serving card's figures equal the Engine tile's to the digit.

**Must not break:**
- one derivation (engineFigures, fit rule) and no new poller (subscribe to `useMacs`, the glance and the latest-status store)
- Q-128 identity
- Q-154 unique names

### S3: Chat routing to a node or strategy (Q-196 R11)

**Order:** after S0, parallel with S2. **Confidence: MEDIUM-HIGH.** The router has a fake-probe test harness (`NodeProbe` trait) that makes every branch testable. The subtle part is stickiness under `share` and the smallest-window rule for strategies. The summon change is small but touches every delegate, so it is covered by a test that a non-strategy parent is byte-identical.

**Owns:**
- `crates/goose/src/providers/swarm.rs`: `route_for` gains `node:`, `strategy:`, `@role`, and the `swarm-build:strategy:` prefix for S8
- `crates/goose/src/providers/swarm_router.rs`: candidates from definitions, when-rules, weighted round-robin per conversation, a lease carrying `{node, role, rank, reason}`, context window per route, one shared cloud table (D7)
- `crates/goose/src/agents/platform_extensions/summon.rs`: delegates → `@build`

**Tests** (router unit tests on fake probes):
- each when-rule
- a chain exhausted → loud error naming each entry
- sticky kept under `share`
- `overflow` queues on all when every entry is busy
- a `node_not_loaded` refusal carries the node
- window = the smallest in the chain
- `swarm` model unchanged (the existing router tests pass)
- summon: a strategy parent gives `@build`; `GOOSE_SUBAGENT_MODEL` still wins; a non-strategy parent is unchanged

**Must not break:**
- gate 5: no clock in any wait
- the router's existing queue-on-all semantics
- Q-18 (the context window saved on sessions)
- Q-128 follow rules for `swarm`

### S4: The chip and session start (R11)

**Order:** after S3. **Confidence: MEDIUM.**
- `chatServedBy` is a large derivation with many consumers (composer, glance, tray title). Adding the lease's node must not disturb its readiness logic.
- The auto-sync at `ModelsBottomBar.tsx:214-246` must stay out of `node:` and `strategy:` sessions, or it would silently rewrite the session model (the gate 1 class).

**Owns:**
- `ModelsBottomBar.tsx`: menu and label
- `SwitchModelModal.tsx`: drops the two swarm rows
- `components/chatServedBy/chatServedBy.ts` and `useChatServedBy.ts`: the lease's node, role, rank and reason
- `components/noNodeNotice/NoNodeNotice.tsx`: Start <node>, Choose a node, and the displaced notice
- `ModelAndProviderContext.tsx`: `forNewChats` → defaults

**Tests:**
- `ModelsBottomBar.test.tsx`: the menu lists strategies and nodes with states; picking one sets the session model; a new chat opens on `forNewChats`
- the auto-sync does not fire for `node:` or `strategy:`
- `chatServedBy` over a fallback lease gives the turn line
- `NoNodeNotice` covers the not-loaded, needs-step and displaced states
- `SwitchModelModal.test.tsx` updated

**CDP live:** the chip's node equals the card that says Serving (livecheck extension, §10.3).

**Must not break:** Q-12 (the chip names model and Mac), Q-147 (live session rows), and the composer readiness bar's "only when something needs the user" rule.

### S5: The loader and load measurements (R10)

**Order:** after S3. **Confidence: LOW-MEDIUM**, flagged plainly:
- Porting PlacementCard's stop-first switch to Rust is where races live. `r5.mjs` exists because Run-it switch races were real.
- A split start involves Link, the ranks' own budget re-checks (fit.rs notes a 0.5–0.6% drift refusal) and the Mac load lock.
- The batching rule must be proven under two chats alternating, or it will thrash or starve.

The mitigations:
- The shared switch fixture.
- The loader refuses anything that needs a step rather than attempting it.
- `r5.mjs` extended with two-chat alternation before this ships.
- PlacementCard switches to the loader only after both pass.

**Owns:**
- `crates/goose/src/nodes/loader.rs`: the real body
- `crates/goose/src/acp/server/nodes.rs`: `ensureServing`, `residency`, `loadHistory` (plus their DTOs; S0 hands both files over)
- new `crates/goose-sidecar/src/placement/loads.rs`: the load measurement store, written by the manager's mount-ready path, the remote-single ready path and the split's ready path
- the minimal `pub(crate)` core-function extraction in `acp/server/mlx_remote_single.rs` and `mlx_distributed.rs`, so the loader calls the same cores as the handlers
- `ui/desktop/src/components/leanzero-swarm/PlacementCard.tsx`: `run(way)` → `nodes/ensureServing`, landed after S2 and S7 merged, in its own commit
- `local-edition/mlx/quality/harness/r5.mjs`: two-chat alternation

**Tests:**
- Rust: stop-set computation over a fixture of (running ways, target) → (stops, start), shared with a TS copy of PlacementCard's rules
- keepLoaded refusal; fit Block refusal with the verdict text; in-flight → Wait, woken by a lease release (a test clock-free notifier); FIFO and batching order; a needs-step refusal carries the planner words
- `loads.rs` append, median and absent → `None`
- **Live:** r5 two-chat alternation on both Macs with no double mount, no orphan engine (`clean.sh` list empty after), and every swap recorded; `split-start.mjs` still passes

**Must not break:**
- gate 4: stops go through the existing unmount and stop paths, never killpg
- gate 5
- the Mac load lock
- the serving intent restore
- Make room

### S6: Strategies UI (R8, R9)

**Order:** after S2 and S3. **Confidence: MEDIUM.** The editor is dense. The risk is words and layout at 460, not logic. `strategyFit` is pure and simple in v1 (conflict = shared Mac, different way).

**Owns:** `components/nodes/StrategiesTab.tsx`, `StrategyCard.tsx`, `StrategyEditor.tsx`, `strategyFit.ts`, `strategySentence.ts`, and the Strategies half of `NodesView.tsx`. S2 leaves a `<StrategiesTab/>` slot there, so this is an insertion only.

**Tests:**
- `sentenceFor` for every when × ifNotLoaded × chain length, compared against exact expected strings
- `strategyFit`: one Mac, two Macs with a split, split plus single on the same Mac → swaps, cloud-only
- the editor's validation refusals rendered
- "Use for swarm builds" refused on a swapping strategy before Tier B
- inheritance display

### S7: The glance, My Macs and Run it links (R3)

**Order:** after S2. **Confidence: HIGH.** These are additive lines on existing surfaces, each read from `nodes/read`.

**Owns:**
- `ui/desktop/src/utils/engineGlance.ts`: an optional `nodeName`, set in main from the way-to-node match
- `components/engineGlance/EngineGlanceCard.tsx`: one line
- `components/leanzero-swarm/MyMacs.tsx`: "Nodes on this Mac" and the "Add another Mac" card
- `PlacementCard.tsx`: "Save as node" per way, plus the post-start line. This lands **before** S5's PlacementCard commit; S5 rebases.

**Tests:** `engineGlance.test.ts` (nodeName from a matching way; absent otherwise), `EngineGlanceCard` line, a new `MyMacs.test.tsx` (MyMacs has none today), and PlacementCard's save-as-node.

### S8: Swarm Tier A, the strategy projected for builds

**Order:** after S3 and S6. **Confidence: MEDIUM-HIGH.** The env override path is proven in code (`base.rs:733`). The risk is that the child's children inherit `SWARM`: the app under test does, harmlessly, but the S8 test lists the child's env to prove nothing else consumes it. The benchmark isolation is proven by `bench_dispatch.mjs`'s path never calling `project`.

**Owns:**
- `crates/goose/src/nodes/project.rs`
- `crates/goose/src/providers/swarm.rs`: the `swarm-build` spawn sets `SWARM`. The prefix parsing was already landed in S3; this is S3's file, so S8 is sequenced after S3 merges.
- No main.ts change. The Benchmark (`run_build.py`) and Agent Work (`swarm agent run`) spawns stay untouched by construction.
- the "Swarm builds use" selector wiring in `NodesView.tsx` (after S6)

**Tests:**
- `project(block, golden_identity) == block`, byte-identical
- Build chain with weights → `speed_weight`s
- Planning 1st → `planner_model`
- a swapping strategy refused
- a spawn test asserts the child env carries `SWARM` and the global config is unchanged

**CDP/E2E:** a build from chat with "Everyday": the run's `pool_resolved` names exactly the Build chain.

**Must not break:** gate 3 (the Benchmark view untouched), the r6h golden (no engine file touched), and `levers_resolved` still echoes the operator's config.

### S9: Swarm Tier B, roles in the engine (gated)

**Order:** after S5 and S8, plus a measured run. **Confidence: LOW.** It is an engine change on the guarded path, the frontend/backend classification is new, and phase-boundary swaps interact with the research fan's work-stealing.

**Owns:** `crates/goose-cli/src/commands/swarm.rs` (swarm-surgeon) and `crates/goose-swarm/src/scheduler.rs` (scheduler-surgeon), per §7.2.

**Lands only with:**
- the identity arm reproducing the golden's `pool_resolved` and `levers_resolved`, with score at or above 0.4616
- the role arm graded per phase
- gate 8 traces

**Ledger status:** `SCHEDULED waits on: S5 LANDED + identity-arm sb-7 run`.

### S10: Two ways on one Mac when they fit (gated)

**Confidence: LOW.** It needs `MlxEngineManager` to supervise more than one engine (ports, the load lock, the tray and glance's one-engine assumption).

**Waits on:** a measurement of how much a second engine on the same GPU slows the first (decode tok/s of A while B serves, on the M4 Max and the M3 Ultra). Two ways share the Metal working set (Q-11, Q-106), and nothing measures the interference today.

**Ledger status:** `SCHEDULED waits on: the co-residency interference measurement`.

**Parallel plan:**
- {S0, S1}
- then {S2, S3}
- then {S4, S5, S6, S7}, with S5's PlacementCard commit last
- then S8
- then S9 and S10 on their measurements

At most three surgeons run concurrently (memory `be-mindful-of-usage`).

---

## 10. Test plan

The owner asked for "quite a lot of testing". Four layers follow. Each one refuses something the layer below it cannot see.

### 10.1 Unit

**Rust:**
- `nodes/*`: fixture round-trip, migrate, validate, resolve, project, loader stop-sets, loads store
- `swarm_router`: every route and when-rule on fake probes
- `summon`: delegate role

**TS (vitest):**
- the model and resolve mirror on the same fixture
- `nodeGlance` for every state, `strategyFit`, `strategySentence`
- `MlxSetupStrip`, `NodeCard`, `NewNodeDialog`, `StrategyEditor`
- the chip menu, `chatServedBy` with leases, `NoNodeNotice` states, routes and redirects, nav
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
- remote single serving on the Studio
- a swap in progress (stopping the source; loading the target in each load phase)
- a node that can't run (too big; Mac not connected; engine failed)
- needs a step
- displaced
- legacy follows
- cloud ready, key missing, and failing
- a strategy with primary down → secondary
- a strategy that swaps
- a strategy refused for builds

**Where it renders:** `#/harness/nodes?state=<id>&theme=<l|d>`, registered only when goose runs with `GOOSE_UI_HARNESS=1`. That route renders the NodeCard grid, the strategy card, the editor, the chip menu (open) and the composer line for the state, fed by fixture facts through the same components.

**The driver:** `local-edition/mlx/quality/harness/nodes-states.mjs` runs the **packaged** binary on an isolated `GOOSE_PATH_ROOT` with `--remote-debugging-port` (memory `frontend-check-packaged-binary`). For each state × {light, dark} × {460, 1000, 1600} it:
1. Takes a screenshot to `~/goose-screenshots/nodes-states/<state>-<theme>-<w>.png`.
2. Asserts:
   - no horizontal overflow (`scrollWidth <= clientWidth`)
   - no `select`, and no `dialog` opened by `window.confirm`
   - no element with a left border wider than 1px that differs from its other borders (the rail ban)
   - every chip's ink-to-fill contrast is at least 4.5, computed from the computed styles
   - no text with opacity below 1
   - every visible string exists in en.json (no raw ids)

The mlx-ux-critic then **reads** the screenshots. The assertions are tripwires; the reader is the gate (gate 7's law applied to UI).

### 10.3 Live walk over CDP (installed build, read-only)

`local-edition/mlx/quality/harness/nodes-livecheck.mjs` follows the `livecheck.mjs` pattern. It never clicks anything that changes state, and it navigates back to the chat it found.

It checks:
- the nav shows Nodes first
- Providers has two tabs, and LeanZero MLX has four with My Macs second
- every node card's state agrees with engine truth (`mlxEngineActivity`, `mlxDistributedStatus`, `remoteSingleStatus`)
- the serving card's figures equal the Engine tile's
- the chip's node equals the Serving card
- the glance's node line equals it too
- the redirects land where they should

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
   4. The next turn goes to the 2nd, and the turn line reads `nodes.fellBack`.
   5. Start the node from the notice.
   6. The next turn is back on the 1st.
3. **J3 (a swap).**
   1. Strategy: Chat is Flash single on this Mac, Build is the 27B split.
   2. A chat that delegates.
   3. Observe the loader's Wait line, then Loading, then the swap.
   4. `mlx-load-measurements.jsonl` gains a row.
   5. The displaced chat shows its notice.
   6. `clean.sh` lists no orphan engine.
4. **J4 (two chats alternating: the thrash and starvation probe).** Run by the extended `r5.mjs`. Two chats on two nodes that share a Mac send turns in turn for 20 turns. Check:
   - swaps equal alternations
   - no turn is starved
   - no double mount
   - the Mac load lock is always held by one process
5. **J5 (Tier A).** A swarm build from chat on "Everyday": `pool_resolved` names the Build chain, the benchmark's `bench_dispatch.mjs 9897 sb-7 1` path still runs the untouched pool, and the global config is diffed unchanged.

---

## 11. Open questions (the work proceeds on each recommendation)

1. **The nav name.** The options are "Nodes", "Swarm" and "Swarm Settings". **Recommended: "Nodes".** It is the owner's own word for the thing, "Swarm Settings" sounds like levers, and "Swarm" is the app's name.
2. **The nav position.** **Recommended: first, above MCPs.** A session starts from it (R11).
3. **Auto-load by default.** **Recommended: `ifNotLoaded = load` by default** for strategies and nodes the user creates, with every wait and swap announced and "Keep loaded" one click away. Migrated legacy nodes (`follows`) never trigger loads.
4. **Should "Use for swarm builds" change what the Benchmark view runs?** **Recommended: no.** The benchmark keeps the untouched `swarm` pool (control arm, gate 3). Strategies reach builds only through the per-spawn `SWARM` env.
5. **Two models resident on one Mac at once.** **Recommended: not in v1.** Swaps are honest and measured. S10 comes after the interference measurement.
6. **Per-node sampling overrides.** **Recommended: no.** The model's profile in Sampling stays the one source, and a node shows which profile it uses, with a link. A second place to set temperature would be a second door.
7. **Keep "Any node (Auto)"?** **Recommended: keep it** as the migration default and for users who never make a strategy. It is last in the chip menu's node list.
8. **Chat roles beyond Chat and delegates.** **Recommended: none in v1.** Planning, Testing, Frontend and Backend act on swarm builds (Tier B), and the editor says so on those rows. A chat-level Plan → Build hand-off button (the Copilot pattern) is a later, separately measured feature.

---

## 12. Invariants this design touches, and how each is kept

| Invariant | Kept by |
|---|---|
| Gate 1: no silent substitution | A chain exhausted is a loud refusal naming every entry. "Not measured yet" replaces any guessed load time. Unknown Macs are kept and shown. Tier A prints what it cannot express |
| Gate 2: specific text | The role and task text reaching models is unchanged. Tier B's role routing adds no prompt text |
| Gate 3: the benchmark launch | Strategies never touch the global `swarm` block. Per-spawn env only |
| Gate 4: reaping | The loader stops through the existing unmount and stop paths |
| Gate 5: no time inputs | Every loader wait is a lease or status event. Load durations are displayed, never decisions |
| Gate 6: one door | Node definitions and strategies go through `nodes/write` only. The pool projection is `project()` only. PlacementCard and the loader share one choreography after S5 |
| Gate 10: no absolutes | Weights are user ratios. The file-role table is named and tested. No new numeric const |
| Q-128 model identity | Every node-to-way match uses `node_names_model` / `nodeNamesModel` |
| The one fit rule | The loader, the strategy view and the cards read `fit.rs` verdicts. None recomputes them |
| One glance | Cards and the glance share `engineFigures`. No new poller |
| Owner UI rules | The harness assertions in §10.2 plus the critic's read |
