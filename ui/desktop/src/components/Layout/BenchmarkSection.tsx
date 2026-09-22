import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { ChevronDown, ChevronRight, Gauge, Plus, Trash2 } from 'lucide-react';
import { toast } from 'react-toastify';
import { Button, Chip, SectionHeader, TNUM, TYPE, WEIGHT, cx } from '../lz';
import type { BenchSession, CatalogBenchmark } from '../benchmark/bridge';
import { fmtWhen, OutcomeChip } from '../benchmark/outcome';
import {
  TreeChildren,
  TreeContextMenu,
  treeParentClass,
  treeRowClass,
  treeStateRowClass,
  TREE_PREVIEW_COUNT,
} from './tree';
import { defineMessages, useIntl } from '../../i18n';
import { useStartChatAbout } from './useStartChatAbout';

const i18n = defineMessages({
  title: { id: 'benchmarkSection.title', defaultMessage: 'Benchmark' },
  newRun: { id: 'benchmarkSection.newRun', defaultMessage: 'New benchmark run' },
  empty: {
    id: 'benchmarkSection.empty',
    defaultMessage: 'No benchmark runs on this machine yet. Start one from the Benchmark view.',
  },
  noRuns: { id: 'benchmarkSection.noRuns', defaultMessage: 'No runs yet' },
  showMore: { id: 'benchmarkSection.showMore', defaultMessage: 'Show more' },
  showLess: { id: 'benchmarkSection.showLess', defaultMessage: 'Show less' },
  current: { id: 'benchmarkSection.current', defaultMessage: 'CURRENT' },
  frozen: { id: 'benchmarkSection.frozen', defaultMessage: 'FROZEN' },
  openRun: { id: 'benchmarkSection.openRun', defaultMessage: 'Open run' },
  askAbout: {
    id: 'benchmarkSection.askAbout',
    defaultMessage: 'Start an AI session about this run',
  },
  deleteRun: { id: 'benchmarkSection.deleteRun', defaultMessage: 'Delete run' },
  confirmDelete: {
    id: 'benchmarkSection.confirmDelete',
    defaultMessage: 'Confirm delete (removes its files)',
  },
  cannotDeleteRunning: {
    id: 'benchmarkSection.cannotDeleteRunning',
    defaultMessage: 'A running session cannot be deleted — cancel the run first',
  },
  cannotDeleteNoId: {
    id: 'benchmarkSection.cannotDeleteNoId',
    defaultMessage: 'This session has no run id yet — it appears moments after launch',
  },
  deleteFailed: { id: 'benchmarkSection.deleteFailed', defaultMessage: 'Could not delete the run' },
});

export const BENCHMARK_PATH = '/benchmark';

/** Stable identity for a run row: the engine's runId, or the just-launched row's start stamp. */
export const benchRunKey = (s: BenchSession): string => s.runId ?? `start-${s.startedAt}`;

/** The run's URL: `/benchmark?era=<scorerVersion>&run=<key>` — the sidebar's link and the view's
 *  selection. `?new=1` opens the view on its run setup. */
export function benchRunHref(era: string, key?: string): string {
  const params = new URLSearchParams({ era });
  if (key) params.set('run', key);
  return `${BENCHMARK_PATH}?${params.toString()}`;
}

export interface BenchEra {
  scorerVersion: string;
  title: string;
  current: boolean;
  frozen: boolean;
  fromCatalog: boolean;
  sessions: BenchSession[];
}

const startMs = (s: BenchSession): number => {
  const t = Date.parse(s.startedAt);
  return Number.isNaN(t) ? 0 : t;
};

/** One era per benchmark the catalog names plus every era the runs on this machine reference,
 *  the current one first, runs newest first — the same fold the Benchmark view uses. */
export function deriveEras(
  sessions: readonly BenchSession[],
  catalog: readonly CatalogBenchmark[] | null
): BenchEra[] {
  const map = new Map<string, BenchEra>();
  for (const b of catalog ?? []) {
    map.set(b.scorerVersion, {
      scorerVersion: b.scorerVersion,
      title: b.title,
      current: b.current,
      frozen: b.frozen,
      fromCatalog: true,
      sessions: [],
    });
  }
  for (const s of sessions) {
    if (!map.has(s.scorerVersion)) {
      map.set(s.scorerVersion, {
        scorerVersion: s.scorerVersion,
        title: s.scorerVersion,
        current: false,
        frozen: false,
        fromCatalog: false,
        sessions: [],
      });
    }
    map.get(s.scorerVersion)!.sessions.push(s);
  }
  const list = [...map.values()];
  for (const era of list) era.sessions.sort((a, b) => startMs(b) - startMs(a));
  list.sort(
    (a, b) =>
      Number(b.current) - Number(a.current) ||
      b.scorerVersion.localeCompare(a.scorerVersion, undefined, { numeric: true })
  );
  return list;
}

