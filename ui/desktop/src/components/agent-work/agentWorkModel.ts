/**
 * AGENT WORK — the desk model. Pure: the shapes main.ts hands the renderer (what the engine wrote
 * under `<agent dir>/.swarm/agent/` and `.swarm/activity/`) and the ONE fold that turns them into
 * what the desk view shows: liveness, the tick clock, the phase, the lanes of the current tick with
 * their live words, the node occupancy and queue, what needs the human, the ledger, the tick history.
 *
 * Every lane's stream fields come through `digestFields` — one join, never hand-copied per path.
 */

export interface AgentManifestLite {
  name?: string;
  title?: string;
  cadence?: string;
  timezone?: string;
  window?: { days?: string[]; from?: string; to?: string; always?: boolean };
  surgeons?: { name: string; read_only?: boolean; match?: string[] }[];
  review?: { enabled?: boolean; lenses?: string[] };
  post?: { command: string; approval?: 'human' | 'none' };
  poll?: string[];
  guard?: string[];
  scratchpad?: string;
  pending?: string;
  ledger?: string;
  charter?: string;
}

export interface TickSummary {
  tick: number;
  started_at: string;
  ended_at: string;
  outcome: 'done' | 'held' | 'failed' | string;
  summary: string;
  lanes: number;
  staged: number;
  posted: number;
  asks: number;
  lane_secs: number;
  wall_secs: number;
}

export interface DeviceSlot {
  id: string;
  model_id: string;
  weight: number;
  supervision: boolean;
}

export interface DeskState {
  status: string;
  agent: string;
  title: string;
  pid: number | null;
  run_id: string;
  tick: number;
  phase: string;
  phase_started_at: string | null;
  next_tick_at: string | null;
  next_tick_reason: string;
  next_tick_local: string;
  cadence: string;
  timezone: string;
  window_open: boolean;
  last_tick: TickSummary | null;
  lanes_planned: number;
  lanes_done: number;
  hold_reason: string | null;
  decisions_applied: number;
  planner_model: string;
  devices: DeviceSlot[];
  updated_at: string;
}

export interface PreparedRow {
  id: string;
  tick: number;
  lane: string;
  surgeon: string;
  target: string;
  kind: string;
  body: string;
  evidence: string[];
  status: 'staged' | 'approved' | 'declined' | 'posted' | 'failed' | 'done' | string;
  staged_at: string;
  decided_at?: string | null;
  decided_note?: string | null;
  posted_tick?: number | null;
  result?: string | null;
  review?: { lens: string; verdict: string; notes: string }[];
}

export interface AskRow {
  id: string;
  tick: number;
  question: string;
  why: string;
  status: 'open' | 'answered' | 'dismissed' | string;
  raised_at: string;
  answer?: string | null;
  answered_at?: string | null;
  consumed_tick?: number | null;
}

export interface AgentEvent {
  event: string;
  ts?: string;
  seq?: number;
  tick?: number;
  [k: string]: unknown;
}

export interface TickRecord {
  tick: number;
  started_at?: string;
  ended_at?: string;
  outcome?: string;
  summary?: string;
  notes?: string[];
  poll?: {
    command: string;
    exit: number | null;
    secs: number;
    stdout_chars: number;
    stderr_tail: string;
  }[];
  orient?: {
    summary: string;
    lanes: { id: string; surgeon: string; item: string; objective: string; kind: string }[];
    asks: { question: string; why: string }[];
    drop: { item: string; why: string }[];
  };
  lanes?: Record<string, unknown>[];
  review?: Record<string, unknown>[];
  synthesis?: {
    source?: { mode: 'lane_report'; lane: string; key: string } | { mode: 'model' };
    staged: string[];
    asks: string[];
    facts: string[];
    log_line: string;
    handoff: string;
    pending: string[];
  };
  posted?: string[];
  lane_secs?: number;
  wall_secs?: number;
}

/** The engine's activity digest as main.ts hands it over (plus the tails it attaches). */
export interface LaneDigest {
  model?: string;
  phase?: string;
  tool_calls?: number;
  errors?: number;
  malformed?: number;
  last_text?: string;
  last_thinking?: string;
  thinking_chars?: number;
  calls?: { name?: string; ok?: boolean | null; args?: string; result?: string; at?: string }[];
  inflight?: { name?: string; since?: string }[];
  forming?: {
    id: string;
    name: string;
    since_ms: number;
    args_bytes?: number;
    args_preview?: string;
  }[];
  full_thinking?: string;
  thinking_bytes?: number;
  full_transcript?: string;
  transcript_bytes?: number;
  dispatched_at?: string;
  said_at?: string | null;
  attempt?: number;
  [k: string]: unknown;
}

