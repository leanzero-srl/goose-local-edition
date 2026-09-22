import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { Bot, ChevronDown, ChevronRight, Copy, FolderOpen, Plus, X } from 'lucide-react';
import { toast } from 'react-toastify';
import { Button, Chip, SectionHeader, StatusDot, TNUM, TYPE, WEIGHT, cx, type Tone } from '../lz';
import {
  countdown,
  liveness,
  type AgentWorkRosterRow,
  type TickRecord,
} from '../agent-work/agentWorkModel';
import { useAgentRoster } from '../agent-work/useAgentWork';
import {
  TreeChildren,
  TreeContextMenu,
  timeAgo,
  treeParentClass,
  treeRowClass,
  treeStateRowClass,
  TREE_PREVIEW_COUNT,
} from './tree';
import { defineMessages, useIntl } from '../../i18n';
import { useStartChatAbout } from './useStartChatAbout';

const i18n = defineMessages({
  title: { id: 'agentWorkSection.title', defaultMessage: 'Agent Work' },
  newAgent: { id: 'agentWorkSection.newAgent', defaultMessage: 'New agent' },
  empty: {
    id: 'agentWorkSection.empty',
    defaultMessage: 'No agents yet. Create one, or add a folder that already has an agent.yaml.',
  },
  noTicks: { id: 'agentWorkSection.noTicks', defaultMessage: 'No ticks yet' },
  loadingTicks: { id: 'agentWorkSection.loadingTicks', defaultMessage: 'Reading the desk…' },
  ticksFailed: { id: 'agentWorkSection.ticksFailed', defaultMessage: "Couldn't read the desk" },
  retry: { id: 'agentWorkSection.retry', defaultMessage: 'Retry' },
  showMore: { id: 'agentWorkSection.showMore', defaultMessage: 'Show more' },
  showLess: { id: 'agentWorkSection.showLess', defaultMessage: 'Show less' },
  tick: { id: 'agentWorkSection.tick', defaultMessage: 'Tick {n}' },
  noManifest: { id: 'agentWorkSection.noManifest', defaultMessage: 'no agent.yaml' },
  openDesk: { id: 'agentWorkSection.openDesk', defaultMessage: 'Open desk' },
  revealInFinder: { id: 'agentWorkSection.revealInFinder', defaultMessage: 'Reveal in Finder' },
  copyPath: { id: 'agentWorkSection.copyPath', defaultMessage: 'Copy path' },
  pathCopied: { id: 'agentWorkSection.pathCopied', defaultMessage: 'Agent path copied' },
  askAbout: {
    id: 'agentWorkSection.askAbout',
    defaultMessage: 'Start an AI session about this agent',
  },
  remove: { id: 'agentWorkSection.remove', defaultMessage: 'Remove from roster' },
  confirmRemove: {
    id: 'agentWorkSection.confirmRemove',
    defaultMessage: 'Confirm remove (keeps the folder)',
  },
  removeFailed: {
    id: 'agentWorkSection.removeFailed',
    defaultMessage: 'Could not remove the agent',
  },
  current: { id: 'agentWorkSection.current', defaultMessage: 'Open agent' },
});

const LIVE_TONE: Record<string, Tone> = { running: 'ok', stale: 'err', stopped: 'stopped' };

export const AGENT_WORK_PATH = '/agent-work';

export function deskName(row: AgentWorkRosterRow): string {
  return row.manifest?.title || row.manifest?.name || row.dir.split('/').pop() || row.dir;
}

/** The desk's URL: `/agent-work?desk=<dir>[&tick=<n>]` — the sidebar's link and the view's state. */
export function deskHref(dir: string, tick?: number): string {
  const params = new URLSearchParams({ desk: dir });
  if (tick != null) params.set('tick', String(tick));
  return `${AGENT_WORK_PATH}?${params.toString()}`;
}

