import {
  useEffect,
  useRef,
  useState,
  type PointerEventHandler,
  type ReactNode,
  type RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import type { FormingStatus } from '@aaif/goose-sdk';
import {
  BookOpen,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  EyeOff,
  Hand,
  Hourglass,
  Loader2,
  Maximize2,
  MessageSquare,
  Minus,
  Moon,
  Network,
  PenLine,
  Play,
  Square,
  X,
} from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, LAYER, MOTION, PHASE_FILL, RADIUS, TNUM, TONE_FILL, WEIGHT, cx } from '../lz';
import type { EngineFigure } from '../leanzero-swarm/engineFigures';
import { formatMlxMode, formatRemoteMode } from '../leanzero-swarm/mlxModeLabel';
import { formatElapsed, formatRate } from '../leanzero-swarm/mlxLiveStats';
import type { EngineGlance, GlancePush, GlanceStage } from '../../utils/engineGlance';
import { backgroundWorkFor, backgroundWorkLabel } from '../sessionActivity/backgroundWorkText';
import { FormingPanel } from '../forming/FormingPanel';

/**
 * The engine glance — the Engine tab's state tile made small. Pure presentation of main's glance
 * (utils/engineGlance.ts): the same solid engine-phase fill, the tile's figures in fewer words, the
 * chat it serves, goose's own calls beside it, and what waits on the person. It leads with what is
 * happening and how fast; the rate ranges and each Mac's memory sit behind "More".
 *
 * The whole card opens the Engine tab (one stretched button under the content); the chat line
 * opens that chat; the small controls sit above it. Two sizes: `dock` (the foot of the sidebar —
 * it hides from its own control, and lists what the chat's turn is forming behind "What it's
 * writing") and `desktop` (the floating mini window, which shrinks to a pill: the stage and its one
 * figure).
 */