export interface Rollup {
  rebuilt_at?: string;
  counts?: Record<string, number>;
  kinds?: Record<string, Record<string, unknown>[]>;
  dropped?: { file: string; error: string }[];
}

export interface AgentWorkRosterRow {
  dir: string;
  addedAt: string;
  manifest: AgentManifestLite | null;
  state: DeskState | null;
  pid: number | null;
  heartbeatMs: number | null;
  exists: boolean;
}

export interface AgentWorkRead {
  dir: string;
  manifest: AgentManifestLite | null;
  state: DeskState | null;
  pid: number | null;
  heartbeatMs: number | null;
  events: AgentEvent[];
  ticks: TickRecord[];
  lanes: Record<string, LaneDigest>;
  laneMtimes: Record<string, number>;
  prepared: PreparedRow[];
  asks: AskRow[];
  ledger: Rollup | null;
  scratchpad: string;
  pending: string;
  dailyLog: string;
  engineLog: string;
  now: number;
}

// ---------------------------------------------------------------- the fold

export type Liveness = 'running' | 'stale' | 'stopped';

export type LaneKind = 'orient' | 'lane' | 'lens' | 'synthesis';
export type LaneStatus = 'queued' | 'running' | 'done' | 'failed' | 'interrupted';

export interface DeskLane {
  key: string;
  tick: number;
  kind: LaneKind;
  laneId: string;
  lens?: string;
  surgeon?: string;
  item?: string;
  objective?: string;
  model: string;
  status: LaneStatus;
  startedAt?: string;
  secs?: number;
  hasDraft?: boolean;
  confidence?: number | null;
  ask?: string | null;
  route?: string | null;
  verdict?: string;
  error?: string | null;
  /** The live line: the answer channel when it advanced last, else the reasoning channel. */
  liveLine: string;
  thinkingTail: string;
  answerTail: string;
  fullThinking: string;
  fullTranscript: string;
  thinkingBytes: number;
  transcriptBytes: number;
  toolCalls: number;
  errors: number;
  calls: NonNullable<LaneDigest['calls']>;
  forming: NonNullable<LaneDigest['forming']>;
  digestAgeMs: number | null;
}

export interface NodeOccupancy extends DeviceSlot {
  running: DeskLane[];
  free: number;
}

export interface DeskTotals {
  ticks: number;
  lanes: number;
  staged: number;
  posted: number;
  asks: number;
  laneMinutes: number;
}

export interface DeskModel {
  liveness: Liveness;
  status: string;
  tick: number;
  /** The tick the view shows (`?tick=N`, else the current one) — lanes and phases follow it. */
  viewTick: number;
  viewRecord: TickRecord | null;
  /** Where the viewed tick's time went, from its `tick_phase` events; null when nothing recorded it. */
  phases: PhaseSpan[] | null;
  phase: string;
  phaseElapsedMs: number | null;
  nextTickAt: number | null;
  nextTickInMs: number | null;
  nextTickReason: string;
  nextTickLocal: string;
  windowOpen: boolean;
  holdReason: string | null;
  lanes: DeskLane[];
  queue: DeskLane[];
  nodes: NodeOccupancy[];
  openAsks: AskRow[];
  answeredAsks: AskRow[];
  pendingDrafts: PreparedRow[];
  settledDrafts: PreparedRow[];
  ticks: TickRecord[];
  totals: DeskTotals;
  facts: { tick: number; fact: string; at?: string }[];
  lastTick: TickSummary | null;
}

/** A heartbeat older than this, with the pid alive, is a process that stopped writing — stale. */
export const HEARTBEAT_STALE_MS = 20_000;

export const PHASES: readonly { key: string; label: string }[] = [
  { key: 'guard', label: 'Guard' },
  { key: 'poll', label: 'Poll' },
  { key: 'orient', label: 'Orient' },
  { key: 'lanes', label: 'Lanes' },
  { key: 'review', label: 'Review' },
  { key: 'synthesis', label: 'Synthesis' },
  { key: 'post', label: 'Post' },
  { key: 'close', label: 'Close' },
];

export type PhaseState = 'done' | 'live' | 'failed' | 'interrupted' | 'next' | 'skipped';