/** What is asked of the model when a desk is opened as a chat. */
export function askAboutAgentPrompt(row: AgentWorkRosterRow): string {
  return [
    `I want to work on my Agent Work desk "${deskName(row)}" at ${row.dir}.`,
    'Its agent.yaml (the brief, cadence and tools), its ledger and its ticks live under that folder. Read agent.yaml and the latest ticks first with the developer tools.',
    'You can modify agent.yaml in place, or fork the desk by copying the folder to a new directory with a new name and adjusting its agent.yaml.',
    'Ask me what I want changed before you write anything, then make the edit and show me the result.',
  ].join('\n');
}

interface TicksState {
  ticks: TickRecord[];
  loading: boolean;
  loaded: boolean;
  error: boolean;
}

const TickLeafRow: React.FC<{ tick: TickRecord; active: boolean; onClick: () => void }> = ({
  tick,
  active,
  onClick,
}) => {
  const intl = useIntl();
  const when = timeAgo(tick.ended_at ?? tick.started_at);
  const label = intl.formatMessage(i18n.tick, { n: tick.tick });
  return (
    <button
      onClick={onClick}
      title={tick.summary ?? label}
      aria-current={active ? 'true' : undefined}
      className={cx(
        treeRowClass,
        active ? 'ring-2 ring-inset ring-lz-accent' : 'hover:bg-lz-surface-2'
      )}
      data-testid={`tick-row-${tick.tick}`}
    >
      <span className={cx('shrink-0 text-lz-body text-lz-ink', TNUM)}>{label}</span>
      <span className="min-w-0 flex-1 truncate text-lz-meta text-lz-ink-3">
        {tick.outcome ?? ''}
        {tick.summary ? ` · ${tick.summary}` : ''}
      </span>
      {when ? <span className={cx('shrink-0', TYPE.meta, TNUM)}>{when}</span> : null}
    </button>
  );
};

