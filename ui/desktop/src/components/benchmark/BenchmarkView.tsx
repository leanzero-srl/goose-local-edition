import { useCallback, useEffect, useId, useMemo, useState, type ReactNode } from 'react';
import {
  AlertTriangle,
  BadgeCheck,
  Check,
  CheckCircle2,
  CircleSlash,
  Loader2,
  Play,
  Upload,
  XCircle,
} from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import {
  BASELINES_BY_TIER,
  COMPARABLE_SCORER,
  DEFAULT_TIER,
  TIERS,
  TIER_SCORER,
  type BenchTier,
  TIER_LABELS,
  type BenchmarkRow,
  type Tier,
} from './baselines';
import { ScoreBars } from './ScoreBars';
import { TierBreakdown } from './TierBreakdown';
import { ScoringDetail, type VerdictDetail } from './ScoringDetail';
import { SwarmRunPanel } from '../swarm/SwarmRunPanel';
import { useSwarmRun } from '../swarm/useSwarmRun';
import SamplingKnobs from '../swarm/SamplingKnobs';
import { useSaveSamplingDefaults } from '../swarm/useSamplingDefaults';
import {
  loadSamplingDefaults,
  sanitizeSampling,
  type SamplingSettings,
} from '../swarm/sampling';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  KeyValue,
  PageHeader,
  Panel,
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

const MODEL_MIN_CHARS = 8;
const MODEL_MAX_CHARS = 120;

/** Why the Model field cannot be published as typed, or null when it can. The one rule the input's
 *  aria-invalid, its visible hint and the Publish button's tooltip all read from — the three used to
 *  each restate "8–120 characters" and the input carried aria-invalid with no message linked to it. */
export function modelFieldProblem(model: string): string | null {
  const n = model.trim().length;
  if (n === 0) return 'Required — the exact model id your fleet ran.';
  if (n < MODEL_MIN_CHARS) return `Too short — ${n} of at least ${MODEL_MIN_CHARS} characters.`;
  if (n > MODEL_MAX_CHARS) return `Too long — ${n} of at most ${MODEL_MAX_CHARS} characters.`;
  return null;
}

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

/** What each tier segment says and what its tooltip explains — the same words the buttons carried. */
const TIER_SEGMENT_LABEL: Record<BenchTier, string> = {
  'sb-7': 'sb-7 · rc',
  'sb-6': 'sb-6 · HARD',
  'sb-5.3': 'sb-5.3',
};
const TIER_BLURB: Record<BenchTier, string> = {
  'sb-7':
    'Meridian Payments Console — the current tier: full web console, 3D scene, concurrency and resilience under seeded SIGKILL (scorer sb-7.0-rc, UNCALIBRATED)',
  'sb-6':
    'VendorSync Pro — the hard tier: raw-WebGL 3D, webhooks, optimistic concurrency (scorer sb-6.0)',
  'sb-5.3': 'VendorSync — the standard tier (scorer sb-5.3)',
};

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

/**
 * The segmented strip the tier and node pickers share — the lz Segmented's exact class recipe on
 * plain buttons. Each toggle must stay a `button` carrying its own `title` and `aria-describedby`
 * (the locked-while-live reason the publish-form test pins); the primitive's radio options carry
 * neither, so the recipe is composed here from the same tokens.
 */
const STRIP = cx(
  'inline-flex items-center gap-0.5 bg-lz-surface p-0.5',
  SURFACE.outline,
  RADIUS.control
);
const SEGMENT =
  'inline-flex h-7 items-center gap-1.5 whitespace-nowrap rounded-[4px] px-2.5 text-[12px] font-lz-medium';
function segmentClass(active: boolean): string {
  return cx(
    SEGMENT,
    // A locked strip keeps its selection readable: the active segment stays the accent fill and
    // the others take the solid disabled neutral — never an opacity.
    active
      ? cx(SURFACE.selected, 'disabled:pointer-events-none')
      : cx('text-lz-ink-2 hover:text-lz-ink', SURFACE.hover, DISABLED),
    FOCUS,
    MOTION
  );
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
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onIndex((index + 1) % shots.length)}
              >
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

