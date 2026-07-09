All load-bearing claims verified against the repo. Confirmed: `LOCAL_PROVIDER_FRAGMENTS = ["lmstudio","ollama","swarm","llama","localai"]` with bare `p.contains()` (edition.rs:35,49); Rust `resolve_edition_from` already auto-derives Local from a local provider (edition.rs:77) and reads config.yaml (:86), while desktop stamps the skin synchronously pre-paint from localStorage (renderer.tsx:13); `run_swarm` targets `std::env::current_dir()` (swarm.rs:8826) but the dispatcher writes the real tree to `self.working_dir` and reads existing files via `existing_files_manifest` (:4000); workers are `SessionType::Hidden` (:51) filtered out of listings (:1844); the Swarm settings tab is gated on `isLocalEdition` (SettingsView.tsx:192,258). Here is the synthesized plan.

---

# Making the Swarm a First-Class Capability in goose Desktop — Definitive Design + Build Plan

## What the adversarial pass changed (read this first)

Three headline design positions from the scouts did **not** survive contact and are changed below:

1. **Scout said:** register `swarm` as a provider and it becomes a transparent, selectable chat model with "zero new UI code." **Changed to:** register the provider (picker presence really is free), but **do not present or behave as a transparent chat model.** Selecting Swarm flips the chat pane into an honest **"build run" surface**. The Provider contract is a stateless, per-turn *chat* shape; the swarm is a stateful, minutes-long, file-writing *batch builder*. Forcing "you are now talking to GPT" onto it is the trap the attack proved. We keep the seam, drop the pretense.
2. **Scout floated:** auto-derive Edition from the selected provider (Option 3). **Changed to:** keep Edition an **explicit user preference, like Theme.** Auto-derive is a granularity error — a global synchronous document skin can't be derived from a per-session, async-resolved provider pick without four provable defects (skin never flips for the one session on the swarm, LOCAL badge becomes a privacy lie, multi-window unresolvable, first-paint flash reintroduced).
3. **Scout floated:** surface exploratory swarm-gym runs as chat sessions. **Changed to:** a **dedicated "Runs" surface**; workers stay `Hidden`; only a *user-initiated* swarm turn becomes one real session. Reclassifying reverses a deliberate `SessionType::Hidden` decision and floods history with message-less, often byte-identical eval artifacts.

Additionally added (not in scouts): an **`is_local` metadata flag on `ProviderDef`** to replace the bare-substring `LOCAL_PROVIDER_FRAGMENTS` match (which mislabels any hosted `llama`/`localai` alias as local), and a **single async dispatch/collect engine** shared by both the provider adapter and the MCP tools so the risky streaming seam is built once.

---

## 1. HOW SWARM BECOMES A SELECTABLE CHOICE

### Recommended architecture: subprocess-backed `swarm` Provider, reframed as a run surface — with the async engine shared with MCP

**Vehicle for "selectable":** a real `swarm` provider registered in `providers/init.rs`. It appears automatically as one entry in the flat provider dropdown in `SwitchModelModal.tsx:481-498` (CLI `configure.rs:704` gets it too) with no picker plumbing. This is the *only* architecture that literally lands "Swarm" in the picker, and the registration seam is genuinely free.

**Why subprocess, not in-process (non-negotiable):** `run_swarm` mutates process-global state — `std::env::set_var(LMSTUDIO_HOST, GOOSE_LOCAL_CONTEXT_CAP)` and `std::env::current_dir()` (swarm.rs:8811-8826). The orchestration also lives in `goose-cli`, which depends on `goose` core, so a core-crate provider can't call it in-process without a dependency cycle. The provider therefore spawns `goose swarm run --output-format json` as a subprocess and translates its NDJSON into `Message` chunks — the **exact pattern `ClaudeCodeProvider` already ships** (`claude_code.rs` stream()→spawn→translate, `manages_own_context()=true`). This is proven precedent, not invention.

**Why it survives the "wrong shape" attack — it stops pretending to be chat.** The ten failure modes the attack raised all stem from one root: the contract is per-turn streaming chat. We neutralize each by reframing the affordance, not by fighting it:

