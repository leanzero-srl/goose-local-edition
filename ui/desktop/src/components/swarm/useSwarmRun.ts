import { useEffect, useRef, useState } from 'react';
import type { FormationEvidence, RunPhase } from './formationVisualState';

/**
 * Reads the LIVE swarm run for a working directory (via the `read-swarm-run` IPC) and folds its event
 * stream + per-turn activity digests into one lane PER TASK — the "turn loops" the user inspects. A
 * `goose swarm run` writes <cwd>/.swarm/run-<id>.jsonl (task_dispatched / task_completed / task_retry …)
 * plus <cwd>/.swarm/activity/<task>.json ({tool_calls, errors, recent[], last_text}); this turns that raw
 * data into what each node is DOING right now. Polls so an in-flight run animates.
 */

export type TurnStatus = 'running' | 'done' | 'error';

export interface SwarmCall {
  name: string;
  summary: string;
  ok: boolean | null;
  /** A snippet of what the call produced — test output, a traceback, a printed value. */
  result?: string;
  /** The engine's request id, when a digest carries it; the key an `inflight` row is deduplicated by. */
  id?: string;
}

/** One tool request whose result has not landed — written by the engine at the REQUEST moment and removed
 *  when the result arrives (`build_worker_digest`, swarm.rs). `args` is a bounded preview of what the call
 *  is about ("write app/cli.py (83 lines, 2100 bytes)"), never the content; `since` is RFC 3339 UTC. */
export interface InflightCall {
  id: string;
  tool: string;
  args: string;
  since: string;
}

// What a tool call MEANS — so the UI stops rendering every ok:false as a scary red "failure". The load-bearing
// distinction: a MALFORMED call (bad tool arguments) is a genuine slip Goose retries; an APP-ERROR (the tool ran
// fine but the COMMAND it invoked exited non-zero / a test failed / a traceback) is the worker PRODUCTIVELY
// running + testing the app — finding a failing test is the job, not a failure. Mirrors the backend taxonomy.
// 'ran-nothing' is the LYING-GREEN case: the reported exit is 0 but the OUTPUT proves nothing ran (a `| head`
// pipe swallowed pytest's failure exit while it printed "no tests ran") — never allowed to render plain green.
export type CallKind = 'ok' | 'app-error' | 'ran-nothing' | 'malformed' | 'pending';

export interface CallMeaning {
  kind: CallKind;
  /** tool-type bucket for the icon */
  icon: 'terminal' | 'test' | 'build' | 'run' | 'write' | 'edit' | 'read' | 'search' | 'tool';
  /** plain-English intent, e.g. "Ran the tests", "Wrote src/store.rs" */
  action: string;
  /** plain-English outcome, e.g. "3 tests failed — iterating", "malformed — missing the path arg, retried" */
  outcome: string;
}

const MALFORMED_SIGNS = [
  'missing field',
  'failed to parse',
  'unknown variant',
  'no such tool',
  'invalid type:',
  'expected `',
  'invalid arguments',
  'unexpected argument',
];

// Output signatures proving a "successful" shell call actually ran NOTHING. MEASURED live (2026-08-17): a
// worker ran `python3 -m pytest test_meridian.py -v 2>&1 | head -80`; the file did not exist, pytest printed
// "ERROR: file or directory not found … no tests ran in 0.00s" — but the `| head` pipe made the exit code 0
// and the panel painted the call green. Twice. The exit code lies through a pipe; the output does not.
const RAN_NOTHING_SIGNS: RegExp[] = [
  /no tests ran/i,
  /collected 0 items/i,
  /file or directory not found/i,
  /no such file or directory/i,
  /command not found/i,
  /command exited with code\s*:?\s*[1-9]\d*/i,
];

/** Does this call's OUTPUT prove nothing ran, regardless of the reported exit status? Pure + exported so the
 *  lying-green detection is unit-testable against the verbatim measured output. */
export function ranNothing(result: string | undefined | null): boolean {
  if (!result) return false;
  return RAN_NOTHING_SIGNS.some((re) => re.test(result));
}