/** The board as a table: who | run | tier | nodes | started | score | duration. Baselines carry no
 *  start time; the user's row reads it from the stored result. */
function boardColumns(mine: MineRow | null): DataTableColumn<BenchmarkRow>[] {
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
    { key: 'nodes', header: 'Nodes', numeric: true, cell: (r) => r.nodes ?? ABSENT },
    {
      key: 'started',
      header: 'Started',
      numeric: true,
      cell: (r) => (r.mine ? (fmtWhen(mine?.runMeta?.startedAt) ?? ABSENT) : ABSENT),
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
      cell: (r) => (typeof r.wallSecs === 'number' ? fmtElapsed(r.wallSecs * 1000) : ABSENT),
    },
  ];
}

/**
 * The benchmark is two buttons and a node choice. Everything else on the page is the result.
 *
 * Baselines are BAKED, never run by the user: the frozen sb-5.2 cloud ladder ships as versioned
 * data, so a user's run costs them nothing and every board is comparable. Their own result is
 * added to the same roster and marked as theirs — but ONLY when the scorer versions match.
 */
export default function BenchmarkView() {
  const [nodes, setNodes] = useState<NodeChoice>(3);
  const [tier, setTier] = useState<BenchTier>(DEFAULT_TIER);
  // The strip's editable values — what the NEXT run will use. Prefilled from the shared defaults
  // (localStorage `swarmSamplingDefaults`); passed into benchmarkRun where set knobs become env.
  const [sampling, setSampling] = useState<SamplingSettings>(() => loadSamplingDefaults());
  // Non-null while a run is live: the values that run LAUNCHED with (strip renders them read-only).
  const [launchedSampling, setLaunchedSampling] = useState<SamplingSettings | null>(null);
  const [running, setRunning] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [mine, setMine] = useState<MineRow | null>(null);
  const [handle, setHandle] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [model, setModel] = useState('');
  const [shots, setShots] = useState<BenchShot[]>([]);
  const [activeWorkdir, setActiveWorkdir] = useState<string | null>(null);
  const [runStartedAt, setRunStartedAt] = useState<number | null>(null);
  const [lastLine, setLastLine] = useState<string | null>(null);
  const [scored, setScored] = useState(false);
  const [now, setNow] = useState(() => Date.now());

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
        setModel((result as MineRow).modelId ?? '');
        void loadShots((result as MineRow).workdir);
      }
    } catch {
      // no prior result on disk is the normal first-run state, not an error
    }
  }, [loadShots]);

  useEffect(() => {
    void loadExisting();
    void window.electron.benchmarkIdentity?.().then(
      (id) => setHandle(id?.handle ?? null),
      () => setHandle(null)
    );
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
  }, [loadExisting]);

  // Stream the harness's lifecycle from main: workdir on start, stdout/stderr lines, terminal row.
  useEffect(() => {
    const onStarted = (_e: unknown, payload: unknown) => {
      const p = payload as { workdir?: string; startedAt?: string; sampling?: unknown };
      if (p?.workdir) {
        setActiveWorkdir(p.workdir);
        setRunStartedAt(p.startedAt ? Date.parse(p.startedAt) : Date.now());
        setLaunchedSampling(sanitizeSampling(p.sampling));
        setScored(false);
        setLastLine(null);
        setShots([]);
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
        // A fresh run's engine truth replaces any stale edit — the prefill is the honest default.
        setModel(p.row.modelId ?? '');
        setStatus('Run complete.');
        void loadShots(p.row.workdir);
      } else if (p?.error) {
        setStatus(`The run failed: ${p.error}`);
      }
    };
    window.electron.on('benchmark-started', onStarted);
    window.electron.on('benchmark-log', onLog);
    window.electron.on('benchmark-finished', onFinished);
    return () => {
      window.electron.off('benchmark-started', onStarted);
      window.electron.off('benchmark-log', onLog);
      window.electron.off('benchmark-finished', onFinished);
    };
  }, [loadShots]);

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
        }
      });
    }, 1000);
    return () => clearInterval(iv);
  }, [running, activeWorkdir, loadExisting]);

  const phase: BenchPhase = useMemo(() => {
    if (scored) return 'done';
    if (swarm.present && swarm.finished) return 'score';
    if (swarm.present) return 'build';
    return 'boot';
  }, [scored, swarm.present, swarm.finished]);

  // The board follows the SELECTED tier; a stored result whose scorer matches a different tier
  // flips the view to that tier's board (so finishing an sb-6 run shows the sb-6 ladder).
  const comparable = mine != null && mine.scorerVersion === TIER_SCORER[tier];
  const rows = useMemo<BenchmarkRow[]>(
    () =>
      (comparable && mine ? [...BASELINES_BY_TIER[tier], mine] : BASELINES_BY_TIER[tier])
        .slice()
        .sort((a: BenchmarkRow, b: BenchmarkRow) => b.score - a.score),
    [comparable, mine, tier]
  );
  useEffect(() => {
    const mineScorer = mine?.scorerVersion;
    const owningTier = TIERS.find((t) => TIER_SCORER[t] === mineScorer);
    if (owningTier) setTier(owningTier);
  }, [mine?.scorerVersion]);

  const run = useCallback(async () => {
    setRunning(true);
    setScored(false);
    setStatus(null);
    setLaunchedSampling(sampling);
    try {
      const result = await window.electron.benchmarkRun?.(nodes, tier, sampling);
      if (result) {
        setMine(result as MineRow);
        setModel((result as MineRow).modelId ?? '');
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
    }
  }, [nodes, tier, sampling, loadShots]);

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

  const publish = useCallback(async () => {
    if (!mine) return;
    setPublishing(true);
    setStatus('Publishing to leanzero.net…');
    try {
      const res = await window.electron.benchmarkPublish?.({
        title: title.trim() || undefined,
        model: model.trim(),
      });
      setStatus(
        res?.ok
          ? 'Published for review. It appears once a human approves it.'
          : `Publish failed: ${res?.error ?? 'unknown error'}`
      );
    } catch (err) {
      setStatus(`Publish failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setPublishing(false);
    }
  }, [mine, title, model]);

  const publishable = mine != null && mine.runMeta != null;
  const modelProblem = modelFieldProblem(model);
  const modelValid = modelProblem === null;
  const mineFinished = fmtWhen(mine?.runMeta?.finishedAt);
  const uid = useId();
  const lockedId = `${uid}-locked`;
  const modelId = `${uid}-model`;
  const modelHintId = `${uid}-model-hint`;
  const titleId = `${uid}-title`;
  const lockedWhy = 'Locked while a run is live — the tier and node count are fixed at launch';

  return (
    <MainPanelLayout>
      <div className={cx('flex-1 overflow-y-auto', SURFACE.page)}>
        <div className={cx('mx-auto flex w-full max-w-5xl flex-col', SPACE.page, SPACE.section)}>
          <PageHeader
            title="Benchmark"
            subtitle="Your fleet against frontier models on the same frozen build task, graded by running what it produces — not by asking a model what it thinks."
            actions={
              <>
                <div role="group" aria-label="Benchmark tier" className={STRIP}>
                  {TIERS.map((t) => (
                    <button
                      key={t}
                      type="button"
                      onClick={() => setTier(t)}
                      disabled={running}
                      aria-pressed={tier === t}
                      aria-describedby={running ? lockedId : undefined}
                      title={`${TIER_BLURB[t]}${running ? `. ${lockedWhy}` : ''}`}
                      className={segmentClass(tier === t)}
                    >
                      {TIER_SEGMENT_LABEL[t]}
                    </button>
                  ))}
                </div>
                {running ? (
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
                )}
              </>
            }
          />

          {/* Run setup — the fleet size and the sampling knobs the next run will use, editable until
              launch; while a run is live they freeze on the values that run launched with. EVERY
              unset knob — temperature included — falls through to the config/model default: the 0.2
              benchmark pin was deleted in main.ts ("NO HARDCODED TEMPERATURE" — it overrode the
              per-model value Mihai sets in LM Studio), and a card still saying "0.2 (pinned)" claimed
              a pin the run no longer sends (caught live on r4-relaunch, 2026-08-30). */}
          <section aria-label="Run setup" className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-3">
              <span className={TYPE.meta}>Nodes</span>
              <div role="group" aria-label="Nodes" className={STRIP}>
                {NODE_CHOICES.map((n) => (
                  <button
                    key={n}
                    type="button"
                    onClick={() => setNodes(n)}
                    disabled={running}
                    aria-pressed={nodes === n}
                    aria-describedby={running ? lockedId : undefined}
                    title={running ? lockedWhy : `Run on ${n} node${n === 1 ? '' : 's'}`}
                    className={cx(segmentClass(nodes === n), TNUM)}
                  >
                    {n}
                  </button>
                ))}
              </div>
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

          {mine && !comparable && (
            <ToneBand tone="warn">
              {/* NAME THE SAME CONSTANT THE PREDICATE USES. `comparable` compares against
                  TIER_SCORER[tier], but this printed COMPARABLE_SCORER — so the banner read "scored by
                  sb-5.3, but the board runs on sb-5.3", telling the operator two identical versions were
                  incompatible. */}
              Your last result was scored by {mine.scorerVersion}, but this board runs on{' '}
              {TIER_SCORER[tier]} — the numbers are not comparable, so your row sits out. Run the
              benchmark again to enter the board.
            </ToneBand>
          )}

          {mine && !running && (
            <Panel
              title="Your last run"
              headerRight={
                mineFinished ? (
                  <span className={cx(TYPE.meta, TNUM)}>completed {mineFinished}</span>
                ) : undefined
              }
            >
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <StatCell label="Score" value={`${(mine.score * 100).toFixed(1)}%`} tone="accent" />
                {typeof mine.wallSecs === 'number' && (
                  <StatCell label="Wall time" value={fmtElapsed(mine.wallSecs * 1000)} />
                )}
                {mine.runMeta && (
                  <StatCell label="Repair rounds" value={String(mine.runMeta.repairRounds)} />
                )}
                {mine.runMeta && (
                  <StatCell
                    label="Engine events"
                    value={mine.runMeta.engineEvents.toLocaleString()}
                  />
                )}
              </div>
            </Panel>
          )}

          {shots.length > 0 && !running && (
            <Panel
              title="What it built"
              count={shots.length}
              headerRight={<span className={TYPE.meta}>before and after repairs</span>}
            >
              <ShotsStrip shots={shots} />
            </Panel>
          )}

          {running && mine && comparable && (
            // PREVIOUS RESULT — a NEW run is in progress, so the "Your fleet" row below is the
            // PREVIOUS run's stored result. The warn chip is the run-in-progress mark — never
            // ambiguous against the live run.
            <Panel
              title="Previous result"
              headerRight={
                <Chip tone="warn">
                  {(mine.score * 100).toFixed(1)}%{mineFinished ? ` · ${mineFinished}` : ''}
                </Chip>
              }
            >
              <p className={TYPE.body}>
                The &ldquo;Your fleet&rdquo; rows below are your last completed run — the run in
                progress replaces them when it finishes.
              </p>
            </Panel>
          )}

          <Panel
            title="Board"
            count={rows.length}
            headerRight={<span className={cx(TYPE.meta, TNUM)}>scorer {TIER_SCORER[tier]}</span>}
            padded={false}
          >
            <DataTable
              aria-label="Benchmark board"
              columns={boardColumns(mine)}
              rows={rows}
              rowKey={(r) => (r.mine ? 'mine' : r.label)}
              empty={
                <EmptyState
                  title="No entrants"
                  body="No baseline is published for this tier yet."
                />
              }
            />
          </Panel>

          <Panel title="Overall">
            <ScoreBars rows={rows} />
          </Panel>

          <Panel title="Where the points went">
            <p className={cx('mb-3 max-w-[70ch]', TYPE.bodyMuted)}>
              {TIER_LABELS.A} · {TIER_LABELS.B} · {TIER_LABELS.C} · {TIER_LABELS.D}. A build can be
              perfectly structured and still score nothing on behaviour — the split is the diagnosis.
            </p>
            <TierBreakdown rows={rows} />
          </Panel>

          {mine && !running && (
            <Panel title="How this score was built">
              {mine.verdict ? (
                <>
                  <p className={cx('mb-4 max-w-[80ch]', TYPE.bodyMuted)}>
                    Every number below is scorer evidence from YOUR run — the exact checks it ran,
                    what each one saw, and what the misses cost. The formula:{' '}
                    <span className={cx(WEIGHT.semibold, 'text-lz-ink')}>
                      60% core build + 15% journey + 10% visual + 5% performance + 10% hard block
                    </span>
                    .
                  </p>
                  <ScoringDetail verdict={mine.verdict} score={mine.score} />
                </>
              ) : (
                <p className={TYPE.bodyMuted}>
                  This stored result predates the detailed verdict — the full check-by-check
                  breakdown appears from your next run.
                </p>
              )}
            </Panel>
          )}

          {mine && (
            <Panel
              title="Publish to leanzero.net"
              headerRight={
                handle ? (
                  <Chip
                    icon={<BadgeCheck />}
                    title="Your public pseudonym on leanzero.net — stable for this install"
                  >
                    publishing as {handle}
                  </Chip>
                ) : undefined
              }
            >
              <p className={cx('max-w-[70ch]', TYPE.bodyMuted)}>
                Posts your score, the full check-by-check breakdown and the before/after
                screenshots as{' '}
                <span className={cx(WEIGHT.semibold, 'text-lz-ink')}>
                  {handle ?? 'your handle'}
                </span>
                . The result appears on the leanzero.net board immediately.
              </p>
              <div className="mt-4 flex flex-col gap-4">
                <div className="max-w-[560px]">
                  <label htmlFor={modelId} className={cx('mb-1.5 block', TYPE.meta)}>
                    Model <span className={TONE_TEXT.err}>*</span>
                  </label>
                  <input
                    id={modelId}
                    type="text"
                    value={model}
                    onChange={(e) => setModel(e.target.value.slice(0, MODEL_MAX_CHARS))}
                    maxLength={MODEL_MAX_CHARS}
                    placeholder="The exact model your fleet ran — e.g. qwen3.6-27b-…-mtp"
                    disabled={publishing}
                    title={publishing ? 'Locked while publishing' : undefined}
                    aria-invalid={!modelValid}
                    aria-describedby={modelHintId}
                    className={INPUT}
                  />
                  <p id={modelHintId} className={cx('mt-1.5', TYPE.meta)}>
                    Prefilled from the run&apos;s own pool — edit it if that is not the exact model.
                    {modelProblem && (
                      <span className={cx('ml-1', WEIGHT.medium, TONE_TEXT.err)}>
                        {modelProblem}
                      </span>
                    )}
                  </p>
                </div>
                <div className="flex flex-wrap items-end gap-3">
                  <div className="w-full max-w-[360px]">
                    <label htmlFor={titleId} className={cx('mb-1.5 block', TYPE.meta)}>
                      Title <span className="text-lz-ink-4">(optional)</span>
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
                      className={INPUT}
                    />
                  </div>
                  <Button
                    variant="primary"
                    onClick={publish}
                    disabled={!publishable || running || publishing || !modelValid}
                    title={
                      !publishable
                        ? 'Run the benchmark (v2) first'
                        : running
                          ? 'Publishing waits for the run in progress to finish'
                          : modelProblem
                            ? `Model: ${modelProblem}`
                            : 'Publish this result to leanzero.net'
                    }
                    icon={publishing ? <Loader2 className="animate-spin" /> : <Upload />}
                  >
                    Publish
                  </Button>
                </div>
              </div>
              {!publishable && (
                <p className={cx('mt-3', TYPE.meta)}>
                  This result predates the v2 publisher — run the benchmark again to publish.
                </p>
              )}
            </Panel>
          )}

          <footer className={cx('border-t pt-4 text-lz-body text-lz-ink-2', SURFACE.hairline)}>
            Baselines were captured on our own fleet against this exact frozen spec ({COMPARABLE_SCORER})
            and ship with the app, so your run costs you nothing and every board is comparable.
            Scores below 100 are expected: the finesse tier is graded against a theoretical optimum,
            and a perfect score would mean the task had stopped measuring.
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
    </MainPanelLayout>
  );
}

export type { BenchmarkRow, Tier };