const i18n = defineMessages({
  generating: { id: 'engineGlance.stage.generating', defaultMessage: 'Writing' },
  prefill: { id: 'engineGlance.stage.prefill', defaultMessage: 'Reading prompt' },
  queued: { id: 'engineGlance.stage.queued', defaultMessage: 'Queued' },
  idle: { id: 'engineGlance.stage.idle', defaultMessage: 'Idle' },
  notLoaded: { id: 'engineGlance.stage.notLoaded', defaultMessage: 'No model loaded' },
  running: { id: 'engineGlance.stage.running', defaultMessage: 'Running' },
  loading: { id: 'engineGlance.stage.loading', defaultMessage: 'Loading' },
  failed: { id: 'engineGlance.stage.failed', defaultMessage: 'Failed' },
  reconnecting: { id: 'engineGlance.stage.reconnecting', defaultMessage: 'Reconnecting' },
  away: { id: 'engineGlance.stage.away', defaultMessage: 'Mac away' },
  serving: { id: 'engineGlance.stage.serving', defaultMessage: 'Serving' },
  hosting: { id: 'engineGlance.stage.hosting', defaultMessage: 'Serving a rank' },
  held: { id: 'engineGlance.stage.held', defaultMessage: 'Held · memory low' },
  stale: { id: 'engineGlance.stage.stale', defaultMessage: 'Not refreshed' },
  off: { id: 'engineGlance.stage.off', defaultMessage: 'Off' },
  writeRate: { id: 'engineGlance.writeRate', defaultMessage: 'tok/s writing' },
  writeMedian: {
    id: 'engineGlance.writeMedian',
    defaultMessage: 'tok/s writing, {count, plural, one {# run} other {median of # runs}}',
  },
  promptSize: {
    id: 'engineGlance.promptSize',
    defaultMessage: 'prompt tokens, reading for {elapsed}',
  },
  queuedCount: {
    id: 'engineGlance.queuedCount',
    defaultMessage: '{count, plural, one {request waiting} other {requests waiting}}',
  },
  readRate: { id: 'engineGlance.readRate', defaultMessage: 'tok/s reading this prompt' },
  readMedian: {
    id: 'engineGlance.readMedian',
    defaultMessage: 'tok/s reading, {count, plural, one {# prompt} other {median of # prompts}}',
  },
  pillRate: { id: 'engineGlance.pillRate', defaultMessage: '{rate} tok/s' },
  pillPrompt: { id: 'engineGlance.pillPrompt', defaultMessage: '{tokens} · {elapsed}' },
  pillQueued: { id: 'engineGlance.pillQueued', defaultMessage: '{count} waiting' },
  waiting: { id: 'engineGlance.waiting', defaultMessage: '{count} waiting' },
  inflight: {
    id: 'engineGlance.inflight',
    defaultMessage: '{count, plural, one {# request in flight} other {# requests in flight}}',
  },
  chat: { id: 'engineGlance.chat', defaultMessage: 'Chat · {name}' },
  openChat: { id: 'engineGlance.openChat', defaultMessage: 'Open the chat it serves — {name}' },
  others: {
    id: 'engineGlance.others',
    defaultMessage: '{count, plural, one {+# other client} other {+# other clients}}',
  },
  sessionsRunning: {
    id: 'engineGlance.sessionsRunning',
    defaultMessage: '{count, plural, one {# session running} other {# sessions running}}',
  },
  openEngine: { id: 'engineGlance.openEngine', defaultMessage: 'Open the Engine' },
  groupLabel: { id: 'engineGlance.groupLabel', defaultMessage: 'Engine: {stage}' },
  more: { id: 'engineGlance.more', defaultMessage: 'Show rates and memory' },
  less: { id: 'engineGlance.less', defaultMessage: 'Hide rates and memory' },
  collapse: { id: 'engineGlance.collapse', defaultMessage: 'Shrink to a pill' },
  expand: { id: 'engineGlance.expand', defaultMessage: 'Show the whole card' },
  close: {
    id: 'engineGlance.close',
    defaultMessage: 'Hide until the engine is quiet again',
  },
  hide: {
    id: 'engineGlance.hide',
    defaultMessage: 'Hide this card — bring it back from the foot of the sidebar or Settings › App',
  },
  side: { id: 'engineGlance.side', defaultMessage: 'Beside it: {work}' },
  forming: {
    id: 'engineGlance.forming',
    defaultMessage: 'What it’s writing · {count, plural, one {# tool call} other {# tool calls}}',
  },
  formingClose: { id: 'engineGlance.formingClose', defaultMessage: 'Hide what it’s writing' },
  writeRange: {
    id: 'engineGlance.writeRange',
    defaultMessage: 'Writing {low}–{high} tok/s, middle half of runs',
  },
  readRange: {
    id: 'engineGlance.readRange',
    defaultMessage: 'Reading {low}–{high} tok/s, middle half of runs',
  },
  nodePeak: { id: 'engineGlance.nodePeak', defaultMessage: 'peak {peak} of {budget} GB' },
  nodePeakOnly: { id: 'engineGlance.nodePeakOnly', defaultMessage: 'peak {peak} GB' },
  nodeNoPeak: { id: 'engineGlance.nodeNoPeak', defaultMessage: 'no peak yet' },
  nodeBar: { id: 'engineGlance.nodeBar', defaultMessage: 'Memory on {node} against its budget' },
  progressRead: { id: 'engineGlance.progressRead', defaultMessage: 'Prompt read so far' },
  progressLoad: { id: 'engineGlance.progressLoad', defaultMessage: 'Weights loaded so far' },
  needsYou: {
    id: 'engineGlance.needsYou',
    defaultMessage: '{count, plural, one {# needs you} other {# need you}}',
  },
  openQuestion: {
    id: 'engineGlance.openQuestion',
    defaultMessage: 'Open {name} — it asked: {question}',
  },
  untitled: { id: 'engineGlance.untitled', defaultMessage: 'Untitled session' },
  node: { id: 'nodes.glanceNode', defaultMessage: 'Node · {name}' },
  openNode: { id: 'nodes.glanceOpenNode', defaultMessage: 'Open {name} on the Nodes page' },
  nodeUnknown: {
    id: 'nodes.glanceNodeUnknown',
    defaultMessage: 'Which node serves is not known: {error}',
  },
});

const STAGE_WORD: Record<GlanceStage, (typeof i18n)['idle']> = {
  generating: i18n.generating,
  prefill: i18n.prefill,
  queued: i18n.queued,
  idle: i18n.idle,
  not_loaded: i18n.notLoaded,
  running: i18n.running,
  loading: i18n.loading,
  failed: i18n.failed,
  reconnecting: i18n.reconnecting,
  away: i18n.away,
  serving: i18n.serving,
  hosting: i18n.hosting,
  held: i18n.held,
  stale: i18n.stale,
  off: i18n.off,
};