/** Best-effort filename out of a call summary (write/edit/read args or a path in a shell command). */
function pathHint(summary: string): string {
  const m = summary.match(
    /(?:^|[\s"'`(=])((?:[\w.-]+\/)*[\w.-]+\.(?:py|ts|tsx|js|rs|go|toml|json|md|txt|cfg|ya?ml))/
  );
  return m ? m[1] : '';
}

/** Resolve a filesystem path referenced by an activity line to an ABSOLUTE path for "Reveal in Finder".
 *  Prefers a full absolute path already in the text (activity lines carry them, e.g. "Wrote a file /Users/…/x.ts");
 *  else a relative path (via pathHint) resolved against the run's cwd. Returns null when no path is found. */
export function resolveActivityPath(
  text: string | undefined,
  workingDir: string | undefined
): string | null {
  if (!text) return null;
  const abs = text.match(/(\/(?:[\w.-]+\/)*[\w.-]+\.[\w]+)/);
  if (abs) return abs[1];
  const rel = pathHint(text);
  if (!rel || !workingDir) return null;
  return `${workingDir.replace(/\/+$/, '')}/${rel}`;
}

/** Classify + humanize a single tool call so the panel can explain what it MEANS, not just color it red. */
export function classifyCall(call: SwarmCall): CallMeaning {
  const name = (call.name || '').replace(/^developer__/, '').toLowerCase();
  const sum = call.summary || '';
  const low = sum.toLowerCase();
  const res = (call.result || '').toLowerCase();
  const file = pathHint(sum);

  // ---- tool-type bucket + intent verb ----
  let icon: CallMeaning['icon'] = 'tool';
  let action = name || 'tool call';
  if (name === 'write') {
    icon = 'write';
    action = file ? `Wrote ${file}` : 'Wrote a file';
  } else if (name === 'edit' || name === 'str_replace' || name === 'text_editor') {
    icon = 'edit';
    action = file ? `Edited ${file}` : 'Edited a file';
  } else if (name === 'read' || name === 'view') {
    icon = 'read';
    action = file ? `Read ${file}` : 'Read a file';
  } else if (name === 'shell' || name === 'bash') {
    if (/\b(pytest|cargo test|npm test|jest|vitest|go test|unittest|-m pytest)\b/.test(low)) {
      icon = 'test';
      action = 'Ran the tests';
    } else if (
      /\b(cargo build|tsc|npm run build|npm run make|go build|make\b|pnpm build)\b/.test(low)
    ) {
      icon = 'build';
      action = 'Built the project';
    } else if (/--help|python3? -m |node (dist|build)|cargo run|\.\/|bin\//.test(low)) {
      icon = 'run';
      action = 'Ran the program';
    } else if (/^\s*(cat|head|tail|less|more)\b/.test(low)) {
      icon = 'read';
      action = file ? `Read ${file}` : 'Read a file';
    } else if (/^\s*(ls|tree|find)\b/.test(low)) {
      icon = 'search';
      action = 'Listed files';
    } else if (/^\s*(grep|rg|ag|ack)\b/.test(low)) {
      icon = 'search';
      action = 'Searched the code';
    } else if (/^\s*(sed|wc|awk|diff)\b/.test(low)) {
      icon = 'search';
      action = 'Inspected files';
    } else {
      icon = 'terminal';
      action = 'Ran a shell command';
    }
  }

  // ---- kind + outcome ----
  if (call.ok === null || call.ok === undefined) {
    return { kind: 'pending', icon, action, outcome: 'running…' };
  }
  if (call.ok === true) {
    // Exit 0 is not proof of work: a `| head` pipe reports the pipe's exit, not the command's. When the
    // output itself says nothing ran, the row must never read plain green.
    if (ranNothing(call.result)) {
      return {
        kind: 'ran-nothing',
        icon,
        action,
        outcome: 'exit 0, but the output shows nothing ran',
      };
    }
    return { kind: 'ok', icon, action, outcome: 'done' };
  }
  // ok === false — is it a genuine tool-format slip, or the app reporting something while being tested?
  if (MALFORMED_SIGNS.some((s) => res.includes(s))) {
    const why = res.includes('missing field')
      ? 'missing a required argument'
      : res.includes('no such tool')
        ? 'called a tool that does not exist'
        : 'the arguments did not parse';
    return { kind: 'malformed', icon, action, outcome: `malformed call — ${why}; Goose retries` };
  }
  // productive app-error — the command ran and reported an issue (the worker is testing/iterating)
  let outcome = 'the command reported an error — the worker is iterating';
  if (/traceback|panic|exception|\bthrow\b/.test(res))
    outcome = 'hit a runtime error while testing — iterating';
  else if (/\bfailed\b|assert|\d+ failed|test result: FAILED|error\[/i.test(res))
    outcome = 'found a failing test/check — iterating';
  else if (/no such file|not found|cannot find|does not exist/.test(res))
    outcome = 'a referenced file/target is not there yet';
  else if (/already exists/.test(res)) outcome = 'the target already existed';
  else if (/permission denied/.test(res)) outcome = 'a permission error';
  return { kind: 'app-error', icon, action, outcome };
}

/** Roll a lane's calls into honest counts. The five buckets are DISJOINT and sum to calls.length —
 *  'ran nothing' is the LYING-GREEN case and gets its own number rather than hiding inside app-errors,
 *  which is precisely the folding that let a header count one thing while a body showed another. */
export function callTallies(calls: SwarmCall[]): {
  ok: number;
  appError: number;
  ranNothing: number;
  malformed: number;
  pending: number;
} {
  let ok = 0;
  let appError = 0;
  let ranNothing = 0;
  let malformed = 0;
  let pending = 0;
  for (const c of calls) {
    const k = classifyCall(c).kind;
    if (k === 'ok') ok++;
    else if (k === 'malformed') malformed++;
    else if (k === 'ran-nothing') ranNothing++;
    else if (k === 'app-error') appError++;
    else if (k === 'pending') pending++;
  }
  return { ok, appError, ranNothing, malformed, pending };
}

/** The one call a reader must not have to hunt for. An APP-ERROR is deliberately not it: the command ran
 *  and reported something while the worker tests, and auto-opening twenty of those buries the lane. */
export function firstCallNeedingAttention(calls: SwarmCall[]): number {
  return calls.findIndex((c) => {
    const k = classifyCall(c).kind;
    return k === 'malformed' || k === 'ran-nothing';
  });
}

export interface CallRowMeta {
  key: string;
  ordinal: number | null;
}

/** WHERE EACH SHOWN CALL SITS IN THE LANE'S WHOLE HISTORY, and a key that survives the window sliding.
 *  The engine sends the LAST 60 resolved records plus every in-flight one, so array index is not position.
 *  A pending call has NO ordinal: `tool_calls` counts only resolved records, so it does not have a number yet. */
export function callRowMeta(calls: SwarmCall[], toolCalls?: number): CallRowMeta[] {
  // `!= null` covers both null and a digest that omitted the field entirely — a pending call is any call
  // the engine has not resolved yet, and `tool_calls` does not count it.
  const isResolved = (c: SwarmCall) => c.ok != null;
  const resolved = calls.filter(isResolved).length;
  const total = Math.max(toolCalls ?? resolved, resolved);
  const base = total - resolved;
  let n = 0;
  const seenPending = new Map<string, number>();
  return calls.map((c) => {
    if (isResolved(c)) {
      n += 1;
      const ordinal = base + n;
      return { key: `#${ordinal}`, ordinal };
    }
    // A pending call has no stable identity from the engine: it arrives out of a HashMap, so its position
    // in `calls` can change between polls. Key it by what it IS, plus a duplicate counter.
    const sig = `${c.name ?? ''}|${c.summary ?? ''}`;
    const dupIndex = seenPending.get(sig) ?? 0;
    seenPending.set(sig, dupIndex + 1);
    return { key: `pending:${dupIndex}:${c.name ?? ''}:${c.summary ?? ''}`, ordinal: null };
  });
}

/** WHAT THE WORK PANE'S HEADER IS ALLOWED TO SAY. One rule, three call sites.
 *  `tool_calls` counts resolved records; `calls` is the last 60 of those PLUS the in-flight ones, so a lane
 *  with 3 resolved + 1 pending would otherwise report "last 4 of 3". `total` is clamped for exactly that. */
export function workCaption(
  shown: number,
  engineTotal: number | undefined,
  t: ReturnType<typeof callTallies>
): string {
  if (shown === 0) return 'no tool calls yet';
  const total = Math.max(engineTotal ?? shown, shown);
  const head =
    shown === total
      ? `${total} tool call${total === 1 ? '' : 's'}`
      : `last ${shown} of ${total} tool calls`;
  const parts = [
    t.ok ? `${t.ok} ok` : '',
    t.appError ? `${t.appError} app output` : '',
    t.ranNothing ? `${t.ranNothing} ran nothing` : '',
    t.malformed ? `${t.malformed} retried` : '',
    t.pending ? `${t.pending} running` : '',
  ].filter(Boolean);
  return [head, ...parts].join(' · ');
}

/**
 * THE WORK PANE'S ROWS: the calls that finished, and the requests still running, with no call in both.
 *
 * The engine writes the pending set twice into one digest — as provisional `ok: null` rows in `calls`
 * (older panels render those as "running…") and as `inflight` rows carrying the id, an argument preview
 * and the request time. Rendering both lists a running write twice. When a digest carries `inflight`,
 * that array IS the running set, so the provisional rows are dropped, and any completed row that still
 * names an in-flight id is dropped too. A digest from an engine without the key keeps the old shape.
 * When a result lands the engine removes the inflight row and the finished record is already in `calls`,
 * so the running row drops and the completed row takes its place — one row per call, before and after.
 */
export function workRows(
  calls: SwarmCall[] | undefined,
  inflight: InflightCall[] | undefined
): { completed: SwarmCall[]; running: InflightCall[]; tallies: ReturnType<typeof callTallies> } {
  const all = calls ?? [];
  if (!inflight) {
    return { completed: all, running: [], tallies: callTallies(all) };
  }
  const runningIds = new Set(inflight.map((c) => c.id));
  const completed = all.filter((c) => c.ok != null && !(c.id && runningIds.has(c.id)));
  const t = callTallies(completed);
  return { completed, running: inflight, tallies: { ...t, pending: t.pending + inflight.length } };
}

/** "12s" / "1m 05s" since an RFC 3339 stamp; empty when the stamp does not parse. */
export function elapsedSince(since: string, now: number): string {
  const t = Date.parse(since);
  if (!Number.isFinite(t)) return '';
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  return `${m}m ${String(r).padStart(2, '0')}s`;
}

export interface TurnLane {
  taskId: string;
  /** The architect's one-line human description of the subtask (e.g. "Tokenize the template source") — the
   *  readable name the panel shows, so a lane isn't just the terse id "lexer". */
  description?: string;
  device: string;
  model?: string;
  status: TurnStatus;
  lastText?: string;
  recent?: string[];
  reasoning?: string;
  /** The worker's full narration (all substantive text chunks) — the "reasoning in plain" the panel shows. */
  fullReasoning?: string;
  calls?: SwarmCall[];
  /** The tool requests still running — see InflightCall. A `calls` record appears only once its result
   *  lands, so without this a long write or shell command is invisible for its whole duration. */
  inflight?: InflightCall[];
  toolCalls?: number;
  /** The JUDGE'S OWN estimate of how many more minutes this call needs, from the `ETA=<n>m` token it is
   *  asked for on every look. It is the only estimate anyone has that is based on reading the work: the
   *  judge has seen what the call established, what it is doing now and how fast it is producing.
   *  The panel's run-level "min left" is arithmetic — elapsed / items_done x remaining — and ignored this
   *  entirely, so a model judgement was being computed over rather than shown. */
  judgeEtaMins?: number;
  /** Reasoning-channel activity — a node drafting in the <think> channel has thinking but empty text. Used so
   *  a heavily-generating node counts as WORKING and its thinking previews inline instead of reading "idle". */
  thinkingChars?: number;
  /** The WHOLE reasoning channel from `<task>.think.log` — the digest only keeps a 2,400-char window. */
  fullThinking?: string;
  /// The TRUE size of `<task>.think.log` on disk. main.ts reads only its last 400,000 bytes, so without
  /// this a CLIPPED thinking pane is indistinguishable from a complete one — the exact bug
  /// `transcriptBytes` was added to fix on the other channel, left open on this one.
  thinkingBytes?: number;
  /// The durable `<task>.log` — every chunk of the ANSWER channel, appended, with no clip. `lastText` is
  /// the digest's ROLLING view of the same stream, which is why the OUTPUT pane appeared to scroll away
  /// its own beginning. main.ts has been supplying this all along and nothing read it.
  fullTranscript?: string;
  /// The TRUE size of `<task>.log` on disk. main.ts has attached this to every digest all along and
  /// nothing read it, so a pane could show a clipped tail with no way to say how much was dropped.
  transcriptBytes?: number;
  /// main.ts's OWN answer to "is `fullTranscript` only the tail?" — it compares the file size against the
  /// byte budget it read with, which is the only place both numbers exist. The panel must never re-derive
  /// this by comparing bytes against a rendered string's length: that compares bytes to UTF-16 units.
  transcriptClipped?: boolean;
  /// True while an omni-judge probe is in flight for this lane, written into the digest by the ENGINE
  /// (swarm.rs:15981), not attached by main.ts.
  ///
  /// It no longer means the lane is frozen. The engine used to BUFFER the worker's stream during a probe,
  /// so every counter stopped at the value the look recorded and a judged lane was indistinguishable from
  /// a dead one; it now processes each event where it arrives, so counters, transcripts and recurrence
  /// fingerprints all keep advancing while a probe runs. `judging` survives as the honest label for "a
  /// supervisor is reading this call" — useful context, no longer an explanation for frozen numbers.
  ///
  /// `queuedChunks` went with the buffering. Nothing queues any more, the engine stopped writing the
  /// field, and a type that outlives its producer is a badge that can never render.
  judging?: boolean;
  lastThinking?: string;
  /** "processing" while the node is prompt-processing (dispatched, no tokens yet) — shown before generation. */
  phase?: string;
  /** Which stream the LIVE LINE reads: the one that advanced most recently. A lane key reused call after
   *  call (REVIEW, every round) keeps its previous answer in `<task>.log` while the new call is still
   *  thinking, so a fixed transcript-first order shows the OLD answer for the whole of the new call. */
  liveChannel?: LiveChannel;
  errors?: number;
  elapsedMs?: number;
  /** How many attempts the task took (from task_completed) — surfaced in the status tooltip. */
  attempts?: number;
  /** The last retry's failure text, so a failed/interrupted lane can say WHY (was silently dropped before). */
  error?: string;
  /** SAID provenance — see the Digest fields of the same names. `attempt` is the CURRENT attempt
   *  (task_dispatched / the digest), distinct from `attempts` above (the terminal count). */
  attempt?: number;
  dispatchedAt?: string;
  saidAt?: string | null;
  saidKind?: SaidKind;
  superseded?: SupersededSaid[];
  /** Forming tool calls (II-11b) — see Digest.forming. Never carried from prev: an absent sidecar
   *  MEANS nothing is forming now, and a carried row would outlive its own call. */
  forming?: FormingCall[];
  seq: number;
}

/**
 * Has this row's WORK finished — regardless of whether it has been VERIFIED?
 *
 * The two are deliberately different. A completed build task is 'unverified': the worker returned and the
 * file passed a syntax gate, but the app was never RUN, and only Verify's green e2e may promote it to 'done'.
 * That rule is engine-truth and stays.
 *
 * The progress COUNTER is a different question — "how far along is this?" — and it used to answer it with
 * `state === 'done'`, i.e. with the verification status. So it read 0 for every task that had actually
 * finished, and the only Build row born 'done' is the `Re-planned +N tasks` bookkeeping row, which made the
 * numerator literally count REPLANS.
 *
 * MEASURED (loop-ab-baseline): six tasks completed 16:56:37 → 17:08:52 while the panel showed "Build 0/7".
 * It first moved at 17:09:46 — when `replanned` fired. The one time the number moved, work had been ADDED.
 */
export function isFinishedWork(state: TodoState): boolean {
  return state === 'done' || state === 'unverified';
}

export interface SwarmRunTotals {
  tasks: number;
  running: number;
  done: number;
  failed: number;
}

// A per-phase TODO checklist, derived ENTIRELY from the engine's deterministic event stream (scheduler task
// states + orchestrator phase events) — never from a model self-reporting. The honesty hinges on TodoState:
// a built task is 'unverified' (the worker's loop returned + passed a syntax gate; the app was NOT run), and
// only Verify's complete_result.passed&&verified earns a green 'done'. Advisory items (LLM reviewers) are
// info, never checks. See the phase-todo design workflow (2026-07-15).
// The engine's pipeline, one key per phase it actually runs: OPEN (balanced semantic slices) -> ASK (only
// when the opener names open decisions) -> SYNTHESIS (wire the slices into a task DAG) -> REVIEW (one round
// of structural patches) -> BUILD -> INTEGRATE -> REPAIR. `research` and `contracts` are RETIRED keys:
// P1-5 deleted the RESEARCH fan and P1-4 deleted CONTRACTS from the engine, but archived run.jsonl files
// still carry their phase events, so the keys stay for the historical rows those runs render — a NEW run
// must never be offered either as a pending stage (see RETIRED_PHASES in formationVisualState).
export type PhaseKey =
  | 'open'
  | 'ask'
  | 'research'
  | 'synthesis'
  | 'review'
  | 'contracts'
  | 'build'
  | 'integrate'
  | 'repair'
  | 'done';
export type TodoState =
  | 'pending'
  | 'running'
  | 'done' // class-A VERIFIED complete (green)
  | 'unverified' // built/shipped but NOT proven to run — neutral, never green
  | 'failed' // engine hard-fail or terminal block
  | 'judge_failed' // a task the JUDGE LLM decided to fail (not the engine)
  | 'blocked' // never dispatched but doomed (cascade dep-fail / scheduler stuck)
  | 'skipped' // phase legitimately no-op'd (research off / gate off)
  | 'advisory'; // LLM-reviewer / heuristic signal — info only, never a checkbox
export interface PhaseTodoItem {
  id: string;
  label: string;
  state: TodoState;
  detail?: string;
  device?: string;
  advisory?: boolean;
  // --- rich detail for the expandable Build rows (title + summary collapsed; the rest on expand) ---
  /** Short one-line human description shown next to the title (collapsed). */
  summary?: string;
  /** The FULL task description — shown only on expand. */
  description?: string;
  /** Owned files — shown on expand. */
  files?: string[];
  /** WHY the judge intervened on this task (verdict = diagnosis, hint = the note it gave the worker). Expand. */
  judge?: { verdict: string; hint: string; action: string };
}
export interface PhaseTodo {
  key: PhaseKey;
  label: string;
  items: PhaseTodoItem[];
  state: TodoState;
  active: boolean;
  counts: { done: number; total: number };
}

export type ActivityKind =
  | 'note'
  | 'judge-act'
  | 'phase'
  | 'plan'
  | 'dispatch'
  | 'done'
  | 'fail'
  | 'retry'
  | 'retarget'
  | 'review'
  | 'judge'
  | 'prereview'
  | 'smoke'
  | 'brief'
  | 'config';
export type ActivityTone = 'info' | 'good' | 'warn' | 'bad';
export interface ActivityItem {
  kind: ActivityKind;
  text: string;
  sub?: string;
  tone?: ActivityTone;
  seq: number;
}

export interface PlanTask {
  id: string;
  description?: string;
  files: string[];
  deps: string[];
  difficulty: string;
}
export interface RunMeta {
  prompt: string;
  plannerModel: string;
  endpoint: string;
  nodes: string[];
  gates: boolean;
}
export interface SmokeResult {
  ran: boolean;
  testsPass: boolean | null;
  entryOk: boolean | null;
  pyFiles: number;
}
/** Per-node aggregate for the completion summary — how much each fleet node actually did. */
export interface DeviceStat {
  node: string;
  device: string;
  dispatched: number;
  toolCalls: number;
  busyMs: number;
}
/** The end-of-run tally from run_finished — done/failed task counts, total wall-clock minutes, and the
 *  per-node breakdown. Present only after a CLEAN finish; a run that died without run_finished has none (the
 *  panel falls back to counting lanes + wall time from startedAt). */
export interface RunSummary {
  done: number;
  failed: number;
  totalMin: number | null;
  perDevice: DeviceStat[];
}

/** The full plan-confidence breakdown behind the single number, so the panel can show WHICH signal is low
 *  (agreement = do the drafts agree on structure; spec-clarity = is the product pinned down) and what would
 *  raise it. Parsed from the additive `plan_confidence_breakdown` event key; null on runs that predate it. */
export interface ConfidenceBreakdown {
  final: number;
  agreement: number;
  agreementReason: string;
  specClarity: number;
  specClarityReason: string;
  productSpecified: boolean;
  openDecisions: string[];
}

// End-of-run overview (shown at DONE). features/engage/next are a GROUNDED, scrubbed summary from the model;
// runCommand is engine-stamped (never the model). VERIFICATION is NOT here — the panel re-derives it from
// phaseTodo (engine-only) so no model string can reach the honesty surface.
//
// `generated` ONLY says whether the model wrote the summary PROSE. It says NOTHING about whether the build
// runs. MEASURED: a run finished 7/7 tasks, 0 failed, with complete_result{passed:true, verified:true},
// review{findings:[]} and run_overview{generated:false, run_command_verified:TRUE} — and the panel showed a
// RED "This build did not reach a runnable, verified state". The engine said the app runs; the UI called it
// failed because the summarizer stayed quiet. That is a FALSE RED — the same sin as a false green, and it
// breaks the one rule this panel exists for: only a deterministic engine event may create or kill a verdict.
// Runnability comes from phaseTodo's v-e2e check and runCommandVerified. Never from `generated`.
export interface RunOverview {
  generated: boolean;
  runCommand: string | null;
  runCommandLang: string | null;
  runCommandVerified: boolean;
  features: string[];
  engage: string | null;
  next: string[];
}

/** The OPEN phase's cut of the request, plus what RESEARCH produced from it. `weights` are the opener's own
 *  effort estimates (a lopsided cut is the shape that leaves a node idle), `briefChars` the size of the spec
 *  each slice owner wrote back. */
export interface SliceFan {
  ids: string[];
  weights: number[];
  openSecs: number | null;
  briefChars: number[];
  researchSecs: number | null;
}

/** The clarify PROXY: a question is always answered, by the user or — after a wait, or instantly on an
 *  unattended run — by goose itself from the spec. Both halves are surfaced so a run that answered its own
 *  questions never looks like a run the user steered. */
export interface ClarifyProxy {
  armed: { mode: 'immediate' | 'after_wait'; waitSecs: number; questions: number } | null;
  answered: { questions: string[]; answers: string[] } | null;
  /** The proxy call itself failed; the engine unblocked the run with a conventional default. */
  failed: string | null;
  /** P1-5's proxyless engine: the ask window expired unanswered (`low_confidence_ask_timeout`),
   *  the decisions were folded into every brief as "choose the most conventional option", and the
   *  build CONTINUED. Without this the card said "paused, waiting for you" for the rest of the
   *  run — Mihai hit exactly that on r4-relaunch (2026-08-30): the run was mid-REVIEW while the
   *  UI begged for answers no reader would ever collect. */
  timedOut: { questions: number; waitedSecs: number } | null;
}

/** One REVIEW round: what it found, how much of that was a repeat, and what its patch actually touched.
 *  `patchTouches` is the honest measure — findings without a patch are commentary, and the engine settles
 *  the loop on exactly that distinction. */
export interface ReviewRound {
  round: number;
  fresh: number;
  repeated: number;
  findings: string[];
  patchTouches: number;
  patch: { replace: number; add: number; remove: number } | null;
  rejected: string | null;
}

export interface SwarmRunState {
  present: boolean;
  runId: string | null;
  /** Where the run actually lives. Differs from the session working dir when the engine redirected the
   *  build (it refuses to treat $HOME as an app tree). Anything writing BACK into the run — pause, notes,
   *  activity file paths — must use this, or it writes where the engine never looks. */
  runDir: string | null;
  lanes: TurnLane[];
  /** PLAN-phase generation lanes (architect drafts) — what each model produced while decomposing the app.
   *  Separate from `lanes` (build tasks) and excluded from `totals`. */
  planLanes: TurnLane[];
  /** RESEARCH (scout-<lens>) + CONTRACTS (contract-<id>) per-node lanes — surfaced in developer mode so those
   *  phases show live per-node activity instead of a spinner. */
  scoutLanes: TurnLane[];
  contractLanes: TurnLane[];
  detailLanes: TurnLane[];
  /** RESEARCH per-slice lanes (slice-<id>) — one per node, each owner writing its module's spec. Without
   *  these the RESEARCH phase renders empty lanes while the whole fleet is generating. */
  sliceLanes: TurnLane[];
  /** The single-node planning calls that own no slice: open / open-resplit / synthesis / review /
   *  proxy-answer / rate. Each writes its own digest, so the node running one reads WORKING, not idle. */
  planningLanes: TurnLane[];
  /** Verify REPAIR-WAVE twin lanes (complete_fix_dispatched/…completed) — the fix work runs OUTSIDE the
   *  task_dispatched lifecycle, so without these a node grinding a 10-18 min fix twin read "idle". */
  fixLanes: TurnLane[];
  /** The RESOLVED fleet as canonical node names (pool_resolved, falling back to run_started.pool) — the
   *  honest fleet size. Every pool node renders a fleet row, idle ones included. */
  pool: string[];
  /** OPEN supervision spans (judge generations with no lane) — see foldSupervision. */
  supervision: SupervisionSpan[];
  /** Per-phase TODO checklist, derived entirely from the engine's deterministic events (see buildPhaseTodo). */
  phaseTodo: PhaseTodo[];
  /** End-of-run overview (what built / how to run / next) — null until the run cleanly finishes at DONE. */
  overview: RunOverview | null;
  totals: SwarmRunTotals;
  /** A human-readable timeline of what the swarm is doing — shown even during PLANNING (before any worker
   *  executes), so the user isn't left staring at a blank "working on it". */
  activity: ActivityItem[];
  /** Per-task activity DIGESTS from .swarm/activity/<task>.json (tool_calls, thinking_chars, reasoning/full
   *  reasoning, model, per-tool call breakdown), keyed by task id. Powers the per-task "live generation" detail
   *  in the phase checklist — the answer to "what is the model actually doing / why is it taking so long". */
  activityDigests: Record<string, unknown>;
  /** Per-digest file mtimes (ms), same keys as activityDigests — the per-node realtime signal deriveFleet
   *  uses to tell a live open call from a crashed worker's leftover digest. */
  activityMtimes: Record<string, number>;
  /** The FULL timeline — every dispatch, judge verdict + hint, pre-review, completion, smoke result — for
   *  the verbose view. The compact `activity` is a subset of headline phases. */
  verboseActivity: ActivityItem[];
  /** Run header detail (the brief, planner model, nodes, gates) for the verbose view. */
  meta: RunMeta | null;
  /** The planned task graph (files/deps/difficulty per task) for the verbose view. */
  plan: PlanTask[];
  /** End-to-end smoke-test result, once the run reaches verification. */
  smoke: SmokeResult | null;
  /** Friendly current-phase label (Opening / Researching / Building / Repairing / Done…). */
  phase: string;
  /** The ENGINE's own phase key — from its `phase` event and the task lifecycle, never from parsing the
   *  label above. null before the first phase event, and while the run is held. The ribbon renders no
   *  active step for null rather than inventing one. */
  runPhase: RunPhase | null;
  /** Which phases the engine actually emitted, so the ribbon can mark an un-run stage skipped instead of
   *  back-filling a green check for work that never happened. */
  runPhasesObserved: FormationEvidence;
  /** The OPEN cut + what RESEARCH wrote back from it. */
  slices: SliceFan | null;
  /** Who is answering the clarifying questions — the user, or goose from the spec (see ClarifyProxy). */
  proxy: ClarifyProxy;
  /** The REVIEW rounds, with what each patch actually touched. */
  reviewRounds: ReviewRound[];
  /** The plan's join was renamed to the engine's canonical sink id — worth saying once, because every sink
   *  check downstream matches on that id. */
  sinkRenamedFrom: string | null;
  /** Synthesis did not return and the engine fell back to one task per slice: flatter, more serial, but
   *  every module still specified and owned. */
  synthesisFallback: { error: string; tasks: number } | null;
  /** KNOWN ACTIVE BUGS — the MINOR defects the engine shipped green with (complete_result.known_active_bugs,
   *  and defects_rated.minors while the run is still going). These are NOT failures: the run passed, and
   *  these are what is imperfect about what it passed with. Rendered as their own list, never as errors. */
  knownActiveBugs: string[];
  /** Cross-draft-agreement plan confidence (0-100) — how sure the planner was about the decomposition.
   *  null before planning finishes / when not computed. Updates live as the swarm retargets to raise it. */
  planConfidence: number | null;
  /** The full breakdown behind planConfidence (agreement vs spec-clarity + drivers), null on older runs. */
  confidence: ConfidenceBreakdown | null;
  /** The confidence FLOOR this run was held to (from the engine's plan_loaded), or null when no floor is set.
   *  The panel must judge planConfidence against THIS, never a hardcoded band — a fixed band told the user
   *  "Strong — ready to build" about a 73 that was below an 80 floor, had exhausted retarget, proceeded at
   *  the cap, and had asked 3 questions. */
  askFloor: number | null;
  /** Distinct successive confidence values across the run (initial → each retarget → final) for the climb
   *  trail / sparkline. */
  confidenceTrail: number[];
  /** True while a run is underway (started, not finished, and its files are still fresh). */
  inProgress: boolean;
  /** True once a clean run_finished event was seen — distinguishes a real completion from a run that just went
   *  quiet (killed/crashed), which the panel renders as "Stopped" rather than "Done". */
  finished: boolean;
  /** End-of-run tally, present only after a clean finish (see RunSummary). */
  summary: RunSummary | null;
  /** Epoch ms of the run's first event, for computing wall time when there is no clean-finish total. */
  startedAt: number | null;
  /** Set when the planner's confidence is below the ask floor and the swarm is BLOCKED waiting for the user
   *  to answer clarifying questions (via the run panel's clarify prompt, written to answerPath). */
  clarify: {
    pending: boolean;
    questions: Array<{
      question: string;
      options: string[];
      rationale?: string;
      resolves?: string;
    }>;
    planConfidence?: number;
    confidence?: ConfidenceBreakdown | null;
    answerPath: string;
  } | null;
  mtime: number | null;
  /** Epoch ms of the stamp INSIDE .swarm/heartbeat (rewritten every ~5s while running), or null for a run
   *  with no heartbeat file. Lets the panel detect a killed engine in seconds without false "stopped" during
   *  a legitimately long tool call (which leaves task files quiet but keeps the heartbeat ticking). */
  heartbeat: number | null;
  /** The heartbeat file reads `EXITED:<rfc3339>` — the guard's Drop ran, so the engine returned early and
   *  tore itself down. A FROZEN timestamp with no sentinel is the other death: a hard kill, where Drop never
   *  ran. The two demand opposite fixes, which is why the file's CONTENT is read and not just its mtime. */
  heartbeatExited: boolean;
  /** True while the user has REQUESTED a pause (the .swarm/pause sentinel exists) — the optimistic "Pausing…"
   *  signal. A request is not a fact: it flips the moment the button is clicked, before the engine reacts. */
  pauseRequested: boolean;
  /** ENGINE-TRUTH that the scheduler actually reached the hold (last run_paused seen, no later run_unpaused).
   *  Only this earns the word "Held/Paused" on screen — never the sentinel alone. */
  held: boolean;
  loading: boolean;
}

const EMPTY: SwarmRunState = {
  present: false,
  runId: null,
  runDir: null,
  lanes: [],
  planLanes: [],
  scoutLanes: [],
  contractLanes: [],
  detailLanes: [],
  sliceLanes: [],
  planningLanes: [],
  fixLanes: [],
  pool: [],
  supervision: [],
  phaseTodo: [],
  overview: null,
  totals: { tasks: 0, running: 0, done: 0, failed: 0 },
  activity: [],
  activityDigests: {},
  activityMtimes: {},
  verboseActivity: [],
  meta: null,
  plan: [],
  smoke: null,
  phase: '',
  runPhase: null,
  runPhasesObserved: {},
  slices: null,
  proxy: { armed: null, answered: null, failed: null, timedOut: null },
  reviewRounds: [],
  sinkRenamedFrom: null,
  synthesisFallback: null,
  knownActiveBugs: [],
  planConfidence: null,
  confidence: null,
  askFloor: null,
  confidenceTrail: [],
  inProgress: false,
  finished: false,
  summary: null,
  startedAt: null,
  clarify: null,
  mtime: null,
  heartbeat: null,
  heartbeatExited: false,
  pauseRequested: false,
  held: false,
  loading: true,
};

const num = (v: unknown): number | null => (typeof v === 'number' ? v : null);
const str = (v: unknown): string => (typeof v === 'string' ? v : '');
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);

/** Canonical node label: the prefix before the first '-' or '/' of a model id ('gabee-qwen…' -> 'gabee'). */
const shortNode = (s: string): string => s.match(/^([^-/]+)/)?.[1] ?? s;

/**
 * Canonical node labeler: any device or model id -> the node's short name (gabee/mihai/workhorse).
 *
 * The engine names the SAME physical node two ways — a pool/device id ('mac-gabee-qwen3.6-27b-fable-fusi',
 * sometimes truncated to a trailing dash) and a model id ('gabee-qwen3.6-…-mtp'). Guessing a short name off
 * the raw device id is how the feed printed "Fleet: 3 nodes — fusi, fusi, fable" (the last dash-segment of a
 * truncated id). run_started.pool / pool_resolved.devices tie the two ({id, model_id}), and the model id's
 * prefix IS the node name — so every rendered node label goes through this one map. Pure + exported so the
 * mapping is unit-testable against the measured pool shapes.
 */
/**
 * The accumulator behind `nodeLabeler`, so the incremental fold can build the SAME map one event at a
 * time instead of re-scanning the whole log on every 500ms tick.
 *
 * `pool` and `devices` are kept APART rather than merged as they arrive: the resolved pool must override
 * run_started's, which the two `find()`s got for free by reading 'pool' first and 'devices' second. Only
 * the FIRST run_started and the FIRST pool_resolved count — that is `find()` semantics, and the `seen`
 * flags are what preserve it when the events arrive one at a time.
 */
type NodeCanon = {
  pool: Record<string, string>;
  devices: Record<string, string>;
  poolSeen: boolean;
  devicesSeen: boolean;
};

const emptyNodeCanon = (): NodeCanon => ({
  pool: {},
  devices: {},
  poolSeen: false,
  devicesSeen: false,
});

function absorbNodeCanon(canon: NodeCanon, e: Record<string, unknown>): void {
  const type = e['event'];
  const src =
    type === 'run_started' && !canon.poolSeen
      ? 'pool'
      : type === 'pool_resolved' && !canon.devicesSeen
        ? 'devices'
        : null;
  if (!src) return;
  if (src === 'pool') canon.poolSeen = true;
  else canon.devicesSeen = true;
  const into = canon[src];
  for (const p of arr(e[src])) {
    const rec = p as Record<string, unknown>;
    const id = str(rec['id']);
    const modelId = str(rec['model_id']);
    const label = shortNode(modelId) || shortNode(id);
    if (id) into[id] = label;
    if (modelId) into[modelId] = label;
  }
}

const nodeCanonLabeler = (canon: NodeCanon): ((device: string) => string) => {
  const merged = { ...canon.pool, ...canon.devices };
  return (device: string) => merged[device] ?? shortNode(device);
};

export function nodeLabeler(events: Array<Record<string, unknown>>): (device: string) => string {
  const canon = emptyNodeCanon();
  for (const e of events) absorbNodeCanon(canon, e);
  return nodeCanonLabeler(canon);
}

/**
 * The run's RESOLVED fleet, as canonical node names — from `pool_resolved.devices` (the engine's
 * post-push truth), falling back to `run_started.pool` for logs that predate it. This is the honest
 * fleet SIZE: a node that never got a task is still in the pool, and the fleet strip must render it
 * as explicitly idle rather than omit it (measured: a 3-device pool read "FLEET · 2 NODES" because
 * the third was idle at that moment and only lane devices were counted).
 */
export function resolvePool(events: Array<Record<string, unknown>>): string[] {
  const fromList = (list: unknown): string[] =>
    (Array.isArray(list) ? list : [])
      .map((d) => {
        const rec = d as Record<string, unknown>;
        return shortNode(str(rec['model_id'])) || shortNode(str(rec['id']));
      })
      .filter(Boolean);
  const resolved = events.find((e) => e['event'] === 'pool_resolved');
  const fromResolved = fromList(resolved?.['devices']);
  if (fromResolved.length > 0) return Array.from(new Set(fromResolved)).sort();
  const started = events.find((e) => e['event'] === 'run_started');
  return Array.from(new Set(fromList(started?.['pool']))).sort();
}

// The engine ships each task's FULL worker spec as `description` — a wall of markdown ("**Subtask: [id] Do X**
// **Owned files:** … **Implementation spec:** …"). For the todo list + lane headers we want a clean one-liner,
// not that wall truncated. Strip md emphasis/code fences, drop the "Subtask: [id]" lead, cut at the first
// structural section marker (Owned files / Implementation / a blank line), collapse whitespace, cap length.
// Falls back to the id when nothing usable remains.
/// Is this streamed chunk worth showing as a node's live line?
///
/// The coder models stream reasoning in the <think> channel while the TEXT channel emits single-token
/// fragments — "m", ".", " with", "(group". An ungated fallback therefore renders a busy node as one
/// meaningless letter (observed live: a node mid-generation showing just "m"). A fragment is not a
/// sentence; better to fall through to the recent activity than to print it.
export function substantiveChunk(t?: string | null): string {
  return t && t.trim().length >= 8 && /[a-zA-Z]{3,}/.test(t) ? t.trim() : '';
}

/// A looping model emits the SAME line over and over. Showing it verbatim turns the thinking box into a
/// wall of repeats — measured live: a scout's 2000-char thinking tail was one paragraph repeated three
/// times, 15 duplicate lines. Fold each run of identical lines into one and SAY how many times it repeated.
/// This never hides the loop; it makes it legible as a loop, which is the useful fact.
/// Pure + exported so it is unit-testable.
export function collapseRepeats(text: string): string {
  const lines = text.split('\n');
  const out: string[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    let n = 1;
    while (i + n < lines.length && lines[i + n] === line) n += 1;
    if (line.trim().length === 0) {
      // A run of blank lines is SPACING, not repetition — keep it exactly as authored.
      for (let k = 0; k < n; k += 1) out.push(line);
    } else {
      out.push(n > 1 ? `${line}  ⟲ ×${n}` : line);
    }
    i += n;
  }
  // A whole BLOCK can repeat too (the measured case). Detect a repeated tail-block of >= 2 lines.
  const nonEmpty = out.filter((l) => l.trim().length > 0);
  if (nonEmpty.length >= 6) {
    const half = Math.floor(nonEmpty.length / 2);
    for (let size = half; size >= 3; size -= 1) {
      const tail = nonEmpty.slice(-size).join('\n');
      const before = nonEmpty.slice(0, -size).join('\n');
      if (before.endsWith(tail)) {
        let reps = 2;
        let rest = before.slice(0, before.length - tail.length).replace(/\n$/, '');
        while (rest.endsWith(tail)) {
          reps += 1;
          rest = rest.slice(0, rest.length - tail.length).replace(/\n$/, '');
        }
        const head = rest.trim().length > 0 ? `${rest}\n` : '';
        return `${head}${tail}\n\n⟲ the model repeated the block above ${reps}× — it is looping.`;
      }
    }
  }
  return out.join('\n');
}

export function cleanTaskTitle(desc: string | undefined, id: string): string {
  if (!desc) return id;
  // A DESCRIPTION THAT OPENS WITH A MARKDOWN HEADING IS A DOCUMENT, NOT A TITLE.
  //
  // Since the DETAIL fan was deleted, a task's description IS its slice owner's research brief — a
  // multi-thousand-character specification that opens with whatever section heading the research model
  // chose. Stripping the `#` and cutting at the first blank line then yields that heading as the task's
  // "title".
  //
  // MEASURED 2026-08-28, and Mihai has raised it repeatedly: the FLEET strip showed
  //   gabee     -> "Answers to slice questions"
  //   mihai     -> "Questions Answered"
  //   workhorse -> "Answers to slice questions"
  // while those nodes were building `vendor-sync-engine`, `frontend-css-styling` and
  // `frontend-table-interactions`. Three nodes, two identical labels, none of them naming the work —
  // the §0 one-glance test failed inside our own UI. Four of the six loaded tasks opened with
  // "## Answers to slice questions" or "## Questions answered".
  //
  // The id is the honest handle and it always names the work. The brief is still there, in the row's
  // detail and in the node inspector, which is where a specification belongs.
  if (/^\s*#{1,6}\s/.test(desc)) return humanizeTaskId(id);
  // TRIM first: the architect's descriptions start with "\n\n", which would make the "\n\s*\n" cut match at
  // index 0 and get skipped — leaving the whole wall. Strip md emphasis/fences (NOT underscores — they occur
  // in filenames like test_projects.py), then cut at the first section marker or blank line.
  let s = desc
    .replace(/[*`#>]+/g, ' ')
    .replace(/\r/g, '')
    .trim();
  const cut = s.search(
    /\b(Owned files?|Files? owned|Implementation spec|Files?\s*:|Depends on|Acceptance|Contract|Deliverable)\b|\n\s*\n/i
  );
  if (cut >= 0) s = s.slice(0, cut); // cut===0 => nothing before the marker => falls back to id below
  s = s
    .replace(/^\s*Subtask(\s+Spec)?\s*:?\s*/i, '')
    .replace(/^\s*\[[^\]]+\]\s*/, '')
    .replace(/^\s*[—–\-:]\s*/, '')
    .replace(/\s+/g, ' ')
    .trim();
  if (!s) return id;
  return s.length > 120 ? s.slice(0, 117).trimEnd() + '…' : s;
}

/** A clean TITLE from a task id: "store-hash-validation" -> "Store hash validation". Stable handle, always
 *  readable — unlike the description, which can be a wall of markdown or raw code. */
export function humanizeTaskId(id: string): string {
  const s = id.replace(/[-_]+/g, ' ').replace(/\s+/g, ' ').trim();
  if (!s) return id;
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** A SHORT summary line for a task row. Prefers a real prose description ("Shared types, object format, and
 *  store I/O"); if the description is raw code or a bare instruction, falls back to the owned files, so the
 *  row is never a wall. Returns '' when nothing useful remains (the title alone then stands). */
export function taskSummary(desc: string | undefined, id: string, files?: string[]): string {
  const cleaned = cleanTaskTitle(desc, id);
  let s = cleaned === id ? '' : cleaned;
  // drop a leading "<id> — " / "<id>: " so we don't repeat the title
  const idEsc = id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  s = s.replace(new RegExp(`^${idEsc}\\s*[—:\\-]\\s*`, 'i'), '').trim();
  const looksCode =
    /^(rust\b|use\s|pub\s|fn\s|def\s|class\s|import\s|const\s|let\s|in\ssrc|#!|mod\s|\/\/|package\s|\{)/i.test(
      s
    ) || /(::|=>|\{\s|\}\s|;\s*$)/.test(s.slice(0, 40));
  if (s && !looksCode) return s.length > 90 ? s.slice(0, 87).trimEnd() + '…' : s;
  // fall back to the owned files as context
  const src = (files ?? []).filter((f) => /\.[a-z]+$/i.test(f)).slice(0, 3);
  return src.length ? src.join(', ') : '';
}

/** Parse the additive `plan_confidence_breakdown` object (agreement/spec-clarity split + drivers). Returns
 *  null on older runs where the key is absent — the panel then just shows the scalar. */
function parseConfidence(v: unknown): ConfidenceBreakdown | null {
  if (!v || typeof v !== 'object') return null;
  const o = v as Record<string, unknown>;
  const agreement = num(o['agreement']);
  const specClarity = num(o['spec_clarity']);
  const final =
    num(o['final']) ??
    (agreement != null && specClarity != null ? Math.min(agreement, specClarity) : null);
  if (final == null) return null;
  return {
    final,
    agreement: agreement ?? final,
    agreementReason: str(o['agreement_reason']),
    specClarity: specClarity ?? final,
    specClarityReason: str(o['spec_clarity_reason']),
    productSpecified: o['product_specified'] === true,
    openDecisions: arr(o['open_decisions']).map(String),
  };
}

/** The build prompt is wrapped by goose's <turn-context>…</turn-context> preamble (time, cwd, todo
 *  boilerplate). Strip it so the Brief shows just the user's actual spec. */
function cleanBrief(prompt: string): string {
  return prompt.replace(/^[\s\S]*?<\/turn-context>\s*/i, '').trim() || prompt.trim();
}

/** The judge's verdict maps to a tone: ok is good; a named failure mode (spec_drift/over_reading/looping…)
 *  is a warning the user should see. */
function judgeTone(verdict: string): ActivityTone {
  return verdict === 'ok' || verdict === '' ? 'good' : 'warn';
}

/**
 * EVERY `{"event":"phase","phase":"…"}` value the engine emits, mapped onto a ribbon step.
 *
 * The engine writes eleven of these; this table understood five, and the other six were read and dropped on
 * the floor by `foldRunPhase`. `build` and `repair` survived that by accident — they are re-derived from the
 * task lifecycle below — but CONTRACTS, TEST, RATE and FIX had no representation at all, so the fleet could
 * fan contract stubs across three nodes or grind a fix wave for hours while the ribbon still lit the
 * previous stage. Keep this exhaustive against the `"event": "phase"` sites in swarm.rs.
 *
 * The mappings that are not one-to-one, and why:
 *   repair    the engine names the WHOLE complete loop `repair`, and that loop OPENS by verifying. Only
 *             findings turn it into a repair — the identical rule `complete_verify` already applies below —
 *             so a first-time-green run must not light Repair and claim a stage it never ran.
 *   test      the three-node test fan is verification, not repair, for the same reason.
 *
 * `ask` and `contracts` used to collapse onto Open and Build. CONTRACTS is the whole fleet generating
 * interface stubs for minutes, and it rendered as "Build active" with three nodes parked under Build — the
 * owner's words: "Contracts is somehow in build". A phase the engine announces gets its own step.
 */
const ENGINE_PHASE: Record<string, RunPhase> = {
  open: 'open',
  ask: 'ask',
  research: 'research',
  synthesis: 'synthesize',
  review: 'review',
  contracts: 'contracts',
  build: 'build',
  repair: 'integrate',
  test: 'integrate',
  rate: 'repair',
  fix: 'repair',
};

/**
 * The run's CURRENT phase, from the engine's own events — never from parsing a human label.
 *
 * The previous ribbon regex-matched the friendly `phase` string and returned Build for anything it did not
 * recognise, so a Paused run rendered "Build active" while every node was deliberately idle. The engine
 * emits a first-class `phase` event for every stage it runs — ELEVEN of them, not the four planning ones
 * this comment used to claim — and ENGINE_PHASE above maps all of them. The task lifecycle stays as a
 * SECOND, equally deterministic source for Build/Integrate/Repair, because it is finer grained than the
 * once-per-phase event: only `task_dispatched` can tell the sink's integrate-verify from a worker task,
 * and only `complete_verify` knows whether a round found anything. Last write wins, so the answer always
 * reflects the newest thing the engine actually did.
 *
 * `observed` records which phases were seen at all, so the ribbon can mark a stage SKIPPED instead of
 * back-filling a check for work that never ran (Integrate and Repair are both conditional).
 *
 * Pure + exported so the mapping is unit-testable against verbatim event streams.
 */
export function foldRunPhase(events: Array<Record<string, unknown>>): {
  phase: RunPhase | null;
  observed: FormationEvidence;
} {
  let phase: RunPhase | null = null;
  const observed: FormationEvidence = {};
  const enter = (next: RunPhase) => {
    phase = next;
    observed[next] = true;
  };
  for (const e of events) {
    const type = String(e['event'] ?? '');
    switch (type) {
      case 'phase': {
        const next = ENGINE_PHASE[str(e['phase'])];
        if (next) enter(next);
        break;
      }
      case 'plan_loaded':
        enter('build');
        break;
      case 'task_dispatched':
        enter(str(e['task_id']) === 'integrate-verify' ? 'integrate' : 'build');
        break;
      case 'complete_verify':
        // The verify itself is integration work; only findings turn the run into a repair.
        enter((num(e['findings']) ?? 0) > 0 ? 'repair' : 'integrate');
        break;
      case 'defects_rated':
      case 'complete_fix_dispatched':
      case 'complete_fix_completed':
      case 'complete_fix_wave':
      case 'spec_repair_wave':
        enter('repair');
        break;
      case 'run_finished':
        enter('done');
        break;
      default:
        break;
    }
  }
  return { phase, observed };
}

/** Turn the run event stream into TWO timelines — a compact headline feed (phases only) and a VERBOSE feed
 *  (every dispatch, judge verdict + hint, pre-review, completion, smoke) — plus the run header meta, the
 *  planned task graph, and the smoke result. All of this is already in the event stream; nothing is dropped
 *  server-side, so verbose mode is purely a richer read of the same data. */
export function buildActivity(events: Array<Record<string, unknown>>): {
  activity: ActivityItem[];
  verbose: ActivityItem[];
  meta: RunMeta | null;
  plan: PlanTask[];
  smoke: SmokeResult | null;
  phase: string;
  finished: boolean;
  planConfidence: number | null;
  confidence: ConfidenceBreakdown | null;
  askFloor: number | null;
  confidenceTrail: number[];
  summary: RunSummary | null;
  startedAt: number | null;
  overview: RunOverview | null;
  slices: SliceFan | null;
  proxy: ClarifyProxy;
  reviewRounds: ReviewRound[];
  sinkRenamedFrom: string | null;
  synthesisFallback: { error: string; tasks: number } | null;
  knownActiveBugs: string[];
} {
  const feed: ActivityItem[] = [];
  const vfeed: ActivityItem[] = [];
  let phase = 'Starting…';
  // Accumulated separately and assembled at the end: OPEN writes the cut, RESEARCH writes the spec sizes,
  // and the two events are minutes apart.
  let sliceIds: string[] = [];
  let sliceWeights: number[] = [];
  let sliceOpenSecs: number | null = null;
  let sliceBriefChars: number[] = [];
  let sliceResearchSecs: number | null = null;
  const proxy: ClarifyProxy = { armed: null, answered: null, failed: null, timedOut: null };
  const reviewRounds: ReviewRound[] = [];
  let sinkRenamedFrom: string | null = null;
  let synthesisFallback: { error: string; tasks: number } | null = null;
  let knownActiveBugs: string[] = [];
  let finished = false;
  let cseq = 0;
  let vseq = 0;
  let meta: RunMeta | null = null;
  let plan: PlanTask[] = [];
  let planConfidence: number | null = null;
  let confidence: ConfidenceBreakdown | null = null;
  let askFloor: number | null = null;
  const confTrail: number[] = [];
  let smoke: SmokeResult | null = null;
  let summary: RunSummary | null = null;
  let startedAt: number | null = null;
  let overview: RunOverview | null = null;
  const compact = (it: Omit<ActivityItem, 'seq'>) => feed.push({ ...it, seq: cseq++ });
  const verbose = (it: Omit<ActivityItem, 'seq'>) => vfeed.push({ ...it, seq: vseq++ });
  // NODE NAMES, never truncated device-id fragments — the same canonical map foldEvents uses, so the feed's
  // "Fleet: … — gabee, mihai, workhorse" and "on workhorse" match the fleet rows letter for letter.
  const nodeOf = nodeLabeler(events);
  // Push each distinct confidence value onto the trail (initial → retargets → final) and set the live
  // header value; last-write-wins on each poll makes the pill advance without ref plumbing.
  const setConf = (v: number) => {
    if (confTrail[confTrail.length - 1] !== v) confTrail.push(v);
    planConfidence = v;
  };

  for (const e of events) {
    const type = String(e['event'] ?? '');
    // First event with a parseable timestamp anchors the run's start, so the terminal summary can show wall
    // time even for a run that died without a run_finished (no phases.total_min to read).
    if (startedAt == null && typeof e['ts'] === 'string') {
      const ms = Date.parse(e['ts'] as string);
      if (!Number.isNaN(ms)) startedAt = ms;
    }
    switch (type) {
      case 'run_started': {
        const pool = arr(e['pool']).map((d) => {
          const rec = d as Record<string, unknown>;
          // model_id's prefix is the node's real name; the raw id may be truncated ('…-fable-' -> 'fable').
          return shortNode(str(rec['model_id'])) || nodeOf(str(rec['id']));
        });
        // `gates` is now an OBJECT of per-gate booleans, so `!!e['gates']` was always true. Treat gates as on
        // when the assured bundle is on or ANY individual gate is enabled.
        const gatesVal = e['gates'];
        const anyGate =
          typeof gatesVal === 'object' && gatesVal !== null
            ? Object.values(gatesVal as Record<string, unknown>).some((v) => v === true)
            : !!gatesVal;
        meta = {
          prompt: cleanBrief(str(e['prompt'])),
          plannerModel: str(e['planner_model']),
          endpoint: str(e['endpoint']),
          nodes: pool,
          gates: anyGate || !!e['assured'],
        };
        compact({ kind: 'phase', text: 'Starting the build' });
        verbose({ kind: 'phase', text: 'Starting the build' });
        if (meta.prompt) verbose({ kind: 'brief', text: 'Brief', sub: meta.prompt, tone: 'info' });
        verbose({
          kind: 'config',
          text: `Fleet: ${pool.length} node${pool.length === 1 ? '' : 's'}${pool.length ? ' — ' + pool.join(', ') : ''}`,
          sub: `planner ${nodeOf(meta.plannerModel)}${meta.gates ? ' · gates on' : ''}`,
          tone: 'info',
        });
        phase = 'Starting';
        break;
      }
      case 'phase': {
        // The engine's own stage banner. It names the phase; the events below say what happened INSIDE it,
        // so the banner alone is deliberately terse and only sets the header label.
        const label: Record<string, string> = {
          open: 'Opening',
          ask: 'Asking you',
          research: 'Researching',
          synthesis: 'Synthesizing',
          review: 'Reviewing the plan',
          contracts: 'Freezing contracts',
        };
        const p = str(e['phase']);
        if (label[p]) phase = label[p];
        break;
      }
      case 'slices_opened': {
        sliceIds = arr(e['slices']).map(String);
        sliceWeights = arr(e['weights']).map((w) => num(w) ?? 1);
        sliceOpenSecs = num(e['secs']);
        const ids = sliceIds;
        const weights = sliceWeights;
        const n = num(e['count']) ?? ids.length;
        const sub = ids.map((id, i) => `${id} (w${weights[i] ?? 1})`).join(' · ') || undefined;
        compact({ kind: 'plan', text: `Cut into ${n} slice${n === 1 ? '' : 's'}`, sub });
        verbose({
          kind: 'plan',
          text: `Cut into ${n} slice${n === 1 ? '' : 's'}${e['secs'] != null ? ` in ${num(e['secs'])}s` : ''}`,
          sub,
        });
        break;
      }
      case 'clarify_proxy_armed': {
        const mode = str(e['mode']) === 'immediate' ? 'immediate' : 'after_wait';
        const waitSecs = num(e['wait_secs']) ?? 0;
        const questions = num(e['questions']) ?? 0;
        proxy.armed = { mode, waitSecs, questions };
        // A QUESTION IS ALWAYS ANSWERED. Say WHO will answer it before the answer exists, so an unattended
        // run never looks like a run someone steered.
        const text =
          mode === 'immediate'
            ? `Unattended run — goose is answering ${questions} open decision${questions === 1 ? '' : 's'} itself`
            : `Asking you ${questions} open decision${questions === 1 ? '' : 's'} — goose answers in ${Math.round(waitSecs / 60)} min if you don't`;
        compact({ kind: 'plan', text, tone: 'warn' });
        verbose({ kind: 'plan', text, tone: 'warn' });
        break;
      }
      case 'clarify_proxy_answered': {
        const questions = arr(e['questions']).map(String);
        const answers = arr(e['answers']).map(String);
        proxy.answered = { questions, answers };
        const sub =
          questions.map((q, i) => `${i + 1}. ${q}\n   → ${answers[i] ?? ''}`).join('\n') ||
          undefined;
        compact({ kind: 'plan', text: 'Answered by goose — you did not reply', tone: 'warn', sub });
        verbose({ kind: 'plan', text: 'Answered by goose — you did not reply', tone: 'warn', sub });
        break;
      }
      case 'clarify_proxy_failed': {
        proxy.failed = str(e['error']);
        const text =
          'The proxy answer failed — goose took the most conventional option and carried on';
        compact({ kind: 'plan', text, tone: 'warn' });
        verbose({ kind: 'plan', text, tone: 'warn', sub: proxy.failed || undefined });
        break;
      }
      case 'research_completed': {
        const n = num(e['slices']);
        const chars = arr(e['brief_chars']).map((c) => num(c) ?? 0);
        if (n != null || chars.length > 0) {
          // The rewritten engine reports SLICES + spec sizes; the old one reported a findings count. Only
          // the new shape carries `slices`, so the legacy branch below still handles old logs verbatim.
          sliceBriefChars = chars;
          sliceResearchSecs = num(e['secs']);
          const total = chars.reduce((a, b) => a + b, 0);
          const t = `Slice specs written — ${n ?? chars.length} slice${(n ?? chars.length) === 1 ? '' : 's'}, ${total.toLocaleString()} chars of spec`;
          compact({ kind: 'phase', text: t });
          verbose({
            kind: 'phase',
            text: t,
            sub:
              (sliceIds.length === chars.length
                ? sliceIds.map((id, i) => `${id}: ${chars[i]} chars`).join(' · ')
                : chars.join(' · ')) || undefined,
          });
          phase = 'Synthesizing';
          break;
        }
        const t = `Research done — ${num(e['findings']) ?? 0} findings`;
        compact({ kind: 'phase', text: t });
        verbose({ kind: 'phase', text: t });
        phase = 'Planning';
        break;
      }
      case 'synthesis_fallback': {
        synthesisFallback = { error: str(e['error']), tasks: num(e['tasks']) ?? 0 };
        // Not a failure: the fallback IS a valid plan, one task per slice, each carrying its owner's brief.
        // It costs parallelism, never the research already paid for — so this is a warning, not an error.
        const t = `Synthesis didn't return — building one task per slice instead (${synthesisFallback.tasks})`;
        compact({ kind: 'plan', text: t, tone: 'warn' });
        verbose({ kind: 'plan', text: t, tone: 'warn', sub: synthesisFallback.error || undefined });
        break;
      }
      case 'review_findings': {
        const round = num(e['round']) ?? reviewRounds.length + 1;
        const findings = arr(e['findings']).map(String);
        const fresh = num(e['new']) ?? findings.length;
        const repeated = num(e['repeated']) ?? 0;
        const patchTouches = num(e['patch_touches']) ?? 0;
        reviewRounds.push({
          round,
          fresh,
          repeated,
          findings,
          patchTouches,
          patch: null,
          rejected: null,
        });
        // A round that requested NO change has found nothing to fix, whatever prose it wrapped that in —
        // the engine settles the loop on exactly that, so the feed says it in those terms.
        const t =
          patchTouches > 0
            ? `Review round ${round} — ${fresh} finding${fresh === 1 ? '' : 's'}, patching ${patchTouches} task${patchTouches === 1 ? '' : 's'}`
            : `Review round ${round} — settled, no change requested`;
        compact({ kind: 'review', text: t, tone: patchTouches > 0 ? 'warn' : 'good' });
        verbose({
          kind: 'review',
          text: t,
          tone: patchTouches > 0 ? 'warn' : 'good',
          sub:
            [
              findings.map((f, i) => `${i + 1}. ${f}`).join('\n'),
              repeated > 0 ? `(${repeated} repeated from an earlier round)` : '',
            ]
              .filter(Boolean)
              .join('\n') || undefined,
        });
        break;
      }
      case 'plan_patched': {
        const round = num(e['round']) ?? 0;
        const patch = {
          replace: num(e['replace']) ?? 0,
          add: num(e['add']) ?? 0,
          remove: num(e['remove']) ?? 0,
        };
        const target = reviewRounds.find((r) => r.round === round);
        if (target) target.patch = patch;
        verbose({
          kind: 'review',
          text: `Plan patched (round ${round})`,
          sub: `${patch.replace} replaced · ${patch.add} added · ${patch.remove} removed`,
          tone: 'info',
        });
        break;
      }
      case 'plan_repaired': {
        // The deterministic pass (DESIGN-STABILITY-FIRST.md step 1) that fixes what the measured plan flags
        // name, without a model round: it fires ONCE per plan and its before/after is the whole story.
        const actions = Array.isArray(e['actions']) ? (e['actions'] as unknown[]).length : 0;
        const before = (e['before'] ?? {}) as Record<string, unknown>;
        const after = (e['after'] ?? {}) as Record<string, unknown>;
        const count = (o: Record<string, unknown>, k: string) => {
          const v = o[k];
          return Array.isArray(v) ? v.length : (num(v) ?? 0);
        };
        const t = actions === 0 ? 'Plan needed no repair' : `Plan repaired — ${actions} deterministic fix${actions === 1 ? '' : 'es'}`;
        const sub =
          `owning nothing ${count(before, 'tasks_owning_nothing')}→${count(after, 'tasks_owning_nothing')} · ` +
          `shared files ${count(before, 'shared_files')}→${count(after, 'shared_files')} · ` +
          `module/package collisions ${count(before, 'module_package_collisions')}→${count(after, 'module_package_collisions')} · ` +
          `unassigned endpoints ${count(before, 'unassigned_endpoints')}→${count(after, 'unassigned_endpoints')}`;
        compact({ kind: 'plan', text: t, tone: actions === 0 ? 'info' : 'good', sub });
        verbose({ kind: 'plan', text: t, tone: actions === 0 ? 'info' : 'good', sub });
        break;
      }
      case 'plan_patch_rejected': {
        const round = num(e['round']) ?? 0;
        const diagnostic = str(e['diagnostic']);
        const target = reviewRounds.find((r) => r.round === round);
        if (target) target.rejected = diagnostic;
        // A bad patch costs ONE patch: it is dropped, the plan it was meant to fix is untouched, and the run
        // continues. Worth showing, but never as a failure of the run.
        const t = `Review patch rejected (round ${round}) — dropped, plan unchanged`;
        compact({ kind: 'review', text: t, tone: 'warn' });
        verbose({ kind: 'review', text: t, tone: 'warn', sub: diagnostic || undefined });
        break;
      }
      case 'sink_id_pinned': {
        sinkRenamedFrom = str(e['from']);
        verbose({
          kind: 'plan',
          text: `Sink renamed — \`${sinkRenamedFrom}\` → \`${str(e['to'])}\``,
          sub: 'so the engine’s own sink checks keep matching',
          tone: 'info',
        });
        break;
      }
      case 'defects_rated': {
        const critical = num(e['critical']) ?? 0;
        const minor = num(e['minor']) ?? 0;
        const forced = num(e['engine_forced']) ?? 0;
        knownActiveBugs = arr(e['minors']).map(String);
        const t =
          critical === 0
            ? `Every critical defect closed — shipping with ${minor} known active bug${minor === 1 ? '' : 's'}`
            : `${critical} critical defect${critical === 1 ? '' : 's'} remain · ${minor} minor`;
        compact({
          kind: critical === 0 ? 'review' : 'fail',
          text: t,
          tone: critical === 0 ? 'good' : 'bad',
        });
        verbose({
          kind: critical === 0 ? 'review' : 'fail',
          text: t,
          tone: critical === 0 ? 'good' : 'bad',
          sub:
            [
              forced > 0 ? `${forced} forced critical by the engine, not the rater` : '',
              knownActiveBugs.map((m, i) => `${i + 1}. ${m}`).join('\n'),
            ]
              .filter(Boolean)
              .join('\n') || undefined,
        });
        phase = 'Repairing';
        break;
      }
      case 'complete_result': {
        const bugs = arr(e['known_active_bugs']).map(String);
        if (bugs.length > 0) knownActiveBugs = bugs;
        break;
      }
      case 'complete_result_revised': {
        // The engine took its own green back. This is a deterministic finding, not an advisory opinion, so
        // it belongs in the COMPACT feed too — the compact lane is what a user watches, and a run that
        // silently drops from verified to unverified with no line is the exact confusion this event exists
        // to prevent.
        const why = str(e['reason']);
        const mods = arr(e['evidence']).map(String);
        const text =
          why === 'unwired-module-unfixed'
            ? `Not verified — ${mods.length} module${mods.length === 1 ? '' : 's'} built but imported by nothing`
            : `Not verified — the engine retracted its green (${why || 'revised'})`;
        compact({ kind: 'fail', text, tone: 'bad' });
        verbose({ kind: 'fail', text, tone: 'bad', sub: mods.join('\n') || undefined });
        break;
      }
      case 'judge_look':
      case 'judge_nudge': {
        // The judge can no longer kill or move a task — it observes and steers. Say what it ESTABLISHED and
        // what it asked for next; a verdict with no next step is noise on a board that already shows state.
        const established = str(e['established']);
        const next = str(e['next']);
        if (!established && !next) break;
        const delivery = str(e['delivery']);
        const task = str(e['task_id']) || 'a worker';
        const sub = [established && `established: ${established}`, next && `next: ${next}`]
          .filter(Boolean)
          .join('\n');
        // A NUDGE REPEATS ITS OWN LOOK, WORD FOR WORD. The engine emits judge_look then judge_nudge in the
        // same breath carrying the identical `established` and `next`, so rendering both put every action
        // on screen twice and buried the three moments that DID something in a wall of moments that did
        // not. When the pair matches, replace the observation with the action rather than appending to it.
        const prev = vfeed[vfeed.length - 1];
        if (
          type === 'judge_nudge' &&
          prev &&
          (prev.kind === 'judge' || prev.kind === 'judge-act') &&
          prev.sub === sub &&
          prev.text.includes(task)
        ) {
          prev.kind = 'judge-act';
          prev.text = `Judge steered ${task}${delivery ? ` (${delivery})` : ''}`;
          prev.tone = 'warn';
          break;
        }
        verbose({
          kind: type === 'judge_nudge' ? 'judge-act' : 'judge',
          text:
            type === 'judge_nudge'
              ? `Judge steered ${task}${delivery ? ` (${delivery})` : ''}`
              : `Judge looked at ${task}${e['verdict'] ? ` → ${str(e['verdict'])}` : ''}`,
          sub,
          tone: type === 'judge_nudge' ? 'warn' : 'info',
        });
        break;
      }
      // THE ENGINE ENDED A CALL. The most consequential thing the supervisor path can do, and until now
      // it happened with nothing on screen: a lane would simply stop and the phase would move on. An
      // operator needs to see it as it happens, because it is the difference between "a lane finished"
      // and "a lane was given up on".
      //
      // Placed AFTER the judge_nudge case on purpose: `case 'judge_look':` FALLS THROUGH into it, so a
      // case inserted between the two silently steals every look.
      case 'judge_call_ended_unproductive': {
        const endedTask = str(e['task_id']) || 'a call';
        const endedChars = num(e['thinking_chars']);
        // NOT "zero tool calls", and not "never called its output tool". The engine terminator fires on
        // "no tool call SINCE the last nudge" — a call with tool calls earlier in its life is terminable,
        // and this card asserted the opposite about it. `calls_since_last_nudge` is the number the
        // sentence is actually about; archived runs predate the field, so it falls back to '?'.
        const sinceNudge = num(e['calls_since_last_nudge']);
        verbose({
          kind: 'judge-act',
          tone: 'bad',
          text: `Ended ${endedTask} — it owed a structured reply and stopped acting`,
          sub: [
            `${num(e['nudges']) ?? '?'} nudges, ${endedChars === null ? '?' : endedChars.toLocaleString()} reasoning chars, ${sinceNudge === null ? '?' : sinceNudge} tool calls since the last nudge`,
            str(e['reason']),
          ]
            .filter(Boolean)
            .join('\n'),
        });
        break;
      }
      // The same state, on a lane whose caller cannot absorb a lost result. Measured and reported, never
      // acted on — see `may_terminate` in swarm.rs.
      case 'judge_call_end_declined': {
        const declinedTask = str(e['task_id']) || 'a call';
        verbose({
          kind: 'judge',
          tone: 'warn',
          text: `${declinedTask} is out of moves — left running`,
          sub: str(e['reason']) ?? '',
        });
        break;
      }
      // The supervisor repeated itself verbatim: escalation has hit its floor. Worth showing because it is
      // the warning that precedes an ending, and on a lane that recovers it explains a long stretch of
      // nudges that changed nothing.
      case 'judge_out_of_moves': {
        const stuckTask = str(e['task_id']) || 'a call';
        verbose({
          kind: 'judge',
          tone: 'warn',
          text: `Judge out of moves on ${stuckTask}`,
          sub: `same direction repeated after ${num(e['nudges']) ?? '?'} nudges: ${str(e['repeated_direction'])}`,
        });
        break;
      }
      // The judge replaced its own un-delivered notes. Only shown when it actually dropped something,
      // because that is the interesting case: a pure-reasoning call never reaches a turn boundary, so
      // nudges QUEUE, and open-coverage-2 once had fifteen waiting to land at once. A reader seeing many
      // nudges and a still-silent lane needs to know the stale ones are no longer on their way.
      case 'judge_notes_superseded': {
        const droppedN = num(e['dropped']) ?? 0;
        if (droppedN <= 0) break;
        verbose({
          kind: 'judge',
          tone: 'info',
          text: `Replaced ${droppedN} undelivered note${droppedN === 1 ? '' : 's'} to ${str(e['task_id']) || 'a call'}`,
          sub: "the newest direction supersedes the judge's own stale ones, so only it lands",
        });
        break;
      }
      // HOW THE SLICE LIST GROWS. An operator watching a run saw 11 slices at OPEN and 21 at BUILD with
      // nothing in between explaining it -- the coverage loop's whole job is invisible. These four make it
      // legible: what was added, what was kept as coverage rather than work, and when the loop closed.
      //
      // Placed after a case that ends in `break`, never between a bare `case X:` and the one below it --
      // a case inserted into a fall-through pair silently steals every event of the first kind.
      case 'coverage_gap': {
        const added = arr(e['titles']).map(String).filter(Boolean);
        if (!added.length) break;
        verbose({
          kind: 'plan',
          tone: 'warn',
          text: `Coverage found ${added.length} unowned thing${added.length === 1 ? '' : 's'} — adding slices`,
          sub: added.join(', '),
        });
        break;
      }
      // Rows the enumerator declined to turn into work. Shown because the engine used to FABRICATE a slice
      // here from the component's name, which is how a hex colour became a build task.
      case 'coverage_rows_not_work': {
        const names = arr(e['names']).map(String).filter(Boolean);
        if (!names.length) break;
        verbose({
          kind: 'plan',
          tone: 'info',
          text: `Kept ${names.length} row${names.length === 1 ? '' : 's'} as coverage, not work`,
          sub: names.join(', '),
        });
        break;
      }
      case 'coverage_complete': {
        verbose({
          kind: 'plan',
          tone: 'good',
          text: `Coverage settled — every named component has an owner`,
          sub: `${num(e['slices']) ?? '?'} slices`,
        });
        break;
      }
      // The review proposed the same patch and validation refused it the same way twice. Worth surfacing
      // loudly: it means the plan is going to BUILD with the defect the reviewer just described.
      // THE VERIFIER'S FINDINGS. These are FACTS about files on disk -- a missing file, an empty file, a
      // syntax error, an import nobody wrote -- not the judge's opinion about a reasoning tail. They are
      // the highest-signal thing on the board and must never be buried.
      case 'delivery_defects': {
        const defects = arr(e['defects']).map(String).filter(Boolean);
        if (!defects.length) break;
        verbose({
          kind: 'fail',
          tone: 'bad',
          text: `${str(e['task_id']) || 'a task'} finished with ${defects.length} broken deliverable${defects.length === 1 ? '' : 's'}`,
          sub: defects.join('\n'),
        });
        break;
      }
      // Two slices claiming one path, seen at the END of RESEARCH instead of after REVIEW has spent rounds
      // unpicking it.
      case 'brief_defects': {
        const cols = arr(e['collisions']);
        const nofiles = arr(e['slices_declaring_no_files']).map(String);
        const bits: string[] = [];
        for (const c of cols) {
          const o = c as Record<string, unknown>;
          bits.push(`${String(o['file'])} claimed by ${arr(o['slices']).map(String).join(', ')}`);
        }
        if (nofiles.length) bits.push(`declaring no files: ${nofiles.join(', ')}`);
        if (!bits.length) break;
        verbose({
          kind: 'plan',
          tone: 'warn',
          text: `Briefs collide before synthesis even runs`,
          sub: bits.join('\n'),
        });
        break;
      }
      // A nudge the judge WANTED to send and did not, because the call was producing. Shown so the saving
      // is visible rather than assumed -- 33 of 34 such nudges changed nothing and cost 66 minutes.
      case 'judge_drift_held': {
        verbose({
          kind: 'judge',
          tone: 'info',
          text: `Held a drift nudge on ${str(e['task_id']) || 'a call'} — it is producing`,
          sub: `${num(e['produced_since_last_look']) ?? '?'} chars since the last look`,
        });
        break;
      }
      case 'scouts_planned': {
        const lenses = arr(e['lenses']).map(String).join(', ');
        compact({ kind: 'phase', text: 'Planning research', sub: lenses || undefined });
        verbose({ kind: 'phase', text: 'Planning research', sub: lenses || undefined });
        phase = 'Planning research';
        break;
      }
      case 'research_planned':
        verbose({ kind: 'phase', text: 'Researching the problem' });
        phase = 'Researching';
        break;
      case 'pillars':
        verbose({ kind: 'phase', text: 'Defining quality pillars' });
        phase = 'Planning';
        break;
      case 'low_confidence_ask': {
        // Surface the confidence AS SOON AS the swarm asks (before plan_loaded), so the badge is visible at
        // the exact moment it pauses for the user — not only after the re-plan.
        const b = parseConfidence(e['plan_confidence_breakdown']);
        if (b) confidence = b;
        const pc = num(e['plan_confidence']);
        if (typeof pc === 'number') setConf(pc);
        const af = num(e['ask_floor']);
        if (typeof af === 'number') askFloor = af;
        const qs = arr(e['questions']) as Array<Record<string, unknown>>;
        const nq = qs.length;
        // Surface the ACTUAL questions in the feed's `sub` so they PERSIST after the clarify prompt is answered
        // and dismissed — a durable record that goose polled + what it asked (Mihai: "I expect to SEE the
        // questions goose is asking").
        const qsub = qs.map((q, i) => `${i + 1}. ${str(q['question'])}`).join('\n') || undefined;
        compact({
          kind: 'plan',
          text: `Paused — asking you ${nq} question${nq === 1 ? '' : 's'}`,
          tone: 'warn',
          sub: qsub,
        });
        verbose({
          kind: 'plan',
          text: `Low confidence (${typeof pc === 'number' ? pc : '?'}/100) — asking the user ${nq} clarifying question${nq === 1 ? '' : 's'} before building`,
          tone: 'warn',
          sub: qsub,
        });
        break;
      }
      case 'low_confidence_ask_timeout': {
        const nq = num(e['questions_unanswered']) ?? 0;
        const waited = num(e['waited_secs']) ?? 0;
        proxy.timedOut = { questions: nq, waitedSecs: waited };
        const t = `Unanswered at the ${waited}s window — goose chose conventional options; the build continues`;
        compact({ kind: 'plan', text: t, tone: 'info' });
        verbose({
          kind: 'plan',
          text: t,
          tone: 'info',
          sub: 'Each open decision was folded into every worker brief as "choose the most conventional option and note the choice in a code comment".',
        });
        break;
      }
      case 'low_confidence_answered': {
        compact({ kind: 'plan', text: 'Got your answers — re-planning', tone: 'good' });
        verbose({
          kind: 'plan',
          text: 'Answers received — re-planning with your input',
          tone: 'good',
        });
        break;
      }
      case 'confidence_rescored': {
        // The user answered the open decisions; the engine re-scored spec-clarity and the plan confidence
        // climbed (e.g. 30 → 68). WITHOUT this case the rescore was DROPPED and the pill froze at the
        // pre-answer value — which read on screen as "building at 30" when the run was in fact still PLANNING
        // and the real confidence had already risen. Only spec-clarity moves on a rescore (the drafts are
        // unchanged, so agreement is held); final = min(agreement, spec-clarity) = conf_after.
        const after = num(e['conf_after']);
        const clarityAfter = num(e['spec_clarity_after']);
        const clarityBefore = num(e['spec_clarity_before']);
        const answered = num(e['answered']) ?? 0;
        if (confidence && typeof clarityAfter === 'number') {
          const next: ConfidenceBreakdown = { ...confidence, specClarity: clarityAfter };
          next.final = Math.min(next.agreement, next.specClarity);
          confidence = next;
        }
        if (typeof after === 'number') setConf(after);
        const sub =
          clarityBefore != null && clarityAfter != null
            ? `spec clarity ${clarityBefore} → ${clarityAfter}`
            : undefined;
        compact({ kind: 'retarget', text: 'Re-scored after your answers', tone: 'good', sub });
        verbose({
          kind: 'retarget',
          text: `Re-scored with your ${answered} answer${answered === 1 ? '' : 's'}${typeof after === 'number' ? ` — confidence now ${after}/100` : ''}`,
          tone: 'good',
          sub,
        });
        break;
      }
      case 'confidence_retarget': {
        // The swarm is dynamically raising the meter (re-drafting to a consensus / researching the open
        // decisions). conf_after may be null (the new value lands on the next plan step) — either way we log
        // the action; the header pill climbs via setConf on this or the subsequent plan event.
        const before = num(e['conf_before']);
        const after = num(e['conf_after']);
        const signal = str(e['binding_signal']);
        const action = str(e['action']);
        const round = num(e['round']);
        const detail = str(e['detail']);
        if (typeof after === 'number') {
          setConf(after);
          if (confidence) {
            const next: ConfidenceBreakdown = { ...confidence };
            if (signal === 'agreement') next.agreement = after;
            else if (signal === 'spec_clarity') next.specClarity = after;
            next.final = Math.min(next.agreement, next.specClarity);
            confidence = next;
          }
        }
        const label =
          signal === 'spec_clarity'
            ? 'spec clarity'
            : signal === 'agreement'
              ? 'agreement'
              : 'confidence';
        const actionLabel =
          action === 'redraft'
            ? 're-drafting a consensus plan'
            : action === 're_research'
              ? 'researching the open decisions'
              : action === 'proceed_at_cap'
                ? 'proceeding at the round cap'
                : action || 'improving';
        const climbed = before != null && after != null && after > before;
        const sub =
          [
            before != null && after != null
              ? `${label} ${before} → ${after}`
              : before != null
                ? `${label} at ${before}`
                : null,
            detail || null,
          ]
            .filter(Boolean)
            .join(' · ') || undefined;
        compact({
          kind: 'retarget',
          text: `Retargeting confidence: ${actionLabel}`,
          tone: climbed ? 'good' : 'info',
          sub,
        });
        verbose({
          kind: 'retarget',
          text: `Retargeting confidence: ${actionLabel}${round != null ? ` (round ${round})` : ''}`,
          tone: climbed ? 'good' : 'info',
          sub,
        });
        break;
      }
      case 'plan_loaded': {
        const b = parseConfidence(e['plan_confidence_breakdown']);
        if (b) confidence = b;
        const pc = num(e['plan_confidence']);
        if (typeof pc === 'number') setConf(pc);
        // plan_loaded ALSO carries the floor, and it is the only place it appears on a run where goose did
        // not need to ask. Reading it only from low_confidence_ask meant the bar was known precisely when
        // the swarm had paused for you, and unknown on every confident run — so the gauge's floor marker
        // and the "your bar is N" line silently never rendered for the healthy case.
        const afp = num(e['ask_floor']);
        if (typeof afp === 'number') askFloor = afp;
        const tasks = arr(e['tasks']) as Array<Record<string, unknown>>;
        plan = tasks.map((t) => ({
          id: str(t['id']),
          description: t['description'] ? str(t['description']) : undefined,
          files: arr(t['files']).map(String),
          deps: arr(t['deps']).map(String),
          difficulty: str(t['difficulty']),
        }));
        const n = num(e['task_count']) ?? tasks.length;
        compact({
          kind: 'plan',
          text: `Plan ready — ${n} task${n === 1 ? '' : 's'}`,
          sub: plan.map((t) => t.id).join(', '),
        });
        verbose({
          kind: 'plan',
          text: `Plan ready — ${n} task${n === 1 ? '' : 's'}`,
          tone: 'info',
        });
        for (const t of plan) {
          const bits = [
            t.difficulty && `${t.difficulty}`,
            t.deps.length && `after ${t.deps.join(', ')}`,
            t.files.length && t.files.join(', '),
          ]
            .filter(Boolean)
            .join(' · ');
          verbose({ kind: 'plan', text: `· ${t.id}`, sub: bits || undefined });
        }
        phase = 'Building';
        break;
      }
      case 'task_dispatched': {
        const task = str(e['task_id']);
        // integrate-verify is the SINK — its work IS the INTEGRATE phase (assemble the modules, run the
        // suite, boot the advertised entry). Dispatching it means the run has entered Integrate, not Build.
        // Without this the ribbon sat on "Build" for the whole (long) sink grind while it was actually
        // integrating — the exact "isn't this already Verify? it still shows Building" confusion.
        const isSink = task === 'integrate-verify';
        const verb = isSink ? 'Integrating & verifying' : `Building ${task}`;
        const node = e['device'] ? nodeOf(str(e['device'])) : '';
        const attempt = num(e['attempt']) ?? 0;
        compact({ kind: 'dispatch', text: verb, sub: node ? `on ${node}` : undefined });
        const owned = arr(e['owned_files']).map(String).join(', ');
        verbose({
          kind: 'dispatch',
          text: `${verb}${attempt > 0 ? ` (attempt ${attempt + 1})` : ''}`,
          sub: [node && `on ${node}`, owned].filter(Boolean).join(' — ') || undefined,
        });
        phase = isSink ? 'Integrating' : 'Building';
        break;
      }
      case 'task_retry': {
        const t = `Retrying ${str(e['task_id'])}`;
        compact({ kind: 'retry', text: t, tone: 'warn' });
        // The failure reason is on `error`, not `reason` — the old key was always empty, dropping the "why".
        verbose({ kind: 'retry', text: t, tone: 'warn', sub: str(e['error']) || undefined });
        break;
      }
      case 'replanned': {
        // A dynamic-replan round that actually spliced in new tasks — the swarm noticed missing work and grew
        // the plan. The routine "checked, nothing to add" rounds (added empty) are noise and stay hidden.
        const added = arr(e['added']).map(String);
        if (added.length > 0) {
          const t = `Re-planned — added ${added.length} task${added.length === 1 ? '' : 's'}`;
          compact({ kind: 'retry', text: t, tone: 'warn' });
          verbose({ kind: 'retry', text: t, sub: added.join(', '), tone: 'warn' });
        }
        break;
      }
      case 'scheduler_stuck': {
        // Terminal deadlock: tasks remain but none can be dispatched (an unsatisfiable dep / a sink that never
        // unblocks). The run cannot finish — surface it loudly; it is effectively a failure end-state.
        const rem = num(e['remaining']) ?? 0;
        const t = `Scheduler stuck — ${rem} task${rem === 1 ? '' : 's'} blocked, run can't finish`;
        compact({ kind: 'fail', text: t, tone: 'bad' });
        verbose({ kind: 'fail', text: t, tone: 'bad' });
        break;
      }
      case 'judge_verdict': {
        const verdict = str(e['verdict']);
        const conf = num(e['confidence']);
        const hint = str(e['hint']);
        // Only surface a judge verdict in verbose when it's actionable (not a routine "ok/observed", and an
        // empty/missing verdict is the ok case too) OR it carries a hint — those are the moments the AI
        // judge catches spec-drift/looping/over-reading.
        if ((verdict !== 'ok' && verdict !== '') || hint) {
          verbose({
            kind: 'judge',
            text: `Judge: ${str(e['task_id'])} → ${verdict || 'ok'}${conf != null ? ` (${Math.round(conf * 100)}%)` : ''}`,
            sub: hint || undefined,
            tone: judgeTone(verdict),
          });
        }
        break;
      }
      // The user typed something mid-build; this is the engine confirming a worker actually received it.
      // Not verbose-only: it is the user's own input landing, which they should always see.
      case 'user_notes_delivered': {
        const n = num(e['count']) ?? (arr(e['notes']) as unknown[]).length;
        const dropped = num(e['dropped']) ?? 0;
        // BOTH feeds. This is the user's OWN input landing in the build — the one activity line they are
        // entitled to see without switching the panel to verbose. (Verified live: it was reaching only the
        // verbose feed, so a delivery the engine had recorded showed nowhere in the default view.)
        compact({
          kind: 'note',
          text: `Your note${n === 1 ? '' : `s (${n})`} reached ${str(e['task_id'])}`,
          sub:
            dropped > 0 ? `${dropped} older note(s) left out to keep the prompt small` : undefined,
          tone: 'good',
        });
        verbose({
          kind: 'note',
          text: `Your note${n === 1 ? '' : `s (${n})`} reached ${str(e['task_id'])}`,
          sub:
            dropped > 0 ? `${dropped} older note(s) left out to keep the prompt small` : undefined,
          tone: 'good',
        });
        break;
      }
      case 'pre_review': {
        const had = !!e['had_findings'];
        verbose({
          kind: 'prereview',
          text: `Pre-review ${str(e['task_id'])}: ${had ? 'findings raised' : 'clean'}`,
          tone: had ? 'warn' : 'good',
        });
        break;
      }
      case 'task_completed': {
        const failed = /fail|error/i.test(str(e['status']));
        const task = str(e['task_id']);
        const secs = num(e['elapsed_ms']);
        const nCalls = arr(e['tool_calls']).length;
        compact({
          kind: failed ? 'fail' : 'done',
          text: `${task} ${failed ? 'failed' : 'done'}`,
          tone: failed ? 'bad' : 'good',
        });
        const detail = [
          secs != null && `${Math.round(secs / 1000)}s`,
          nCalls && `${nCalls} tool calls`,
        ]
          .filter(Boolean)
          .join(' · ');
        verbose({
          kind: failed ? 'fail' : 'done',
          text: `${task} ${failed ? 'failed' : 'done'}`,
          sub: detail || undefined,
          tone: failed ? 'bad' : 'good',
        });
        break;
      }
      case 'smoke': {
        const r = (e['result'] ?? {}) as Record<string, unknown>;
        const tests = (r['tests'] ?? {}) as Record<string, unknown>;
        const testsPass = 'kind' in tests ? str(tests['kind']) === 'pass' : null;
        smoke = {
          ran: !!r['ran'],
          testsPass,
          entryOk: typeof r['entry_ok'] === 'boolean' ? (r['entry_ok'] as boolean) : null,
          pyFiles: num(r['py_files']) ?? 0,
        };
        compact({ kind: 'phase', text: 'Running end-to-end smoke tests' });
        verbose({
          kind: 'smoke',
          text: 'End-to-end smoke test',
          sub: [
            smoke.testsPass == null ? null : `tests ${smoke.testsPass ? 'pass' : 'fail'}`,
            smoke.entryOk == null ? null : `entry ${smoke.entryOk ? 'ok' : 'broken'}`,
            smoke.pyFiles ? `${smoke.pyFiles} files` : null,
          ]
            .filter(Boolean)
            .join(' · '),
          tone: smoke.testsPass === false || smoke.entryOk === false ? 'bad' : 'good',
        });
        phase = 'Integrating';
        break;
      }
      case 'complete_verify': {
        // The sink's verdict: findings mean the run has moved into REPAIR, none mean it is still integrating.
        phase = (num(e['findings']) ?? 0) > 0 ? 'Repairing' : 'Integrating';
        break;
      }
      case 'complete_fix_dispatched':
      case 'complete_fix_completed':
      case 'complete_fix_wave':
      case 'spec_repair_wave': {
        phase = 'Repairing';
        break;
      }
      case 'run_finished': {
        const report = (e['report'] ?? {}) as Record<string, unknown>;
        const done = arr(report['done']).length;
        // Only CORE failures decide the verdict: a failed BONUS task (optional extra work) must not make the
        // whole run read as "failed".
        const bonus = new Set(arr(report['bonus']).map(String));
        const coreFailed = arr(report['failed'])
          .map(String)
          .filter((id) => !bonus.has(id)).length;
        const phases = (e['phases'] ?? {}) as Record<string, unknown>;
        const perDevRaw = (report['per_device'] ?? {}) as Record<string, unknown>;
        const perDevice: DeviceStat[] = Object.entries(perDevRaw)
          .map(([device, v]) => {
            const d = (v ?? {}) as Record<string, unknown>;
            return {
              node: nodeOf(device),
              device,
              dispatched: num(d['dispatched']) ?? 0,
              toolCalls: num(d['tool_calls']) ?? 0,
              busyMs: num(d['busy_ms']) ?? 0,
            };
          })
          .filter((d) => d.dispatched > 0 || d.toolCalls > 0)
          .sort((a, b) => a.node.localeCompare(b.node));
        summary = { done, failed: coreFailed, totalMin: num(phases['total_min']), perDevice };
        compact({ kind: 'phase', text: 'Build complete', tone: coreFailed ? 'warn' : 'good' });
        verbose({
          kind: 'phase',
          text: 'Build complete',
          sub: `${done} done${coreFailed ? ` · ${coreFailed} failed` : ''}`,
          tone: coreFailed ? 'warn' : 'good',
        });
        phase = 'Done';
        finished = true;
        break;
      }
      case 'run_overview': {
        overview = {
          generated: e['generated'] === true,
          runCommand: typeof e['run_command'] === 'string' ? (e['run_command'] as string) : null,
          runCommandLang:
            typeof e['run_command_lang'] === 'string' ? (e['run_command_lang'] as string) : null,
          runCommandVerified: e['run_command_verified'] === true,
          features: arr(e['features']).map(String),
          engage: typeof e['engage'] === 'string' ? (e['engage'] as string) : null,
          next: arr(e['next']).map(String),
        };
        break;
      }
      default:
        break;
    }
  }
  return {
    activity: feed.slice(-30),
    verbose: vfeed.slice(-200),
    meta,
    plan,
    smoke,
    phase,
    finished,
    confidence,
    askFloor,
    confidenceTrail: confTrail,
    planConfidence,
    summary,
    startedAt,
    overview,
    slices:
      sliceIds.length > 0 || sliceBriefChars.length > 0
        ? {
            ids: sliceIds,
            weights: sliceWeights,
            openSecs: sliceOpenSecs,
            briefChars: sliceBriefChars,
            researchSecs: sliceResearchSecs,
          }
        : null,
    proxy,
    reviewRounds,
    sinkRenamedFrom,
    synthesisFallback,
    knownActiveBugs,
  };
}

type Digest = {
  tool_calls?: number;
  errors?: number;
  recent?: string[];
  last_text?: string;
  reasoning?: string;
  full_reasoning?: string;
  calls?: SwarmCall[];
  inflight?: InflightCall[];
  /** Reasoning-channel activity: these local coder models do their PLAN drafting in the <think> channel, so
   *  reasoning/last_text stay empty while thinking_chars climbs. A lane with thinking is WORKING — without
   *  these fields a heavily-generating node reads as "idle — no task". */
  thinking_chars?: number;
  last_thinking?: string;
  /** Set to "processing" on the seed digest written at dispatch (before the first token) so the node shows as
   *  prompt-processing rather than idle; cleared once real tokens arrive. */
  phase?: string;
  /** The durable append-only logs beside the digest, attached by main.ts as it reads — NOT written by the
   *  engine into the JSON. They were typed nowhere, so every reader cast the digest inline and each cast
   *  was free to name a different field. (`judging` below is the exception and is marked as such.) */
  full_thinking?: string;
  thinking_bytes?: number;
  full_transcript?: string;
  transcript_bytes?: number;
  transcript_clipped?: boolean;
  /// Engine-written, unlike the fields above it: swarm.rs:15981 stamps it into the digest JSON itself.
  judging?: boolean;
  /** SAID provenance (engine-written, all five together from the shared builder): which ATTEMPT produced
   *  `last_text`, when the call was dispatched, when the answer channel last advanced, and whether the
   *  text is the model's (`said`) or an agent transport error (`error`). r0's ledger-core-tests showed
   *  attempt 0's "Network error … Please resend your message" as the live answer for 24+ minutes while
   *  attempt 1 ran — these fields are what lets the pane say WHOSE text it is showing. */
  attempt?: number;
  dispatched_at?: string;
  said_at?: string | null;
  said_kind?: SaidKind;
  /** What a previous attempt (or a previous call reusing this lane key) left behind — folded in by the
   *  new attempt's seed instead of being silently erased. Oldest first. */
  superseded?: SupersededSaid[];
  /** II-11b: tool calls the provider stream has NAMED whose argument bodies are still buffering
   *  server-side — attached by main.ts from `<key>.forming.json` (absent = nothing forming; the
   *  engine removes the file on completion or scope exit). Open-frame keyed ONLY: LM Studio ships
   *  the whole argument body in one terminal delta, so there is no byte progress to show. */
  forming?: FormingCall[];
};

/** One forming tool call: named by the stream, arguments not yet arrived. */
export interface FormingCall {
  id: string;
  name: string;
  since_ms: number;
}

export type SaidKind = 'said' | 'error';

/** One prior attempt's SAID text, carried in the digest so a retry never silently erases it. */
export interface SupersededSaid {
  /** null on an entry folded from a legacy digest that predates the provenance keys. */
  attempt?: number | null;
  last_text?: string;
  said_kind?: SaidKind;
  said_at?: string | null;
  model?: string | null;
}

/** The agent-authored closers of the three provider-error texts (agent.rs) — the ONLY strings that reach
 *  the answer channel without the model having said them. MIRRORS swarm.rs `AGENT_ERROR_CLOSERS`; used to
 *  classify transcript segments and legacy digests that carry no `said_kind`. */
const AGENT_ERROR_CLOSERS = [
  'Please resend your message to try again.',
  'Please retry if you think this is a transient or recoverable error.',
  'resending this conversation is likely to be refused again.',
];

/** `error` when the text is (the tail of) an agent transport-error message, `said` otherwise. */
export function saidKindOf(text: string | undefined): SaidKind {
  const t = (text ?? '').trimEnd();
  return AGENT_ERROR_CLOSERS.some((c) => t.endsWith(c)) ? 'error' : 'said';
}

/** One attempt's slice of an append-only transcript, cut at the engine's attempt-marker lines. */
export interface TranscriptAttempt {
  /** null for text that predates the first marker (a legacy log, or a resumed run's earlier bytes). */
  attempt: number | null;
  dispatchedAt?: string;
  text: string;
}

/** The boundary line `append_attempt_marker` (swarm.rs) writes into `<task>.log`/`.think.log` at every
 *  dispatch: `===== swarm attempt N · dispatched <rfc3339> =====`. */
const ATTEMPT_MARKER_RE = /^===== swarm attempt (\d+) · dispatched (\S+) =====$/gm;

/**
 * Split an append-only transcript at its attempt markers: the LIVE segment (after the last marker) plus
 * every superseded one before it. The transcripts deliberately survive retries — that is their value as
 * the durable record — but rendering the whole file as "what the model says" is exactly how attempt 0's
 * transport error read as the live answer for 24+ minutes while attempt 1 was still thinking. A legacy
 * log with no markers comes back whole as the live segment, so old runs read exactly as before.
 */
export function splitTranscriptAttempts(text: string | undefined): {
  live: TranscriptAttempt;
  superseded: TranscriptAttempt[];
} {
  const whole = text ?? '';
  const segments: TranscriptAttempt[] = [];
  let cursor = 0;
  let current: TranscriptAttempt = { attempt: null, text: '' };
  for (const m of whole.matchAll(ATTEMPT_MARKER_RE)) {
    current.text = whole.slice(cursor, m.index);
    segments.push(current);
    current = { attempt: Number(m[1]), dispatchedAt: m[2], text: '' };
    cursor = (m.index ?? 0) + m[0].length;
  }
  current.text = whole.slice(cursor);
  segments.push(current);
  const live = segments[segments.length - 1];
  const superseded = segments.slice(0, -1).filter((s) => s.text.trim().length > 0);
  return { live, superseded };
}

export type LiveChannel = 'thinking' | 'transcript';

/**
 * WHICH CHANNEL ADVANCED LAST, per lane, across polls.
 *
 * MEASURED on r1 (gabee, 19:52, two consecutive ticks): the fleet cell for `review-build-app-meridian-…`
 * showed round 1's final answer — data-gen-len 2676, unchanged for twenty minutes — while the digest's
 * thinking_chars climbed past 24,000 with a new call's reasoning behind it. REVIEW reuses the lane key
 * every round; `<task>.log` is append-only, so the previous answer never goes away; and a live line that
 * prefers the transcript whenever it is non-empty shows that answer for as long as the new call thinks.
 *
 * So the live line follows the channel that GREW in the most recent poll. Growth is measured per signal,
 * because no single one is reliable alone: `transcript_bytes` / `thinking_bytes` are the true file sizes
 * but arrive only with a durable log; `full_*.length` stops moving once main.ts clips the tail; and
 * `thinking_chars` RESETS on a re-stream — it is a counter, not a size. Any signal moving up is that
 * channel advancing. Ties and silence keep the previous answer; a transcript that only SHRANK is a
 * different log under the same key (a new run in the same directory) and starts over.
 *
 * Module state, like the fold cache: the previous poll's lengths have to live where the next poll can see
 * them, and neither the carry (rebuilt on a generation change) nor the laneless fleet rows (built outside
 * the fold) can hold them.
 */
interface ChannelSeen {
  transcript: number[];
  thinking: number[];
  channel: LiveChannel;
}
const channelMemory = new Map<string, ChannelSeen>();

const grew = (now: number[], before: number[]) => now.some((n, i) => n > before[i]);
const shrank = (now: number[], before: number[]) => now.some((n, i) => n < before[i]);

function liveChannelFor(key: string, d: Digest | undefined, done: boolean): LiveChannel {
  const transcript = [d?.transcript_bytes ?? 0, d?.full_transcript?.length ?? 0];
  const thinking = [d?.thinking_bytes ?? 0, d?.thinking_chars ?? 0, d?.full_thinking?.length ?? 0];
  const seen = channelMemory.get(key);
  const firstSight =
    !seen || (shrank(transcript, seen.transcript) && !grew(transcript, seen.transcript));
  let channel: LiveChannel;
  if (firstSight) {
    channel = transcript.some((n) => n > 0) ? 'transcript' : 'thinking';
  } else {
    const transcriptGrew = grew(transcript, seen.transcript);
    const thinkingGrew = grew(thinking, seen.thinking);
    channel =
      transcriptGrew === thinkingGrew ? seen.channel : transcriptGrew ? 'transcript' : 'thinking';
  }
  // A finished call's live line is its answer, whichever channel happened to move last.
  if (done) channel = 'transcript';
  channelMemory.set(key, { transcript, thinking, channel });
  return channel;
}

/** Test seam, cleared with the fold cache: the channel memory is module state keyed by lane. */
export function resetLiveChannelMemory(): void {
  channelMemory.clear();
}

/**
 * EVERY FIELD A DIGEST CONTRIBUTES TO A LANE, IN ONE PLACE.
 *
 * This join was copy-pasted into FIVE lane-building paths, each free to diverge, and it did — twice.
 * `fullThinking` reached one path while `thinkingChars` reached four, so the inspector's header counted a
 * transcript its body was not showing. Extracting only the nine STREAM fields left the other eight still
 * hand-copied, and the sixth omission was already sitting in that remainder: the repair-twin path set
 * `errors` and not `phase`, so a node prompt-processing a fix read idle in the fleet strip
 * (`phase === 'processing'` is the only thing that says WORKING before the first token) and a twin whose
 * digest stamped `phase: 'done'` never dropped out of running.
 *
 * So the whole join lives here. A path that spreads this cannot be half-wired, and a new digest field is
 * one edit rather than five.
 *
 * `prev` is the lane's carried event-derived value, kept when a digest has not (yet) supplied one. It is
 * absent on the paths built FROM a digest (plan drafts, scouts, contracts, the fleet strip's laneless rows),
 * where the digest is the only source there is.
 */
export function digestStreamFields(
  key: string,
  d: Digest | undefined,
  prev?: Partial<TurnLane>
): Pick<
  TurnLane,
  | 'lastText'
  | 'recent'
  | 'reasoning'
  | 'fullReasoning'
  | 'calls'
  | 'inflight'
  | 'toolCalls'
  | 'thinkingChars'
  | 'lastThinking'
  | 'fullThinking'
  | 'thinkingBytes'
  | 'fullTranscript'
  | 'transcriptBytes'
  | 'transcriptClipped'
  | 'judging'
  | 'phase'
  | 'liveChannel'
  | 'errors'
  | 'attempt'
  | 'dispatchedAt'
  | 'saidAt'
  | 'saidKind'
  | 'superseded'
  | 'forming'
> {
  // ATTEMPT-KEYED CARRY (the measured 24m30s failure). A carried `lastText` belongs to the attempt
  // that produced it; when the digest names a NEWER attempt, that carry is a dead attempt's text —
  // now living, labeled, in the digest's own `superseded` list — and must not masquerade as the new
  // attempt's answer. When either side lacks an attempt (legacy digests), today's carry stands.
  const carryIsStale = d?.attempt != null && prev?.attempt != null && d.attempt > prev.attempt;
  return {
    // `||`, not `??`: an empty `last_text` is a digest that has produced no answer text yet, and the
    // carried value is better than blanking the lane. Every other field distinguishes absent from empty.
    lastText: d?.last_text || (carryIsStale ? undefined : prev?.lastText),
    recent: d?.recent ?? prev?.recent,
    reasoning: d?.reasoning ?? prev?.reasoning,
    fullReasoning: d?.full_reasoning ?? prev?.fullReasoning,
    calls: d?.calls ?? prev?.calls,
    inflight: d?.inflight ?? prev?.inflight,
    toolCalls: d?.tool_calls ?? prev?.toolCalls,
    thinkingChars: d?.thinking_chars ?? prev?.thinkingChars,
    lastThinking: d?.last_thinking ?? prev?.lastThinking,
    fullThinking: d?.full_thinking ?? prev?.fullThinking,
    thinkingBytes: d?.thinking_bytes ?? prev?.thinkingBytes,
    fullTranscript: d?.full_transcript ?? prev?.fullTranscript,
    transcriptBytes: d?.transcript_bytes ?? prev?.transcriptBytes,
    transcriptClipped: d?.transcript_clipped ?? prev?.transcriptClipped,
    judging: d?.judging ?? prev?.judging,
    phase: d?.phase ?? prev?.phase,
    // `key` is what the channel memory is kept by: the digest carries no id of its own, and the whole
    // point is remembering the previous poll of THIS lane.
    liveChannel: liveChannelFor(key, d, d?.phase === 'done' || prev?.status === 'done'),
    errors: d?.errors ?? prev?.errors,
    attempt: d?.attempt ?? prev?.attempt,
    dispatchedAt: d?.dispatched_at ?? prev?.dispatchedAt,
    saidAt: d?.said_at ?? prev?.saidAt,
    saidKind: d?.said_kind ?? prev?.saidKind,
    superseded: d?.superseded ?? prev?.superseded,
    // Deliberately NO prev carry: the sidecar's absence is the engine saying nothing is forming.
    forming: d?.forming,
  };
}

/** What the panel reads off one fold of the run log plus the activity digests. */
export interface FoldedRun {
  lanes: TurnLane[];
  totals: SwarmRunTotals;
  planLanes: TurnLane[];
  scoutLanes: TurnLane[];
  contractLanes: TurnLane[];
  detailLanes: TurnLane[];
  sliceLanes: TurnLane[];
  planningLanes: TurnLane[];
  fixLanes: TurnLane[];
}

/**
 * Everything the fold derives from the EVENT STREAM ALONE, carried between ticks so an appended event is
 * folded once instead of the whole log being re-folded twice a second.
 *
 * The activity digests are deliberately absent: they are re-joined from scratch on every call, because a
 * digest is REWRITTEN in place (~2.5x/s while a node streams) rather than appended, so nothing about it
 * can be carried. What is carried here is only what an append-only log can produce.
 */
interface FoldCarry {
  tasks: Map<string, TurnLane>;
  fixTasks: Map<string, TurnLane>;
  descriptions: Map<string, string>;
  judgeEta: Map<string, number>;
  canon: NodeCanon;
  seq: number;
  planned: boolean;
  researchOver: boolean;
  contractsOver: boolean;
}

const newFoldCarry = (): FoldCarry => ({
  tasks: new Map(),
  // The verify REPAIR WAVE dispatches fix twins via complete_fix_dispatched / complete_fix_completed —
  // NOT the task_dispatched lifecycle — so for its whole duration (10-18 min per twin, measured) no lane
  // existed and every busy node read "idle — no task". Kept separate from `tasks` so the header's task
  // totals stay engine-task counts.
  fixTasks: new Map(),
  descriptions: new Map(),
  // THE JUDGE'S OWN ETA, kept per task. It is asked for an `ETA=<n>m` on every look and it answers — the
  // live run shows open-coverage-2 estimating 5, 5, 3, 3, 2 as it converged. Nothing consumed it, so the
  // only "time left" on screen was the panel's own extrapolation from item counts. The last look wins:
  // the judge revises as it reads more, and an older estimate is strictly worse information.
  judgeEta: new Map(),
  canon: emptyNodeCanon(),
  seq: 0,
  planned: false,
  researchOver: false,
  contractsOver: false,
});

/**
 * One event onto the carried state. This is the ONLY place the event stream is interpreted — the
 * from-scratch fold and the incremental fold both run exactly this, which is what makes
 * "prefix then remainder === whole" true by construction rather than by two implementations agreeing.
 */
function absorbEvent(c: FoldCarry, e: Record<string, unknown>): void {
  const type = String(e['event'] ?? '');

  // The engine reports a device by its POOL id ('mac-qwen3.6-27b') for worker tasks but by its MODEL id
  // ('gabee-qwen/qwen3.6-27b') for scouts/plan drafts, so the SAME physical node showed up twice — 6 rows
  // for 3 machines. nodeLabeler ties them via run_started.pool / pool_resolved and is the ONE canonical map
  // every rendered node label goes through (the feed included — see buildActivity).
  absorbNodeCanon(c.canon, e);

  // The phase flags were `events.some(...)` scans of the whole array. Latched here instead: once true they
  // never go back, which is what `.some()` means for an append-only log.
  if (type === 'plan_loaded' || type === 'task_dispatched') {
    c.planned = true;
    c.researchOver = true;
  }
  if (type === 'research_completed') c.researchOver = true;
  // The BUILD phase event is the engine's own end-of-contracts mark; task_dispatched stays for runs that
  // predate it, and for the contract lane a node is still writing when the first worker starts.
  if (type === 'task_dispatched' || (type === 'phase' && e['phase'] === 'build'))
    c.contractsOver = true;

  if (type === 'judge_look') {
    const id = typeof e['task_id'] === 'string' ? e['task_id'] : '';
    const eta = e['eta_mins'];
    if (id && typeof eta === 'number' && Number.isFinite(eta)) c.judgeEta.set(id, eta);
  }

  if (type === 'plan_loaded') {
    const ts = Array.isArray(e['tasks']) ? (e['tasks'] as Array<Record<string, unknown>>) : [];
    for (const t of ts) {
      const d = t['description'] ? String(t['description']) : '';
      if (d) c.descriptions.set(String(t['id'] ?? ''), d);
    }
    return;
  }
  if (type === 'spec_repair_wave' || type === 'complete_result' || type === 'run_finished') {
    // The wave (or the run) is over — no twin may keep spinning past it, even if its own
    // complete_fix_completed was lost (early close).
    for (const [k, t] of c.fixTasks)
      if (t.status === 'running') c.fixTasks.set(k, { ...t, status: 'done', seq: c.seq++ });
    return;
  }
  let taskId = String(e['task_id'] ?? '');
  // The race/fan arms' completed events historically carried twin/shard but NO task_id, so a
  // finished twin's lane stayed "running" until the wave's end event — a completed node showed
  // "Repairing…" for 10+ minutes while actually idle (caught on screen against the live event
  // stream). The engine now emits task_id everywhere; this fallback reconstructs it for streams
  // recorded before that, using the same id scheme the dispatch events use.
  if (!taskId && (type === 'complete_fix_completed' || type === 'fix_attempt_progress')) {
    const twin = num(e['twin']);
    const shard = e['shard'] ? String(e['shard']) : '';
    if (twin != null) taskId = `complete-fix::twin${twin}`;
    else if (shard && shard !== '(cross-file)') taskId = `complete-fix::${shard}`;
    else if (shard) taskId = 'complete-fix::cross-file';
  }
  if (!taskId) return;

  if (type === 'complete_fix_dispatched') {
    const model = str(e['model']);
    c.fixTasks.set(taskId, {
      taskId,
      description: `Repairing verify findings (round ${num(e['round']) ?? 0})`,
      device: model || '?',
      model: model || undefined,
      status: 'running',
      seq: c.seq++,
    });
    return;
  }
  if (type === 'complete_fix_completed') {
    const prev = c.fixTasks.get(taskId);
    if (prev) c.fixTasks.set(taskId, { ...prev, status: 'done', seq: c.seq++ });
    return;
  }

  if (type === 'task_dispatched') {
    const prev = c.tasks.get(taskId);
    c.tasks.set(taskId, {
      taskId,
      device: String(e['device'] ?? prev?.device ?? '?'),
      model: e['model'] ? String(e['model']) : prev?.model,
      status: 'running',
      // The scheduler's attempt counter — the event-side half of the SAID provenance, so the
      // attempt-keyed lastText carry works even before the new attempt's first digest lands.
      attempt: typeof e['attempt'] === 'number' ? e['attempt'] : prev?.attempt,
      // A re-dispatch after a retry keeps the retry's failure text: it is the WHY behind the
      // superseded chip ("retried: <reason>") until the task terminally completes.
      error: prev?.error,
      seq: c.seq++,
    });
  } else if (type === 'task_retry') {
    const prev = c.tasks.get(taskId);
    c.tasks.set(taskId, {
      taskId,
      device: String(e['from_device'] ?? prev?.device ?? '?'),
      model: prev?.model,
      status: 'running',
      // Keep the retry's failure text so a lane that ultimately fails/stalls can explain why.
      error: e['error'] ? String(e['error']) : prev?.error,
      attempts: prev?.attempts,
      attempt: prev?.attempt,
      seq: c.seq++,
    });
  } else if (type === 'task_completed') {
    const prev = c.tasks.get(taskId);
    const statusStr = String(e['status'] ?? '').toLowerCase();
    const status: TurnStatus =
      statusStr.includes('fail') || statusStr.includes('error') ? 'error' : 'done';
    const toolCalls = Array.isArray(e['tool_calls'])
      ? (e['tool_calls'] as unknown[]).length
      : prev?.toolCalls;
    c.tasks.set(taskId, {
      taskId,
      device: String(e['device'] ?? prev?.device ?? '?'),
      model: e['model'] ? String(e['model']) : prev?.model,
      status,
      toolCalls,
      elapsedMs: typeof e['elapsed_ms'] === 'number' ? (e['elapsed_ms'] as number) : undefined,
      attempts: typeof e['attempts'] === 'number' ? (e['attempts'] as number) : prev?.attempts,
      // A failed completion keeps the last retry's reason; a clean one clears it.
      error: status === 'error' ? prev?.error : undefined,
      seq: c.seq++,
    });
  } else if (type === 'judge_verdict' && str(e['action']) === 'split') {
    // The judge decomposed this task into freshly-dispatched children. The parent gets no task_completed, so
    // its lane would spin forever. Drop it — the children have their own lanes and the split stays in the feed.
    c.tasks.delete(taskId);
  }
}

/**
 * The carried event state joined to the CURRENT activity digests.
 *
 * Every field a digest contributes is read fresh here on every call, and every lane is a new object — the
 * carry is never mutated by this join. That is the whole reason a stale digest can never be served: only
 * the event-derived half is cached, and this half is rebuilt each tick.
 */
function finishFold(c: FoldCarry, activity: Record<string, unknown>): FoldedRun {
  const canonDevice = nodeCanonLabeler(c.canon);

  const lanes = [...c.tasks.values()].map((t) => {
    const act = activity[t.taskId] as Digest | undefined;
    return {
      ...t,
      device: canonDevice(t.device),
      description: cleanTaskTitle(c.descriptions.get(t.taskId) ?? t.description, t.taskId),
      // THE BUILD LANE IS THE FIFTH PATH, AND IT WINS.
      //
      // The digest fields were once believed to be "set on all four of these paths". There are FIVE.
      // This one -- the BUILD worker lane -- was never counted, and it is first in `laneSources`, so it
      // is what BOTH the fleet strip and the inspector receive for a build node. The whole of BUILD,
      // which is where a run spends its hours, therefore had every one of these undefined.
      //
      // What that cost, all of it invisible to me because I only ever inspected planner lanes:
      //   - the thinking line is gated on thinkingChars > 0, so it never rendered
      //   - phase === 'processing' never fired; a node chewing 100k of prompt read "generating..."
      //   - canExpand is fullGen.length > 0, so a THINKING-ONLY model produced an UNCLICKABLE ROW --
      //     the node you most need to open is the one you cannot
      //   - the "supervisor reading" badge and the char count were dead
      //   - LaneRow and BoardTaskRow fell back to the 24k full_reasoning CLIP, which is exactly the
      //     truncation I had already declared fixed twice
      ...digestStreamFields(t.taskId, act, t),
    };
  });

  // Repair-wave twins, canonicalized + joined to any digest they wrote — the fleet strip folds these in
  // so a node grinding a fix twin reads WORKING, not idle.
  const fixLanes = [...c.fixTasks.values()].map((t) => {
    const act = activity[t.taskId] as Digest | undefined;
    return {
      ...t,
      device: canonDevice(t.device),
      ...digestStreamFields(t.taskId, act, t),
    };
  });

  // DONE on top (finished history), IN-PROGRESS at the BOTTOM — the active front sits just above the chat
  // input where the eye is. Within a group, oldest-activity first so the freshest work sinks to the bottom.
  const order: Record<TurnStatus, number> = { done: 0, error: 1, running: 2 };
  lanes.sort((a, b) => order[a.status] - order[b.status] || a.seq - b.seq);

  const totals: SwarmRunTotals = {
    tasks: lanes.length,
    running: lanes.filter((l) => l.status === 'running').length,
    done: lanes.filter((l) => l.status === 'done').length,
    failed: lanes.filter((l) => l.status === 'error').length,
  };

  // PLAN-phase generation lanes: each parallel architect draft writes a `plandraft-N` digest (model + full
  // reasoning). Surface them as their OWN group so you can see what every model generated while decomposing
  // the app — the reasoning that was invisible before. NOT build tasks, so they're excluded from `totals`.
  // Running until the plan is chosen (plan_loaded / first dispatch), then done (kept, collapsed, for review).
  const planned = c.planned;
  const planLanes: TurnLane[] = Object.keys(activity)
    .filter((k) => /^plandraft-\d+$/.test(k))
    .sort()
    .map((k, i) => {
      const d = (activity[k] ?? {}) as Digest & { model?: string };
      return {
        taskId: k,
        description: `Drafting the plan skeleton (candidate ${i + 1})`,
        device: canonDevice(d.model ?? 'planner'),
        model: d.model,
        // A per-call phase="done" (written when THIS draft's call ends) marks the lane done immediately, so the
        // node stops showing as working the instant its call finishes — not when the whole phase ends.
        status: (d.phase === 'done' || planned ? 'done' : 'running') as TurnStatus,
        ...digestStreamFields(k, d),
        seq: i,
      };
    })
    // Keep a lane the moment it's dispatched: prompt-PROCESSING (phase set, no tokens yet), text, OR
    // reasoning-channel thinking. These coder models draft in the <think> channel (reasoning/last_text empty
    // while thinking_chars climbs), and prompt-processing precedes the first token — the old text-only filter
    // dropped both, so the Fleet strip read "idle" while the node was busy.
    .filter(
      (l) =>
        (l.fullReasoning || l.reasoning || l.lastText || '').trim().length > 0 ||
        (l.thinkingChars ?? 0) > 0 ||
        l.phase === 'processing'
    );

  // RESEARCH (scout-<lens>) and CONTRACTS (contract-<id>) now write per-node digests too, so those phases are no
  // longer a black box. Surface them as lanes, mirroring planLanes — but KEEP a lane that has TOOL CALLS even
  // before any narration (a scout/contract emits calls before it writes prose, so a text-only filter would hide a
  // live node mid-lookup). Running until the phase's next stage begins.
  const researchOver = c.researchOver;
  const contractsOver = c.contractsOver;
  const laneFromDigest = (k: string, desc: string, over: boolean, i: number): TurnLane => {
    const d = (activity[k] ?? {}) as Digest & { model?: string };
    return {
      taskId: k,
      description: desc,
      judgeEtaMins: c.judgeEta.get(k),
      device: canonDevice(d.model ?? 'planner'),
      model: d.model,
      // Per-call phase="done" (written when THIS call ends) drops the node out of "working" immediately, so a
      // finished/capped scout stops reading as "Scouting" while the node is actually idle.
      status: (d.phase === 'done' || over ? 'done' : 'running') as TurnStatus,
      ...digestStreamFields(k, d),
      seq: i,
    };
  };
  const hasActivity = (l: TurnLane) =>
    (l.fullReasoning || l.reasoning || l.lastText || '').trim().length > 0 ||
    (l.calls?.length ?? 0) > 0 ||
    (l.thinkingChars ?? 0) > 0 ||
    l.phase === 'processing';
  const scoutLanes: TurnLane[] = Object.keys(activity)
    .filter((k) => /^scout-/.test(k))
    .sort()
    .map((k, i) => laneFromDigest(k, `Scouting · ${k.replace(/^scout-/, '')}`, researchOver, i))
    .filter(hasActivity);
  const contractLanes: TurnLane[] = Object.keys(activity)
    .filter((k) => /^contract-/.test(k))
    .sort()
    .map((k, i) => laneFromDigest(k, `Contract · ${k.replace(/^contract-/, '')}`, contractsOver, i))
    .filter(hasActivity);
  // DETAILER (detail-<id>): the "Finalizing the plan" phase fleshes each module skeleton out on the fleet and
  // writes a per-node digest, so without these lanes the Fleet strip reads "idle" during finalizing too.
  const detailLanes: TurnLane[] = Object.keys(activity)
    .filter((k) => /^detail-/.test(k))
    .sort()
    .map((k, i) => laneFromDigest(k, `Detailing · ${k.replace(/^detail-/, '')}`, planned, i))
    .filter(hasActivity);

  // THE SLICE FAN — the rewritten engine's RESEARCH phase. Every node owns one slice and writes that
  // module's full specification into `.swarm/activity/slice-<id>.json`. Without an entry here the whole
  // fleet generates for minutes behind an empty Research lane list, which is exactly what the old
  // scout-/contract-/detail- table did for the phases it did not know about.
  const sliceLanes: TurnLane[] = Object.keys(activity)
    .filter((k) => /^slice-/.test(k))
    .sort()
    .map((k, i) => laneFromDigest(k, `Slice · ${k.replace(/^slice-/, '')}`, researchOver, i))
    .filter(hasActivity);

  // The single-node planning calls. Each is over the moment its own digest stamps phase='done', so a node
  // that finished synthesising stops reading as working without waiting for the next phase to open.
  // PIPELINE ORDER, with each fan slotted beside the phase it belongs to. Sorting these alphabetically
  // would put `rate` before `review` and `synthesis` last, which is not the order they run in — the list
  // is read as a sequence.
  const planningKeys: string[] = [];
  const activityKeys = Object.keys(activity);
  for (const k of PLANNING_DIGEST_KEYS) {
    if (activity[k] != null) planningKeys.push(k);
    const prefix = PLANNING_FAN_AFTER[k];
    if (prefix) {
      planningKeys.push(...activityKeys.filter((x) => x.startsWith(prefix)).sort());
    }
  }
  const planningLanes: TurnLane[] = planningKeys
    .map((k, i) => laneFromDigest(k, digestLabel(k), planned, i))
    .filter(hasActivity);

  return {
    lanes,
    totals,
    planLanes,
    scoutLanes,
    contractLanes,
    detailLanes,
    sliceLanes,
    planningLanes,
    fixLanes,
  };
}

export function foldEvents(
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown>
): FoldedRun {
  const carry = newFoldCarry();
  for (const e of events) absorbEvent(carry, e);
  return finishFold(carry, activity);
}

/**
 * WHICH LOG these events came from, and WHICH GENERATION of it — the whole cache key, straight from main.
 *
 * The events array is structured-cloned across IPC, so the renderer never sees the same object twice and
 * reference identity cannot answer "is this the same log, extended?". A content fingerprint (first/middle/
 * last event + length) can and does answer it WRONG: two arrays of the same length differing in the middle
 * fingerprint identically, and the fold would then serve a carry that never saw the differing event.
 *
 * main knows the answer for free. `readEvents` accumulates one array per file and rebuilds it only when the
 * file's IDENTITY changes (inode + birthtime) or it shrank; `generation` is bumped on exactly those rebuilds.
 * So: same runId + same generation + a length that did not go backwards IS an append, exactly, with no
 * heuristic. Anything else refolds from scratch, which is today's behaviour and always correct.
 */
export interface FoldSource {
  runId: string;
  generation: number;
}

interface FoldCacheEntry {
  runId: string;
  generation: number;
  length: number;
  carry: FoldCarry;
}

let foldCache: FoldCacheEntry | null = null;
let foldCounters = { fullFolds: 0, incrementalFolds: 0, eventsFolded: 0 };

/** Test seam: the fold's cache and the per-lane channel memory are module state, so a test that cares
 *  about either must be able to clear both. */
export function resetFoldCache(): void {
  foldCache = null;
  foldCounters = { fullFolds: 0, incrementalFolds: 0, eventsFolded: 0 };
  resetLiveChannelMemory();
}

/** How much work the fold has actually done — `eventsFolded` is the number a full re-fold per tick inflates. */
export function foldStats(): { fullFolds: number; incrementalFolds: number; eventsFolded: number } {
  return { ...foldCounters };
}

/**
 * `foldEvents`, folding only what was appended since the last call for the same log.
 *
 * `source` null (or a generation main could not supply) means no key, so no reuse — a full fold, every
 * time. Losing the optimisation is not a defect; serving a carry that does not match the array is.
 */
export function foldEventsIncremental(
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown>,
  source: FoldSource | null | undefined
): FoldedRun {
  const key = source && Number.isFinite(source.generation) ? source : null;
  const reuse =
    key !== null &&
    foldCache !== null &&
    foldCache.runId === key.runId &&
    foldCache.generation === key.generation &&
    events.length >= foldCache.length
      ? foldCache
      : null;
  const carry = reuse ? reuse.carry : newFoldCarry();
  const from = reuse ? reuse.length : 0;
  // DROP THE CACHE BEFORE TOUCHING THE CARRY. An event that throws half way through leaves a carry that
  // holds part of a tick — a task dispatched but not completed, a seq that skipped. Serving that on the
  // next call would render a run that never happened, and it would look plausible. With the slot already
  // null, nothing can reach the damaged carry: the next call starts from scratch.
  foldCache = null;
  for (let i = from; i < events.length; i++) absorbEvent(carry, events[i]);
  foldCounters.eventsFolded += events.length - from;
  if (reuse) foldCounters.incrementalFolds++;
  else foldCounters.fullFolds++;
  if (key) {
    foldCache = { runId: key.runId, generation: key.generation, length: events.length, carry };
  }
  return finishFold(carry, activity);
}

// How long a digest may go unwritten and still count as a live open call. A streaming worker rewrites
// its digest ~2.5x/s, so this is generous headroom for a long single tool call inside a laneless worker
// (verify::*), while an interrupted call (phase never stamped 'done', file gone quiet) drops out.
export const DIGEST_FRESH_MS = 120_000;
// The longer window for a digest whose OWN RECORD says a tool call is still open (last call `ok: null`,
// phase not stamped 'done'). The engine rewrites the digest only while STREAMING; during one long shell
// call (cargo build, a big pytest run) the file sits unmodified, so the 120s mtime window alone flipped a
// laneless node to "idle" mid-call. The pending-call record is the digest's own open-state — trust it for
// as long as a legitimate single tool call can run; the run-level heartbeat still catches a dead engine.
export const DIGEST_OPEN_CALL_FRESH_MS = 900_000;

/**
 * THE DIGESTS THIS RUN ACTUALLY WROTE.
 *
 * `.swarm/activity/` is not cleared when a run starts — the engine truncates only `.swarm/prereview` — and
 * main globs the whole directory, so a SECOND run in the same working directory inherits every digest the
 * previous one left behind. Those carry a task id, a model and a `phase` that never reaches 'done' on a
 * killed run, so they mint lanes, claim nodes in the fleet strip and stamp a checklist row for work that
 * belongs to a run that is over. Nothing downstream can tell them apart: a digest names no run.
 *
 * The mtime can, and it is the one signal that needs no engine change. A digest written by THIS run cannot
 * predate this run's first event, so anything older is another run's leftover.
 *
 * Two deliberate non-rules. An UNKNOWN mtime is kept: an older main supplies none, and blanking every lane
 * beats nothing at all only if the gate is certain. And an unknown `startedAtMs` gates nothing — a stream
 * with no parseable timestamp cannot say when the run began, and a floor guessed from the clock would drop
 * live digests.
 */
export function digestsFromThisRun<T>(
  digests: Record<string, T>,
  mtimes: Record<string, number>,
  startedAtMs: number | null
): Record<string, T> {
  if (startedAtMs == null) return digests;
  const kept: Record<string, T> = {};
  for (const [key, value] of Object.entries(digests)) {
    const mtime = mtimes[key];
    if (typeof mtime === 'number' && mtime < startedAtMs) continue;
    kept[key] = value;
  }
  return kept;
}

/** An OPEN supervision generation — engine work that creates no task lane. */
export interface SupervisionSpan {
  /** `judge` is the post-task verdict; `look` is the omni-judge's MID-STREAM probe of a running call. */
  kind: 'judge' | 'look';
  /** The task being supervised (NOT the node doing the supervising — the events never name it at start). */
  taskId: string;
  /** Honest row label, e.g. "Judging · verify::meridian". */
  label: string;
  /** Epoch ms of the span's opening event, or null when the ts did not parse. */
  sinceMs: number | null;
}

// Longest a judge span may stay open before it is presumed lost (measured semantic reviews run 50–175s;
// a span past this is a crashed run's leftover, not live work).
export const JUDGE_SPAN_MAX_MS = 600_000;

/**
 * OPEN supervision spans from the event stream — the workload class the fleet strip was blind to.
 *
 * MEASURED (swarm-3node-r0, live): workhorse read "idle — no task" while LM Studio showed it processing 2
 * requests; the log tail was task_completed verify::web → pre_review web-js → judge_verdict verify::web →
 * judge_observed verify::meridian. The node's real work was SUPERVISION generations.
 *
 * TWO of that family have a derivable lifecycle PAIR, and both are folded here:
 *   judge  `judge_observed` opens (emitted on every judge invocation, before any early return) and
 *          `judge_verdict` / `judge_skipped` closes (Δ = the semantic review's 50–175s generation).
 *   look   `judge_look_dispatched` opens the omni-judge's MID-STREAM probe of a still-running call, and
 *          `judge_look` / `judge_look_abandoned` closes it. This half was emitted by the engine and read
 *          by nobody, which is precisely the state it exists to make visible: a look that is dispatched
 *          and never returns is a supervisor that died while supervising — measured once as 2h56m of
 *          engine silence with two of three nodes idle — and with only the CLOSING event folded, the
 *          span it opened had no representation at all until it came back.
 *
 * They are keyed separately: a task can be probed mid-stream and judged afterwards, and one map key would
 * let the probe's close silently retire the verdict's span.
 *
 * pre_review / testgen / sink_review emit a SINGLE end-stamped event (verified in swarm.rs) — no open span
 * is derivable for them, which is why a busy-but-unexplained node still needs the LM Studio join (see
 * deriveFleet). A task's completion closes both of its spans: neither a verdict nor a look on finished work
 * ever arrives.
 */
export function foldSupervision(events: Array<Record<string, unknown>>): SupervisionSpan[] {
  const open = new Map<string, SupervisionSpan>();
  const start = (kind: SupervisionSpan['kind'], taskId: string, label: string, ts: unknown) => {
    const ms = typeof ts === 'string' ? Date.parse(ts) : NaN;
    open.set(`${kind}:${taskId}`, {
      kind,
      taskId,
      label,
      sinceMs: Number.isNaN(ms) ? null : ms,
    });
  };
  for (const e of events) {
    const t = String(e['event'] ?? '');
    const taskId = str(e['task_id']);
    if (!taskId) continue;
    switch (t) {
      case 'judge_observed':
        start('judge', taskId, `Judging · ${taskId}`, e['ts']);
        break;
      case 'judge_look_dispatched':
        start('look', taskId, `Reading · ${taskId}`, e['ts']);
        break;
      case 'judge_look':
      case 'judge_look_abandoned':
      // THE THIRD TERMINAL STATE. A look ends three ways, not two: it returns (`judge_look`), the call
      // it was reading finishes underneath it (`judge_look_abandoned`), or the judge's own model call
      // fails (`judge_look_failed`, swarm.rs:16454). The last was never folded, so its span stayed open
      // forever and the fleet strip went on labelling a node "Reading · <task>" — attached to whatever
      // it picked up next — for the rest of the run. A supervisor that is not reading anything is the
      // single most misleading thing that strip can say, because it is the label a reader trusts to
      // explain why a lane looks quiet.
      case 'judge_look_failed':
        open.delete(`look:${taskId}`);
        break;
      case 'judge_verdict':
      case 'judge_skipped':
        open.delete(`judge:${taskId}`);
        break;
      case 'task_completed':
        open.delete(`judge:${taskId}`);
        open.delete(`look:${taskId}`);
        break;
      default:
        break;
    }
  }
  return [...open.values()];
}

/** The whole-run planning calls that own no slice and no task. Each writes exactly one digest under this
 *  key, so a node running one is WORKING — the fleet strip reads "idle — no task" for every key missing
 *  from this table, which is what made the Open/Synthesis/Review phases look like a dead fleet. */
export const PLANNING_DIGEST_KEYS = [
  'open',
  'open-resplit',
  'proxy-answer',
  'synthesis',
  'review',
  'rate',
] as const;

/** Planning calls that FAN, so their lane count is a property of the fleet and cannot be a fixed list.
 *
 *  A hardcoded key set goes stale the moment a phase learns to fan, and it did: coverage runs one lane per
 *  host as `open-coverage-1..N` and is the heaviest part of OPEN, yet the panel showed "PLANNING CALLS ·
 *  2 NODES" — only `open` and `open-resplit` — while three coverage lanes were live. They appeared in
 *  FLEET, which reads node state, so the two halves of the same screen disagreed. REVIEW now fans the same
 *  way as `review-1..N` and would have been invisible identically. */
const PLANNING_FAN_PREFIXES = ['open-coverage-', 'review-'] as const;

/** Which fixed key each fan follows, so the lanes render in the order the phases actually run. */
const PLANNING_FAN_AFTER: Record<string, string | undefined> = {
  'open-resplit': 'open-coverage-',
  review: 'review-',
};

export function isPlanningDigestKey(key: string): boolean {
  return (
    (PLANNING_DIGEST_KEYS as readonly string[]).includes(key) ||
    PLANNING_FAN_PREFIXES.some((p) => key.startsWith(p))
  );
}

/** Human label for a digest key that has no lane of its own ('verify::api' -> 'Verifying api'). */
function digestLabel(key: string): string {
  if (key.startsWith('verify-e2e::')) return 'End-to-end verify';
  if (key.startsWith('verify::')) return `Verifying ${key.slice('verify::'.length)}`;
  if (key.startsWith('complete-fix::')) return 'Repairing verify findings';
  if (key.startsWith('slice-')) return `Slice · ${key.slice('slice-'.length)}`;
  if (key.startsWith('open-coverage-'))
    return `Coverage ${key.slice('open-coverage-'.length)} · what the request names that nothing owns`;
  if (key.startsWith('review-'))
    return `Review ${key.slice('review-'.length)} · this part of the request against the whole plan`;
  if (key === 'open') return 'Opening · cutting the request into slices';
  if (key === 'open-resplit') return 'Opening · re-cutting a lopsided slice';
  if (key === 'proxy-answer') return 'Answering the open decisions';
  if (key === 'synthesis') return 'Synthesis · wiring the slices into a task DAG';
  if (key === 'review') return 'Review · the request against the plan';
  if (key === 'rate') return 'Rating each defect critical or minor';
  if (key.startsWith('scout-')) return `Scouting · ${key.slice('scout-'.length)}`;
  if (key.startsWith('contract-')) return `Contract · ${key.slice('contract-'.length)}`;
  if (key.startsWith('detail-')) return `Detailing · ${key.slice('detail-'.length)}`;
  if (/^plandraft-\d+$/.test(key)) return 'Drafting the plan';
  return humanizeTaskId(key);
}

/**
 * The fleet strip's single source of truth — PURE so it is testable.
 *
 * Rows: every node of the RESOLVED POOL renders, idle ones included (absence is not an idle state),
 * plus any lane device the pool missed. WORKING: a node with an open lane per the engine's task
 * lifecycle (build tasks, plan drafts, scouts/contracts/detailers, repair twins), else a node whose
 * activity digest shows an OPEN call — `phase` not stamped 'done' (the engine stamps 'done' the
 * instant a call ends, seeds 'processing' at dispatch, and omits the key mid-stream) — with a fresh
 * file mtime as the crashed-worker guard. This is what makes the strip realtime: the digest is
 * rewritten continuously while a node generates, so a busy node reads WORKING within a poll or two
 * and idle the moment its call closes.
 */
export function deriveFleet(args: {
  pool: string[];
  laneSources: TurnLane[];
  digests: Record<string, unknown>;
  digestMtimes: Record<string, number>;
  now: number;
  /** Open supervision spans (foldSupervision) — judge generations that create no lane. */
  supervision?: SupervisionSpan[];
  /** Nodes LM Studio itself reports generating/prompt-processing — the join that attributes a
   *  supervision span to the node actually running it (the events never name it at start). */
  busyNodes?: string[];
}): {
  devices: string[];
  workingByDevice: Map<string, TurnLane>;
  /** A node's live lanes BEYOND the one in `workingByDevice`. Nodes run PARALLEL: 2, so this is
   *  routinely non-empty and dropping it hid the largest lanes in a run entirely. */
  alsoRunningByDevice: Map<string, TurnLane[]>;
  /** Open supervision spans that could not be pinned to a busy node — still real work; the panel
   *  shows them as an unattributed supervision line rather than dropping them. */
  unattributed: SupervisionSpan[];
} {
  const devices = Array.from(
    new Set([...args.pool, ...args.laneSources.map((l) => l.device)])
  ).sort();
  const workingByDevice = new Map<string, TurnLane>();
  /** The node's OTHER live lanes — see the PARALLEL: 2 note below. */
  const alsoRunningByDevice = new Map<string, TurnLane[]>();
  // A lane the engine OPENED and never closed is a claim, not an observation. It stays 'running'
  // through a re-stream that produced nothing, and through a kill that never got to write a closing
  // event -- so on its own it renders a dead node as working for as long as the panel is open.
  // MEASURED 2026-08-28: gabee showed "Review 1 · working" with a live nudge quoted under it while
  // `lms ps` reported all three nodes IDLE; Mihai saw it and asked. The lane had been re-streamed 13
  // times and its stream was gone.
  //
  // So the claim must be CORROBORATED, and only by evidence we already collect -- no timer is added
  // here, because a clock is what we deleted everywhere else. Demote only when BOTH independent
  // signals disagree with the claim: LM Studio is reporting fleet state at all AND does not list this
  // node as busy, AND the lane's own digest is stale past the open-call window. Either signal alone
  // has demoted a genuinely working node before: mtime did it mid-shell-call, and busyNodes is empty
  // for a cloud device, which never appears in `lms ps`.
  const reportingBusy = args.busyNodes != null && args.busyNodes.length > 0;
  const laneLooksDead = (l: TurnLane): boolean => {
    if (!reportingBusy || (args.busyNodes ?? []).includes(l.device)) return false;
    const d = args.digests[l.taskId] as Digest | undefined;
    const lastCall = d?.calls?.length ? d.calls[d.calls.length - 1] : undefined;
    const callOpen = lastCall != null && (lastCall.ok === null || lastCall.ok === undefined);
    const age = args.now - (args.digestMtimes[l.taskId] ?? 0);
    return age > (callOpen ? DIGEST_OPEN_CALL_FRESH_MS : DIGEST_FRESH_MS);
  };
  // EVERY node runs PARALLEL: 2, so a node routinely has TWO live lanes and this map holds one.
  //
  // Measured on run swarm-20260829-100743413: gabee was running open-coverage-1 (68,393 reasoning
  // characters) alongside slice-index-html, and mihai was running open-coverage-2 (45,712) alongside
  // slice-styles-css. Five live lanes, three cells, and the two BIGGEST lanes in the run had no cell at
  // all -- the fleet strip structurally could not show them, whatever they did. That is the "I cannot
  // see what the nodes are doing" complaint with a mechanism behind it.
  //
  // The primary stays first-wins so the cell's identity is stable between polls; the rest are carried
  // beside it rather than dropped, and the strip renders them under their node.
  for (const l of args.laneSources) {
    if (l.status !== 'running' || laneLooksDead(l)) continue;
    if (!workingByDevice.has(l.device)) {
      workingByDevice.set(l.device, l);
    } else if (workingByDevice.get(l.device)?.taskId !== l.taskId) {
      const also = alsoRunningByDevice.get(l.device) ?? [];
      if (!also.some((x) => x.taskId === l.taskId)) {
        also.push(l);
        alsoRunningByDevice.set(l.device, also);
      }
    }
  }
  // A lane the LIFECYCLE closed (task_completed / fix completed) is over even if its digest predates
  // the phase stamp — engine truth beats file freshness.
  const closed = new Set(
    args.laneSources.filter((l) => l.status !== 'running').map((l) => l.taskId)
  );
  for (const [key, raw] of Object.entries(args.digests)) {
    if (closed.has(key)) continue;
    const d = raw as (Digest & { model?: string }) | undefined;
    const device = shortNode(str(d?.model));
    if (!device || workingByDevice.has(device) || !devices.includes(device)) continue;
    const open = d?.phase !== 'done';
    // The digest's OWN open-call record (a provisional `ok: null` tail entry the engine appends while a
    // tool call is in flight) beats the short mtime window: one long shell call streams no tokens, so the
    // file legitimately sits unmodified past 120s while the node is hard at work. Mtime alone demoted
    // exactly that node to "idle" mid-call.
    const lastCall = d?.calls?.length ? d.calls[d.calls.length - 1] : undefined;
    const callOpen = lastCall != null && (lastCall.ok === null || lastCall.ok === undefined);
    const age = args.now - (args.digestMtimes[key] ?? 0);
    const fresh = age < DIGEST_FRESH_MS || (callOpen && age < DIGEST_OPEN_CALL_FRESH_MS);
    if (!open || !fresh) continue;
    workingByDevice.set(device, {
      taskId: key,
      description: digestLabel(key),
      device,
      model: d?.model,
      status: 'running',
      ...digestStreamFields(key, d),
      seq: 0,
    });
  }
  // SUPERVISION: an open judge span is real work on SOME node, but judge_observed never names it (the
  // engine picks an idle device; only the closing verdict carries judge_node). LM Studio's own busy
  // signal is the join: a node it reports generating, with no lane and no digest, is running exactly
  // this class of call — attach the span there so the busy state always has a visible explanation.
  const liveSpans = (args.supervision ?? []).filter(
    (s) => s.sinceMs == null || args.now - s.sinceMs < JUDGE_SPAN_MAX_MS
  );
  const freeBusy = (args.busyNodes ?? []).filter(
    (n) => devices.includes(n) && !workingByDevice.has(n)
  );
  const unattributed: SupervisionSpan[] = [];
  for (const span of liveSpans) {
    const node = freeBusy.shift();
    if (!node) {
      unattributed.push(span);
      continue;
    }
    workingByDevice.set(node, {
      taskId: `supervision:${span.kind}:${span.taskId}`,
      description: span.label,
      device: node,
      status: 'running',
      phase: 'supervision',
      elapsedMs: span.sinceMs != null ? args.now - span.sinceMs : undefined,
      seq: 0,
    });
  }
  return { devices, workingByDevice, alsoRunningByDevice, unattributed };
}

// Derive the per-phase TODO from the engine's deterministic event stream. Every checkbox is flipped by a
// scheduler/orchestrator EVENT, never by a model claiming it did something — that is what makes it honest.
// The load-bearing rule: a completed build task is 'unverified' (the app was never run), and only Verify's
// complete_result.passed&&verified earns a green 'done'.
export function buildPhaseTodo(
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown>,
  opts: { clarifyPending: boolean }
): PhaseTodo[] {
  let gates: Record<string, unknown> = {};
  let scoutsN: number | null = null;
  let researchQ: number | null = null;
  let researchDone: number | null = null;
  let planConf: number | null = null;
  let taskCount: number | null = null;
  let pillarsN: number | null = null;
  const retargets: Array<{ round: number; action: string }> = [];
  let askedQ: number | null = null;
  let askTimedOut = false;
  let planned = false;
  let planLoaded = false;
  let contractsModules: number | null = null;
  const planTasks: Array<{ id: string; description?: string; files?: string[] }> = [];
  const judgeInfo = new Map<string, { verdict: string; hint: string; action: string }>();
  const tstate = new Map<string, { state: TodoState; device?: string; error?: string }>();
  const judgeFailed = new Set<string>();
  const salvaged = new Set<string>();
  const splitParents = new Set<string>();
  const replans: number[] = [];
  let schedulerStuck: number | null = null;
  const reportFailed = new Set<string>();
  let completeResult: { passed: boolean; verified: boolean; remaining?: number | null } | null =
    null;
  let completeRan = false;
  // THE ENGINE'S OWN RETRACTION OF ITS GREEN, and the only event allowed to make one.
  //
  // `complete_result_revised` is emitted AFTER complete_result by the unwired-module demote (default ON in
  // SwarmConfig, and `review: true` in the shipped user config, so it is reachable on an ordinary run). An
  // ast.parse import-graph walk found a pure library the app builds and nothing imports: it cannot run, so
  // `verified` was never true. No model is in that path, which is why it may overrule a green.
  //
  // MEASURED: the engine emitted this event with NO consumer anywhere — not here, not in the scorer. So the
  // CLI printed "NOT VERIFIED - dead code shipped" while this panel promoted every build row to 'done' and
  // headed the run "Finished - app verified" off the retracted complete_result.
  let completeRevision: { verified: boolean; reason: string; evidence: string[] } | null = null;
  let smoke: { ran: boolean; kind?: string } | null = null;
  let repro: number | null = null;
  let reviewFix: { reproduced: number; accepted: number } | null = null;
  let astReview: number | null = null;
  // --- the rewritten pipeline's own events (see foldRunPhase for the phase mapping) ---
  const phasesSeen = new Set<string>();
  let sliceIds: string[] = [];
  let sliceWeights: number[] = [];
  let openSecs: number | null = null;
  let briefChars: number[] = [];
  let researchSecs: number | null = null;
  let proxyArmed: { mode: string; waitSecs: number; questions: number } | null = null;
  let proxyAnswered = 0;
  let proxyFailed = false;
  let sinkRenamedFrom: string | null = null;
  let synthesisFallback: number | null = null;
  const reviewRounds: Array<{ round: number; fresh: number; touches: number; rejected: boolean }> =
    [];
  let defects: { critical: number; minor: number; forced: number } | null = null;
  let fixWaves = 0;
  // Canonical node names throughout — a raw pool id stored here reaches the node chip and mis-keys its
  // letter/hue against the canonical device order (same defect class as the "fusi, fusi, fable" feed line).
  const nodeOf = nodeLabeler(events);

  for (const e of events) {
    const t = String(e['event'] ?? '');
    if (t === 'run_started' && e['gates'] && typeof e['gates'] === 'object')
      gates = e['gates'] as Record<string, unknown>;
    else if (t === 'phase') phasesSeen.add(str(e['phase']));
    else if (t === 'slices_opened') {
      sliceIds = arr(e['slices']).map(String);
      sliceWeights = arr(e['weights']).map((w) => num(w) ?? 1);
      openSecs = num(e['secs']);
    } else if (t === 'clarify_proxy_armed')
      proxyArmed = {
        mode: str(e['mode']),
        waitSecs: num(e['wait_secs']) ?? 0,
        questions: num(e['questions']) ?? 0,
      };
    else if (t === 'clarify_proxy_answered') proxyAnswered = arr(e['answers']).length;
    else if (t === 'clarify_proxy_failed') proxyFailed = true;
    else if (t === 'synthesis_fallback') synthesisFallback = num(e['tasks']) ?? 0;
    else if (t === 'sink_id_pinned') sinkRenamedFrom = str(e['from']);
    else if (t === 'review_findings')
      reviewRounds.push({
        round: num(e['round']) ?? reviewRounds.length + 1,
        fresh: num(e['new']) ?? 0,
        touches: num(e['patch_touches']) ?? 0,
        rejected: false,
      });
    else if (t === 'plan_patch_rejected') {
      const target = reviewRounds.find((r) => r.round === (num(e['round']) ?? 0));
      if (target) target.rejected = true;
    } else if (t === 'defects_rated')
      defects = {
        critical: num(e['critical']) ?? 0,
        minor: num(e['minor']) ?? 0,
        forced: num(e['engine_forced']) ?? 0,
      };
    else if (t === 'complete_fix_wave' || t === 'spec_repair_wave') fixWaves += 1;
    else if (t === 'scouts_planned') scoutsN = arr(e['lenses']).length || (num(e['count']) ?? 0);
    else if (t === 'research_planned') researchQ = num(e['count']) ?? arr(e['questions']).length;
    else if (t === 'research_completed') {
      // Two shapes share this name: the rewritten engine reports SLICES + per-slice spec sizes; the old one
      // reported a bare findings count. Only the new shape carries `brief_chars`.
      const chars = arr(e['brief_chars']).map((c) => num(c) ?? 0);
      if (chars.length > 0 || e['slices'] != null) {
        briefChars = chars;
        researchSecs = num(e['secs']);
      } else researchDone = num(e['findings']) ?? 0;
    } else if (t === 'pillars') pillarsN = num(e['count']) ?? arr(e['pillars']).length;
    else if (t === 'confidence_retarget')
      retargets.push({ round: num(e['round']) ?? 0, action: str(e['action']) });
    else if (t === 'low_confidence_ask') askedQ = arr(e['questions']).length;
    else if (t === 'low_confidence_ask_timeout') askTimedOut = true;
    else if (t === 'contracts') contractsModules = num(e['modules']) ?? 0;
    else if (t === 'plan_loaded') {
      planLoaded = true;
      planned = true;
      planConf = num(e['plan_confidence']) ?? planConf;
      const ts = arr(e['tasks']) as Array<Record<string, unknown>>;
      taskCount = ts.length;
      for (const tk of ts)
        planTasks.push({
          id: String(tk['id'] ?? ''),
          description: tk['description'] ? String(tk['description']) : undefined,
          files: arr(tk['files']).map((f) => String(f)),
        });
    } else if (t === 'task_dispatched') {
      planned = true;
      const id = str(e['task_id']);
      if (id) tstate.set(id, { state: 'running', device: nodeOf(str(e['device'])) });
    } else if (t === 'task_retry') {
      const id = str(e['task_id']);
      if (id)
        tstate.set(id, { ...(tstate.get(id) ?? { state: 'running' }), error: str(e['error']) });
    } else if (t === 'task_completed') {
      const id = str(e['task_id']);
      if (id) {
        const cur = tstate.get(id);
        const rawDev = str(e['device']);
        const dev = rawDev ? nodeOf(rawDev) : cur?.device;
        if (str(e['status']) === 'failed')
          // CARRY THE REASON. The engine now says WHY a task failed -- `fail_descendants` emits
          // `error: "dependency 'x' failed"` -- and this handler was dropping it on the floor, so the
          // row at :3246 (`else if (s.error) detail = ...`) had nothing to render and a cascade-failed
          // task read as a bare red row with no explanation. `cur?.error` is the fallback so a reason
          // already recorded from an earlier attempt is not erased by a later completion event.
          tstate.set(id, {
            device: dev,
            state: judgeFailed.has(id) ? 'judge_failed' : 'failed',
            error: str(e['error']) || cur?.error,
          });
        // BUILT but UNVERIFIED — the worker loop returned + passed a syntax gate; the app was NOT run.
        else tstate.set(id, { device: dev, state: 'unverified' });
      }
    } else if (t === 'judge_verdict') {
      const id = str(e['task_id']);
      const a = str(e['action']);
      if (a === 'failed') judgeFailed.add(id);
      if (a === 'salvaged') salvaged.add(id);
      // A SPLIT decomposes this task into freshly-dispatched children; the parent never gets a task_completed,
      // so without this it would render 'running' forever (a stuck spinner on a done run). Mark it superseded.
      if (a === 'split') splitParents.add(id);
      // Keep the judge's REASONING so the row can explain WHY it intervened. Prefer an actionable decision
      // (re_dispatch/failed/salvaged/split) over a passive 'observed'; keep the latest of those.
      const verdict = str(e['verdict']);
      const hint = str(e['hint']);
      const prev = judgeInfo.get(id);
      if (!prev || a !== 'observed' || prev.action === 'observed')
        judgeInfo.set(id, { verdict, hint, action: a });
    } else if (t === 'replanned') {
      const added = arr(e['added']).length;
      if (added > 0) replans.push(added);
    } else if (t === 'scheduler_stuck') schedulerStuck = num(e['remaining']) ?? 0;
    else if (t === 'complete_verify') completeRan = true;
    else if (t === 'complete_result')
      completeResult = {
        passed: e['passed'] === true,
        verified: e['verified'] === true,
        remaining: num(e['remaining_findings']),
      };
    else if (t === 'complete_result_revised') {
      // Always AFTER complete_result in the stream (the engine emits it from the same block, downstream of
      // the claim), so folding it onto completeResult here is enough to correct every derived verdict at
      // once: the v-e2e row, the d-outcome headline, and the e2eVerified promotion of built tasks.
      // `passed` is deliberately NOT touched — the engine never flips it red either, because false-failing a
      // correct app costs the whole run while an honest UNVERIFIED slate costs nothing.
      completeRevision = {
        verified: e['verified'] === true,
        reason: str(e['reason']),
        evidence: arr(e['evidence']).map(String),
      };
      if (completeResult) completeResult.verified = completeRevision.verified;
    } else if (t === 'smoke' || t === 'smoke_after_fix') {
      const r = (e['result'] ?? {}) as Record<string, unknown>;
      const tests = (r['tests'] ?? {}) as Record<string, unknown>;
      smoke = { ran: r['ran'] === true, kind: str(tests['kind']) };
    } else if (t === 'review_repro') repro = num(e['reproduced']) ?? repro;
    else if (t === 'review_fix_summary')
      reviewFix = { reproduced: num(e['reproduced']) ?? 0, accepted: num(e['accepted']) ?? 0 };
    else if (t === 'review' || t === 'review_after_fix')
      astReview = num(e['new_findings']) ?? arr(e['new_findings']).length ?? astReview;
  }

  // Second pass: task_completed(failed) may precede its judge_verdict in some orderings — reconcile.
  for (const [id, s] of tstate)
    if (s.state === 'failed' && judgeFailed.has(id)) s.state = 'judge_failed';

  // Once the engine's OWN end-to-end verify passes green (deterministic complete_result — not a model claim),
  // the built tasks it exercised are verified. Promote 'unverified' -> 'done' so the Build checklist stops
  // contradicting the green Verify/Done rows. This is still engine-truth: it flips ONLY on complete_result.
  // Mid-run (no completeResult yet, or an unverified/failed ship) the tasks correctly stay 'unverified'.
  const e2eVerified = !!completeResult && completeResult.passed && completeResult.verified;
  if (e2eVerified) for (const [, s] of tstate) if (s.state === 'unverified') s.state = 'done';

  const plandraftN = Object.keys(activity).filter((k) => /^plandraft-\d+$/.test(k)).length;
  const gateOn = (k: string) => gates[k] !== false;
  // DELIVERY hard-completion gate (GOOSE_SWARM_DELIVERY, default OFF). Unlike `gateOn` (which defaults a
  // missing key to ON), this defaults to OFF — an old run without the key, or the gate off, keeps today's
  // label logic byte-for-byte.
  const deliveryOn = gates['delivery'] === true;

  const it = (
    id: string,
    label: string,
    state: TodoState,
    detail?: string,
    device?: string
  ): PhaseTodoItem => ({ id, label, state, detail, device, advisory: state === 'advisory' });

  // ---- OPEN ---- (one node cuts the request into balanced semantic slices, and names what it cannot decide)
  const open: PhaseTodoItem[] = [];
  const openRan = phasesSeen.has('open') || sliceIds.length > 0;
  if (openRan) {
    open.push(it('o-start', 'Fleet configured, run started', 'done'));
    if (sliceIds.length > 0) {
      const lopsided =
        sliceWeights.length > 1 && Math.max(...sliceWeights) > 2 * Math.min(...sliceWeights);
      open.push(
        it(
          'o-slices',
          `Request cut into ${sliceIds.length} slice${sliceIds.length === 1 ? '' : 's'}`,
          'done',
          [
            sliceIds.map((id, i) => `${id} (w${sliceWeights[i] ?? 1})`).join(', '),
            // An uneven cut costs queue time — one node finishes early and waits. Worth saying, never a failure.
            lopsided ? 'uneven — the heaviest slice is more than twice the lightest' : '',
            openSecs != null ? `${openSecs}s` : '',
          ]
            .filter(Boolean)
            .join(' · ')
        )
      );
    } else open.push(it('o-run', 'Cutting the request into slices…', 'running'));
  } else if (!planned) open.push(it('o-start', 'Fleet configured, run started', 'done'));

  // ---- ASK ---- (the opener's own open decisions; the engine emits the phase only when there are any)
  const ask: PhaseTodoItem[] = [];
  if (proxyArmed) {
    // A QUESTION IS ALWAYS ANSWERED — by you, or by goose from the spec. Which of those happened is the
    // fact the checklist must carry: a run that answered itself is not a run someone steered.
    const answeredByProxy = proxyAnswered > 0 || proxyFailed;
    ask.push(
      it(
        'a-ask',
        `${proxyArmed.questions} open decision${proxyArmed.questions === 1 ? '' : 's'}`,
        opts.clarifyPending && !answeredByProxy ? 'running' : 'done',
        answeredByProxy
          ? proxyFailed
            ? 'the proxy answer failed — goose took the most conventional option'
            : 'answered by goose — you did not reply'
          : opts.clarifyPending
            ? proxyArmed.mode === 'immediate'
              ? 'unattended run — goose is answering these'
              : `waiting on you — goose answers in ${Math.round(proxyArmed.waitSecs / 60)} min`
            : 'answered by you'
      )
    );
  } else if (askedQ != null)
    // An OLD run has no phase event and no proxy — its ask is the bare low_confidence_ask.
    ask.push(
      it(
        'a-ask-legacy',
        `Asked you ${askedQ} question${askedQ === 1 ? '' : 's'}`,
        opts.clarifyPending && !askTimedOut ? 'running' : 'done',
        askTimedOut
          ? 'unanswered at the unattended window — conventional options chosen, folded into every brief'
          : opts.clarifyPending
            ? 'waiting on your answer'
            : undefined
      )
    );
  else if (phasesSeen.has('ask')) ask.push(it('a-run', 'Naming the open decisions…', 'running'));
  // The engine skips ASK outright when the opener has no open decisions, so a run past Open with no ask
  // event did not ask — say that, rather than leaving a gap the ribbon marks skipped and the list omits.
  else if (openRan && (phasesSeen.has('research') || planned))
    ask.push(it('a-none', 'No open decisions — nothing to ask', 'skipped'));

  // ---- RESEARCH ---- (RETIRED: deleted from the engine by P1-5 — historical rows for archived runs only)
  const legacyResearch = scoutsN != null || researchQ != null || researchDone != null;
  const research: PhaseTodoItem[] = [];
  if (briefChars.length > 0) {
    const total = briefChars.reduce((a, b) => a + b, 0);
    research.push(
      it(
        'r-specs',
        `Slice specs written — ${briefChars.length} of ${sliceIds.length || briefChars.length}`,
        'done',
        [`${total.toLocaleString()} chars of spec`, researchSecs != null ? `${researchSecs}s` : '']
          .filter(Boolean)
          .join(' · ')
      )
    );
    // An EMPTY brief is a slice whose owner returned nothing — the module is unspecified and the synthesis
    // will have to invent it. Say so; it never shows up as a failure anywhere else.
    const empty = briefChars.filter((c) => c === 0).length;
    if (empty > 0)
      research.push(
        it(
          'r-empty',
          `${empty} slice${empty === 1 ? '' : 's'} came back with no spec`,
          'unverified'
        )
      );
  } else if (phasesSeen.has('research')) {
    research.push(
      it(
        'r-run',
        `Every node writing its slice's spec${sliceIds.length ? ` — ${sliceIds.length} slices` : ''}`,
        'running'
      )
    );
  } else if (legacyResearch) {
    if (scoutsN != null)
      research.push(it('r-scouts', `Scouts dispatched — ${scoutsN} lenses`, 'done'));
    else if (researchQ != null)
      research.push(it('r-q', `Research questions scoped — ${researchQ}`, 'done'));
    if (researchDone != null)
      research.push(it('r-done', `Research finished — ${researchDone} findings returned`, 'done'));
    else research.push(it('r-legacy-run', 'Researching…', 'running'));
  }
  // No `planned -> skipped` placeholder any more: RESEARCH is deleted from the engine (P1-5), so a new
  // run gets NO research row at all — an empty items list drops the chip. The branches above stay so an
  // ARCHIVED run that ran research still renders its history.

  // ---- SYNTHESIS ---- (one node wires the researched slices into a task DAG; the specs splice in verbatim)
  const synthesis: PhaseTodoItem[] = [];
  if (synthesisFallback != null) {
    // NOT a failure: the fallback IS a valid plan — one task per slice, each carrying its owner's brief.
    // Flatter and more serial than a good synthesis, but every module is still specified and owned.
    synthesis.push(
      it(
        's-fallback',
        `Synthesis didn't return — one task per slice instead (${synthesisFallback})`,
        'unverified',
        'flatter and more serial; every module still specified and owned'
      )
    );
  } else if (phasesSeen.has('synthesis') && !planLoaded && !phasesSeen.has('review'))
    synthesis.push(it('s-run', 'Wiring the slices into a task DAG…', 'running'));
  // SYNTHESIS ENDS WHEN REVIEW OPENS, NOT WHEN THE PLAN LOADS. `plan_loaded` is emitted only after REVIEW
  // has finished patching the DAG, so gating this row on it made Synthesize render as still-running for the
  // whole of Review — on EVERY run, which read as the two phases executing in parallel. They never do:
  // measured 07:04:52 phase=synthesis -> 07:08:27 phase=review, strictly sequential.
  else if (phasesSeen.has('review') && !planLoaded)
    synthesis.push(
      it(
        's-wired',
        'Slices wired into a DAG',
        'done',
        'review is still patching it, so the task count lands with the plan'
      )
    );
  if (sinkRenamedFrom)
    synthesis.push(
      it(
        's-sink',
        `Sink renamed from \`${sinkRenamedFrom}\``,
        'done',
        'so the engine’s own sink checks keep matching'
      )
    );
  if (planLoaded) synthesis.push(it('s-done', `Plan wired — ${taskCount ?? 0} tasks`, 'done'));
  // Legacy plan-phase evidence (candidate drafts, the confidence score, retarget rounds) has no phase of its
  // own any more — the multi-draft vote and the redraft ladder were deleted. Keep it here so an OLD run still
  // renders its planning history instead of dropping it on the floor.
  if (plandraftN > 0)
    synthesis.push(
      it(
        's-draft',
        `Drafting — ${plandraftN} candidate${plandraftN === 1 ? '' : 's'}`,
        planned ? 'done' : 'running'
      )
    );
  if (planConf != null) synthesis.push(it('s-conf', `Confidence scored — ${planConf}/100`, 'done')); // the NUMBER, never a verdict
  for (const r of retargets)
    synthesis.push(it(`s-rt-${r.round}`, `Retarget round ${r.round} — ${r.action}`, 'done'));

  // ---- REVIEW ---- (structural patches only; it stops when it requests no change)
  const review: PhaseTodoItem[] = [];
  for (const r of reviewRounds) {
    // The honest measure is what the patch TOUCHED. A round can raise well-worded observations and request
    // nothing — the engine settles on exactly that, because commentary is not a reason to spend a round.
    review.push(
      it(
        `rv-${r.round}`,
        r.touches > 0
          ? `Round ${r.round} — patched ${r.touches} task${r.touches === 1 ? '' : 's'}`
          : `Round ${r.round} — settled, no change requested`,
        r.rejected ? 'unverified' : 'done',
        [
          r.fresh > 0 ? `${r.fresh} new finding${r.fresh === 1 ? '' : 's'}` : '',
          r.rejected ? 'patch rejected — dropped, plan unchanged' : '',
        ]
          .filter(Boolean)
          .join(' · ') || undefined
      )
    );
  }
  if (reviewRounds.length === 0 && phasesSeen.has('review') && !planLoaded)
    review.push(it('rv-run', 'Reading the request against the plan…', 'running'));

  // ---- CONTRACTS ---- (RETIRED: deleted from the engine by P1-4 — historical rows for archived runs only;
  // the pillars distillation rode the same pre-EXECUTE gap and its row stays for the same reason)
  const contracts: PhaseTodoItem[] = [];
  if (contractsModules != null)
    contracts.push(it('c-frozen', `Frozen interfaces — ${contractsModules} modules`, 'done'));
  else if (phasesSeen.has('contracts'))
    contracts.push(it('c-run', 'Every node freezing one module’s interface…', 'running'));
  // No `planned -> skipped` placeholder any more: CONTRACTS is deleted from the engine (P1-4 — the
  // 2,527-char contract that silently dropped meridian/viz/static was the causal origin of r2's
  // GET / -> 404), so a new run gets NO contracts row — an empty items list drops the chip. The
  // branches above stay so an ARCHIVED run that froze interfaces still renders its history.
  if (pillarsN != null)
    contracts.push(
      it(
        'c-pillars',
        `Quality pillars distilled — ${pillarsN}`,
        'done',
        'injected into every worker before Build'
      )
    );

  // ---- BUILD ---- (per plan task — surfaces PENDING + BLOCKED tasks lanes can't show)
  const build: PhaseTodoItem[] = [];
  const plannedIds = new Set(planTasks.map((tk) => tk.id));
  const buildRow = (id: string, description?: string, files?: string[]) => {
    const s = tstate.get(id);
    let state: TodoState;
    let detail: string | undefined;
    if (splitParents.has(id)) {
      // Decomposed by the judge — the children (dispatched below) carry the real work. Never leave it 'running'.
      state = 'skipped';
      detail = 'split into sub-tasks';
    } else if (s) {
      state = s.state;
      // 'done' in Build only ever means "promoted after the green e2e verify" (build tasks never self-complete
      // to green). Say so honestly — the verification was at the app level, not this unit grading itself.
      if (state === 'done')
        detail = salvaged.has(id) ? 'salvaged · verified end-to-end' : 'verified end-to-end';
      else if (state === 'unverified' && salvaged.has(id)) detail = 'salvaged — judge cut a loop';
      // SAY WHAT IT IS WAITING FOR. 'unverified' is a pipeline STAGE, not a failure: the worker returned
      // and the file passed a syntax gate, but nobody has RUN the app yet. Mihai read a board of them as
      // things having gone wrong, which is fair — a bare negative word with no explanation reads as one.
      //
      // The promotion is real and it is engine truth, not a model claim: complete_result.passed &&
      // verified, at the end of REPAIR. It is deliberately the ONLY trigger. The standalone `smoke` gate
      // looked like an earlier one and is not — it runs only when GOOSE_SWARM_COMPLETE is off, which it
      // never is on these runs, so no run has ever emitted it. Promoting on anything weaker would launder
      // work nobody ran into "verified", which is the one thing this state exists to prevent.
      else if (state === 'unverified')
        detail = 'built — the app has not been run yet; verified end-to-end after Repair';
      else if (state === 'judge_failed') detail = 'judge decision';
      else if (s.error) detail = s.error.slice(0, 80);
    } else if (reportFailed.has(id) || schedulerStuck != null) {
      state = 'blocked';
      detail = schedulerStuck != null ? 'scheduler stuck' : 'a dependency failed';
    } else {
      state = 'pending';
    }
    // TITLE = the stable, readable task id; SUMMARY = a short human line; the FULL description / files / judge
    // reasoning are carried for the expand. This is the redesign: clean row collapsed, everything else tucked.
    const item = it(`b-${id}`, humanizeTaskId(id), state, detail, s?.device);
    const summary = taskSummary(description, id, files);
    if (summary) item.summary = summary;
    if (description && description.trim()) item.description = description.trim();
    if (files && files.length) item.files = files;
    const judge = judgeInfo.get(id);
    if (judge && (judge.verdict || judge.hint)) item.judge = judge;
    return item;
  };
  // integrate-verify is the SINK — its work IS the INTEGRATE phase, so it is added to `integrate` below, NOT
  // here (otherwise Build carried a stuck 12th row and read "Building" while the run was actually integrating).
  for (const tk of planTasks)
    if (tk.id !== 'integrate-verify') build.push(buildRow(tk.id, tk.description, tk.files));
  // Split children (+ any other dynamically-dispatched task) are NOT in planTasks — surface them so the todo
  // reflects the work that actually ran, not just the original plan. Keep them right after their siblings.
  for (const [id] of tstate)
    if (!plannedIds.has(id) && id !== 'integrate-verify') build.push(buildRow(id));
  for (const n of replans) build.push(it(`b-replan-${n}`, `Re-planned +${n} tasks`, 'done'));
  if (schedulerStuck != null)
    build.push(
      it('b-stuck', `Scheduler blocked — ${schedulerStuck} task(s) unschedulable`, 'blocked')
    );

  // ---- INTEGRATE ---- (the sink assembles the modules, runs the suite, boots what the request advertises)
  const integrate: PhaseTodoItem[] = [];
  // integrate-verify is the SINK — the run's actual integration work. It LEADS this phase, reusing the same
  // state logic as a build task, so Integrate shows RUNNING while it grinds instead of the run reading
  // "Building 11/12" with a stuck integrate-verify row.
  if (plannedIds.has('integrate-verify') || tstate.has('integrate-verify')) {
    const ivTask = planTasks.find((t) => t.id === 'integrate-verify');
    integrate.push(buildRow('integrate-verify', ivTask?.description, ivTask?.files));
  }
  const verifyGate = gateOn('complete') || gateOn('smoke');
  if (verifyGate) {
    let vs: TodoState;
    let vdetail: string | undefined;
    if (completeResult) {
      vs = completeResult.passed ? (completeResult.verified ? 'done' : 'unverified') : 'failed';
      vdetail = completeResult.passed
        ? completeResult.verified
          ? 'app runs — verified end-to-end'
          : 'shipped — no oracle ran'
        : `${completeResult.remaining ?? 0} findings remain`;
    } else if (completeRan || smoke) vs = 'running';
    else vs = 'pending';
    // A would-be pass with no real test run is downgraded and never reads "tests pass".
    if (vs === 'done' && smoke && smoke.kind !== 'pass') {
      vs = 'unverified';
      vdetail = 'no tests ran';
    }
    // The engine RETRACTED verified after the claim. The row that states the verdict is the row that must
    // carry the reason — a downgrade with no cause reads as a missing oracle, which is a different thing.
    if (completeRevision && !completeRevision.verified) {
      if (vs === 'done') vs = 'unverified';
      vdetail =
        completeRevision.reason === 'unwired-module-unfixed'
          ? `dead code shipped — built but imported by nothing: ${completeRevision.evidence.join(', ') || 'a module'}`
          : `verified retracted by the engine — ${completeRevision.reason || 'revised'}`;
    }
    integrate.push(it('v-e2e', 'End-to-end verify', vs, vdetail));
  }

  // ---- REPAIR ---- (defects found at integration are rated, repaired, and checked again)
  const repair: PhaseTodoItem[] = [];
  if (defects) {
    // A green run with minors is still GREEN. The critical count is the verdict; the minors are the KNOWN
    // ACTIVE BUGS the run shipped with, and they get their own surface — never a red mark here.
    repair.push(
      it(
        'x-rated',
        defects.critical === 0
          ? `Every critical defect closed — ${defects.minor} known active bug${defects.minor === 1 ? '' : 's'}`
          : `${defects.critical} critical defect${defects.critical === 1 ? '' : 's'} remain`,
        defects.critical === 0 ? 'done' : 'failed',
        defects.forced > 0
          ? `${defects.forced} forced critical by the engine, not the rater`
          : `${defects.minor} minor`
      )
    );
  }
  if (fixWaves > 0)
    repair.push(it('x-waves', `Repair wave${fixWaves === 1 ? '' : 's'} — ${fixWaves}`, 'done'));
  if (repro != null)
    repair.push(it('v-repro', `Repro gate — ${repro} findings reproduced`, 'done'));
  if (reviewFix)
    repair.push(
      it(
        'v-fix',
        `Review fixes — ${reviewFix.accepted} accepted / ${reviewFix.reproduced} reproduced`,
        'done'
      )
    );
  if (astReview != null)
    repair.push(it('v-ast', `Unwired-module review — ${astReview} new findings`, 'done'));

  // ---- DONE ----
  const done: PhaseTodoItem[] = [];
  const finishedEvent = events.some((e) => e['event'] === 'run_finished');
  if (schedulerStuck != null)
    done.push(it('d-blocked', 'Run blocked — scheduler deadlocked', 'blocked'));
  else if (finishedEvent) {
    // 'failed' is the DETERMINISTIC-BLOCK lane — a file-owning task the engine failed. 'judge_failed' is the
    // model JUDGE (owns-nothing integrate-verify sink) and is INFORMATIONAL: it never counts as a hard fail.
    const anyHardFail = [...tstate.values()].some((s) => s.state === 'failed');
    const e2eGreen = completeResult ? completeResult.passed && completeResult.verified : false;
    if (deliveryOn) {
      // HARD COMPLETION GATE: "app verified" requires the green e2e verdict AND no coexisting deterministic-
      // block failure — a failed deterministic check can never sit beside a green claim. A judge dissent on an
      // otherwise-green gate is surfaced, but as informational (reachable only because the engine's C1 keeps
      // the owns-nothing judge out of the green-blocking set).
      const green = e2eGreen && !anyHardFail;
      const judgeDissent = green && [...tstate.values()].some((s) => s.state === 'judge_failed');
      done.push(
        it(
          'd-outcome',
          green
            ? judgeDissent
              ? 'Finished — app verified (judge dissent — informational)'
              : 'Finished — app verified'
            : anyHardFail
              ? 'Finished — with failures'
              : 'Finished — unverified',
          green ? 'done' : anyHardFail ? 'failed' : 'unverified'
        )
      );
    } else {
      const green = e2eGreen;
      done.push(
        it(
          'd-outcome',
          green
            ? 'Finished — app verified'
            : anyHardFail
              ? 'Finished — with failures'
              : 'Finished — unverified',
          green ? 'done' : anyHardFail ? 'failed' : 'unverified'
        )
      );
    }
  }

  const rollUp = (items: PhaseTodoItem[]): TodoState => {
    const real = items.filter((i) => i.state !== 'advisory');
    if (real.length === 0) return 'pending';
    // A phase whose work has NOT STARTED — every real item is still 'pending', nothing has run — is WAITING,
    // not running. This is the fix for Verify reading RUNNING while Build is still going: the sink
    // (integrate-verify) and the e2e row are both 'pending' until Build finishes and the sink dispatches, so
    // without this guard the roll-up below painted Verify as RUNNING alongside Build.
    const started = real.some((i) => i.state !== 'pending');
    if (!finishedEvent && !started) return 'pending';
    // While the run is still LIVE and any task is running/pending, the phase is IN-PROGRESS — a single failed
    // sibling must NOT flip the whole phase to FAILED mid-build (it may re-dispatch, split, or the run still
    // recovers via complete_verify). Only once the run cleanly finished does an unrecovered failure roll up.
    // (On a crashed/stale run the panel relabels a 'running' phase to 'interrupted' separately.)
    const anyActive = real.some((i) => i.state === 'running' || i.state === 'pending');
    if (!finishedEvent && anyActive) return 'running';
    if (
      real.some((i) => i.state === 'failed' || i.state === 'judge_failed' || i.state === 'blocked')
    )
      return 'failed';
    if (real.some((i) => i.state === 'running')) return 'running';
    if (real.every((i) => i.state === 'skipped')) return 'skipped';
    if (real.some((i) => i.state === 'done')) return 'done';
    if (real.some((i) => i.state === 'unverified')) return 'unverified';
    return 'pending';
  };
  const mk = (key: PhaseKey, label: string, items: PhaseTodoItem[]): PhaseTodo => ({
    key,
    label,
    items,
    state: rollUp(items),
    active: false,
    counts: {
      // FINISHED work, not VERIFIED work. These are different questions and this counter answers the first.
      //
      // It used to count `state === 'done'` only. But a completed build task is deliberately 'unverified'
      // (the worker returned and passed a syntax gate; the app was never RUN — only Verify's green promotes
      // it), so `done` was 0 for every task that had actually finished. The ONLY Build row born 'done' is the
      // `Re-planned +N tasks` bookkeeping row — which made the numerator literally count REPLANS.
      //
      // MEASURED on loop-ab-baseline: db 16:56:37, entry-point 16:59:01, ledger 17:00:46, models 17:02:37,
      // frontend 17:07:14, api 17:08:52 — six tasks finished, and the panel read "Build 0/7" for 15 minutes.
      // It moved to 1/10 at 17:09:46, when `replanned` fired: the one time the number moved, it moved because
      // work was ADDED. A progress counter that only advances when the job grows is not a progress counter.
      //
      // 'unverified' counts as finished; the ROW still renders neutral and rollUp still decides the phase's
      // colour off the real states, so this cannot manufacture a green claim — it only stops the panel
      // reporting 0 while six of seven tasks are done.
      done: items.filter((i) => isFinishedWork(i.state)).length,
      // exclude advisory (info-only) AND skipped (off / superseded, e.g. a split parent) from the denominator
      total: items.filter((i) => i.state !== 'advisory' && i.state !== 'skipped').length,
    },
  });
  // ENGINE ORDER — the list is read as a sequence, and it must carry every phase the engine announces.
  // `research` and `contracts` stay in the sequence at their historical position: their items are empty
  // unless the (archived) run actually emitted their events, and the PlanningZone drops empty phases, so
  // a new run never shows either chip while an old run.jsonl renders exactly what it ran.
  const phases: PhaseTodo[] = [
    mk('open', 'Open', open),
    mk('ask', 'Ask', ask),
    mk('research', 'Research', research),
    mk('synthesis', 'Synthesize', synthesis),
    mk('review', 'Review', review),
    mk('contracts', 'Contracts', contracts),
    mk('build', 'Build', build),
    mk('integrate', 'Integrate', integrate),
    mk('repair', 'Repair', repair),
    mk('done', 'Done', done),
  ];
  // Active = the last phase that has started (any non-pending item) and isn't fully done — monotonic pipeline.
  let activeIdx = -1;
  phases.forEach((p, i) => {
    if (p.items.some((x) => x.state !== 'pending')) activeIdx = i;
  });
  if (activeIdx >= 0) phases[activeIdx].active = true;
  return phases;
}

/** Readable WORK-board title for a task id — names the verify/repair machinery for what it is instead of
 *  echoing its raw id ('verify::api' -> 'Verify api', 'complete-fix::twin2' -> 'Repair twin 2'). */
export function boardTitle(id: string): string {
  if (id === 'integrate-verify') return 'Integrate & verify';
  if (id.startsWith('verify-e2e::')) return `End-to-end verify ${id.slice('verify-e2e::'.length)}`;
  if (id.startsWith('verify::')) return `Verify ${id.slice('verify::'.length)}`;
  if (id.startsWith('complete-fix::twin'))
    return `Repair twin ${id.slice('complete-fix::twin'.length)}`;
  return humanizeTaskId(id);
}

/** One row of the WORK board — a unit of work the run planned, is doing, or finished. */
export interface BoardRow {
  id: string;
  title: string;
  summary?: string;
  /** Engine-truth state (phase-todo / task lifecycle) — never a model claim. */
  state: TodoState;
  kind: 'build' | 'verify' | 'repair';
  detail?: string;
  deps: string[];
  difficulty?: string;
  files?: string[];
  description?: string;
  judge?: { verdict: string; hint: string; action: string };
  /** The task's lane (canonical device, live calls/reasoning, elapsed/attempts) when one exists. */
  lane?: TurnLane;
  device?: string;
  elapsedMs?: number;
  attempts?: number;
}

export interface TaskBoard {
  running: BoardRow[];
  queued: BoardRow[];
  done: BoardRow[];
  /** Tasks added by dynamic re-planning — header bookkeeping, not board rows. */
  addedByReplan: number;
  /** Set when the scheduler deadlocked — surfaced as a banner, never a silent row. */
  stuck: string | null;
}

/**
 * The WORK zone's single source of truth: the plan + task lifecycle folded into ONE board of three groups —
 * RUNNING (live), QUEUED (planned, waiting on deps), DONE (finished, failures distinct). This is the
 * de-duplication fix: build tasks used to appear three times (phase-checklist rows, per-task lanes, feed
 * lines) with no statement of which was authoritative. Every row keeps its engine-truth TodoState (a
 * finished build task is 'unverified', never green) and carries its lane so the tool-call/reasoning card is
 * the row's own expansion instead of a parallel list. Pure + exported for unit tests.
 */
export function deriveTaskBoard(args: {
  plan: PlanTask[];
  phaseTodo: PhaseTodo[];
  lanes: TurnLane[];
  fixLanes: TurnLane[];
}): TaskBoard {
  const laneById = new Map(args.lanes.map((l) => [l.taskId, l]));
  const planById = new Map(args.plan.map((t) => [t.id, t]));
  const rows: BoardRow[] = [];
  const seen = new Set<string>();
  let addedByReplan = 0;
  let stuck: string | null = null;
  for (const phase of args.phaseTodo) {
    if (phase.key !== 'build' && phase.key !== 'integrate' && phase.key !== 'repair') continue;
    for (const item of phase.items) {
      if (/^b-replan-/.test(item.id)) {
        addedByReplan += Number(item.id.slice('b-replan-'.length)) || 0;
        continue;
      }
      if (item.id === 'b-stuck') {
        stuck = item.label;
        continue;
      }
      if (item.state === 'advisory') continue;
      const isTask = item.id.startsWith('b-');
      const id = isTask ? item.id.slice(2) : item.id;
      if (seen.has(id)) continue;
      seen.add(id);
      const lane = laneById.get(id);
      const pt = planById.get(id);
      rows.push({
        id,
        // Verdict rows (v-e2e, v-repro…) already carry a good label; task rows get the readable title.
        title: isTask ? boardTitle(id) : item.label,
        summary: item.summary,
        state: item.state,
        kind:
          phase.key === 'repair'
            ? 'repair'
            : isTask && !/^verify/.test(id) && id !== 'integrate-verify'
              ? 'build'
              : 'verify',
        detail: item.detail,
        deps: pt?.deps ?? [],
        difficulty: pt?.difficulty || undefined,
        files: item.files ?? pt?.files,
        description: item.description ?? pt?.description,
        judge: item.judge,
        lane,
        device: lane?.device ?? item.device,
        elapsedMs: lane?.elapsedMs,
        attempts: lane?.attempts,
      });
    }
  }
  // Verify repair twins run OUTSIDE the task lifecycle (complete_fix_*) — fold their lanes in as repair
  // rows so the board covers everything the fleet is actually grinding.
  for (const l of args.fixLanes) {
    if (seen.has(l.taskId)) continue;
    seen.add(l.taskId);
    rows.push({
      id: l.taskId,
      title: boardTitle(l.taskId),
      summary: l.description,
      state: l.status === 'running' ? 'running' : l.status === 'error' ? 'failed' : 'done',
      kind: 'repair',
      deps: [],
      lane: l,
      device: l.device,
      elapsedMs: l.elapsedMs,
      attempts: l.attempts,
    });
  }
  const groupOf = (r: BoardRow): 'running' | 'queued' | 'done' =>
    r.state === 'running'
      ? 'running'
      : r.state === 'pending' || r.state === 'blocked'
        ? 'queued'
        : 'done';
  const running = rows.filter((r) => groupOf(r) === 'running');
  const done = rows.filter((r) => groupOf(r) === 'done');
  // RUNNING in dispatch order, DONE in completion order (lane seq); rows with no lane (skipped split
  // parents, verify verdict rows) sink to the end. QUEUED keeps plan order — that IS the plan.
  running.sort((a, b) => (a.lane?.seq ?? 0) - (b.lane?.seq ?? 0));
  done.sort(
    (a, b) => (a.lane?.seq ?? Number.MAX_SAFE_INTEGER) - (b.lane?.seq ?? Number.MAX_SAFE_INTEGER)
  );
  return {
    running,
    queued: rows.filter((r) => groupOf(r) === 'queued'),
    done,
    addedByReplan,
    stuck,
  };
}

/** A human name for WHAT this run is building — the RUN HEADER's identity. From the brief's first heading
 *  ('# Build `vendorsync`' -> 'vendorsync'), else the run directory's basename. Pure + exported for tests. */
export function runAppName(prompt: string | undefined, runDir: string | null | undefined): string {
  const line =
    (prompt ?? '')
      .split('\n')
      .find((l) => l.trim().length > 0)
      ?.trim() ?? '';
  const heading = line.match(/^#+\s*(?:build\s+)?(.+)$/i)?.[1] ?? '';
  const name = heading.replace(/[`*_"']/g, '').trim();
  if (name) return name.length > 48 ? name.slice(0, 45).trimEnd() + '…' : name;
  const base = (runDir ?? '').replace(/\/+$/, '').split('/').pop() ?? '';
  return base || 'build';
}

export function useSwarmRun(workingDir: string | undefined, pollMs = 500): SwarmRunState {
  const [state, setState] = useState<SwarmRunState>(EMPTY);
  // Keep the last non-empty run visible between polls so a finished run does not flicker away.
  const lastRunId = useRef<string | null>(null);

  useEffect(() => {
    if (!workingDir) {
      setState({ ...EMPTY, loading: false });
      return;
    }
    let alive = true;

    const read = async () => {
      try {
        const data = await window.electron.readSwarmRun(workingDir);
        if (!alive) return;
        if (!data) {
          setState({ ...EMPTY, loading: false });
          lastRunId.current = null;
          return;
        }
        const {
          activity,
          verbose,
          meta,
          plan,
          smoke,
          phase,
          finished,
          planConfidence,
          confidence,
          askFloor,
          confidenceTrail,
          summary,
          startedAt,
          overview,
          slices,
          proxy,
          reviewRounds,
          sinkRenamedFrom,
          synthesisFallback,
          knownActiveBugs,
        } = buildActivity(data.events);
        // Gated BEFORE the fold, so a previous run's leftover digest cannot mint a lane, claim a node or
        // stamp a checklist row anywhere downstream. Every consumer below reads the gated pair.
        const rawMtimes = data.activityMtimes ?? {};
        const digests = digestsFromThisRun(data.activity, rawMtimes, startedAt);
        const digestMtimes = digestsFromThisRun(rawMtimes, rawMtimes, startedAt);
        const {
          lanes,
          totals,
          planLanes,
          scoutLanes,
          contractLanes,
          detailLanes,
          sliceLanes,
          planningLanes,
          fixLanes,
        } = foldEventsIncremental(
          data.events,
          digests,
          // main's key, never a fingerprint the renderer computed for itself: with the same runId and the
          // same generation the array IS the previous one extended, so only the appended events are folded.
          typeof data.generation === 'number'
            ? { runId: data.runId, generation: data.generation }
            : null
        );
        const phaseTodo = buildPhaseTodo(data.events, digests, {
          clarifyPending: !!data.clarify?.pending,
        });
        const { phase: runPhase, observed: runPhasesObserved } = foldRunPhase(data.events);
        lastRunId.current = data.runId;
        // Engine-truth hold state: replay the pause events; the last run_paused with no later run_unpaused
        // means the scheduler actually reached the hold. This — never the sentinel stat — earns "Held".
        const held = data.events.reduce((h: boolean, e) => {
          const ev = (e as { event?: string }).event;
          if (ev === 'run_paused') return true;
          if (ev === 'run_unpaused') return false;
          return h;
        }, false);
        setState({
          present: true,
          runId: data.runId,
          lanes,
          planLanes,
          scoutLanes,
          contractLanes,
          detailLanes,
          sliceLanes,
          planningLanes,
          fixLanes,
          pool: resolvePool(data.events),
          supervision: foldSupervision(data.events),
          phaseTodo,
          overview,
          totals,
          activity,
          activityDigests: digests,
          activityMtimes: digestMtimes,
          verboseActivity: verbose,
          meta,
          plan,
          smoke,
          // PAUSED BEATS THE PHASE LABEL. `phase` is derived purely from task progress, so a held run kept
          // reading "Building" while every node sat idle — Mihai watched exactly that and reasonably read it
          // as a hang. `held` is engine truth (the last run_paused with no later run_unpaused), so when the
          // scheduler has actually reached the hold it OVERRIDES the progress-derived label. The distinction
          // that matters to someone watching is not which task is next, it is "is this thing working or not".
          phase: held ? 'Paused' : phase,
          // The RIBBON gets null while held for the same reason: a held run is not in a phase, it is
          // stopped between them, and lighting a step would assert work that is not happening.
          runPhase: held ? null : runPhase,
          runPhasesObserved,
          slices,
          proxy,
          reviewRounds,
          sinkRenamedFrom,
          synthesisFallback,
          knownActiveBugs,
          planConfidence,
          confidence,
          askFloor,
          confidenceTrail,
          inProgress: !finished,
          finished,
          summary,
          startedAt,
          clarify: data.clarify,
          runDir: data.dir ?? null,
          mtime: data.mtime,
          heartbeat: data.heartbeat,
          heartbeatExited: !!data.heartbeatExited,
          pauseRequested: !!data.pauseRequested,
          held,
          loading: false,
        });
      } catch {
        if (alive) setState((s) => ({ ...s, loading: false }));
      }
    };

    // Deltas arrive far faster than a read completes, so reads never stack: one in flight, at most one
    // queued behind it. Dropping the extra instead of queueing it would lose the run's LAST delta.
    //
    // SINGLE-FLIGHT IS ALSO THE ORDERING GUARANTEE, and that is the half that must not be optimised away.
    // `read` awaits an IPC round trip and then setStates what it read; with two reads in flight the older
    // one can resolve LAST, and the panel then regresses — done rows back to running, a lane count back
    // down, the ribbon back a stage — off data that was already stale when it landed. With one in flight
    // resolution order IS start order, so no such regression is representable. `missed` keeps that from
    // costing liveness: the queued tick runs the moment the current read returns, so a coalesced burst
    // still ends on the newest state rather than skipping it.
    let reading = false;
    let missed = false;
    const tick = async () => {
      if (reading) {
        missed = true;
        return;
      }
      reading = true;
      try {
        await read();
      } finally {
        reading = false;
      }
      if (missed && alive) {
        missed = false;
        void tick();
      }
    };

    void tick();
    // The interval is the SAFETY NET, not the transport: main fs.watches the run directory and pushes
    // a hint the moment the engine writes, but fs.watch coalesces and on some filesystems drops an
    // update outright, and a dropped one must not freeze the panel until the run ends.
    const iv = setInterval(() => void tick(), pollMs);
    const offDelta = window.electron.onSwarmDelta((delta) => {
      if (alive && delta.workingDir === workingDir) void tick();
    });
    return () => {
      alive = false;
      clearInterval(iv);
      offDelta();
    };
  }, [workingDir, pollMs]);

  return state;
}
