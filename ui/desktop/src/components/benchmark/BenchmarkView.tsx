import { useCallback, useEffect, useId, useMemo, useState, type ReactNode } from 'react';
import {
  AlertTriangle,
  BadgeCheck,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleSlash,
  Loader2,
  Play,
  Trash2,
  Upload,
  XCircle,
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
import SamplingKnobs from '../swarm/SamplingKnobs';
import { useSaveSamplingDefaults } from '../swarm/useSamplingDefaults';
import { loadSamplingDefaults, sanitizeSampling, type SamplingSettings } from '../swarm/sampling';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { acpReadConfig } from '../../acp/config';
import type { SwarmConfig, SwarmDeviceRow } from '../settings/swarm/golden';
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  KeyValue,
  PageHeader,
  Panel,
  Segmented,
  StatusDot,
  DISABLED,
  FOCUS,
  MOTION,
  RADIUS,
  SPACE,
  SURFACE,
  TNUM,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type DataTableColumn,
  type Tone,
} from '../lz';

const NODE_CHOICES = [1, 2, 3] as const;
type NodeChoice = (typeof NODE_CHOICES)[number];

/**
 * How many nodes the CONFIGURED pool can offer: the enabled swarm devices, floored at 1 and capped at
 * the largest choice. A pool of one MLX sidecar (this machine, 2026-09-05) was offered 1/2/3 with 3
 * preselected — a run asking for nodes the pool does not have. No/empty devices is the legacy LM
 * Studio discovery pool and keeps every choice; with ≥3 devices nothing changes.
 */
export function nodeCapFor(cfg: SwarmConfig | null): NodeChoice {
  const rows: SwarmDeviceRow[] = Array.isArray(cfg?.devices) ? cfg.devices : [];
  if (rows.length === 0) return NODE_CHOICES[NODE_CHOICES.length - 1];
  const enabled = rows.filter((d) => d.enabled !== false).length;
  const max = NODE_CHOICES[NODE_CHOICES.length - 1];
  const capped = Math.max(1, Math.min(max, enabled));
  return (NODE_CHOICES.find((n) => n === capped) ?? max) as NodeChoice;
}

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
  /** The engine's run id main stamps at close (null when .swarm/current-run.json never appeared) —
   *  the exact mine↔session join. Absent on rows written by builds before the stamp landed. */
  runId?: string | null;
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


function fmtElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m ${s % 60}s`;
}

/** Short stamp for a moment ("Aug 17, 18:27") — how a stored result or a live run stays
 *  identifiable. Takes the ISO string a result carries or the epoch ms a live run keeps. */
function fmtWhen(when: string | number | undefined | null): string | null {
  if (when == null || when === '') return null;
  const t = typeof when === 'number' ? when : Date.parse(when);
  if (Number.isNaN(t)) return null;
  return new Date(t).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** A Studio text input: outline, radius 6, ink-4 placeholder, the err border when aria-invalid. */
const INPUT = cx(
  'h-8 w-full bg-lz-surface px-3 text-lz-body text-lz-ink placeholder:text-lz-ink-4 aria-[invalid=true]:border-lz-err',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION,
  DISABLED
);

const BAND_ICON: Record<Tone, ReactNode> = {
  ok: <CheckCircle2 />,
  warn: <AlertTriangle />,
  err: <XCircle />,
  stopped: <CircleSlash />,
  accent: <Loader2 className="animate-spin" />,
  secondary: null,
};

/** A one-line message in a solid tone fill — the status / refusal banner register. */
function ToneBand({ tone, children }: { tone: Tone; children: ReactNode }) {
  return (
    <div
      role="status"
      data-testid="tone-band"
      data-tone={tone}
      className={cx(
        'flex items-center gap-2 px-4 py-2.5 text-lz-body [&>svg]:size-4 [&>svg]:shrink-0',
        WEIGHT.medium,
        RADIUS.card,
        TONE_FILL[tone]
      )}
    >
      {BAND_ICON[tone]}
      <span>{children}</span>
    </div>
  );
}

/** The tone a status line carries, read from its own words — the words are the fact, the fill
 *  only agrees with them. */
function statusTone(status: string): Tone {
  if (/failed/i.test(status)) return 'err';
  if (/cancelled/i.test(status)) return 'stopped';
  if (/complete|published/i.test(status)) return 'ok';
  return 'accent';
}

/**
 * BENCHMARK PIPELINE panel — the harness AROUND the swarm: which phase is live, what already
 * finished, the run's own facts and the harness's latest output line — never a bare spinner. The
 * swarm-build phase gets its full live panel below; this panel covers the phases that are NOT the
 * swarm (vendor sim boot, scoring). Started and the run directory come from main's status — the
 * tier and node count of a re-attached run are NOT known here and are deliberately not claimed.
 */
function PhaseStrip({
  phase,
  lastLine,
  elapsedMs,
  startedAt,
  workdir,
}: {
  phase: BenchPhase;
  lastLine: string | null;
  elapsedMs: number;
  startedAt: number | null;
  workdir: string | null;
}) {
  const activeIdx = PHASES.findIndex((p) => p.key === phase);
  return (
    <Panel
      title="Benchmark pipeline"
      headerRight={
        <>
          <StatusDot tone="accent" live label="run in progress" />
          <span className={cx(TYPE.meta, TNUM)}>{fmtElapsed(elapsedMs)}</span>
        </>
      }
    >
      <p className={TYPE.bodyMuted}>
        The harness around the swarm — boot, build, scoring. The swarm build has its full live panel
        below.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        {PHASES.map((p, i) => {
          const state = i < activeIdx ? 'done' : i === activeIdx ? 'active' : 'pending';
          return (
            <Chip
              key={p.key}
              tone={state === 'done' ? 'ok' : state === 'active' ? 'accent' : undefined}
              icon={
                state === 'done' ? (
                  <Check />
                ) : state === 'active' ? (
                  <Loader2 className="animate-spin" />
                ) : undefined
              }
            >
              {p.label}
            </Chip>
          );
        })}
      </div>
      <KeyValue
        dense
        className="mt-4"
        aria-label="Run facts"
        items={[
          { key: 'started', label: 'Started', value: fmtWhen(startedAt) ?? '—' },
          { key: 'workdir', label: 'Run directory', value: workdir ?? '—', mono: true },
        ]}
      />
      {lastLine && (
        <div
          title={lastLine}
          className={cx(
            'mt-3 truncate px-3 py-2 font-mono text-lz-mono text-lz-ink-2',
            SURFACE.inset,
            RADIUS.control
          )}
        >
          {lastLine.length > 200 ? lastLine.slice(0, 197) + '…' : lastLine}
        </div>
      )}
    </Panel>
  );
}

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
      className={cx('fixed inset-0 z-50 flex flex-col', SURFACE.page)}
      onClick={onClose}
      role="presentation"
    >
      <div
        className={cx(
          'flex items-center justify-between gap-3 border-b bg-lz-surface px-4 py-2',
          SURFACE.hairline
        )}
        onClick={(e) => e.stopPropagation()}
        role="presentation"
      >
        <span className={cx(TYPE.h2, 'truncate')}>{shot.caption}</span>
        <div className="flex items-center gap-2">
          <span className={cx(TYPE.meta, TNUM)}>
            {index + 1} / {shots.length}
          </span>
          {shots.length > 1 && (
            <>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onIndex((index - 1 + shots.length) % shots.length)}
              >
                ‹ Prev
              </Button>
              <Button size="sm" variant="ghost" onClick={() => onIndex((index + 1) % shots.length)}>
                Next ›
              </Button>
            </>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void window.electron.toggleFullscreen?.()}
          >
            Fullscreen
          </Button>
          <Button size="sm" variant="secondary" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>
      <div className="flex-1 overflow-auto p-4" onClick={onClose} role="presentation">
        {/* No stopPropagation here: ANY click below the header closes the viewer. The automated
            journey drives this page blind over CDP — if a stale-coordinate click ever opens the
            viewer, the next click must escape it rather than being swallowed by the image. */}
        <img
          src={`data:image/png;base64,${shot.b64}`}
          alt={shot.caption}
          className="mx-auto block max-w-full bg-lz-surface"
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
            className={cx('w-[260px] cursor-zoom-in overflow-hidden', SURFACE.card)}
            onClick={() => setOpen(i)}
            role="presentation"
            title="Click to view full size"
          >
            <img
              src={`data:image/png;base64,${s.b64}`}
              alt={s.caption}
              className={cx('block h-[160px] w-full object-cover object-top', SURFACE.inset)}
            />
            <figcaption className={cx('border-t px-2.5 py-1.5', SURFACE.hairline, TYPE.meta)}>
              {s.caption}
            </figcaption>
          </figure>
        ))}
      </div>
      {open !== null && (
        <ShotLightbox shots={shots} index={open} onIndex={setOpen} onClose={() => setOpen(null)} />
      )}
    </>
  );
}

/** One number of the last run: h1-scale tabular figure over a meta label, in an inset well. */
function StatCell({ label, value, tone }: { label: string; value: string; tone?: Tone }) {
  return (
    <div className={cx('px-3 py-2.5', RADIUS.control, SURFACE.inset)}>
      <div className={cx('text-lz-h1', TNUM, tone ? TONE_TEXT[tone] : 'text-lz-ink')}>{value}</div>
      <div className={cx('mt-0.5', TYPE.meta)}>{label}</div>
    </div>
  );
}

const ABSENT = <span className="text-lz-ink-4">—</span>;

/** The board as a table: who | run | tier | nodes | started | score | duration. Catalog baselines
 *  publish only the overall number, so their other cells read as absent — never invented; the
 *  session's own row reads its start from the session and the rest from the stored result. */
function boardColumns(own: { startedAt: string; wallMs: number | null; nodes?: number }): DataTableColumn<BenchmarkRow>[] {
  return [
    {
      key: 'who',
      header: <span className="sr-only">Entrant</span>,
      width: 28,
      cell: (r) => (
        <StatusDot
          tone={r.mine ? 'accent' : 'stopped'}
          label={r.mine ? 'your fleet' : 'baseline'}
        />
      ),
    },
    {
      key: 'run',
      header: 'Run',
      cell: (r) => <span className={cx(r.mine && WEIGHT.semibold)}>{r.label}</span>,
    },
    {
      key: 'tier',
      header: 'Tier',
      cell: (r) => <span className="text-lz-ink-2">{r.scorerVersion}</span>,
    },
    {
      key: 'nodes',
      header: 'Nodes',
      numeric: true,
      cell: (r) => (r.mine ? (own.nodes ?? r.nodes ?? ABSENT) : (r.nodes ?? ABSENT)),
    },
    {
      key: 'started',
      header: 'Started',
      numeric: true,
      cell: (r) => (r.mine ? (fmtWhen(own.startedAt) ?? ABSENT) : ABSENT),
    },
    {
      key: 'score',
      header: 'Score',
      numeric: true,
      cell: (r) => (
        <span className={cx(WEIGHT.semibold, r.mine && TONE_TEXT.accent)}>
          {(r.score * 100).toFixed(1)}%
        </span>
      ),
    },
    {
      key: 'duration',
      header: 'Duration',
      numeric: true,
      cell: (r) =>
        r.mine
          ? own.wallMs != null
            ? fmtElapsed(own.wallMs)
            : ABSENT
          : typeof r.wallSecs === 'number'
            ? fmtElapsed(r.wallSecs * 1000)
            : ABSENT,
    },
  ];
}

/** Solid tone per outcome — every state names itself; a dead run never looks finished. */
const OUTCOME_TONE: Record<SessionOutcome, Tone> = {
  running: 'accent',
  finished: 'ok',
  did_not_finish: 'err',
  did_not_start: 'stopped',
};
const OUTCOME_WORDS: Record<SessionOutcome, string> = {
  running: 'Running',
  finished: 'Finished',
  did_not_finish: 'Did not finish',
  did_not_start: 'Did not start',
};

function OutcomeChip({ session }: { session: BenchSession }) {
  return (
    <Chip
      tone={OUTCOME_TONE[session.outcome]}
      icon={
        session.outcome === 'running' ? (
          // DESIGN.md motion: the live dot SCALES (animate-lz-live), it never fades.
          <span className={cx('inline-block size-1.5 animate-lz-live bg-white', RADIUS.pill)} />
        ) : undefined
      }
    >
      {OUTCOME_WORDS[session.outcome]}
      {session.outcome === 'finished' && (
        <span className={TNUM}>
          {session.score != null ? ` · ${(session.score * 100).toFixed(1)}%` : ' · score missing'}
        </span>
      )}
    </Chip>
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
      className={cx(
        'flex items-center gap-3 px-3 py-2',
        RADIUS.control,
        selected ? cx(SURFACE.selectedRing, 'border border-transparent') : SURFACE.outline
      )}
      data-selected={selected || undefined}
    >
      <button
        type="button"
        onClick={onSelect}
        aria-pressed={selected}
        title="Show this session's result"
        className={cx('flex flex-1 items-center gap-3 text-left', RADIUS.control, FOCUS)}
      >
        <span className={cx('text-lz-body text-lz-ink', WEIGHT.semibold, TNUM)}>{when}</span>
        <OutcomeChip session={session} />
      </button>
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        onClick={onDelete}
        disabled={undeletable != null}
        aria-label={`Delete session ${session.runId ?? session.startedAt}`}
        title={undeletable ?? 'Delete this session'}
        icon={<Trash2 />}
      />
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
      <ToneBand tone="accent">
        This session is running — the result lands here when the run finishes. The live run panel
        above is the engine truth in the meantime.
      </ToneBand>
    );
  }
  if (session.outcome === 'did_not_start') {
    return (
      <ToneBand tone="stopped">
        Started {when} — this run did not start: the engine never launched, so nothing was built and
        there is no score.
      </ToneBand>
    );
  }
  if (session.outcome === 'did_not_finish') {
    return (
      <ToneBand tone="err">
        Started {when} — this run did not finish: it ended before the scorer reached a verdict, so
        it has no score.
      </ToneBand>
    );
  }

  const ownRow: BenchmarkRow | null =
    session.score == null
      ? null
      : {
          label: 'Your fleet',
          score: session.score,
          tiers: session.tiers,
          nodes: session.nodes,
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
  const noRows = catalogAbsent
    ? 'Catalog unreachable — no comparison rows.'
    : fromCatalog
      ? 'The catalog publishes no baselines for this benchmark yet — your score stands alone.'
      : `The catalog carries no comparison rows for ${session.scorerVersion}.`;

  return (
    <div className={cx('flex flex-col', SPACE.section)}>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCell
          label="Score"
          value={session.score != null ? `${(session.score * 100).toFixed(1)}%` : 'missing'}
          tone={session.score != null ? 'accent' : 'err'}
        />
        {wallMs != null && <StatCell label="Wall time" value={fmtElapsed(wallMs)} />}
        {mineMatched && mine?.runMeta && (
          <StatCell label="Repair rounds" value={String(mine.runMeta.repairRounds)} />
        )}
        {mineMatched && mine?.runMeta && (
          <StatCell label="Engine events" value={mine.runMeta.engineEvents.toLocaleString()} />
        )}
      </div>

      {mineMatched && shots.length > 0 && (
        <Panel
          title="What it built"
          count={shots.length}
          headerRight={<span className={TYPE.meta}>before and after repairs</span>}
        >
          <ShotsStrip shots={shots} />
        </Panel>
      )}

      <Panel
        title="Board"
        count={rows.length}
        headerRight={<span className={cx(TYPE.meta, TNUM)}>scorer {session.scorerVersion}</span>}
        padded={false}
      >
        <DataTable
          aria-label="Benchmark board"
          columns={boardColumns({ startedAt: session.startedAt, wallMs, nodes: session.nodes })}
          rows={rows}
          rowKey={(r) => (r.mine ? 'mine' : r.label)}
          empty={<EmptyState title="No entrants" body={noRows} />}
        />
      </Panel>

      <Panel title="Overall">
        {baselines.length === 0 ? <ToneBand tone="err">{noRows}</ToneBand> : <ScoreBars rows={rows} />}
      </Panel>

      <Panel title="Where the points went">
        <p className={cx('mb-3 max-w-[70ch]', TYPE.bodyMuted)}>
          {TIER_LABELS.A} · {TIER_LABELS.B} · {TIER_LABELS.C} · {TIER_LABELS.D}. A build can be
          perfectly structured and still score nothing on behaviour — the split is the diagnosis.
        </p>
        {ownRow && session.tiers ? (
          <TierBreakdown rows={[ownRow]} />
        ) : (
          <p className={TYPE.bodyMuted}>This session recorded no per-tier split.</p>
        )}
      </Panel>

      <Panel title="How this score was built">
        {mineMatched && mine?.verdict ? (
          <>
            <p className={cx('mb-4 max-w-[80ch]', TYPE.bodyMuted)}>
              Every number below is scorer evidence from YOUR run — the exact checks it ran, what
              each one saw, and what the misses cost. The formula:{' '}
              <span className={cx(WEIGHT.semibold, 'text-lz-ink')}>
                60% core build + 15% journey + 10% visual + 5% performance + 10% hard block
              </span>
              .
            </p>
            <ScoringDetail verdict={mine.verdict} score={mine.score} />
          </>
        ) : mineMatched ? (
          <p className={TYPE.bodyMuted}>
            This stored result predates the detailed verdict — the full check-by-check breakdown
            appears from your next run.
          </p>
        ) : (
          <p className={TYPE.bodyMuted}>
            The check-by-check breakdown is kept with the latest stored result only — this session
            shows its score and tier split.
          </p>
        )}
      </Panel>

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
 * catalog — never baked. The CURRENT era is the only one a run can enter (the user cannot choose a
 * benchmark — the header's launch runs the newest bundled one); every era lists its own sessions
 * with honest outcomes, and a session's result compares only against its own era's published
 * baselines.
 */
export default function BenchmarkView() {
  const [nodes, setNodes] = useState<NodeChoice>(3);
  // The pool's size caps the offered node counts and is the default; read once per mount (a device
  // edit is a config change, and the next mount sees it). Unreadable config keeps every choice.
  const [nodeCap, setNodeCap] = useState<NodeChoice>(3);
  useEffect(() => {
    let alive = true;
    void acpReadConfig('swarm', false)
      .then((raw) => {
        if (!alive) return;
        const cap = nodeCapFor((raw as SwarmConfig | null) ?? null);
        setNodeCap(cap);
        setNodes((n) => (n > cap ? cap : n));
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);
  const nodeChoices = NODE_CHOICES.filter((n) => n <= nodeCap);
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
    // Rows stamped with the engine's run id name their session — join on it EXACTLY. A newer
    // finished sibling in the era must not borrow this result; and if the named session was
    // deleted, the result attributes to nothing rather than to a neighbour.
    if (mine.runId) return mine.runId;
    // Newest-finished-of-era heuristic: ONLY the fallback for rows written by builds older than
    // the runId stamp — new rows never reach this branch.
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
  const lockedWhy = 'Locked while a run is live — the node count is fixed at launch';

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
      <Panel title="Publish to leanzero.net">
        <p className={cx('max-w-[70ch]', TYPE.bodyMuted)}>
          Posts your score, the full check-by-check breakdown and the before/after screenshots
          under the title you choose. The result appears on the leanzero.net board immediately.
        </p>
        <div className="mt-4 flex flex-col gap-4">
          <div className="max-w-[560px]">
            <label htmlFor={modelId} className={cx('mb-1.5 block', TYPE.meta)}>
              Model <span className="text-lz-ink-4">(from the run — not editable)</span>
            </label>
            {/* READ-ONLY, not disabled: readOnly keeps the field labeled, focusable and
                selectable-for-copy; disabled would kill all three. The model id is engine
                truth (pool_resolved) — an editable field here publishes a lie. */}
            <input
              id={modelId}
              type="text"
              value={mine.modelId ?? ''}
              readOnly
              aria-describedby={modelHintId}
              className={cx(INPUT, 'cursor-default bg-lz-surface-2')}
            />
            <p id={modelHintId} className={cx('mt-1.5', TYPE.meta)}>
              Engine truth from this run&apos;s pool — publishing sends exactly this.
              {modelProblem && (
                <span className={cx('ml-1', WEIGHT.medium, TONE_TEXT.err)}>{modelProblem}</span>
              )}
            </p>
          </div>
          <div className="flex flex-wrap items-end gap-3">
            <div className="w-full max-w-[360px]">
              <label htmlFor={titleId} className={cx('mb-1.5 block', TYPE.meta)}>
                Title <span className={TONE_TEXT.err}>*</span>
              </label>
              <input
                id={titleId}
                type="text"
                value={title}
                onChange={(e) => setTitle(e.target.value.slice(0, 80))}
                maxLength={80}
                placeholder="e.g. My M4 fleet first run"
                disabled={publishing}
                title={publishing ? 'Locked while publishing' : undefined}
                aria-invalid={publishTitleProblem != null}
                aria-describedby={titleHintId}
                className={INPUT}
              />
              <p id={titleHintId} className={cx('mt-1.5', TYPE.meta)}>
                Your name for this run — the public title on the board.
                {publishTitleProblem && (
                  <span className={cx('ml-1', WEIGHT.medium, TONE_TEXT.err)}>
                    {publishTitleProblem}
                  </span>
                )}
              </p>
            </div>
            <Button
              variant="primary"
              onClick={publish}
              disabled={publishWhy != null || publishing}
              title={
                publishing
                  ? 'Publishing — waiting for leanzero.net'
                  : (publishWhy ?? 'Publish this result to leanzero.net')
              }
              icon={publishing ? <Loader2 className="animate-spin" /> : <Upload />}
            >
              {publishing ? 'Publishing…' : 'Publish'}
            </Button>
          </div>
        </div>
        {selectedFrozen && (
          <p className={cx('mt-3', TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}>
            Benchmark frozen — submissions closed.
          </p>
        )}
        {!publishable && (
          <p className={cx('mt-3', TYPE.meta)}>
            This result predates the v2 publisher — run the benchmark again to publish.
          </p>
        )}
        {/* The publish outcome — a solid saturated state block, never a gray status line.
            aria-live so the change is announced; the error carries the SERVER'S words. */}
        <div aria-live="polite" role="status">
          {pub.kind === 'accepted' && (
            <div
              className={cx(
                'mt-3 flex items-start gap-2 px-4 py-3 text-lz-body [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0',
                WEIGHT.medium,
                RADIUS.card,
                TONE_FILL.ok
              )}
            >
              <BadgeCheck />
              <span>
                Live on leanzero.net — &ldquo;{pub.title}&rdquo; ·{' '}
                {(pub.score * 100).toFixed(1)}%{pub.url ? ` · leanzero.net${pub.url}` : ''}
              </span>
            </div>
          )}
          {pub.kind === 'error' && (
            <div
              className={cx(
                'mt-3 flex items-start gap-2 px-4 py-3 text-lz-body [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0',
                WEIGHT.medium,
                RADIUS.card,
                TONE_FILL.err
              )}
            >
              <XCircle />
              <span>Publish failed: {pub.message}</span>
            </div>
          )}
        </div>
      </Panel>
    ) : null;

  return (
    <MainPanelLayout>
      <div className={cx('flex-1 overflow-y-auto', SURFACE.page)}>
        <div className={cx('mx-auto flex w-full max-w-5xl flex-col', SPACE.page, SPACE.section)}>
          <PageHeader
            title="Benchmark"
            subtitle="Your fleet against frontier models on the same frozen build task, graded by running what it produces — not by asking a model what it thinks."
            actions={
              running ? (
                <Button
                  onClick={() => setConfirmCancel(true)}
                  disabled={cancelling}
                  title={
                    cancelling
                      ? 'Cancelling — the engine, the vendor sim and the scorer are being stopped'
                      : 'Stop this run'
                  }
                  icon={
                    cancelling ? (
                      <Loader2 className="animate-spin" />
                    ) : (
                      <XCircle className={TONE_TEXT.err} />
                    )
                  }
                >
                  {cancelling ? 'Cancelling…' : 'Cancel run'}
                </Button>
              ) : (
                <Button variant="primary" onClick={run} icon={<Play />}>
                  Run benchmark
                </Button>
              )
            }
          />

          {/* Run setup — the fleet size and the sampling knobs the next run will use, editable until
              launch; while a run is live they freeze on the values that run launched with. EVERY
              unset knob — temperature included — falls through to the config/model default: the 0.2
              benchmark pin was deleted in main.ts ("NO HARDCODED TEMPERATURE" — it overrode the
              per-model value Mihai sets in LM Studio), and a card still saying "0.2 (pinned)" claimed
              a pin the run no longer sends (caught live on r4-relaunch, 2026-08-30). There is no
              benchmark chooser: the launch runs the catalog's CURRENT benchmark, the only one open. */}
          <section aria-label="Run setup" className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-3">
              <span className={TYPE.meta}>Nodes</span>
              <Segmented
                as="buttons"
                aria-label="Nodes"
                options={nodeChoices.map((n) => ({
                  value: String(n),
                  label: <span className={TNUM}>{n}</span>,
                  title: running ? lockedWhy : `Run on ${n} node${n === 1 ? '' : 's'}`,
                  describedBy: running ? lockedId : undefined,
                }))}
                value={String(nodes)}
                onChange={(v) => {
                  const n = nodeChoices.find((c) => String(c) === v);
                  if (n != null) setNodes(n);
                }}
                disabled={running}
              />
              {running && (
                <span id={lockedId} className={TYPE.meta}>
                  locked while the run is live
                </span>
              )}
            </div>
            <SamplingKnobs
              value={launchedSampling ?? sampling}
              onChange={setSampling}
              active={running}
              onSaveDefaults={() => saveDefaults(sampling)}
            />
          </section>

          {catalog.kind === 'absent' && (
            <ToneBand tone="err">
              Catalog unreachable — no comparison rows. Sessions on this machine still render below;
              baselines return when leanzero.net is reachable. ({catalog.message})
            </ToneBand>
          )}
          {catalog.kind === 'ok' && catalog.stale && (
            <div>
              <Chip tone="warn">
                catalog cached {fmtWhen(catalog.fetchedAt) ?? catalog.fetchedAt ?? 'at an unknown time'}
              </Chip>
            </div>
          )}
          {catalogMismatch && (
            <ToneBand tone="warn">
              The site&rsquo;s current benchmark is {catalogMismatch.siteCurrent}, but this app
              bundles {catalogMismatch.bundled} — the site&rsquo;s current benchmark needs an app
              update.
            </ToneBand>
          )}

          {running && (
            <section className="flex flex-col gap-4">
              <PhaseStrip
                phase={phase}
                lastLine={lastLine}
                elapsedMs={runStartedAt ? now - runStartedAt : 0}
                startedAt={runStartedAt}
                workdir={activeWorkdir}
              />
              <SwarmRunPanel workingDir={activeWorkdir ?? undefined} run={swarm} />
            </section>
          )}

          {status && <ToneBand tone={statusTone(status)}>{status}</ToneBand>}

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
              <Panel
                key={sec.scorerVersion}
                padded={false}
                header={
                  <button
                    type="button"
                    onClick={() =>
                      setExpandedByEra((prev) => ({ ...prev, [sec.scorerVersion]: !expanded }))
                    }
                    aria-expanded={expanded}
                    className={cx(
                      'flex h-full w-full items-center gap-3 text-left [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-lz-ink-3',
                      FOCUS
                    )}
                  >
                    {expanded ? <ChevronDown /> : <ChevronRight />}
                    <span className={TYPE.h2}>{sec.title}</span>
                    <span className="font-mono text-lz-mono text-lz-ink-3">{sec.scorerVersion}</span>
                    {sec.current ? (
                      <Chip tone="ok">CURRENT</Chip>
                    ) : sec.frozen ? (
                      <Chip tone="warn">FROZEN</Chip>
                    ) : null}
                    <span className={cx('ml-auto', TYPE.meta, TNUM)}>
                      {sec.sessions.length} session{sec.sessions.length === 1 ? '' : 's'}
                    </span>
                  </button>
                }
              >
                {expanded && (
                  <div className={cx('flex flex-col gap-3', SPACE.card)}>
                    {sec.frozen && (
                      <p className={cx(TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}>
                        Frozen on the site — sessions stay viewable; submissions are closed.
                      </p>
                    )}
                    {sec.sessions.length === 0 ? (
                      <p className={TYPE.bodyMuted}>
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
                      <div className={cx('mt-2 border-t pt-4', SURFACE.hairline)}>
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
              </Panel>
            );
          })}
          {sections.length === 0 && catalog.kind !== 'absent' && (
            <Panel>
              <EmptyState
                title={catalog.kind === 'loading' ? 'Loading the benchmark catalog…' : 'No benchmarks yet'}
                body={
                  catalog.kind === 'loading'
                    ? 'Retrieving the published benchmarks from leanzero.net.'
                    : 'The catalog names no benchmarks and this machine has no sessions yet.'
                }
              />
            </Panel>
          )}

          <footer className={cx('border-t pt-4 text-lz-body text-lz-ink-2', SURFACE.hairline)}>
            Comparison rows are retrieved from leanzero.net&rsquo;s published board for each benchmark —
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