export function askAboutRunPrompt(era: BenchEra, run: BenchSession): string {
  const score = run.score != null ? `${(run.score * 100).toFixed(1)}%` : 'no score';
  return `I want to look at my benchmark run ${run.runId ?? run.startedAt} on ${era.scorerVersion} (${era.title}), started ${run.startedAt}, outcome ${run.outcome}, ${score}. Its files live under the app's benchmark/runs folder. Help me understand what happened and what to try next.`;
}

const RunLeafRow: React.FC<{
  era: BenchEra;
  run: BenchSession;
  active: boolean;
  onOpen: () => void;
  onAsk: () => void;
  onDelete: () => void;
}> = ({ era, run, active, onOpen, onAsk, onDelete }) => {
  const intl = useIntl();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const when = fmtWhen(run.startedAt) ?? run.startedAt;
  const undeletable =
    run.outcome === 'running'
      ? intl.formatMessage(i18n.cannotDeleteRunning)
      : run.runId == null
        ? intl.formatMessage(i18n.cannotDeleteNoId)
        : null;
  return (
    <div
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <button
        onClick={onOpen}
        title={`${era.scorerVersion} · ${when}`}
        aria-current={active ? 'true' : undefined}
        data-testid={`bench-run-${benchRunKey(run)}`}
        className={cx(
          treeRowClass,
          active ? 'ring-2 ring-inset ring-lz-accent' : 'hover:bg-lz-surface-2'
        )}
      >
        <span className={cx('shrink-0 text-lz-body text-lz-ink', TNUM)}>{when}</span>
        <span className="ml-auto shrink-0">
          <OutcomeChip session={run} />
        </span>
      </button>
      {menu && (
        <TreeContextMenu
          x={menu.x}
          y={menu.y}
          testId="bench-run-context-menu"
          onClose={() => setMenu(null)}
          items={[
            {
              key: 'open',
              label: intl.formatMessage(i18n.openRun),
              icon: <Gauge />,
              onClick: () => {
                setMenu(null);
                onOpen();
              },
            },
            {
              key: 'ask',
              label: intl.formatMessage(i18n.askAbout),
              icon: <Plus />,
              onClick: () => {
                setMenu(null);
                onAsk();
              },
            },
            {
              key: 'delete',
              label: intl.formatMessage(i18n.deleteRun),
              icon: <Trash2 />,
              danger: true,
              separator: true,
              disabled: undeletable != null,
              title: undeletable ?? undefined,
              confirmLabel: intl.formatMessage(i18n.confirmDelete),
              onClick: () => {
                setMenu(null);
                onDelete();
              },
            },
          ]}
        />
      )}
    </div>
  );
};

/**
 * Benchmark in the sidebar, the same shape as Projects: one row per benchmark era (the SB tier),
 * its runs nested under it newest first, the open run marked. Opening a run navigates the
 * Benchmark view by URL (`?era=…&run=…`); the view no longer lists runs itself.
 */
