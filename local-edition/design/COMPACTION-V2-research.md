# Context compaction in coding agents: primary-source research notes

Compiled 2026-10-16 to feed the goose (local model, exact-prefix KV cache) compaction design.
Problem being solved: after a compaction the model loses its working state and spends 7-16 tool calls
re-reading 14-26 files plus a 13-19 minute re-plan, even though the summary itself was good.

Legend: **[P]** = read from a primary source (official docs, the project's own source code at a named
commit/tag, the authors' paper or blog). **[S]** = secondary source (third-party write-up or an
extraction of a closed binary); treat as unverified. **[UNVERIFIED]** = could not confirm.

---

## 1. Claude Code and the Anthropic API

### 1a. Claude Code auto-compact (the CLI)

**Trigger** [P]
- Official docs: "If you don't set an auto-compact window, Claude Code compacts when the conversation reaches
  the model's context limit", with exceptions: Sonnet/Opus 4.6 without extended context "compact at the 200K
  boundary"; native-1M models "compact before the window fills, at about 967K tokens by default"; cloud
  sessions "compact as the conversation approaches the model's limit".
  https://code.claude.com/docs/en/model-config (section "Default auto-compact thresholds")
- Configurable: `/autocompact 500k`, `autoCompactWindow` setting, `--autocompact` flag,
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW` env var; window 100K-1M. Same URL.
- There is also an `idleCompaction` setting (compacts a long conversation while the terminal is idle) —
  https://code.claude.com/docs/en/settings-reference (index entry only; full entry not read).
- Thrash guard: "If a single file or tool output is so large that context refills immediately after each
  summary, Claude Code stops auto-compacting after a few attempts" — error text
  `Autocompact is thrashing: the context refilled to the limit...`.
  https://code.claude.com/docs/en/how-claude-code-works , https://code.claude.com/docs/en/troubleshooting
- [S] Older third-party figures (95% of window; `AUTOCOMPACT_BUFFER_TOKENS = 13,000`) are out of date or
  unverified: LangChain (July 2025) wrote Claude Code runs "auto-compact after you exceed 95% of the context
  window" (https://www.langchain.com/blog/context-engineering-for-agents). Do not rely on them.

**Two tiers: clear tool outputs first, then summarize** [P]
- "Claude Code manages context automatically as you approach the limit. It clears older tool outputs first,
  then summarizes the conversation if needed. Your requests and key code snippets are preserved; detailed
  instructions from early in the conversation may be lost."
  https://code.claude.com/docs/en/how-claude-code-works
- "Microcompact" (the name) and its exact stub text: **[S]/[UNVERIFIED]**. ClaudeLog says micro-compact
  "automatically clears old tool calls to extend your session length", introduced v1.0.68, no stub text or
  threshold given and no primary citation (https://claudelog.com/faqs/what-is-micro-compact/). The only
  primary statement is the "clears older tool outputs first" sentence above.

**What the summary keeps** [P]
- Docs (context-window explorer, compaction step): "The summary keeps: your requests and intent, key
  technical concepts, files examined or modified with important code snippets, errors and how they were
  fixed, pending tasks, and current work. It replaces the verbatim conversation: full tool outputs and
  intermediate reasoning are gone." https://code.claude.com/docs/en/context-window
- [S] Exact prompt text, extracted verbatim from the npm package by Piebald-AI
  (https://github.com/Piebald-AI/claude-code-system-prompts, files
  `system-prompts/agent-prompt-conversation-summarization.md`, `...partial-compaction-instructions.md`,
  `...summarization-no-tools-guard.md`, ccVersion 2.1.27x-2.1.290). It asks for an `<analysis>` block
  (chronological pass over every message: requests, approach, decisions, "file names / full code snippets /
  function signatures / file edits", errors and fixes, user feedback) then a `<summary>` with 9 sections:
  1 Primary Request and Intent, 2 Key Technical Concepts, 3 Files and Code Sections ("include full code
  snippets where applicable"), 4 Errors and fixes, 5 Problem Solving, 6 All user messages ("List ALL user
  messages that are not tool results"), 7 Pending Tasks, 8 Current Work, 9 Optional Next Step (with
  "direct quotes from the most recent conversation ... verbatim to ensure there's no drift").
  The same 9-section prompt appears verbatim in Roo Code's `CONDENSE` support prompt (see 6b), which
  independently corroborates the extraction.
- [S] The summarizer turn is preceded by a no-tools guard: "CRITICAL: Respond with TEXT ONLY. Do NOT call
  any tools ... You already have all the context you need in the conversation above." — i.e. the summary is
  produced as a continuation of the live conversation (same prefix), not as a fresh request.
- [P] Since v2.1.198 the summarization request inherits the session's extended-thinking setting.
  https://code.claude.com/docs/en/context-window

**What is re-injected after compaction (this is the key answer to "does it restore files?")** [P]
Verbatim table, https://code.claude.com/docs/en/context-window ("What survives compaction"):

| Mechanism | After compaction |
| :- | :- |
| System prompt and output style | Both still apply |
| Project-root CLAUDE.md and unscoped rules | Re-injected from disk |
| Auto memory | Re-injected from disk |
| Git status snapshot | a fresh one is read |
| The plan Claude wrote in plan mode | Re-injected from disk |
| Rules with `paths:` frontmatter / nested CLAUDE.md | reloaded on demand |
| **Files Claude read or edited** | **Claude Code re-reads up to five, most recently modified first** |
| Invoked skill bodies | Re-injected, capped at 5,000 tokens per skill and 25,000 tokens total; oldest dropped first |
| Background commands / subagents | Keep running; Claude is reminded which are still running so it doesn't start a duplicate |
| SessionStart hooks matching `compact` | run, output added to the compacted context |

- "Right after compaction, Claude Code re-reads up to five of the files Claude has read or edited in the
  session, choosing the ones modified most recently. A file over 5,000 tokens comes back as a path reference
  without its content, shown as `Referenced file` instead of `Read`."
- [S] Stub for the over-size file (extracted): "Note: ${filename} was read before the last conversation was
  summarized, but the contents are too large to include. Use ${READ_TOOL_NAME} tool if you need to access it."
- Anthropic engineering post corroborates: the agent continues "with this compressed context plus the five
  most recently accessed files." https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- So: YES, verified. Claude Code restores the 5 most-recently-modified read/edited files (each <=5k tokens),
  the plan file, the todo-like plan, invoked skills, and re-reads CLAUDE.md/memory from disk.

**Prompt caching** [P/partial]
- Docs say nothing explicit about cache handling for the CLI's compaction. Inference from the [S] no-tools
  guard: the summary call reuses the cached conversation prefix; after the swap the new context is
  system prompt (cached separately) + summary + re-injected attachments, which is a new prefix from the
  summary onward.

**Measured results**: none published for the CLI itself.

### 1b. Anthropic API: context editing (`clear_tool_uses_20250919`) [P]
Source: https://platform.claude.com/docs/en/build-with-claude/context-editing (beta header
`context-management-2025-06-27`).
- (a/b) "clears tool results when conversation context grows beyond your configured threshold ... the API
  automatically clears the oldest tool results in chronological order." "The API replaces each cleared
  result with placeholder text indicating to Claude that it was removed." (exact stub string not published)
  By default only results are cleared; `clear_tool_inputs: true` also clears the call parameters.
- (c) Defaults: `trigger` = 100,000 input tokens (or a `tool_uses` count); `keep` = 3 tool uses ("how many
  recent tool use/result pairs to keep"); `clear_at_least` = none ("If the API can't clear at least the
  specified amount, the strategy will not be applied"); `exclude_tools` = none ("tool names whose tool uses
  and results should never be cleared"); `clear_tool_inputs` = false.
- (d) Caching: "Invalidates cached prompt prefixes when content is cleared. To account for this, clear enough
  tokens to make the cache invalidation worthwhile. Use the `clear_at_least` parameter ... You'll incur cache
  write costs each time content is cleared, but subsequent requests can reuse the newly cached prefix."
- `clear_thinking_20251015` clears old thinking blocks; "When thinking blocks are kept ... the prompt cache is
  preserved ... When ... cleared, the cache is invalidated at the point where clearing occurs."
- With the memory tool: "When your conversation context approaches the configured clearing threshold, Claude
  receives an automatic warning to preserve important information" so it can write to memory files first.
- (e) Measured, https://claude.com/blog/context-management (2025-09-29): memory tool + context editing
  "39% over baseline" on an internal agentic-search eval; context editing alone +29%; in a 100-turn web-search
  eval context editing cut token consumption by 84%.

### 1c. Anthropic API: server-side compaction [P]
- Threshold compaction `compact_20260112` (beta `compact-2026-01-12`):
  https://platform.claude.com/docs/en/build-with-claude/compaction-threshold
  - `trigger` default `{"type":"input_tokens","value":150000}`, "`value` must be at least 50,000 tokens".
  - Default prompt (varies by model; one variant quoted): "You have written a partial transcript for the
    initial task above. Please write a summary of the transcript. The purpose of this summary is to provide
    continuity so you can continue to make progress towards solving the task in a future context, where the
    raw history above may not be accessible and will be replaced with this summary. Write down anything that
    would be helpful, including the state, next steps, learnings etc. You must wrap your summary in a
    <summary></summary> block."
  - `instructions` "Completely replaces the default prompt". `pause_after_compaction` returns
    stop_reason `compaction` so the client can insert content (e.g. re-insert recent turns / files) before
    continuing.
  - "The API automatically drops all content blocks prior to the `compaction` block".
  - Caching: put a `cache_control` breakpoint at the end of the system prompt "so when compaction occurs:
    The system prompt cache remains valid and is read from cache; Only the compaction summary needs to be
    written as a new cache entry." Also allowed: `cache_control` on the compaction block itself.
- On-demand compaction (beta `compact-2026-09-04`) and keep-tail compaction:
  https://platform.claude.com/docs/en/build-with-claude/compaction ,
  https://platform.claude.com/docs/en/build-with-claude/compaction-keep-recent-turns
  - "Keep-tail compaction keeps the last few turns of a conversation word for word after the summary."
    "No parameter sets which turns are kept. You pick a cut point". "Put the cut where no tool call is left
    open, with each tool call and its result on the same side." Background compaction: "The request runs
    while work continues, and you swap the block in when it arrives."
- Client-side SDK compaction (deprecated `compaction_control`, default threshold 100,000), default prompt
  verbatim (also extracted by Piebald as Claude Code's SDK prompt): sections 1 Task Overview, 2 Current State
  ("Files created, modified, or analyzed (with paths if relevant)"), 3 Important Discoveries ("What approaches
  were tried that didn't work (and why)"), 4 Next Steps, 5 Context to Preserve; "err on the side of including
  information that would prevent duplicate work or repeated mistakes."
  https://platform.claude.com/docs/en/build-with-claude/context-editing (section "Client-side compaction")
  - Limitation noted there: "Less ideal use cases: Tasks requiring precise recall of early conversation
    details ... Tasks that need to maintain exact state across many variables".

### 1d. Anthropic "Effective context engineering for AI agents" (2025-09-29) [P]
https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- Compaction: "The model preserves architectural decisions, unresolved bugs, and implementation details while
  discarding redundant tool outputs or messages." Agent continues "with this compressed context plus the five
  most recently accessed files."
- "One of the safest lightest touch forms of compaction is tool result clearing".
- Structured note-taking: "the agent regularly writes notes persisted to memory outside of the context window"
  (Pokemon example: "for the last 1,234 steps I've been training my Pokémon in Route 1, Pikachu has gained 8
  levels toward the target of 10").
- Just-in-time retrieval: "maintain lightweight identifiers (file paths, stored queries, web links, etc.)".
- Sub-agents return "a condensed, distilled summary of its work (often 1,000-2,000 tokens)."
- No percentage numbers in this post (they are in 1b's blog).

---

## 2. OpenHands condensers

Sources [P]: V0 implementations at commit `25d9cf28905c2f5ece4e2a5fb6256952b27ad620` (2025-08-18) of
https://github.com/OpenHands/OpenHands (formerly All-Hands-AI/OpenHands), path
`openhands/memory/condenser/impl/` and `openhands/core/config/condenser_config.py`. The V0 package was
removed 2026-04-24 (commit 7498353, "Removed V0 memory package"); the current implementation is the SDK:
https://github.com/OpenHands/software-agent-sdk `openhands-sdk/openhands/sdk/context/condenser/`.

| Condenser | (a) kept verbatim | (b) masked/pruned + stub | (c) trigger |
|---|---|---|---|
| ObservationMaskingCondenser | every action/thought; observations inside the last `attention_window` events | observation content replaced by `AgentCondensationObservation('<MASKED>')` | every step (stateless view transform). Class default `attention_window=5`, config default 100 |
| RecentEventsCondenser | first `keep_first` (1) events + last `max_events` | everything between is dropped, no stub | every step |
| AmortizedForgettingCondenser | `keep_first` head + tail so that view shrinks to `max_size//2` | middle forgotten (CondensationAction range), no summary | when `len(view) > max_size` (100) |
| LLMAttentionCondenser | head + the events an LLM ranks most important (structured-output list of ids), padded with most recent | the rest forgotten | `len(view) > max_size` (100) |
| LLMSummarizingCondenser (V0) | `keep_first=1` head; tail = `max_size//2 - keep_first - 1` most recent events | forgotten middle replaced by one summary event at offset `keep_first`; each forgotten event truncated to `max_event_length=10_000` chars before summarizing; previous summary is fed back in (`<PREVIOUS SUMMARY>`) | `len(view) > max_size` (100) |
| LLMSummarizingCondenser (SDK, current) | `keep_first=2`; back half of the view | front half -> one summary; hard reset (summarize everything) on context-window errors, scaling event strings by 0.8 per retry | `len(view) > max_size` (240) or tokens > `max_tokens`; refuses if it would forget < `minimum_progress=0.1` of the view |