export function stageWord(intl: IntlShape, stage: GlanceStage): string {
  return intl.formatMessage(STAGE_WORD[stage]);
}

export function StageIcon({ stage }: { stage: GlanceStage }) {
  switch (stage) {
    case 'generating':
      return <PenLine />;
    case 'prefill':
      return <BookOpen />;
    case 'queued':
    case 'held':
      return <Hourglass />;
    case 'loading':
    case 'reconnecting':
      return <Loader2 className="animate-spin" />;
    case 'failed':
      return <X />;
    case 'idle':
    case 'stale':
      return <Moon />;
    case 'serving':
    case 'hosting':
    case 'away':
      return <Network />;
    case 'off':
    case 'not_loaded':
      return <Square />;
    case 'running':
      return <Play />;
  }
}

function compact(intl: IntlShape, n: number): string {
  return intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });
}

/** The mode line the tile says too (mlxModeLabel.ts): "Split across 2 Macs · over Thunderbolt". */
export function modeLine(intl: IntlShape, engine: EngineGlance['engine']): string {
  return engine.mode === 'remote'
    ? formatRemoteMode(intl, engine.peerName)
    : formatMlxMode(intl, engine, null);
}

/** A figure as the card says it: the number, and what it is. */
export function figureText(intl: IntlShape, fig: EngineFigure): { value: string; label: string } {
  const rate = (tps: number) => formatRate(tps, intl.locale);
  switch (fig.kind) {
    case 'writing':
      return { value: rate(fig.tps), label: intl.formatMessage(i18n.writeRate) };
    case 'writingMedian':
      return {
        value: rate(fig.median),
        label: intl.formatMessage(i18n.writeMedian, { count: fig.runs }),
      };
    case 'prompt':
      return {
        value: fig.tokens != null ? compact(intl, fig.tokens) : '—',
        label: intl.formatMessage(i18n.promptSize, { elapsed: formatElapsed(fig.elapsedS) }),
      };
    case 'queued':
      return {
        value: intl.formatNumber(fig.count),
        label: intl.formatMessage(i18n.queuedCount, { count: fig.count }),
      };
    case 'reading':
      return { value: rate(fig.tps), label: intl.formatMessage(i18n.readRate) };
    case 'readingMedian':
      return {
        value: rate(fig.median),
        label: intl.formatMessage(i18n.readMedian, { count: fig.runs }),
      };
  }
}

/** The pill's one figure: the live one when there is one, else nothing (never a stale median). */
export function pillFigure(intl: IntlShape, engine: EngineGlance): string | null {
  const hero = engine.hero;
  if (!hero || !engine.busy) return null;
  switch (hero.kind) {
    case 'writing':
      return intl.formatMessage(i18n.pillRate, { rate: formatRate(hero.tps, intl.locale) });
    case 'prompt': {
      const reading = engine.second?.kind === 'reading' ? engine.second.tps : null;
      if (reading != null) {
        return intl.formatMessage(i18n.pillRate, { rate: formatRate(reading, intl.locale) });
      }
      return hero.tokens != null
        ? intl.formatMessage(i18n.pillPrompt, {
            tokens: compact(intl, hero.tokens),
            elapsed: formatElapsed(hero.elapsedS),
          })
        : formatElapsed(hero.elapsedS);
    }
    case 'queued':
      return intl.formatMessage(i18n.pillQueued, { count: hero.count });
    default:
      return null;
  }
}

/** A hollow track in the card's ink with a solid fill of it — no tint, no opacity (the tile's bar). */
function Bar({
  progress,
  label,
}: {
  progress: EngineGlance['progress'] | { done: number; total: number };
  label: string;
}) {
  if (progress == null) return null;
  if (progress === 'indeterminate') {
    return (
      <div
        role="progressbar"
        aria-label={label}
        aria-valuetext={label}
        data-testid="engine-glance-progress"
        data-measured="false"
        className={cx('relative h-2 w-full overflow-hidden border border-current', RADIUS.pill)}
      >
        <div className="absolute inset-y-0 left-0 w-1/3 animate-lz-indeterminate bg-current" />
      </div>
    );
  }
  const pct = Math.round(
    Math.min(1, Math.max(0, progress.total > 0 ? progress.done / progress.total : 0)) * 100
  );
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      data-testid="engine-glance-progress"
      data-measured="true"
      className={cx('h-2 w-full overflow-hidden border border-current', RADIUS.pill)}
    >
      <div className="h-full bg-current" style={{ width: `${pct}%` }} />
    </div>
  );
}

