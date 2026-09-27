# Session loops: design for lane L (Q-227, Q-228)

Written 2026-09-27 by the lane L architect. The code was read at `a781b6b29` (main). The installed app (3.0.62) was
walked read-only over CDP while E2E #3l ran in a chat; nothing was created, started or deleted, every dialog opened
was closed, and the window was returned to the chat URL it was found on (`#/pair?resumeSessionId=20260927_8`).
Screenshots: `~/goose-screenshots/loops-design/` (00 chat as found, 01 Recipes & loops dialog, 02 Changes rail open,
03 Agent Work row expanded, 04 an Agent Work desk, 05 Create an agent, 06 back in the chat). No product code was
written; implementers cut from this document.

Abbreviations: `S` = `ui/desktop/src`, `G` = `crates/goose/src`, `AW` = `crates/goose-cli/src/commands/swarm/agent_work`.

---

## 0. What this document decides, in fifteen lines

1. **A session loop is a chat that re-runs a stated goal, tick after tick, in the same session.** One tick is one
   ordinary agent turn in that chat, sent through the same prompt door a typed message uses. The loop owns: the goal
   (the user's words), a cadence, a state file, stop rules, and a tick ledger.
2. **Three cadences:** "Every 10m" (fixed, time between tick starts, an overrunning tick is never cut), "When goose
   decides" (self-paced: each tick names its next delay and why), "Right after each tick" (back to back).
3. **Each tick ends by calling one tool, `loop_report`** (verdict, summary, next step, and for self-paced the delay).
   A named check command, when set, decides "goal met"; the model's "done" alone ends the loop only when no check is
   set, and says so. A missing report, a missing delay, a check that cannot run: each is a named, visible state, never a default.
4. **The loop pauses itself on repeats, not counts:** the same failure twice in a row, two ticks in a row with no report,
   or a tick that changed no file and named the same next step as the tick before it (stalled).
5. **The user's turn always wins.** A tick never starts while a user turn runs. A user turn that starts in another
   chat cancels the running tick through the Stop door, and the tick continues after that turn. A message typed
   into the loop's own chat during a tick steers the tick (today's steer path).
6. **The loop lives in the chat's right rail, beside Changes.** The rail stays an overlay (Q-190: it never narrows the
   chat). Collapsed, it is two pills. Open, it has two tabs, Loop and Changes. The Loop tab shows the loop's status,
   its next tick, the controls, and a tick timeline. Each tick row shows its verdict and summary, plus the files that
   tick changed, computed by the same `sessionChanges` model over that tick's messages.
7. **You start a loop in one of three ways:** a Loop button in the composer, the `/loop` command, or "Loop this" on a
   message. Four starting templates: Software quality loop (discover → critique → fix → prove), Until a check
   passes, Watch and act, and Blank.
8. **The clock lives in goosed (one process-wide runner); the window's renderer fires the tick** through
   `acpChatSessionController.submitMessage`, the same door a typed message uses. The runner owns the record, the
   prompt text and every decision.
9. **Honest limits.** Ticks run only while the chat's window is open, because each window runs its own goosed. A
   window closed mid-loop comes back as "Paused, goose was closed". While the Mac sleeps no tick runs: on wake, one
   tick runs rather than a burst, and "Keep awake" is the existing wakelock setting.
10. **Q-227: the composer's "Recipes & loops" button and its whole dialog are removed.** Evidence of use on both Macs:
    0 schedules, 0 saved recipes, 0 recipe-driven user sessions out of 387, and the last scheduled session was
    2026-07-13. The recipe ENGINE is untouched (sub-recipes, CLI, deeplinks, the agent's schedule tool). The URL-only
    `/loop` view goes too, so "loop" has one meaning. The scheduler's `LoopConfig` is retired in its own slice.
11. **Agent Work stays the desk** (a multi-lane, fleet-wide, detached process), so there is one loop concept, not
    two engines. Session loops and desks share ONE clock, moved from `AW/window.rs` into the goose crate, and one
    vocabulary: tick, cadence, overdue, "Run a tick now", pause, stop.
12. **The nodes seam (§5.5):** a tick is a reply through `on_prompt`, so it runs on the session's chip (`node:` /
    `strategy:` / Auto), is covered by the S5 loader's per-reply guard, and gets a `nodes.served` record that its
    tick row shows. The loop never loads or swaps a model itself.
13. **Slices, in data order, with confidence:** L1 remove the button and dialog (high) ∥ L6 one clock (high) → L0
    contract and pure rules (high) → L2 runner and door (**low-medium**) ∥ L3 the report tool and `/loop` (medium) →
    L4 start UI (medium-high) ∥ L5 the rail (medium-high) ∥ L7 sidebar and activity (medium) → L8 retire
    `LoopConfig` (medium-high) → L9 harness and live proof (medium).
14. **No new time input decides model work:** a cadence decides only when a tick STARTS; nothing cuts a tick or a
    check. Gates 1, 2, 5 and 10 are kept (§12).
15. **Open questions** carry recommendations and the work proceeds on them (§11). The largest: where the state file
    lives (recommended `.goose/loops/<slug>/NOW.md` in the working dir).

---

## 1. The owner's ask, as requirements

| # | Requirement | His words |
|---|---|---|
| R1 | Decide the fate of the composer's "Recipes & loops" button and dialog | "this might not be required anymore … we no longer use recipes … I am not sure this is still required. even as a button in the bottom layer" |
| R2 | Loops are for software building, not only Agent Work | "loops are not only required for agent work but rather for other pieces of work too like software building" |
| R3 | The loop can do what this quality loop does: discover, criticise, fix, on a tick | "especially if you want to implement something like we have now where we're discovering, criticizing etc." |
| R4 | Claude-Code-like ticking over work | "in claude code we do have this capability of going over work on tick" |
| R5 | A software-building session's loop shows on the right side, where the file modifications are | "it should show up in the right side where we have the file modifications too" |
| R6 | A good way to implement AND use loops, thought through as UX | "Can you give this UX a thought through as well, design a good way to implement and use loops" |

What his reference loop (this quality loop, run from Claude Code) actually has, which R3 and R4 imply:
- a heartbeat, which is a cron every 10 minutes;
- a self-paced `/loop`;
- a state spine (NOW, BACKLOG) rewritten every tick (`local-edition/mlx/quality/NOW.md:1`: "rewritten every tick, ≤ 60 lines");
- stop rules;
- a tick that runs discover → critique → fix → prove;
- a user who can steer at any time, and whose own messages go first.

§4 maps each of these onto the product.

---

## 2. What exists today (mapped at `a781b6b29`)

### 2.1 The composer's "Recipes & loops" button, and what it actually leads to

- **The button** renders only when the session's provider is `swarm` and the bottom bar is not narrow:
  `isSwarmProvider` (`S/components/ChatInput.tsx:328`), render at `:1784-1794`, test id `recipes-and-loops`,
  state `agentWizardOpen` (`:336`), dialog mount `:1796-1803`, import `:48`. Its label and tooltip are the only
  translated strings (`ChatInput.tsx:178-184`).
- **The dialog** is `S/components/swarm/AgentSetupWizard.tsx` (360 lines). Every string inside it is hard-coded
  English: the title at `:141`, the buttons at `:186`, `:201` and `:216`, "available." at `:290`, "Manage" at
  `:304`. On open it fetches the skill count and the scheduler's jobs that carry a `loopConfig` (`:64-84`, `:75-76`).
  Walk: `01-recipes-loops-dialog.png`.
- **What each action does:**
  - **Build a recipe with the fleet (chat)** (`:175`) opens `RecipeChatWizard.tsx` (534 lines). It is not an ACP
    session. It sends raw chat completions over IPC `fleet-chat` (`RecipeChatWizard.tsx:205` →
    `S/preload.ts:594` → `S/main.ts:2105` → `S/utils/fleetIpc.ts:37`) to a target picked by
    `recipeChatTarget.ts`. Save calls ACP `recipes/save` (`RecipeChatWizard.tsx:311`).
  - **…or draft one by hand** (`:191`) opens `RecipeWizard.tsx` (177 lines): title, description and instructions,
    then `saveRecipe`. Its footer says "Saved to Recipes — then create a loop from it." (`RecipeWizard.tsx:161`),
    but no picker in the loop form reads a saved recipe (defect D3).
  - **Create a loop (recipe + schedule + iterations)** (`:206`) opens `S/components/loop/LoopModal.tsx` (409 lines):
    - the recipe comes from a YAML file or a `goose://recipe` deeplink only (`:71`, `:144-160`);
    - cron defaults to `0 0 14 * * *` and max iterations to 10 (`:90-91`);
    - there is an optional stop-check command and state file (`:200-209`).
    Submit calls `acpCreateSchedule` (`AgentSetupWizard.tsx:92`), which writes into the scheduler (§2.3).
  - **Run / Run agent now** (`:232`, `:255`) calls `acpRunScheduleNow` (`:107`). `Scheduler::run_now` awaits the whole
    job, every iteration, before returning (`G/scheduler.rs:688-695`), so the "started" toast fires only once it has
    finished (`AgentSetupWizard.tsx:107-109`, defect D1).
  - **Manage (skills)** (`:295`) calls `setView('skills')` → `/skills` (`AgentSetupWizard.tsx:115-117`). Skills is
    already in the left nav (`S/hooks/useNavigationItems.ts:44-55`), so this shortcut duplicates the nav.