| Attack failure | How the design survives |
|---|---|
| Multi-turn chat structurally broken (no `--resume`) | **Turns are build iterations, not conversation.** Turn 1 scaffolds into the project dir; turn 2 ("add auth") re-runs `run_swarm` against the **same working_dir**, which now contains the prior tree — the planner already reads `existing_files_manifest(&self.working_dir)` (swarm.rs:4000). Continuity lives in the *artifact on disk*, not in flattened chat history. Honest and grounded. **(medium confidence — the planner uses the manifest, but "treats it as continuity" is not yet proven; flag.)** |
| Writes files to server cwd, not chat's dir | **Thread the working dir.** The subprocess `current_dir` is set from the session's `agent-working-dir` header (`session_context.rs`). This is the single most important correctness fix — without it, typing into a Swarm chat scaffolds a project into an unpredictable directory. |
| Usage/cost/context meters unpopulatable | **Report honest unknown, never fake zeros.** The adapter emits `ProviderUsage` with `None`/sentinel token counts; the UI shows `—` for Swarm (see §4). No garbage cost readout. |
| Every incidental completion becomes a fleet run | **Stub every side path.** Mirror `claude_code`'s `is_session_description_request` short-circuit (claude_code.rs:722) for session-title, tool-call-summary, and fast-model helper requests — return a cheap local heuristic string, **never** spawn the fleet. Any missed guard = a 5–15 min build to name a tab, so this is a hard checklist, not best-effort. |
| Cancellation leaks GPU (reboot-to-clear) | `Drop`→`start_kill` the child (claude_code.rs:247) **plus a fleet-abort** that unloads in-flight remote generations. **(LOW confidence — see risk below; the operator's own note is that a crashed run leaks wired GPU memory needing a reboot. I cannot yet guarantee a clean abort.)** |
| No queue for a shared physical fleet | **A cross-run fleet lock.** A file/OS lock serializes swarm runs; a second concurrent selection queues or is rejected with an honest "fleet busy" message rather than thrashing into "Model is unloaded." |
| Errors collapse to one red toast | The run surface renders `RunReport` (done/failed/per-device) inline (§3/§4); partial success (`failed` non-empty) becomes a real structured summary, not a toast. |
| Model second-stage is a lie | The "model" axis for Swarm = a **named swarm profile** (or the planner model), not `GOOSE_MODEL`. The model Select lists profiles, not phantom model ids. |

### Where it appears in the picker

One entry, **"Swarm,"** in the existing flat provider dropdown (`SwitchModelModal.tsx:485`) — but visually marked as a *capability*, not a cloud model (distinct icon + a one-line affordance note "Runs your local fleet and writes files to this project"). It must **not** read as "one more Anthropic/OpenAI row"; the attack is right that hiding a fleet-launcher in a flat list of chat models mis-sets every expectation.

### The complementary path (build the engine once): MCP `swarm serve` with async dispatch/collect

The scaffold `swarm_serve.rs` already exists but defers `swarm_dispatch`/`swarm_collect` (the sync path dies at `DEFAULT_EXTENSION_TIMEOUT = 300s`). Build these: `swarm_dispatch` returns a run handle immediately; `swarm_collect` polls/streams progress. This gives the honest "delegate a big build to the swarm **while chatting with a normal, correctly-metered model**" affordance. **Crucially, the provider adapter and these MCP tools drive the same run-handle/progress substrate** — so the risky async streaming seam is written and hardened once.

### Honest UX (streaming / latency)

Selecting Swarm and submitting a brief does **not** stream tokens like a chat. It streams **run progress**: plan → fan-out across devices → per-task green/red → integrate/verify/review → final `RunReport`. First useful output is seconds (the plan); full completion is minutes. The "assistant message" is a **live fan-in run card** (device tiles + task states) that resolves into a written-files summary. This is a fundamentally different affordance than a chat model, and the UI says so.

**Confidence:** picker presence + provider registration — **HIGH**. The streaming adapter (cancellation/fleet-abort, concurrency lock, honest multi-turn refine) — **LOW**, and this is where a subtle bug hides. Do not ship the provider as "done" until the abort path is proven to not leak GPU on a real fleet.

---

## 2. EDITION DECISION

### Keep it explicit (like Theme). Drop auto-derive. Sever "LOCAL" from any locality claim.

**Decision: KEEP an explicit, user-controlled Edition.** `EditionSelector` already mirrors the Theme pattern; keep the synchronous pre-paint stamp (`renderer.tsx:13`) exactly as-is. Reject the desktop Option-3 auto-derive — it produces the headline defect that the *one* session actually running on the swarm never gets the swarm skin (a per-session pick never touches the global `currentProvider` state, `ModelAndProviderContext.tsx:116-119`), plus a privacy lie, multi-window contradiction, and the first-paint flash the code exists to prevent.

Note the current inconsistency this exposes: Rust `resolve_edition_from` **already auto-derives** Local from a local provider name (edition.rs:77), while desktop is a manual toggle. We are standardizing on **explicit** and making the desktop the source of truth (see store fix).

### What gates BEHAVIOR vs what gates LOOK

| Concern | Gated by | Rule |
|---|---|---|
| Colors / skin / badge presence | **Edition (explicit pref)** | Presentation only. Never gates capability — preserve the `edition.rs` invariant. |
| Whether Swarm is selectable | **Provider registered + `is_configured` (fleet reachable)** | Decoupled from Edition entirely. |
| Whether the Swarm config/Runs surface is reachable | **Provider registered/configured** | Decoupled from Edition — fixes the chicken-and-egg (today the Swarm tab is hidden behind `isLocalEdition`, SettingsView.tsx:192, so you must already be on the swarm to find how to configure it). |
| The "LOCAL" badge | **A real locality signal** | See below — must not be a provider-name substring. |

### Two required fixes to Edition mechanics

1. **Kill the substring footgun / privacy lie.** `LOCAL_PROVIDER_FRAGMENTS` with bare `p.contains(frag)` (edition.rs:35,49) mislabels any hosted `llama` or `localai`-branded gateway as local. Replace with an **`is_local: bool` on `ProviderDef` metadata** (set truthfully per provider); derive the badge from that, or better, from the resolved endpoint being loopback/LAN. Either way, **"LOCAL" must stop being a name-guess** — it currently reads to users as "nothing leaves my machine," which is a false claim the moment a cloud provider name contains a fragment.
2. **Fix the store divergence.** Desktop writes Edition only to Electron `settings.json` (`main.ts:1931`, `EditionContext.tsx:89`); Rust reads it from `config.yaml` (`edition.rs:86`). They silently disagree today. Make **config.yaml canonical**: the desktop writes the `edition` key there (or Rust reads the Electron setting). This is a prerequisite for any Edition change to be trustworthy.

### What UI moves / is removed

- **Removed:** the Option-3 auto-derive proposal and any "auto" tri-state (which `resolve_edition_from` would treat as unknown→fall-through anyway).
- **Moved:** the Swarm settings tab stops being `isLocalEdition`-gated and becomes a first-class nav destination (see §4).
- **Optional nudge (not silent):** if wanted, a one-time confirm dialog ("You picked a local provider — switch to Local Edition?") — a custom dialog, never a native `confirm`, never a silent global re-skin.

**Confidence:** HIGH. These are contained, well-grounded changes.

---

## 3. SESSIONS / EXPLORATORY-IN-UI

### Keep swarm-gym runs and workers OUT of chat history. Give runs a dedicated surface.

**Do not reclassify.** swarm-gym runs (`evals/swarm-gym/harness/runner.py`) are a Python test harness that shells `goose swarm run --output-format json` into gitignored `apps/<slug>/` and `runs/`, driven by Claude Code or a local judge — not conversations. Each run fans out to N worker sessions that are deliberately `SessionType::Hidden` (`session_manager.rs:51`) so they stay out of listings (the Sessions list is filtered to `User+Scheduled`, `session_manager.rs:1844`). Surfacing them would dump dozens of message-less traces per run, show FROZEN-suite replays as near-duplicate "chats," and offer "resume" on dead workers.

### Mechanism + UI (three distinct surfaces)

1. **A "Runs" nav destination (new).** A dedicated read-only view — *not* the Sessions list — backed by the artifacts runs **already emit**: `report.html`, `ledger.sqlite`, and `runs/`. Each row = one swarm-gym or user run: status, device fan-out, done/failed counts, links to the report. Read-only, not resumable. This is where exploratory/benchmark runs live.
2. **Live sub-agent activity inside a run.** The swarm already writes per-worker heartbeats to `.swarm/activity/<task_id>.json` (swarm.rs:3309) and a `.swarm/run-<id>.jsonl` event log. The Runs view (and the in-chat run card) reads these to render fan-out **within one card** — workers appear as expandable tiles inside their parent run, **never as sidebar rows**. They stay `Hidden` at the data layer.
3. **A user-initiated Swarm turn = exactly ONE `User` session.** When a user picks Swarm in a chat and submits a brief, that chat is one real session in the normal Sessions list, with the **FanInCard rendered inline** as the assistant response. Workers spawned by that run stay `Hidden` and are surfaced only as sub-activity inside the card. This is the single canonical session a swarm run maps onto — there is no attempt to map the N hidden workers onto history.

**Confidence:** HIGH on the "keep Hidden + dedicated Runs view" decision. MEDIUM on the live-heartbeat wiring (the data exists; the render is new work).

---

## 4. UI CLEANUP — coherent end-to-end flow

### Primary entry point

**The model picker.** "Swarm" is a selectable provider (§1) — that is the first-class, discoverable entry the requirement asks for. Secondary/support entry: a **"Swarm" nav destination** in the sidebar consolidating configuration, fleet status, and the Runs view.

### What consolidates / moves

- **`SwarmSettingsSection`** (today an edition-gated Settings tab) → promoted to the **"Swarm" sidebar destination** with three panes: **Configure** (tunables/fleet endpoint), **Fleet** (live device status), **Runs** (§3). No longer hidden behind `isLocalEdition`.
- **`FanInCard`** (today only a static preview in `AppSettingsSection.tsx:499`) → becomes the **real live in-chat run component** and the live element of a Runs detail view. Single component, two mount points.
- The in-chat model readout (`ModelsBottomBar`) shows "Swarm" with the capability icon and, during a run, a compact progress affordance linking to the full card.

### End-to-end flow

Pick "Swarm" in the picker → chat pane reframes to a build surface with the honest affordance note → user submits a brief → live FanInCard streams plan/fan-out/per-device/verify → resolves to a `RunReport` summary with written-files list and a link into Runs → follow-up turn refines in the same project dir. Delegation variant: keep a normal chat model, call the swarm via the MCP `swarm_dispatch`/`swarm_collect` tools, and watch the same card render from tool output.

### Hard-UI-rule compliance (mandatory, per standing rules)

- **No left accent rail / left border-strip** on the run card, device tiles, task rows, or Runs list items. Emphasis via full borders, solid fills, icons, bold numerics — never a left rail.
- **No faded/washed tints.** Device and task status use **solid, saturated** fills — vivid green (done), strong red (failed), bold amber (running). When color-coding N devices, use a **full rainbow of distinct solid hues**, not pastel washes.
- **No native primitives.** The picker is already a custom `Select` (keep it). "Stop run" uses a **custom dialog**, never `window.confirm`. No native `<select>` anywhere in the Swarm surface.
- **Honest, not decorative, numbers.** Usage/cost for Swarm renders `—` (unknown), never a fake `0`. **(needs-decision: confirm the cost/context meters tolerate `None` without rendering `$0.00` or a broken context bar — if not, hide the meter for Swarm sessions.)**

---

## 5. SEQUENCED BUILD PLAN — ranked by correctness risk, confidence-flagged

Ranking is by **correctness risk, not effort.** Big/small noted only as blast radius.

### Tier 0 — Safe to build now, HIGH confidence, do first (correctness-cheap, unblocks the rest)

1. **Register a `swarm` `ProviderDef`** in `providers/init.rs` with an `is_local: true` metadata flag; gate `is_configured` on fleet reachability (mirror how `SwarmSettingsSection` probes the fleet). *Blast radius: small, additive.* Gets "Swarm" into both pickers.
2. **Add `is_local` to `ProviderDef`; replace `LOCAL_PROVIDER_FRAGMENTS` bare-substring match** (edition.rs:35,49) with the metadata flag. Kills the mislabel/privacy footgun. *Small, but touches core-crate `edition.rs` — gate the build.*
3. **Fix the Edition store divergence** — make `config.yaml` `edition` canonical from the desktop. *Small, correctness-critical prerequisite for anything Edition-related.*
4. **Decouple the Swarm surface from Edition** — un-gate the tab from `isLocalEdition`, promote to a sidebar destination gated on provider-configured. *Small–medium, front-end only.*
5. **Keep Edition explicit; delete the Option-3 auto-derive proposal.** *No-op removal / decision.*
6. **Dedicated "Runs" view** over existing `report.html` + `ledger.sqlite` + `runs/`; workers stay `Hidden`. *Medium, front-end + a read endpoint, no core risk.*

### Tier 1 — Needs the async engine, MEDIUM confidence

7. **Build the shared async dispatch/collect engine** (run handle + progress stream over `.swarm/activity` heartbeats + `run-<id>.jsonl`). This is the substrate both the MCP tools and the provider adapter consume. *Medium. Batch-shaped, matches the swarm's nature — this is the honest core.*
8. **MCP `swarm_dispatch` / `swarm_collect`** on the `swarm serve` scaffold (beats the 300s `DEFAULT_EXTENSION_TIMEOUT`). *Medium. This is the higher-confidence "delegate while chatting a normal model" path — arguably the safest way to expose swarm value even if Tier 2 slips.*
9. **Thread `agent-working-dir` → subprocess `current_dir`.** *Medium, but a hard correctness gate — without it, Swarm scaffolds into the wrong directory (swarm.rs:8826 uses process cwd).*
10. **Live `FanInCard` wiring** from heartbeats; single component, two mounts. *Medium, front-end.*

### Tier 2 — The provider streaming adapter, LOW confidence — flag bluntly, verify adversarially before "done"

11. **The `swarm` `Provider::stream` adapter** (subprocess-backed, `manages_own_context=true`). This is where the concentrated correctness risk lives. Ship only with all of:
    - **Cancellation → child kill + fleet-abort.** **LOWEST confidence in the whole plan.** The operator's own note is that a crashed/aborted run leaks wired GPU memory requiring a reboot. `Drop`→`start_kill` aborts the local client; it does **not** by itself unload in-flight remote generations. I am not confident I can guarantee a clean abort on the first pass — this needs to be proven on the real fleet, and until then "stop" is a known hazard.
    - **Cross-run fleet lock/queue.** Concurrency policy is a **decision** (serialize vs queue vs reject-second). Without it, two Swarm windows thrash the shared GPU.
    - **Incidental-completion stubs** for session-title / tool-call-summary / fast-model — a hard checklist; any miss spawns a multi-minute fleet run to name a tab.
    - **Honest multi-turn refine semantics** via `existing_files_manifest`. **Medium-low** — grounded in swarm.rs:4000 but unproven that the planner treats prior files as continuity rather than re-planning.
    - **Honest usage (`—`, not `0`).**
    *Blast radius: touches core-crate provider registration + a new adapter; gate builds, verify on a real fleet, do not claim done off a compile.*

### Needs a decision before building

- **Is the picker entry non-negotiable, or is MCP-delegation an acceptable primary?** Recommendation: ship **both** — Tier 0/1 (picker presence + MCP engine) delivers the requirement and value at higher confidence; Tier 2 (transparent-run provider) is the lower-confidence layer. Do not let Tier 2's risk block Tier 0/1 value.
- **Fleet concurrency policy** (serialize / queue / reject).
- **Usage meter** — render `—` (needs UI `None` tolerance) vs hide for Swarm.
- **Follow-up turn** — refine-in-place (same working_dir) vs always-fresh. Recommendation: refine-in-place, but validate the planner honors the manifest.

### Bottom line

Registering the provider makes "Swarm" selectable for free and satisfies the requirement — but "free to register" is not "safe to select as a chat model." The honest, survivable design is: **provider entry for discoverability + a reframed build-run UX that never pretends to be chat + a batch-shaped async engine shared with MCP.** Edition stays explicit (Theme-like) with a truthful locality signal; exploratory runs get their own Runs surface while workers stay Hidden. The one piece I would not sign off without real-fleet verification is the provider's cancellation/fleet-abort path — that is the LOW-confidence seam where a GPU-leak bug hides.
---
## CANCELLATION SEAM — RESOLVED (empirical, 2026-07-09)
The LOW-confidence "GPU-leak on cancel" was MISATTRIBUTED. The swarm uses LM Studio over HTTP (create("lmstudio"), /v1, swarm.rs:3193) — NOT the JACCL tensor-parallel cluster the reboot-to-clear note is about. EMPIRICAL TEST (scratchpad/abort_test.py): fired a long generation on mihai (6 chunks/4s = generating), aborted mid-stream by closing the connection, probed immediately -> 0.5s response = node INSTANTLY FREE. LM Studio stops generation on client disconnect; no leak. => swarm provider Drop->kill-child->connections-close->LM Studio-stops is a CLEAN cancellation. Confidence raised LOW->HIGH.

---
## EXPLORATION LOOP + REFLECT-IN-UI — WORKING (2026-07-09)
invtrack (1st exploratory of the reflected loop): FULL PASS by running — 66 tests, golden (widget 6 not-LOW / bolt LOW), low=bolt-only, robustness 4/4 clean (H1 item-not-found, H2 malformed argparse + corrupt-db EXPLICIT 'Corrupted store', H3 below-zero clamped-to-0+clean-message). Baseline quality; mold intact on warm fleet. REFLECTED: explore-invtrack user session created in sessions.db (id 8d9d212f, 2 msgs brief+verdict) -> appears in desktop CHATS; 825 workers stay Hidden. reflect_run.py mechanism PROVEN end-to-end. Harness stuck-idle post-complete (pre_review) -> verified by running + torn down.
gradebook FULL PASS (54 tests, golden Ann 85=B/Cyril 90=A, robustness 6/6 incl no-ZeroDiv + out-of-range reject + corrupt-db explicit) -> reflected as explore-gradebook. 5 apps all full pass.