const CONTROL = cx(
  'pointer-events-auto flex size-7 shrink-0 items-center justify-center border border-transparent hover:border-current [&_svg]:size-4',
  RADIUS.control,
  FOCUS,
  MOTION
);

function Control({
  label,
  onClick,
  children,
  testId,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  testId: string;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      aria-label={label}
      title={label}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={CONTROL}
    >
      {children}
    </button>
  );
}

function Details({ engine }: { engine: EngineGlance }) {
  const intl = useIntl();
  const range = (r: { low: number; high: number }) => ({
    low: formatRate(r.low, intl.locale),
    high: formatRate(r.high, intl.locale),
  });
  const gb = (n: number) =>
    intl.formatNumber(n, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const lines = [
    engine.ranges.writing
      ? intl.formatMessage(i18n.writeRange, range(engine.ranges.writing))
      : null,
    engine.ranges.reading ? intl.formatMessage(i18n.readRange, range(engine.ranges.reading)) : null,
  ].filter((l): l is string => l != null);
  if (lines.length === 0 && engine.nodes.length === 0) return null;
  return (
    <div
      data-testid="engine-glance-details"
      className="flex flex-col gap-2 border-t border-current pt-2"
    >
      {lines.map((line) => (
        <span key={line} className={cx('text-lz-meta', TNUM)}>
          {line}
        </span>
      ))}
      {engine.nodes.map((node) => (
        <div key={node.name} data-testid="engine-glance-node" className="flex flex-col gap-1">
          <div className={cx('flex items-baseline justify-between gap-2 text-lz-meta', TNUM)}>
            <span className={cx('min-w-0 truncate', WEIGHT.semibold)}>{node.name}</span>
            <span className="shrink-0">
              {node.peakGb == null
                ? intl.formatMessage(i18n.nodeNoPeak)
                : node.budgetGb != null
                  ? intl.formatMessage(i18n.nodePeak, {
                      peak: gb(node.peakGb),
                      budget: gb(node.budgetGb),
                    })
                  : intl.formatMessage(i18n.nodePeakOnly, { peak: gb(node.peakGb) })}
            </span>
          </div>
          {node.load ? (
            <Bar progress={node.load} label={intl.formatMessage(i18n.progressLoad)} />
          ) : node.peakGb != null && node.budgetGb != null && node.budgetGb > 0 ? (
            <Bar
              progress={{ done: node.peakGb, total: node.budgetGb }}
              label={intl.formatMessage(i18n.nodeBar, { node: node.name })}
            />
          ) : null}
        </div>
      ))}
    </div>
  );
}

function NeedsYouStrip({
  push,
  onOpenSession,
}: {
  push: GlancePush;
  onOpenSession: (sessionId: string) => void;
}) {
  const intl = useIntl();
  const items = push.sessions.needsYou;
  if (items.length === 0) return null;
  const first = items[0];
  const name = first.sessionName || intl.formatMessage(i18n.untitled);
  return (
    <button
      type="button"
      data-testid="engine-glance-needs-you"
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        onOpenSession(first.sessionId);
      }}
      title={intl.formatMessage(i18n.openQuestion, { name, question: first.question })}
      className={cx(
        'pointer-events-auto flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left text-lz-meta [&_svg]:size-4 [&_svg]:shrink-0',
        TONE_FILL.warn,
        RADIUS.control,
        FOCUS
      )}
    >
      <Hand aria-hidden />
      <span className={cx('shrink-0', WEIGHT.semibold, TNUM)}>
        {intl.formatMessage(i18n.needsYou, { count: items.length })}
      </span>
      <span className="min-w-0 truncate">{name}</span>
    </button>
  );
}

/**
 * The node the serving way belongs to (design §7.3), under the mode line: a link to its card on the
 * Nodes page where the surface can navigate, plain words where it cannot (the desktop window).
 * Two nodes naming one way are both said; the link opens the first (the chat's own, else pinned).
 */
