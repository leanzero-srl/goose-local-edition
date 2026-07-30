import { useCallback, useEffect, useMemo, useState } from 'react';
import { Gauge, Play, Upload, Loader2 } from 'lucide-react';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { BASELINES, TIER_LABELS, type BenchmarkRow, type Tier } from './baselines';
import { ScoreBars } from './ScoreBars';
import { TierBreakdown } from './TierBreakdown';

const NODE_CHOICES = [1, 2, 3] as const;
type NodeChoice = (typeof NODE_CHOICES)[number];

/**
 * The benchmark is two buttons and a node choice. Everything else on the page is the result.
 *
 * Baselines are BAKED, never run by the user: they were captured on our fleet against the frozen
 * spec and ship as versioned data, so a user's run costs them nothing and every board is comparable.
 * Their own result is added to the same roster and marked as theirs.
 */
export default function BenchmarkView() {
  const [nodes, setNodes] = useState<NodeChoice>(3);
  const [running, setRunning] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [mine, setMine] = useState<BenchmarkRow | null>(null);

  const loadExisting = useCallback(async () => {
    try {
      const result = await window.electron.benchmarkRead?.();
      if (result) setMine(result as BenchmarkRow);
    } catch {
      // no prior result on disk is the normal first-run state, not an error
    }
  }, []);

  useEffect(() => {
    void loadExisting();
  }, [loadExisting]);

  const rows = useMemo<BenchmarkRow[]>(
    () => (mine ? [...BASELINES, mine] : BASELINES).slice().sort((a: BenchmarkRow, b: BenchmarkRow) => b.score - a.score),
    [mine]
  );

  const run = useCallback(async () => {
    setRunning(true);
    setStatus(`Running the frozen suite on ${nodes} node${nodes > 1 ? 's' : ''}. This takes a while.`);
    try {
      const result = await window.electron.benchmarkRun?.(nodes);
      if (result) {
        setMine(result as BenchmarkRow);
        setStatus('Run complete.');
      } else {
        setStatus('The run finished without producing a result. Check the log.');
      }
    } catch (err) {
      setStatus(`The run failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setRunning(false);
    }
  }, [nodes]);

  const publish = useCallback(async () => {
    if (!mine) return;
    setPublishing(true);
    setStatus('Publishing to leanzero.net…');
    try {
      const res = await window.electron.benchmarkPublish?.(mine);
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
  }, [mine]);

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

          <section className="mt-6 flex flex-wrap items-center gap-3">
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

            <button
              type="button"
              onClick={run}
              disabled={running}
              className="ml-auto flex items-center gap-2 rounded bg-[var(--color-block-teal)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
            >
              {running ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
              {running ? 'Running…' : 'Run benchmark'}
            </button>

            <button
              type="button"
              onClick={publish}
              disabled={!mine || running || publishing}
              title={mine ? 'Publish this result to leanzero.net' : 'Run the benchmark first'}
              className="flex items-center gap-2 rounded border border-border-primary px-4 py-2 text-sm font-semibold text-text-primary disabled:opacity-40"
            >
              {publishing ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Upload className="h-4 w-4" />
              )}
              Publish
            </button>
          </section>

          {status && (
            <p className="mt-4 rounded border border-border-primary bg-background-secondary px-4 py-3 text-sm text-text-primary">
              {status}
            </p>
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

          <footer className="mt-10 border-t border-border-primary pt-4 text-xs text-text-secondary">
            Baselines were captured on our own fleet against this exact frozen spec and ship with the
            app, so your run costs you nothing and every board is comparable. Scores below 100 are
            expected: the finesse tier is graded against a theoretical optimum, and a perfect score
            would mean the task had stopped measuring.
          </footer>
        </div>
      </div>
    </MainPanelLayout>
  );
}

export type { BenchmarkRow, Tier };
