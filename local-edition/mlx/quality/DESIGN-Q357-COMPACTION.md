# Q-357 — compaction: visible, steerable, better (design, 2026-09-28)

Owner, 2026-09-28: "So 3 things, visual, steer for compaction and finally optimize compaction process for better
results after compaction … some way in this menu [the rail's Loop/Changes] to inspect the pillars and what the
compaction would be looking like or some way to define better pillars … the overall UX/UI I leave with you."

## Today (verified file:line at d29c0bb19)
- Paths: all call `compact_messages` (context_mgmt/mod.rs:84) — reply start (agent.rs:1978–2055), context-length
  recovery (agent.rs:2810–2870), per-turn proactive (agent.rs:3249–3305), `/compact` (execute_commands.rs:165).
  Threshold `GOOSE_AUTO_COMPACT_THRESHOLD` 0.8 (mod.rs:25). Prompt prompts/compaction.md (generic, 9 sections,
  `<analysis>` wrapper). Q-342 (branch worktree-agent-a3ee8f967e7ed1ab8) = `SummaryRequest::ExtendsChat`.
- W1 cost: #3p 678 s = 441 s cold prefill (97,590 tok) + 237 s decode of 2,493 tok. After Q-342 decode dominates.
- W2 waste: #3p's stored summary (sessions.db messages.id 771038) 8,821 chars, 6,460 (73%) is the never-stripped
  `<analysis>` scratch, re-read every later turn.
- W3 paraphrase: the model re-writes the person's words, files and errors from memory (the #3p summary contradicts
  its own analysis on the 6 leads).
- W4 no steer: `"compact" => handle_compact_command(session_id)` drops the argument (execute_commands.rs:141);
  `COMPACT_TRIGGERS.contains(trim)` is exact-match (agent.rs:1930).
- W5 invisible: manual compaction runs in `execute_command` before the stream exists; auto shows two text lines
  (agent.rs:73); `ChatState.Compacting` (types/chatState.ts:6) never set; the summary is agent-only.
- W6: a failed threshold save uses `window.alert` (alerts/AlertBox.tsx:92) — owner rule violation.
- W7: first request after compaction prefills system+tools cold (Q-347, separate agent).
- "Pillars" exist today only as the swarm's acceptance criteria (swarm.rs:23331) — unrelated. Chat anchors that
  already survive compaction: the scratchpad (platform_extensions/todo.rs, whole every turn) and the ledger tail
  (ledger.rs:32, newest 5).