function NodeLine({
  servedBy,
  onOpenNode,
}: {
  servedBy: NonNullable<EngineGlance['servedBy']>;
  onOpenNode?: (nodeId: string) => void;
}) {
  const intl = useIntl();
  if ('error' in servedBy) {
    return (
      <span
        data-testid="engine-glance-node-unknown"
        title={servedBy.error}
        className="line-clamp-2 break-words text-lz-meta"
      >
        {intl.formatMessage(i18n.nodeUnknown, { error: servedBy.error })}
      </span>
    );
  }
  const name = intl.formatList(
    servedBy.nodes.map((n) => n.name),
    { type: 'conjunction' }
  );
  const text = intl.formatMessage(i18n.node, { name });
  const first = servedBy.nodes[0];
  if (!onOpenNode) {
    return (
      <span
        data-testid="engine-glance-served-node"
        className={cx('line-clamp-2 break-words text-lz-meta', WEIGHT.semibold)}
      >
        {text}
      </span>
    );
  }
  const label = intl.formatMessage(i18n.openNode, { name: first.name });
  return (
    <button
      type="button"
      data-testid="engine-glance-served-node"
      data-node={first.id}
      title={label}
      aria-label={label}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        onOpenNode(first.id);
      }}
      className={cx(
        'pointer-events-auto flex min-w-0 max-w-full items-start gap-1.5 self-start text-left text-lz-meta underline decoration-1 underline-offset-2 hover:decoration-2 [&_svg]:mt-px [&_svg]:size-3.5',
        WEIGHT.semibold,
        RADIUS.control,
        FOCUS
      )}
    >
      <Network aria-hidden />
      <span className="line-clamp-2 min-w-0 break-words">{text}</span>
    </button>
  );
}

export type GlanceVariant = 'dock' | 'desktop';

export interface EngineGlanceCardProps {
  push: GlancePush;
  variant: GlanceVariant;
  collapsed: boolean;
  expanded: boolean;
  onOpenEngine: () => void;
  onOpenSession: (sessionId: string) => void;
  /** Opens a node's card on the Nodes page. Absent = the node is named, not linked. */
  onOpenNode?: (nodeId: string) => void;
  onToggleExpanded: () => void;
  onCollapsedChange: (collapsed: boolean) => void;
  /** The desktop window's close: snoozes it for this live spell. Absent = no close control. */
  onClose?: () => void;
  /** The docked card's hide: gone until the person brings it back (Q-218). Absent = no control. */
  onHide?: () => void;
  /**
   * What the served chat's turn is still forming (formingStore.ts) — the docked card lists it
   * behind "What it's writing" while the chat line is that turn (Q-215). Absent = never offered.
   */
  forming?: FormingStatus | null;
  /** The card is moved by dragging its body (the desktop window); a click still opens. */
  dragHandlers?: {
    onPointerDown: PointerEventHandler<HTMLElement>;
    onPointerMove: PointerEventHandler<HTMLElement>;
    onPointerUp: PointerEventHandler<HTMLElement>;
    onPointerCancel: PointerEventHandler<HTMLElement>;
  };
  /** A drag just ended: the click it produced must not open the Engine. */
  consumeDrag?: () => boolean;
  /**
   * The served chat's name as this window's session lists show it (" · 5" included, Q-185);
   * absent (the desktop window, which lists no sessions) = the session's own name.
   */
  chatName?: (sessionId: string, name: string) => string;
}

const WIDTH: Record<GlanceVariant, string> = {
  dock: 'w-full',
  desktop: 'w-[300px]',
};

/** px between the docked card and the forming panel that opens beside it, over the content. */
const PANEL_GAP_PX = 8;
/** The panel never grows wider than a reading column, and keeps this much off the window edge. */
const PANEL_MAX_WIDTH_PX = 560;
const PANEL_EDGE_PX = 16;

/**
 * What the turn is forming, opened beside the docked card over the content: the sidebar is too
 * narrow for call titles and the text beside them, and its frame clips anything that overflows, so
 * the panel is portalled and placed from the card's own rect (tree.tsx's context menu does the same).
 */