- **Reachability.** This button is the only in-app door to recipes and loops. `/recipes`, `/schedules` and `/loop`
  have no nav entry ("Recipes, Apps, Scheduler, Loop, Extensions and Session History left the nav on purpose — their
  routes and views stay in code and reachable by URL", `useNavigationItems.ts:20-21`). No `setView`/`navigate` call
  reaches them either. The only other reference is the new-window route map, which has no `loop` entry (`main.ts:1549-1559`).

### 2.2 Recipes: the engine is load-bearing, the desktop authoring is not

- **Engine, all used, none touched by this design:**
  - `G/recipe/`, deeplinks `recipe_deeplink.rs:13,19`, ACP handlers `G/acp/server/recipe/mod.rs:97-323`;
  - summon's sub-recipes and delegate (`G/agents/platform_extensions/summon.rs:10-12, :675, :1315`);
  - recipe slash commands (`G/slash_commands/recipe_slash_command.rs:11-12`);
  - the scheduler (`G/scheduler.rs:875`);
  - the agent's `platform__manage_schedule` tool (`G/agents/agent.rs:1307-1314`);
  - the CLI `goose run --recipe` / `goose recipe` / `goose schedule` (`crates/goose-cli/src/cli.rs:244-250, :1002, :1042`);
  - `goose-self-test.yaml` (a recipe that uses summon, `:122`).
- **Swarm and Agent Work use only the `goose::recipe::Response` output type** (`swarm.rs:18`, `AW/tick.rs:20`),
  not recipes.
- **Desktop recipe code that survives this design:**
  - `RecipesView` at `/recipes` (URL-only; `App.tsx:822`);
  - "create recipe from session" (`S/components/SessionActionsHeader.tsx:17, :363`);
  - `RecipeActivities` (`BaseChat.tsx:26, :608`);
  - the settings import (`GooseImportSection.tsx:120-129`);
  - `goose://recipe` deeplinks (`main.ts:607, :789, :828, :877`).

### 2.3 The scheduler, and the recipe loop inside it

- **The model.** `LoopConfig { max_iterations, stop_check, state_artifact }` (`G/scheduler.rs:107-117`) is a field
  of `ScheduledJob` (`:141-142`). `execute_job` (`:862`) runs a looped job's iterations **back to back inside one
  cron fire** (`:898-932`), so "schedule + iterations" means N runs in a row at 14:00, not one per schedule.
- **Each iteration is a new `Agent` and a new `SessionType::Scheduled` session** (`:941-964`). It runs on:
  - the **global** provider and model (`:952-953`), not the chat's node;
  - `std::env::current_dir()` as its working dir (`:961`), which is goosed's cwd, not the recipe's folder.
- **The stop check** goes through `execute_success_checks` with a 300 s timeout, and `.unwrap_or(false)`
  (`:918-926`) makes a check that could not run read as "not passed" (defect D4, the gate-1 shape).
- **Where the scheduler runs.**
  - It runs inside goosed and is built lazily per ACP server (`G/acp/server_factory.rs:67-84`), with jobs kept in
    `<data_dir>/schedule.json` (`:76`).
  - The desktop runs one goosed per window (`main.ts:2391`), released when the window closes and cleaned up on quit
    (`main.ts:6782-6786`). So schedules do not fire while the app is closed. The CLI says so itself
    (`crates/goose-cli/src/commands/schedule.rs:269-275`).
  - Several windows each load the same `schedule.json` and could fire one job twice (D7, not verified).
- **Who writes `loop_config`.** Only the desktop `LoopModal`, through ACP (`G/acp/server/schedule.rs:227`). The REST
  route (`crates/goose-server/src/routes/schedule.rs:148`), the CLI (`crates/goose-cli/src/commands/schedule.rs:94`)
  and the test fixture (`G/../tests/acp_fixtures/mod.rs:105`) always write `None`. The agent's schedule tool does
  not set it.
- **Seven doors create a schedule:**
  1. the dialog;
  2. LoopView (`S/components/loop/LoopView.tsx:191`);
  3. SchedulesView (`:299`);
  4. RecipesView (`:562`);
  5. the agent tool (`G/agents/schedule_tool.rs:79`);
  6. the CLI;
  7. REST.

  Removing doors 1 and 2 leaves the product's own writers (5, 6, 7) and the two URL-only views.

### 2.4 Agent Work: the desk (used, one desk registered on this Mac)

- **The definition** is `agent.yaml`. Its fields cover name, charter, timezone, window, cadence, guard, poll, surgeons,
  review, post, and more (`AW/manifest.rs:17-67`). "Cadence" is the time between tick STARTS; a tick that overruns
  starts the next as soon as it ends and is never cut (`AW/manifest.rs:31-32`). The default is `30m` (`:149-150`).
  **No stop condition exists**: no max ticks, no budget, no "until".
- **Runtime.**
  - The desk runs as a separate CLI process, `goose swarm agent run <dir>`, spawned by main (`main.ts:7005-7016`,
    args `:7009`).
  - The serve loop is `AW/mod.rs:167-353`. The next tick comes from `DeskClock::next_tick` (`AW/window.rs:117-147`),
    which returns a reason: "first tick", "cadence", "overdue — the previous tick overran the cadence", or "desk closed".
  - Control is by flag files under `.swarm/agent/`: `paused`, `tick-now`, `stop` (`AW/mod.rs:164-166`), written by
    main's `agent-work-*` IPC (`main.ts:7034`, `:7052`, `:7059`).
  - A tick runs the phases guard → poll → orient → lanes → review → handoff → synthesis → post → close
    (`AW/tick.rs:162..757`).
- **The spawn is not tied to the app.** It is not detached, but `will-quit` (`main.ts:6772-6790`) never stops it, so a
  running desk outlives the app as an orphan and is not re-adopted on relaunch (D8).
- **UI.**
  - The sidebar section is `S/components/Layout/AgentWorkSection.tsx:328-333`; the view is at `/agent-work`
    (`App.tsx:824`).
  - The desk page (walk `04-agent-desk.png`) shows:
    - a status block ("Stopped · Not scheduled · start the desk to schedule one");
    - "every 30m · always open UTC" with "Run once" and "Start schedule";
    - counters (Ticks, Lanes run, Lane-minutes, Drafts staged, Posted, Asks raised);
    - "Nothing waits on you";
    - a phase ribbon per tick ("Guard 0s · Poll 0s · no poll command · Orient 52s · Lanes 2m 10s …");
    - Result, Next step, Sources, Lanes;
    - a ticks ledger ("#, OUTCOME, WHAT HAPPENED, LANES, STAGED, POSTED, LANE-MIN, WALL");
    - "TELL THE DESK — A note the orchestrator reads at its next tick".
  - New agent (walk `05-new-agent-dialog.png`) offers cadence presets 15m / 30m / 1h / 4h / daily = `24h`
    (`S/components/agent-work/NewAgentDialog.tsx:30-36`), validates `<n>s|m|h` (`:163`), and has a work window
    (days, from/to, always open).
- **Duplicated.** Agent Work keeps its own "needs you" (`S/components/agent-work/NeedsYou.tsx:6-12`, backed by
  `asks.json`), separate from the session needs-you store.

**Why a session loop is not an Agent Work desk.** A desk's tick is an orchestrator plus lanes on the fleet in another
process, writing to its own folder. It is not a chat: it has no conversation to steer and does not feed the chat's
Changes model. The owner's R5 needs the loop IN the chat.

**What a session loop takes from the desk:**
- the clock (`DeskClock`: cadence grammar, overdue, window);
- the start-to-start cadence semantics;
- the control verbs;
- the tick ledger's shape.

### 2.5 `/goal` and `/grind`: loops inside one reply

- The built-ins are `goal` and `grind` (`G/agents/execute_commands.rs:44-52`).
- Inside one reply, when the model would finish:
  - `/goal` injects a hidden "Before finishing, check whether the following goal has been fully met" once
    (`G/agents/agent.rs:2958-2975`);
  - `/grind` injects "Keep working. The grind goal is not yet complete" (`:2977-2990`) "until max_turns"
    (`execute_commands.rs:47-50`).
- Both are held in memory only (`agent.rs:273`, `:3298-3312`), not persisted.

They stay. A session loop works BETWEEN replies (cadence, state file, ledger); `/goal` works WITHIN one reply. §7.4
says how the two meet.

### 2.6 The Changes rail (Q-190), where R5 puts the loop

- **Mounting.** Mounted once in `BaseChat` (`S/components/BaseChat.tsx:666-673`) as `absolute right-4` with
  `LAYER.chrome` (`z-30`, `S/components/lz/tokens.ts:195`). Its top offset depends on local/mobile/nav (`:671`). The
  docblock: "takes no width from the conversation … opened it is an overlay panel over the chat's right side"
  (`S/components/changes/ChangesRail.tsx:36-42`).
- **Collapsed:** a pill "N files · +A −R" (`:74-92`, test id `changes-rail-pill`). **Open:** a `role="region"` panel
  `max-h-[65vh] w-[min(32rem,calc(100vw-2rem))]` (`:105`). Open state is plain `useState(false)` (`:52`) and is not
  persisted. Escape closes and returns focus (`:63-66`, `:101-103`). The rail renders nothing when no file changed (`:61`).
- **Data.** `sessionChanges(messages)` (`S/components/changes/fileDiff.ts:127-148`) is a flat pass over the tool
  responses' `fileDiff` (from the engine's `file_diff.rs` via `_meta.goose.fileDiff`, `G/acp/server.rs:2206,2231`),
  grouped per file in first-touched order. It has no per-turn grouping. **It is a pure function over a message
  array, so a slice of messages gives one tick's changes with no new derivation.**
- **Primitives.** There is no tabs primitive in the rail. `Segmented as="tabs"` exists (`S/components/lz/Segmented.tsx:34-39`),
  used with Radix tabs in `SettingsView.tsx:192-228`.
- **Walk.** In `02-changes-rail-expanded.png` (2056 px window) the open panel covers the right third of the chat's
  text: lines run under it ("which phase of support 9…" is cut). This follows from the overlay rule; §8.3 keeps the
  rule and names the cost.

### 2.7 Session state: running, needs you, background

- **ACP.** `_goose/unstable/session_activity/get` answers
  `SessionActivityResponse { running, needs_you, failed, stopped, background }`
  (`crates/goose-sdk-types/src/custom_requests/needs_you.rs:49-55, :86-99`), served by `G/acp/server/needs_you.rs`
  (busy set `:45-63`, background `:66-95`, `on_session_activity` `:97+`).
- **Needs you.** Needs-you items come only from `User` sessions (`G/needs_you.rs:80-90`), raised by the `ask_user`
  tool (`G/agents/platform_extensions/needs_you.rs:17,89`). **A tick is a turn in a User session, so a tick can ask the user.**
- **Background (Q-185).** `BackgroundWorkKind` (`needs_you.rs:104-127`) tags helper calls (fact check, title,
  compaction …) so the rows show "Checking". A tick is not background work: it is a real turn.
- **Desktop store.** `S/components/sessionActivity/sessionActivityStore.ts` polls every 5 s (`:56, :144`).
  - The states are `'running' | 'needs-you' | 'background' | 'failed' | 'stopped' | 'idle'` (`:221`), with
    precedence at `:229-236`.
  - "Active now" is needs-you or running (`:240-242`, rows `:305-336`, section `S/components/Layout/ActiveNowSection.tsx:24-32`).
- **Pills and cards.**
  - Pills live in `S/components/sessionActivity/ActivityPills.tsx`: Running `:83`, Background `:103` (secondary
    fill), NeedsYou `:129` (warn fill), Failed `:147` (err fill), Stopped `:166`. `SessionActivityMarker` is what
    `ProjectsSection`, `ProjectLanding` and `SessionListView` render.
  - The NeedsYou card is `S/components/sessionActivity/NeedsYouCard.tsx:201-237`, mounted at `BaseChat.tsx:677`.

### 2.8 How a turn starts, and what the server allows

- **Desktop door.** `acpPromptSession` (`S/acp/prompt.ts:6-15`) is called only by
  `acpChatSessionController.submitMessage` (`S/acp/chatSessionController.ts:149, :165`). Callers:
  - `useChatSession.handleSubmit` (`S/hooks/useChatSession.ts:151`);
  - the composer (`BaseChat.tsx:370-374, :697`);
  - `useAutoSubmit` (`S/hooks/useAutoSubmit.ts:83-110`);
  - the retry (`GooseMessage.tsx:194-241`);
  - `/compact` (`ChatInput.tsx:761`);
  - needs-you answers (`NeedsYouCard.tsx:235`).
- **Server door.** `on_prompt` (`G/acp/server.rs:2636-3022`):
  - It takes `turn_priority::user_turn()` for the life of the turn (`:2647`).
  - `start_active_run` refuses a second concurrent run on a session with "session already has active run …; use
    _goose/unstable/session/steer" (`:2473-2478`).
  - It detects slash commands (`:2675-2702`), calls `agent.reply` (`:2712`), and on Stop breaks and drops the stream
    (`:2752-2760`).
- **Steer.** A message sent while a run is active is steered into that run, not queued
  (`useChatSession.ts:217-240` → `acpSteerSession`, `S/acp/prompt.ts:22`; server `on_steer_session`, `:3023-3060`).
- **Sessions and connections.**
  - A session does not have to be open to receive a prompt: `get_session_agent` loads it (`:2424-2457`).
  - `GooseAcpAgent` is per connection. `client_cx` is set once per connection at dispatch (`G/acp/server/dispatch.rs:20-24`)
    and used for notifications outside a request (`server.rs:2442`, `:3050`).
  - Notifications go to that connection only.
  - `spawn_session_name_update_notifier(cx.clone())` (`server.rs:1072`) is the existing pattern for a notification
    the server sends on its own.
- **turn_priority (Q-132)** (`G/turn_priority.rs`):
  - `user_turn()` bumps `running` and `started` (`:48-54`).
  - `after_user_turns` runs a call when no user turn runs, and a user turn that starts drops it and re-asks it from
    the start (`:58-76`).
  - It is **process-local** (`:85-97`), and each window has its own goosed. So a user turn in window B does not reach
    window A's goosed (D9).
- **Session storage.**
  - `Session` has `working_dir`, `session_type`, `extension_data` and `project_id` (`G/session/session_manager.rs:62-96`).
  - `extension_data` is keyed `"name.version"` (`G/session/extension_data.rs:14-41`). It is read and written through
    the `ExtensionState` trait (`:44`) and persisted with `set_extension_state` (`session_manager.rs:484`).
  - The `todo.v0` key (`extension_data.rs:90`) is the model for a per-session record.
- **The todo extension.** `todo_write` (`G/agents/platform_extensions/todo.rs:91-106`) is a per-session scratchpad
  injected every turn (`:160-178`). It is a state spine that already exists, but it is invisible to the user (§4.5).

### 2.9 Evidence of use (read-only, 2026-09-27)

| Source | This Mac | workhorse |
|---|---|---|
| `~/.local/share/goose/schedule.json` | `[]` (no schedules, no loops) | empty |
| `~/.config/goose/recipes/` (saved recipes) | empty (dir from 2026-07-13) | 0 files |
| sessions by type (`sessions.db`) | user 387 · scheduled **2** (last 2026-07-13, the `looptest` recipe) · hidden 72,658 · sub_agent 197 | user 46 · hidden 187 |
| user sessions started from a recipe (`recipe_json` set) | **0** (the one row with a recipe is a hidden swarm planner, 2026-06-25) | — |
| `~/.config/goose/agent-work.json` | 1 desk ("Public web research", stopped, 1 tick, 1w ago) | — |

### 2.10 Defects found while mapping