export const BenchmarkSection: React.FC<{ className?: string }> = ({ className }) => {
  const startChat = useStartChatAbout();
  const intl = useIntl();
  const navigate = useNavigate();
  const location = useLocation();
  const [params] = useSearchParams();
  const [sessions, setSessions] = useState<BenchSession[]>([]);
  const [catalog, setCatalog] = useState<CatalogBenchmark[] | null>(null);
  const [loaded, setLoaded] = useState(false);
  // Eras the user flipped; the default is open for the current era and the one holding the open run.
  const [toggled, setToggled] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState<ReadonlySet<string>>(new Set());

  const onBenchmark = location.pathname === BENCHMARK_PATH;
  const activeRun = onBenchmark ? params.get('run') : null;

  const refresh = useCallback(async () => {
    try {
      const r = await window.electron.benchmarkSessions?.();
      if (r && Array.isArray(r.sessions)) setSessions(r.sessions);
    } catch (error) {
      console.error('Failed to list benchmark runs:', error);
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    window.electron
      .benchmarkCatalog?.()
      .then((r) => {
        if (r && Array.isArray(r.benchmarks)) setCatalog(r.benchmarks);
      })
      .catch(() => {
        // the eras then come from the runs alone; the view says when the catalog is unreachable
      });
    // The sidebar follows the run list at a slow cadence; the view's own poller is the live one.
    const id = setInterval(() => void refresh(), 15_000);
    return () => clearInterval(id);
  }, [refresh]);

  const eras = useMemo(() => deriveEras(sessions, catalog), [sessions, catalog]);

  const remove = useCallback(
    async (run: BenchSession) => {
      if (!run.runId) return;
      try {
        const r = await window.electron.benchmarkDeleteSession(run.runId);
        if (!r.ok) throw new Error(r.error ?? 'refused');
        await refresh();
      } catch (error) {
        console.error('Failed to delete the benchmark run:', error);
        toast.error(intl.formatMessage(i18n.deleteFailed));
      }
    },
    [refresh, intl]
  );

  const runsCount = sessions.length;

  return (
    <div className={cx('flex min-h-0 flex-col', className)} data-testid="benchmark-section">
      <SectionHeader
        title={intl.formatMessage(i18n.title)}
        count={runsCount}
        className="px-4"
        right={
          <Button
            variant="ghost"
            size="sm"
            icon={<Plus className="text-lz-accent" strokeWidth={2.5} />}
            onClick={() => navigate(`${BENCHMARK_PATH}?new=1`)}
            aria-label={intl.formatMessage(i18n.newRun)}
            title={intl.formatMessage(i18n.newRun)}
          />
        }
      />
      <div className="flex flex-col gap-px px-2 pb-2">
        {loaded && eras.length === 0 ? (
          <div className={cx('px-2 py-2', TYPE.bodyMuted)}>{intl.formatMessage(i18n.empty)}</div>
        ) : (
          eras.map((era) => {
            const defaultOpen =
              era.current ||
              (activeRun != null && era.sessions.some((r) => benchRunKey(r) === activeRun));
            const expanded = toggled.has(era.scorerVersion) ? !defaultOpen : defaultOpen;
            const all = showAll.has(era.scorerVersion);
            const shown = all ? era.sessions : era.sessions.slice(0, TREE_PREVIEW_COUNT);
            return (
              <div key={era.scorerVersion} data-testid={`bench-era-${era.scorerVersion}`}>
                <button
                  onClick={() =>
                    setToggled((prev) => {
                      const next = new Set(prev);
                      if (next.has(era.scorerVersion)) next.delete(era.scorerVersion);
                      else next.add(era.scorerVersion);
                      return next;
                    })
                  }
                  aria-expanded={expanded}
                  title={era.title}
                  className={treeParentClass}
                >
                  {expanded ? (
                    <ChevronDown className="size-3.5 shrink-0 text-lz-ink-3" />
                  ) : (
                    <ChevronRight className="size-3.5 shrink-0 text-lz-ink-3" />
                  )}
                  <Gauge className="size-4 shrink-0 text-lz-ink-2" />
                  <span className={cx('truncate text-lz-body text-lz-ink', WEIGHT.medium)}>
                    {era.scorerVersion}
                  </span>
                  {era.current ? (
                    <Chip tone="ok">{intl.formatMessage(i18n.current)}</Chip>
                  ) : era.frozen ? (
                    <Chip tone="warn">{intl.formatMessage(i18n.frozen)}</Chip>
                  ) : null}
                  <span className={cx('ml-auto', TYPE.meta, TNUM)}>{era.sessions.length}</span>
                </button>
                {expanded && (
                  <TreeChildren>
                    {shown.map((run) => (
                      <RunLeafRow
                        key={benchRunKey(run)}
                        era={era}
                        run={run}
                        active={activeRun === benchRunKey(run)}
                        onOpen={() => navigate(benchRunHref(era.scorerVersion, benchRunKey(run)))}
                        onAsk={() => void startChat(askAboutRunPrompt(era, run))}
                        onDelete={() => void remove(run)}
                      />
                    ))}
                    {era.sessions.length === 0 ? (
                      <div className={treeStateRowClass}>{intl.formatMessage(i18n.noRuns)}</div>
                    ) : era.sessions.length > shown.length ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="self-start"
                        onClick={() => setShowAll((prev) => new Set(prev).add(era.scorerVersion))}
                      >
                        {intl.formatMessage(i18n.showMore)}
                      </Button>
                    ) : all && era.sessions.length > TREE_PREVIEW_COUNT ? (
                      <Button
                        variant="ghost"
                        size="sm"
                        className="self-start"
                        onClick={() =>
                          setShowAll((prev) => {
                            const next = new Set(prev);
                            next.delete(era.scorerVersion);
                            return next;
                          })
                        }
                      >
                        {intl.formatMessage(i18n.showLess)}
                      </Button>
                    ) : null}
                  </TreeChildren>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
};