- Summary prompt (V0 and SDK `summarizing_system.j2`), verbatim headers: "You are maintaining a context-aware
  state summary for an interactive agent." Track USER_CONTEXT, TASK_TRACKING ("PRESERVE TASK IDs", SDK only),
  COMPLETED, PENDING, CURRENT_STATE; for code: CODE_STATE "{File paths, function signatures, data
  structures}", TESTS "{Failing cases, error messages, outputs}", CHANGES, DEPS, VERSION_CONTROL_STATUS.
- (d) Caching [P]: V0 `from_config` sets `caching_prompt = False` for the condenser LLM: "This condenser cannot
  take advantage of prompt caching. If it happens to be set, we'll pay for the cache writes but never get a
  chance to save on a read." SDK README: "condensation destroys the prompt cache, but doing so regularly keeps
  the cost of rebuilding the prompt cache low"; "the back half of the context is untouched, so the agent has
  an easy time continuing the current task." The halving design is explicitly an amortization: condense
  rarely, drop a lot at once, so the prefix is stable for many turns in between.
- Note the ObservationMasking V0 condenser re-masks on every step (sliding window) — every step changes an
  earlier message, so it is cache-hostile as written.
- (e) Measured [P], blog "OpenHands Context Condensation for More Efficient AI Agents" (Calvin Smith,
  2025-04-09), https://www.openhands.dev/blog/openhands-context-condensensation-for-more-efficient-ai-agents :
  "Up to 2x" per-turn API cost reduction, per-turn cost ends at "less than half" of baseline; baseline cost
  grows quadratically, condensed grows linearly; SWE-bench Verified subset: 54% solved with condensation vs
  53% baseline; trade-off was more turns. (Exact `max_size` used in the blog not stated in the post.)