| # | Defect | Evidence | Fate |
|---|---|---|---|
| D1 | "Run agent now" says "started" only after every iteration finished | `scheduler.rs:688-695`, `AgentSetupWizard.tsx:107-109` | removed with the dialog (L1) |
| D2 | The dialog's strings are not translated | `AgentSetupWizard.tsx:141, :186, :201, :216, :290, :304` | removed (L1) |
| D3 | "Saved to Recipes — then create a loop from it" but the loop form cannot pick a saved recipe | `RecipeWizard.tsx:161`, `LoopModal.tsx:71, :144-160` | removed (L1) |
| D4 | A stop check that cannot run reads as "not passed" and the loop continues | `scheduler.rs:926` `.unwrap_or(false)` | retired with `LoopConfig` (L8); session loops name it (§4.6) |
| D5 | "Schedule + iterations" runs all iterations back to back in one fire | `scheduler.rs:898-932` | retired (L8) |
| D6 | Scheduled iterations use the global model and goosed's cwd, not the chat's node or folder | `scheduler.rs:952-953, :961` | stays for scheduled recipes; session loops use the session's own |
| D7 | Two windows may fire one schedule twice (each goosed loads `schedule.json`) | `server_factory.rs:67-84`, `main.ts:2391` | **not verified**; filed for the scheduler owner, out of this lane |
| D8 | A running Agent Work desk outlives the app as an orphan and is not re-adopted | `main.ts:7010-7014`, `:6772-6790` | out of this lane; filed |
| D9 | turn_priority is per goosed, so a user turn in another window does not make this window's background work yield | `turn_priority.rs:85-97`, `main.ts:2391` | §5.3 names the seam (S5 holders) |
| D10 | The open Changes panel covers the chat's right third | walk `02-…png` | kept by the Q-190 rule; §8.3 mitigates for a long-open loop tab |

---

## 3. Research: how comparable products run loops