const DeskRow: React.FC<{
  row: AgentWorkRosterRow;
  now: number;
  expanded: boolean;
  active: boolean;
  activeTick: number | null;
  ticks: TicksState | undefined;
  showAll: boolean;
  onToggle: () => void;
  onOpen: (tick?: number) => void;
  onAsk: () => void;
  onRemove: () => void;
  onRetry: () => void;
  onShowMore: () => void;
  onShowLess: () => void;
}> = ({
  row,
  now,
  expanded,
  active,
  activeTick,
  ticks,
  showAll,
  onToggle,
  onOpen,
  onAsk,
  onRemove,
  onRetry,
  onShowMore,
  onShowLess,
}) => {
  const intl = useIntl();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const live = liveness(row.pid, row.heartbeatMs, now);
  const st = row.state;
  const status =
    live === 'stopped' ? 'stopped' : live === 'stale' ? 'stale' : (st?.status ?? 'unknown');
  const next = st?.next_tick_at && live !== 'stopped' ? Date.parse(st.next_tick_at) - now : null;
  const name = deskName(row);
  const known = ticks?.ticks ?? [];
  const shown = showAll ? known : known.slice(0, TREE_PREVIEW_COUNT);

  return (
    <div data-testid={`desk-row-${row.dir}`}>
      <div
        className="group relative flex items-center"
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <button
          onClick={onToggle}
          aria-expanded={expanded}
          title={row.dir}
          className={cx(treeParentClass, active && 'ring-2 ring-inset ring-lz-accent')}
        >
          {expanded ? (
            <ChevronDown className="size-3.5 shrink-0 text-lz-ink-3" />
          ) : (
            <ChevronRight className="size-3.5 shrink-0 text-lz-ink-3" />
          )}
          <StatusDot tone={LIVE_TONE[live]} live={status === 'ticking'} label={status} />
          <span className={cx('truncate text-lz-body text-lz-ink', WEIGHT.medium)}>{name}</span>
          {!row.exists && <Chip tone="err">{intl.formatMessage(i18n.noManifest)}</Chip>}
          <span className={cx('ml-auto shrink-0', TYPE.meta, TNUM)}>
            {status === 'ticking' && st
              ? `tick ${st.tick}`
              : next != null
                ? countdown(next)
                : status}
          </span>
        </button>
        {menu && (
          <TreeContextMenu
            x={menu.x}
            y={menu.y}
            testId="desk-context-menu"
            onClose={() => setMenu(null)}
            items={[
              {
                key: 'open',
                label: intl.formatMessage(i18n.openDesk),
                icon: <Bot />,
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
                key: 'reveal',
                label: intl.formatMessage(i18n.revealInFinder),
                icon: <FolderOpen />,
                onClick: () => {
                  setMenu(null);
                  void window.electron.revealInFinder(row.dir);
                },
              },
              {
                key: 'copy',
                label: intl.formatMessage(i18n.copyPath),
                icon: <Copy />,
                onClick: () => {
                  setMenu(null);
                  void navigator.clipboard.writeText(row.dir);
                  toast.success(intl.formatMessage(i18n.pathCopied));
                },
              },
              {
                key: 'remove',
                label: intl.formatMessage(i18n.remove),
                icon: <X />,
                danger: true,
                separator: true,
                confirmLabel: intl.formatMessage(i18n.confirmRemove),
                onClick: () => {
                  setMenu(null);
                  onRemove();
                },
              },
            ]}
          />
        )}
      </div>
      {expanded && (
        <TreeChildren>
          <button
            onClick={() => onOpen()}
            className={cx(treeRowClass, 'hover:bg-lz-surface-2 text-lz-accent')}
            data-testid={`desk-open-${row.dir}`}
          >
            <span className="text-lz-body">{intl.formatMessage(i18n.openDesk)}</span>
          </button>
          {shown.map((tick) => (
            <TickLeafRow
              key={tick.tick}
              tick={tick}
              active={active && activeTick === tick.tick}
              onClick={() => onOpen(tick.tick)}
            />
          ))}
          {ticks?.error ? (
            <div className={cx('flex items-center gap-2 px-2', 'h-lz-row-dense')}>
              <span className="text-lz-meta text-lz-err">
                {intl.formatMessage(i18n.ticksFailed)}
              </span>
              <Button variant="ghost" size="sm" onClick={onRetry}>
                {intl.formatMessage(i18n.retry)}
              </Button>
            </div>
          ) : !ticks || ticks.loading ? (
            <div className={treeStateRowClass}>{intl.formatMessage(i18n.loadingTicks)}</div>
          ) : known.length === 0 ? (
            <div className={treeStateRowClass}>{intl.formatMessage(i18n.noTicks)}</div>
          ) : known.length > shown.length ? (
            <Button variant="ghost" size="sm" className="self-start" onClick={onShowMore}>
              {intl.formatMessage(i18n.showMore)}
            </Button>
          ) : showAll && known.length > TREE_PREVIEW_COUNT ? (
            <Button variant="ghost" size="sm" className="self-start" onClick={onShowLess}>
              {intl.formatMessage(i18n.showLess)}
            </Button>
          ) : null}
        </TreeChildren>
      )}
    </div>
  );
};

/**
 * Agent Work in the sidebar, the same shape as Projects: one row per desk (the roster), its ticks
 * nested under it newest first, the open desk marked. Opening a desk or a tick navigates the Agent
 * Work view by URL (`?desk=…&tick=…`) — the view holds no roster of its own any more.
 */