export interface PhaseSpan {
  key: string;
  label: string;
  state: PhaseState;
  /** Time spent in the phase; null when only the phase clock (not the events) says it passed. */
  ms: number | null;
  note?: string;
}

const PHASE_NOTE: Record<string, string> = {
  poll_absent: 'no poll command',
  synthesis_not_needed: 'report passed on as written',
};

/**
 * The anatomy of one tick from the engine's own `tick_phase` events: every phase it ENTERED with the
 * time it spent there (entry → next entry, the last one → `tick_done`, or → now while live), the
 * phases it never entered as skipped, and — while it runs — what is still ahead. Direct delivery
 * enters `handoff`, so it is named Handoff and synthesis is never invented. With no events for the
 * tick, only a LIVE tick is drawn, from the state file's phase clock (no durations are guessed);
 * a finished tick without events returns null and the view shows nothing rather than a guess.
 */
export function tickPhases(
  events: AgentEvent[],
  tick: number,
  opts: {
    live: boolean;
    now: number;
    phase: string;
    phaseElapsedMs: number | null;
    outcome?: string;
  }
): PhaseSpan[] | null {
  const mine = events.filter((e) => e.tick === tick);
  const entered = mine.filter((e) => e.event === 'tick_phase' && typeof e.phase === 'string');
  const slot = (key: string) => (key === 'handoff' ? 'synthesis' : key);
  const label = (key: string) =>
    key === 'handoff' ? 'Handoff' : (PHASES.find((p) => p.key === key)?.label ?? key);
  if (entered.length === 0) {
    if (!opts.live) return null;
    const current = phaseIndex(opts.phase);
    if (current < 0) return null;
    return PHASES.map((p, i) => {
      const key = i === current && opts.phase === 'handoff' ? 'handoff' : p.key;
      return {
        key,
        label: label(key),
        state: i < current ? 'done' : i === current ? 'live' : 'next',
        ms: i === current ? opts.phaseElapsedMs : null,
      };
    });
  }
  const done = mine.find((e) => e.event === 'tick_done');
  const endMs = done?.ts ? Date.parse(done.ts) : opts.live ? opts.now : null;
  const notes = new Map<string, string>();
  let at = '';
  for (const e of mine) {
    if (e.event === 'tick_phase') at = String(e.phase);
    else if (PHASE_NOTE[e.event] && at) notes.set(at, PHASE_NOTE[e.event]);
  }
  const spans: PhaseSpan[] = entered.map((e, i) => {
    const key = String(e.phase);
    const start = e.ts ? Date.parse(e.ts) : NaN;
    const next = entered[i + 1];
    const end = next?.ts ? Date.parse(next.ts) : endMs;
    const last = i === entered.length - 1;
    const state: PhaseState = !last
      ? 'done'
      : done
        ? opts.outcome === 'failed' || done.outcome === 'failed'
          ? 'failed'
          : 'done'
        : opts.live
          ? 'live'
          : 'interrupted';
    return {
      key,
      label: label(key),
      state,
      ms:
        end != null && Number.isFinite(start) && Number.isFinite(end)
          ? Math.max(0, end - start)
          : null,
      note: notes.get(key),
    };
  });
  const seen = new Set(entered.map((e) => slot(String(e.phase))));
  const reached = Math.max(...entered.map((e) => phaseIndex(String(e.phase))));
  const rest: PhaseSpan[] = PHASES.filter((p) => !seen.has(p.key)).map((p) => ({
    key: p.key,
    label: p.label,
    state: opts.live && phaseIndex(p.key) > reached ? 'next' : 'skipped',
    ms: null,
  }));
  return [...spans, ...rest].sort((a, b) => phaseIndex(a.key) - phaseIndex(b.key));
}

/** What a tick handed over, pulled apart for reading: the lead text, its sources, the evidence. */
export interface TickReport {
  /** `lane_report`: one worker's report delivered as written; `handoff`: the synthesis wrote it. */
  mode: 'lane_report' | 'handoff' | 'none';
  lead: string;
  nextStep: string;
  homework: string;
  evidence: string[];
  sources: string[];
  confidence: number | null;
  lane: Record<string, unknown> | null;
}

