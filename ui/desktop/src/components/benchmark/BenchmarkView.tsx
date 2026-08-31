import { useCallback, useEffect, useId, useMemo, useState, type ReactNode } from 'react';
import {
  Gauge,
  Play,
  Upload,
  Loader2,
  XCircle,
  BadgeCheck,
  ChevronDown,
  ChevronRight,
  Trash2,
} from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { TIER_LABELS, type BenchmarkRow, type Tier } from './baselines';
import type {
  BenchSession,
  CatalogBaseline,
  CatalogBenchmark,
  CatalogMismatch,
  SessionOutcome,
} from './bridge';
import { ScoreBars } from './ScoreBars';
import { TierBreakdown } from './TierBreakdown';
import { ScoringDetail, type VerdictDetail } from './ScoringDetail';
import { SwarmRunPanel } from '../swarm/SwarmRunPanel';
import { useSwarmRun } from '../swarm/useSwarmRun';
import { ZoneHeader, ZONE_HUES } from '../swarm/ZoneHeader';
import SamplingKnobs from '../swarm/SamplingKnobs';
import { useSaveSamplingDefaults } from '../swarm/useSamplingDefaults';
import {
  loadSamplingDefaults,
  sanitizeSampling,
  type SamplingSettings,
} from '../swarm/sampling';
import { Input } from '../ui/input';
import { ConfirmationModal } from '../ui/ConfirmationModal';

const NODE_CHOICES = [1, 2, 3] as const;
type NodeChoice = (typeof NODE_CHOICES)[number];

const MODEL_MIN_CHARS = 8;

/** Why this result's ENGINE-TRUTH model id cannot be published, or null when it can. The field is
 *  read-only — a user-editable model id publishes a lie — so the only problem left is absence:
 *  a result whose run never recorded pool_resolved (or recorded junk) refuses with the reason. */
export function modelIdProblem(modelId: string | undefined): string | null {
  const n = (modelId ?? '').trim().length;
  if (n === 0) return 'This result carries no model id from the engine — run the benchmark again.';
  if (n < MODEL_MIN_CHARS) return `The engine recorded a ${n}-character model id — too short to publish.`;
  return null;
}

/** Why the Title field blocks publishing, or null when it does not. The title is the USER'S name
 *  for the run on the public board — never auto-generated. */
export function titleProblem(title: string): string | null {
  if (title.trim().length === 0) return 'Title is required — name this run; it is the public title on the board.';
  return null;
}

/** What the Publish button is in: the state machine ask 4 demands — a request in flight, the
 *  server's acceptance (with what went live), or the server's OWN error words. */
type PublishState =
  | { kind: 'idle' }
  | { kind: 'publishing' }
  | { kind: 'accepted'; title: string; score: number; url: string | null }
  | { kind: 'error'; message: string };

/** The stored result row — the v1 chart fields plus the v2 publisher inputs main.ts persists. */
interface MineRow extends BenchmarkRow {
  runMeta?: { startedAt: string; finishedAt: string; engineEvents: number; repairRounds: number };
  workdir?: string;
  /** Full scoring detail (every check + evidence + repair story) — absent on pre-detail results. */
  verdict?: VerdictDetail;
  /** Engine-truth model identifier (pool_resolved, host prefixes stripped) — the Model prefill. */
  modelId?: string;
}

interface BenchShot {
  name: string;
  caption: string;
  b64: string;
}

type BenchPhase = 'boot' | 'build' | 'score' | 'done';

const PHASES: Array<{ key: BenchPhase; label: string }> = [
  { key: 'boot', label: 'Boot' },
  { key: 'build', label: 'Swarm build' },
  { key: 'score', label: 'Scoring' },
  { key: 'done', label: 'Done' },
];

// Same status palette as SwarmRunPanel so a phase reads the same across the app.
const PHASE_ACTIVE = '#f5a623';
const PHASE_DONE = '#2ecc71';
const STATE_ERROR = '#e5484d';

function fmtElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m ${s % 60}s`;
}

/** Short completion stamp for a stored result ("Aug 17, 18:27") — how a result stays identifiable. */
function fmtWhen(iso: string | undefined): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  return new Date(t).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/**
 * BENCHMARK PIPELINE zone — the harness AROUND the swarm: which phase is live, what already finished,
 * and the harness's latest output line — never a bare spinner. Labeled in the same zone register as the
 * swarm panel's own zones (its "SWARM RUN" band sits right below), so benchmark chrome and swarm run are
 * never ambiguous. The swarm-build phase gets its full live panel below; this strip covers the phases
 * that are NOT the swarm (vendor sim boot, scoring).
 */
function PhaseStrip({
  phase,
  lastLine,
  elapsedMs,
}: {
  phase: BenchPhase;
  lastLine: string | null;
  elapsedMs: number;
}) {
  const activeIdx = PHASES.findIndex((p) => p.key === phase);
  return (
    <div className="rounded border border-border-primary">
      <ZoneHeader
        hue={ZONE_HUES.bench}
        label="Benchmark pipeline"
        explain="the harness around the swarm — boot, build, scoring"
        className="border-b border-border-primary py-2"
        right={
          <span className="text-xs font-semibold tabular-nums text-text-primary">
            {fmtElapsed(elapsedMs)}
          </span>
        }
      />
      <div className="flex flex-wrap gap-2 px-3 py-3">
        {PHASES.map((p, i) => {
          const state = i < activeIdx ? 'done' : i === activeIdx ? 'active' : 'pending';
          return (
            <span
              key={p.key}
              className="flex items-center gap-1.5 rounded border px-2.5 py-1 text-xs font-bold"
              style={
                state === 'active'
                  ? { backgroundColor: PHASE_ACTIVE, borderColor: PHASE_ACTIVE, color: '#1a1a1a' }
                  : state === 'done'
                    ? { backgroundColor: PHASE_DONE, borderColor: PHASE_DONE, color: '#fff' }
                    : {
                        borderColor: 'var(--color-border-primary)',
                        color: 'var(--color-text-secondary)',
                      }
              }
            >
              {state === 'active' && <Loader2 className="h-3 w-3 animate-spin" />}
              {p.label}
            </span>
          );
        })}
      </div>
      {lastLine && (
        <div className="border-t border-border-primary bg-background-secondary px-3 py-2 font-mono text-[11px] text-text-secondary">
          {lastLine.length > 200 ? lastLine.slice(0, 197) + '…' : lastLine}
        </div>
      )}
    </div>
  );
}

const shotColor = (name: string) =>
  name === 'loaded-before' ? 'var(--color-node-5)' : 'var(--color-block-teal)';

/**
 * Full-size viewer for one screenshot. The strip crops to 160px of the top of a full page render,
 * which is enough to notice something is wrong and never enough to read it — the counters, the row
 * values and the console state all live below that crop.
 */
function ShotLightbox({
  shots,
  index,
  onIndex,
  onClose,
}: {
  shots: BenchShot[];
  index: number;
  onIndex: (i: number) => void;
  onClose: () => void;
}) {
  const shot = shots[index];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
      if (e.key === 'ArrowRight') onIndex((index + 1) % shots.length);
      if (e.key === 'ArrowLeft') onIndex((index - 1 + shots.length) % shots.length);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [index, shots.length, onIndex, onClose]);

  if (!shot) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex flex-col"
      style={{ backgroundColor: 'rgba(6,8,14,0.97)' }}
      onClick={onClose}
      role="presentation"
    >
      <div
        className="flex items-center justify-between gap-3 px-4 py-2.5"
        style={{ backgroundColor: shotColor(shot.name) }}
        onClick={(e) => e.stopPropagation()}
        role="presentation"
      >
        <span className="text-[13px] font-bold text-white">{shot.caption}</span>
        <div className="flex items-center gap-2">
          <span className="text-[12px] font-bold text-white opacity-90">
            {index + 1} / {shots.length}
          </span>
          {shots.length > 1 && (
            <>
              <button
                type="button"
                className="rounded bg-black px-2.5 py-1 text-[12px] font-bold text-white"
                onClick={() => onIndex((index - 1 + shots.length) % shots.length)}
              >
                ‹ Prev
              </button>
              <button
                type="button"
                className="rounded bg-black px-2.5 py-1 text-[12px] font-bold text-white"
                onClick={() => onIndex((index + 1) % shots.length)}
              >
                Next ›
              </button>
            </>
          )}
          <button
            type="button"
            className="rounded bg-black px-2.5 py-1 text-[12px] font-bold text-white"
            onClick={() => void window.electron.toggleFullscreen?.()}
          >
            Fullscreen
          </button>
          <button
            type="button"
            className="rounded bg-white px-2.5 py-1 text-[12px] font-bold text-black"
            onClick={onClose}
          >
            Close
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-auto p-4" onClick={onClose} role="presentation">
        {/* No stopPropagation here: ANY click below the header closes the viewer. The automated
            journey drives this page blind over CDP — if a stale-coordinate click ever opens the
            viewer, the next click must escape it rather than being swallowed by the image. */}
        <img
          src={`data:image/png;base64,${shot.b64}`}
          alt={shot.caption}
          className="mx-auto block max-w-full bg-white"
        />
      </div>
    </div>
  );
}

/** Before/after screenshot strip from the run's bench-shots — the product story of the build. */
function ShotsStrip({ shots }: { shots: BenchShot[] }) {
  const [open, setOpen] = useState<number | null>(null);
  if (shots.length === 0) return null;
  return (
    <>
      <div className="flex flex-wrap gap-3">
        {shots.map((s, i) => (
          <figure
            key={s.name}
            className="w-[260px] cursor-zoom-in overflow-hidden rounded border border-border-primary"
            onClick={() => setOpen(i)}
            role="presentation"
            title="Click to view full size"
          >
            <img
              src={`data:image/png;base64,${s.b64}`}
              alt={s.caption}
              className="block h-[160px] w-full bg-background-secondary object-cover object-top"
            />
            <figcaption
              className="px-2 py-1.5 text-[11px] font-bold text-white"
              style={{ backgroundColor: shotColor(s.name) }}
            >
              {s.caption}
            </figcaption>
          </figure>
        ))}
      </div>
      {open !== null && (
        <ShotLightbox
          shots={shots}
          index={open}
          onIndex={setOpen}
          onClose={() => setOpen(null)}
        />
      )}
    </>
  );
}

/** Solid stat tile — full border, saturated number, no washes. */
function StatTile({ label, value, color }: { label: string; value: string; color: string }) {
  return (
    <div className="rounded border border-border-primary px-4 py-3">
      <div className="text-2xl font-extrabold tabular-nums" style={{ color }}>
        {value}
      </div>
      <div className="mt-0.5 text-xs font-semibold uppercase tracking-wider text-text-secondary">
        {label}
      </div>
    </div>
  );
}

/** Solid saturated chip per outcome — every state names itself; a dead run never looks finished. */
const OUTCOME_COLORS: Record<SessionOutcome, string> = {
  running: PHASE_ACTIVE,
  finished: PHASE_DONE,
  did_not_finish: STATE_ERROR,
  did_not_start: '#8e44ad',
};
const OUTCOME_WORDS: Record<SessionOutcome, string> = {
  running: 'Running',
  finished: 'Finished',
  did_not_finish: 'Did not finish',
  did_not_start: 'Did not start',
};

function OutcomeChip({ session }: { session: BenchSession }) {
  const color = OUTCOME_COLORS[session.outcome];
  const dark = session.outcome === 'running';
  return (
    <span
      className="flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] font-bold"
      style={{ backgroundColor: color, color: dark ? '#1a1a1a' : '#fff' }}
    >
      {session.outcome === 'running' && (
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-[#1a1a1a]" aria-hidden />
      )}
      {OUTCOME_WORDS[session.outcome]}
      {session.outcome === 'finished' && (
        <span className="tabular-nums">
          {session.score != null ? ` · ${(session.score * 100).toFixed(1)}%` : ' · score missing'}
        </span>
      )}
    </span>
  );
}

/** One session under its benchmark's section: started stamp, honest state, delete. Keyed by runId. */
function SessionRow({
  session,
  selected,
  onSelect,
  onDelete,
}: {
  session: BenchSession;
  selected: boolean;
  onSelect: () => void;
  onDelete: () => void;
}) {
  const when = fmtWhen(session.startedAt) ?? session.startedAt;
  // Deleting needs the engine's runId; the just-launched row has none yet (and the running
  // session refuses deletion in main anyway).
  const undeletable =
    session.outcome === 'running'
      ? 'A running session cannot be deleted — cancel the run first'
      : session.runId == null
        ? 'This session has no run id yet — it appears moments after launch'
        : null;
  return (
    <div
      className="flex items-center gap-3 rounded border-2 px-3 py-2"
      style={{
        borderColor: selected ? 'var(--color-block-teal)' : 'var(--color-border-primary)',
      }}
    >
      <button
        type="button"
        onClick={onSelect}
        aria-pressed={selected}
        title="Show this session's result"
        className="flex flex-1 items-center gap-3 text-left"
      >
        <span className="text-sm font-semibold tabular-nums text-text-primary">{when}</span>
        <OutcomeChip session={session} />
      </button>
      <button
        type="button"
        onClick={onDelete}
        disabled={undeletable != null}
        aria-label={`Delete session ${session.runId ?? session.startedAt}`}
        title={undeletable ?? 'Delete this session'}
        className="rounded p-1.5 text-text-secondary hover:text-[#e5484d] disabled:opacity-40"
      >
        <Trash2 className="h-4 w-4" />
      </button>
    </div>
  );
}

/** Node count + Run/Cancel. Rendered inside the CURRENT benchmark's section while idle (the user
 *  cannot choose a benchmark — only the latest is runnable) and inside the live-run block while
 *  running (so Cancel sits beside what it cancels). Never both at once. */
function RunControls({
  nodes,
  onNodes,
  running,
  cancelling,
  onRun,
  onCancel,
  lockedId,
}: {
  nodes: NodeChoice;
  onNodes: (n: NodeChoice) => void;
  running: boolean;
  cancelling: boolean;
  onRun: () => void;
  onCancel: () => void;
  lockedId: string;
}) {
  const lockedWhy = 'Locked while a run is live — the node count is fixed at launch';
  return (
    <div className="flex flex-wrap items-center gap-3">
      <span className="text-sm text-text-secondary">Nodes</span>
      <div className="flex overflow-hidden rounded border border-border-primary">
        {NODE_CHOICES.map((n) => (
          <button
            key={n}
            type="button"
            onClick={() => onNodes(n)}
            disabled={running}
            aria-pressed={nodes === n}
            aria-describedby={running ? lockedId : undefined}
            title={running ? lockedWhy : `Run on ${n} node${n === 1 ? '' : 's'}`}
            className={`px-4 py-2 text-sm font-semibold tabular-nums transition-colors ${
              nodes === n
                ? 'bg-[var(--color-block-teal)] text-white'
                : 'bg-background-secondary text-text-secondary hover:text-text-primary'
            }`}
          >
            {n}
          </button>
        ))}
      </div>
      {running && (
        <span id={lockedId} className="text-xs font-semibold text-text-secondary">
          locked while the run is live
        </span>
      )}

      {running ? (
        <button
          type="button"
          onClick={onCancel}
          disabled={cancelling}
          title={
            cancelling
              ? 'Cancelling — the engine, the vendor sim and the scorer are being stopped'
              : 'Stop this run'
          }
          className="ml-auto flex items-center gap-2 rounded bg-background-danger px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {cancelling ? <Loader2 className="h-4 w-4 animate-spin" /> : <XCircle className="h-4 w-4" />}
          {cancelling ? 'Cancelling…' : 'Cancel run'}
        </button>
      ) : (
        <button
          type="button"
          onClick={onRun}
          className="ml-auto flex items-center gap-2 rounded bg-[var(--color-block-teal)] px-4 py-2 text-sm font-semibold text-white"
        >
          <Play className="h-4 w-4" />
          Run benchmark
        </button>
      )}
    </div>
  );
}

/**
 * The selected session's result, rendered INSIDE its own benchmark's section — a session compares
 * only within its own era. Every outcome renders its own truth: a run that died before scoring
 * says so in words; it never borrows the look of a finished one.
 */
function SessionDetail({
  session,
  baselines,
  catalogAbsent,
  fromCatalog,
  mine,
  mineMatched,
  shots,
  publishSlot,
}: {
  session: BenchSession;
  baselines: CatalogBaseline[];
  catalogAbsent: boolean;
  fromCatalog: boolean;
  mine: MineRow | null;
  mineMatched: boolean;
  shots: BenchShot[];
  publishSlot: ReactNode;
}) {
  const when = fmtWhen(session.startedAt) ?? session.startedAt;

  if (session.outcome === 'running') {
    return (
      <div
        className="rounded border-2 px-4 py-3 text-sm font-semibold text-text-primary"
        style={{ borderColor: PHASE_ACTIVE }}
      >
        This session is running — the result lands here when the run finishes. The live run panel
        above is the engine truth in the meantime.
      </div>
    );
  }
  if (session.outcome === 'did_not_start') {
    return (
      <div
        className="rounded px-4 py-3 text-sm font-bold text-white"
        style={{ backgroundColor: OUTCOME_COLORS.did_not_start }}
      >
        Started {when} — this run did not start: the engine never launched, so nothing was built and
        there is no score.
      </div>
    );
  }
  if (session.outcome === 'did_not_finish') {
    return (
      <div
        className="rounded px-4 py-3 text-sm font-bold text-white"
        style={{ backgroundColor: STATE_ERROR }}
      >
        Started {when} — this run did not finish: it ended before the scorer reached a verdict, so
        it has no score.
      </div>
    );
  }

  const ownRow: BenchmarkRow | null =
    session.score == null
      ? null
      : {
          label: 'Your fleet',
          score: session.score,
          tiers: session.tiers,
          mine: true,
          scorerVersion: session.scorerVersion,
        };
  const rows: BenchmarkRow[] = [
    ...baselines.map((b) => ({
      label: b.label || b.model,
      score: b.score,
      scorerVersion: session.scorerVersion,
    })),
    ...(ownRow ? [ownRow] : []),
  ].sort((a, b) => b.score - a.score);

  const started = Date.parse(session.startedAt);
  const ended = session.endedAt ? Date.parse(session.endedAt) : NaN;
  const wallMs = !Number.isNaN(started) && !Number.isNaN(ended) ? ended - started : null;

  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatTile
          label="Score"
          value={session.score != null ? `${(session.score * 100).toFixed(1)}%` : 'missing'}
          color={session.score != null ? 'var(--color-block-teal)' : STATE_ERROR}
        />
        {wallMs != null && (
          <StatTile label="Wall time" value={fmtElapsed(wallMs)} color="var(--color-node-2)" />
        )}
        {mineMatched && mine?.runMeta && (
          <StatTile
            label="Repair rounds"
            value={String(mine.runMeta.repairRounds)}
            color="var(--color-node-4)"
          />
        )}
        {mineMatched && mine?.runMeta && (
          <StatTile
            label="Engine events"
            value={mine.runMeta.engineEvents.toLocaleString()}
            color="var(--color-node-1)"
          />
        )}
      </div>

      {mineMatched && shots.length > 0 && (
        <section>
          <h3 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
            What it built — before and after repairs
          </h3>
          <div className="mt-3">
            <ShotsStrip shots={shots} />
          </div>
        </section>
      )}

      <section>
        <h3 className="text-xs font-bold uppercase tracking-widest text-text-secondary">Overall</h3>
        {baselines.length === 0 ? (
          <p
            className="mt-3 rounded px-4 py-3 text-sm font-bold text-white"
            style={{ backgroundColor: STATE_ERROR }}
          >
            {catalogAbsent
              ? 'Catalog unreachable — no comparison rows.'
              : fromCatalog
                ? 'The catalog publishes no baselines for this benchmark yet — your score stands alone.'
                : `The catalog carries no comparison rows for ${session.scorerVersion}.`}
          </p>
        ) : (
          <ScoreBars rows={rows} />
        )}
      </section>

      <section>
        <h3 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
          Where the points went
        </h3>
        <p className="mb-3 mt-1 max-w-[70ch] text-sm text-text-secondary">
          {TIER_LABELS.A} · {TIER_LABELS.B} · {TIER_LABELS.C} · {TIER_LABELS.D}. A build can be
          perfectly structured and still score nothing on behaviour — the split is the diagnosis.
        </p>
        {ownRow && session.tiers ? (
          <TierBreakdown rows={[ownRow]} />
        ) : (
          <p className="rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
            This session recorded no per-tier split.
          </p>
        )}
      </section>

      <section>
        <h3 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
          How this score was built
        </h3>
        {mineMatched && mine?.verdict ? (
          <>
            <p className="mb-4 mt-1 max-w-[80ch] text-sm text-text-secondary">
              Every number below is scorer evidence from YOUR run — the exact checks it ran, what
              each one saw, and what the misses cost. The formula:{' '}
              <span className="font-bold text-text-primary">
                60% core build + 15% journey + 10% visual + 5% performance + 10% hard block
              </span>
              .
            </p>
            <ScoringDetail verdict={mine.verdict} score={mine.score} />
          </>
        ) : mineMatched ? (
          <p className="mt-3 rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
            This stored result predates the detailed verdict — the full check-by-check breakdown
            appears from your next run.
          </p>
        ) : (
          <p className="mt-3 rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
            The check-by-check breakdown is kept with the latest stored result only — this session
            shows its score and tier split.
          </p>
        )}
      </section>

      {publishSlot}
    </div>
  );
}

type CatalogState =
  | { kind: 'loading' }
  | { kind: 'ok'; benchmarks: CatalogBenchmark[]; fetchedAt?: string; stale: boolean }
  | { kind: 'absent'; message: string };

interface EraSection {
  scorerVersion: string;
  title: string;
  current: boolean;
  frozen: boolean;
  baselines: CatalogBaseline[];
  /** False when the era exists only because sessions reference it — the catalog never named it. */
  fromCatalog: boolean;
  sessions: BenchSession[];
}

const startMs = (s: BenchSession): number => {
  const t = Date.parse(s.startedAt);
  return Number.isNaN(t) ? 0 : t;
};

/** Stable identity for a session row: the engine's runId, or — for the just-launched row whose
 *  runId is still null (it reconciles ~2s in, when .swarm/current-run.json appears) — its
 *  startedAt stamp, which never changes for a given session. Never an index. */
const sessionKey = (s: BenchSession): string => s.runId ?? `start-${s.startedAt}`;

/**
 * The benchmark page: one expand/collapse section per benchmark era, RETRIEVED from the site's
 * catalog — never baked. The CURRENT era carries the only Run button (the user cannot choose a
 * benchmark); every era lists its own sessions with honest outcomes, and a session's result
 * compares only against its own era's published baselines.
 */
export default function BenchmarkView() {
  const [nodes, setNodes] = useState<NodeChoice>(3);
  // The strip's editable values — what the NEXT run will use. Prefilled from the shared defaults
  // (localStorage `swarmSamplingDefaults`); passed into benchmarkRun where set knobs become env.
  const [sampling, setSampling] = useState<SamplingSettings>(() => loadSamplingDefaults());
  // Non-null while a run is live: the values that run LAUNCHED with (strip renders them read-only).
  const [launchedSampling, setLaunchedSampling] = useState<SamplingSettings | null>(null);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [pub, setPub] = useState<PublishState>({ kind: 'idle' });
  const [status, setStatus] = useState<string | null>(null);
  const [mine, setMine] = useState<MineRow | null>(null);
  const [title, setTitle] = useState('');
  const [shots, setShots] = useState<BenchShot[]>([]);
  const [activeWorkdir, setActiveWorkdir] = useState<string | null>(null);
  const [runStartedAt, setRunStartedAt] = useState<number | null>(null);
  const [lastLine, setLastLine] = useState<string | null>(null);
  const [scored, setScored] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [catalog, setCatalog] = useState<CatalogState>({ kind: 'loading' });
  const [sessions, setSessions] = useState<BenchSession[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  // The 'benchmark-started' fact that the site's current benchmark outruns this app's bundle —
  // each new launch restates or clears it, so a stale notice cannot outlive an app update.
  const [catalogMismatch, setCatalogMismatch] = useState<CatalogMismatch | null>(null);
  const [expandedByEra, setExpandedByEra] = useState<Record<string, boolean>>({});
  const [deleteTarget, setDeleteTarget] = useState<BenchSession | null>(null);
  const [deleting, setDeleting] = useState(false);

  // Live engine truth for the active run — the ONE poller on this route. It drives the phase strip
  // (present/finished) and is handed to SwarmRunPanel as `run`, so the panel renders from this state
  // instead of mounting a second 500ms poller on the same dir: two pollers doubled the IPC and let
  // the two copies disagree about the phase for a poll at a time.
  const swarm = useSwarmRun(activeWorkdir ?? undefined);
  const saveDefaults = useSaveSamplingDefaults();

  const loadShots = useCallback(async (workdir?: string) => {
    try {
      const s = await window.electron.benchmarkShots?.(workdir);
      if (Array.isArray(s)) setShots(s as BenchShot[]);
    } catch {
      // no screenshots is a normal state (probe unavailable on this machine)
    }
  }, []);

  const loadExisting = useCallback(async () => {
    try {
      const result = await window.electron.benchmarkRead?.();
      if (result) {
        setMine(result as MineRow);
        void loadShots((result as MineRow).workdir);
      }
    } catch {
      // no prior result on disk is the normal first-run state, not an error
    }
  }, [loadShots]);

  // Called unconditionally on mount: main's frozen publish gate and the catalogMismatch check
  // run off the CACHED catalog, and this mount call is what refreshes that cache.
  const loadCatalog = useCallback(async () => {
    try {
      const c = await window.electron.benchmarkCatalog?.();
      if (c && c.ok !== false && Array.isArray(c.benchmarks)) {
        setCatalog({
          kind: 'ok',
          benchmarks: c.benchmarks,
          fetchedAt: c.fetchedAt,
          stale: c.stale === true,
        });
      } else {
        // The stated absence — the view says "catalog unreachable" and shows NO comparison rows;
        // it never falls back to invented bars (the baked boards this replaced are deleted).
        setCatalog({
          kind: 'absent',
          message: c?.error ?? (c ? 'the catalog carried no benchmark list' : 'this build has no catalog bridge'),
        });
      }
    } catch (err) {
      setCatalog({ kind: 'absent', message: err instanceof Error ? err.message : String(err) });
    }
  }, []);

  const loadSessions = useCallback(async () => {
    try {
      const r = await window.electron.benchmarkSessions?.();
      if (r && Array.isArray(r.sessions)) setSessions(r.sessions);
      // No bridge (older preload) leaves the list as-is: an unreadable history must not
      // impersonate an empty one.
    } catch {
      // read failure: keep the last known list rather than flashing an empty history
    }
  }, []);

  useEffect(() => {
    void loadExisting();
    void loadCatalog();
    void loadSessions();
    // Re-attach to a run started before this mount — a run takes hours and must survive navigation.
    void window.electron.benchmarkStatus?.().then((s) => {
      if (s?.running && s.workdir) {
        setRunning(true);
        setActiveWorkdir(s.workdir);
        setRunStartedAt(s.startedAt ? Date.parse(s.startedAt) : Date.now());
        // main.ts kept the launched knobs with the run — the strip shows the truth, not this
        // mount's defaults.
        setLaunchedSampling(sanitizeSampling(s.sampling));
        // FINDING 17: scored/lastLine are MAIN's facts now. They used to be component state fed by a
        // log-line regex, so this re-attach restored neither and a recreated window's strip dropped a
        // finished 'done' back to a spinning 'score' until the child exited.
        setScored(s.scored === true);
        setLastLine(typeof s.lastLine === 'string' ? s.lastLine : null);
        setStatus(null);
      }
    });
  }, [loadExisting, loadCatalog, loadSessions]);

  // Stream the harness's lifecycle from main: workdir on start, stdout/stderr lines, terminal row.
  useEffect(() => {
    const onStarted = (_e: unknown, payload: unknown) => {
      const p = payload as {
        workdir?: string;
        startedAt?: string;
        sampling?: unknown;
        tier?: string;
        scorerVersion?: string;
        catalogMismatch?: CatalogMismatch;
      };
      if (p?.workdir) {
        setCatalogMismatch(p.catalogMismatch ?? null);
        setActiveWorkdir(p.workdir);
        setRunStartedAt(p.startedAt ? Date.parse(p.startedAt) : Date.now());
        setLaunchedSampling(sanitizeSampling(p.sampling));
        setScored(false);
        setLastLine(null);
        setShots([]);
        // The new session appears in its era's list the moment main registers it.
        void loadSessions();
      }
    };
    const onLog = (_e: unknown, payload: unknown) => {
      // The regex is gone from the renderer: `scored` rides every log payload from main, which owns
      // the fact (finding 17) — a scorer output change breaks ONE matcher in one process, and the
      // strip can never re-derive a stale answer from lines it happened to see.
      const p = payload as { line?: string; scored?: boolean };
      if (typeof p?.line === 'string') setLastLine(p.line);
      if (p?.scored === true) setScored(true);
    };
    const onFinished = (_e: unknown, payload: unknown) => {
      const p = payload as { row?: MineRow; error?: string; cancelled?: boolean };
      setRunning(false);
      setCancelling(false);
      setActiveWorkdir(null);
      setRunStartedAt(null);
      setLaunchedSampling(null);
      if (p?.cancelled) {
        setStatus('Run cancelled.');
      } else if (p?.row) {
        setMine(p.row);
        setStatus('Run complete.');
        void loadShots(p.row.workdir);
      } else if (p?.error) {
        setStatus(`The run failed: ${p.error}`);
      }
      // The terminating event also updates the session row — running must flip to its real
      // outcome, never linger because only a feed line changed.
      void loadSessions();
    };
    window.electron.on('benchmark-started', onStarted);
    window.electron.on('benchmark-log', onLog);
    window.electron.on('benchmark-finished', onFinished);
    return () => {
      window.electron.off('benchmark-started', onStarted);
      window.electron.off('benchmark-log', onLog);
      window.electron.off('benchmark-finished', onFinished);
    };
  }, [loadShots, loadSessions]);

  // Elapsed ticker while a run is live — and the safety net for a window recreated mid-run: the
  // 'benchmark-finished' event went to the old (destroyed) window, so also poll main's status and
  // fold back to the on-disk result when the run is over. Gated on activeWorkdir so the brief
  // moment between clicking Run and main registering the child can't read as "finished".
  useEffect(() => {
    if (!running) return;
    const iv = setInterval(() => {
      setNow(Date.now());
      if (!activeWorkdir) return;
      void window.electron.benchmarkStatus?.().then((s) => {
        if (s && !s.running) {
          setRunning(false);
          setCancelling(false);
          setActiveWorkdir(null);
          setRunStartedAt(null);
          setLaunchedSampling(null);
          void loadExisting();
          void loadSessions();
        }
      });
    }, 1000);
    return () => clearInterval(iv);
  }, [running, activeWorkdir, loadExisting, loadSessions]);

  const phase: BenchPhase = useMemo(() => {
    if (scored) return 'done';
    if (swarm.present && swarm.finished) return 'score';
    if (swarm.present) return 'build';
    return 'boot';
  }, [scored, swarm.present, swarm.finished]);

  // One section per era: the catalog's benchmarks, plus any era that exists only in the session
  // history (sessions from previous benchmarks may display even after the site moves on).
  const sections = useMemo<EraSection[]>(() => {
    const map = new Map<string, EraSection>();
    if (catalog.kind === 'ok') {
      for (const b of catalog.benchmarks) {
        map.set(b.scorerVersion, { ...b, fromCatalog: true, sessions: [] });
      }
    }
    for (const s of sessions) {
      if (!map.has(s.scorerVersion)) {
        map.set(s.scorerVersion, {
          scorerVersion: s.scorerVersion,
          title: s.scorerVersion,
          current: false,
          frozen: false,
          baselines: [],
          fromCatalog: false,
          sessions: [],
        });
      }
      map.get(s.scorerVersion)!.sessions.push(s);
    }
    const list = [...map.values()];
    for (const sec of list) sec.sessions.sort((a, b) => startMs(b) - startMs(a));
    list.sort(
      (a, b) =>
        Number(b.current) - Number(a.current) ||
        b.scorerVersion.localeCompare(a.scorerVersion, undefined, { numeric: true })
    );
    return list;
  }, [catalog, sessions]);

  const currentEra = catalog.kind === 'ok' ? catalog.benchmarks.find((b) => b.current) : undefined;

  // Default selection: the running session (its live truth is the page's point), else the newest.
  // When a running row's key changes (null runId reconciling into the real one ~2s in), the old
  // key stops matching and this re-picks the same live session under its new identity.
  useEffect(() => {
    if (sessions.length === 0) {
      if (selectedKey != null) setSelectedKey(null);
      return;
    }
    if (selectedKey != null && sessions.some((s) => sessionKey(s) === selectedKey)) return;
    const live = sessions.find((s) => s.outcome === 'running');
    const newest = sessions.slice().sort((a, b) => startMs(b) - startMs(a))[0];
    setSelectedKey(sessionKey(live ?? newest));
  }, [sessions, selectedKey]);

  const selectedSession = useMemo(
    () => sessions.find((s) => sessionKey(s) === selectedKey) ?? null,
    [sessions, selectedKey]
  );

  // The stored result (mine/result.json) is THE LAST run's row — attribute its verdict, shots and
  // publishability only to the newest finished session of its own era; an older sibling with a
  // similar score must not borrow them.
  const mineSessionKey = useMemo(() => {
    if (!mine) return null;
    const candidates = sessions.filter(
      (s) => s.outcome === 'finished' && s.scorerVersion === mine.scorerVersion
    );
    if (candidates.length === 0) return null;
    return sessionKey(candidates.reduce((a, b) => (startMs(b) > startMs(a) ? b : a)));
  }, [mine, sessions]);

  const run = useCallback(async () => {
    setRunning(true);
    setScored(false);
    setStatus(null);
    setLaunchedSampling(sampling);
    try {
      // No tier argument — the user cannot choose a benchmark; main runs the newest bundled one.
      const result = await window.electron.benchmarkRun?.(nodes, sampling);
      if (result) {
        setMine(result as MineRow);
        setStatus('Run complete.');
        void loadShots((result as MineRow).workdir);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setStatus(/cancelled/i.test(msg) ? 'Run cancelled.' : `The run failed: ${msg}`);
    } finally {
      setRunning(false);
      setCancelling(false);
      setActiveWorkdir(null);
      setRunStartedAt(null);
      setLaunchedSampling(null);
      void loadSessions();
    }
  }, [nodes, sampling, loadShots, loadSessions]);

  const cancel = useCallback(async () => {
    setConfirmCancel(false);
    setCancelling(true);
    try {
      const res = await window.electron.benchmarkCancel?.();
      if (res && !res.ok) {
        setStatus(`Cancel failed: ${res.error ?? 'unknown error'}`);
        setCancelling(false);
      }
    } catch (err) {
      setStatus(`Cancel failed: ${err instanceof Error ? err.message : String(err)}`);
      setCancelling(false);
    }
  }, []);

  const confirmDelete = useCallback(async () => {
    // The delete IPC takes the engine's runId; a row without one cannot be deleted (its delete
    // control is disabled with the reason, so reaching here without an id is a race, not a path).
    if (!deleteTarget?.runId) return;
    setDeleting(true);
    try {
      const res = await window.electron.benchmarkDeleteSession?.(deleteTarget.runId);
      if (res && res.ok === false) {
        setStatus(`Delete failed: ${res.error ?? 'the delete handler gave no reason'}`);
      } else {
        if (selectedKey === sessionKey(deleteTarget)) setSelectedKey(null);
        await loadSessions();
      }
    } catch (err) {
      setStatus(`Delete failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setDeleting(false);
      setDeleteTarget(null);
    }
  }, [deleteTarget, selectedKey, loadSessions]);

  const publish = useCallback(async () => {
    if (!mine) return;
    const chosen = title.trim();
    if (!chosen) return;
    setPub({ kind: 'publishing' });
    try {
      const res = await window.electron.benchmarkPublish?.({ title: chosen });
      if (res?.ok) {
        setPub({
          kind: 'accepted',
          title: chosen,
          score: mine.score,
          url: typeof res.url === 'string' ? res.url : null,
        });
      } else {
        // The server's OWN words (its 4xx body travels through main.ts verbatim), or main's
        // server-shaped refusal (`message` — e.g. the frozen-benchmark gate answered from the
        // cached catalog). Never a generic line; "no response" appears only when the handler
        // itself returned nothing.
        setPub({
          kind: 'error',
          message: res?.error ?? res?.message ?? 'the publish handler returned no response',
        });
      }
    } catch (err) {
      setPub({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
    }
  }, [mine, title]);

  const publishing = pub.kind === 'publishing';
  const publishable = mine != null && mine.runMeta != null;
  const modelProblem = modelIdProblem(mine?.modelId);
  const publishTitleProblem = titleProblem(title);
  const uid = useId();
  const lockedId = `${uid}-locked`;
  const modelId = `${uid}-model`;
  const modelHintId = `${uid}-model-hint`;
  const titleId = `${uid}-title`;
  const titleHintId = `${uid}-title-hint`;

  const selectedEra = selectedSession
    ? sections.find((sec) => sec.scorerVersion === selectedSession.scorerVersion)
    : undefined;
  const selectedFrozen = selectedEra?.frozen === true;
  const publishWhy = !publishable
    ? 'Run the benchmark (v2) first'
    : running
      ? 'Publishing waits for the run in progress to finish'
      : selectedFrozen
        ? 'Benchmark frozen — submissions closed'
        : selectedSession && selectedSession.publishable === false
          ? 'This session is not publishable — its stored result predates the v2 publisher'
          : modelProblem
            ? modelProblem
            : publishTitleProblem
              ? publishTitleProblem
              : null;

  // Publish belongs to the selected session's detail, and ONLY when that session is the one the
  // stored result describes — publishing posts mine/result.json, nothing else.
  const publishSection =
    mine && selectedSession && sessionKey(selectedSession) === mineSessionKey ? (
      <section className="rounded border border-border-primary p-4">
        <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
          Publish to leanzero.net
        </h2>
        <p className="mt-1 max-w-[70ch] text-sm text-text-secondary">
          Posts your score, the full check-by-check breakdown and the before/after screenshots
          under the title you choose. The result appears on the leanzero.net board immediately.
        </p>
        <div className="mt-3 flex flex-col gap-3">
          <div className="max-w-[560px]">
            <label
              htmlFor={modelId}
              className="mb-1 block text-xs font-bold uppercase tracking-wider text-text-secondary"
            >
              Model{' '}
              <span className="font-medium normal-case tracking-normal">
                (from the run — not editable)
              </span>
            </label>
            {/* READ-ONLY, not disabled: readOnly keeps the field labeled, focusable and
                selectable-for-copy; disabled would kill all three. The model id is engine
                truth (pool_resolved) — an editable field here publishes a lie. */}
            <Input
              id={modelId}
              value={mine.modelId ?? ''}
              readOnly
              className="cursor-default bg-background-secondary"
              aria-describedby={modelHintId}
            />
            <p id={modelHintId} className="mt-1 text-[11px] text-text-secondary">
              Engine truth from this run's pool — publishing sends exactly this.
              {modelProblem && (
                <span className="ml-1 font-bold" style={{ color: STATE_ERROR }}>
                  {modelProblem}
                </span>
              )}
            </p>
          </div>
          <div className="flex flex-wrap items-end gap-3">
            <div className="w-full max-w-[360px]">
              <label
                htmlFor={titleId}
                className="mb-1 block text-xs font-bold uppercase tracking-wider text-text-secondary"
              >
                Title <span style={{ color: STATE_ERROR }}>*</span>
              </label>
              <Input
                id={titleId}
                value={title}
                onChange={(e) => setTitle(e.target.value.slice(0, 80))}
                maxLength={80}
                placeholder="e.g. My M4 fleet first run"
                disabled={publishing}
                title={publishing ? 'Locked while publishing' : undefined}
                aria-invalid={publishTitleProblem != null}
                aria-describedby={titleHintId}
              />
              <p id={titleHintId} className="mt-1 text-[11px] text-text-secondary">
                Your name for this run — the public title on the board.
                {publishTitleProblem && (
                  <span className="ml-1 font-bold" style={{ color: STATE_ERROR }}>
                    {publishTitleProblem}
                  </span>
                )}
              </p>
            </div>
            <button
              type="button"
              onClick={publish}
              disabled={publishWhy != null || publishing}
              title={
                publishing
                  ? 'Publishing — waiting for leanzero.net'
                  : (publishWhy ?? 'Publish this result to leanzero.net')
              }
              className="flex items-center gap-2 rounded bg-[var(--color-block-teal)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-40"
            >
              {publishing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Upload className="h-4 w-4" />}
              {publishing ? 'Publishing…' : 'Publish'}
            </button>
          </div>
        </div>
        {selectedFrozen && (
          <p className="mt-2 text-xs font-bold" style={{ color: PHASE_ACTIVE }}>
            Benchmark frozen — submissions closed.
          </p>
        )}
        {!publishable && (
          <p className="mt-2 text-xs text-text-secondary">
            This result predates the v2 publisher — run the benchmark again to publish.
          </p>
        )}
        {/* The publish outcome — a solid saturated state block, never a gray status line.
            aria-live so the change is announced; the error carries the SERVER'S words. */}
        <div aria-live="polite" role="status">
          {pub.kind === 'accepted' && (
            <div
              className="mt-3 flex items-start gap-2 rounded px-4 py-3 text-sm font-semibold text-white"
              style={{ backgroundColor: PHASE_DONE }}
            >
              <BadgeCheck className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                Live on leanzero.net — &ldquo;{pub.title}&rdquo; ·{' '}
                {(pub.score * 100).toFixed(1)}%{pub.url ? ` · leanzero.net${pub.url}` : ''}
              </span>
            </div>
          )}
          {pub.kind === 'error' && (
            <div
              className="mt-3 flex items-start gap-2 rounded px-4 py-3 text-sm font-semibold text-white"
              style={{ backgroundColor: STATE_ERROR }}
            >
              <XCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>Publish failed: {pub.message}</span>
            </div>
          )}
        </div>
      </section>
    ) : null;

  const runControls = (
    <RunControls
      nodes={nodes}
      onNodes={setNodes}
      running={running}
      cancelling={cancelling}
      onRun={() => void run()}
      onCancel={() => setConfirmCancel(true)}
      lockedId={lockedId}
    />
  );

  return (
    <MainPanelLayout>
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-5xl px-6 py-8">
          <header className="flex flex-wrap items-end gap-4 border-b border-border-primary pb-5">
            <div>
              <h1 className="flex items-center gap-2 text-2xl font-bold text-text-primary">
                <Gauge className="h-6 w-6" />
                Benchmark
              </h1>
              <p className="mt-1 max-w-[60ch] text-sm text-text-secondary">
                Your fleet against frontier models on the same frozen build task, graded by running
                what it produces — not by asking a model what it thinks.
              </p>
            </div>
          </header>

          {/* Run settings — the sampling knobs the next run will use, editable until launch; while
              a run is live they freeze on the values that run launched with. EVERY unset knob —
              temperature included — falls through to the config/model default: the 0.2 benchmark
              pin was deleted in main.ts ("NO HARDCODED TEMPERATURE" — it overrode the per-model
              value Mihai sets in LM Studio), and a card still saying "0.2 (pinned)" claimed a pin
              the run no longer sends (caught live on r4-relaunch, 2026-08-30). */}
          <SamplingKnobs
            className="mt-6"
            value={launchedSampling ?? sampling}
            onChange={setSampling}
            active={running}
            onSaveDefaults={() => saveDefaults(sampling)}
          />

          {catalog.kind === 'absent' && (
            <p
              className="mt-6 rounded px-4 py-3 text-sm font-bold text-white"
              style={{ backgroundColor: STATE_ERROR }}
            >
              Catalog unreachable — no comparison rows. Sessions on this machine still render below;
              baselines return when leanzero.net is reachable. ({catalog.message})
            </p>
          )}
          {catalog.kind === 'ok' && catalog.stale && (
            <p
              className="mt-6 inline-flex rounded px-2.5 py-1 text-xs font-bold"
              style={{ backgroundColor: PHASE_ACTIVE, color: '#1a1a1a' }}
            >
              catalog cached {fmtWhen(catalog.fetchedAt) ?? catalog.fetchedAt ?? 'at an unknown time'}
            </p>
          )}
          {catalogMismatch && (
            <p
              className="mt-6 rounded px-4 py-3 text-sm font-bold"
              style={{ backgroundColor: PHASE_ACTIVE, color: '#1a1a1a' }}
            >
              The site&rsquo;s current benchmark is {catalogMismatch.siteCurrent}, but this app
              bundles {catalogMismatch.bundled} — the site&rsquo;s current benchmark needs an app
              update.
            </p>
          )}

          {running && (
            <section className="mt-6 flex flex-col gap-4">
              {runControls}
              <PhaseStrip
                phase={phase}
                lastLine={lastLine}
                elapsedMs={runStartedAt ? now - runStartedAt : 0}
              />
              <SwarmRunPanel workingDir={activeWorkdir ?? undefined} run={swarm} />
            </section>
          )}

          {/* No catalog current era to host the Run button — the controls still render (main owns
              which benchmark a run launches), just without a section to live in. */}
          {!running && currentEra == null && <section className="mt-6">{runControls}</section>}

          {status && (
            <p className="mt-4 rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
              {status}
            </p>
          )}

          <div className="mt-6 flex flex-col gap-3">
            {sections.map((sec) => {
              const selInSection =
                selectedSession != null && selectedSession.scorerVersion === sec.scorerVersion;
              // Open by default: the CURRENT era, the era holding the selected session (the
              // latest session's result shows without a click), and everything when the catalog
              // is absent. An explicit user toggle always wins.
              const expanded =
                expandedByEra[sec.scorerVersion] ??
                (sec.current || selInSection || catalog.kind !== 'ok');
              return (
                <section
                  key={sec.scorerVersion}
                  className="rounded border border-border-primary"
                >
                  <button
                    type="button"
                    onClick={() =>
                      setExpandedByEra((prev) => ({ ...prev, [sec.scorerVersion]: !expanded }))
                    }
                    aria-expanded={expanded}
                    className="flex w-full flex-wrap items-center gap-3 px-4 py-3 text-left"
                  >
                    {expanded ? (
                      <ChevronDown className="h-4 w-4 shrink-0 text-text-secondary" />
                    ) : (
                      <ChevronRight className="h-4 w-4 shrink-0 text-text-secondary" />
                    )}
                    <span className="text-base font-bold text-text-primary">{sec.title}</span>
                    <span className="font-mono text-xs text-text-secondary">{sec.scorerVersion}</span>
                    {sec.current ? (
                      <span
                        className="rounded px-2 py-0.5 text-[10px] font-extrabold tracking-wider text-white"
                        style={{ backgroundColor: PHASE_DONE }}
                      >
                        CURRENT
                      </span>
                    ) : sec.frozen ? (
                      <span
                        className="rounded px-2 py-0.5 text-[10px] font-extrabold tracking-wider"
                        style={{ backgroundColor: PHASE_ACTIVE, color: '#1a1a1a' }}
                      >
                        FROZEN
                      </span>
                    ) : null}
                    <span className="ml-auto text-xs font-semibold tabular-nums text-text-secondary">
                      {sec.sessions.length} session{sec.sessions.length === 1 ? '' : 's'}
                    </span>
                  </button>
                  {expanded && (
                    <div className="flex flex-col gap-3 border-t border-border-primary px-4 py-4">
                      {sec.current && !running && runControls}
                      {sec.frozen && (
                        <p className="text-xs font-bold" style={{ color: PHASE_ACTIVE }}>
                          Frozen on the site — sessions stay viewable; submissions are closed.
                        </p>
                      )}
                      {sec.sessions.length === 0 ? (
                        <p className="text-sm text-text-secondary">
                          {sec.current
                            ? 'No sessions yet — Run benchmark starts the first one.'
                            : 'No sessions from this benchmark on this machine.'}
                        </p>
                      ) : (
                        <div className="flex flex-col gap-2">
                          {sec.sessions.map((s) => (
                            <SessionRow
                              key={sessionKey(s)}
                              session={s}
                              selected={sessionKey(s) === selectedKey}
                              onSelect={() => setSelectedKey(sessionKey(s))}
                              onDelete={() => setDeleteTarget(s)}
                            />
                          ))}
                        </div>
                      )}
                      {selInSection && selectedSession && (
                        <div
                          className="mt-2 border-t-2 pt-4"
                          style={{ borderColor: 'var(--color-block-teal)' }}
                        >
                          <SessionDetail
                            session={selectedSession}
                            baselines={sec.baselines}
                            catalogAbsent={catalog.kind === 'absent'}
                            fromCatalog={sec.fromCatalog}
                            mine={mine}
                            mineMatched={sessionKey(selectedSession) === mineSessionKey}
                            shots={shots}
                            publishSlot={publishSection}
                          />
                        </div>
                      )}
                    </div>
                  )}
                </section>
              );
            })}
            {sections.length === 0 && catalog.kind !== 'absent' && (
              <p className="rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
                {catalog.kind === 'loading'
                  ? 'Loading the benchmark catalog…'
                  : 'The catalog names no benchmarks and this machine has no sessions yet.'}
              </p>
            )}
          </div>

          <footer className="mt-10 border-t border-border-primary pt-4 text-xs text-text-secondary">
            Comparison rows are retrieved from leanzero.net's published board for each benchmark —
            nothing is baked into the app, so a shipped number can never outlive the board it came
            from. Scores below 100 are expected: the finesse tier is graded against a theoretical
            optimum, and a perfect score would mean the task had stopped measuring.
          </footer>
        </div>
      </div>

      <ConfirmationModal
        isOpen={confirmCancel}
        title="Cancel this benchmark run?"
        message="The engine, the vendor sim and the scorer are all stopped. Nothing is scored and nothing is published — the run is simply gone."
        confirmLabel="Cancel the run"
        cancelLabel="Keep running"
        confirmVariant="destructive"
        onConfirm={() => void cancel()}
        onCancel={() => setConfirmCancel(false)}
      />

      <ConfirmationModal
        isOpen={deleteTarget != null}
        title="Delete this benchmark session?"
        message={
          deleteTarget
            ? `Started ${fmtWhen(deleteTarget.startedAt) ?? deleteTarget.startedAt} · ${OUTCOME_WORDS[deleteTarget.outcome]}${
                deleteTarget.outcome === 'finished' && deleteTarget.score != null
                  ? ` at ${(deleteTarget.score * 100).toFixed(1)}%`
                  : ''
              }. The session and its stored result are removed from this machine — anything already published stays on the board.`
            : ''
        }
        confirmLabel="Delete session"
        cancelLabel="Keep it"
        confirmVariant="destructive"
        isSubmitting={deleting}
        onConfirm={() => void confirmDelete()}
        onCancel={() => setDeleteTarget(null)}
      />
    </MainPanelLayout>
  );
}

export type { BenchmarkRow, Tier };