export const AgentWorkSection: React.FC<{ className?: string }> = ({ className }) => {
  const startChat = useStartChatAbout();
  const intl = useIntl();
  const navigate = useNavigate();
  const location = useLocation();
  const [params] = useSearchParams();
  const roster = useAgentRoster(10_000);
  const [now, setNow] = useState(() => Date.now());
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [showAll, setShowAll] = useState<ReadonlySet<string>>(new Set());
  const [ticksByDesk, setTicksByDesk] = useState<Record<string, TicksState>>({});

  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  const onAgentWork = location.pathname === AGENT_WORK_PATH;
  const activeDesk = onAgentWork ? params.get('desk') : null;
  const activeTickRaw = onAgentWork ? params.get('tick') : null;
  const activeTick =
    activeTickRaw != null && /^\d+$/.test(activeTickRaw) ? Number(activeTickRaw) : null;

  const loadTicks = useCallback(async (dir: string) => {
    setTicksByDesk((prev) => ({
      ...prev,
      [dir]: {
        ticks: prev[dir]?.ticks ?? [],
        loading: true,
        loaded: prev[dir]?.loaded ?? false,
        error: false,
      },
    }));
    try {
      const read = await window.electron.agentWorkRead(dir);
      const ticks = [...read.ticks].sort((a, b) => b.tick - a.tick);
      setTicksByDesk((prev) => ({
        ...prev,
        [dir]: { ticks, loading: false, loaded: true, error: false },
      }));
    } catch (error) {
      console.error('Failed to read the desk:', error);
      setTicksByDesk((prev) => ({
        ...prev,
        [dir]: {
          ticks: prev[dir]?.ticks ?? [],
          loading: false,
          loaded: prev[dir]?.loaded ?? false,
          error: true,
        },
      }));
    }
  }, []);

  const toggle = useCallback(
    (dir: string) => {
      const opening = !expanded.has(dir);
      setExpanded((prev) => {
        const next = new Set(prev);
        if (opening) next.add(dir);
        else next.delete(dir);
        return next;
      });
      if (opening && !ticksByDesk[dir]?.loaded) void loadTicks(dir);
    },
    [expanded, ticksByDesk, loadTicks]
  );

  // The open desk's ticks follow the desk while it ticks (a new tick lands within a poll).
  useEffect(() => {
    if (!activeDesk || !expanded.has(activeDesk)) return;
    void loadTicks(activeDesk);
  }, [activeDesk, expanded, loadTicks, roster.rows]);

  const remove = useCallback(
    async (dir: string) => {
      try {
        const ok = await window.electron.agentWorkRemove(dir);
        if (!ok) throw new Error('the engine refused to remove the agent');
        await roster.refresh();
      } catch (error) {
        console.error('Failed to remove the agent:', error);
        toast.error(intl.formatMessage(i18n.removeFailed));
      }
    },
    [roster, intl]
  );

  const rows = useMemo(() => roster.rows, [roster.rows]);

  return (
    <div className={cx('flex min-h-0 flex-col', className)} data-testid="agent-work-section">
      <SectionHeader
        title={intl.formatMessage(i18n.title)}
        count={rows.length}
        className="px-4"
        right={
          <Button
            variant="ghost"
            size="sm"
            icon={<Plus className="text-lz-accent" strokeWidth={2.5} />}
            onClick={() => navigate(`${AGENT_WORK_PATH}?new=1`)}
            aria-label={intl.formatMessage(i18n.newAgent)}
            title={intl.formatMessage(i18n.newAgent)}
          />
        }
      />
      <div className="flex flex-col gap-px px-2 pb-2">
        {roster.loaded && rows.length === 0 ? (
          <div className={cx('px-2 py-2', TYPE.bodyMuted)}>{intl.formatMessage(i18n.empty)}</div>
        ) : (
          rows.map((row) => (
            <DeskRow
              key={row.dir}
              row={row}
              now={now}
              expanded={expanded.has(row.dir)}
              active={activeDesk === row.dir}
              activeTick={activeTick}
              ticks={ticksByDesk[row.dir]}
              showAll={showAll.has(row.dir)}
              onToggle={() => toggle(row.dir)}
              onOpen={(tick) => navigate(deskHref(row.dir, tick))}
              onAsk={() => void startChat(askAboutAgentPrompt(row))}
              onRemove={() => void remove(row.dir)}
              onRetry={() => void loadTicks(row.dir)}
              onShowMore={() => setShowAll((prev) => new Set(prev).add(row.dir))}
              onShowLess={() =>
                setShowAll((prev) => {
                  const next = new Set(prev);
                  next.delete(row.dir);
                  return next;
                })
              }
            />
          ))
        )}
      </div>
    </div>
  );
};