- JetBrains (section 4) also tested on OpenHands: see there.

---

## 3. SWE-agent

Source [P]: https://github.com/SWE-agent/SWE-agent `sweagent/agent/history_processors.py` (main).
- `LastNObservations` (the "most classic history processor, used in the original paper to elide but the last
  5 observations"):
  - (a) kept: all thoughts/actions; the first observation ("We never remove the first observation, as it is
    the instance template"); the last `n` observations; any observation tagged `keep_output`.
  - (b) stub, verbatim: `Old environment output: ({num_text_lines} lines omitted)` (+ `({num_images} images
    omitted)`); `remove_output`-tagged observations are always elided.
  - (c) per step; `polling` parameter: "How many steps to keep between updating the number of observations to
    keep. This is useful for caching ... every time we change the history, we need to cache everything again.
    Effectively, we will now keep between `n` and `n+polling` observations."
  - (d) docstring: "using this history processor will break prompt caching (as the history of every query
    will change every time due to the elided observations). There are some workarounds possible with the
    `polling` parameter." and "most SotA models can now fit a lot of context, so generally this history
    processor is not always needed anymore."
- `ClosedWindowHistoryProcessor`: for file viewer windows, keeps only the LAST window per file; older windows
  of the same file replaced by `Outdated window with {N} lines omitted...` (stale file views are the first
  thing to go — directly relevant to re-read storms).
- `CacheControlHistoryProcessor`: moves `cache_control` marks to the last N user messages.
- (e) Measured [P], SWE-agent paper (NeurIPS 2024), https://arxiv.org/abs/2405.15793 (HTML v3):
  - Text: "observations preceding the last 5 are each collapsed into a single line ... we maintain essential
    information about the plan and action history while reducing unnecessary context, which allows for more
    interaction cycles and avoids showing outdated file information." Also "all past error messages except
    the first are omitted."
  - Table 3 (SWE-bench Lite, % resolved, GPT-4 Turbo): Last 5 Obs. 18.0; Full history 15.0 (-3.0);
    w/o demo 16.3. File viewer window: 30 lines 14.3, 100 lines 18.0, full file 12.7 (-5.3).
  - Table 5 dev-split sweep: GPT-4 Turbo T=0 window 100: Full 14.1 vs Last-5 15.1; Claude 3 Opus T=0 window
    100: Full 5.4 vs Last-5 8.1. Masking helped in 7 of 8 configs.

---

## 4. JetBrains: "The Complexity Trap" (Lindenbauer, Slinko, Felder, Bogomolov, Zharov) [P]
Paper: https://arxiv.org/abs/2508.21433 (v3 2025-10-27, DL4C workshop @ NeurIPS'25), HTML
https://arxiv.org/html/2508.21433 . Code: https://github.com/JetBrains-Research/the-complexity-trap (a
SWE-agent fork).
- (a/b) Observation masking: observations older than the last M=10 turns replaced by a placeholder;
  reasoning and actions kept in full. In the code the stub is SWE-agent's
  `Old environment output: (N lines omitted)` (fork's `history_processors.py` line 143). M=10 was the best
  window in their sweep (App. D.1).
- LLM-Summary: summarize N=21 turns at once, keep last M=10 turns verbatim; triggers after N+M turns;
  OpenHands-style prompt; separate summarizer at temperature 0. Summary placeholder in code:
  `Previous {omitted_count} turns omitted for brevity.` for omitted turns.
- Hybrid: masking (W=10) for the early phase, summarization deferred as last resort with N=43, M=10. Config
  `default_no_demo_checkpoint_same_model_openhands_N=43_M=10_masking_M=10.yaml` chains
  `summarize_every_n_turns(n=43, keep_last_m_turns=10)` then `last_n_observations(n=10)`. Reusing N=21 in the
  hybrid "made the hybrid less cost-efficient".
- (e) Results, SWE-agent on SWE-bench Verified (Table 1), solve % / $ per instance:
  - Qwen3-32B: raw 17.0/$1.12, masking 15.0/$0.55, summary 16.0/$0.50
  - Qwen3-32B thinking: raw 23.0/$0.51, masking 24.6/$0.46, summary 24.8/$0.51
  - Qwen3-Coder 480B: raw 53.4/$1.29, masking 54.8/$0.61 (-52.7%), summary 53.8/$0.64 (-50.4%)
  - Gemini 2.5 Flash: raw 32.8/$0.41, masking 35.6/$0.18, summary 36.0/$0.24
  - Gemini 2.5 Flash thinking: raw 40.4/$0.56, masking 36.4/$0.24, summary 31.4/$0.25
  - Abstract: masking "halves cost relative to the raw agent" while matching (sometimes slightly exceeding)
    summarization's solve rate.
  - Trajectory elongation: summary runs are longer — Gemini 2.5 Flash 52 turns (summary) vs 44 (masking) vs
    50 (raw); Qwen3-Coder 480B ~15% more turns than raw, ~13% more than masking. Authors: summaries may act
    as "a reinforcing signal, encouraging the agent to keep going" (they hide the signs that it is stuck).
  - Summary-generation calls are up to 7.2% of total cost (Qwen3-Coder 480B $0.0439/instance); subtracting
    them, the summary-vs-masking gap "largely disappears".
  - Hybrid (Qwen3-Coder 480B, Verified-50): +2.6 pp solve rate over raw, 7% cheaper than masking, 11% cheaper
    than summary.
- (d) Caching: each summarization call processes a unique turn sequence, so cache reuse is limited to the
  summary system prompt; with cache hits priced up to 10x below misses (Gemini), summarization costs more.

---

## 5. Aider [P]
Sources: https://github.com/Aider-AI/aider (main) `aider/history.py`, `aider/prompts.py`,
`aider/repomap.py`, `aider/models.py`, `aider/coders/base_coder.py`, `aider/coders/chat_chunks.py`;
docs https://aider.chat/docs/repomap.html , https://aider.chat/docs/usage/caching.html
- **Architecture that matters most:** file contents are NOT history. `ChatChunks.all_messages()` order is
  `system + examples + readonly_files + repo(map) + done(history) + chat_files + cur + reminder`. Files added
  to the chat are re-sent fresh every turn as their own chunk, after the summarized history. Summarizing
  history therefore never loses file contents — the "working set" survives by construction.
- Repo map: "a list of the files in the repo, along with the key symbols which are defined in each file";
  "the most important classes and functions along with their types and call signatures"; ranked with
  PageRank (networkx) over a file graph: edge weight = sqrt(num_refs) x multipliers (identifier mentioned in
  chat x10; long snake/camel/kebab identifiers (>=8 chars) x10; leading underscore x0.1; defined in >5 files
  x0.1; references from files in the chat x50); personalization vector boosts chat files and mentioned
  files/idents. `--map-tokens` default 1k; expands (map_mul_no_files=8) when no files are in the chat.
- Chat history summarization:
  - (c) trigger: when `done_messages` token count > `max_chat_history_tokens` =
    `min(max(max_input_tokens / 16, 1024), 8192)`; runs in a BACKGROUND THREAD after each exchange
    (`summarize_start` -> thread; `summarize_end` joins before the next send and only swaps if history did
    not change meanwhile).
  - (a/b) keeps the most recent messages up to half the budget verbatim (tail ends on an assistant message),
    summarizes the head; recurses up to depth 3 if still too big.
  - Model: `[weak_model, main_model]` — tries the cheap "weak model" first, falls back to main.
  - Prompt, verbatim: "*Briefly* summarize this partial conversation about programming. Include less detail
    about older parts and more detail about the most recent messages. ... The summary *MUST* include the
    function names, libraries, packages that are being discussed. The summary *MUST* include the filenames
    that are being referenced ... The summaries *MUST NOT* include ```...``` fenced code blocks! Phrase the
    summary with the USER in first person ... Start the summary with "I asked you..."." Prefix:
    "I spoke to you previously about a number of things."
- (d) Caching: cache_control breakpoints on examples/system, on repo map (+ read-only files), and on chat
  files; `--cache-keepalive-pings N` pings "up to N times over a period of N*5 minutes" to keep Anthropic's
  5-minute cache warm. Note the ordering puts the slowly-changing history BEFORE the frequently-edited chat
  files, so an edit to a file only invalidates from the chat-files chunk onward.
- (e) No published measurement of summarization quality.

---

## 6. Cline and Roo Code

### 6a. Cline [P]
Sources: https://github.com/cline/cline tag `v3.89.2`: `apps/vscode/src/core/prompts/contextManagement.ts`,
`apps/vscode/src/core/context/context-management/ContextManager.ts`, `context-window-utils.ts`,
`apps/vscode/src/core/prompts/responses.ts`, `apps/vscode/src/core/task/tools/handlers/SummarizeTaskHandler.ts`;
current main SDK `sdk/packages/core/src/extensions/context/{compaction.ts,compaction-shared.ts,
basic-compaction.ts,agentic-compaction.ts}`; doc `docs/features/auto-compact.mdx`.
- Legacy truncation (v3): (c) trigger when last request's `tokensIn+tokensOut+cacheWrites+cacheReads >=
  maxAllowedSize` where maxAllowedSize = window - 27k (64k models), - 30k (128k), - 40k (200k),
  max(window-40k, 0.8*window) otherwise. (a/b) "We always keep the first user-assistant pairing", drops half
  (or 3/4 when half is not enough) of the remaining pairs; notice inserted: "[NOTE] Some previous
  conversation history with the user has been removed to maintain optimal context window length. The initial
  user task has been retained for continuity, while intermediate conversation history has been removed."
- File-read deduplication before truncation: all but the latest read of the same file are replaced by
  `[[NOTE] This file read has been removed to save space in the context window. Refer to the latest file read
  for the most up to date version of this file.]`; if this saves >= 30% (`needToTruncate: percentSaved < 0.3`)
  truncation is skipped.
- Auto-compact / `summarize_task` (v3): summary prompt is appended to the LIVE conversation as explicit
  instructions ("The current conversation is rapidly running out of context ... you must call the
  summarize_task tool"), so the summary call reuses the cached prefix. Doc: "Summarization leverages your
  existing prompt cache from the conversation, so it costs about the same as any other tool call."
  Sections: Primary Request and Intent, Key Technical Concepts, Files and Code Sections, Problem Solving,
  Pending Tasks, Task Evolution (Original Task / Modifications / Current Active Task with direct quotes),
  Current Work, Next Step (verbatim quotes), **9. Required Files** ("the minimum number of files necessary to
  continue", most important first).
- **File restore after condense (verified in source):** `SummarizeTaskHandler` parses the "Required Files"
  section and auto-reads them: `MAX_FILES_LOADED = 8`, `MAX_FILES_PROCESSED = 10`, `MAX_CHARS = 100_000`; the
  contents are appended to the continuation with: "The following files were automatically read based on the
  files listed in the Required Files section: ... These are the latest versions of these files - you should
  reference them directly and not re-read them:". Continuation prompt: "This session is being continued from
  a previous conversation that ran out of context ... Please continue the conversation from where we left it
  off without asking the user any further questions."
- Current SDK (2026): `COMPACTION_TRIGGER_RATIO = 0.9` of usable input (usable = 0.9 x window if only the
  window is known), `DEFAULT_TARGET_RATIO = 0.7`, `DEFAULT_PRESERVE_RECENT_TOKENS = 20_000` kept verbatim,
  tool results and file contents capped at 2,000 chars inside the summarizer input. The summary always gets
  a deterministic `## Files` section (Read: / Modified:) built from tool-call history by `extractFileOps`,
  merged across compactions — not left to the model. Summary request sections: Goal, State
  (Done/In Progress/Blocked), Highlights, Next, Files. Messages that survived an earlier compaction are
  "frozen: a later compaction never re-folds them". A summarizer-free "basic" strategy keeps every typed user
  prompt and each older turn's concluding assistant answer, and attaches "dropped-work summaries" (files
  read, files edited, commands ran) to surviving prompts.
