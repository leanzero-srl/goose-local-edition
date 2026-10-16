# Compaction v2: never make the model re-acquire what goose already knows

Status: implemented on branch `t12/compaction-fixes` (worktree `~/Projects/goose-compaction`). Two
independent reviews returned FIX-FIRST; the fixes are in the review-fix commit (§9). Those fixes are NOT
YET COMPILED OR TESTED (laptop disk at the floor). Not yet run live. Primary-source research behind it:
[COMPACTION-V2-research.md](COMPACTION-V2-research.md). Written 2026-10-16.

## 1. The measured problem

Two Forge 2.0 runs on Gabee, model Qwen3.8-27B through the `openai` provider at the app's loopback MLX
engine, chat effort `medium`. Run A = `cloud-f02f8ef9`, run B = `cloud-8e3bdc1d`. The numbers come from
the runs' `sessions.db`, `state/logs/llm_request.*.jsonl` and the engine log. The parse scripts are in the
session scratchpad `compaction/`.

| what | measured |
|---|---|
| trigger | all 6 compactions threshold-triggered at 0.8 × the 128,000 default (102.4k), because main's window probe missed; the engine serves 158,833. This branch carries the probe (`probe_context_window`, `goose-providers/src/served_window.rs`). |
| context at each compaction (B) | 103,412-111,303 prompt tokens: 15-43k of them old reasoning (Qwen3.8 `preserve_thinking`), 33-44k old tool output |
| one tool result | `tree` depth 3 = 161,890 chars / 6,793 lines, node_modules included, 64% of A's context |
| task prompt | duplicated per compaction: the kept block carried the 5,074-char prompt 1, 1, 2, 3, 4 times, plus the appended copy, so 5 copies by C5 |
| summary call | out 4,941 / 11,417 / 11,615 tokens and 483.9 / 1,036.0 / 1,279.7 s at C1-C3, while the stored text was only ~3.0k / 4.9k / 6.2k tokens. The rest was reasoning at `medium`. |
| C4 | the chat-shaped summary request read 100,658 of 103,412 tokens from cache and was answered with a TOOL CALL (91 tokens). The transcript fallback then read 0 of 85,085, took 568.8 s to its first token and 1,207 s in all. |
| after the compaction, B | C1: **37 calls and no write before the next compaction**. C2: 18 calls and 74.6 min to the first write. C3: 6 calls and 18.9 min. C4: 10 calls and 38.4 min. C5: 2 calls and 9.7 min. |
| after the compaction, A | C1: 27 calls and no write until the data ends. The files it had read before were re-read (7.2k → 34.8k tokens), then it re-planned for 1,154 s. |

