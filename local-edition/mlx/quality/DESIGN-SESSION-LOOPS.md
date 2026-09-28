# Session loops: design for lane L (Q-227, Q-228)

Written 2026-09-27 by the lane L architect. The code was read at `a781b6b29` (main). The installed app (3.0.62) was
walked read-only over CDP while E2E #3l ran in a chat; nothing was created, started or deleted, every dialog opened
was closed, and the window was returned to the chat URL it was found on (`#/pair?resumeSessionId=20260927_8`).
Screenshots: `~/goose-screenshots/loops-design/` (00 chat as found, 01 Recipes & loops dialog, 02 Changes rail open,
03 Agent Work row expanded, 04 an Agent Work desk, 05 Create an agent, 06 back in the chat). No product code was
written; implementers cut from this document.

**Revised 2026-09-27 after the adversarial review** (verdict CONFIRMED-WITH-CORRECTION: L1 and L6 could be cut as
written; L2, L3, L4, L5 and L8 could not). Every item was re-verified in the code at `978344982` (main, L1 landed as
`fdcac1d1a`) before it was accepted or rejected. §13 lists each item and what changed; the sections below are the
corrected design, and §9 is re-cut so a surgeon can cut without hitting a known defect.

Abbreviations: `S` = `ui/desktop/src`, `G` = `crates/goose/src`, `AW` = `crates/goose-cli/src/commands/swarm/agent_work`.

---

## 0. What this document decides, in fifteen lines

1. **A session loop is a chat that re-runs a stated goal, tick after tick, in the same session.** One tick is one
   ordinary agent turn in that chat, sent through the same prompt door a typed message uses. The loop owns: the goal
   (the user's words), a cadence, a state file, stop rules, and a tick ledger.
2. **Three cadences:** "Every 10m" (fixed, time between tick starts, an overrunning tick is never cut), "When goose
   decides" (self-paced: each tick names its next delay and why), "Right after each tick" (back to back).
3. **Each tick ends by calling one tool, `loop_report`** (verdict, summary, next step, and for self-paced the delay),
   and that call ENDS the turn (the `ask_user` mechanism, `END_TURN_META_KEY`). A named check command, when set,
   decides "goal met"; the model's "done" alone ends the loop only when no check is set, and says so. A missing
   report, a missing delay, a check that cannot run, a tick that asked the user: each is a named, visible state,
   never a default.
4. **The loop pauses itself on repeats, not counts:** the same failure twice in a row, two ticks in a row with no report,
   or a tick that made no write/edit outside its state file and named the same next step as the tick before it (stalled).
5. **The user's turn always wins.** A tick never starts while a user turn runs in this goosed, or while this chat's
   renderer has a turn or a queued message of its own. A user reply that contends with a running tick makes the tick
   yield (it is cancelled with a named cause, recorded `yielded`, never "You stopped this answer"), and the loop
   continues after that turn: in v1a any user turn in this goosed; after L2c only a user reply on the tick's own
   engine way, in any window on the Mac. A message typed into the loop's own chat during a tick is
   QUEUED, as for any running turn; the queue's "Send now" steers it into the tick, and the composer says so.
6. **The loop lives in the chat's right rail, beside Changes.** The rail stays an overlay (Q-190: it never narrows the
   chat). Collapsed, it is two pills. Open, it has two tabs, Loop and Changes. The Loop tab shows the loop's status,
   its next tick, the controls, and a tick timeline. Each tick row shows its verdict and summary, plus the files goose
   wrote or edited in that tick, computed by the same `sessionChanges` model over that tick's messages.
7. **You start a loop in one of three ways:** a Loop button in the composer, the `/loop` command, or "Loop this" on a
   message. Four starting templates: Software quality loop (discover → critique → fix → prove), Until a check
   passes, Watch and act, and Blank.
8. **The clock lives in goosed (one process-wide runner); the window's renderer fires the tick** through
   `acpChatSessionController.submitMessage`, the same door a typed message uses, carrying `_meta.goose.loopTick`.
   The runner OFFERS a tick (`loops/tickDue`); the offer stands until `on_prompt` accepts it; a renderer that cannot
   submit answers `loops/tickRefused{reason}` and the runner re-offers on the renderer's own "attempt cleared" event.
   A tick is never dropped silently. The runner owns the record, the prompt text and every decision.
9. **Honest limits.** Ticks run only while the chat's window is open, because each window runs its own goosed. A
   window closed mid-loop, or an app that quit and left its goosed orphaned (Q-223), comes back as "Paused, goose
   was closed". While the Mac sleeps no tick runs: on wake, one tick runs rather than a burst. "Keep awake" is the
   existing wakelock setting, which does nothing until Q-230's fix lands; the loop design depends on that fix and
   adds no blocker of its own.
10. **Q-227: the composer's "Recipes & loops" button and its whole dialog are removed.** Evidence of use on both Macs:
    0 schedules, 0 saved recipes, 0 recipe-driven user sessions out of 387, and the last scheduled session was
    2026-07-13. The recipe ENGINE is untouched (sub-recipes, CLI, deeplinks, the agent's schedule tool). The URL-only
    `/loop` view goes too, so "loop" has one meaning. The scheduler's `LoopConfig` is retired in its own slice.
11. **Agent Work stays the desk** (a multi-lane, fleet-wide, detached process), so there is one loop concept, not
    two engines. Session loops and desks share ONE clock, moved from `AW/window.rs` into the goose crate, and one
    vocabulary: tick, cadence, overdue, "Run a tick now", pause, stop.
12. **The nodes seam (§5.5):** a tick is a reply through `on_prompt`, so it runs on the session's chip (`node:` /
    `strategy:` / Auto), is covered by the S5 loader's per-reply guard, and gets a `nodes.served` record that its
    tick row shows. S5's holder records carry `kind: user | tick`, so a tick is never read as a user's reply, and a
    tick's demand never swaps a way a user's reply holds and never queues ahead of a user's demand. The loop never
    loads or swaps a model itself.
13. **Slices, in data order, with confidence** (§9): L1 LANDED (`fdcac1d1a`) · L6 one clock (high) → L0 contract,
    pure rules and the generated SDK (high) → L2a runner core (medium) ∥ L3 report tool, `/loop`, fork strip
    (medium) ∥ L4r the renderer door (medium) ∥ L7 sidebar (medium) ∥ L8 retire `LoopConfig` (medium-high) →
    **S5 merged** → L2b the server door in `on_prompt` (**low-medium**) → L2c the Mac-wide half (medium-low) · L4
    start UI and L5 the rail (medium-high, after L0/L4r) · L10 wake broadcast (after S7 and Q-230) → L9 harness
    and live proof (medium).
14. **No new time input decides model work:** a cadence decides only when a tick STARTS; nothing cuts a tick or a
    check. Gates 1, 2, 4, 5 and 10 are kept (§12).
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

## 2. What exists today (mapped at `a781b6b29`, re-checked at `978344982` for the review)

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
  tool (`G/agents/platform_extensions/needs_you.rs:17,89`). **A tick is a turn in a User session, so a tick can ask the
  user, and asking ENDS the turn**: the tool's result carries `END_TURN_META_KEY` (`G/needs_you.rs:22`,
  `needs_you.rs:128`), the agent exits the reply once that tool batch is in (`G/agents/agent.rs:2499-2506`), and the
  person's answer arrives as their next message (`NeedsYouCard.tsx:232-236`: `resolveNeedsYou`, then `sendAnswer`).
  The key is generic: any tool result carrying it ends the turn.
- **Background (Q-185).** `BackgroundWorkKind` (`needs_you.rs:104-127`) tags helper calls (fact check, title,
  compaction …) so the rows show "Checking". A tick is not background work: it is a real turn. But a tick's own
  end-of-turn reviewers ARE background work, and they run after the turn has cleared (§2.8), on the same model:
  under Q-132's static-batch shape a next tick started at once would sit behind them.
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

- **Desktop door.** `acpPromptSession` (`S/acp/prompt.ts:6-15`) sends only `{sessionId, prompt}`: no `_meta`
  and no message id reach the server. It is called only by `acpChatSessionController.submitMessage`
  (`S/acp/chatSessionController.ts:149, :165`), which **returns silently** when the session's snapshot already has an
  `activePromptAttemptId` (`:157-158`), and which does not append the message to the transcript: the caller does
  (`useChatSession.handleSubmit` calls `setMessages` first, `useChatSession.ts:186-192`). Callers:
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
  - It builds its own `Message::user()` from the content blocks (`convert_acp_prompt_to_message`, `:1310`). The only
    server-assigned id today is the steer's `steer_{uuid}` (`:3046`). `PromptRequest` does carry an ACP `_meta`
    field (agent-client-protocol-schema 1.1, `v1/agent.rs` `#[serde(rename = "_meta")]`); `on_prompt` never reads it.
  - It detects slash commands (`:2675-2702`), calls `agent.reply` (`:2712`), and on Stop breaks and drops the stream
    (`:2752-2760`). A cancelled turn is recorded by `turn_outcome::record_stopped` (`:2897-2916`), whose notice is
    "You stopped this answer after …" (`turn_outcome.rs:93-128`, `stopped_line` at `:116`).
  - The run is cleared (`clear_active_run`, `:2929`) BEFORE the response returns (`:3020`), and the end-of-turn
    reviewers (`assess_turn`, `check_turn_answer`) are spawned detached between the two (`:2975-3019`).
- **Steer is not the default.** A message typed while a turn runs is QUEUED by the composer
  (`ChatInput.tsx:1295-1297` → `handleInterruptionAndQueue`, `:1123`), and is sent as a new turn when the running
  one ends. It is steered into the running turn only through the queue's "Send now" button (`MessageQueue.tsx:243-256`
  → `handleStopAndSend`, `ChatInput.tsx:1461` → `onSteerQueuedMessage`, `useChatSession.ts:199-248` →
  `acpSteerSession`, server `on_steer_session`, `:3023-3060`). A slash command never steers
  (`useChatSession.ts:213-215` returns false). A message matching an interruption word ("stop", …) stops the running
  turn (`ChatInput.tsx:1131-1136`, `onStop`) and is sent after it.