| Product | What it does | Take | Avoid |
|---|---|---|---|
| **Claude Code `/loop`** ([docs](https://code.claude.com/docs/en/scheduled-tasks)) | A fixed interval (`/loop 5m …`) or self-paced (no interval; Claude picks 1 min–1 h after each iteration and prints the delay and its reason). Ends itself via `ScheduleWakeup` with `stop: true`. Fires "between your turns, not while Claude is mid-response"; "no catch-up for missed fires"; tasks "only fire while Claude Code is running and idle". | Both cadences; the printed reason for a self-chosen delay; the model ending its own loop; firing only between turns; one fire after a gap, never a burst. | A silent 20-minute "fallback wakeup" when an iteration neither reschedules nor stops (gate 1): here that is a visible waiting state. A 7-day auto-expiry: a user-set tick count instead. Jitter: one local engine, no thundering herd. |
| **Claude Code `/goal`** ([docs](https://code.claude.com/docs/en/goal)) | Keeps starting turns until a condition holds; "a small fast model checks whether the condition holds" after every turn; the condition should be "something Claude's own output can demonstrate" with "a stated check"; status shows turns, time, spend, last reason; stops when "no tool use for several turns". | A stated check as the goal's judge; a status that shows turns, spend and the last reason; a no-progress stop (ours: stalled). | A second evaluator model per turn: on a local engine that is a second request per tick on the same node. Here the tick's own `loop_report` plus a shell check does the job. |
| **Claude Code Desktop tasks and Routines** | Durable schedules outside any session (local or cloud). | A later path for loops that must run with goose closed (§11 Q4). | Building it into v1: the owner's loops are attended build sessions. |
| **Codex app Automations** ([docs](https://developers.openai.com/codex/app/automations)) | Cron/webhook/manual runs in dedicated background worktrees; results land in a Triage inbox; "runs with no results are automatically archived". | Quiet ticks collapse in the timeline (a tick with no change and verdict progress is one line). | A separate inbox away from the work: R5 wants the loop beside the chat's changes. |
| **Cursor cloud agents** | "hold a /goal until it is met"; "@cursor check back in an hour and keep going until that feedback is in". | The phrasing: the loop is a policy set once and executed by the agent on its own schedule. | Remote VMs: not our product. |
| **Devin scheduled sessions** ([docs](https://docs.devin.ai/product-guides/scheduled-sessions)) | Recurring sessions; "reads and writes its own notes across sessions, which means each run builds on the context of the one before it"; Devin "figures out the cadence". | The state spine (notes read first and written last); self-pacing. | New session per run: R5 needs one chat. |
| **Replit Agent 3** ([blog](https://blog.replit.com/introducing-agent-3-our-most-autonomous-agent-yet)) | "periodically test your app in the browser and automatically fix issues"; Max autonomy runs 200+ min with "self-supervision". | "Prove" as a first-class step of a build loop. | Long opaque runs: every tick here is a visible turn with a verdict. |
| **GitHub Actions `schedule`** | Cron in UTC; the shortest interval is 5 minutes; runs may be delayed under load; public-repo schedules are disabled after 60 days without activity. | Say plainly that a scheduled time is "not before": ours shows "overdue" and "delayed by your turn". | Silent disabling: our pause always carries its reason. |
| **launchd `StartCalendarInterval`** (`man launchd.plist`, read locally) | "Unlike cron which skips job invocations when the computer is asleep, launchd will start the job the next time the computer wakes up. If multiple intervals transpire before the computer is woken, those events will be coalesced into one event upon wake from sleep." | Coalesce missed ticks into ONE on wake (§5.4). | — |
| **Agent loop UX patterns** (the above and Agent Work's own desk page) | A tick log with verdicts; next-run countdown; pause/resume; "run now"; a steer channel ("Tell the desk"); human checkpoints (needs you). | Every one of these. Steer reuses the chat itself: the composer IS "Tell the loop". | A second steer box: the chat already has one. |

---

## 4. The model

### 4.1 What a session loop is

> A **session loop** belongs to one chat session. It holds the user's **goal**, a **cadence**, a **state file**
> and **stop rules**, and it sends the chat a **tick**: one ordinary turn built from those facts. The tick
> works, proves what it did, and ends by calling `loop_report`. The loop records the tick, decides from the report
> and the check whether to continue, and schedules the next tick.

**Invariants:**
- A chat holds at most one loop. Starting a new one replaces the old one after a confirm dialog
  ("Replace the loop? The current loop ends after tick {n}.").
- A tick is a turn **in the chat**: its prompt, its tool calls, its file edits and its answer are in the transcript,
  the Changes model sees its edits, and the user can scroll, steer, stop or fork it like any turn.
- The loop never picks a model. A tick runs on whatever the chat's chip names (§5.5).
- A loop cannot be put on a `swarm-build` session. The start dialog refuses with "Loops run chat turns. This chat
  builds with the swarm, so every tick would start a full build. Use Agent Work for recurring builds." A tick in
  that session would spawn a whole `goose swarm run` (`providers/swarm.rs:717`).

### 4.2 The loop record: `extension_data["loop.v0"]`, owned by goosed

Stored through `ExtensionState` (`G/session/extension_data.rs:44`) and persisted with `set_extension_state`
(`session_manager.rs:484`), the same way as `todo.v0`, so it survives restarts and replays with the session.

```
LoopRecord {
  id: String,                       // "lp_<8 hex>", stable for the loop's life
  goal: String,                     // the user's words, verbatim
  template: TemplateId,             // quality | until_check | watch | blank
  steps: String,                    // the template's step text as the user left it (editable in the dialog)
  cadence: Cadence,                 // §4.3
  state_file: String,               // relative to the session working_dir (§4.5)
  check: Option<String>,            // shell command run in working_dir after a tick (§4.6)
  stop_after_ticks: Option<u32>,    // the user's own count, None by default
  status: LoopStatus,               // §4.7
  status_reason: Option<StatusReason>,
  next_tick_at: Option<rfc3339>, next_tick_reason: NextReason,
  owner: Option<Owner>,             // { goosed_pid, goosed_started_at }: which process runs the clock (§5.1)
  created_at, started_at, ended_at,
  ticks: Vec<TickRecord>,
}
TickRecord {
  n: u32, origin: TickOrigin,       // cadence | now | self_paced | back_to_back | after_your_turn | on_wake | resume
  started_at, ended_at,
  first_message_id: String,         // the tick's prompt message (§4.4); the next tick's or user's message ends it
  report: Option<LoopReport>,       // what loop_report said, verbatim
  outcome: TickOutcome,             // progress | done | blocked | failed{error_class, error} | no_report | yielded{to_session} | stopped_by_you
  check: Option<CheckRun>,          // { command, exit: Option<i32>, output_tail, ran: bool, error: Option<String> }
  served: Option<ServedRef>,        // the nodes.served record of the tick's lease (§5.5), when present
  tokens: Option<TokenDelta>,       // session token totals after − before: a measurement, shown, never a decision
}
```

Pure functions over the record live in `G/session_loops/rules.rs` (L0) and `S/components/loops/model.ts` (L0).
Both are pinned by one shared fixture, `G/session_loops/loops.fixture.json`, the pattern `nodes.fixture.json` uses.

### 4.3 Cadence: three kinds, one clock

| Kind | Record | Next tick | Label |
|---|---|---|---|
| Every | `every: "<n>s|m|h"`, parsed by the ONE grammar (`AW/window.rs:10-23`, moved in L6) | `DeskClock::next_tick(last_start, now)` (`AW/window.rs:117-147`): last start + cadence; if that time has passed, as soon as the running tick ends ("overdue") | "every 10 min" |
| When goose decides | `self_paced` | `report.next_in` from the tick just ended, parsed by the same grammar; the model's `next_reason` shown verbatim | "goose decides when" |
| Right after each tick | `back_to_back` | when the tick just ended has been recorded and its check has finished | "back to back" |

**Rules, each paid for elsewhere:**
- The cadence decides only when a tick **starts**. No tick, check or model call is ever cut by time (gate 5).
- A self-paced tick that gives no `next_in`, or one that fails the grammar, is not given a default. The loop enters
  **Waiting for you** with "Tick {n} didn't say when to come back" and the buttons [Run next tick] [Pause]. Claude
  Code's silent 20-minute fallback is the thing avoided (gate 1).
- The window (days, from/to, timezone) comes free with the shared clock. v1 always uses "always open" (§11 Q6).
- The dialog's presets are 5m / 10m / 30m / 1h / custom, all strings in the one grammar.

### 4.4 The tick: its prompt, its marker, its report

**The tick prompt is assembled from THIS loop's facts** (`G/session_loops/prompt.rs`, L0). Every line is a fact
or the user's own words; nothing is a generic instruction (gate 2's intent). Shape, with the real slots:

```
Loop tick {n} — "{goal first line}" · {cadence label}{ · stop after {k} ticks}
Your goal (the user's words):
{goal}
State file: {state_file} — read it before anything else; rewrite it before you finish
(Now · Next · Found · Done; keep it short enough to read in one go).
What each tick does (the user's steps):
{steps}
Last tick ({n-1}, {time}, {outcome}): "{report.summary}" — next step it named: "{report.next_step}"
{if check ran}  Check `{check}` after tick {n-1}: {passed | exited {code}}. Its output ended with:
{output_tail}
{if outcome was yielded}  Tick {n-1} was stopped at {time} for the user's turn in "{chat}"; its partial work is above.
{if self_paced}  Say when to come back: next_in ("10m", "2h") and why.
Finish by calling loop_report.
```

- **Message id.** The prompt is sent with id `looptick_{loopId}_{n}_{uuid}`. That follows the precedent of `steer_`
  ids (`server.rs:3046`), so the transcript and a replay from `sessions.db` both recognise a tick with no change to
  the core `Message` type. `parse_tick_id` is one function in Rust (L0) and one in TS (L0), pinned by the fixture.
- **`output_tail`** is the same tail the rail shows for that check, whose size is set in §8. It is one derivation in
  `rules.rs`, as a share of the check's output rather than a typed line count (gate 10's intent).

**The `loop_report` tool** comes from the `loop` platform extension (`G/agents/platform_extensions/loop_report.rs`, L3).
It is added to the session's agent when the loop starts and removed when it ends, so a chat without a loop never
carries the tool in its context:

```
loop_report {
  verdict: "progress" | "done" | "blocked",
  summary: string,        // what this tick did, with its evidence (command output, file:line)
  next_step: string,      // the one concrete next step
  next_in?: string,       // self-paced only: "<n>s|m|h"
  next_reason?: string,   // why that delay
  blocked_on?: string     // verdict=blocked: what only the user can decide
}
```

The tool validates and stores the report against the running tick, then answers
"Recorded. This tick ends when you finish your reply." A call outside a tick is refused with
"No loop tick is running in this chat." A second call in the same tick replaces the first; the ledger keeps the last.

### 4.5 The state spine

The **state file** is a file in the session's working dir. Its default is `.goose/loops/{slug}/NOW.md`, where
`{slug}` comes from the goal's first words; the dialog shows the path and the user can change it. The tick reads it
first and rewrites it last.

- **Why a file and not session notes.** It survives compaction, because it is re-read every tick. The user can open
  and edit it, which is how the owner steers his own quality loop. Its rewrites show in Changes like any edit, as
  `+N −M` on the spine. And it is the owner's own practice (`NOW.md:1`).
- **Why not the todo scratchpad** (`todo.rs:91-106`). It is invisible to the user, injected into every turn of the
  session (not only ticks), and a single overwrite slot that the model also uses for its own todo list.
- **The runner never writes the file.** When a tick ends and the file does not exist, the rail says "State file not
  written yet". There is no template content (gate 1).

### 4.6 Stop conditions and self-pause rules

| Condition | Who decides | Result |
|---|---|---|
| **Goal met, with a check set** | `check` exits 0 after a tick whose verdict is `done` or `progress` | **Ended**: "Goal met — `{check}` passed after tick {n}" |
| **Goal reported done, no check set** | the tick's `loop_report.verdict = done` | **Ended**: "goose reported the goal done after tick {n} — no check was set" (the model's own claim, labelled as such) |
| **Reported done, check fails** | check exits ≠ 0 | loop **continues**; the next prompt carries "you reported done; `{check}` exited {code}" and the tail |
| **Check could not run** | spawn error, missing shell | **Paused**: "The check could not run: {error}" (never read as "failed"; the D4 shape refused) |
| **Blocked** | `verdict = blocked` | **Paused**: "Blocked — {blocked_on}" |
| **The user's count** | `stop_after_ticks` reached | **Ended**: "Reached {k} ticks, as you set" |
| **The user stops** | Stop loop / `/loop stop` | **Ended**: "Stopped by you after tick {n}" (a running tick is stopped through the Stop door and keeps its partial) |
| **Same failure twice** | tick n and n−1 both `failed` with the same `error_class` | **Paused**: "Ticks {n-1} and {n} failed the same way: {error}" |
| **No report twice** | tick n and n−1 both `no_report` | **Paused**: "Ticks {n-1} and {n} ended without a loop report" |
| **Stalled** | tick n: verdict `progress`, zero file changes in its message range, `next_step` equal (whitespace- and case-normalised) to tick n−1's | **Paused**: "Stalled — tick {n} named the same next step as tick {n-1} and changed no files" |

**How these rules are built:**
- Every self-pause is a **repeat of the previous tick**, the same evidence shape as the judge's repeat trigger. None
  is a count or a clock (gates 5, 10).
- A single failed tick does not pause the loop. The next tick's prompt carries the error.
- **The check runs** in the session's `working_dir`, spawned in its own process group (`G/subprocess.rs:55`
  `process_group(0)`). Stop kills that group, the one the loop itself created, and no other (gate 4's discipline).
- **The check has no timeout.** The rail shows "Checking `{check}` · {elapsed}" and a [Stop check] button. A stopped
  check is recorded `ran: false, error: "stopped by you"`, and the loop pauses with that reason.

### 4.7 States

| Status | Meaning | Rail pill | Sidebar marker |
|---|---|---|---|
| `running` | a tick's turn is in flight | "Tick {n} running · {elapsed}" (ok fill) | existing Running pill |
| `checking` | the check command runs after tick n | "Checking · {elapsed}" (accent) | Looping pill |
| `waiting` | next tick scheduled | "Next tick {HH:MM}" (accent) | Looping pill "Next {HH:MM}" |
| `waiting_turn` | a tick is due but a user turn runs (this or another chat) | "Next tick after your turn" (secondary) | Looping pill |
| `waiting_you` | self-paced tick named no delay | "Loop waiting for you" (warn) | NeedsYou-style pill, NOT in Active now (it is not a question) |
| `needs_you` | the tick's turn is blocked on `ask_user` | "Loop needs you" (warn) | existing NeedsYou (a User session, `G/needs_you.rs:80-90`) |
| `paused` | by you or by a self-pause rule, `status_reason` names which | "Loop paused" (stopped fill) | Looping pill "Paused" |
| `ended` | a stop condition held | "Loop ended" (stopped fill) | none |
| `elsewhere` | another live goosed owns the clock (§5.1) | "Looping in another window" (secondary) | Looping pill |

### 4.8 One engine or two: the decision

| Thing | Keep? | Why |
|---|---|---|
| Session loops (new) | **the one loop in a chat** | R2–R5 |
| Scheduler `LoopConfig` (recipe loop) | **retire** (L8) | 0 uses on both Macs (§2.9); iterations run back to back as separate Scheduled sessions on the global model (D5, D6); swallows a check that cannot run (D4); its only writer is the dialog being removed |
| Scheduled recipes (no loop) | keep | the agent's own `platform__manage_schedule` tool, the CLI and REST write them; SchedulesView (URL-only) is where a user can see what the agent scheduled |
| Agent Work desks | keep, separate product | not a chat; fleet lanes, posts, asks, a work window; runs outside the app |
| `/goal`, `/grind` | keep | within one reply; §7.4 |
| **Shared** | the clock (`DeskClock`, moved to `G/loop_clock.rs` in L6); the vocabulary (tick, cadence, overdue, "Run a tick now", Pause, Resume, Stop); the tick ledger's shape | one cadence grammar and one next-tick rule instead of two copies of the same rule |

---

## 5. Engine

### 5.1 Where the clock lives: one runner per goosed, one owner per loop

- **`G/session_loops/runner.rs` (L2) is a process-wide runner**, a `LazyLock` like `turn_priority`'s `PRIORITY`
  (`turn_priority.rs:85`). It holds the loops this goosed owns.
  - **Arm.** On `loops/start`, `loops/control{resume}` and `loops/get` (when the record says `running`/`waiting` but
    its owner is dead), the runner claims the loop by writing `owner = {goosed_pid, goosed_started_at}` into the
    record, then computes `next_tick_at`.
  - **Owner check.** Before arming, the runner proves the recorded owner is gone: the pid is not alive, or it is alive
    with a different start time (the `machine.rs` proof-of-gone pattern the nodes design uses for holders). While the
    owner is alive, the second window sees **Looping in another window**, can pause or stop through the record, and
    never ticks.
- **Waiting.** The runner waits on a `tokio::sleep_until` computed from the WALL time `next_tick_at`. It re-evaluates
  (it never "catches up") on three events:
  1. a record change (control, edit);
  2. a tick ending;
  3. a `loops/wake` call, which the renderer sends on the system's resume from sleep (§5.4).
- **The door to the window.** goosed's `GooseAcpAgent` is per connection, and `client_cx` is set once per connection
  (`dispatch.rs:20-24`). The runner therefore keeps a registered **tick door**, the live connection, registered in
  `dispatch.rs` beside `client_cx.set`.
  - When a tick is due, the runner sends a goose custom notification `loops/tickDue { sessionId, loopId, n, messageId,
    prompt }` over that door. The existing `spawn_session_name_update_notifier` (`server.rs:1072`) is the pattern for
    such a server-initiated notification.
  - When no door is registered, which happens during a renderer reload, the tick stays due. The runner re-sends when a
    door registers. A tick is never dropped and never sent twice: `tickDue` for `(loopId, n)` is idempotent.
- **The hands are the renderer's.** `S/components/loops/LoopDriver.tsx` (L4) is mounted once per window
  (`AppLayout`). It subscribes to `loops/tickDue` and calls **`acpChatSessionController.submitMessage(sessionId, …)`**
  with the prompt, the message id and `_meta.goose.loopTick = { loopId, n }`.
  - That is the SAME door a typed message goes through (`chatSessionController.ts:149-205`). The streaming, the
    `activeRunId`, the Changes model and the steer path all behave as for a user send, in the displayed chat or in a
    chat the user is not looking at (the store is per session).
  - Why the renderer fires rather than goosed calling `agent.reply` itself: goosed's notifications for a turn go to
    the connection that asked for it (`on_prompt` writes to `cx`), and the renderer's session store is built around
    turns it submits. A server-internal turn would stream to nobody. Firing through the renderer makes a tick follow
    exactly the same path as a typed message.

### 5.2 The one prompt door: two conditionals in `on_prompt`, no second path

`on_prompt` (`server.rs:2636`) is the door for both. L2 adds exactly two branches, keyed by
`_meta.goose.loopTick` being present **and** matching the runner's due tick for that session. The match refuses a
forged meta: a client cannot mark an arbitrary message as a tick.
1. **Do not take `user_turn()`** (`:2647`) for a tick. A tick is not a user turn, so it must not make the
   end-of-turn checks of other chats wait for it as if the user were typing.
2. **Tell the runner the tick started** (`tick_started(session, n, run_id)`) and, at the end of the turn (the same
   exits that call `clear_active_run`), **tell it how the tick ended**: completed, cancelled, or errored with the
   error text. The runner then reads the stored `loop_report`, runs the check, applies §4.6, records the tick and
   schedules the next.

Everything else is unchanged for ticks: slash-command detection, the reply stream, cancellation, the `link_serve`
tap (`:2726`), the S5 loader's per-reply guard and the served-turn record.

### 5.3 The user's turn always wins

- **A tick does not start while a user turn runs** in this goosed. `turn_priority` gains one public fn (L2):
  `wait_no_user_turn() -> started` (the first half of `after_user_turns`, `turn_priority.rs:64-68`). A tick due
  during a user turn is `waiting_turn` and starts when the turn ends. Its origin is `after_your_turn` (one tick, not
  one per missed interval).
- **A user turn that starts in ANOTHER chat while a tick runs stops the tick.** `turn_priority` gains
  `user_turn_started_since(started)` (the second half, `:69-74`). The runner selects on it and, when it fires, calls
  the same cancel `on_cancel` uses (`server.rs:3065`). The tick keeps its partial work, is recorded as
  `yielded{to_session}`, and is re-sent after that turn as tick n+1, with origin `after_your_turn`. Its prompt says
  what happened: "Tick {n} was stopped at {time} for the user's turn in "{chat}"; its partial work is above."
  - **Why cancel instead of `after_user_turns`' drop-and-re-ask.** A dropped reply future would skip
    `clear_active_run`, and the session would read as busy. Re-asking a tool-using turn from the start would also
    repeat its edits.
- **A message typed into the loop's own chat during a tick is a steer** (`useChatSession.ts:217-240`, server
  `on_steer_session`). The user's words enter the running tick at once, which beats cancelling the tick to start a
  new turn. The composer says so (§8.1).
- **A message typed into the loop's chat between ticks is an ordinary user turn.** The next tick waits for it and
  then reads it as part of the conversation.
- **Across windows (D9).** A user turn in another window's goosed is not visible to this runner. **Seam:** DESIGN-NODES
  §6.4 step 2 and S5 create Mac-wide holder records (`goose-sidecar/src/holders.rs`, "no agent reply that was open
  before this demand still uses it, in any goose process on this Mac"). Once S5 lands, `wait_no_user_turn` and the
  yield also read "a user reply is open on the engine this tick would use" from those records. Until S5 lands, the
  limit is named in the start dialog: "Your turns in this window always go first."

### 5.4 App closed, window closed, Mac asleep: the honest limits

| Situation | What happens | What the user sees |
|---|---|---|
| The chat's window is open (any chat shown) | ticks fire | normal states |
| The window is closed (its goosed is released, `main.ts:1433-1435`) | no ticks; the record keeps `owner` = a dead goosed | reopening the chat: **Paused** "goose was closed at {time}; {k} ticks were due" with [Resume] (one tick now) and [Stop loop] |
| The app quits | same as closed | same |
| The Mac sleeps | tokio's clock does not advance during sleep (Rust's `Instant` on macOS does not count suspended time), so a timer armed before sleep would fire late by the sleep's length | **L1** adds `powerMonitor.on('resume')` in main → `system-resumed` to every window → the renderer calls `loops/wake` → the runner re-reads the wall clock: one tick if one or more were due (origin `on_wake`, "Missed while your Mac slept — ran once on wake"), never a burst (launchd's coalescing) |
| Keep awake | not a new mechanism: the existing wakelock setting (`main.ts:2828` `set-wakelock`, per-window blockers `:1131`) | the start dialog shows its state and a toggle: "Keep this Mac awake while goose is open" |

**Why no daemon in v1.** Every loop the owner describes is an attended build session in an open chat. A loop that
runs with the app closed has no chat to show its changes in, which contradicts R5, and Agent Work already covers that
case (§11 Q4).

### 5.5 Local-model cost, and the seam with DESIGN-NODES-AND-STRATEGIES §7.1

**One tick costs:**
- one agent reply, which may make several model calls with tool calls, on the node the chat's chip names;
- the end-of-turn fact check goose already runs after every reply (Q-185's "Checking the reply", `background_work::run`);
- the check command, which is a shell call and no model work.

**Everything goes through §7.1's doors unchanged:**
- The model id is the session's (`swarm`, `node:<id>`, `strategy:<id>`). The router leases a node, applies the
  when-rule, and writes the **served-turn record** (`nodes/served.rs`, key `nodes.served`). The tick row shows
  "on {node}" from that record; when S3 has not landed, the row omits it rather than guessing.
- **The S5 loader's per-reply guard** in `on_prompt` holds the way for the whole tick, because a tick is a reply.
- A tick on a node that is not loaded follows the node's `ifNotLoaded` rule. When that rule is `load`, each tick may
  swap the engine. The start dialog says so when it can tell: "Each tick may load {node} and stop {serving way}"
  (from `nodes/residency`).
- A `strategy:<id>` chat's tick uses the Chat chain, like any reply. **Nothing in v1 routes ticks to the Build role**
  (§11 Q7).

**What the loop never does:**
- choose, load or swap a model;
- register as an engine holder of its own (its ticks are replies; S8's holders are swarm runs).

**What the cost display shows:**
- per tick: wall time, the token delta (session totals after − before) and the node;
- in the header: total ticks, total wall time and total tokens.

These are measurements. None decides anything.

### 5.6 The repo gates this touches

- **Gate 5 (no time input) binds the swarm engine's model work.** A cadence here is a user's schedule of when a tick
  STARTS, like Agent Work's (`manifest.rs:31-32`). Nothing cuts a tick or a check. The check has no timeout, and
  "stalled" is a repeat, not a clock. No new seconds constant is added.
- **Gate 1:** every missing input is a named state (no report, no delay, check cannot run, state file not written,
  record unreadable → "The loop record could not be read: {error}", never "no loop").
- **Gate 2:** the tick prompt is built from the loop's facts (§4.4).
- **Gate 10:** no new numeric const. The presets are grammar strings; the output tail is a share of the check's output.
- **Gates 3, 6 and 9** are not engaged: no benchmark, no swarm DAG, no swarm phase.
- **No swarm engine file is touched** (`swarm.rs`, `crates/goose-swarm/*`). L6 moves the Agent Work clock, a pure
  file with no engine behaviour, and runs `cargo test -p goose-swarm --test development_gates` to prove the ratchets
  unchanged.

---

## 6. Q-227: the button, the dialog, recipes. The decision

**Decision: remove the composer's "Recipes & loops" button and everything only it reaches. Keep the recipe engine and
the scheduler's plain schedules. Retire the scheduler's recipe loop. The Loop button (L4) takes the slot.**

Evidence, from §2.9:
- zero schedules or loops on either Mac;
- zero saved recipes;
- zero user sessions started from a recipe out of 387;
- the last scheduled run was a July test.

The dialog's three actions each carry a defect (D1–D3). "Manage" duplicates the Skills nav item. The button shows only
for the `swarm` provider, so most chats never had it. And its "loop" means N back-to-back runs in separate sessions
(D5), which is not what the owner means by a loop.

| Removed (L1) | Its dependents, checked by grep | Why nothing breaks |
|---|---|---|
| `ChatInput.tsx` button, state, mount, import, `isSwarmProvider` (`:48, :328, :336, :1782-1803`) | `isSwarmProvider` has no other use in the file | — |
| `swarm/AgentSetupWizard.tsx` | imported only by `ChatInput.tsx`, its two ChatInput test mocks and two wizard tests | tests updated in L1 |
| `swarm/RecipeChatWizard.tsx`, `swarm/recipeChatTarget.ts`, `swarm/RecipeWizard.tsx` | imported only by AgentSetupWizard and tests | — |
| IPC `fleet-chat`: `main.ts:2105`, `preload.ts:497, :594`, `fleetIpc.ts` `fleetChatHandler` (`:37`), its test, the `test/setup.ts:107` mock | `fleetChat` used only by RecipeChatWizard; `fleetProbeHandler` (same file) stays | the probe IPC is untouched |
| `loop/LoopView.tsx`, `loop/LoopModal.tsx`, the `/loop` route (`App.tsx:37, :306-308, :823`) | LoopModal is used by AgentSetupWizard and LoopView only; LoopView is route-only; no in-app link to `/loop` | "loop" now has one meaning in the product |
| i18n `chatInput.recipesAndLoops*`, `loopView.*`, `loopModal.*` | regenerated by `pnpm i18n:extract` | — |

**Kept, and why:**
- `/recipes` and RecipesView (URL-only): a user can reach recipes made from a session ("create recipe from session",
  `SessionActionsHeader.tsx:363`), and deeplinks land in it.
- `/schedules` and SchedulesView (URL-only): the only place a user sees what the agent's own schedule tool created.
- The whole recipe engine (§2.2).
- `/goal` and `/grind`.

**L8 retires `LoopConfig`:**
- the struct and field (`scheduler.rs:107-117, :141-142`) and the iteration branch (`:897-932`);
- the DTO (`goose-sdk-types/src/custom_requests/schedule.rs:10-16, :33, :58`);
- the ACP mapping (`G/acp/server/schedule.rs:113-131, :145, :227`);
- the fixture line (`G/../tests/acp_fixtures/mod.rs:105`), `acp-schema.json:4649`, and REST/CLI `loop_config: None`.

L8 has a migration rule. Serde ignores the unknown field, so a `schedule.json` job that carried a `loop_config` runs
once per fire after L8. Both Macs hold none (`[]`). The L8 commit states this.

---

## 7. Starting a loop

### 7.1 From the composer: the Loop button

- **The slot.** The Loop button takes the slot of Recipes & loops (`ChatInput.tsx:1782-1794`) for **every** provider,
  except when the chat's model is `swarm-build`: there it is shown disabled, with the refusal as its tooltip.
- **Narrow bottom bar.** When the bottom bar is narrow (`isBottomBarNarrow`, below 480, `ChatInput.tsx:367`), the
  button hides like its predecessor did. `/loop` and the rail remain.
- **When the chat has a loop,** the button becomes a status chip that opens the rail's Loop tab: "Looping · next
  22:40", "Tick 4 running", "Loop paused" or "Loop needs you".

### 7.2 The `/loop` command

`/loop` is registered in `COMMANDS` (`G/agents/execute_commands.rs:19-57`, L3), handled server-side like `/goal`.
The subcommand words are fixed, so "stop" is never read as a goal.

| Typed | Effect | Reply |
|---|---|---|
| `/loop <goal>` | start a self-paced loop, template Blank, state file default; first tick now | "Loop started: {goal} · goose decides when. The first tick runs now." |
| `/loop every 10m <goal>` | start with a fixed cadence (one grammar) | "Loop started: {goal} · every 10 min. The first tick runs now." |
| `/loop` | status | "Loop: {goal} · {status sentence} · tick {n} · next {time}" or "No loop in this chat. Use /loop <goal> or the Loop button." |
| `/loop now` | run a tick now | "Tick {n} starts now." |
| `/loop pause` / `/loop resume` | control | "Loop paused after tick {n}." / "Loop resumed — next tick {time}." |
| `/loop stop` | end | "Loop stopped after tick {n}." |
| `/loop every 10m` with no goal | refused | "Say what the loop should do: /loop every 10m <goal>." |

**Routing.** `command_starts_turn` (`execute_commands.rs:99-104`) does NOT start a turn for `/loop`, which only
returns its reply. The first tick then arrives through `tickDue`, like every other tick. The one door holds.

### 7.3 Promote a message: "Loop this"

User messages already carry hover actions ("Edit", "Copy", walk `00-…png`; `S/components/UserMessage.tsx`). L5 adds
**"Loop this"**, which opens the Start dialog (§8.2) with that message's text as the goal. It does not appear on tick
markers.

### 7.4 Templates

The template text lives once, in `G/session_loops/templates.rs` (L0), and is served by `loops/templates`. The dialog
shows the steps editable. The prompt uses what the user left.

| Template | Name | Steps (the text a tick receives under "What each tick does") | Suggested check | Default cadence |
|---|---|---|---|---|
| `quality` | Software quality loop | 1. Discover: open the state file, then run or read what the goal names. List what is broken, missing or confusing, each with the evidence you saw (command output, file:line). 2. Critique: rank what you found by how much it blocks the goal; pick the ONE item that matters most. 3. Fix: make that change, and only that change. 4. Prove: run the check (or the command that shows the change works) and quote its result. A fix without a quoted result is not done. 5. Rewrite the state file: what is now true, what is next, what you found but did not fix. | the project's test command, from the dialog | every 10m |
| `until_check` | Until a check passes | 1. Run the check and read why it fails. 2. Fix the first cause it names. 3. Run the check again and quote the result. 4. Rewrite the state file. | required | back to back |
| `watch` | Watch and act | 1. Look at what the goal names (a build, a deploy, a folder, a URL) and compare it with the state file. 2. If nothing changed, say so in one line and report progress. 3. If something changed, do what the goal asks and quote the evidence. 4. Rewrite the state file. | optional | every 30m |
| `blank` | Blank | (empty; the goal alone) | optional | goose decides |

**`/goal` inside a tick.** The quality template's step 4 already demands proof inside the tick. A user who also wants
the in-reply self-check can type `/goal` in the chat, and it applies to every reply, ticks included. Nothing couples the two.

---

## 8. Screens

**Common rules for every screen:**
- Studio primitives only (`StudioButton`, `OverlayDialog`, `Segmented`, the Studio input). No native `<select>`,
  `alert`, `confirm` or `prompt`. The Replace and Stop confirmations use `OverlayDialog`.
- Solid fills only, from `TONE_FILL` (`S/components/lz/tokens.ts`):
  - tick outcomes: done = `ok`, progress = `accent`, failed = `err`, no report / stalled / paused = `stopped`,
    yielded = `secondary`, needs you / waiting for you = `warn`, blocked = `err`;
  - six solid hues and no tints. Ink comes from the tone's own pair, which passes 4.5:1 in both themes (the harness
    asserts it, §10.2).
- No left accent rails: tick rows are separated by full-width borders, and the running tick is emphasised with a
  solid pill and weight.
- Every string goes through `defineMessages` with key prefix `loops.*`; `en.json` is regenerated.
- Widths: **460** is listed by the brief, but the window's `minWidth` is 480 (`main.ts:1397`), so the narrow layout
  is designed at 480. The other widths are **1000** and **1600**.
- Both themes use the same hues. In dark mode the fills are the tokens' dark values, which carry their own ink.

### 8.1 The composer (1000, light)

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ Ask goose to build, fix or explain something                                               │
│ ● Writing Qwen3.8-27B · Mihai Macbook an…  [⟳ Loop]  [▭ mihaiperdum]      58k/262k  ⚙ 📎 ■ │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

- **No loop:** `[⟳ Loop]` (ghost StudioButton). Tooltip: "Run this chat's goal again and again — each run is a tick".
- **Loop exists:** a solid chip in the same slot:
  - `[⟳ Tick 4 running]` (ok)
  - `[⟳ Next 22:40]` (accent)
  - `[⟳ Loop paused]` (stopped)
  - `[⟳ Loop needs you]` (warn)
  - `[⟳ Loop ended]` (stopped; hidden once the user dismisses it in the rail)
  - Click opens the rail on Loop.
- **Placeholder while a tick runs in this chat:** "Tick {n} is running — what you send steers it".
- **Placeholder while a tick waits on the user's turn:** unchanged.
- **Chat model `swarm-build`:** `[⟳ Loop]` disabled; tooltip "Loops run chat turns. This chat builds with the swarm —
  every tick would start a full build. Use Agent Work for recurring builds."
- **480:** the button and chip hide (the existing narrow rule); the rail pill remains.

### 8.2 The Start dialog (OverlayDialog; 1000 light)

```
┌ ⟳ Loop this chat ──────────────────────────────────────────────── ✕ ┐
│ goose runs your goal again and again in this chat — each run is a    │
│ tick. Every tick reads its state file, works, proves what it did,    │
│ and says what comes next. You can steer, pause or stop it any time.  │
│                                                                      │
│ Start from                                                           │
│ [■ Software quality loop] [Until a check passes] [Watch and act] [Blank]
│   Discover what's broken, pick the one thing that matters most,      │
│   fix it, prove it with a check. Repeat.                             │
│                                                                      │
│ Goal                                                                 │
│ ┌──────────────────────────────────────────────────────────────────┐ │
│ │ Make the users.csv generator produce every problem class Aoife…  │ │
│ └──────────────────────────────────────────────────────────────────┘ │
│ What each tick does                                    [Reset steps]  │
│ ┌──────────────────────────────────────────────────────────────────┐ │
│ │ 1. Discover: open the state file, then run or read what the …    │ │
│ └──────────────────────────────────────────────────────────────────┘ │
│ How goose proves it                                                  │
│ Command  [ node scripts/validate_users.js                        ]  │
│ Runs in ~/goose-builds/…/work after every tick. When it succeeds     │
│ after goose says it's done, the loop ends.                           │
│                                                                      │
│ When the next tick runs                                              │
│ [■ Every] [When goose decides] [Right after each tick]               │
│ [5m] [■10m] [30m] [1h] [custom]                                      │
│                                                                      │
│ State file  [ .goose/loops/users-csv-generator/NOW.md            ]   │
│ goose reads this first and rewrites it last, so the loop survives    │
│ long chats and compaction. You can edit it too.                      │
│                                                                      │
│ Stop when                                                            │
│ ● the check succeeds after goose reports done                        │
│ ● you stop it                                                        │
│ [ ] after [  ] ticks                                                 │
│                                                                      │
│ Each tick is one turn on 27B · both Macs. Your turns in this window  │
│ always go first — a tick waits or pauses while you chat.             │
│ Your Mac may sleep; ticks wait until it wakes. [◯ Keep this Mac awake while goose is open]
│                                                                      │
│                                    [Cancel]  [■ Start loop]          │
│                                    The first tick runs now.          │
└──────────────────────────────────────────────────────────────────────┘
```

**Strings and states**

- **Title and intro.** Title "Loop this chat", or "Edit loop" when editing. Intro as drawn.
- **Template chips.** A Segmented group whose selected chip is a solid accent fill. Descriptions:
  - quality: "Discover what's broken, pick the one thing that matters most, fix it, prove it with a check. Repeat."
  - until_check: "Keep working until a command you name succeeds — tests, a build, a lint."
  - watch: "Look at something on a schedule and act when it changes — a build, a deploy, a folder."
  - blank: "Your goal, your steps."
- **Goal.**
  - Placeholder: "What should every tick move forward? e.g. Make every test in ui/desktop pass without changing the tests".
  - Empty → Start is disabled, with "Say what the loop should do." under the field.
- **Steps.** Label "What each tick does", action "Reset steps". The field is empty for Blank.
- **Check.**
  - Label "How goose proves it", field label "Command", placeholder "pnpm test".
  - Help: "Runs in {dir} after every tick. When it succeeds after goose says it's done, the loop ends."
  - `until_check` with no command: "This template needs a command to check." and Start is disabled.
- **Cadence.**
  - Labels: "Every" · "When goose decides" · "Right after each tick".
  - Presets 5m / 10m / 30m / 1h / custom. Custom's help is the existing wording, "custom: a number and s, m or h (90m, 2h)".
    An invalid value shows "Use a number and s, m or h — 90m, 2h".
  - Self-paced help: "After each tick goose names when to come back and why — you'll see its reason."
  - Back-to-back help: "The next tick starts as soon as one ends."
- **State file.** Label "State file", help as drawn. A path outside the working dir shows "Keep the state file inside
  {dir}." and Start is disabled.
- **Stop when.**
  - With a check: "the check succeeds after goose reports done".
  - Without a check: "goose reports the goal is done".
  - Always shown and not removable: "you stop it".
  - Optional: "after [n] ticks", with the help "Leave empty to run until the goal is met or you stop it."
- **Cost line.**
  - "Each tick is one turn on {served label}. Your turns in this window always go first — a tick waits or pauses
    while you chat." `{served label}` is the chip's own label, one derivation.
  - When the nodes residency says a tick would swap: add "Each tick may load {node} and stop {way}."
- **Sleep line.** "Your Mac may sleep; ticks wait until it wakes." The toggle "Keep this Mac awake while goose is open"
  mirrors the existing wakelock setting (`set-wakelock`).
- **Buttons.** "Cancel" and "Start loop" (or "Save" when editing), with the sub-line "The first tick runs now."
  (Editing does not start a tick.)
- **Replace confirm.** When the chat already has a loop: "Replace the loop? The current loop ends after tick {n}."
  with [Keep it] [Replace].
- **Refusal.** For a `swarm-build` chat the dialog cannot open, and the button says why (§8.1).
- **480:** full-width sheet; the chips wrap; the preset chips become two rows.
- **1600:** max width 40rem, centred.

### 8.3 The right rail, collapsed (1000, light)

```
                                                  ┌──────────────────────┐ ┌─────────────────┐
  (chat, full width, unchanged)                   │⟳ Tick 5 running · 1m │ │▣ 2 files +107 −9│
                                                  └──────────────────────┘ └─────────────────┘
```

**Pills.**
- Two pills in the rail's corner, loop first. Each is a solid pill. The loop pill carries the status label of §4.7;
  the Changes pill is unchanged (`changes-rail-pill`).
- The loop pill is absent when the chat has no loop. The Changes pill is absent when no file changed (unchanged).
- **Ended loop:** the pill shows "Loop ended" until the user opens the Loop tab once. After that it hides; the tab
  keeps the ended loop's ledger, and "Start a new loop" is available there.
- **480:** the pills stack vertically; each is capped to the width available (`calc(100vw-2rem)`), with the label
  truncated by characters (Ink-style pre-truncation, not CSS overflow).

**Rail placement.**
- The rail stays an overlay at `absolute right-4`, `LAYER.chrome`. **It never narrows the chat** (Q-190's rule and R5's "right side").
- **D10 mitigation.** The open panel covers the chat's right side. A loop tab kept open for hours makes that cost
  real, so:
  1. the panel's open/closed state and last tab are remembered per session (localStorage, a per-viewer convenience,
     wrapped in try/catch);
  2. at **1600** the panel docks to the top-right with its height capped at 65vh (unchanged), and the chat's own
     message column is not widened under it. The overlay rule is kept, and nothing moves under the user's eyes.

### 8.4 The rail, open on Loop (1000, light): every state

The frame shared by all states:

```
┌ [■ Loop]  [Changes 2]                                                     ✕ ┐
│ Make the users.csv generator produce every problem class…      [■ Running]  │
│ every 10 min · tick 5 · since 21:40 · 41m · 58k tokens                       │
│ Check: node scripts/validate_users.js                                        │
│ State file: .goose/loops/users-csv-generator/NOW.md  [Open]                  │
│ [Run a tick now] [Pause] [Stop loop] [Edit]                                  │
├──────────────────────────────────────────────────────────────────────────────┤
│ NOW                                                                           │
│ Tick 5 · started 22:40 · 1m 12s · on 27B · both Macs        [Show in chat]   │
├──────────────────────────────────────────────────────────────────────────────┤
│ TICKS                                                                         │
│ 4  22:31 · 6m 12s  [■ Progress]                                  ▾            │
│    Added case-only duplicate emails and 30 no-email rows; the generator now   │
│    seeds 42 and writes 400 rows.                                              │
│    ▣ 2 files +41 −7                                                           │
│      generate_users.js  +38 −7   ▸                                            │
│      NOW.md             +3  −0   ▸                                            │
│    Check validate_users.js exited 1 — "missing svc- accounts"                 │
│    Next: add svc- service accounts with no last_login                         │
│    [Show in chat]                                                             │
│ 3  22:21 · 5m 40s  [■ Progress]  Generator scaffolded; 400 rows…  ▸           │
│ 2  22:11 · 4m 02s  [■ Failed]    Provider error: stream ended early   ▸       │
│ 1  22:01 · 7m 30s  [■ Progress]  Read kickoff.md; listed 6 problem…  ▸        │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Header and controls**
- **Tabs:** `Segmented as="tabs"` (`S/components/lz/Segmented.tsx:34-39`) with "Loop" and "Changes {count}". The
  Changes tab is today's panel body, unchanged.
- **Status chip:** the §4.7 status word, solid. The words are "Running", "Checking", "Waiting", "Waiting for your
  turn", "Waiting for you", "Needs you", "Paused", "Ended" and "In another window".
- **Line 2:** the cadence label · "tick {n}" · "since {HH:MM}" · total wall · total tokens. For self-paced: "goose
  decides when"; for back-to-back: "back to back"; with the user's count: "tick {n} of {k}".
- **Check line:** "Check: {command}", or "No check — the loop ends when goose reports done or you stop it".
- **State file line:**
  - "State file: {path} [Open]", where Open reveals the file in the chat's file opener, or the system opener when
    none exists.
  - Before the first write: "State file: {path} · not written yet".
- **Controls,** each a StudioButton:
  - "Run a tick now", disabled while running or checking, with tooltip "A tick is already running";
  - "Pause" or "Resume";
  - "Stop loop", which confirms with "Stop the loop? Tick {n} stops now and keeps what it did." and [Keep running] [Stop loop];
  - "Edit", which opens the §8.2 dialog in edit mode.

**NOW block, by state**

| State | NOW block |
|---|---|
| running | "Tick {n} · started {HH:MM} · {elapsed} · on {node}" + [Show in chat] |
| checking | "Checking `{command}` · {elapsed}" + [Stop check] |
| waiting (every) | "Next tick {HH:MM} · in {rel}" · overdue: "Next tick starts as soon as tick {n} ends (overdue)" |
| waiting (self-paced) | "Next tick {HH:MM} — goose chose {interval}: "{next_reason}"" |
| waiting (back-to-back) | "Next tick starts now" (shown briefly between record and send) |
| waiting_turn | "Tick {n+1} is due — it starts when your turn in "{chat}" ends" |
| waiting_you | "Tick {n} didn't say when to come back." + [Run next tick] [Pause] |
| needs_you | "Tick {n} is waiting for your answer" + [Go to the question] (scrolls to the NeedsYou card, `BaseChat.tsx:677`) |
| paused (you) | "Paused by you after tick {n}." + [Resume] |
| paused (rule) | the §4.6 reason verbatim, e.g. "Stalled — tick 7 named the same next step as tick 6 and changed no files" + [Resume] [Edit] |
| paused (closed) | "goose was closed at {HH:MM}; {k} ticks were due." + [Resume — run one tick now] |
| paused (check cannot run) | "The check could not run: {error}" + [Edit] [Resume] |
| ended | the §4.6 ended sentence + [Start a new loop] (opens §8.2 prefilled from this loop) |
| elsewhere | "This loop runs in another goose window." + [Pause] [Stop loop] (through the record) |
| record unreadable | "The loop record could not be read: {error}" (no controls but [Stop loop], which rewrites it) |

**Tick row**
- **Collapsed:** "{n}", "{HH:MM} · {duration}", the outcome chip, the first line of `summary`, and a ▸ expander.
- **Expanded:**
  - `summary` in full;
  - files changed in THIS tick: `sessionChanges(messages[range])` with the same file rows and hunks as the Changes
    tab, the component reused;
  - the check line;
  - "Next: {next_step}";
  - the self-paced reason;
  - [Show in chat], which scrolls to the tick marker.
- **Range.** A tick's message range runs from its marker message to the next tick marker, or to the end. It
  deliberately includes the user's steers and user turns inside it: they happened during the tick.
- **Outcome chips:** "Progress", "Done", "Goal met", "Blocked", "Failed", "No report", "Stalled", "Yielded", "Stopped by you".
- **Special rows:**
  - A yielded tick: "Yielded to your turn in "{chat}"". Its partial changes are shown.
  - A failed tick: "Failed: {error}" (verbatim, first line).
  - A no-report tick: "Ended without a loop report" plus the last assistant line, quoted.
- **Quiet ticks** (progress, zero files, check unchanged) collapse to one line (the Codex "no results" pattern):
  "{n} {HH:MM} · nothing changed · {next_step}".
- The newest tick is first. The ledger is virtualised past what fits (the existing list pattern).

**Empty Loop tab** (no loop, opened via `/loop` status or the Changes tab's second tab): "No loop in this chat." plus
"A loop runs a goal again and again here, on a schedule or when goose decides. Start one from the Loop button, with
/loop <goal>, or with "Loop this" on a message." and [Start a loop].

**Widths**
- **480:** full width minus 2rem; the header controls wrap to two rows; tick rows show chip + summary only, and the
  files list is shown on expand.
- **1000:** as drawn (32rem).
- **1600:** 32rem, same (§8.3).

**Dark mode:** the same layout; fills use the dark token values; the panel surface is `--lz-surface-raised`; borders
are the full `border-lz-border`.

### 8.5 The tick in the transcript

```
──────────── ⟳ Loop tick 5 · 22:40 · every 10 min ──────────── [Show prompt ▾]
(assistant turn, tool cards, file diffs — unchanged)
```

- **The marker.** A tick's prompt message (id `looptick_…`) renders as a full-width divider, not a user bubble.
  "Show prompt" expands the exact text sent, the §4.4 transparency.
- **Yielded tick:** the divider gets a second line: "Stopped at {HH:MM} for your message in "{chat}" — the loop
  continues after your turn."
- **Steers** inside a tick render as today (user bubble, `metadata.steer`).
- Hover actions on a marker: "Copy prompt" only. There is no Edit or Loop this.

### 8.6 The sidebar

- **Rows.** Session rows in PROJECTS and the session list get a **Looping** pill: solid accent, ⟳ icon, text
  "Looping" or "Next {HH:MM}", and "Paused" in the stopped fill. This comes through the existing `SessionActivityMarker`.
- **Precedence.** A running tick shows the existing Running pill, and a tick blocked on ask_user shows NeedsYou.
  Looping ranks below running and needs-you, and above idle.
- **Active now.** It lists a loop session only while a tick runs or needs you, which is the existing rule
  (`sessionActivityStore.ts:240-242`). A waiting loop is not "active": it is a scheduled future, not work in flight.
- **Top bar.** The top bar's "N running" counts ticks as running, which they are.

---

## 9. Slices

Every slice runs the full gate from `goose-feature-dev`: fmt, clippy `-D warnings`, cargo test for the crate, tsc,
eslint, `i18n:check` and vitest. `en.json` is regenerated with `pnpm i18n:extract` after each merge and never
hand-merged.

**Rules across slices:**
- Every file has exactly one owning slice. A file handed from one slice to a later one is named "handed from Lx".
- No slice edits `swarm.rs`, `crates/goose-swarm/*` or anything under `crates/goose-cli/src/commands/swarm/` except
  L6's two files (`agent_work/window.rs`, `agent_work/manifest.rs`).
- `custom_dispatch.rs` and `custom_requests.rs` are shared with the nodes design. L0 adds its own lines and a new
  file (`custom_requests/loops.rs`) and never edits the nodes lines.

### 9.0 The slice table

| Slice | Owns | Depends on | Confidence |
|---|---|---|---|
| L1 | Q-227 removal + the resume broadcast: `ChatInput.tsx` (until merged, then handed to L4), `swarm/AgentSetupWizard.tsx`, `RecipeChatWizard.tsx`, `RecipeWizard.tsx`, `recipeChatTarget.ts`, their tests, `loop/LoopView.tsx`, `loop/LoopModal.tsx`, `App.tsx` (the loop route lines), `main.ts` (fleet-chat line + `powerMonitor` resume), `preload.ts`, `utils/fleetIpc.ts` + test, `test/setup.ts` | — | high |
| L6 | one clock: `G/loop_clock.rs` (new), `G/lib.rs` (its `pub mod` line), `crates/goose/Cargo.toml` (`chrono-tz` via `cargo add`), `AW/window.rs` (becomes a re-export), `AW/manifest.rs` (`WorkWindow` import) | — | high |
| L0 | contract + pure rules: `G/session_loops/{mod.rs, rules.rs, prompt.rs, templates.rs, acp.rs, seam.rs, loops.fixture.json}`, `G/lib.rs` (its line; handed from L6), `goose-sdk-types/src/custom_requests/loops.rs` + its two lines in `custom_requests.rs`, `G/acp/server/custom_dispatch.rs` (the `dispatch_loops_*` fns), `S/acp/loops.ts`, `S/components/loops/model.ts` + tests | L6 | high |
| L2 | runner + door: `G/session_loops/runner.rs`, `G/turn_priority.rs`, `G/acp/server.rs` (the two `on_prompt` branches), `G/acp/server/dispatch.rs` (register the door) | L0 | **low-medium** |
| L3 | report tool + command: `G/agents/platform_extensions/loop_report.rs`, `platform_extensions/mod.rs` (its registration lines), `G/agents/execute_commands.rs` (`/loop`) | L0 (compile); L2 (live) | medium |
| L4 | start UI: `ChatInput.tsx` (handed from L1), `S/components/loops/StartLoopDialog.tsx`, `S/components/loops/LoopDriver.tsx`, `S/components/Layout/AppLayout.tsx` (mount LoopDriver) + tests | L0; L1 merged | medium-high |
| L5 | the rail + transcript: `S/components/session-rail/SessionRail.tsx`, `S/components/loops/{LoopPanel.tsx, TickRow.tsx, LoopPill.tsx}`, `S/components/changes/ChangesRail.tsx` (split into pill + `ChangesPanelBody`), `S/components/BaseChat.tsx` (the mount at `:666-673`), `S/components/UserMessage.tsx` (tick divider + "Loop this"), `S/acp/adapter/messages.ts` (map `looptick_` ids to `metadata.loopTick`) + tests | L0 | medium-high |
| L7 | sidebar + activity: `goose-sdk-types/src/custom_requests/needs_you.rs` (`looping` list), `G/acp/server/needs_you.rs`, `S/components/sessionActivity/sessionActivityStore.ts`, `S/components/sessionActivity/ActivityPills.tsx` + tests | L0 | medium |
| L8 | retire `LoopConfig`: `G/scheduler.rs`, `goose-sdk-types/src/custom_requests/schedule.rs`, `G/acp/server/schedule.rs`, `crates/goose/tests/acp_fixtures/mod.rs`, `crates/goose/acp-schema.json` (regenerated), `crates/goose-server/src/routes/schedule.rs`, `crates/goose-cli/src/commands/schedule.rs` | L1 merged | medium-high |
| L9 | harness + live: `local-edition/mlx/quality/harness/loops-state.mjs`, `…/harness/loops-walk.mjs`, the E2E brief `local-edition/mlx/quality/briefs/loop-5-ticks.md` | L2–L7 | medium |

Order by data dependency: **L1 ∥ L6 → L0 → (L2 ∥ L3) → (L4 ∥ L5 ∥ L7) → L8 → L9.** L4, L5 and L7 can be cut in
worktrees against L0's contract before L2 lands. Only their live proof needs L2.

### L1: Remove Recipes & loops (Q-227) and broadcast resume

**Confidence: HIGH.** It is a deletion with every dependent enumerated (§6), plus four lines of main/preload.

**Owns:** the table row above.

**Changes:**
- Delete the files in §6.
- In `ChatInput.tsx`, remove `:48`, `:328`, `:336` and `:1782-1803`. Leave the slot empty; L4 fills it.
- Remove the `/loop` route (`App.tsx:37, :306-308, :823`).
- Remove the `fleet-chat` IPC (`main.ts:2105`, `preload.ts:497, :594`, `fleetIpc.ts` `fleetChatHandler`).
  `fleetProbeHandler` stays.
- In `main.ts`, add `powerMonitor.on('resume', …)`, which sends `system-resumed` to every window. In `preload.ts`,
  add `onSystemResumed(cb)`.

**Tests:**
- The launcher cases are removed from `ChatInput.localEngine.test.tsx:154-169` and its mock (`:62-64`), and from
  `ChatInput.bottomBar.test.tsx:59`.
- The wizard tests (`wizards.studio.test.tsx`, `wizards.escape.test.tsx`, `RecipeChatWizard.mlx.test.tsx`) are deleted.
- `fleetIpc.test.ts` keeps its probe cases.
- New: a preload/main unit asserts that `system-resumed` reaches a mock window.
- `i18n:check` shows no orphaned keys.

**Must not break:**
- `/recipes`, `/schedules` and RecipesView's start / new-window / schedule / slash-command actions;
- "create recipe from session";
- `goose://recipe` deeplinks;
- `fleet-probe`;
- the Skills nav item.

### L6: One clock

**Confidence: HIGH.** A pure move with the existing tests moving along.

**Owns:** see the table.

**Changes:**
- Move `parse_cadence`, `parse_hm`, `parse_day`, `DeskClock` and `WorkWindow` into `G/loop_clock.rs` unchanged.
- `AW/window.rs` becomes `pub use goose::loop_clock::*;`, and `manifest.rs` imports `WorkWindow` from there.
- Add `chrono-tz` to `crates/goose` with `cargo add`, at the same version as goose-cli's (`0.10`,
  `crates/goose-cli/Cargo.toml:73`).

**Tests:**
- `window.rs`'s tests move and pass byte-identically.
- `cargo test -p goose-cli` shows Agent Work's tests unchanged.
- `cargo test -p goose-swarm --test development_gates` passes: the ratchet counts do not rise, because `window.rs`
  carries no numeric const.

**Must not break:** Agent Work's next-tick reasons and window behaviour. `AW/mod.rs:167-353` is untouched.

### L0: Contract and pure rules

**Confidence: HIGH.** Types, a store over `extension_data`, pure rules, the prompt builder and templates, with a
shared TS/Rust fixture. The risk is fixture drift, which the shared fixture refuses.

**Owns:** see the table.

**Contract** (every method now; bodies call the runner through `seam.rs`, and before L2 lands they answer the named
refusal "The loop runner is not in this build"):

| Method | Request | Response |
|---|---|---|
| `_goose/unstable/loops/get` | `{sessionId}` | `{loop: LoopRecord?, error?: string}`. An unreadable record answers `error`, never `loop: null`. |
| `_goose/unstable/loops/start` | `{sessionId, goal, template, steps, cadence, stateFile, check?, stopAfterTicks?}` | `{loop}` or a refusal `{reason}` (swarm-build chat, empty goal, bad cadence, state file outside the working dir) |
| `_goose/unstable/loops/update` | `{sessionId, patch}` (edit) | `{loop}` |
| `_goose/unstable/loops/control` | `{sessionId, action: pause \| resume \| stop \| tickNow \| stopCheck}` | `{loop}` |
| `_goose/unstable/loops/wake` | `{}` | `{rearmed: number}` |
| `_goose/unstable/loops/templates` | `{}` | `{templates: [{id, name, description, steps, suggestedCadence, needsCheck}]}` |
| `_goose/unstable/loops/list` | `{}` | `{loops: [{sessionId, status, nextTickAt?}]}` (for L7) |
| notification `loops/tickDue` | — | `{sessionId, loopId, n, messageId, prompt}` |
| notification `loops/changed` | — | `{sessionId, loop}` (the rail and pills update on events, not polls) |

**Pure rules** (`rules.rs` / `model.ts`, pinned by `loops.fixture.json`):
- `next_tick(record, now)` over `loop_clock::DeskClock`;
- `decide_after_tick(record, tick, check)`, which returns the §4.6 outcome, status and reason;
- `stalled(prev, cur, files_changed)`;
- `status_sentence(record)`, which returns every string key and its facts;
- `parse_tick_id` / `tick_id`;
- `tick_ranges(messages, ticks)`, which returns `[first, next)` per tick;
- `output_tail(output)`.

**Tests:**
- serde round-trip of every fixture case;
- each §4.6 row as a fixture case, run by BOTH suites;
- `next_tick` for every / self-paced (valid, missing, invalid) / back-to-back × first / overdue;
- `tick_ranges` with steers and user turns between ticks;
- the prompt builder for each template with and without a previous tick, check or yield, with the snapshot reviewed
  once by a reader for gate-2 specificity;
- the seam refusal before L2.

**Must not break:** nothing is written into a session's `extension_data` until `loops/start`; `loops/get` is a pure read.

### L2: The runner and the door

**Confidence: LOW-MEDIUM.** Stated plainly, because this is where a subtle bug can hide:
1. **Owner claims across goosed processes over one SQLite** (§5.1). A race between two windows arming the same loop
   is refused by a compare-and-set on `owner` inside one write. This must be proven by a two-process test, not assumed.
2. **The door registration across renderer reloads.** `client_cx` is per connection (`dispatch.rs:20-24`); whether a
   reload makes a new `GooseAcpAgent` in the same goosed must be measured first (§11 Q9).
3. **The renderer receiving a `tickDue` for a chat it is not displaying** and submitting through the controller.
   `submitMessage` is keyed by session, but the path has only been exercised for the displayed chat.
4. **The yield.** Cancel, record `yielded`, then re-send after the user's turn, without leaving `activeRun` stale.
   On sleep, Rust's macOS `Instant` behaviour is asserted by a measurement, not by the doc.

**Owns:** see the table.

**Changes:**
- `runner.rs`: arm, claim, wait (wall-clock `next_tick_at`, re-evaluated on record change, tick end or wake), send
  `tickDue`, receive start/end from `on_prompt`, run the check in its own process group, apply
  `decide_after_tick`, and emit `loops/changed`.
- `turn_priority.rs`: `wait_no_user_turn()` and `user_turn_started_since(started)`, both factored from
  `after_user_turns` (`:58-76`), whose own behaviour is unchanged.
- `server.rs` `on_prompt`: the two branches of §5.2.
- `dispatch.rs`: `runner::register_door(...)` beside `client_cx.set` (`:24`).

**Tests:**
- runner unit tests with a fake door and a fake clock, **fed as values**: the runner takes `now` from a trait so tests
  inject it; no seconds constant is added;
- a tick not started while a user turn is held;
- a user turn started mid-tick → cancel → `yielded` → re-sent after;
- a closed owner → paused-closed;
- two runners racing the claim → exactly one owner;
- a check that fails to spawn → paused with the reason;
- a check stopped → paused;
- self-paced with no `next_in` → `waiting_you`;
- `after_user_turns`' existing tests unchanged.

**Must not break:**
- `on_prompt` for every non-tick prompt: the same `user_turn()`, the same errors, the same notifications (existing
  ACP tests plus a new one asserting a prompt with a forged `loopTick` meta takes `user_turn()` and is not recorded
  as a tick);
- the end-of-turn reviewer's yield (Q-132).

### L3: The report tool and `/loop`

**Confidence: MEDIUM.** The code is small. The risk is whether the local models call `loop_report` reliably at the
end of a tick. That is measured in L9's E2E (the ≥ 5-tick journey counts `no_report` ticks) and the tool description
is iterated on the words, gate 7's read-the-words practice.

**Owns:** see the table.

**Changes:**
- The `loop` platform extension exposes one tool, `loop_report` (§4.4 schema). The runner adds it to the session's
  agent on start and removes it on end (the `agent.add_extension` door the scheduler uses, `G/scheduler.rs:974-976`).
- `/loop` goes in `COMMANDS` with the §7.2 grammar. `command_starts_turn` returns false for it.

**Tests:**
- the schema validates each verdict;
- a call outside a tick is refused with its string;
- a second call replaces the first;
- `/loop` parse table (each row of §7.2), including "stop" never read as a goal;
- the extension is absent from a chat with no loop.

**Must not break:** `/goal`, `/grind`, `/compact`, recipe slash commands, and the slash popover list (the builtin is
tagged Builtin, `G/acp/response_builder.rs:354`).

### L4: Start UI

**Confidence: MEDIUM-HIGH.**

**Owns:** see the table.

**Changes:**
- The Loop button and status chip in the L1-emptied slot.
- `StartLoopDialog.tsx`, which covers every §8.2 state and string.
- `LoopDriver.tsx`, mounted once in `AppLayout`. It subscribes to `loops/tickDue` and submits through
  `acpChatSessionController.submitMessage`, and subscribes to `system-resumed` → `loops/wake`.

**Tests:**
- the dialog's validation states;
- the refusal on a `swarm-build` chat;
- presets map to grammar strings;
- the Keep awake toggle calls the existing `set-wakelock`;
- the driver submits with the meta and id and is idempotent on a repeated `tickDue`;
- the chip's labels per status.

**Must not break:** the composer's other controls and the narrow rule (`ChatInput.tsx:367`).

### L5: The rail and the transcript

**Confidence: MEDIUM-HIGH.** `sessionChanges` is reused unchanged over slices. The risk is the ChangesRail split,
guarded by its existing 5 + 5 + 4 tests (§2.6).

**Owns:** see the table.

**Changes:**
- `SessionRail` renders the two pills and one panel with `Segmented as="tabs"`. The panel keeps today's size, overlay
  and Escape/focus behaviour.
- `ChangesRail` becomes `ChangesPill` + `ChangesPanelBody`, with no behaviour change.
- `LoopPanel`, `TickRow` and `LoopPill` implement §8.4.
- `UserMessage` renders the tick divider and adds "Loop this".
- The adapter maps `looptick_` ids.

**Tests:**
- ChangesRail's existing tests pass on the split;
- one test per §8.4 state (from L0's fixture);
- `TickRow` files equal `sessionChanges(slice)`;
- a quiet tick collapses;
- the divider replaces the bubble;
- "Loop this" prefills the goal;
- per-session open state survives a remount and survives a throwing `localStorage`.

**Must not break:**
- Q-190: the chat never changes width (a layout test measures the conversation column with the panel open and closed);
- the Changes pill's test id and strings.

### L7: Sidebar and activity

**Confidence: MEDIUM.** It adds a state to a precedence order that three surfaces read.

**Owns:** see the table.

**Changes:**
- `SessionActivityResponse` gains `looping: [{sessionId, status, nextTickAt}]`, from `session_loops::acp::list` in
  `on_session_activity`.
- The store gains `'looping'` with the §8.6 precedence, and `ActivityPills` gains `LoopingPill`.

**Tests:**
- precedence (running > needs-you > looping > idle);
- Active now is unchanged by a waiting loop;
- the pill text for waiting and paused.

**Must not break:** Q-185's background state and the running/needs-you rows.

### L8: Retire `LoopConfig`

**Confidence: MEDIUM-HIGH.** An ACP schema change. The regenerated `acp-schema.json` and the desktop's generated
SDK types must agree.

**Owns:** see the table.

**Tests:**
- scheduler tests for single-run jobs unchanged;
- a `schedule.json` fixture carrying `loop_config` loads and runs once (the migration rule, §6).

**Must not break:** SchedulesView, the agent's schedule tool, the CLI and REST.

### L9: The state harness and the live proof

**Confidence: MEDIUM.** §10.

---

## 10. Test plan

### 10.1 Unit

- **Rust:**
  - L0 rules and prompt over the shared fixture;
  - L2 runner with a fake door and injected time;
  - L3 tool and command;
  - L6 the moved clock;
  - L8 the scheduler.
- **TS:**
  - L0 `model.ts` over the same fixture;
  - L4 dialog and driver;
  - L5 rail, rows and divider;
  - L7 store precedence.
- **Gates:**
  - `cargo test -p goose-swarm --test development_gates`, which L6 and L2 must keep green with no baseline change;
  - clippy `-D warnings`.

### 10.2 State harness (`harness/loops-state.mjs`, L9)

It renders every §4.7 status and every §8.4 NOW-block and tick-row variant from the fixture.

**Widths and themes:** at 480, 1000 and 1600, light and dark.

**What it asserts:**
- every visible string resolves through i18n (no raw keys);
- every chip's ink/fill contrast is ≥ 4.5:1;
- no element has a left border wider than its other borders (the no-rail rule, measured);
- no fill has alpha < 1 (no faded tints);
- no native `select`, `alert` or `confirm` in the DOM or on `window`;
- the conversation column's width is identical with the panel open and closed (Q-190);
- the ended/paused/waiting sentences equal the fixture's expected strings, character for character.

### 10.3 Live walk over CDP (installed build, read-only)

This uses `mainPage()` (`harness/mainpage.mjs`), never the first `index.html` page.

**What it checks:**
- the composer shows Loop and not Recipes & loops;
- `/loop` status on a chat with no loop answers its string;
- the Start dialog opens and closes with every template's steps shown;
- the rail pills render in a chat with changes;
- `#/loop` no longer routes;
- `/recipes` and `/schedules` still render by URL.

Screenshots go to `~/goose-screenshots/loops-walk/`.

### 10.4 End-to-end journeys (on the owner's two Macs, the local engine)

**J1: five real ticks (the landing proof for Q-228).**
- **Setup.** A software chat on the 27B node, working dir = a fresh copy of the #3l work folder. Template Software
  quality loop. Goal: "make scripts/generate_users.js produce every problem class in notes/kickoff.md". Check:
  `node scripts/validate_users.js` (written by the brief first). Every 10m.
- **Pass criteria, every one checked, none by proxy:**
  1. five ticks recorded;
  2. each tick's marker, turn and `loop_report` are in the transcript;
  3. each row's files equal what the tick's tool cards changed;
  4. the state file was rewritten every tick (its Changes entry grows);
  5. the check ran after every tick with its exit code shown;
  6. `next_tick_at` minus the previous start equals the cadence, or "overdue";
  7. the owner can read the timeline and say what each tick did from the rail alone.
- **Words.** The tail of each tick's reply is read and quoted in `E2E-RUNS.md` (gate 7 practice): did it discover,
  critique, fix and prove, or restate?

**J2: the user's turn wins.** During tick 3, send a message in another chat on the same engine. Expect: the tick is
cancelled within that user turn's start, recorded `yielded`, and the next tick carries the yielded line. Send a message
in the loop's own chat during tick 4. Expect: it is a steer, visible in the tick.

**J3: stop rules.**
- Make the check pass → "Goal met".
- A no-check blank loop reporting done → the done sentence.
- Force a stalled tick (a goal already met with no check) → Stalled.
- Close the window mid-wait → reopen → paused-closed → Resume → one tick.

**J4: sleep.** `pmset sleepnow` during a wait, then wake after two cadences. Expect exactly one `on_wake` tick and
the wake sentence. This measures, rather than assumes, the `Instant`-during-sleep behaviour.

**J5: self-paced.** A watch loop with "goose decides". Expect each tick's reason shown. Force a tick with no
`next_in` (a model that omits it) → `waiting_you`.

**Status rule.** Each journey's result lands in `E2E-RUNS.md`. A journey not run is reported as not run, never implied.

---

## 11. Open questions (the work proceeds on each recommendation)

1. **Where the state file lives.** **Recommended: `.goose/loops/{slug}/NOW.md` in the working dir**, shown and
   editable in the dialog. Keeping it inside the working dir makes it visible, editable and diffable. The dot-folder
   keeps it out of the way. The owner's own spine lives in the repo, so a user who prefers the repo root types it there.
2. **Q-227: remove the button, or fold it into the Loop button.** **Recommended: remove, and put Loop in the slot**
   (§6). Folding keeps the three recipe paths nobody used, and each has a defect.
3. **Retire the scheduler's `LoopConfig` (L8), or leave it.** **Recommended: retire.** After L1 nothing in the product
   writes it; it is a second meaning of "loop" in the ACP API, and its iteration semantics (D5) and check swallow (D4)
   are the opposite of this design.
4. **Loops that run with goose closed.** **Recommended: not in v1.** The owner's loops are attended build sessions
   whose home is the chat. Agent Work is the product for work that runs unattended. A later step can "promote a loop
   to an agent" (the goal becomes the charter, the cadence carries over), measured on need.
5. **Keep awake by default while a loop runs.** **Recommended: no.** Show the existing wakelock toggle in the dialog
   and the wake sentence in the rail. Forcing wakefulness is a battery decision the user makes once.
6. **Work windows for session loops ("only 9–18 on weekdays").** **Recommended: not in v1.** The shared clock
   already supports it, so it is a dialog field and one record field whenever wanted.
7. **Ticks on a strategy's Build role.** **Recommended: no in v1.** A tick is a chat reply and uses the Chat chain.
   Routing ticks to Build would make a loop swap models every tick on a two-way strategy (DESIGN-NODES §6.4's delegate
   warning, twice per tick).
8. **Worktree isolation for software loops (the Codex pattern).** **Recommended: not in v1.** R5 wants the loop's
   changes beside the chat's own changes in the working dir. A worktree toggle can come later, if a loop's edits
   collide with the user's own.
9. **Does a renderer reload create a new `GooseAcpAgent` in the same goosed?** **Recommended: measure it first in
   L2** (reload the window with a loop armed, then check that `tickDue` reaches the new renderer). If it does not, the
   door keys on the goosed process and the reload re-subscribes. The design holds either way; only the registration
   point moves.
10. **Should `loop_report` be required, or inferred from the last message?** **Recommended: required, with the loud
    `no_report` state.** Inferring a verdict from prose is the fallback gate 1 forbids. L9's J1 measures how often
    the local models miss it before any wording change.

---

## 12. Invariants this design touches, and how each is kept

| Invariant | Kept by |
|---|---|
| Gate 1: no silent substitution | No default delay, no inferred verdict, no "check failed" for a check that could not run, no template state file, no `null` for an unreadable record. Each is a named state (§4.6, §8.4) |
| Gate 2: specific text | The tick prompt is assembled from the loop's facts and the user's own goal and steps (§4.4). The template steps are shown to and editable by the user before they reach a model |
| Gate 4: reaping | The check runs in its own process group, and only that group is killed on Stop (`subprocess.rs:55`) |
| Gate 5: no time input | The cadence only starts ticks. No tick, check or model call has a timeout. Stall is a repeat. No new seconds constant; the runner takes `now` as a value |
| Gate 6 in spirit: one door | Ticks enter through `submitMessage` → `on_prompt`, the door typed messages use. `/loop` does not start a turn itself |
| Gate 10: no absolutes | No new numeric const. Presets are grammar strings. The output tail is a share |
| The swarm engine is untouched | No edit in `swarm.rs`, `crates/goose-swarm/*` or the swarm command tree beyond L6's pure clock move, proven by `development_gates` |
| Q-132 turn priority | `after_user_turns` is unchanged. Ticks never take `user_turn()` and yield to one (§5.3) |
| Q-190 the chat's width | The rail stays an overlay; the L5 layout test measures it |
| Q-185 background kinds | A tick is a turn, not background. The fact check after a tick keeps its "Checking" kind |
| Owner UI rules | Solid `TONE_FILL` chips, no rails, no tints, Studio dialogs, i18n. Asserted by §10.2 |
| DESIGN-NODES §7.1 | Ticks are replies on the chip's route. The served record, the loader guard and the when-rules apply unchanged. The loop never loads a model |