- (e) No published numbers.

### 6b. Roo Code [P]
Sources: https://github.com/RooCodeInc/Roo-Code (main) `src/core/condense/index.ts`,
`src/core/condense/foldedFileContext.ts`, `src/core/context-management/index.ts`,
`src/shared/support-prompt.ts`, `src/core/webview/ClineProvider.ts`.
- (c) Trigger: `contextPercent >= autoCondenseContextPercent` (default 100, range 5-100, per-profile override)
  OR tokens > `window * (1 - 0.1) - reservedTokens` (`TOKEN_BUFFER_PERCENTAGE = 0.1`). Falls back to sliding
  window truncation (`truncateConversation(messages, 0.5)`) with marker
  `[Sliding window truncation: N messages hidden to reduce context]`.
- (a/b) Non-destructive: all old messages tagged `condenseParent` (hidden from the API, restorable on
  rewind); one summary message (role user, "Fresh start model") appended. The summary content gets:
  the LLM summary; an "## Active Workflows" `<system-reminder>` with preserved `<command>` blocks;
  **folded file context** — for every file Roo read, tree-sitter signatures only ("function signatures, class
  declarations ... hiding implementation bodies"), one `<system-reminder>` per file, total cap 50,000 chars;
  and environment details on automatic triggers.
- Summary prompt: the Claude Code 9-section prompt verbatim (Primary Request and Intent ... All user
  messages ... Optional Next Step with verbatim quotes), prefixed by "CRITICAL: This summarization request is
  a SYSTEM OPERATION, not a user message."
- (d) Caching: the summary call is sent with a DIFFERENT system prompt (`SUMMARY_PROMPT`, "DO NOT call any
  tools") and tool blocks rewritten to text (`transformMessagesForCondensing`) — so the summary request does
  not share the agent's cached prefix. On an exact-prefix local KV cache this would be a full cold prefill of
  the whole history just to write the summary.
- (e) No published numbers.

---

## 7. Manus: "Context Engineering for AI Agents: Lessons from Building Manus" (Yichao 'Peak' Ji, 2025-07-18) [P]
https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus
- KV cache: "the KV-cache hit rate is the single most important metric for a production-stage AI agent."
  Average input:output ratio "around 100:1"; "A typical task in Manus requires around 50 tool calls on
  average." Cached vs uncached Claude Sonnet input: "0.30 USD/MTok" vs "3 USD/MTok".
- Prefix stability: "A common mistake is including a timestamp—especially one precise to the second—at the
  beginning of the system prompt." "Make your context append-only." "Avoid modifying previous actions or
  observations." "Ensure your serialization is deterministic." Mark cache breakpoints, at least the end of the
  system prompt. Self-hosting: "make sure prefix/prompt caching is enabled" and use "session IDs to route
  requests consistently across distributed workers."
- Tools: do not add/remove tools mid-run — "tool definitions live near the front of the context after
  serialization ... any change will invalidate the KV-cache for all subsequent actions and observations."
  Instead "it masks the token logits during decoding".
- Restorable compression: "we treat the file system as the ultimate context"; "Our compression strategies are
  always designed to be restorable"; "the content of a web page can be dropped from the context as long as the
  URL is preserved, and a document's contents can be omitted if its path remains available in the sandbox";
  "any irreversible compression carries risk."
- Recitation: Manus rewrites `todo.md` step by step; "By constantly rewriting the todo list, Manus is
  reciting its objectives into the end of the context. This pushes the global plan into the model's recent
  attention span, avoiding "lost-in-the-middle" issues".
- Keep errors: "leave the wrong turns in the context." "Erasing failure removes evidence."
- Few-shot: "don't few-shot yourself into a rut." "The more uniform your context, the more brittle your agent
  becomes."
- (e) No benchmark numbers beyond the cost/ratio facts above.

---

## 8. Other primary sources

- **Factory.ai, "Evaluating context compression" (2025-12-16)** [P] https://factory.com/news/evaluating-compression
  (redirected from factory.ai). Probe-based eval (Recall, Artifact, Continuation, Decision) over 36k+
  production messages, LLM judge 0-5. Overall: Factory 3.70, Anthropic 3.44, OpenAI 3.35. Artifact
  (file-tracking) dimension: 2.45 / 2.33 / 2.19 — "Artifact tracking remains an unsolved problem." Compression
  removed 98.6% / 98.7% / 99.3% of tokens. Factory's method = anchored iterative summarization: a "structured,
  persistent summary with explicit sections" (session intent, file modifications, decisions, next steps); on
  each compression "only the newly-truncated span is summarized and merged with the existing summary."
  "The right optimization target is not tokens per request. It is tokens per task." (The 36k figure comes
  from the secondary ZenML write-up; the table is from the primary page.)
- **Cognition, "Don't Build Multi-Agents" (Walden Yan, 2025-06-12)** [P] https://cognition.com/blog/dont-build-multi-agents
  — "we introduce a new LLM model whose key purpose is to compress a history of actions & conversation into
  key details, events, and decisions"; "you might even consider fine-tuning a smaller model ... this is in
  fact something we've done at Cognition." Principles: "Share context, and share full agent traces, not just
  individual messages"; "Actions carry implicit decisions, and conflicting decisions carry bad results".
- **LangChain, "Context Engineering for Agents" (2025-07-02)** — aggregation, not primary for any mechanism;
  useful taxonomy: write / select / compress / isolate. https://www.langchain.com/blog/context-engineering-for-agents

---

## 9. Cross-cutting findings relevant to goose's symptom (re-read storm + long re-plan)

1. Every mature system restores the WORKING SET deterministically instead of trusting the summary to carry it:
   Claude Code re-reads the 5 most-recently-modified files (<=5k tokens each, else a path stub) plus the plan
   file and invoked skills; Cline v3 auto-reads up to 8 "Required Files" (100k chars) and tells the model
   "you should reference them directly and not re-read them"; Roo injects tree-sitter-folded signatures of
   every file read (50k chars); Cline's current SDK appends a code-built `## Files` Read/Modified list;
   Aider never puts file contents in history at all. Factory measured file tracking as the weakest dimension
   of every summarizer (2.2-2.45/5), which matches goose's symptom exactly: the summary is good but the
   artifact trail is not, so the model re-reads.
2. The plan must live outside the summary: Claude Code re-injects the plan-mode file from disk; Manus recites
   todo.md at the tail; OpenHands SDK forces TASK_TRACKING with exact IDs; Anthropic's memory tool gets a
   pre-clear warning. A re-plan after compaction means the plan was only in the conversation.
3. Masking old observations is as good as summarizing for solve rate at roughly half the cost, and does not
   lengthen trajectories (summaries made runs ~13-15% longer). SWE-agent's own ablation: last-5 masking beat
   full history (18.0 vs 15.0). Stale views of the same file are the first thing to drop (SWE-agent
   ClosedWindow, Cline dedup).
4. Cache-aware batching is the universal fix for masking's cache cost: Anthropic `clear_at_least`, SWE-agent
   `polling` (keep between n and n+polling), OpenHands' "condense rarely, drop half" amortization, the
   hybrid's large N=43. Mask in big steps at rare moments, never one observation per turn.
5. Produce the summary as a continuation of the already-cached conversation (Claude Code no-tools guard,
   Cline summarize_task) rather than a separate request with its own system prompt (Roo, OpenHands V0 which
   disables caching). On an exact-prefix cache the difference is one cold prefill of the full history.
