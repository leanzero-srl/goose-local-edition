import { useCallback, useEffect, useMemo, useState } from 'react';
import { Gauge, Play, Upload, Loader2, XCircle, BadgeCheck } from 'lucide-react';
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
import { ZoneHeader, ZONE_HUES } from '../swarm/ZoneHeader';
import { Input } from '../ui/input';
import { ConfirmationModal } from '../ui/ConfirmationModal';

const NODE_CHOICES = [1, 2, 3] as const;
type NodeChoice = (typeof NODE_CHOICES)[number];

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

  // Live engine truth for the active run — drives the phase strip (present/finished), while the
  // full SwarmRunPanel below renders the same dir with its own poller.
  const swarm = useSwarmRun(activeWorkdir ?? undefined);

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
        setStatus(null);
      }
    });
  }, [loadExisting]);

  // Stream the harness's lifecycle from main: workdir on start, stdout/stderr lines, terminal row.
  useEffect(() => {
    const onStarted = (_e: unknown, payload: unknown) => {
      const p = payload as { workdir?: string; startedAt?: string };
      if (p?.workdir) {
        setActiveWorkdir(p.workdir);
        setRunStartedAt(p.startedAt ? Date.parse(p.startedAt) : Date.now());
        setScored(false);
        setLastLine(null);
        setShots([]);
      }
    };
    const onLog = (_e: unknown, payload: unknown) => {
      const p = payload as { line?: string };
      if (typeof p?.line === 'string') {
        setLastLine(p.line);
        if (/rep0 \(/.test(p.line)) setScored(true);
      }
    };
    const onFinished = (_e: unknown, payload: unknown) => {
      const p = payload as { row?: MineRow; error?: string; cancelled?: boolean };
      setRunning(false);
      setCancelling(false);
      setActiveWorkdir(null);
      setRunStartedAt(null);
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
    try {
      const result = await window.electron.benchmarkRun?.(nodes, tier);
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
    }
  }, [nodes, loadShots]);

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
  const modelValid = model.trim().length >= 8 && model.trim().length <= 120;
  const mineFinished = fmtWhen(mine?.runMeta?.finishedAt);

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
            {handle && (
              <span
                className="ml-auto flex items-center gap-1.5 rounded px-2.5 py-1 text-xs font-bold text-white"
                style={{ backgroundColor: 'var(--color-node-4)' }}
                title="Your public pseudonym on leanzero.net — stable for this install"
              >
                <BadgeCheck className="h-3.5 w-3.5" />
                publishing as {handle}
              </span>
            )}
          </header>

          <section className="mt-6 flex flex-wrap items-center gap-3">
            <span className="text-sm text-text-secondary">Benchmark</span>
            <div className="flex overflow-hidden rounded border border-border-primary">
              {TIERS.map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setTier(t)}
                  disabled={running}
                  aria-pressed={tier === t}
                  title={
                    t === 'sb-6'
                      ? 'VendorSync Pro — the hard tier: raw-WebGL 3D, webhooks, optimistic concurrency (scorer sb-6.0)'
                      : 'VendorSync — the standard tier (scorer sb-5.3)'
                  }
                  className={`px-4 py-2 text-sm font-bold transition-colors ${
                    tier === t
                      ? 'bg-[var(--color-node-5)] text-white'
                      : 'bg-background-secondary text-text-secondary hover:text-text-primary'
                  }`}
                >
                  {t === 'sb-6' ? 'sb-6 · HARD' : 'sb-5.3'}
                </button>
              ))}
            </div>
            <span className="text-sm text-text-secondary">Nodes</span>
            <div className="flex overflow-hidden rounded border border-border-primary">
              {NODE_CHOICES.map((n) => (
                <button
                  key={n}
                  type="button"
                  onClick={() => setNodes(n)}
                  disabled={running}
                  aria-pressed={nodes === n}
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

            {running ? (
              <button
                type="button"
                onClick={() => setConfirmCancel(true)}
                disabled={cancelling}
                className="ml-auto flex items-center gap-2 rounded bg-background-danger px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
              >
                {cancelling ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <XCircle className="h-4 w-4" />
                )}
                {cancelling ? 'Cancelling…' : 'Cancel run'}
              </button>
            ) : (
              <button
                type="button"
                onClick={run}
                className="ml-auto flex items-center gap-2 rounded bg-[var(--color-block-teal)] px-4 py-2 text-sm font-semibold text-white"
              >
                <Play className="h-4 w-4" />
                Run benchmark
              </button>
            )}
          </section>

          {running && (
            <section className="mt-6 flex flex-col gap-4">
              <PhaseStrip
                phase={phase}
                lastLine={lastLine}
                elapsedMs={runStartedAt ? now - runStartedAt : 0}
              />
              <SwarmRunPanel workingDir={activeWorkdir ?? undefined} />
            </section>
          )}

          {status && (
            <p className="mt-4 rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
              {status}
            </p>
          )}

          {mine && !comparable && (
            <p className="mt-4 rounded border-2 border-[var(--color-node-5)] px-4 py-3 text-sm font-semibold text-text-primary">
              Your last result was scored by {mine.scorerVersion}, but the board runs on{' '}
              {COMPARABLE_SCORER} — the numbers are not comparable, so your row sits out. Run the
              benchmark again to enter the board.
            </p>
          )}

          {mine && !running && (
            <section className="mt-8">
              <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
                Your last run{mineFinished ? ` · completed ${mineFinished}` : ''}
              </h2>
              <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-4">
                <StatTile
                  label="Score"
                  value={`${(mine.score * 100).toFixed(1)}%`}
                  color="var(--color-block-teal)"
                />
                {typeof mine.wallSecs === 'number' && (
                  <StatTile
                    label="Wall time"
                    value={fmtElapsed(mine.wallSecs * 1000)}
                    color="var(--color-node-2)"
                  />
                )}
                {mine.runMeta && (
                  <StatTile
                    label="Repair rounds"
                    value={String(mine.runMeta.repairRounds)}
                    color="var(--color-node-4)"
                  />
                )}
                {mine.runMeta && (
                  <StatTile
                    label="Engine events"
                    value={mine.runMeta.engineEvents.toLocaleString()}
                    color="var(--color-node-1)"
                  />
                )}
              </div>
            </section>
          )}

          {shots.length > 0 && !running && (
            <section className="mt-8">
              <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
                What it built — before and after repairs
              </h2>
              <div className="mt-3">
                <ShotsStrip shots={shots} />
              </div>
            </section>
          )}

          {running && mine && comparable && (
            // PREVIOUS RESULT zone — a NEW run is in progress, so the "Your fleet" row below is the
            // PREVIOUS run's stored result. Same zone-header register as the swarm panel, solid amber
            // (the run-in-progress color), full border, no washes — never ambiguous against the live run.
            <div className="mt-8 rounded border-2" style={{ borderColor: PHASE_ACTIVE }}>
              <ZoneHeader
                hue={PHASE_ACTIVE}
                label="Previous result"
                explain="your last completed run — replaced when this run finishes"
                className="pt-2"
                right={
                  <span className="text-xs font-bold tabular-nums" style={{ color: PHASE_ACTIVE }}>
                    {(mine.score * 100).toFixed(1)}%{mineFinished ? ` · ${mineFinished}` : ''}
                  </span>
                }
              />
              <p className="px-3 pb-3 pt-1 text-sm text-text-primary">
                The &ldquo;Your fleet&rdquo; rows below are your last completed run — the run in
                progress replaces them when it finishes.
              </p>
            </div>
          )}

          <section className="mt-8">
            <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
              Overall
            </h2>
            <ScoreBars rows={rows} />
          </section>

          <section className="mt-10">
            <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
              Where the points went
            </h2>
            <p className="mb-3 mt-1 max-w-[70ch] text-sm text-text-secondary">
              {TIER_LABELS.A} · {TIER_LABELS.B} · {TIER_LABELS.C} · {TIER_LABELS.D}. A build can be
              perfectly structured and still score nothing on behaviour — the split is the diagnosis.
            </p>
            <TierBreakdown rows={rows} />
          </section>

          {mine && !running && (
            <section className="mt-10">
              <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
                How this score was built
              </h2>
              {mine.verdict ? (
                <>
                  <p className="mb-4 mt-1 max-w-[80ch] text-sm text-text-secondary">
                    Every number below is scorer evidence from YOUR run — the exact checks it ran,
                    what each one saw, and what the misses cost. The formula:{' '}
                    <span className="font-bold text-text-primary">
                      60% core build + 15% journey + 10% visual + 5% performance + 10% hard block
                    </span>
                    .
                  </p>
                  <ScoringDetail verdict={mine.verdict} score={mine.score} />
                </>
              ) : (
                <p className="mt-3 rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
                  This stored result predates the detailed verdict — the full check-by-check
                  breakdown appears from your next run.
                </p>
              )}
            </section>
          )}

          {mine && (
            <section className="mt-10 rounded border border-border-primary p-4">
              <h2 className="text-xs font-bold uppercase tracking-widest text-text-secondary">
                Publish to leanzero.net
              </h2>
              <p className="mt-1 max-w-[70ch] text-sm text-text-secondary">
                Posts your score, the full check-by-check breakdown and the before/after
                screenshots as{' '}
                <span className="font-bold text-text-primary">{handle ?? 'your handle'}</span>.
                The result appears on the leanzero.net board immediately.
              </p>
              <div className="mt-3 flex flex-col gap-3">
                <div className="max-w-[560px]">
                  <label className="mb-1 block text-xs font-bold uppercase tracking-wider text-text-secondary">
                    Model <span style={{ color: '#e5484d' }}>*</span>
                  </label>
                  <Input
                    value={model}
                    onChange={(e) => setModel(e.target.value.slice(0, 120))}
                    maxLength={120}
                    placeholder="The exact model your fleet ran — e.g. qwen3.6-27b-…-mtp"
                    disabled={publishing}
                    aria-invalid={!modelValid}
                  />
                  <p className="mt-1 text-[11px] text-text-secondary">
                    Prefilled from the run's own pool — edit it if that is not the exact model.
                    {!modelValid && (
                      <span className="ml-1 font-bold" style={{ color: '#e5484d' }}>
                        Required, 8–120 characters.
                      </span>
                    )}
                  </p>
                </div>
                <div className="flex flex-wrap items-center gap-3">
                  <Input
                    value={title}
                    onChange={(e) => setTitle(e.target.value.slice(0, 80))}
                    maxLength={80}
                    placeholder="Optional title — e.g. My M4 fleet first run"
                    className="max-w-[360px]"
                    disabled={publishing}
                  />
                  <button
                    type="button"
                    onClick={publish}
                    disabled={!publishable || running || publishing || !modelValid}
                    title={
                      !publishable
                        ? 'Run the benchmark (v2) first'
                        : !modelValid
                          ? 'Set the Model field first (8–120 characters)'
                          : 'Publish this result to leanzero.net'
                    }
                    className="flex items-center gap-2 rounded bg-[var(--color-block-teal)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-40"
                  >
                    {publishing ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Upload className="h-4 w-4" />
                    )}
                    Publish
                  </button>
                </div>
              </div>
              {!publishable && (
                <p className="mt-2 text-xs text-text-secondary">
                  This result predates the v2 publisher — run the benchmark again to publish.
                </p>
              )}
            </section>
          )}

          <footer className="mt-10 border-t border-border-primary pt-4 text-xs text-text-secondary">
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