## OPTIMIZE — seven pillars, split by who writes them
Code-built from the whole stored session (hidden messages stay in sessions.db → no drift across compactions):
- P1 What you asked — every user message verbatim; newest whole, older cut to `RECORD_EXCERPT_CHARS` (mod.rs:690).
- P2 Files touched — from tool requests (engine-side twin of changes/fileDiff.ts:127 sessionChanges).
- P3 Errors — failed calls via `outcome_and_output` excerpts (mod.rs:728).
- P4 Your notes — the compaction note + pins, verbatim.
- P5 Ledger decisions/facts older than the 5-entry tail (the tail + scratchpad already ride every turn).
Model-written, ONLY: P6 Where we are + next step (goal from `/goal` if set) · P7 Decisions and their reasons not in
the ledger.
Stored summary = model text with the `NOTE` line and any `<analysis>` stripped, then a `<kept-by-goose>` block with
P1–P5. The instruction names P1–P5 and says goose keeps them itself — do not list them. ExtendsChat drops
`<analysis>` (changes Q-342's pinned `the_transcript_prompt_is_unchanged…` test deliberately); the Transcript
path + swarm workers stay BYTE-IDENTICAL, pinned by a test. Q-342 cache reuse kept: note + new text go only into
the final instruction message. Second compaction: "update the summary above; keep what still holds."

## STEER
- Stored in session `extension_data` key `compaction.v0 {note, standing, pins[]}`; entry points: meter menu,
  `/compact <note>`, the Context tab.
- Assessment costs no extra call: it is the first line of the same (cached) compaction request. Manual: the model
  writes `NOTE OK` or `NOTE QUESTION: <one question>` and stops. Auto (a turn cannot wait): `NOTE OK` or
  `NOTE CONCERN: <one sentence>`, then the summary. Stream the call (`Provider::complete` is stream-then-collect,
  goose-provider-types/src/base.rs:427, so no wire change) and read the first line as it arrives; QUESTION → drop
  the stream, the card asks. Missing line → said plainly (no silent fallback). One-time note cleared after use;
  standing kept.

## VISUAL
- In-chat CompactionCard at the compaction point, fed by `StatusMessage::Progress{compaction: CompactionStatus}`
  (optional-field pattern like `forming`/`stopped`, goose-sdk-types/src/custom_notifications.rs:64) + this
  session's `promptRead` (Q-337). Stages: Reading (PromptReadBar cached/new split) → Writing (tokens written, rate,
  parts = the model's section headings as they stream; bar = parts done / total) → Done (one line + "What was kept").
- Messages sent during compaction QUEUE and run after, against the compacted context (one engine, one resident
  prefix; a side request evicts it — Q-296; the old context is already past threshold).
- `/compact` moves into the reply stream so manual and auto show the same events.

## Context tab in the right rail (beside Loop, Changes)
"Kept word for word by goose" (P1–P5 with token sizes, from `_goose/unstable/session/compaction/preview`, code only,
no model call) · "goose writes" (P6–P7 + last compaction's size/time) · "Always here" (scratchpad + ledger tail) ·
"Your note" · "Pinned" (add/remove) · "Last compaction kept" (the stored summary).

## Words
Meter menu: "Context · 142.1K of 178.2K tokens · 80%" · "Auto compact at 80% ✎" (inline "Couldn't save: {error}") ·
"Note for the next compaction" (placeholder "What must survive — a rule, a number, a decision") · checkbox
"Use this note for every compaction in this chat" · "Compact now" · "See what it keeps".
Card: "Compacting the conversation" → "Reading the conversation · 142.1K tokens · 141.7K from cache · 412 new" →
"Writing the summary · 1.2K tokens written · 10.4 tok/s · part 2 of 3" → "Conversation compacted · 142.1K → 6.3K
tokens · 3m 58s · What was kept ▸". Question: "goose asks about your note: “{question}”" [Compact as written]
[Edit note] [Cancel]. Concern: "Compacted following your note. goose noted: “{concern}”". No verdict: "goose
didn't say whether your note was clear — it was followed as written." Queue: "Sends right after compacting".
Failure: "Compaction failed: {error}. Your conversation is unchanged." [Try again].

## Slices (file-disjoint)
| slice | owns | confidence |
|---|---|---|
| S1 core | context_mgmt/mod.rs, new context_mgmt/pillars.rs, prompts/compaction.md | high (pillars/strip) · medium (quality) |
| S2 wiring | agents/agent.rs, agents/execute_commands.rs, custom_notifications.rs, new custom_requests/compaction.rs, new context_mgmt/{state,acp}.rs, acp/server.rs | high |
| S3 card | new components/compaction/*, acp/adapter/gooseSessionNotifications.ts, ProgressiveMessageList.tsx, MessageQueue.tsx | medium-high |
| S4 menu | alerts/AlertBox.tsx, alerts/types.ts, ChatInput.tsx | high |
| S5 rail | session-rail/SessionRail.tsx, new components/contextRail/* | medium |
| S6 i18n | 16 catalogs + compiled | high |
| S7 | sidecar rank_wrapper.py system+tools cut — tracked under Q-347 | medium |

Live proves: S1 replay #3p (the 6 facts — 24-month rule, lead exception, seed 20260928, ISO/British/zero deps,
svc-edi has no row, 10/10 tests — recalled after; model part ≤ 2,361 chars vs 8,821). S2+S3 a split chat past 80%:
cached bar near full, parts counting up, a message typed during it queues then runs. S4 note "use the 12-month
cutoff" (chat says 24) → a question.

Sources: code.claude.com/docs/en/how-claude-code-works · decodeclaude.com/compaction-deep-dive ·
gist badlogic/cd2ef65b… (Claude Code/Codex/OpenCode/Amp) · kangwooklee.com codex_context_compaction ·
docs.openhands.dev context-condenser · factory.ai/news/evaluating-compression ·
anthropic.com/engineering/effective-context-engineering-for-ai-agents