const URL_RE = /https?:\/\/[^\s<>"'`|\][()]+/g;

/** Every distinct http(s) URL in the texts, in first-seen order, trailing punctuation trimmed. */
export function sourceUrls(texts: readonly string[]): string[] {
  const out: string[] = [];
  for (const t of texts) {
    for (const m of t.match(URL_RE) ?? []) {
      const url = m.replace(/[.,;:!?]+$/, '');
      if (!out.includes(url)) out.push(url);
    }
  }
  return out;
}

export function tickReport(tick: TickRecord): TickReport {
  const source = tick.synthesis?.source;
  const lane =
    source?.mode === 'lane_report' ? (tick.lanes?.find((l) => l.key === source.key) ?? null) : null;
  const str = (v: unknown) => (typeof v === 'string' ? v.trim() : '');
  const finding = str(lane?.finding);
  const evidence = Array.isArray(lane?.evidence)
    ? (lane!.evidence as unknown[]).filter((e): e is string => typeof e === 'string')
    : [];
  const lead =
    finding || str(tick.synthesis?.handoff) || str(tick.synthesis?.log_line) || str(tick.summary);
  return {
    mode: finding ? 'lane_report' : tick.synthesis?.handoff?.trim() ? 'handoff' : 'none',
    lead,
    nextStep: str(lane?.next_step),
    homework: str(lane?.homework),
    evidence,
    sources: sourceUrls([str(lane?.item), lead, ...evidence]),
    confidence: typeof lane?.confidence === 'number' ? lane.confidence : null,
    lane,
  };
}

export function phaseIndex(phase: string): number {
  return PHASES.findIndex((p) => p.key === (phase === 'handoff' ? 'synthesis' : phase));
}

/** ONE join from the engine's digest to a lane's stream fields. */
export function digestFields(d: LaneDigest | undefined, mtime: number | undefined, now: number) {
  const answerTail = (d?.last_text ?? '').trim();
  const thinkingTail = (d?.last_thinking ?? '').trim();
  const forming = Array.isArray(d?.forming) ? d!.forming! : [];
  let liveLine = '';
  if (forming.length > 0) {
    const f = forming[forming.length - 1];
    liveLine = `forming ${f.name}${f.args_preview ? ` — ${f.args_preview}` : ''}`;
  } else if (d?.said_at && answerTail) {
    liveLine = answerTail;
  } else if (thinkingTail) {
    liveLine = thinkingTail;
  } else if (answerTail) {
    liveLine = answerTail;
  } else if (d?.phase === 'processing') {
    liveLine = 'processing the prompt…';
  }
  return {
    liveLine: lastLine(liveLine),
    thinkingTail,
    answerTail,
    fullThinking: d?.full_thinking ?? '',
    fullTranscript: d?.full_transcript ?? '',
    thinkingBytes: d?.thinking_bytes ?? 0,
    transcriptBytes: d?.transcript_bytes ?? 0,
    toolCalls: d?.tool_calls ?? 0,
    errors: d?.errors ?? 0,
    calls: Array.isArray(d?.calls) ? d!.calls! : [],
    forming,
    digestAgeMs: mtime != null ? Math.max(0, now - mtime) : null,
    model: d?.model ?? '',
  };
}

function lastLine(s: string): string {
  const lines = s
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
  const l = lines.length ? lines[lines.length - 1] : '';
  return l.length > 240 ? `…${l.slice(l.length - 240)}` : l;
}

export function liveness(pid: number | null, heartbeatMs: number | null, now: number): Liveness {
  if (!pid) return 'stopped';
  if (heartbeatMs == null) return 'stale';
  return now - heartbeatMs <= HEARTBEAT_STALE_MS ? 'running' : 'stale';
}

/** Lane keys the engine mints: `t<n>-orient`, `t<n>-<lane>`, `t<n>-<lane>-lens-<lens>`, `t<n>-synthesis`. */
export function classifyKey(
  key: string
): { tick: number; kind: LaneKind; laneId: string; lens?: string } | null {
  const m = /^t(\d+)-(.+)$/.exec(key);
  if (!m) return null;
  const tick = Number.parseInt(m[1], 10);
  const rest = m[2];
  if (rest === 'orient') return { tick, kind: 'orient', laneId: 'orient' };
  if (rest === 'synthesis') return { tick, kind: 'synthesis', laneId: 'synthesis' };
  const lens = /^(.+)-lens-([^-]+(?:-[^-]+)*)$/.exec(rest);
  if (lens && rest.includes('-lens-')) {
    const idx = rest.lastIndexOf('-lens-');
    return { tick, kind: 'lens', laneId: rest.slice(0, idx), lens: rest.slice(idx + 6) };
  }
  return { tick, kind: 'lane', laneId: rest };
}

/**
 * The lanes of ONE tick: seeded from that tick's events (queue → dispatched → done) and completed
 * from the digests (words, calls, forming). The current tick reads the live phase clock; an earlier
 * tick is settled — anything it never finished reads as interrupted, never as running.
 */
function foldLanes(
  read: AgentWorkRead,
  tick: number,
  current: boolean,
  live: Liveness,
  now: number
): DeskLane[] {
  const st = read.state;
  const events = read.events.filter((e) => (current ? (e.tick ?? tick) : e.tick) === tick);
  const rows = new Map<string, DeskLane>();
  const ensure = (key: string): DeskLane | null => {
    const c = classifyKey(key);
    if (!c || c.tick !== tick) return null;
    let r = rows.get(key);
    if (!r) {
      const f = digestFields(read.lanes[key], read.laneMtimes[key], now);
      r = {
        key,
        tick: c.tick,
        kind: c.kind,
        laneId: c.laneId,
        lens: c.lens,
        status: 'running',
        ...f,
      };
      rows.set(key, r);
    }
    return r;
  };
  for (const e of events) {
    const key = typeof e.key === 'string' ? e.key : null;
    switch (e.event) {
      case 'lane_queued': {
        const r = key && ensure(key);
        if (r) r.status = 'queued';
        break;
      }
      case 'lane_dispatched':
      case 'lens_dispatched': {
        const r = key && ensure(key);
        if (r) {
          r.status = 'running';
          r.model = String(e.model ?? r.model);
          r.startedAt = e.ts;
          if (typeof e.surgeon === 'string') r.surgeon = e.surgeon;
          if (typeof e.item === 'string') r.item = e.item;
          if (typeof e.lens === 'string') r.lens = e.lens;
        }
        break;
      }
      case 'lane_done':
      case 'lane_failed':
      case 'lens_done': {
        const r = key && ensure(key);
        if (r) {
          r.status = e.event === 'lane_failed' || e.error ? 'failed' : 'done';
          r.secs = typeof e.secs === 'number' ? e.secs : r.secs;
          r.hasDraft = e.has_draft === true;
          r.confidence = typeof e.confidence === 'number' ? e.confidence : null;
          r.ask = typeof e.ask === 'string' ? e.ask : null;
          r.route = typeof e.route === 'string' ? e.route : null;
          r.verdict = typeof e.verdict === 'string' ? e.verdict : undefined;
          r.error = typeof e.error === 'string' ? e.error : null;
        }
        break;
      }
      default:
        break;
    }
  }
  // Orient / synthesis have no dispatch events of their own: the digest is their presence, the
  // phase clock their status.
  for (const key of Object.keys(read.lanes)) {
    const c = classifyKey(key);
    if (!c || c.tick !== tick) continue;
    const r = ensure(key);
    if (!r) continue;
    if (c.kind === 'orient' || c.kind === 'synthesis') {
      const phase = st?.phase ?? 'idle';
      const done =
        events.some((e) => e.event === `${c.kind}_done`) ||
        (current &&
          (c.kind === 'orient'
            ? phaseIndex(phase) > phaseIndex('orient') || (phase === 'idle' && live !== 'stopped')
            : (phase === 'idle' && live !== 'stopped') ||
              phaseIndex(phase) > phaseIndex('synthesis')));
      r.status = done ? 'done' : 'running';
      const completion = [...events].reverse().find((event) => event.event === `${c.kind}_done`);
      const summary = c.kind === 'orient' ? completion?.summary : completion?.log_line;
      if (typeof summary === 'string' && summary.trim()) r.liveLine = summary.trim();
      r.model = r.model || st?.planner_model || '';
    }
  }
  // Objectives from the tick record's orient plan; a finished lane speaks with its finding.
  const rec = read.ticks.find((t) => t.tick === tick);
  for (const report of rec?.lanes ?? []) {
    if (typeof report.key !== 'string' || typeof report.finding !== 'string') continue;
    const row = rows.get(report.key);
    if (row?.status === 'done' && report.finding.trim()) row.liveLine = report.finding.trim();
  }
  if (rec?.orient?.lanes) {
    for (const p of rec.orient.lanes) {
      const r = rows.get(`t${tick}-${p.id}`);
      if (r) {
        r.objective = p.objective;
        r.item = r.item || p.item;
        r.surgeon = r.surgeon || p.surgeon;
      }
    }
  }
  const order: Record<LaneKind, number> = { orient: 0, lane: 1, lens: 2, synthesis: 3 };
  const lanes = Array.from(rows.values()).sort(
    (a, b) => order[a.kind] - order[b.kind] || a.key.localeCompare(b.key)
  );
  if (live === 'stopped' || !current) {
    for (const lane of lanes) {
      if (lane.status === 'running' || lane.status === 'queued') lane.status = 'interrupted';
    }
  }
  return lanes;
}

/**
 * `viewTick` is the tick the person opened (`?tick=N`); absent, the desk's current tick. The lanes
 * and phases follow the viewed tick; the queue and the node occupancy are always the live tick's.
 */
export function foldDesk(
  read: AgentWorkRead | null,
  now: number,
  viewTick?: number | null
): DeskModel | null {
  if (!read) return null;
  const st = read.state;
  const live = liveness(read.pid, read.heartbeatMs, now);
  const tick = st?.tick ?? 0;
  const shown = viewTick ?? tick;
  const currentLanes = foldLanes(read, tick, true, live, now);
  const lanes = shown === tick ? currentLanes : foldLanes(read, shown, false, live, now);
  const queue = currentLanes.filter((l) => l.status === 'queued');
  const running = currentLanes.filter((l) => l.status === 'running' && live === 'running');
  const nodes: NodeOccupancy[] = (st?.devices ?? []).map((d) => {
    const mine = running.filter((l) => l.model === d.model_id);
    return { ...d, running: mine, free: Math.max(0, d.weight - mine.length) };
  });

  const openAsks = read.asks.filter((a) => a.status === 'open');
  const answeredAsks = read.asks.filter((a) => a.status !== 'open');
  const pendingDrafts = read.prepared.filter((r) =>
    ['staged', 'approved', 'failed'].includes(r.status)
  );
  const settledDrafts = read.prepared.filter(
    (r) => !['staged', 'approved', 'failed'].includes(r.status)
  );

  const kinds = read.ledger?.kinds ?? {};
  const tickRows = (kinds.tick ?? []) as {
    tick?: number;
    lanes?: number;
    staged?: number;
    posted?: number;
    lane_secs?: number;
  }[];
  const totals: DeskTotals = {
    ticks: tickRows.length,
    lanes: tickRows.reduce((n, t) => n + (t.lanes ?? 0), 0),
    staged: tickRows.reduce((n, t) => n + (t.staged ?? 0), 0),
    posted: tickRows.reduce((n, t) => n + (t.posted ?? 0), 0),
    asks: ((kinds.ask ?? []) as unknown[]).length,
    laneMinutes: tickRows.reduce((n, t) => n + (t.lane_secs ?? 0), 0) / 60,
  };
  const facts = ((kinds.fact ?? []) as { tick?: number; fact?: string; at?: string }[])
    .map((f) => ({ tick: f.tick ?? 0, fact: f.fact ?? '', at: f.at }))
    .reverse();

  const nextTickAt = st?.next_tick_at ? Date.parse(st.next_tick_at) : null;
  const phaseStarted = st?.phase_started_at ? Date.parse(st.phase_started_at) : null;
  const status = live === 'stopped' ? 'stopped' : (st?.status ?? 'unknown');
  const phase = live === 'stopped' ? 'idle' : (st?.phase ?? 'idle');
  const phaseElapsedMs =
    phaseStarted != null && Number.isFinite(phaseStarted) ? Math.max(0, now - phaseStarted) : null;
  const viewRecord = read.ticks.find((t) => t.tick === shown) ?? null;
  return {
    liveness: live,
    status,
    tick,
    viewTick: shown,
    viewRecord,
    phases: tickPhases(read.events, shown, {
      live: shown === tick && live === 'running',
      now,
      phase: shown === tick ? phase : 'idle',
      phaseElapsedMs,
      outcome: viewRecord?.outcome,
    }),
    phase,
    phaseElapsedMs,
    nextTickAt:
      nextTickAt != null && Number.isFinite(nextTickAt) && live !== 'stopped' ? nextTickAt : null,
    nextTickInMs:
      nextTickAt != null && Number.isFinite(nextTickAt) && live !== 'stopped'
        ? nextTickAt - now
        : null,
    nextTickReason: st?.next_tick_reason ?? '',
    nextTickLocal: st?.next_tick_local ?? '',
    windowOpen: st?.window_open ?? false,
    holdReason: st?.hold_reason ?? null,
    lanes,
    queue,
    nodes,
    openAsks,
    answeredAsks,
    pendingDrafts,
    settledDrafts,
    ticks: [...read.ticks].sort((a, b) => b.tick - a.tick),
    totals,
    facts,
    lastTick: st?.last_tick ?? null,
  };
}

/** "in 12m 04s" / "now" / "overdue 1m". */
export function countdown(ms: number | null): string {
  if (ms == null) return '—';
  if (ms <= 0) return ms > -1500 ? 'now' : `overdue ${fmtDuration(-ms)}`;
  return `in ${fmtDuration(ms)}`;
}

export function fmtDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, '0')}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${String(m % 60).padStart(2, '0')}m`;
}

export function fmtClock(msEpoch: number | null): string {
  if (msEpoch == null || !Number.isFinite(msEpoch)) return '—';
  const d = new Date(msEpoch);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** Cadence + window as one line: "every 30m · Mon–Fri 09:00–18:00 Europe/Zurich". */
export function scheduleLine(m: AgentManifestLite | null): string {
  if (!m) return '';
  const w = m.window;
  const days = w?.days?.length
    ? `${cap(w.days[0])}–${cap(w.days[w.days.length - 1])}`
    : 'every day';
  const window = w?.always ? 'always open' : `${days} ${w?.from ?? '00:00'}–${w?.to ?? '24:00'}`;
  return `every ${m.cadence ?? '?'} · ${window}${m.timezone ? ` ${m.timezone}` : ''}`;
}

function cap(s: string): string {
  return s.length ? s[0].toUpperCase() + s.slice(1, 3) : s;
}

/** The YAML the New Agent dialog writes. Built by hand so the file reads like the starter. */
export function manifestYaml(input: {
  name: string;
  title: string;
  timezone: string;
  from: string;
  to: string;
  days: string[];
  always: boolean;
  cadence: string;
  envFile: string;
  poll: string[];
  guard: string[];
  surgeons: { name: string; brief: string; readOnly: boolean }[];
  lenses: string[];
  postCommand: string;
  approval: 'human' | 'none';
  commit: boolean;
  extensions?: string[];
}): string {
  const q = (s: string) => JSON.stringify(s);
  const list = (xs: string[]) => (xs.length ? `[${xs.map(q).join(', ')}]` : '[]');
  const lines: string[] = [
    `name: ${input.name}`,
    `title: ${q(input.title || input.name)}`,
    `charter: CHARTER.md`,
    `timezone: ${q(input.timezone)}`,
    `window:`,
    `  days: ${list(input.days)}`,
    `  from: ${q(input.from)}`,
    `  to: ${q(input.to)}`,
    `  always: ${input.always ? 'true' : 'false'}`,
    `cadence: ${input.cadence}`,
  ];
  if (input.envFile.trim()) lines.push(`env_file: ${q(input.envFile.trim())}`);
  lines.push(`guard: ${list(input.guard.filter(Boolean))}`);
  lines.push(`poll: ${list(input.poll.filter(Boolean))}`);
  lines.push(`close: []`);
  if (input.surgeons.length) {
    lines.push(`surgeons:`);
    for (const s of input.surgeons) {
      lines.push(`  - name: ${q(s.name)}`);
      lines.push(`    brief: ${q(s.brief)}`);
      lines.push(`    match: []`);
      lines.push(`    read_only: ${s.readOnly ? 'true' : 'false'}`);
    }
  } else {
    lines.push(`surgeons: []`);
  }
  lines.push(`review:`);
  lines.push(`  enabled: ${input.lenses.length ? 'true' : 'false'}`);
  lines.push(`  lenses: ${list(input.lenses)}`);
  if (input.postCommand.trim()) {
    lines.push(`post:`);
    lines.push(`  command: ${q(input.postCommand.trim())}`);
    lines.push(`  approval: ${input.approval}`);
  }
  lines.push(`ledger: DAILY-LOG.md`);
  lines.push(`pending: PENDING.md`);
  lines.push(`scratchpad: SCRATCHPAD.md`);
  lines.push(`commit: ${input.commit ? 'true' : 'false'}`);
  lines.push(`extensions: ${list(input.extensions ?? [])}`);
  return `${lines.join('\n')}\n`;
}