The summaries were GOOD: they held the file list, the decisions and the next step. Yet the model re-read
14-26 files and re-planned for 800-1,150 s, and in B the next step was written 2 h 37 min after a
compaction. Richer summary text is not the lever. Factory's compression evaluation measured the same
weakness in every summarizer: file tracking scored lowest, 2.2-2.45 out of 5. The model loses its working
state (the files' current contents, its own recent actions and reasoning trail) and rebuilds it with calls.
After B C1, that rebuild refilled the window before a single write: the thrash.

## 2. Principles

1. **Never make the model re-acquire what goose already knows.** goose knows which files were read and
   written, and it can read their current content from disk. It knows what the model wrote and which
   calls ran. Give these back by CODE, not through a summary the model must trust.
2. **Cache-friendly: change the prefix at most once per compaction, never per turn.** On an exact-prefix KV
   cache a changed early message re-prefills everything after it. Q-294 measured per-turn condensation
   reading 0 of 109,655 tokens. A cold prefill of ~100k tokens costs 150-450 s here.
3. **Drop before summarizing.** Old tool outputs and old reasoning are the bulk of the context and the
   least of its information. Only what still does not fit gets summarized. Evidence: JetBrains "Complexity
   Trap", Claude Code's documented "clears older tool outputs first, then summarizes", SWE-agent.
4. **Every omission is said** (gate 1). A stub names the outcome and the output's first and last lines.
   A file left out of the working set is named, with the reason.
5. **No new absolutes** (gate 10). Budgets are shares of the probed window. The acceptance ceiling derives
   from the threshold. Size limits reuse the shell tool's.
6. **The person's copy of the conversation is never edited.** Masking and summaries act on the agent's
   copy (agent-only messages). The originals stay user-visible, as compaction already does.

## 3. The architecture (as built)

```
 threshold crossed (0.8 × the SERVED window: probe_context_window)
        │
        ├── stage 0, always (prevention): tree cap + dependency folders named; the task prompt kept once
        │
        ├── stage 1 MASK   [auto, chat, no note]   no model call
        │      old tool outputs → stubs of facts; old reasoning dropped;
        │      earlier working set → "superseded"; + trim line + WORKING SET
        │      taken iff estimate ≤ limit × (2·threshold − 1)   (60% at 0.8)
        │
        └── stage 2 SUMMARY (otherwise; and every manual / recovery compaction)
               chat-shaped request (Q-342), reasoning OFF, one retry with tool_choice none
               on a tool-call answer, then the transcript fallback;
               stored: summary + <kept-by-goose> + trim line + WORKING SET
                       + the person's message + the newest turns VERBATIM (mid-loop)
```

### 3.1 Trigger

Unchanged mechanism (`check_if_compaction_needed`, `effective_context_limit`). On this branch the window
is the served one: 158,833 on Gabee, read from the engine's guarded `max_model_len`. So the trigger moves
from 102.4k to ~127k. Nothing new was added here.

### 3.2 Stage 0: prevention (commits 128f7a8b0, 74e8edeba)

- **`tree`**: dependency and build folders (`skills::should_skip_dir`, the list the skills walk already
  uses) are named on one line, not walked. The listing is cut by the shell tool's own `truncate_output`
  (50,000 bytes / 2,000 lines): it shows both ends, saves the whole listing to a file, and a notice names
  that file. Replayed on run A's call, the result drops from 161,890 to ~1,290 chars.
  Tunable: `GOOSE_TREE_BOUNDED`.
- **Task prompt kept once**: the preserved copy is compared trimmed, and a chain of hidden compaction
  copies counts as one message. In run B's trace the copies drop from 5 to 1 at C5.

### 3.3 Stage 1: MASK (commit 8f55b8afd, `context_mgmt/mask.rs`)

At an automatic chat compaction with no note for the summary:

- **Outputs.** Every tool output older than the newest `kept_budget_chars` of output (1/16 of the window;
  the newest output is always kept) becomes a stub:
  `[Output elided at compaction (N chars): the call succeeded (exit 0). Output, L lines; first: "…"; last:
  "…" Files it printed are in "Current file contents after compaction" when listed there.]`: facts, with
  no instruction to re-run anything. The facts come from the existing `outcome_and_output`. Outputs of at
  most 2 × `RECORD_EXCERPT_CHARS` stay whole. A stub from an earlier stage 1 is never masked again.
- **Reasoning is KEPT by default** (`GOOSE_COMPACTION_DROP_REASONING=false`). The reasoning is where the
  plan lives. In run B's first span the 89 assistant messages held 555 chars of text and 250,387 chars of
  reasoning. The visible text was "The design is clear now. Time to implement."; the design itself existed
  only in the reasoning. JetBrains' measured masking also kept reasoning and actions whole.
  - When the drop is switched on, turns before the oldest kept output lose their reasoning. The newest
    assistant turn always keeps its own, so Anthropic's rule for the final tool-loop turn holds.
  - Only reasoning the provider actually rendered counts as freed. The openai format sends it only with
    `preserve_thinking_context`, and a template may drop it before the last user message. The bound is
    the provider-measured `used` minus goose's count of the same request with no reasoning
    (`count_chat_tokens`). This is an upper bound, so it may still overstate; it never counts reasoning
    that wasn't sent.
- **Everything the model and the person wrote stays**: the texts, the tool calls with their arguments,
  the reasoning (by default), the order.
- **An earlier working set** becomes one "superseded" line.
- **The new state** is placed after a trim line: "Your context was trimmed: …". It deliberately does NOT
  start with `COMPACTED_PREFIX`, so `latest_summary` and `earlier_summary` see no summary. The working set
  follows it, and the person's trailing message comes last when the compaction ran at a reply's start.
- **Acceptance.** Stage 1 is taken only if `used + added − removed ≤ limit × (2·threshold − 1)`, counted
  with goose's tokenizer over the exact text removed and added. In words: it must leave at least as much
  room before the compaction point as the window has past it. Below that, the next stage 1 would come a
  few turns later and pay another cold prefill. That is thrash, so the summary runs instead.

### 3.4 The working set (commit e63888571, `context_mgmt/working_set.rs`)

**Which files.** The files the summarized calls wrote (`write` / `edit` `path`) or read, and ONLY calls
whose result came back and succeeded. A call that was declined in approve mode, skipped by the repeat
guard, or failed read nothing, so goose reads nothing for it. A read is a shell `cat` / `nl` / `sed` /
`less` / `more` / `head` / `tail` at the head of a pipeline.
- The parser follows `cd` chains, a call's `cwd` argument, quotes, redirections, `-n N`, `sed`'s script
  argument and globs in the last path component.
- `sed -i` is an edit, not a read. Variables, substitutions and markup are never taken as paths.
- Heredocs (`<<`, `<<-`) are handled: the delimiter is consumed and the body skipped up to its closing
  line. A here-string (`<<<`) word is skipped too.

**What is shown.**
- Order: newest touch first, each file once, each text file **whole or not at all** within
  `GOOSE_COMPACTION_WORKING_SET_SHARE` of the window (default 0.25).
- Named but not shown: a file not on disk now, a binary file, an unreadable one, one too big for the budget
  left, one only ever read with head/tail (B's 1,027-line `.dev/rec1.log`), and one inside a dependency
  folder (B's six node_modules `.d.ts`).
- Named and NEVER READ:
  - a file outside the chat's folder;
  - a file that looks like a credential, by folder (`.ssh`, `.aws`, `.gnupg`, `.kube`, `.azure`,
    `.docker`) or by name (`.env*`, `*.pem` / `.key` / `.p12` / `.pfx` / `.keystore` / `.jks`, `id_rsa*`
    and its siblings, `.netrc`, `.npmrc`, `.pypirc`, `.git-credentials`, `credentials`, `*secret*`);
  - a file that isn't regular (a device, FIFO or socket; `cat /dev/urandom | tr …` used to hang the
    compaction).
- Regular files are read through a bound of 4 × the budget left, plus 1 byte.
- The label says these are the contents "as they are on disk NOW", and that they supersede any earlier
  view.

**Where it goes.** It is an agent-only user message right after the summary's or the trim's line. That
keeps it out of `latest_summary` and the Context tab, and out of the preserved-person-message search.

### 3.5 Stage 2: SUMMARY (commits 94f5717c4, 46167cf1f, 2d0929474)

- **The request** is chat-shaped (Q-342) and unchanged, so it reads the cached conversation.
- **Reasoning OFF where the format keeps the prefix** (`GOOSE_COMPACTION_SUMMARY_THINKING`: `off` /
  `chat`, unset = by provider). `Provider::reasoning_off_keeps_the_prefix(model)` is true only for the
  openai chat-completions format at a loopback or Rapid-MLX endpoint. Anthropic's formatter drops signed
  thinking blocks when effort is Off, and Anthropic invalidates its message cache on a thinking change, so
  everywhere else the chat's own setting stays. Run B's own captured request
  (`llm_request.0`: 59 messages, 5 tools, 93,417 chars) was rendered through the Qwen3.8 template both
  ways. At `medium` vs thinking off it is byte-identical up to the generation prompt, so the cache read is
  unchanged. Controls: `xhigh` and `low` write a system line and differ. A chat pinned at those levels pays
  one cold prefill instead of the reasoning; this is documented.
- **A tool-call answer is asked once more** with `tool_choice: none`, but only where the format puts it on
  the wire (`Provider::sends_tool_choice`: the openai chat-completions format). Anthropic, Responses,
  Ollama and Google never send it, so there the transcript fallback follows as before, and the log says
  why. A test asserts the retry on the REQUEST BODY. The retry sends the same messages and tools, so it
  hits the same prefix. Reasoning is pinned off where nothing pins it, because tool_choice none lifts
  the engine's thinking-off for tool requests and Qwen3.8 would write its xhigh line. The retry falls back
  to the transcript, as before, when: it is answered with a tool call again, it errors, or its text has
  none of the summary's sections. The last case covers an engine that hands back the refused call as text.
- **The stored result**: summary + `<kept-by-goose>` + the working set + the person's message + (mid-loop
  only) **the newest turns verbatim**, up to 1/16 of the window.
- **Room guard** (`context_mgmt::Room`, computed per compaction).
  - What follows a summary is sized to fit under `limit × (2·threshold − 1)`. The inputs are the window,
    the compaction point, and the system prompt + tools counted as the next request counts them.
  - The tail takes at most a quarter of the room under that ceiling after the head.
  - The working set gets what is left after the head, the summary with its kept block, the continuation,
    the person's message and the tail. It is shrunk once by its measured token overshoot, and dropped if it
    still does not fit.
  - When even that leaves the context past the compaction point, `compaction_cannot_make_room` is logged
    loudly. Example: a 32k window with a 12k head used to end above 80% and compact on every tool
    iteration. Every surveyed system keeps a verbatim
  tail. Chats kept none, because `GOOSE_COMPACT_KEEP_TAIL` defaults to 0, a swarm-golden default. An
  explicit `GOOSE_COMPACT_KEEP_TAIL` still wins.

### 3.6 What the cache pays

| path | prefills |
|---|---|
| before (B C1) | summary request (cached ~101.8k + reasoning decode) **then** a cold post-compaction request **then** 37 re-read calls |
| stage 1 | ONE cold prefill of the masked conversation and no summary call (68,280 tokens at 150-220 tok/s = 310-455 s, for the reasoning-dropped estimate; by default stage 1 is not taken at any of B's points, §7). The system+tools head stays cached (Q-347). |
| stage 2 | the summary request reads the cached conversation (unchanged), then the post-compaction request is cold once, as before. The working set and tail make it bigger by up to 5/16 of the window. |

Nothing changes per turn. Stage 1 and stage 2 each change the prefix once, at the compaction point.

## 4. Alternatives considered and rejected

| alternative | why not |
|---|---|
| per-turn masking or condensation (SWE-agent `last_n_observations`, the existing `maybe_summarize_tool_pairs`) | Q-294 / Q-516: on a caching engine each turn re-reads 100k+ tokens cold. goose already switches pair condensation off for caching chats. Batching into the compaction point is the universal fix (Anthropic `clear_at_least`, SWE-agent `polling`, OpenHands' drop-half). |
| a richer summary prompt (more sections, a "required files" list written by the model, as in Cline v3) | the summaries already held the file list; the model re-read anyway. Factory measured file tracking as every summarizer's weakest dimension. A code-built list (Cline's current SDK, Claude Code) is the fix. |
| re-injecting files by the model's own list (Cline v3) | the model's list is a claim; the calls are facts. |
| re-injecting the top-5 recently edited files at ≤5k tokens each (Claude Code) | Forge's working set is 15-35 files, mostly read and not edited, and several are 5-30k chars. A budget share fits the measured re-read sets (17-26% of the window) better than a file count. |
| tree-sitter signatures of every file read (Roo) / repo map (Aider) | the model re-reads whole files to edit them, and a skeleton does not stop that. Parsers per language would be new dependencies. Kept as a possible stage-1.5 for files over the budget. |
| a separate summarizer model or transcript request | the request starts cold (C4: 568.8 s to first token). Q-342 already proved the chat-shaped request. |
| dropping reasoning every turn | it changes the prefix every turn (principle 2). |
| raising the threshold instead | it only delays the compaction; the working-state loss is the same. |
| a memory / notes tool the model maintains (Anthropic memory tool, Manus todo.md) | goose already re-injects its ledger and todo into every turn-context block (`get_moim` in ledger.rs / todo.rs) when those extensions are on. A model-maintained file depends on the model doing it. Not needed for this failure. |
| sub-agents for exploration (Anthropic, Cognition caution) | an architecture change outside compaction; out of scope. |

## 5. Failure modes and what guards each

| failure | guard / honest status |
|---|---|
| **thrash**: compacting again a few turns later | Stage 1 is taken only when it leaves ≥ (1 − threshold) of the window before the next point. After stage 2 the room guard (§3.5) sizes the tail and the working set to the room under the same ceiling, and logs `compaction_cannot_make_room` when the head, summary and kept block alone are past the point. **Still not guarded**: a repeat stop like Claude Code's "thrashing" error; goose logs but compacts again. |
| **cold prefix every turn** | none of this runs per turn. |
| **stale file content** | the block is labeled as content at the compaction. Later edits show as later calls. The next compaction marks the old block superseded and places a fresh one. |
| **the working set gives back what masking takes** | MEASURED: at B C2/C3 the working set (~36-39k tokens) ≈ the masked outputs, because those outputs were the file reads. That is why stage 1 declines there. It is the honest cost of giving the files back. |
| **tokens vs chars** | the budget is in chars at 4 chars/token. Code tokenizes denser: B's 128k-char budget was 39k tokens, 30% of the window and not 25%. |
| **files read by `node -e`, `grep`, `find`, a script** | not tracked (B's `manifest-schema.json`). The model reads them again. |
| **the model ignores the block and re-reads anyway** | NOT MEASURED. This is the live run's first number. |
| **the reasoning drop deletes the plan** | MEASURED in review: B's plan lived in the reasoning (555 chars of text vs 250,387 of reasoning). So the drop is OFF by default; `GOOSE_COMPACTION_DROP_REASONING=true` turns it on for an experiment. My first trace claimed the plan was kept with the drop on; that was false. |
| **phantom savings** | reasoning that the provider never rendered would have counted as freed, and stage 1 would have been accepted and then compacted again one iteration later. Fixed by bounding it with `used` minus the reasoning-free count (§3.3). |
| **reading what the model never read** | only calls that succeeded; outside-folder, credential-like and non-regular files are named and never read (§3.4). |
| **tool_choice none retry on an engine that ignores it** | guarded: an answer with no summary section falls back to the transcript. Whether Rapid-MLX honours `none` is NOT MEASURED. |
| **cloud providers** | Reasoning off on the summary is NOT applied to them by default, because Anthropic's formatter drops signed thinking at Off and Anthropic invalidates its cache on a thinking change. The tool_choice retry is skipped where the format can't carry it. Stage 1 masks outputs only, by default, so on a caching cloud provider it costs one cache write per compaction instead of a summary call. The newest assistant turn keeps its thinking. Wrappers answer for themselves (round 3):
- `SwarmProvider` asks every node its chat route may pick (`swarm_router::chat_route_kinds`) and says yes only if
  all do. MLX here and Link peers say yes to both. LM Studio says yes to `tool_choice`, and to the prefix rule only
  at a loopback endpoint. A cloud node says no; its registry format isn't built here. An unresolvable route says no.
- OpenAI-compatible: `tool_choice` yes; the prefix rule only at a loopback host.
- LiteLLM and OpenRouter: `tool_choice` yes (the OpenAI chat format copies it); the prefix rule no, since an
  Anthropic model may be behind them.
- Databricks: no to both, because its Claude and Responses paths vary. The no-regression run is in §7. |

## 6. Tunables (env > config > default)

| knob | default | why the default |
|---|---|---|
| `GOOSE_TREE_BOUNDED` | true | 161,890-char tree measured; shell already bounded |
| `GOOSE_COMPACTION_SUMMARY_THINKING` | unset = `off` where `reasoning_off_keeps_the_prefix` (openai chat format at a loopback / Rapid-MLX engine), the chat's setting elsewhere | 1,036-1,280 s of reasoning per summary at `medium`; prefix byte-identical at `medium` there; Anthropic's would change |
| `GOOSE_COMPACTION_SUMMARY_TOOL_CALL_RETRY` | true (only where `sends_tool_choice`) | C4: 568.8 s cold transcript after a cached 100,658-token request |
| `GOOSE_COMPACTION_WORKING_SET_SHARE` | 0.25 (0 = off) | re-read sets were 17-26% of the window |
| `GOOSE_COMPACTION_MASK_FIRST` | true | outputs-only masking; on B no point passes the ceiling, so B's runs would be unchanged here; it is taken where outputs dominate the context |
| `GOOSE_COMPACTION_DROP_REASONING` | **false** | the plan lives in the reasoning (B_C1: 555 vs 250,387 chars) |
| `GOOSE_COMPACT_KEEP_TAIL` | unset = chats mid-loop keep 1/16 of the window verbatim; a number = that many messages for every compaction | every surveyed system keeps a verbatim tail |

Boolean knobs accept true/false, on/off, yes/no and 1/0 (`knob_bool`); anything else is warned and the
default stands.

Shares reused rather than invented: `KEPT_WINDOW_SHARE` (1/16) for both the kept tail and the newest
outputs stage 1 keeps whole; the shell's `OUTPUT_LIMIT_*`; `RECORD_EXCERPT_CHARS`; and the threshold
itself for stage 1's ceiling.

## 7. Test plan

**Done before the review (unit, `cargo test -p goose --lib -- context_mgmt developer::tree`)**: 66
context_mgmt tests and 7 tree tests passed, and clippy `-D warnings` was clean. **The review-fix commit's
code and tests are UNCOMPILED** (laptop disk at the floor). They must be built and run elsewhere:
`cargo test -p goose --lib -- context_mgmt developer::tree model_config`,
`cargo test -p goose-providers`, and clippy on goose, goose-providers and goose-provider-types. Each new behaviour has a test named in its commit,
including the negative control for the prompt dedupe (3 copies on the old code, 1 on the new).

**Done (offline replay through the real code, ignored tests)**:
`working_set::trace::trace_a_runs_working_set` and `mask::trace::trace_a_runs_compaction_point`. Inputs:
run A's and B's messages exported from `sessions.db`, with paths mapped to local copies of the app folders.

Replay with the drop ON (as first committed; the reasoning figure is not bounded by what was rendered):

| B point | used | stage 1 removes | adds (stubs + working set) | after | decision |
|---|---|---|---|---|---|
| C1 | 104,499 | 73,509 (outputs 37,993) | 1,489 + 35,801 | 68,280 (53%) | stage 1 (WITH THE PLAN DROPPED; see §5) |
| C2 | 111,303 | 38,142 | 1,071 + 39,040 | 113,272 (88%) | summary |
| C3 | 106,813 | 34,436 | 1,006 + 36,083 | 109,466 (86%) | summary |
| C4 | 103,412 | 41,072 | 1,523 + 16,855 | 80,718 (63%) | summary |
| C5 | 104,292 | 38,779 | 6,152 + 11,389 | 83,054 (65%) | summary |

With the DEFAULT (reasoning kept), no B point is taken: C1 would come to 103,796, over the ceiling of
76,800. Stage 2 runs at every point, with the working set, the tail and the room guard. The window is
128,000 here, as the run had it. B's re-read set after
C1 was 23 files; 16 of them are in the replayed working set, and the other 7 are absent from the local
copy or were read via `node -e`. A's 5 genuine re-reads after C1 are all in its block.

To rerun:
`GOOSE_MASK_TRACE=<B_Cn.json> GOOSE_MASK_TRACE_DIR=<app> GOOSE_MASK_TRACE_USED=<prompt tokens>
GOOSE_MASK_TRACE_WINDOW=128000 cargo test -p goose --lib -- --ignored trace_a_runs_compaction_point --nocapture`

**THE metric for the live Forge 2.0 run.** The run is started from the Benchmark view (gate 3), on the
same model and machine. For every compaction, record:
1. which stage ran, and its wall time. For stage 2, the summary call's out-tokens and seconds.
2. **calls and tokens until the first productive write** (`write` / `edit`) after it.
   Baseline: B C1 37 calls / no write, C2 18 / 74.6 min, C3 6 / 18.9 min, C4 10 / 38.4 min,
   C5 2 / 9.7 min; A C1 27 / no write.
3. how many of those calls read a file the working set showed (the re-read rate; target ≈ 0).
4. the first post-compaction request's cache read, and the request after it (one cold prefill, then warm).
5. compactions per run and the score, against the same tier's earlier local runs.

**Cloud no-regression**: one Forge 2.0 (or Gauntlet) run with an OpenRouter/Anthropic model from the
Benchmark view, compared with that model's last run on: the score, the number of compactions, tokens
billed, and any provider 4xx.

**Not run here** (disk at 16 GB, floor 15): `crates/goose/tests/compaction.rs`. Its conversations are
text-only and its auto case compacts at a reply's start, so stage 1 frees nothing and declines, and no tail
is kept. It must still be run before merge.

## 8. Commits (oldest first)

- `128f7a8b0` developer tree: dependency folders named, listing cut the shell's way (`GOOSE_TREE_BOUNDED`)
- `74e8edeba` kept block: task prompt kept once across a tool loop's compactions
- `94f5717c4` summary with reasoning off (`GOOSE_COMPACTION_SUMMARY_THINKING`)
- `46167cf1f` tool-call-answered summary retried once with tool_choice none (`GOOSE_COMPACTION_SUMMARY_TOOL_CALL_RETRY`)
- `e63888571` the working set (`GOOSE_COMPACTION_WORKING_SET_SHARE`)
- `8f55b8afd` stage 1 mask (`GOOSE_COMPACTION_MASK_FIRST`, `GOOSE_COMPACTION_DROP_REASONING`)
- `2d0929474` stage 2 verbatim tail for chats mid-loop (`GOOSE_COMPACT_KEEP_TAIL` default)
- review fixes (§9)

## 9. Review fixes (2026-10-16, two independent reviews, both FIX-FIRST)

Each finding was verified before it was fixed. All are fixed in one commit, uncompiled.

1. Reasoning drop deleted the plan. Confirmed: 555 vs 250,387 chars in B_C1. Fix: default off; only
   rendered reasoning is counted as freed; the trace claim is corrected here and in the commit.
2. The working set read for calls that never ran. Confirmed: no result check. Fix: succeeded calls only;
   outside-folder and credential-like files named, never read.
3. Devices and FIFOs hung compaction. Confirmed: `fs::read` on `/dev/urandom` (size 0 passes the bound).
   Fix: `is_file` is required; the read goes through `take`.
4. Heredocs. Confirmed: `<<` didn't take its delimiter, and body lines were parsed as commands. Fix: both.
5. No room guard after a summary. Confirmed. Fix: `Room`, with the tail and working set sized under the
   ceiling and a loud event.
6. Stubs were re-masked. Fix: guard.
7. The stub told the model to re-read. Fix: facts only.
8. Thinking-off broke Anthropic's cache. Confirmed: anthropic.rs drops signed thinking at Off. Fix: Off is
   the default only where the provider says the prefix holds.
9. `tool_choice` was not sent by most formats. Confirmed. Fix: retry only where it is sent; a REQUEST-BODY
   test.
10. A test didn't bite. Fix: used = 115k, over the ceiling, so only the removal admits stage 1.
11. Bool knobs, the newest turn's thinking with no outputs, and gate-10 markers. Fixed.

Partly disagreed: "Qwen drops reasoning before the last user message". The Qwen3.8 fixture renders ALL
reasoning when `preserve_thinking` is unset. The fix doesn't depend on this, because the measured bound
covers both cases.

## 10. Round 3 (2026-10-16)

Built and tested on Gabee. All three gaps are closed:
1. Wrapper providers now forward the two `Provider` methods, as listed in §5's cloud row. There is a test of
   the per-kind answers.
2. Room guard tests:
   - a 32,768-token window with a 12,000-token head: the ceiling is 19,660, the working set fits under it,
     and the verdict is `Fits`;
   - a 22,000-token head: no working set, and the verdict is `CannotMakeRoom`.
   There are also tests for `knob_bool`'s spellings and for the `Chat` default on a provider that doesn't
   say the prefix holds.
3. A working set still over the room after its one shrink is now dropped with
   `compaction_working_set_dropped`, which is also the verdict. The "Not shown" names count against the
   budget: past it, they are counted in one line instead of listed.

Build trap found on Gabee (macOS 27): proc-macro dylibs stripped by cargo's build-override default fail
dyld with "mis-aligned LINKEDIT string pool". Build with `CARGO_PROFILE_DEV_BUILD_OVERRIDE_STRIP=false`.