function FormingPopover({
  anchor,
  forming,
  onClose,
}: {
  anchor: RefObject<HTMLDivElement | null>;
  forming: FormingStatus;
  onClose: () => void;
}) {
  const [rect, setRect] = useState<{ right: number; bottom: number } | null>(null);
  useEffect(() => {
    const place = () => setRect(anchor.current?.getBoundingClientRect() ?? null);
    place();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('resize', place);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('resize', place);
      window.removeEventListener('keydown', onKey);
    };
  }, [anchor, onClose]);
  if (!rect) return null;
  const left = rect.right + PANEL_GAP_PX;
  return createPortal(
    <FormingPanel
      forming={forming}
      className={cx('fixed', LAYER.overlay)}
      style={{
        left,
        bottom: window.innerHeight - rect.bottom,
        width: Math.max(0, Math.min(PANEL_MAX_WIDTH_PX, window.innerWidth - left - PANEL_EDGE_PX)),
        maxHeight: Math.max(0, rect.bottom - PANEL_EDGE_PX),
      }}
    />,
    document.body
  );
}

export function EngineGlanceCard(props: EngineGlanceCardProps) {
  const intl = useIntl();
  const { push, variant, collapsed, expanded } = props;
  const engine = push.engine;
  const cardRef = useRef<HTMLDivElement>(null);
  const [formingOpen, setFormingOpen] = useState(false);
  // Only while the chat line is that chat's TURN: goose's fact check forms nothing of the answer.
  const forming =
    engine.chat && engine.chat.work == null && props.forming && props.forming.calls.length > 0
      ? props.forming
      : null;
  // No engine to speak of, only a question: the card is the question, in its solid warn fill, and
  // a click opens the chat that asked it.
  const question = engine.present ? null : (push.sessions.needsYou[0] ?? null);
  const questionName = question ? question.sessionName || intl.formatMessage(i18n.untitled) : null;
  const openEngine = () => {
    if (props.consumeDrag?.()) return;
    if (question) props.onOpenSession(question.sessionId);
    else props.onOpenEngine();
  };
  const phaseFill = engine.present ? PHASE_FILL[engine.phase] : TONE_FILL.warn;
  const word = engine.present
    ? stageWord(intl, engine.stage)
    : intl.formatMessage(i18n.needsYou, { count: push.sessions.needsYou.length });
  const openLabel = question
    ? intl.formatMessage(i18n.openQuestion, { name: questionName, question: question.question })
    : intl.formatMessage(i18n.openEngine);
  const chatName = engine.chat
    ? (props.chatName?.(engine.chat.sessionId, engine.chat.name) ?? engine.chat.name)
    : '';
  // goose's own call for the chat (the fact check after the reply) is named as that (Q-185).
  const chatText = engine.chat?.work
    ? backgroundWorkFor(intl, engine.chat.work, chatName)
    : intl.formatMessage(i18n.chat, { name: chatName });
  const stretched = (
    <button
      type="button"
      data-testid="engine-glance-open"
      aria-label={openLabel}
      title={openLabel}
      onClick={openEngine}
      {...props.dragHandlers}
      className={cx('absolute inset-0 z-0 cursor-pointer', RADIUS.card, FOCUS)}
    />
  );

  if (collapsed) {
    const figure = engine.present ? pillFigure(intl, engine) : null;
    const needs = push.sessions.needsYou.length;
    return (
      <div
        role="group"
        aria-label={intl.formatMessage(i18n.groupLabel, { stage: word })}
        data-testid="engine-glance"
        data-variant={variant}
        data-collapsed="true"
        data-phase={engine.phase}
        data-stage={engine.stage}
        className={cx(
          'relative inline-flex h-9 max-w-full items-center gap-2 whitespace-nowrap pl-3 pr-1 [&_svg]:size-4 [&_svg]:shrink-0',
          RADIUS.pill,
          phaseFill
        )}
      >
        {stretched}
        <span className="pointer-events-none relative z-10 flex min-w-0 items-center gap-2">
          <span aria-hidden>{engine.present ? <StageIcon stage={engine.stage} /> : <Hand />}</span>
          <span className={cx('truncate text-lz-body', WEIGHT.semibold)}>{word}</span>
          {figure && (
            <span data-testid="engine-glance-pill-figure" className={cx('text-lz-body', TNUM)}>
              {figure}
            </span>
          )}
          {engine.present && needs > 0 && (
            <span
              data-testid="engine-glance-pill-needs"
              className={cx(
                'px-1.5 text-lz-meta',
                WEIGHT.semibold,
                TNUM,
                RADIUS.pill,
                TONE_FILL.warn
              )}
            >
              {intl.formatMessage(i18n.needsYou, { count: needs })}
            </span>
          )}
        </span>
        <span className="relative z-10 flex items-center">
          <Control
            testId="engine-glance-expand"
            label={intl.formatMessage(i18n.expand)}
            onClick={() => props.onCollapsedChange(false)}
          >
            <Maximize2 />
          </Control>
          {props.onClose && (
            <Control
              testId="engine-glance-close"
              label={intl.formatMessage(i18n.close)}
              onClick={props.onClose}
            >
              <X />
            </Control>
          )}
        </span>
      </div>
    );
  }

  const hero = engine.hero ? figureText(intl, engine.hero) : null;
  const second = engine.second ? figureText(intl, engine.second) : null;
  const facts = [
    engine.waiting != null && engine.waiting > 0 && engine.hero?.kind !== 'queued'
      ? intl.formatMessage(i18n.waiting, { count: engine.waiting })
      : null,
    engine.inflight != null ? intl.formatMessage(i18n.inflight, { count: engine.inflight }) : null,
    !engine.busy && push.sessions.running > 0
      ? intl.formatMessage(i18n.sessionsRunning, { count: push.sessions.running })
      : null,
  ].filter((f): f is string => f != null);
  const hasDetails =
    engine.ranges.writing != null || engine.ranges.reading != null || engine.nodes.length > 0;
  const progressLabel = intl.formatMessage(
    engine.stage === 'prefill' ? i18n.progressRead : i18n.progressLoad
  );

  return (
    <div
      ref={cardRef}
      role="group"
      aria-label={intl.formatMessage(i18n.groupLabel, { stage: word })}
      data-testid="engine-glance"
      data-variant={variant}
      data-collapsed="false"
      data-phase={engine.present ? engine.phase : 'needs-you'}
      data-stage={engine.present ? engine.stage : 'needs-you'}
      className={cx(
        'relative flex flex-col gap-2.5 p-3 [&_svg]:shrink-0',
        WIDTH[variant],
        RADIUS.card,
        phaseFill
      )}
    >
      {stretched}
      <div className="pointer-events-none relative z-10 flex flex-col gap-2.5">
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="flex min-w-0 items-center gap-1.5 [&_svg]:size-4">
              <span aria-hidden>
                {engine.present ? <StageIcon stage={engine.stage} /> : <Hand />}
              </span>
              <span
                role="status"
                data-testid="engine-glance-stage"
                className={cx('truncate text-lz-h2', WEIGHT.semibold)}
              >
                {word}
              </span>
            </span>
            {engine.present && (
              <span
                data-testid="engine-glance-mode"
                className={cx('break-words text-lz-meta', WEIGHT.semibold)}
              >
                {modeLine(intl, engine.engine)}
              </span>
            )}
          </div>
          <span className="flex shrink-0 items-center">
            {hasDetails && (
              <Control
                testId="engine-glance-details-toggle"
                label={intl.formatMessage(expanded ? i18n.less : i18n.more)}
                onClick={props.onToggleExpanded}
              >
                {expanded ? <ChevronUp /> : <ChevronDown />}
              </Control>
            )}
            {variant !== 'dock' && (
              <Control
                testId="engine-glance-collapse"
                label={intl.formatMessage(i18n.collapse)}
                onClick={() => props.onCollapsedChange(true)}
              >
                <Minus />
              </Control>
            )}
            {props.onClose && (
              <Control
                testId="engine-glance-close"
                label={intl.formatMessage(i18n.close)}
                onClick={props.onClose}
              >
                <X />
              </Control>
            )}
            {props.onHide && (
              <Control
                testId="engine-glance-hide"
                label={intl.formatMessage(i18n.hide)}
                onClick={props.onHide}
              >
                <EyeOff />
              </Control>
            )}
          </span>
        </div>
        {/* The node the way belongs to: under the mode line, on its own full-width row so a node
            name is never squeezed by the controls beside the header. */}
        {engine.present && engine.servedBy && (
          <NodeLine servedBy={engine.servedBy} onOpenNode={props.onOpenNode} />
        )}
        {question && (
          <div data-testid="engine-glance-question" className="flex min-w-0 flex-col gap-0.5">
            <span className={cx('truncate text-lz-body', WEIGHT.semibold)}>{questionName}</span>
            {question.question && (
              <span className="line-clamp-2 break-words text-lz-meta">{question.question}</span>
            )}
          </div>
        )}
        {engine.present && engine.modelId && (
          <span
            data-testid="engine-glance-model"
            title={engine.modelId}
            className="truncate font-mono text-lz-mono"
          >
            {engine.modelId}
          </span>
        )}
        {hero && (
          <div className="flex min-w-0 items-baseline gap-2">
            <span
              data-testid="engine-glance-hero"
              className={cx(
                'shrink-0 leading-none tracking-tight',
                engine.busy ? 'text-[28px]' : 'text-lz-h2',
                WEIGHT.semibold,
                TNUM
              )}
            >
              {hero.value}
            </span>
            <span className="min-w-0 text-lz-meta leading-tight">{hero.label}</span>
          </div>
        )}
        {engine.present && <Bar progress={engine.progress} label={progressLabel} />}
        {(second || facts.length > 0) && (
          <div className={cx('flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-lz-meta', TNUM)}>
            {second && (
              <span data-testid="engine-glance-second">
                <span className={WEIGHT.semibold}>{second.value}</span> {second.label}
              </span>
            )}
            {facts.map((f) => (
              <span key={f} data-testid="engine-glance-fact">
                {f}
              </span>
            ))}
          </div>
        )}
        {engine.detail && (
          <span
            data-testid="engine-glance-detail"
            title={engine.detail}
            className={cx('line-clamp-2 break-words text-lz-meta', WEIGHT.semibold)}
          >
            {engine.detail}
          </span>
        )}
        {engine.chat && (
          <button
            type="button"
            data-testid="engine-glance-chat"
            data-work={engine.chat.work ?? undefined}
            title={intl.formatMessage(i18n.openChat, { name: chatName })}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onOpenSession(engine.chat!.sessionId);
            }}
            className={cx(
              'pointer-events-auto flex min-w-0 max-w-full items-center gap-1.5 self-start text-left text-lz-meta underline decoration-1 underline-offset-2 hover:decoration-2 [&_svg]:size-3.5',
              WEIGHT.semibold,
              RADIUS.control,
              FOCUS
            )}
          >
            <MessageSquare aria-hidden />
            <span className="min-w-0 truncate">{chatText}</span>
          </button>
        )}
        {forming && (
          <button
            type="button"
            data-testid="engine-glance-forming-toggle"
            aria-expanded={formingOpen}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              setFormingOpen((open) => !open);
            }}
            className={cx(
              'pointer-events-auto flex min-w-0 max-w-full items-center gap-1 self-start text-left text-lz-meta underline decoration-1 underline-offset-2 hover:decoration-2 [&_svg]:size-3.5',
              WEIGHT.semibold,
              TNUM,
              RADIUS.control,
              FOCUS
            )}
          >
            <span className="min-w-0 truncate">
              {formingOpen
                ? intl.formatMessage(i18n.formingClose)
                : intl.formatMessage(i18n.forming, { count: forming.calls.length })}
            </span>
            <ChevronRight aria-hidden className={cx(formingOpen && 'rotate-180', MOTION)} />
          </button>
        )}
        {engine.side.length > 0 && (
          <span data-testid="engine-glance-side" className="break-words text-lz-meta">
            {intl.formatMessage(i18n.side, {
              work: intl.formatList(
                engine.side.map((kind) => backgroundWorkLabel(intl, kind)),
                { type: 'conjunction' }
              ),
            })}
          </span>
        )}
        {engine.otherClients > 0 && (
          <span data-testid="engine-glance-others" className={cx('text-lz-meta', TNUM)}>
            {intl.formatMessage(i18n.others, { count: engine.otherClients })}
          </span>
        )}
        {expanded && <Details engine={engine} />}
        {engine.present && <NeedsYouStrip push={push} onOpenSession={props.onOpenSession} />}
      </div>
      {forming && formingOpen && (
        <FormingPopover anchor={cardRef} forming={forming} onClose={() => setFormingOpen(false)} />
      )}
    </div>
  );
}