- **Sessions and connections.**
  - A session does not have to be open to receive a prompt: `get_session_agent` loads it (`:2424-2457`).
  - **Every connection creates its own `GooseAcpAgent`** (`GooseAgentConnection::connect_to`,
    `self.server.create_agent()`, `server.rs:3359`), with its own `sessions` map and `active_prompt_runs`. So a
    renderer reload is a new `GooseAcpAgent` in the same goosed (§11 Q9 answered). `client_cx` is set once per
    connection at dispatch (`G/acp/server/dispatch.rs:20-24`) and used for notifications outside a request
    (`server.rs:2442`, `:3050`). A process-wide object (the runner) has no handle to any connection's Agent.
  - Notifications go to that connection only. A goose notification the generated SDK does not list (`acp-meta.json`
    `notifications`, today only `_goose/unstable/session/update`) falls to `callbacks.extNotification`
    (`ui/sdk/src/generated/client.gen.ts:2353-2365`), which the desktop does not define (`S/acp/acpConnection.ts:27-35`):
    it is dropped without a trace.
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
    the `ExtensionState` trait (`:44`). `update_extension_state` (`session_manager.rs:461-481`) is a read-modify-write
    of ONE key inside a `BEGIN IMMEDIATE` transaction, the right door for a record two writers touch;
    `set_extension_state` (`:484`) overwrites without reading.
  - The `todo.v0` key (`extension_data.rs:90`) is the model for a per-session record; `needs_you.v0`
    (`G/needs_you.rs`) is the model for one written through `update_extension_state`.
  - Forking copies it: `copy_session` copies `extension_data` whole (`session_manager.rs:2186`), and a user
    message's edit defaults to fork (`UserMessage.tsx:192`).
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
| D11 | Settings › Import writes `loop_config` through `acpCreateSchedule`, and reads an unparseable `schedule.json` as "no loops" | `GooseImportSection.tsx:88-98` (`catch { /* malformed schedule.json — no loops */ }`), `:161-170` | L8: the loop half names "Loops are no longer imported" per entry found, and an unreadable file is named with its error |
| D12 | Every cancelled turn is recorded "You stopped this answer", whoever cancelled it | `server.rs:2897-2916`, `turn_outcome.rs:93-128` | L2b: a cancel carries its cause; a tick's yield is recorded `yielded`, never as the user's stop |

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
| **Agent loop UX patterns** (the above and Agent Work's own desk page) | A tick log with verdicts; next-run countdown; pause/resume; "run now"; a steer channel ("Tell the desk"); human checkpoints (needs you). | Every one of these. Steer reuses the chat itself: the composer IS "Tell the loop" (a message typed during a tick is queued for after it, and the queue's "Send now" steers it in, §5.3). | A second steer box: the chat already has one. |

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
  the Changes model sees its edits, and the user can scroll, steer (through the queue's "Send now", §5.3), stop or
  fork it like any turn. **A fork does not carry the loop**: `copy_session` copies `extension_data` whole, so the
  fork path strips `loop.v0` and the loop extension from the copy (L3). An in-place edit that truncates the
  conversation before a tick's marker leaves that tick's row reading "This tick's messages were removed by an edit".
- The loop never picks a model. A tick runs on whatever the chat's chip names (§5.5).
- A loop cannot be put on a `swarm-build` session. The start dialog refuses with "Loops run chat turns. This chat
  builds with the swarm, so every tick would start a full build. Use Agent Work for recurring builds." A tick in
  that session would spawn a whole `goose swarm run` (`providers/swarm.rs:717`).

### 4.2 The loop record: `extension_data["loop.v0"]`, owned by goosed

Stored through `ExtensionState` (`G/session/extension_data.rs:44`) and written ONLY through `update_extension_state`
(`session_manager.rs:461-481`, one key, read-modify-write inside `BEGIN IMMEDIATE`), like `needs_you.v0`. Two
writers touch it (the runner, and the `loop_report` tool inside the tick's turn), and two goosed processes may race
an owner claim; `set_extension_state` would let the later writer erase the earlier one's change. The owner claim is
a compare-and-set inside that one transaction. The record survives restarts and replays with the session.

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
  offer: Option<Offer>,             // { n, message_id, offered_at, refused: Option<RefuseReason> } — §5.1; cleared by tick_started
  owner: Option<Owner>,             // { goosed_pid, goosed_started_at, app_pid }: which process runs the clock (§5.1)
  created_at, started_at, ended_at,
  ticks: Vec<TickRecord>,
}
TickRecord {
  n: u32, origin: TickOrigin,       // cadence | now | self_paced | back_to_back | after_your_turn | after_your_answer | on_wake | resume
  started_at, ended_at,
  first_message_id: String,         // the tick's prompt message (§4.4); the next tick's marker ends its range
  report: Option<LoopReport>,       // what loop_report said, verbatim
  outcome: TickOutcome,             // progress | done | blocked | asked{item_id, question} | failed{error_class, error}
                                    // | no_report | yielded{to_session, way} | stopped_by_you
  wrote: Vec<String>,               // paths goose wrote/edited in the tick (write/edit fileDiff), state file excluded
  check: Option<CheckRun>,          // { command, exit: Option<i32>, output_tail, log_path, ran: bool, error: Option<String> }
  served: Option<ServedRef>,        // the nodes.served record of the tick's lease (§5.5), when present
  tokens: Option<TokenDelta>,       // session token totals after − before: a measurement, shown, never a decision
}
```

- `asked` is decided by the runner, not by the model: after the tick's turn ends it reads the session's
  `needs_you.v0` and finds an item created during the tick that is still `Open` (§4.6). `ask_user` ends the turn
  (§2.7), so a tick that asks can never also report; without this read it would be recorded `no_report` and the next
  tick would fire under the pinned question.
- `wrote` is the Rust half of `sessionChanges`: the runner reads the tick's message range once when the tick ends
  and lists the paths of `write`/`edit` results carrying a file diff (the same rule as `with_file_diff_meta`,
  `server.rs:2205-2233`), with the state file removed. Shell commands that change files do not appear, so every
  sentence built on `wrote` says "write/edit", never "files changed".
- The record grows by one `TickRecord` per tick and is rewritten whole on each write. J1 (§10.4) measures its size
  after the run; a record that grows past what a write should carry moves the ledger to its own key, measured first.

Pure functions over the record live in `G/session_loops/rules.rs` (L0) and `S/components/loops/model.ts` (L0).
Both are pinned by one shared fixture, `G/session_loops/loops.fixture.json`, the pattern `nodes.fixture.json` uses.

### 4.3 Cadence: three kinds, one clock

| Kind | Record | Next tick | Label |
|---|---|---|---|
| Every | `every: "<n>s|m|h"`, parsed by the ONE grammar (`AW/window.rs:10-23`, moved in L6) | `DeskClock::next_tick(last_start, now)` (`AW/window.rs:117-147`): last start + cadence; if that time has passed, as soon as the running tick ends ("overdue") | "every 10 min" |
| When goose decides | `self_paced` | `report.next_in` from the tick just ended, parsed by the same grammar; the model's `next_reason` shown verbatim | "goose decides when" |
| Right after each tick | `back_to_back` | when the tick just ended has been recorded, its check has finished and its end-of-turn reviewers have ended | "back to back" |

**Rules, each paid for elsewhere:**
- The cadence decides only when a tick **starts**. No tick, check or model call is ever cut by time (gate 5).
- **No tick starts before the previous tick's end-of-turn work has ended**, whatever the cadence. `on_prompt` spawns
  the tick's `assess_turn` and `check_turn_answer` after the turn has cleared (`server.rs:2975-3019`); they run on the
  same model, and under Q-132's static batching a tick started beside them waits for their batch. L2b hands the
  runner the two spawned tasks' join handles, and the runner's "reviewed" event is their joint completion: an event,
  not a clock. A due tick waiting on it shows "Next tick after goose's check of tick {n}".
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
State file: {state_file} — read it before anything else; rewrite it before you call loop_report
(Now · Next · Found · Done; keep it short enough to read in one go).
What each tick does (the user's steps, as they left them in the dialog):
{steps}
{f = the last FINISHED tick; a tick a yield or the user's stop cut short is unfinished (Q-278)}
Last tick ({f}, {time}, {outcome}): "{report.summary}" — next step it named: "{report.next_step}"
   ("Last finished tick" when unfinished ticks follow it; "No tick of this loop has finished yet." when none did)
{if check ran}  Check `{check}` after tick {f}: {passed | exited {code}}. Its output ended with:
{output_tail}
{if check could not run}  Check `{check}` could not run after tick {f}: {error}.
{if outcome was asked}  Tick {f} asked the user "{question}"; {they answered: "{answer}" | they dismissed it | it is still open}.
{per unfinished tick k}  Tick {k} ({start}) did not finish: {it was stopped at {time} for the user's turn in "{chat, named as it is NOW}" | the user stopped it at {time}}.{ Before it stopped, it reported: "…" — next step it named: "…".} It wrote or edited {files | no file outside the state file}; any command it ran is in the conversation above.
{if any unfinished}  Tick {n} carries on from there: what tick {k} left unfinished is part of this tick's work, not a tick of its own.
{if self_paced}  Say when to come back: next_in ("10m", "2h") and why.
Finish by calling loop_report; calling it ends this tick.
```

- **Every line is a fact of THIS loop or a rule branched on one** (gate 2). The fixed connectives ("read it before
  anything else", "Finish by calling loop_report") are instructional constants branched on measured predicates
  (self-paced or not, a check or not, a yield or not), the legitimate class `development-gates.md` §1 names; none
  asserts context that may not exist (GEN-4): the "last tick" line appears only when a tick n−1 exists, the check line
  only when a check ran, and so on. The steps are the text the user saw and left in the dialog (§7.4).
- **Message id.** The runner mints the id `looptick_{loopId}_{n}_{uuid}` when it makes the offer, and `tickDue`
  carries it. `on_prompt` stamps it on the message it builds (`user_message.with_id(..)`) ONLY when the prompt's
  `_meta.goose.loopTick = {loopId, n, messageId}` equals the runner's open offer for that session; anything else is a
  plain user prompt. That is the `steer_` precedent (`server.rs:3046`: a server-minted id on a server-built message),
  and the renderer's local marker uses the same id, so the live transcript and a replay from `sessions.db` agree with
  no change to the core `Message` type. `parse_tick_id` is one function in Rust (L0) and one in TS (L0), pinned by
  the fixture.
- **`output_tail`** is bounded by the chat's own context window, not by the check's output (a share of an unbounded
  output is unbounded): the longest suffix of the output whose token count, measured with goose's own `TokenCounter::count_tokens`
  (`G/token_counter.rs:53`, the counter compaction uses), is at most `window / 64`, where `window` is the session's resolved context limit
  (the provider's `get_context_limit`, `providers/swarm.rs:693` for swarm ids, which DESIGN-NODES §7.1 makes route-aware). `// ratio: 1/64 of the session's context window`
  is the one new numeric const, carrying its marker (gate 10): on this fleet's 262,144-token window that is 4,096
  tokens, about one screen of test output, and it keeps a tick prompt that also carries the conversation well inside
  the window. The FULL output is written to `<data_dir>/loops/{loopId}/check-{n}.log` (goose's data dir, never the
  user's working dir) and the rail's [Open log] shows it; only the tail reaches a model.

**The `loop_report` tool** comes from the `loop` platform extension (`G/agents/platform_extensions/loop_report.rs`, L3),
registered `default_enabled: false`, `hidden: true`. It is present on the session's agent for the loop's whole life
and absent otherwise, so a chat without a loop never carries the tool, and a loop chat's tool list does not change
between ticks and user turns (a tool list that flips every turn would bust the local engine's prompt-prefix cache).
**Who adds and removes it.** The process-wide runner has no handle to any connection's Agent (§2.8), so it never
does. `session_loops::agent_sync::sync_loop_extension(agent, session_id, record)` (L3) makes the agent's extension
set match the record (present iff the loop is not `ended`), and it is called from the three places that DO hold the
session's Agent: the `loops/start` and `loops/control` handlers (which run on a connection's `GooseAcpAgent` and call
`get_session_agent`) and `on_prompt` before `agent.reply` (L2b). It is idempotent. A loop the runner ends on its own
(a passing check) keeps the extension until the session's next prompt syncs it, and `loop_report` refuses outside a
tick in the meantime. `agent.add_extension` persists the extension state with the session (`agent.rs:1492-1510`), so
a reload carries it and a fork must strip it (L3).

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

The tool validates and stores the report against the running tick (through `update_extension_state`), then answers
"Recorded. This tick ends now." and **ends the turn**: its result carries `END_TURN_META_KEY` (`G/needs_you.rs:22`),
exactly as `ask_user` does, so the agent exits once that tool batch is in (`agent.rs:2499-2506`). Local models either
keep generating after their final tool (r6f: "Final output successfully collected." at 19:49:04Z, then GENERATING
again at 19:53, `TICK-NOTES.md:739`) or never call it (r4b: `final_output` never taken, `development-gates.md` §7); ending the turn on the call removes the first
failure, and the loud `no_report` state names the second. A call outside a tick is refused with "No loop tick is
running in this chat." A second call in the same batch replaces the first; the ledger keeps the last. A call that
fails validation is refused with the field's error and does NOT end the turn, so the model can call it again.

### 4.5 The state spine

The **state file** is a file in the session's working dir. Its default is `.goose/loops/{slug}/NOW.md`, where
`{slug}` comes from the goal's first words; the dialog shows the path and the user can change it. The tick reads it
first and rewrites it last.

- **Why a file and not session notes.** It survives compaction, because it is re-read every tick. The user can open
  and edit it, which is how the owner steers his own quality loop. Its rewrites show in Changes like any edit, as
  `+N −M` on the spine. And it is the owner's own practice (`NOW.md:1`).
- **Why not the todo scratchpad** (`todo.rs:91-106`). It is invisible to the user, injected into every turn of the
  session (not only ticks), and a single overwrite slot that the model also uses for its own todo list.
- **The runner never writes the file.** When a tick ends and the file does not exist (a `stat`, not a diff: the model
  may write it with a shell command, which leaves no diff), the rail says "State file not written yet". There is no
  template content (gate 1).
- **The state file is excluded from every "did this tick change anything" rule** (§4.6 stalled, §8.4 quiet ticks):
  the tick is told to rewrite it every time, so counting it would make every tick look productive.

### 4.6 Stop conditions and self-pause rules

| Condition | Who decides | Result |
|---|---|---|
| **Goal met, with a check set** | `check` exits 0 after a tick whose verdict is `done` or `progress` | **Ended**: "Goal met — `{check}` passed after tick {n}" |
| **Goal reported done, no check set** | the tick's `loop_report.verdict = done` | **Ended**: "goose reported the goal done after tick {n} — no check was set" (the model's own claim, labelled as such) |
| **Reported done, check fails** | check exits ≠ 0 | loop **continues**; the next prompt carries "you reported done; `{check}` exited {code}" and the tail |
| **Check could not run** | spawn error, missing shell | **Paused**: "The check could not run: {error}" (never read as "failed"; the D4 shape refused) |
| **Blocked** | `verdict = blocked` | **Paused**: "Blocked — {blocked_on}" |
| **The tick asked you** | after the tick's turn ends, `needs_you.v0` holds an item created during the tick that is still `Open` (the check does not run; a report, if any, is kept) | **Needs you**: outcome `asked{item}`. No further tick while the item is open. Answered: the next tick is offered when the turn that carried the answer has ENDED (origin `after_your_answer`). Dismissed: the next tick is offered at once, its prompt saying so |
| **The user's count** | `stop_after_ticks` reached | **Ended**: "Reached {k} ticks, as you set" |
| **The user stops the loop** | Stop loop (rail) / `/loop stop` | **Ended**: "Stopped by you after tick {n}". A running tick is cancelled with cause `loop_stopped`, keeps its partial, and is recorded `stopped_by_you` |
| **The user stops the running tick** | the composer's Stop, Escape, or an interruption word typed in the loop's chat (`ChatInput.tsx:1131-1136`) — any cancel that carries no loop cause | tick `stopped_by_you` (the ordinary "You stopped this answer" notice is right here: the user did stop it). **Paused**: "You stopped tick {n}" + [Resume]. The loop never fires the next tick into a chat the user just stopped |
| **Yielded** | a user reply on the same way started while the tick ran (§5.3) | tick `yielded{to_session, way}`; not a pause. The next tick is offered after that user turn (origin `after_your_turn`) |
| **Same failure twice** | tick n and n−1 both `failed` with the same `error_class` | **Paused**: "Ticks {n-1} and {n} failed the same way: {error}" |
| **No report twice** | tick n and n−1 both `no_report` | **Paused**: "Ticks {n-1} and {n} ended without a loop report" |
| **Stalled** | tick n: verdict `progress`, `wrote` empty (no write/edit outside the state file), `next_step` equal (whitespace- and case-normalised) to tick n−1's | **Paused**: "Stalled — tick {n} named the same next step as tick {n-1} and made no write or edit outside the state file" |

**How these rules are built:**
- Every self-pause is a **repeat of the previous tick**, the same evidence shape as the judge's repeat trigger. None
  is a count or a clock (gates 5, 10).
- A single failed tick does not pause the loop. The next tick's prompt carries the error.
- `asked`, `yielded` and `stopped_by_you` never count toward "no report twice": each is its own named outcome.
- **The check runs** in the session's `working_dir`, spawned through `configure_subprocess` (`G/subprocess.rs:51-58`,
  `process_group(0)`), so it leads its own process group. **Stop check** goes through the ONE sanctioned group kill
  goose already has, `goose_sidecar::sigkill_owned_group(pid)` (`crates/goose-sidecar/src/lib.rs:719-724`, proof-gated
  by `owns_process_group`: `getpgid(pid) == pid` and `pid != getpgrp()`); when the proof fails, the pid alone is
  signalled and that is logged, exactly as the sidecar does. No new group-kill site is written (gate 4).
- **The check ends at its exit status, never at pipe EOF.** A grandchild the check leaves running (a dev server a
  test script started) keeps the pipe's write end open, and an EOF reader parks forever (swarm invariant 5, r0's
  20-minute hang). The runner reads stdout/stderr concurrently into the log file, treats `child.wait()` as the end,
  drains what is buffered without blocking, and drops the readers.
- **The check has no timeout.** The rail shows "Checking `{check}` · {elapsed}" and a [Stop check] button. A stopped
  check is recorded `ran: false, error: "stopped by you"`, and the loop pauses with that reason.

### 4.7 States

| Status | Meaning | Rail pill | Sidebar marker |
|---|---|---|---|
| `running` | a tick's turn is in flight | "Tick {n} running · {elapsed}" (ok fill) | existing Running pill |
| `checking` | the check command runs after tick n | "Checking · {elapsed}" (accent) | Looping pill |
| `waiting` | next tick scheduled | "Next tick {HH:MM}" (accent) | Looping pill "Next {HH:MM}" |
| `waiting_turn` | a tick is due but must wait: a user turn runs in this goosed, this chat's renderer refused the offer (its own turn or queued message), the previous tick's reviewers still run, or (after L2c) a user reply holds the way the tick would swap | "Next tick after your turn" / "…after goose's check of tick {n}" / "…after {node} finishes answering you" (secondary) | Looping pill |
| `waiting_you` | self-paced tick named no delay | "Loop waiting for you" (warn) | Looping pill in the warn fill, NOT in Active now (it is not a question) |
| `needs_you` | tick n ended by asking (`ask_user` ends the turn) and the item is still open, or its answer's turn is still running | "Loop needs you" (warn) | existing NeedsYou (a User session, `G/needs_you.rs:80-90`) |
| `paused` | by you, by a self-pause rule, or because the owning goosed is gone; `status_reason` names which | "Loop paused" (stopped fill) | Looping pill "Paused" |
| `ended` | a stop condition held | "Loop ended" (stopped fill) | none |
| `elsewhere` | another goosed owns the clock and is PROVEN live and attached (§5.1) | "Looping in another window" (secondary) | Looping pill |

A due tick's offer (§5.1) is not a status: while offered and not yet started, the status stays `waiting` with the
NOW line "Next tick starts now".

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

- **`G/session_loops/runner.rs` (L2a) is a process-wide runner**, a `LazyLock` like `turn_priority`'s `PRIORITY`
  (`turn_priority.rs:85`). It holds the loops this goosed owns. It never touches an `Agent`: everything it needs from
  a turn reaches it as a value from `on_prompt` (§5.2).
- **Owner.** `owner = {goosed_pid, goosed_started_at, app_pid}`, where `app_pid` is goosed's parent (the Electron
  main process) at the moment of the claim.
  - **Claim only on intent.** The runner claims a loop ONLY on `loops/start`, `loops/control{resume}` and
    `loops/control{tickNow}`: a compare-and-set of `owner` inside one `update_extension_state` transaction (§4.2), so
    two windows racing produce exactly one owner. **`loops/get` and `loops/list` are pure reads**: they never claim
    and never write.
  - **Proof of gone** (the `machine.rs` pattern, extended for Q-223). The recorded owner is gone when ANY of:
    1. its pid is not alive;
    2. its pid is alive with a different start time (pid reuse);
    3. its pid is alive but its parent is no longer `app_pid` (reparented to launchd, ppid 1): the app that owned
       it has quit and left it orphaned, which Q-223 measured on every quit/update, and whose renderer is gone with
       the app.
  - **The owner releases its own loops** when its last tick door closes (below): it writes `owner = None`,
    `status = paused{closed_at}` for every loop it owns. This covers an orphaned goosed that is still alive and could
    otherwise hold a claim nobody can see.
  - **What a read shows.** A record whose owner is proven gone and whose written status is `running`/`waiting` is
    RETURNED with the derived status `paused{closed}` ("goose was closed at {time}; {k} ticks were due", `k` derived
    from `next_tick_at` and the cadence at read time), without writing. [Resume] claims it. Only a live, attached
    owner reads as `elsewhere` ("Looping in another window"), which is the legitimate case of one chat open in two
    windows.
  - **Control from the other window** (Pause, Stop loop on an `elsewhere` loop) writes the record without claiming.
    The owning runner is in another process and gets no in-process event for it, so it re-reads the record inside
    the same transaction that makes each offer and at each tick end: a paused or stopped record is never offered
    again. A tick already running in the owner's window is not reachable from here (no handle crosses processes); it
    finishes, and the rail says so: "Paused — tick {n} is finishing in the other window".
- **Waiting.** The runner waits on a `tokio::sleep_until` computed from the WALL time `next_tick_at`. It re-evaluates
  (it never "catches up") on these events: a record change (control, edit); a tick ending; the previous tick's
  reviewers ending (§4.3); a user turn ending in this goosed; a door registering; a `loops/tickRefused` or
  `loops/ready` from a renderer; a needs-you item resolved; and a `loops/wake` call on the system's resume (§5.4).
- **The door to the window.** Every connection has its own `GooseAcpAgent` (§2.8; §11 Q9 is answered). The runner
  keeps a set of **tick doors**, one per live connection:
  - **registered** in `dispatch.rs` beside `client_cx.set` (`:24`), the first moment the connection's `cx` exists;
  - **deregistered** when the connection ends: `GooseAgentConnection::connect_to` (`server.rs:3355-3368`) holds a
    door guard across `.connect_to(client).await`, and its drop removes the door. A dead connection is detected by
    the end of its own serving future, not by a send error or a clock.
  - A goosed normally has one renderer connection; a reload replaces it (old door drops, new door registers, and the
    runner re-offers every open offer on the new door).
- **The offer.** When a tick is due and nothing it must wait on holds, the runner mints the message id, writes
  `offer = {n, message_id, offered_at}` into the record, and sends `_goose/unstable/loops/tickDue {sessionId, loopId,
  n, messageId, prompt}` on the doors. The offer stands until `on_prompt` accepts it (`tick_started`). It is
  idempotent: re-sending the same `(loopId, n, messageId)` never produces two ticks, because `on_prompt` accepts one
  offer once. It is re-sent when a door registers and on `loops/ready`.
- **Renderer answers.** The renderer either submits the tick (§5.2) or answers
  `_goose/unstable/loops/tickRefused {sessionId, loopId, n, reason}` with `reason` one of `turn_running` (the store has
  an `activePromptAttemptId`), `queued_message` (the composer's queue for that chat is not empty),
  `pending_cancel`, `load_failed{error}` (the session could not be loaded into the store), `submit_failed{error}` (the
  server refused the prompt). The runner records the refusal on the offer, sets `waiting_turn` with the
  reason's sentence, and does NOT re-send on a timer. The renderer sends `_goose/unstable/loops/ready {sessionId}`
  when the refusing condition clears — the store's attempt clearing and the queue emptying are events it already has
  — and the runner re-sends. So no offer is ever dropped silently, and nothing polls.
- **Both notifications reach the renderer through the generated SDK.** `loops/tickDue` and `loops/changed` are added
  to goose's custom notification schemas (`goose-sdk-types/src/custom_notifications.rs`), `acp-meta.json` and
  `acp-schema.json` are regenerated (`just generate-acp-schema`), and `ui/sdk/src/generated/*` is regenerated
  (`just generate-acp-types`; `just check-acp-schema` refuses a stale copy). The generated dispatcher then routes
  them to typed callbacks, which `S/acp/acpConnection.ts` defines. Without that, they would fall to the undefined
  `extNotification` and vanish (§2.8).
- **The hands are the renderer's.** `S/components/loops/LoopDriver.tsx` (L4r) is mounted once per window
  (`AppLayout`). On `tickDue` for a session it:
  1. ensures the session is loaded in the store (`acpChatSessionController.loadSession`) when the store has no
     snapshot for it — a tick may be for a chat the window is not showing;
  2. calls `acpChatSessionController.submitMessage(sessionId, marker, {..., meta: {goose: {loopTick: {loopId, n,
     messageId}}}, preAppend: true})`. `marker` is the tick's prompt as a user message with id `messageId` and
     `metadata.loopTick`. `submitMessage` (L4r) now RETURNS a status instead of returning silently: `'busy'` (an
     attempt is active; a pending cancel is checked by the driver first, since that path throws) with no side effect, or `'submitted'`, having appended `marker` to the
     transcript only after the busy check passed (the atomic version of what `handleSubmit` does with `setMessages`
     first). `acpPromptSession` (L4r) passes `_meta` through.
  3. on `'busy'`, or when the composer's queue for that chat is non-empty, answers `tickRefused` instead.
  - Why the renderer fires rather than goosed calling `agent.reply` itself: goosed's notifications for a turn go to
    the connection that asked for it (`on_prompt` writes to `cx`), and the renderer's session store is built around
    turns it submits. A server-internal turn would stream to nobody. Firing through the renderer makes a tick follow
    exactly the same path as a typed message.
- **A renderer reload mid-tick** (named risk, measured first in L2b). The tick's `on_prompt` runs on the OLD
  connection's `GooseAcpAgent`. Two outcomes are possible and L2b measures which one this ACP stack produces (close the
  websocket mid-prompt, observe): (a) the old `on_prompt` future runs to completion — then the tick finishes unseen,
  the new renderer sees `running` from `loops/get`, a user prompt to that session is refused by the busy set ("session
  is busy in another run", `server.rs:2486-2494`) and the composer shows "Tick {n} is finishing in the background",
  and on `loops/changed{ended}` the chat reloads the session to show the tick's turn; (b) the future is dropped —
  then `clear_active_run` never runs and the session stays busy forever. For (b), L2b adds a synchronous drop guard
  on the run registration that removes the `active_prompt_runs` entry and the manager's cancel token, and tells the
  runner the tick ended `errored{"the window reloaded during the tick"}`. The design holds either way; only (b) adds
  the guard, and it serves every prompt, not only ticks.

### 5.2 The one prompt door: the tick branches in `on_prompt`, no second path

`on_prompt` (`server.rs:2636`) stays the door for both. **Order: S5 lands its reply guard in `on_prompt` first; L2b
cuts on top of it** (DESIGN-NODES §9 S5 owns that edit). L2b's branches, keyed by `args.meta`'s
`goose.loopTick = {loopId, n, messageId}` **matching** the runner's open offer for that session
(`runner::accept_offer`; a forged or stale meta gets `None` and the prompt is an ordinary user prompt in every
respect). `accept_offer` RESERVES the offer as a guard: `tick_started` confirms it, and any exit of `on_prompt` before
that (`start_active_run`'s busy refusal, a `get_session_agent` error, an early cancel) drops the guard, which returns
the offer to open and re-sends it on `loops/ready`, so no exit path loses a tick and none accepts it twice:

1. **User turn or tick, decided first.** `turn_priority::user_turn()` (`:2647`) is taken only when the prompt is not
   an accepted tick. The S5 reply guard is opened with `kind: tick` for a tick and `kind: user` otherwise (§5.5).
2. **The message.** For a tick, the built message takes the offer's id (`user_message.with_id(messageId)`).
3. **Extension sync.** Before `agent.reply`, `sync_loop_extension(agent, session, record)` (§4.4); idempotent.
4. **`tick_started(ticket)`**, where the ticket carries the run's own `CancellationToken` (the same token
   `active_prompt_runs` holds) and a `cause` cell (`Arc<OnceLock<CancelCause>>`, stored beside the token in
   `ActivePromptRun`). This is the runner's ONLY handle on the tick: it cancels by setting the cause and cancelling
   that token, never through an `Agent` or a connection. `on_cancel` (a user's Stop) cancels without a cause.
5. **A cancelled tick.** When `was_cancelled` and the cause is `yield`, `record_stopped` is SKIPPED (no "You stopped
   this answer" notice, no `stopped` turn outcome, so no sidebar Stopped pill); the tick is recorded `yielded`. With
   cause `loop_stopped` or no cause, `record_stopped` runs as for any turn (the user did stop it) and the tick is
   `stopped_by_you`.
6. **`tick_ended{completed | cancelled{cause} | errored{error}}`** at the exits that call `clear_active_run`, so the
   runner can read the report, the needs-you store and `wrote`, and start the check.
7. **`tick_reviewed`**: the two reviewer tasks (`assess_turn`, `check_turn_answer`, `:2975-3019`) are spawned as today;
   for a tick, their `JoinHandle`s go to a spawned task that awaits both and then tells the runner. The response is not
   delayed.
8. **`user_turn_ended(session)`** for every non-tick prompt at its end, so the runner can offer a tick that was waiting
   on this chat's turn, or on the turn that carried a needs-you answer.

Everything else is unchanged for ticks: slash-command detection, the reply stream, cancellation, the `link_serve`
tap (`:2726`), the S5 loader's per-reply guard and the served-turn record.

### 5.3 The user's turn always wins

- **A tick is not offered while a user turn runs** in this goosed. `turn_priority` gains one public fn (L2a):
  `wait_no_user_turn() -> started` (the first half of `after_user_turns`, `turn_priority.rs:64-68`). A tick due
  during a user turn is `waiting_turn` and is offered when the turn ends. Its origin is `after_your_turn` (one tick,
  not one per missed interval).
- **Nor while this chat's renderer is busy**: the offer protocol's `tickRefused{turn_running | queued_message}`
  (§5.1). A message the user queued while a tick ran is therefore sent BEFORE the next tick, as a queued message is
  for any turn.
- **The yield: a user reply that starts while a tick runs stops the tick only where the two contend.** The review
  named the risk that cancelling the whole tick is wasteful where the engine could serve both. Q-132's receipt is
  specific: the pipeline split batches statically, so a user's request waits behind a running batch on the SAME
  engine. A user turn on a cloud node, or on another node, does not wait for the tick. So:
  - **v1a (L2a/L2b, before L2c):** in-process only, and the way is not yet known when a user turn starts, so a user
    turn that starts in this goosed while a tick runs yields the tick (`user_turn_started_since(started)`, the second
    half of `after_user_turns`, `:69-74`). This is the conservative rule, stated as such.
  - **v1b (L2c, after S5):** the trigger is the user reply's LEASE, not its start: when a `kind: user` holder opens or
    moves onto the way the tick's own holder uses (S5's holders record, observed on the change events S5 exposes), the
    tick yields. A user reply on a different way, a cloud node, or an LM Studio node the tick does not use does not
    yield it. The same holders make this Mac-wide (D9): a user reply in ANOTHER window's goosed is a holder too.
  - The runner cancels through the ticket (§5.2 step 4) with cause `yield`. The tick keeps its partial work, is
    recorded `yielded{to_session, way}`, and is re-offered after that user turn as tick n+1, origin `after_your_turn`.
    Its prompt says what happened.
  - **Why cancel instead of `after_user_turns`' drop-and-re-ask.** A dropped reply future would skip
    `clear_active_run`, and the session would read as busy. Re-asking a tool-using turn from the start would also
    repeat its edits.
- **A message typed into the loop's own chat during a tick is QUEUED** (`ChatInput.tsx:1295-1297`), exactly as while
  any turn runs; it is sent as an ordinary user turn when the tick ends, and the next tick waits for it. Its queue row's
  **"Send now"** steers it into the running tick (`handleStopAndSend` → `onSteerQueuedMessage` → `on_steer_session`).
  An interruption word stops the tick (§4.6: `stopped_by_you`, the loop pauses). The composer's Stop button stops the
  tick the same way. **Only the rail's Stop loop and `/loop stop` stop the LOOP immediately** (§7.2: typed `/loop`
  controls are sent as `loops/control` while a tick runs, never queued, because slash commands never steer).
- **A message typed into the loop's chat between ticks is an ordinary user turn.** The next tick waits for it and
  then reads it as part of the conversation.

### 5.4 App closed, window closed, Mac asleep: the honest limits

| Situation | What happens | What the user sees |
|---|---|---|
| The chat's window is open (any chat shown) | ticks fire | normal states |
| The window is closed (its goosed is released, `main.ts` `mainWindow.once('closed', … releaseWindow)`) | the connection ends → the door drops → the runner, with no door left, releases its loops (`owner = None`, `paused{closed_at}`); if goosed dies first, proof of gone covers it | reopening the chat: **Paused** "goose was closed at {time}; {k} ticks were due" with [Resume] (one tick now) and [Stop loop] |
| The app quits | same as closed. If the quit leaves goosed orphaned (Q-223, open) or hangs (Q-229, open), the orphan either released its loops when its renderer's connection ended, or is proven gone by its reparenting (§5.1 condition 3); the new window never shows "Looping in another window" for it | same |
| The Mac sleeps | tokio's clock does not advance during sleep (Rust's `Instant` on macOS does not count suspended time), so a timer armed before sleep would fire late by the sleep's length | **L10** adds `powerMonitor.on('resume')` in main → `system-resumed` to every window → the renderer calls `loops/wake` → the runner re-reads the wall clock: one tick if one or more were due (origin `on_wake`, "Missed while your Mac slept — ran once on wake"), never a burst (launchd's coalescing). This is NOT in L1: `fdcac1d1a` left it out because `main.ts` is S7's file; L10 cuts it after S7 merges. J4 measures the `Instant` behaviour rather than assuming it |
| Keep awake | the existing wakelock setting (`main.ts:2823` `set-wakelock`), **which today saves the setting and keeps nothing awake (Q-230, open, cutting)**. The loop design DEPENDS on Q-230's fix and adds no loop-specific blocker (§11 Q5): Q-230's stated fix mentions "while a loop is armed, lane L" — lane L declines that half in v1 | the start dialog shows the toggle only once Q-230 has landed (L4 depends on it). Before that the dialog says "Your Mac may sleep; ticks wait until it wakes." and nothing more |

**Why no daemon in v1.** Every loop the owner describes is an attended build session in an open chat. A loop that
runs with the app closed has no chat to show its changes in, which contradicts R5, and Agent Work already covers that
case (§11 Q4).

### 5.5 Local-model cost, and the seam with DESIGN-NODES-AND-STRATEGIES §7.1

**One tick costs:**
- one agent reply, which may make several model calls with tool calls, on the node the chat's chip names;
- the end-of-turn reviewers goose already runs after every reply (Q-185's "Checking the reply", `background_work::run`),
  which the next tick now waits for (§4.3);
- the check command, which is a shell call and no model work.

**Everything goes through §7.1's doors unchanged:**
- The model id is the session's (`swarm`, `node:<id>`, `strategy:<id>`). The router leases a node, applies the
  when-rule, and writes the **served-turn record** (`nodes/served.rs`, key `nodes.served`). The tick row shows
  "on {node}" from that record; when S3 has not landed, the row omits it rather than guessing.
- **The S5 loader's per-reply guard** in `on_prompt` holds the way for the whole tick, because a tick is a reply.
- **The holder record says it is a tick.** DESIGN-NODES §6.4 defines a goosed holder as `{session, root_session, way}`
  with no kind, and the loader treats every holder as a reply to protect. A tick IS a reply holder, so without a kind
  two loops would read each other's ticks as user replies (each tick "waits for a reply in flight" behind the other's
  tick), and the yield of §5.3 could not tell a user's reply from another loop's tick. **Correction, requested of the
  S5 surgeon now:** `holders.rs` carries `kind: user | tick` on each open reply; `on_prompt` opens the guard with the
  kind §5.2 step 1 decided. The sentence the first draft quoted is DESIGN-NODES §0 item 6.2 ("no agent **reply** (not a
  single model call) that was open before this demand still uses it, in any goose process on this Mac"), not §6.4
  step 2 as it was cited; its operative form is §6.4 step 7.
- **A tick never displaces a user.** §6.4 step 8 makes a reply that opens after a queued swap demand wait behind the
  swap; if the queued demand is a tick's, a user's reply would wait behind a loop. So, for a demand whose reply is
  `kind: tick`, the loader (L2c's branch in `nodes_loader.rs`, handed from S5):
  1. never swaps while a `kind: user` holder is open, in any goose process, on a way in its stop set: it WAITS
     (`Wait("{node} is answering you in {chat}; the loop's tick loads {target} when it finishes")`, the runner's
     `waiting_turn` sentence);
  2. never takes a FIFO place ahead of a `kind: user` demand: a user demand that arrives while a tick demand waits, and
     before the swap claim is taken, goes in front of it;
  3. once the claim is taken, runs to its end like any swap (§6.4 step 12's rule; a half-stopped Mac is worse).
  A user's reply that opens after the tick's swap has started waits for that swap, as any reply would; the tick then
  yields to it (§5.3) the moment it leases the way.
- A tick on a node that is not loaded follows the node's `ifNotLoaded` rule. When that rule is `load`, each tick may
  swap the engine between the user's replies. The start dialog says so when it can tell: "Each tick may load {node}
  and stop {serving way}" (from `nodes/residency`), and the displaced chat's notice names the loop ("27B was stopped
  for loop tick 5 in 'Kickoff notes'; your next message loads it back").
- A `strategy:<id>` chat's tick uses the Chat chain, like any reply. **Nothing in v1 routes ticks to the Build role**
  (§11 Q7).

**What the loop never does:**
- choose, load or swap a model;
- register as an engine holder of its own beyond its ticks' reply holders (`kind: tick`); S8's holders are swarm runs.

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
- **Gate 2:** the tick prompt is built from the loop's facts (§4.4); the template steps are parameterised by the
  loop's own slots and shown to the user before any model sees them (§7.4).
- **Gate 10:** ONE new numeric const, carrying its marker: the check's output tail is `1/64` of the session's context
  window (`// ratio: 1/64 of the session's context window`, §4.4). The presets are grammar strings.
- **Gate 4:** Stop check goes through `goose_sidecar::sigkill_owned_group`, the existing proof-gated group kill; no new
  group-kill site (§4.6).
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

The dialog's three actions each carry a defect (D1–D3). "Manage" duplicates the Skills nav item. The button showed only
for the `swarm` provider, and that is where most chats live: of the 130 user sessions created since 2026-09-01 on this
Mac, 76 are on `swarm`, 18 on other providers and 36 carry no provider name (`sessions.db`, read-only, 2026-09-27; the
review counted 75 of 89 earlier the same day). So the button was in front of most chats, and still unused (§2.9). Its
"loop" means N back-to-back runs in separate sessions (D5), which is not what the owner means by a loop.

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

**L8 retires `LoopConfig`, every dependent enumerated by `rg -n 'loop_config|loopConfig|LoopConfig'` at `978344982`:**
- the struct and field (`scheduler.rs:107-117, :141-142`), the iteration branch (`:892-932`), the state-artifact read
  (`:1154-1160`), and the `loop_config: None` lines in its own constructors and tests (`:410, :1287, :1322, :1352,
  :1400, :1454, :1518`);
- the DTO (`goose-sdk-types/src/custom_requests/schedule.rs:10-16, :33, :58`);
- the ACP mapping (`G/acp/server/schedule.rs:5, :16, :113-131, :145, :227`);
- the agent's schedule tool (`G/agents/schedule_tool.rs:164`), REST (`crates/goose-server/src/routes/schedule.rs:148`),
  CLI (`crates/goose-cli/src/commands/schedule.rs:94`) and the fixture (`crates/goose/tests/acp_fixtures/mod.rs:105`):
  each writes `loop_config: None` and breaks when the field goes;
- the GENERATED contract: `crates/goose/acp-schema.json` (`:4505-4508`, `:4524`, `:4649-4652`), `acp-meta.json`, and
  `ui/sdk/src/generated/{types.gen.ts:1777,1780,1849, zod.gen.ts:1854,1884,1938, index.ts}`, regenerated with
  `just generate-acp-types` (never hand-edited; `just check-acp-schema` refuses a stale copy);
- `ui/desktop/openapi.json:6250-6253` (goose-server's OpenAPI, which lists `ScheduledJob`, `openapi.rs:585`),
  regenerated with `just generate-openapi`;
- **Settings › Import** (`S/components/settings/import/GooseImportSection.tsx`): its loop half writes `loop_config`
  through `acpCreateSchedule` (`:161-170`). After L8 it still scans `schedule.json`, and for each job that carries a
  `loop_config` it shows the named row "Loops are no longer imported — {id} ({cron}). Recipes still import." with no
  import button; an unparseable `schedule.json` is named "schedule.json could not be read: {error}" instead of the
  silent `catch` (D11). Its recipe half is untouched.

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

**While a turn runs in the loop's chat, `/loop` never waits in the queue.** The composer queues anything typed while a
turn runs, and a slash command never steers (§2.8), so a typed `/loop stop` would otherwise wait for the very tick it
means to stop. The composer (L4) therefore recognises the CONTROL forms (`/loop`, `/loop now|pause|resume|stop`) while
a turn runs and sends them as `loops/get` / `loops/control` directly, showing the same reply line; a `/loop <goal>` or
`/loop every …` typed during a turn is queued like any message (starting or replacing a loop mid-turn is not urgent).
The grammar is ONE parser, `parse_loop_command`, in Rust (L3's `/loop` handler) and TS (`model.ts`), both pinned by
the L0 fixture, so the two paths cannot read a line differently.

### 7.3 Promote a message: "Loop this"

User messages already carry hover actions ("Edit", "Copy", walk `00-…png`; `S/components/UserMessage.tsx`). L5 adds
**"Loop this"**, which opens the Start dialog (§8.2) with that message's text as the goal. It does not appear on tick
markers.

### 7.4 Templates

The template text lives once, in `G/session_loops/templates.rs` (L0), and is served by `loops/templates`.

**Templates are parameterised by THIS loop's facts, not fixed prose** (the review's gate-2 risk). A step names the
loop's own slots, and the prompt builder fills them from the record at every tick, so an edit to the check or the
state file reaches the next tick without re-typing the steps:

| Slot | Filled from | When the fact is absent |
|---|---|---|
| `{state_file}` | the record's state file | never absent (required at start) |
| `{check}` | the record's check command | the step renders "no check command is set; run the command that shows the change works and quote it" |
| `{goal_first_line}` | the goal | never absent (required at start) |
| `{last_next_step}` | the newest `report.next_step` from the last FINISHED tick on (a yielded tick that never reported does not erase it, Q-278) | the step renders "this is the first tick" (tick 1) or "tick {n} named no next step" |
| `{working_dir}` | the session's working dir | never absent |

The dialog shows the steps RENDERED with the current facts (slots highlighted as chips the user can see are facts)
and editable; the record stores the text as the user left it, slots included. A slot name the builder does not know
is left literally and the dialog flags it ("{foo} is not a fact goose knows"). What reaches a model is therefore
either the user's own words or a sentence built from the loop's facts; the fixed connectives that remain ("rank what
you found", "only that change") are the user-approved method, shown before start and editable, not engine task text
that a user never saw (gate 2's target: engine-dispatched descriptions such as "Integrate every module and VERIFY").

| Template | Name | Steps (the text a tick receives under "What each tick does") | Suggested check | Default cadence |
|---|---|---|---|---|
| `quality` | Software quality loop | 1. Discover: open `{state_file}`, then run or read what your goal points at in `{working_dir}`. List what is broken, missing or confusing, each with the evidence you saw (command output, file:line). 2. Critique: rank what you found by how much it blocks the goal; pick the ONE item that matters most (the last tick named: {last_next_step}). 3. Fix: make that change, and only that change. 4. Prove: run `{check}` and quote its result. A fix without a quoted result is not done. 5. Rewrite `{state_file}`: what is now true, what is next, what you found but did not fix. | the project's test command, from the dialog | every 10m |
| `until_check` | Until a check passes | 1. Run `{check}` and read why it fails. 2. Fix the first cause it names. 3. Run `{check}` again and quote the result. 4. Rewrite `{state_file}`. | required | back to back |
| `watch` | Watch and act | 1. Look at what your goal watches (a build, a deploy, a folder, a URL) and compare it with `{state_file}`. 2. If nothing changed, say so in one line and report progress. 3. If something changed, do what the goal asks and quote the evidence. 4. Rewrite `{state_file}`. | optional | every 30m |
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
- **Placeholder while a tick runs in this chat:** "Tick {n} is running — what you send waits for it. Use Send now to
  steer it." The queue row for a message typed during a tick reads "Queued · Send now steers tick {n}" (the existing
  `MessageQueue` row and its Send now button, unchanged in behaviour; only the label names the tick). The composer's
  Stop button stops the tick (the loop pauses, §4.6). `/loop` controls typed now go straight through (§7.2).
- **Placeholder while a tick finishes on a connection this window lost** (§5.1 reload, outcome a): "Tick {n} is
  finishing in the background — you can send when it ends".
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
│ Your Mac may sleep; ticks wait until it wakes. [◯ Keep this Mac awake while goose is open]  (after Q-230)
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
  mirrors the existing wakelock setting (`set-wakelock`) and is rendered ONLY once Q-230's fix has landed (the setting
  keeps nothing awake before it, `main.ts:2823-2848`; a toggle that does nothing is the dead end Q-230 files). L4
  depends on Q-230 for the toggle; the sleep sentence ships regardless.
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
| waiting_turn (a user turn) | "Tick {n+1} is due — it starts when your turn in "{chat}" ends" |
| waiting_turn (this chat busy or queued) | "Tick {n+1} is due — it starts after your message here" |
| waiting_turn (reviewers) | "Tick {n+1} is due — it starts when goose's check of tick {n} ends" |
| waiting_turn (way held, L2c) | "Tick {n+1} is due — {node} is answering you in "{chat}"; the tick loads {target} after" |
| waiting_you | "Tick {n} didn't say when to come back." + [Run next tick] [Pause] |
| needs_you (open) | "Tick {n} asked you: "{question}"" + [Go to the question] (scrolls to the NeedsYou card, `BaseChat.tsx:677`) |
| needs_you (answer running) | "Your answer to tick {n} is running — the next tick starts after it" |
| paused (you) | "Paused by you after tick {n}." + [Resume] |
| paused (you stopped a tick) | "You stopped tick {n}." + [Resume] [Stop loop] |
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
- **Outcome chips:** "Progress", "Done", "Goal met", "Blocked", "Asked you", "Failed", "No report", "Stalled",
  "Yielded", "Stopped by you".
- **Special rows:**
  - A yielded tick: "Yielded to your turn in "{chat}"". Its partial changes are shown.
  - A failed tick: "Failed: {error}" (verbatim, first line).
  - A no-report tick: "Ended without a loop report" plus the last assistant line, quoted.
  - An asked tick: "Asked you: "{question}"" and, once answered, "You answered: "{answer}"".
  - A tick whose marker an edit removed: "This tick's messages were removed by an edit".
- **Quiet ticks** (progress, `wrote` empty, check exit unchanged) collapse to one line (the Codex "no results"
  pattern): "{n} {HH:MM} · no write or edit outside the state file · {next_step}". The wording is exact on purpose:
  a shell command that changed files leaves no diff (`server.rs:2205-2233` forwards diffs for `write`/`edit` only), so
  "nothing changed" would be a claim the data cannot back.
- **The files list** is titled "Written or edited by goose" (the same scope as the Changes tab).
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
  Its third part is what started the tick: the cadence label for a cadence-started tick, else the origin —
  "after your turn", "after your answer", "after your Mac woke", "after you resumed", "run by you" (Q-279; the
  tick after a yield once read "back to back").
  "Show prompt" expands the exact text sent, the §4.4 transparency.
- **Yielded tick:** the divider gets a second line: "Stopped at {HH:MM} for your message in "{chat}" — the loop
  continues after your turn." `{chat}` is the chat's name when shown (read, then kept by rename events), not the
  name frozen at the yield — a new chat is "New Chat" until its first turn names it (Q-279).
- **Steers** inside a tick render as today (user bubble, `metadata.steer`).
- Hover actions on a marker: "Copy prompt" only. There is no Edit or Loop this.

### 8.6 The sidebar

- **Rows.** Session rows in PROJECTS and the session list get a **Looping** pill: solid accent, ⟳ icon, text
  "Looping" or "Next {HH:MM}", and "Paused" in the stopped fill. This comes through the existing `SessionActivityMarker`.
- **Precedence**, as `sessionStates` (`sessionActivityStore.ts:229-236`) already orders it, with `looping` inserted
  once: **needs-you > running > background > looping > failed > stopped > idle**. The existing function pushes
  needs-you first, then running, else background, and considers failed and stopped only when nothing else holds;
  `looping` is pushed after that block and before failed/stopped, only when no running or background state holds.
  So a tick in flight shows Running, a tick that asked shows NeedsYou (the item is a User session's, `G/needs_you.rs:80-90`),
  the tick's reviewers show Background ("Checking"), a loop between ticks (waiting, waiting_turn, checking, paused,
  waiting_you, elsewhere) shows Looping, and a loop's single failed tick does not show Failed while the loop still
  runs (the failure is on the tick's row; two in a row pause the loop, which shows Looping "Paused"). A yielded tick
  never shows Stopped (§5.2 step 5).
- **Active now.** It lists a loop session only while a tick runs or needs you, which is the existing rule
  (`sessionActivityStore.ts:240-242`). A waiting loop is not "active": it is a scheduled future, not work in flight.
- **Top bar.** The top bar's "N running" counts ticks as running, which they are.

---

## 9. Slices

Every slice runs the full gate from `goose-feature-dev`: fmt, clippy `-D warnings`, cargo test for the crate, tsc,
eslint, `i18n:check` and vitest. `en.json` is regenerated with `pnpm i18n:extract` after each merge and never
hand-merged.

**Rules across slices:**
- **Every file has exactly one owning slice at a time.** A file handed from one slice to a later one is named
  "handed from X" in the later slice's Owns, and the later slice starts editing it only after X merges. That covers
  files owned by the NODES design: `acp/server.rs`'s `on_prompt` is S5's until S5 merges; `nodes_loader.rs` is S5's;
  `main.ts` is S7's; `App.tsx` is S11's (handed from S1, which merged).
- **Generated files are owned too**, by the one slice whose contract change regenerates them, and handed on:
  `crates/goose/acp-schema.json`, `crates/goose/acp-meta.json` and `ui/sdk/src/generated/*` are L0's (the whole loops
  contract, every DTO and both notifications, is closed in L0, the nodes design's S0 rule), then handed to L8, which
  regenerates them once more when `LoopConfigDto` goes. They are always regenerated (`just generate-acp-types`), never
  hand-edited; `just check-acp-schema` refuses a stale copy.
- No slice edits `swarm.rs`, `crates/goose-swarm/*` or anything under `crates/goose-cli/src/commands/swarm/` except
  L6's two files (`agent_work/window.rs`, `agent_work/manifest.rs`).
- `custom_dispatch.rs` and `custom_requests.rs` were closed by the nodes design's S0, which has merged (`7025a2b11`);
  L0 adds its own lines and new files and never edits the nodes lines.
- At most three surgeons at once (memory `be-mindful-of-usage`).

### 9.0 The slice table

| Slice | Owns | Depends on | Confidence |
|---|---|---|---|
| L1 | **LANDED `fdcac1d1a`** (awaiting live prove). It edited `App.tsx` (the `/loop` route; S11's file) and `main.ts` (the fleet-chat line; S7's file) before this revision named those owners; both edits are removals and nothing of lane L remains in either. The `powerMonitor` resume broadcast it listed was NOT cut and moves to L10. `ChatInput.tsx` is handed to L4 | — | high (landed) |
| L6 | one clock: `G/loop_clock.rs` (new), `G/lib.rs` (its `pub mod` line; then handed to L0), `crates/goose/Cargo.toml` + `Cargo.lock` (`chrono-tz` via `cargo add`), `AW/window.rs` (becomes a re-export), `AW/manifest.rs` (`WorkWindow`, its `Default` and its serde defaults `default_days`/`default_from`/`default_to` move out; the manifest imports `WorkWindow`) | — | high |
| L0 | contract, pure rules, generated SDK: `G/session_loops/{mod.rs, record.rs, rules.rs, prompt.rs, templates.rs, acp.rs, seam.rs, loops.fixture.json}`, `G/lib.rs` (handed from L6), `goose-sdk-types/src/custom_requests/loops.rs` + its lines in `custom_requests.rs`, `goose-sdk-types/src/custom_requests/needs_you.rs` (the `looping` field of `SessionActivityResponse`), `goose-sdk-types/src/custom_notifications.rs` (`loops/tickDue`, `loops/changed`), `G/acp/server/custom_dispatch.rs` (the `dispatch_loops_*` fns), `crates/goose/acp-schema.json` + `acp-meta.json` + `ui/sdk/src/generated/*` (regenerated; then handed to L8), `S/acp/loops.ts`, `S/components/loops/model.ts` + tests | L6 | high |
| L2a | runner core, in-process: `G/session_loops/runner.rs`, `G/session_loops/check.rs`, `G/session_loops/owner.rs`, `G/turn_priority.rs` (two fns factored from `after_user_turns`) | L0 | medium |
| L3 | report tool, `/loop`, the extension's life, fork strip: `G/agents/platform_extensions/loop_report.rs` (new), `G/agents/platform_extensions/mod.rs` (its registration lines), `G/agents/execute_commands.rs` (`/loop`, `parse_loop_command`), `G/session_loops/agent_sync.rs` (new), `G/session/session_manager.rs` (`copy_session` strips `loop.v0` and the loop extension from the copy) | L0 | medium |
| L4r | the renderer door: `S/acp/prompt.ts` (`_meta`), `S/acp/chatSessionController.ts` (status return, `meta`, `preAppend`), `S/acp/acpConnection.ts` (the typed loops callbacks), `S/components/loops/LoopDriver.tsx` (new; then handed to L10), `S/components/loops/pendingUserInput.ts` (new: per-session queued-message count), `S/components/Layout/AppLayout.tsx` (mount) + tests | L0 (generated types); live proof L2b | medium |
| L7 | sidebar + activity: `G/acp/server/needs_you.rs` (`looping` from `session_loops::acp::list`; the runner's resolve hook through `seam.rs`), `S/components/sessionActivity/sessionActivityStore.ts`, `S/components/sessionActivity/ActivityPills.tsx` + tests | L0 | medium |
| L8 | retire `LoopConfig`: `G/scheduler.rs`, `goose-sdk-types/src/custom_requests/schedule.rs`, `G/acp/server/schedule.rs`, `G/agents/schedule_tool.rs`, `crates/goose-server/src/routes/schedule.rs`, `crates/goose-cli/src/commands/schedule.rs`, `crates/goose/tests/acp_fixtures/mod.rs`, `S/components/settings/import/GooseImportSection.tsx`, `ui/desktop/openapi.json` (regenerated), `crates/goose/acp-schema.json` + `acp-meta.json` + `ui/sdk/src/generated/*` (handed from L0, regenerated) | code: — (cut now); merge: L0 merged (the generated files) | medium-high |
| L5 | the rail + transcript: `S/components/session-rail/SessionRail.tsx` (new), `S/components/loops/{LoopPanel.tsx, TickRow.tsx, LoopPill.tsx}`, `S/components/changes/ChangesRail.tsx` (split into pill + `ChangesPanelBody`), `S/components/BaseChat.tsx` (the mount at `:666-673`), `S/components/UserMessage.tsx` (tick divider + "Loop this"), `S/acp/adapter/messages.ts` (map `looptick_` ids to `metadata.loopTick`) + tests | L0 | medium-high |
| L4 | start UI: `S/components/ChatInput.tsx` (handed from L1), `S/components/MessageQueue.tsx` (the queued row's tick label), `S/components/loops/StartLoopDialog.tsx` + tests | L0, L4r (`pendingUserInput.ts`); the Keep-awake toggle: Q-230 merged | medium-high |
| L2b | the server door: `G/acp/server.rs` (handed from **S5**: the `on_prompt` tick branches of §5.2, the cause cell in `ActivePromptRun`, the door guard in `connect_to`, and the reload drop guard if §5.1's measurement says (b)), `G/acp/server/dispatch.rs` (register the door) | **S5 merged**, L2a, L3, L0 | **low-medium** |
| L2c | the Mac-wide half: `G/session_loops/mac_wide.rs` (new: the same-way yield trigger and the cross-process user-reply wait, over S5's holders), `G/acp/server/nodes_loader.rs` + `nodes_loader/holds.rs` (handed from **S5**: the `kind: tick` demand branch of §5.5) | **S5 merged** (with `kind` in `holders.rs`), L2b | medium-low |
| L10 | the wake broadcast: `ui/desktop/src/main.ts` (handed from **S7**: `powerMonitor.on('resume')` → `system-resumed` to every window), `ui/desktop/src/preload.ts` (`onSystemResumed`), `S/components/loops/LoopDriver.tsx` (handed from L4r: subscribe → `loops/wake`) | **S7 merged**, Q-230 merged (also `main.ts`), L4r | high |
| L9 | harness + live: `local-edition/mlx/quality/harness/loops-state.mjs`, `…/harness/loops-walk.mjs`, `local-edition/mlx/quality/briefs/loop-5-ticks.md`, `S/components/loops/__states__/states.ts`, `S/components/loops/LoopsHarness.tsx` (new), `S/App.tsx` (handed from **S11**: the `#/harness/loops` route, under `GOOSE_UI_HARNESS=1` only) | L2b, L2c, L4, L5, L7, L10; **S11 merged** | medium |

**Order by data dependency, and what starts now:**
- **Now:** L6 (small, pure move), and L8's CODE in a worktree (it depends on nothing of lane L; only its regeneration
  of the generated files and its merge wait for L0).
- **After L6:** L0.
- **After L0:** L2a, L3, L4r, L7, L5 and L8's merge (three surgeons at a time; L2a and L4r first, because L2b and L4
  wait on them).
- **After L4r:** L4 (its Keep-awake toggle waits on Q-230; the rest does not).
- **After S5 merges, and L2a + L3:** L2b. **After L2b:** L2c.
- **After S7 and Q-230 merge, and L4r:** L10.
- **Last:** L9, after S11 merges.

L4, L5 and L7 can be cut against L0's contract before L2b lands; only their live proof needs it.

### L1: Remove Recipes & loops (Q-227) — LANDED

`fdcac1d1a` removed the button, the dialog, the recipe wizards, `LoopView`/`LoopModal`, the `/loop` route and the
fleet-chat IPC, with every dependent checked by grep (its commit message). It did NOT cut the resume broadcast: that
is L10, after S7 hands `main.ts` over. Live proof is pending (ledger Q-227, "awaiting live prove").

### L6: One clock

**Confidence: HIGH.** A pure move with the existing tests moving along.

**Changes:**
- Move `parse_cadence`, `parse_hm`, `parse_day` and `DeskClock` from `AW/window.rs`, and `WorkWindow` with its
  `Default` impl and its three serde default fns (`default_days`, `default_from`, `default_to`, `manifest.rs:71-92,
  :152-163`) from `AW/manifest.rs`, into `G/loop_clock.rs` unchanged. `window.rs` imports `WorkWindow` from the
  manifest today (`window.rs:8`), so moving the clock without the struct and its defaults does not compile.
- `AW/window.rs` becomes `pub use goose::loop_clock::*;`. `manifest.rs` imports `WorkWindow` from there and keeps its
  own defaults (`default_timezone`, `default_cadence`, `default_ledger`, …); its `window: WorkWindow` field keeps
  `#[serde(default)]`, so existing `agent.yaml` files parse identically.
- Add `chrono-tz` to `crates/goose` with `cargo add`, at the same version as goose-cli's (`0.10`,
  `crates/goose-cli/Cargo.toml:73`).

**Tests:** `window.rs`'s tests move and pass byte-identically; a manifest test parses an `agent.yaml` with no
`window:` block and with a partial one (the moved serde defaults); `cargo test -p goose-cli` shows Agent Work's tests
unchanged; `cargo test -p goose-swarm --test development_gates` passes with no baseline change (the move adds no
numeric const; `next_open`'s `0..15` search range is a loop literal that moves unchanged).

**Must not break:** Agent Work's next-tick reasons and window behaviour. `AW/mod.rs:167-353` is untouched.

### L0: Contract, pure rules and the generated SDK

**Confidence: HIGH.** Types, a store over `extension_data`, pure rules, the prompt builder and templates, with a
shared TS/Rust fixture; the regenerated SDK. The risk is fixture drift, which the shared fixture refuses.

**Contract** (closed here: every loops DTO and notification for the whole design; bodies call the runner and the
extension sync through `seam.rs`, and before L2a/L3 land they answer the named refusal "The loop runner is not in this
build"):

| Method | Request | Response |
|---|---|---|
| `_goose/unstable/loops/get` | `{sessionId}` | `{loop: LoopRecord?, effectiveStatus, error?: string}`. A PURE read (§5.1): never claims, never writes. `effectiveStatus` is the derived `paused{closed}` when the owner is proven gone. An unreadable record answers `error`, never `loop: null`. |
| `_goose/unstable/loops/start` | `{sessionId, goal, template, steps, cadence, stateFile, check?, stopAfterTicks?}` | `{loop}` or a refusal `{reason}` (swarm-build chat, empty goal, bad cadence, state file outside the working dir, an unknown slot). Claims the owner. |
| `_goose/unstable/loops/update` | `{sessionId, patch}` (edit) | `{loop}` |
| `_goose/unstable/loops/control` | `{sessionId, action: pause \| resume \| stop \| tickNow \| stopCheck}` | `{loop}`. `resume` and `tickNow` claim the owner. |
| `_goose/unstable/loops/tickRefused` | `{sessionId, loopId, n, reason: turn_running \| queued_message \| pending_cancel \| load_failed{error} \| submit_failed{error}}` | `{}` |
| `_goose/unstable/loops/ready` | `{sessionId}` | `{reoffered: bool}` |
| `_goose/unstable/loops/wake` | `{}` | `{rearmed: number}` |
| `_goose/unstable/loops/templates` | `{}` | `{templates: [{id, name, description, steps, slots, suggestedCadence, needsCheck}]}` |
| `_goose/unstable/loops/list` | `{}` | `{loops: [{sessionId, status, nextTickAt?}]}` (for L7), a pure read |
| `session_activity/get` (existing) | — | gains `looping: [{sessionId, status, nextTickAt?}]` |
| notification `_goose/unstable/loops/tickDue` | — | `{sessionId, loopId, n, messageId, prompt}` |
| notification `_goose/unstable/loops/changed` | — | `{sessionId, loop}` (the rail and pills update on events, not polls) |

**Pure rules** (`rules.rs` / `model.ts`, pinned by `loops.fixture.json`):
- `next_tick(record, now)` over `loop_clock::DeskClock`;
- `decide_after_tick(record, tick, check, open_items)`, which returns the §4.6 outcome, status and reason, including
  `asked`, `yielded` and `stopped_by_you`;
- `stalled(prev, cur)` over `wrote` (state file excluded);
- `wrote(messages[range], state_file)` (the Rust half of `sessionChanges`: `write`/`edit` diffs only);
- `status_sentence(record)`, which returns every string key and its facts;
- `parse_tick_id` / `tick_id`; `parse_loop_command` (the §7.2 grammar, shared by the server and the composer);
- `tick_ranges(messages, ticks)`, which returns `[first, next)` per tick, and names a range whose marker is gone;
- `render_steps(steps, facts)` (the §7.4 slots, with each absence sentence);
- `output_tail(output, window, count_tokens)` (§4.4: the longest suffix at most `window / 64` tokens).

**Tests:**
- serde round-trip of every fixture case;
- each §4.6 row as a fixture case, run by BOTH suites;
- `next_tick` for every / self-paced (valid, missing, invalid) / back-to-back × first / overdue / reviewers pending;
- `tick_ranges` with steers, user turns and an answer turn between ticks, and with a marker removed;
- `stalled` with a tick that wrote only the state file (stalled) and one that wrote another file (not);
- `render_steps` for every slot present and absent;
- `output_tail` with an output far larger than the window, one smaller than the budget, and an empty one;
- the prompt builder for each template with and without a previous tick, check, yield or asked item, with the
  snapshot reviewed once by a reader for gate-2 specificity;
- the seam refusal before L2a;
- `just check-acp-schema` clean after the regeneration.

**Must not break:** nothing is written into a session's `extension_data` until `loops/start`; `loops/get` and
`loops/list` are pure reads; the existing `session_activity/get` consumers (the new field is additive).

### L2a: The runner core (in-process)

**Confidence: MEDIUM.** Everything in it is testable with a fake door, a fake clock and two processes over one
SQLite, and none of it touches `on_prompt`. Where a bug can hide:
1. **The owner claim across goosed processes over one SQLite.** It is a compare-and-set inside one
   `update_extension_state` transaction (`BEGIN IMMEDIATE`), which SQLite serialises; it is proven by a two-process
   test, not assumed.
2. **Proof of gone by reparenting** (§5.1 condition 3) reads another process's parent pid; `machine.rs` already reads
   pid liveness and start time, and the ppid read is the same `proc_pidinfo`/`sysctl` family, measured in the test.
3. **The offer protocol**: an offer must be neither lost nor accepted twice across a door drop, a refusal and a
   re-offer.

**Changes:**
- `runner.rs`: arm, claim, release on last door drop, wait (wall-clock `next_tick_at`, re-evaluated on the events of
  §5.1), offer / refuse / re-offer, `accept_offer`, receive `tick_started` / `tick_ended` / `tick_reviewed` /
  `user_turn_ended`, read the needs-you store and `wrote`, apply `decide_after_tick`, emit `loops/changed`. The door is a
  trait (`send(TickDue)`), so L2b supplies the connection and tests supply a fake. It never touches an `Agent`.
- `check.rs`: spawn through `configure_subprocess` in `working_dir`, read concurrently into
  `<data_dir>/loops/{loopId}/check-{n}.log`, end at `child.wait()` (never at EOF), drain without blocking, compute the
  tail, and stop through `goose_sidecar::sigkill_owned_group` (pid alone, logged, when the proof fails).
- `owner.rs`: `{goosed_pid, goosed_started_at, app_pid}` and `proven_gone`.
- `turn_priority.rs`: `wait_no_user_turn()` and `user_turn_started_since(started)`, both factored from
  `after_user_turns` (`:58-76`), whose own behaviour is unchanged.

**Tests:**
- runner unit tests with a fake door and a fake clock, **fed as values**: the runner takes `now` from a trait; no
  seconds constant is added;
- no offer while a user turn is held; an offer refused `turn_running` stays open and is re-sent on `ready`, never on a
  timer; an offer re-sent on a new door is accepted once;
- a user turn started mid-tick (v1a) → cancel through the ticket with cause `yield` → `yielded` → re-offered after;
- a tick that left an open needs-you item → `needs_you`, no offer until the item is resolved AND the next user turn
  in that session ended; dismissed → offered at once;
- the next tick waits for `tick_reviewed`;
- the last door dropped → every owned loop `paused{closed}`, `owner = None`;
- a recorded owner whose pid is dead, reused, or reparented → proven gone; `loops/get` derives `paused{closed}` and
  writes nothing;
- two runners racing the claim (two processes) → exactly one owner;
- a check that fails to spawn → paused with the reason; a check stopped → paused; a check whose grandchild holds the
  pipe → the check still ends at its exit;
- self-paced with no `next_in` → `waiting_you`;
- `after_user_turns`' existing tests unchanged.

**Must not break:** Q-132's reviewer yield (`after_user_turns` unchanged).

### L3: The report tool, `/loop`, the extension's life, the fork strip

**Confidence: MEDIUM.** The code is small. The risk is whether the local models call `loop_report` reliably at the
end of a tick. Ending the turn on the call removes the "kept generating after its final tool" failure (r6f); the
"never called it" failure (r4b) is measured in L9's J1 (it counts `no_report` ticks) and the tool description is
iterated on the words, gate 7's read-the-words practice.

**Changes:**
- The `loop` platform extension (`default_enabled: false`, `hidden: true`) exposes one tool, `loop_report` (§4.4). It
  decides "a tick is running" from the record (the last `TickRecord` has `started_at` and no `ended_at`), so it needs
  nothing from the runner; it writes the report through `update_extension_state`; its result carries
  `END_TURN_META_KEY`; a validation failure is refused WITHOUT the end-turn key.
- `agent_sync.rs`: `sync_loop_extension(agent, session_id, record)` (§4.4), idempotent, called by L0's `start` /
  `control` handler bodies through `seam.rs` and by `on_prompt` (L2b).
- `/loop` goes in `COMMANDS` with the §7.2 grammar through `parse_loop_command`. `command_starts_turn` returns false.
- `copy_session` (`session_manager.rs:2167-2200`): the copy's `extension_data` drops `loop.v0`, and the `loop` entry of the
  persisted `EnabledExtensionsState` (which lives in the same `extension_data`, `agent.rs:1404-1408`), so a fork starts
  with no loop and no tool.

**Tests:**
- the schema validates each verdict; an invalid call is refused and does not end the turn;
- a valid call ends the turn (the agent's reply stream ends after that batch, the `ask_user` test's shape);
- a call outside a tick is refused with its string; a second call in the batch replaces the first;
- `/loop` parse table (each row of §7.2), including "stop" never read as a goal, shared with the TS suite;
- the extension is absent from a chat with no loop, present for a loop's life, absent after it ends and the next
  prompt syncs;
- a fork of a loop chat has no `loop.v0` and no loop extension; the original keeps both.

**Must not break:** `/goal`, `/grind`, `/compact`, recipe slash commands, the slash popover list (the builtin is
tagged Builtin, `G/acp/response_builder.rs:354`), and every other key `copy_session` copies.

### L4r: The renderer door

**Confidence: MEDIUM.** Three shared files change behaviour-neutrally for their existing callers, which is the part to
prove; the new paths are new files.

**Changes:**
- `prompt.ts`: `acpPromptSession(sessionId, message, meta?)` passes `_meta` on the `PromptRequest`.
- `chatSessionController.ts`: `submitMessage` returns `'submitted' | 'busy'` (`'busy'` exactly where it returned
  silently, `:157-158`; `assertNoPendingPromptCancellation`, `:79-84`, keeps THROWING for existing callers, and the
  driver checks `pendingCancelPromptAttemptId` itself before calling, answering `tickRefused{pending_cancel}`), and
  takes `meta` and `preAppend` options; with `preAppend`, the message is appended to the session's messages after the busy check and before the
  prompt is sent. Existing callers pass neither and ignore the result.
- `acpConnection.ts`: defines the generated typed callbacks for `loops/tickDue` and `loops/changed` and routes them to
  a small in-module emitter the driver and the rail subscribe to.
- `LoopDriver.tsx`: §5.1's three steps; when the submit itself fails (the server refused the prompt, e.g. "session is
  busy in another run"), it removes the pre-appended marker by its id and answers `tickRefused{submit_failed{error}}`
  with the server's words; answers `tickRefused` with the reason; watches the store's
  `activePromptAttemptId` and `pendingUserInput` for the refusing session and sends `loops/ready` when both clear.
- `pendingUserInput.ts`: a per-session count the composer writes (L4) and the driver reads.
- `AppLayout.tsx`: mounts `LoopDriver` once.

**Tests:** existing `chatSessionController` tests pass unchanged; `'busy'` has no side effect; `preAppend` appends once,
after the busy check; `_meta` reaches the request; a `tickDue` for a chat not in the store loads it then submits; a
busy store → `tickRefused{turn_running}`; a non-empty queue → `tickRefused{queued_message}`; the attempt clearing →
exactly one `loops/ready`; a repeated `tickDue` for the same `(loopId, n, messageId)` submits once.

**Must not break:** every existing `submitMessage` caller (§2.8's list), the Q-169 stopped path, the credits-exhausted
path, and the `MESSAGE_STREAM_FINISHED` event.

### L7: Sidebar and activity

**Confidence: MEDIUM.** It adds a state to a precedence order that three surfaces read.

**Changes:**
- `on_session_activity` fills `looping` from `session_loops::acp::list`.
- The needs-you resolve handler notifies the runner through `seam.rs` (an event for §4.6's asked rule).
- The store gains `'looping'` with §8.6's precedence, and `ActivityPills` gains `LoopingPill`.

**Tests:** the full order (needs-you > running > background > looping > failed > stopped > idle) as a table, including
a looping session whose last tick failed (Looping, not Failed) and a yielded tick (never Stopped); Active now
unchanged by a waiting loop; the pill text for waiting, paused and waiting-for-you.

**Must not break:** Q-185's background state and the running/needs-you rows.

### L8: Retire `LoopConfig`

**Confidence: MEDIUM-HIGH.** Every dependent is enumerated by one `rg` (§6), including the generated contract, the
goose-server OpenAPI and the import section the first draft missed.

**Changes:** §6's list. The import section's loop half becomes the named "Loops are no longer imported" rows; its
malformed-file `catch` becomes a named error (D11).

**Tests:** scheduler tests for single-run jobs unchanged; a `schedule.json` fixture carrying `loop_config` loads and
runs once (the migration rule, §6); the import section with a `schedule.json` holding one loop shows the named row and
imports nothing; an unparseable file shows its error; `just check-acp-schema` clean; the OpenAPI regenerated.

**Must not break:** SchedulesView, the agent's schedule tool, the CLI, REST, and the import section's recipe half.

### L5: The rail and the transcript

**Confidence: MEDIUM-HIGH.** `sessionChanges` is reused unchanged over slices. The risk is the ChangesRail split,
guarded by its existing 5 + 5 + 4 tests (§2.6).

**Changes:**
- `SessionRail` renders the two pills and one panel with `Segmented as="tabs"`. The panel keeps today's size, overlay
  and Escape/focus behaviour.
- `ChangesRail` becomes `ChangesPill` + `ChangesPanelBody`, with no behaviour change.
- `LoopPanel`, `TickRow` and `LoopPill` implement §8.4, including the asked, stopped-by-you, reviewers-pending and
  removed-by-edit rows, and the "no write or edit outside the state file" quiet line.
- `UserMessage` renders the tick divider and adds "Loop this".
- The adapter maps `looptick_` ids.

**Tests:** ChangesRail's existing tests pass on the split; one test per §8.4 state (from L0's fixture); `TickRow` files
equal `sessionChanges(slice)`; a quiet tick collapses with the exact sentence; the divider replaces the bubble; "Loop
this" prefills the goal; per-session open state survives a remount and a throwing `localStorage`.

**Must not break:** Q-190 (the chat never changes width; a layout test measures the conversation column with the panel
open and closed); the Changes pill's test id and strings.

### L4: Start UI

**Confidence: MEDIUM-HIGH.**

**Changes:**
- The Loop button and status chip in the slot L1 emptied.
- The placeholders of §8.1; the queue row's "Queued · Send now steers tick {n}" label (`MessageQueue.tsx`); the
  composer writes its queued count to `pendingUserInput`.
- `/loop` control forms typed while a turn runs go to `loops/get` / `loops/control` directly (§7.2), through
  `parse_loop_command`.
- `StartLoopDialog.tsx`: every §8.2 state and string; steps rendered with their slots (§7.4); the Keep-awake toggle
  rendered only once Q-230 has landed.

**Tests:** the dialog's validation states; the refusal on a `swarm-build` chat; presets map to grammar strings; an
unknown slot is flagged; the chip's labels per status; `/loop stop` typed during a tick calls `loops/control{stop}` and
is not queued, while `/loop fix the tests` typed during a tick is queued; the queued count reaches `pendingUserInput`.

**Must not break:** the composer's other controls, the queue's Send now / interruption behaviour, and the narrow rule
(`ChatInput.tsx:367`).

### L2b: The server door

**Confidence: LOW-MEDIUM.** Stated plainly, because this is where a subtle bug can hide:
1. **It edits `on_prompt` on top of S5's reply guard**, the most shared function in goosed. Every exit of `on_prompt`
   must send exactly one `tick_ended` for a tick and none for a user prompt.
2. **The reload behaviour of the ACP stack is unmeasured** (§5.1): whether a closed websocket drops the in-flight
   `on_prompt` future. L2b measures it FIRST and cuts branch (a) or (b) from the result.
3. **Skipping `record_stopped` on a yield** must not leave the session's `turn_outcome` stale (a previous turn's
   `stopped` would otherwise still show): the yield clears it the way a new turn does.

**Changes:** §5.2's eight steps; `ActivePromptRun` gains the cause cell; `connect_to` holds the door guard; `dispatch.rs`
registers the door beside `client_cx.set`; and, only if measured (b), the synchronous drop guard on the run
registration.

**Tests:**
- every non-tick prompt: the same `user_turn()`, the same errors, the same notifications (existing ACP tests), plus a
  prompt with a forged or stale `loopTick` meta takes `user_turn()`, keeps a normal id, and is not recorded as a tick;
- an accepted tick: no `user_turn()`, the message carries the offer's id, the holder kind is `tick`;
- a yield: no "You stopped this answer" notice, no `stopped` outcome, the tick `yielded`;
- a user Stop on a tick: the notice IS recorded, the tick `stopped_by_you`, the loop paused;
- `tick_reviewed` fires after both reviewer tasks, and the response is not delayed by them;
- a closed connection deregisters its door; a reload re-offers on the new door;
- the reload measurement recorded in the commit message, with its outcome.

**Must not break:** `on_prompt` for every non-tick prompt; S5's guard and the served-turn record; the end-of-turn
reviewer's yield (Q-132); `link_serve`'s tap.

### L2c: The Mac-wide half

**Confidence: MEDIUM-LOW.** It depends on S5's holders and their change observation, which are being built now, and
it adds an ordering rule to S5's FIFO.

**Changes:**
- `mac_wide.rs`: the v1b yield (§5.3: a `kind: user` holder on the tick's way, in any process, yields the tick) and the
  cross-process "no user reply open" wait before an offer, both on S5's holder-change events.
- `nodes_loader.rs` / `holds.rs`: the `kind: tick` demand branch of §5.5 (never swap under an open user holder; never
  queue ahead of a user demand before the claim).

**Tests:** a fixture holder file with a `kind: user` reply on the tick's way → the tick yields; on another way → it
does not; a tick demand queued, then a user demand → the user's goes first; a tick demand that has taken the claim runs
to its end; two loops in two processes do not wait on each other's ticks as user replies.

**Must not break:** every S5 test (a user's demand behaves exactly as before), J3/J4 of DESIGN-NODES.

**As cut (2026-09-28, `db0cd9616`, `b29a38f91`).** Landed:
- `nodes_loader.rs` / `holds.rs`: a person's demand goes before every tick demand whose swap has not begun
  (`Queued.goes_before`), and a person's reply opened after a tick's queued switch is not held behind it
  (`switch_ahead`); a tick swap under way still runs to its end first. A tick demand blocked by a person's reply
  waits with §5.5's sentence verbatim, "{way} is answering you in {chat}; the loop's tick loads {target} when it
  finishes" — `{way}` in the loader's own way words ("this Mac's engine"), `{chat}` the session's name. The reply's
  kind is read in-process from `Holds` and cross-process from the holder record (Q-239: the kind was already
  published since L2b; nothing read it).
- Two early exits of L2b's door that orphaned the window's pre-appended marker: a person's Stop while the tick
  starts, and a yield at the start (a user turn that began between offer and accept). `tick_started` now applies
  that yield itself before it returns (it raced `on_prompt` through a spawned watcher and could reach the model),
  and `on_prompt`'s early cancel stores the marker under the offer's id and settles the cancel through the same
  `record_cancelled_turn` the during-the-reply cancel uses (the notice for a stop; none for a yield).

NOT cut in that pass, cut in the next (2026-09-28, `07b31bd90`..`8a13f0f1d`): `mac_wide.rs`.
- **The yield (v1b).** v1a's "any user turn in this goosed yields" is gone. A running tick yields only to a PERSON's
  reply on the way its own reply leased, in any goose process on this Mac. The reader is the loader's
  (`Holds::persons_on`, the same traversal as `blockers`, kind `User`). A reply opening in another process announces
  nothing, so a tick holding a way re-reads the other records on the loader's `LOOK_AGAIN` observation cadence.
- **The check before an offer.** A `node:<id>` chat on an MLX node waits `WayHeld{way, chat, node}` while a person's
  reply holds any way of this Mac's goose. A tick that yielded waits until THAT reply ends. Each wait ends on an event
  (this process's holds changing; the kernel releasing another process's reply flock). The check runs again at the
  tick's start.
- **Where it stops.** Strategy and Auto chats are not held before the offer, because their node is the router's choice
  at lease time; they yield at the lease instead. `omlx` and endpoint chats never lease through the router, so the
  holders cannot see them.
- **One gap, not cut.** A person's demand that must STOP the tick's way still waits for the tick's reply, because the
  holder record carries no demand target.
- **The early exit.** A person's plain message stopped before its reply is now stored and noticed. A slash command
  stopped there is not stored, because what `agent.reply` stores for one depends on running it.

### L10: The wake broadcast

**Confidence: HIGH.** Four lines of main/preload and one subscription.

**Changes:** `powerMonitor.on('resume', …)` sends `system-resumed` to every window; `preload.ts` exposes
`onSystemResumed(cb)`; `LoopDriver` calls `loops/wake` on it.

**Tests:** a main unit asserts `system-resumed` reaches a mock window; the driver calls `loops/wake` once per event.

**Must not break:** S7's glance lines and Q-230's wakelock lines in `main.ts`.

### L9: The state harness and the live proof

**Confidence: MEDIUM.** §10.

---

## 10. Test plan

### 10.1 Unit

- **Rust:**
  - L0 rules, prompt, slots and tail over the shared fixture;
  - L2a runner, check and owner with a fake door, injected time and two processes over one SQLite;
  - L2b `on_prompt`'s tick branches, the cause cell, the door guard;
  - L2c the tick-demand branch over fixture holder files;
  - L3 tool (including end-turn), command, extension sync and fork strip;
  - L6 the moved clock and the manifest's serde defaults;
  - L8 the scheduler and the migration fixture.
- **TS:**
  - L0 `model.ts` and `parse_loop_command` over the same fixture;
  - L4r the controller's status return, `_meta`, `preAppend`, and the driver's offer/refuse/ready;
  - L4 dialog, composer controls during a turn, queue label;
  - L5 rail, rows and divider;
  - L7 store precedence (the full order);
  - L8 the import section's named loop rows;
  - L10 the resume subscription.
- **Gates:**
  - `cargo test -p goose-swarm --test development_gates`: L6, L2a and L2b keep it green; the one new numeric const
    (§4.4's `1/64`) carries its `// ratio:` marker, so the live-const ratchet does not rise;
  - `just check-acp-schema` after L0 and after L8;
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
  2. each tick's marker, turn and `loop_report` are in the transcript, and each turn ENDS at its `loop_report` call
     (no assistant text after it);
  3. each row's files equal what the tick's `write`/`edit` tool cards changed;
  4. the state file exists after every tick and its content changed (read the file, not only its Changes entry);
  5. the check ran after every tick with its exit code shown and its log openable;
  6. `next_tick_at` minus the previous start equals the cadence, or "overdue", and no tick started before the previous
     tick's reviewers ended (their `background` rows ended first in `session_activity`);
  7. the owner can read the timeline and say what each tick did from the rail alone;
  8. the `loop.v0` record's size after five ticks is written down (§4.2's measurement).
- **Words.** The tail of each tick's reply is read and quoted in `E2E-RUNS.md` (gate 7 practice): did it discover,
  critique, fix and prove, or restate?

**J2: the user's turn wins.**
- During tick 3, send a message in another chat on the same engine. Expect (v1a): the tick is cancelled at that user
  turn's start, recorded `yielded`, with NO "You stopped this answer" line and no Stopped pill; the next tick carries
  the yielded line. After L2c: repeat with the other chat on a different node or a cloud model; expect NO yield.
- During tick 4, type a message in the loop's own chat. Expect: it is QUEUED with "Queued · Send now steers tick 4";
  pressing Send now steers it into the tick (visible as a steer bubble); a second queued message left alone is sent
  after tick 4 ends and BEFORE tick 5 (the offer is refused `queued_message`, then re-offered on `ready`).
- During tick 5, type `/loop pause`. Expect: the loop pauses at once, nothing queued.
- During tick 6, press the composer's Stop. Expect: "You stopped this answer", the tick `stopped_by_you`, the loop
  Paused "You stopped tick 6".

**J3: stop rules.**
- Make the check pass → "Goal met".
- A no-check blank loop reporting done → the done sentence.
- Force a stalled tick (a goal already met with no check) → Stalled.
- Close the window mid-wait → reopen → paused-closed → Resume → one tick.
- Quit the app mid-wait (the real `osascript` quit that Q-223 measured) → relaunch → the chat shows paused-closed,
  never "Looping in another window", whether or not the old goosed survived as an orphan (`ps` evidence recorded).
- Reload the window (Cmd+R) mid-tick → the tick finishes or errors per §5.1's measured branch; the session never
  stays busy after it; the next tick is offered on the new connection.

**J4: sleep.** `pmset sleepnow` during a wait, then wake after two cadences. Expect exactly one `on_wake` tick and
the wake sentence. This measures, rather than assumes, the `Instant`-during-sleep behaviour.

**J5: self-paced.** A watch loop with "goose decides". Expect each tick's reason shown. Force a tick with no
`next_in` (a model that omits it) → `waiting_you`.

**J6: a tick that asks.** A goal that cannot be decided without the user (e.g. "pick the CSV delimiter the owner
wants"). Expect: the tick ends on `ask_user`, the row reads "Asked you", the loop shows Needs you, NO further tick
fires while the card is pinned; answering it runs the answer turn, and the next tick starts only after that turn ends.

**J7: two loops and a user (after L2c).** Two loop chats on the same MLX way and one ordinary chat. Expect: the two
loops' ticks do not wait on each other as if they were user replies; a message in the ordinary chat yields whichever
tick holds the way; a tick whose node would swap the way the user's reply holds waits for that reply (the
`waiting_turn` sentence names the node) and never delays the user's reply behind a swap.

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
   (once Q-230 makes it work; before that it is not shown) and the wake sentence in the rail. Forcing wakefulness is a
   battery decision the user makes once. Q-230's stated fix mentions a blocker "while a loop is armed, lane L"; lane L
   does not ask for that half in v1.
6. **Work windows for session loops ("only 9–18 on weekdays").** **Recommended: not in v1.** The shared clock
   already supports it, so it is a dialog field and one record field whenever wanted.
7. **Ticks on a strategy's Build role.** **Recommended: no in v1.** A tick is a chat reply and uses the Chat chain.
   Routing ticks to Build would make a loop swap models every tick on a two-way strategy (DESIGN-NODES §6.4's delegate
   warning, twice per tick).
8. **Worktree isolation for software loops (the Codex pattern).** **Recommended: not in v1.** R5 wants the loop's
   changes beside the chat's own changes in the working dir. A worktree toggle can come later, if a loop's edits
   collide with the user's own.
9. **Does a renderer reload create a new `GooseAcpAgent` in the same goosed?** **Answered by reading the code: yes.**
   Every connection runs `GooseAgentConnection::connect_to`, which calls `self.server.create_agent()`
   (`server.rs:3359`). So the tick door is per connection, registered at the connection's first dispatch and removed
   when its serving future ends (§5.1). What remains unmeasured is whether a closed connection drops an in-flight
   `on_prompt`; L2b measures that first (§5.1, "A renderer reload mid-tick").
   **MEASURED (L2b, 2026-09-28, `crates/goose/tests/acp_connection_close_test.rs`): (b), the future is DROPPED.**
   Over the real router (`create_acp_router`, a real TCP websocket, a model that never finishes): the close makes
   `agent-client-protocol-http`'s `run_ws` call `Connection::shutdown`, which aborts the connection's task; that task
   owns the connection future, whose task actor owns every `cx.spawn`ed handler, so `on_prompt` is dropped mid-await.
   Observed: the prompt's user turn goes 1 → 0 after the close, no end-of-turn outcome is recorded, and a new
   connection prompts the same chat at once (the busy set is per connection). L2b therefore releases everything a
   prompt holds through guards that drop with the future: the tick (`TickPrompt` → `errored{connection}`), the user
   turn and `user_turn_ended` (`UserPrompt`), the busy entry and manager token (`RunRegistration` — the manager can
   outlive the connection when LeanZero Link holds it), and the door (`LoopDoorClose`, a local of `connect_to`).
   One deviation from §5.1: the door opens right after `initialize` is answered, and only for a client that declared
   goose's custom notifications — a client that cannot hear `loops/tickDue` is no door (an offer there reaches no
   one, and its door would keep the loops attached after the last window that can run them closed).
10. **Should `loop_report` be required, or inferred from the last message?** **Recommended: required, with the loud
    `no_report` state.** Inferring a verdict from prose is the fallback gate 1 forbids. L9's J1 measures how often
    the local models miss it before any wording change.

---

## 12. Invariants this design touches, and how each is kept

| Invariant | Kept by |
|---|---|
| Gate 1: no silent substitution | No default delay, no inferred verdict, no "check failed" for a check that could not run, no template state file, no `null` for an unreadable record. Each is a named state (§4.6, §8.4) |
| Gate 2: specific text | The tick prompt is assembled from the loop's facts and the user's own goal and steps (§4.4). The template steps are shown to and editable by the user before they reach a model |
| Gate 4: reaping | The check leads its own process group (`configure_subprocess`); Stop check goes through the existing proof-gated `goose_sidecar::sigkill_owned_group`, pid alone when the proof fails. No new group-kill site |
| Gate 5: no time input | The cadence only starts ticks. No tick, check or model call has a timeout. Stall is a repeat. No new seconds constant; the runner takes `now` as a value |
| Gate 6 in spirit: one door | Ticks enter through `submitMessage` → `on_prompt`, the door typed messages use, marked by `_meta` that must match the runner's open offer. `/loop` does not start a turn itself |
| Gate 10: no absolutes | One new numeric const, `1/64` of the session's context window for the check's tail, with its `// ratio:` marker. Presets are grammar strings |
| The swarm engine is untouched | No edit in `swarm.rs`, `crates/goose-swarm/*` or the swarm command tree beyond L6's pure clock move, proven by `development_gates` |
| Q-132 turn priority | `after_user_turns` is unchanged. Ticks never take `user_turn()` and yield to one (§5.3, on the same way after L2c). No tick starts beside the previous tick's reviewers (§4.3) |
| Q-190 the chat's width | The rail stays an overlay; the L5 layout test measures it |
| Q-185 background kinds | A tick is a turn, not background. The fact check after a tick keeps its "Checking" kind |
| Owner UI rules | Solid `TONE_FILL` chips, no rails, no tints, Studio dialogs, i18n. Asserted by §10.2 |
| DESIGN-NODES §7.1 / §6.4 | Ticks are replies on the chip's route. The served record, the loader guard and the when-rules apply unchanged. Holders carry `kind: user \| tick`; a tick's demand never swaps under a user's open reply and never queues ahead of a user's demand (§5.5). The loop never loads a model itself |
| Q-169 stopped turns | A user's stop of a tick is recorded as today; a yield is not a user's stop and is never recorded as one (§5.2) |

---

## 13. Review corrections (2026-09-27)

The review's verdict was CONFIRMED-WITH-CORRECTION: L1 and L6 may be cut, L2, L3, L4, L5 and L8 may not be cut as
written. Each item was re-verified in the code at `978344982` before it was accepted; where the reviewer's evidence
was wrong, the row says so with the evidence.

| # | Sev | Item | Verdict | Evidence re-checked | What changed |
|---|---|---|---|---|---|
| 1 | HIGH | The tick cannot go through the named door | ACCEPTED | `prompt.ts:6-15` sends only `{sessionId, prompt}`; `convert_acp_prompt_to_message` builds its own `Message::user()` (`server.rs:1310`); the only server id is `steer_` (`:3046`); `submitMessage` returns silently on `activePromptAttemptId` (`chatSessionController.ts:157-158`); `clear_active_run` (`:2929`) precedes the response (`:3020`); an unlisted notification falls to `extNotification` (`client.gen.ts:2353-2365`), undefined in `acpConnection.ts:27-35`; `handleSubmit` appends first (`useChatSession.ts:191`). One addition: `PromptRequest` already has an ACP `_meta` field, so no protocol change is needed, only reading it | §5.1–§5.2: `_meta.goose.loopTick` carried by `prompt.ts`; the runner-minted id stamped by `on_prompt` only on a matching offer; `submitMessage` returns `'submitted' \| 'busy'` and can `preAppend` the marker; the offer stands until `tick_started`; `loops/tickRefused{reason}` and `loops/ready` (re-offer on the store's attempt clearing and the queue emptying: renderer events, no timer); `loops/tickDue`/`loops/changed` added to the notification schemas and the SDK regenerated; new slice L4r owns `prompt.ts`, `chatSessionController.ts`, `acpConnection.ts`; L0 owns the regenerated contract |
| 2 | HIGH | `ask_user` ends the turn, so `needs_you` never occurs as designed | ACCEPTED | `END_TURN_META_KEY` (`needs_you.rs:22`, set at `platform_extensions/needs_you.rs:128`), honoured at `agent.rs:2499-2506`; the answer returns as the next user message (`NeedsYouCard.tsx:232-236`) | §4.2 `asked{item_id, question}`; §4.6 row: after each tick the runner reads `needs_you.v0` for an item created during the tick and still open → Needs you, no further tick until the item is resolved and (answered) the answer's turn has ended, or (dismissed) at once with the prompt saying so; the resolve handler notifies the runner (L7) |
| 3 | HIGH | A yielded tick is recorded "You stopped this answer" | ACCEPTED | `record_stopped` runs for every `was_cancelled` (`server.rs:2897-2916`), notice text `turn_outcome.rs:116-128` | §5.2 steps 4-5: a cause cell beside the run's cancel token; cause `yield` skips `record_stopped` and records `yielded`; a user's Stop still records the notice (D12). L2b clears a stale `stopped` outcome on a yield |
| 4 | HIGH | "What you send steers it" is false | ACCEPTED | `ChatInput.tsx:1295-1297` queues while loading; steering only via `MessageQueue.tsx:243-256` → `handleStopAndSend` (`ChatInput.tsx:1461`); slash commands never steer (`useChatSession.ts:213-215`) | §0.5, §2.8, §5.3, §8.1 corrected: typed = queued; Send now steers; the queue row reads "Queued · Send now steers tick {n}" (L4 owns `MessageQueue.tsx` for the label only); composer Stop / an interruption word stop the tick (the loop pauses); the rail's Stop loop and `/loop stop` stop the LOOP immediately, because `/loop` controls typed during a turn go to `loops/control` directly (§7.2) |
| 5 | HIGH | Stalled / quiet-tick rules can never fire | ACCEPTED | every tick rewrites the state file; diffs are forwarded for `write`/`edit` only (`server.rs:2205-2233`) | §4.2 `wrote` (write/edit paths, state file excluded); §4.5 the exclusion rule; §4.6 and §8.4 worded "no write or edit outside the state file"; "state file not written" is a `stat`, not a diff |
| 6 | HIGH | After an app quit a loop sticks at "Looping in another window"; §5.4 contradicts itself | ACCEPTED | Q-223 (goosed survives the quit, ppid 1) and Q-229 (the quit can hang) are open in the ledger; the draft had `loops/get` claim (§5.1), be a pure read (L0), and show Paused+Resume (§5.4) | §5.1: `loops/get`/`list` pure reads with a derived `paused{closed}`; claims only on start / resume / tickNow; owner gains `app_pid` and proof of gone accepts "alive but reparented away from `app_pid`" (the orphan); the owner releases its loops when its last door closes; §5.4 rows rewritten |
| 7 | MED | Keep-awake does nothing | ACCEPTED (depends on Q-230) | `set-wakelock` saves the setting only (`main.ts:2823-2848`); Q-230 is open, "cutting" | The design DEPENDS on Q-230's fix: the dialog renders the toggle only once Q-230 has landed; no loop-specific blocker (§11 Q5, declining Q-230's "while a loop is armed" half for v1) |
| 8 | MED | L8 misses dependents | ACCEPTED | `rg 'loop_config\|loopConfig\|LoopConfig'`: `schedule_tool.rs:164`, `types.gen.ts:1777,1780,1849`, `zod.gen.ts:1854,1884,1938`, `openapi.json:6250-6253`, `GooseImportSection.tsx:88-98,161-170`, and more `scheduler.rs` lines (`:1154-1160` and seven constructors) than the draft listed | §6 lists every hit; L8 owns all of them, the regenerated OpenAPI, and the generated contract (handed from L0); the import section's loop half shows "Loops are no longer imported" and names an unreadable file (D11) |
| 9 | MED | The nodes seam is one-sided | ACCEPTED, one sub-claim REJECTED | Holders are `{session, root_session, way}` with no kind (DESIGN-NODES §6.4); step 8 makes a reply opening after a queued swap wait behind it. REJECTED: "the quoted DESIGN-NODES sentence doesn't exist" — it exists, at DESIGN-NODES §0 item 6.2 (line 32: "no agent **reply** (not a single model call) that was open before this demand still uses it, in any goose process on this Mac"); the draft mis-cited it as "§6.4 step 2" and dropped the parenthesis | §5.5: `kind: user \| tick` in `holders.rs` (requested of the S5 surgeon now); a tick demand never swaps under an open user holder and never queues ahead of a user demand (L2c, in `nodes_loader.rs` handed from S5); the citation corrected |
| 10 | MED | Ownership collisions | ACCEPTED | S5 owns the `on_prompt` guard (DESIGN-NODES §9 S5); `fdcac1d1a` edited `App.tsx` and `main.ts`; `crates/goose/src/lib.rs` is S0's, merged (`7025a2b11`); #1's files had no owner | §9 re-cut: L2b takes `server.rs` handed from S5 (S5 first, then L2b on top); L1's `App.tsx`/`main.ts` edits recorded as landed removals; the resume broadcast moves to L10 after S7; `App.tsx` for the harness route is handed from S11 to L9; every file of #1 owned (L4r, L0) |
| 11 | MED | The reviewers of tick n overlap tick n+1 | ACCEPTED | `assess_turn` and `check_turn_answer` spawned after the run clears (`server.rs:2975-3019`); Q-132's static-batch cost | §4.3: no tick starts before the previous tick's reviewers end; `on_prompt` hands their join handles to the runner (`tick_reviewed`), an event. `background_work`'s in-flight list is NOT used for this: the reviewers register there only once their spawned tasks reach `background_work::run`, so an empty list right after the turn does not mean they are done |
| 12 | MED | `loop_report` should end the turn | ACCEPTED | `END_TURN_META_KEY` is generic (any tool result, `agent.rs:2499-2506`); r6f kept generating after its final tool (`TICK-NOTES.md:739`), r4b never called it (`development-gates.md` §7) | §4.4: the result carries `END_TURN_META_KEY`; an invalid call is refused without it so the model can retry; the state file is rewritten BEFORE the call (the prompt says so) |
| 13 | MED | Forking a loop chat copies `loop.v0` | ACCEPTED | `copy_session` copies `extension_data` whole (`session_manager.rs:2186`); edit defaults to fork (`UserMessage.tsx:192`); the enabled extensions persist in the same map (`agent.rs:1404-1408`) | §4.1: the fork strips `loop.v0` and the `loop` extension (L3 owns `session_manager.rs` for this); an in-place edit that removes a tick's marker leaves a named row |
| 14 | MED-LOW | "Stop kills that group" is a new group-kill site | ACCEPTED | gate 4 sanctions `kill_app_tree` and the proof-gated `sigkill_owned_group` (`goose-sidecar/src/lib.rs:710-724`); goose already depends on goose-sidecar (`crates/goose/Cargo.toml:139`) | §4.6: Stop check calls `goose_sidecar::sigkill_owned_group`, pid alone when the proof fails; the check ends at its exit status, never at pipe EOF (swarm invariant 5) |
| 15 | MED-LOW | The check's output tail is unbounded | ACCEPTED | "a share of the check's output" grows with the output | §4.4: the longest suffix at most `window / 64` tokens of the session's context window, counted with `TokenCounter::count_tokens`; one `// ratio:` const; the full output goes to a log in goose's data dir |
| 16 | MED-LOW | Missing states; L7's precedence contradicts the store | ACCEPTED | `sessionStates` orders needs-you, running, background, then failed / stopped only when nothing else holds (`sessionActivityStore.ts:229-236`) | §4.6 rows for "the user stops the running tick" (composer Stop, Escape, an interruption word → `stopped_by_you`, loop paused) and "yielded"; §8.4 NOW rows; §8.6 and L7: needs-you > running > background > looping > failed > stopped > idle, with the reasons |
| 17a | LOW | §6's "most chats never had it" is false | ACCEPTED | `sessions.db` read-only now: 130 user sessions since 2026-09-01, 76 on `swarm`, 18 other, 36 with no provider name (the review's 75 of 89 was earlier the same day) | §6 corrected with the numbers |
| 17b | LOW | Q9: each connection creates a new `GooseAcpAgent`; the door must detect a dead connection; the runner has no handle to an Agent | ACCEPTED | `connect_to` → `create_agent()` (`server.rs:3355-3368`); `sessions` and `active_prompt_runs` are per `GooseAcpAgent` | §2.8, §11 Q9 answered; §5.1 doors registered at first dispatch and removed by a guard across `connect_to`'s serving future; the runner's only handle on a tick is the ticket `on_prompt` gives it (the run's cancel token + cause cell); the loop extension is synced by `on_prompt` and the ACP handlers, which hold the Agent (§4.4) |
| 17c | LOW | §4.2 must name `update_extension_state` | ACCEPTED | `session_manager.rs:461-481` (`BEGIN IMMEDIATE`) vs `set_extension_state` `:484` | §4.2 and §2.8; the owner claim is a compare-and-set inside it |
| 17d | LOW | L6 must also move `WorkWindow`'s serde defaults | ACCEPTED | `window.rs:8` imports `WorkWindow` from `manifest.rs`, whose `#[serde(default = "default_days" / "default_from" / "default_to")]` fns live at `manifest.rs:152-163` | L6 moves the struct, its `Default` and those three fns; a manifest test parses a missing and a partial `window:` |

**The risks the review named without proving:**
- **The yield cancels the whole tick even where the engine could serve both.** ADDRESSED. Q-132's receipt is a
  static-batching ENGINE, so contention exists only on the same way. v1a (in-process, before S5's holders exist)
  keeps the conservative "any user turn in this goosed yields"; L2c makes the trigger the user reply's lease on the
  tick's own way, Mac-wide. A user reply on another node, a cloud model or another way never yields a tick (§5.3).
  Cancelling only part of a tick is not possible: a turn is one stream.
- **A renderer reload mid-tick may leave the session busy.** ADDRESSED as a measurement with both branches designed
  (§5.1): L2b measures whether a closed connection drops `on_prompt`; if it does, a synchronous drop guard on the run
  registration clears the busy set and the tick ends `errored`; if it does not, the tick finishes unseen and the chat
  reloads it on `loops/changed`. J3 reloads mid-tick.
- **Fixed English template text vs gate 2.** ADDRESSED (§7.4): the steps are parameterised by the loop's own slots
  (state file, check, goal, last next step, working dir), each absence has its own sentence, and the text is shown and
  editable before any model sees it. The remaining fixed connectives are the user-approved method, the class gate 1's
  honest-empty list already names ("an instructional constant branched on a measured predicate"), not engine task text
  a user never saw.

**Confidence, restated per slice:** L1 high (landed) · L6 high · L0 high · L2a medium · L3 medium · L4r medium · L7
medium · L8 medium-high · L5 medium-high · L4 medium-high · L2b **low-medium** · L2c medium-low · L10 high · L9
medium. Where this revision is least certain, plainly:
- **L2b** edits `on_prompt` on top of S5's guard, and the reload behaviour is unmeasured; every exit must emit exactly
  one `tick_ended`. It does not ship without J2 and J3.
- **L2c** depends on S5's holder-change observation, which is being built now; a wake observed on a lifecycle read
  can outlast the reply by one observation, so a tick may start one observation late (never early).
- **The offer protocol** adds two requests and relies on the renderer telling the runner when its refusal clears; a
  renderer that dies between the refusal and `ready` leaves the offer open until a door registers again, which is the
  intended recovery, but it is new.
- **L3's value rests on local models calling `loop_report`**; J1 counts it before any wording change.
