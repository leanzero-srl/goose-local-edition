import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Blocks, Loader2, PowerOff, RotateCcw, Wrench } from 'lucide-react';
import { AppEvents } from '../../constants/events';
import { defineMessages, useIntl } from '../../i18n';
import { errorMessage } from '../../utils/conversionUtils';
import { removeFromAgent } from '../settings/extensions/agent-api';
import { formatExtensionName } from '../settings/extensions/subcomponents/ExtensionList';
import {
  Button,
  Chip,
  Disclosure,
  SURFACE,
  SPACE,
  TNUM,
  TONE_DOT,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import {
  listEnvelopeBytes,
  planTurnOff,
  withinBounds,
  type ToolBounds,
  type ToolGroupSize,
} from './toolSchemaBounds';
import { useSessionToolSizes, type ExtensionToolSize } from './useSessionToolSizes';

const i18n = defineMessages({
  title: { id: 'toolBoundsNotice.title', defaultMessage: 'Too many tools for this engine' },
  summary: {
    id: 'toolBoundsNotice.summary',
    defaultMessage:
      'The engine compiles every tool this session sends into a grammar and refuses a set larger than its bounds. Turn off extensions this session does not need, then retry.',
  },
  boundsLabel: { id: 'toolBoundsNotice.boundsLabel', defaultMessage: 'Engine bounds' },
  boundTools: { id: 'toolBoundsNotice.boundTools', defaultMessage: 'max {n, number} tools' },
  boundBytes: { id: 'toolBoundsNotice.boundBytes', defaultMessage: '{n, number} bytes' },
  boundDepth: { id: 'toolBoundsNotice.boundDepth', defaultMessage: 'depth {n, number}' },
  noBounds: {
    id: 'toolBoundsNotice.noBounds',
    defaultMessage: 'The engine did not state its bounds in numbers — its words are below.',
  },
  measuring: {
    id: 'toolBoundsNotice.measuring',
    defaultMessage: 'Measuring this session’s tools',
  },
  measureFailed: {
    id: 'toolBoundsNotice.measureFailed',
    defaultMessage: 'Could not read this session’s tools, so their sizes are unknown: {error}',
  },
  sizesNote: {
    id: 'toolBoundsNotice.sizesNote',
    defaultMessage:
      'Sizes are measured by the app from this session’s tool list in the engine’s own units: each tool’s name and parameters as compact JSON. Descriptions are not compiled and not counted.',
  },
  bytesOver: {
    id: 'toolBoundsNotice.bytesOver',
    defaultMessage: '{total, number} of {limit, number} bytes — {over, number} over',
  },
  bytesWithin: {
    id: 'toolBoundsNotice.bytesWithin',
    defaultMessage: '{total, number} of {limit, number} bytes — within the limit',
  },
  toolsOver: {
    id: 'toolBoundsNotice.toolsOver',
    defaultMessage: '{total, number} of {limit, number} tools — {over, number} over',
  },
  depthOver: {
    id: 'toolBoundsNotice.depthOver',
    defaultMessage: 'Nesting depth {total, number} of {limit, number} — too deep',
  },
  planLeadBytes: {
    id: 'toolBoundsNotice.planLeadBytes',
    defaultMessage:
      'Turning off {n, plural, one {this extension} other {these # extensions}} brings the session to {after, number} of {limit, number} bytes.',
  },
  planLead: {
    id: 'toolBoundsNotice.planLead',
    defaultMessage:
      'Turning off {n, plural, one {this extension} other {these # extensions}} brings the session within the engine’s bounds.',
  },
  planAndRetry: {
    id: 'toolBoundsNotice.planAndRetry',
    defaultMessage:
      '{n, plural, one {Turn off this one and retry} other {Turn off these # and retry}}',
  },
  planOnly: {
    id: 'toolBoundsNotice.planOnly',
    defaultMessage: '{n, plural, one {Turn off this one} other {Turn off these #}}',
  },
  unreachable: {
    id: 'toolBoundsNotice.unreachable',
    defaultMessage:
      'Turning off every extension would not bring this session within the bounds — goose’s own tools alone exceed them.',
  },
  showAll: {
    id: 'toolBoundsNotice.showAll',
    defaultMessage: 'Show all extensions ({n, number})',
  },
  hideAll: {
    id: 'toolBoundsNotice.hideAll',
    defaultMessage: 'Hide all extensions ({n, number})',
  },
  noTools: {
    id: 'toolBoundsNotice.noTools',
    defaultMessage:
      '{n, plural, one {# extension adds no tools} other {# extensions add no tools}}',
  },
  rowSize: {
    id: 'toolBoundsNotice.rowSize',
    defaultMessage: '{tools, plural, one {# tool} other {# tools}} · {bytes, number} bytes',
  },
  rowTooDeep: { id: 'toolBoundsNotice.rowTooDeep', defaultMessage: 'depth {n, number}' },
  unowned: {
    id: 'toolBoundsNotice.unowned',
    defaultMessage: 'goose’s own tools (no extension to turn off)',
  },
  noExtensions: {
    id: 'toolBoundsNotice.noExtensions',
    defaultMessage: 'This session has no extensions to turn off.',
  },
  turnOff: { id: 'toolBoundsNotice.turnOff', defaultMessage: 'Turn off for this session' },
  turningOff: { id: 'toolBoundsNotice.turningOff', defaultMessage: 'Turning off' },
  turnOffFailed: {
    id: 'toolBoundsNotice.turnOffFailed',
    defaultMessage: 'Could not turn it off: {error}',
  },
  within: {
    id: 'toolBoundsNotice.within',
    defaultMessage:
      'The session’s tools are now within the engine’s bounds — retry to send your message again.',
  },
  retry: { id: 'toolBoundsNotice.retry', defaultMessage: 'Retry' },
  openMcps: { id: 'toolBoundsNotice.openMcps', defaultMessage: 'Open MCPs' },
});

function pct(part: number, whole: number): string {
  return `${Math.min(100, Math.max(0, (part / whole) * 100))}%`;
}

/** The measured total against the stated limit: solid fill up to the limit, the overflow in solid
 *  err past it, and the limit itself as a mark that stands out of the track. */
function LimitBar({ value, limit }: { value: number; limit: number }) {
  const scale = Math.max(value, limit, 1);
  const over = value > limit;
  return (
    <div data-testid="tool-bounds-bar" className="relative h-2.5 w-full">
      <div className="absolute inset-0 overflow-hidden rounded-full bg-lz-surface-2">
        <div
          className={cx('absolute inset-y-0 left-0', over ? TONE_DOT.accent : TONE_DOT.ok)}
          style={{ width: pct(Math.min(value, limit), scale) }}
        />
        {over && (
          <div
            data-testid="tool-bounds-bar-over"
            className={cx('absolute inset-y-0', TONE_DOT.err)}
            style={{ left: pct(limit, scale), width: pct(value - limit, scale) }}
          />
        )}
      </div>
      <div
        data-testid="tool-bounds-limit-mark"
        className="absolute -inset-y-1 w-0.5 -translate-x-1/2 rounded-full bg-lz-ink"
        style={{ left: pct(limit, scale) }}
      />
    </div>
  );
}

function ExtensionRow({
  extension,
  testIdPrefix,
  largest,
  tooDeep,
  busy,
  failure,
  onTurnOff,
}: {
  extension: ExtensionToolSize;
  testIdPrefix: string;
  /** The largest row's bytes — a bar is drawn relative to it; omitted, no bar. */
  largest?: number;
  tooDeep: boolean;
  /** The extensions being turned off right now; null when nothing is in flight. */
  busy: readonly string[] | null;
  failure: string | undefined;
  onTurnOff: (name: string) => void;
}) {
  const intl = useIntl();
  const turningOffThis = busy?.includes(extension.name) ?? false;
  return (
    <li
      data-testid={`${testIdPrefix}-row-${extension.name}`}
      className="flex items-center gap-3 py-2"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className={cx(TYPE.body, WEIGHT.medium, 'truncate')}>
            {formatExtensionName(extension.name)}
          </span>
          {tooDeep && (
            <Chip tone="err">{intl.formatMessage(i18n.rowTooDeep, { n: extension.depth })}</Chip>
          )}
        </div>
        <span className={cx(TYPE.meta, TNUM)}>
          {intl.formatMessage(i18n.rowSize, { tools: extension.tools, bytes: extension.bytes })}
        </span>
        {largest != null && (
          <div className="h-1.5 w-full rounded-full bg-lz-surface-2">
            <div
              className={cx('h-1.5 rounded-full', TONE_DOT.accent)}
              style={{ width: pct(extension.bytes, Math.max(1, largest)) }}
            />
          </div>
        )}
        {failure && (
          <p className={cx(TYPE.meta, TONE_TEXT.err, 'break-words')}>
            {intl.formatMessage(i18n.turnOffFailed, { error: failure })}
          </p>
        )}
      </div>
      <Button
        variant="secondary"
        size="sm"
        icon={turningOffThis ? <Loader2 className="animate-spin" /> : <PowerOff />}
        disabled={busy != null}
        data-testid={`${testIdPrefix}-turn-off-${extension.name}`}
        onClick={() => onTurnOff(extension.name)}
      >
        {intl.formatMessage(turningOffThis ? i18n.turningOff : i18n.turnOff)}
      </Button>
    </li>
  );
}

/** The measured total against each stated bound. The byte line always leads when the engine
 *  stated a byte bound; tools and depth speak only when they are the ones over. */
function MeasureLines({ bounds, total }: { bounds: ToolBounds; total: ToolGroupSize }) {
  const intl = useIntl();
  const listBytes = listEnvelopeBytes(total.tools, total.bytes);
  const toolsOver = bounds.maxTools != null && total.tools > bounds.maxTools;
  const depthOver = bounds.maxDepth != null && total.depth > bounds.maxDepth;
  return (
    <div data-testid="tool-bounds-total" className="flex flex-col gap-2">
      {bounds.maxBytes != null && (
        <div className="flex flex-col gap-1.5">
          <p
            data-testid="tool-bounds-bytes-line"
            className={cx(
              TYPE.body,
              WEIGHT.semibold,
              TNUM,
              listBytes > bounds.maxBytes ? TONE_TEXT.err : TONE_TEXT.ok
            )}
          >
            {listBytes > bounds.maxBytes
              ? intl.formatMessage(i18n.bytesOver, {
                  total: listBytes,
                  limit: bounds.maxBytes,
                  over: listBytes - bounds.maxBytes,
                })
              : intl.formatMessage(i18n.bytesWithin, { total: listBytes, limit: bounds.maxBytes })}
          </p>
          <LimitBar value={listBytes} limit={bounds.maxBytes} />
        </div>
      )}
      {toolsOver && bounds.maxTools != null && (
        <p className={cx(TYPE.body, WEIGHT.semibold, TNUM, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.toolsOver, {
            total: total.tools,
            limit: bounds.maxTools,
            over: total.tools - bounds.maxTools,
          })}
        </p>
      )}
      {depthOver && bounds.maxDepth != null && (
        <p className={cx(TYPE.body, WEIGHT.semibold, TNUM, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.depthOver, { total: total.depth, limit: bounds.maxDepth })}
        </p>
      )}
    </div>
  );
}

/**
 * The engine's "tool schema exceeds grammar-compile bounds" refusal, decision first: the measured
 * total against the limit the engine STATED (parsed from its words, never assumed), the smallest
 * set of extensions whose removal brings the session within it with one action that turns them
 * all off and resends, and every other extension behind a disclosure. Turning off uses the same
 * session toggle the chat's extension menu uses, then re-measures from the session's new tool list.
 *
 * `live` is true only while this refusal is the conversation's latest message: an older notice is
 * a record of a past session state, so it neither measures the current tools nor offers actions
 * that would act on them.
 */
export default function ToolBoundsNotice({
  bounds,
  sessionId,
  live,
  retryText,
  onRetry,
}: {
  bounds: ToolBounds;
  sessionId: string;
  live: boolean;
  /** The last user turn's text; null when there is none that can be resent faithfully. */
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const intl = useIntl();
  const navigate = useNavigate();
  const { sizes, reload } = useSessionToolSizes(sessionId, live);
  const [busy, setBusy] = useState<string[] | null>(null);
  const [planInFlight, setPlanInFlight] = useState(false);
  const [failures, setFailures] = useState<Record<string, string>>({});
  const [showAll, setShowAll] = useState(false);

  /** Turns `names` off in order, stopping at the first refusal; true when all went. */
  const removeAll = async (names: string[]): Promise<boolean> => {
    let removed = 0;
    try {
      for (const name of names) {
        try {
          await removeFromAgent(name, sessionId, true);
          removed++;
          setFailures(({ [name]: _gone, ...rest }) => rest);
        } catch (error) {
          setFailures((prev) => ({ ...prev, [name]: errorMessage(error) }));
          return false;
        }
      }
      return true;
    } finally {
      if (removed > 0) {
        window.dispatchEvent(
          new CustomEvent(AppEvents.SESSION_EXTENSIONS_LOADED, { detail: { sessionId } })
        );
      }
    }
  };

  const turnOff = async (name: string) => {
    setBusy([name]);
    try {
      if (await removeAll([name])) await reload();
    } finally {
      setBusy(null);
    }
  };

  const turnOffPlanAndRetry = async (names: string[]) => {
    setBusy(names);
    setPlanInFlight(true);
    try {
      const allGone = await removeAll(names);
      const next = await reload();
      if (!allGone || retryText == null || next?.state !== 'ready') return;
      if (withinBounds(bounds, next.total) !== false) onRetry(retryText);
    } finally {
      setBusy(null);
      setPlanInFlight(false);
    }
  };

  const ready = live && sizes.state === 'ready' ? sizes : null;
  const plan = ready ? planTurnOff(bounds, ready.total, ready.extensions, ready.unowned) : null;
  const fits = ready ? withinBounds(bounds, ready.total) : null;

  const statedBounds = [
    bounds.maxTools != null && intl.formatMessage(i18n.boundTools, { n: bounds.maxTools }),
    bounds.maxBytes != null && intl.formatMessage(i18n.boundBytes, { n: bounds.maxBytes }),
    bounds.maxDepth != null && intl.formatMessage(i18n.boundDepth, { n: bounds.maxDepth }),
  ].filter((b): b is string => typeof b === 'string');

  const renderStated = () =>
    statedBounds.length > 0 ? (
      <div data-testid="tool-bounds-stated" className="flex flex-wrap items-center gap-1.5">
        <span className={cx(TYPE.body, WEIGHT.medium)}>{intl.formatMessage(i18n.boundsLabel)}</span>
        {statedBounds.map((text) => (
          <Chip key={text} className={TNUM}>
            {text}
          </Chip>
        ))}
      </div>
    ) : (
      <p className={TYPE.body}>{intl.formatMessage(i18n.noBounds)}</p>
    );

  const renderMeasureState = () => {
    if (!live || sizes.state === 'idle') return null;
    if (sizes.state === 'loading') {
      return (
        <p className={cx(TYPE.meta, 'flex items-center gap-1.5')}>
          <Loader2 className="size-4 animate-spin" />
          {intl.formatMessage(i18n.measuring)}
        </p>
      );
    }
    if (sizes.state === 'failed') {
      return (
        <p data-testid="tool-bounds-measure-failed" className={cx(TYPE.body, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.measureFailed, { error: sizes.error })}
        </p>
      );
    }
    return null;
  };

  const renderPlan = () => {
    if (!ready || !plan) return null;
    if (plan.kind === 'unreachable') {
      return (
        <p data-testid="tool-bounds-unreachable" className={cx(TYPE.body, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.unreachable)}
        </p>
      );
    }
    if (plan.kind !== 'fix') return null;
    const n = plan.remove.length;
    const names = plan.remove.map((e) => e.name);
    return (
      <div data-testid="tool-bounds-plan" className="flex flex-col gap-2">
        <p className={TYPE.body}>
          {bounds.maxBytes != null
            ? intl.formatMessage(i18n.planLeadBytes, {
                n,
                after: listEnvelopeBytes(plan.after.tools, plan.after.bytes),
                limit: bounds.maxBytes,
              })
            : intl.formatMessage(i18n.planLead, { n })}
        </p>
        <ul
          data-testid="tool-bounds-fix"
          className={cx('flex flex-col divide-y divide-lz-border border-y', SURFACE.hairline)}
        >
          {plan.remove.map((extension) => (
            <ExtensionRow
              key={extension.name}
              extension={extension}
              testIdPrefix="tool-bounds-fix"
              tooDeep={bounds.maxDepth != null && extension.depth > bounds.maxDepth}
              busy={busy}
              failure={failures[extension.name]}
              onTurnOff={(name) => void turnOff(name)}
            />
          ))}
        </ul>
        <div>
          <Button
            variant="primary"
            size="sm"
            icon={planInFlight ? <Loader2 className="animate-spin" /> : <PowerOff />}
            disabled={busy != null}
            data-testid="tool-bounds-plan-action"
            onClick={() => void turnOffPlanAndRetry(names)}
          >
            {intl.formatMessage(retryText != null ? i18n.planAndRetry : i18n.planOnly, { n })}
          </Button>
        </div>
      </div>
    );
  };

  const renderAll = () => {
    if (!ready) return null;
    const sending = ready.extensions.filter((e) => e.bytes > 0);
    const silent = ready.extensions.length - sending.length;
    const largest = Math.max(1, ...sending.map((e) => e.bytes), ready.unowned.bytes);
    return (
      <div className="flex flex-col gap-1.5">
        {ready.extensions.length === 0 ? (
          <p className={TYPE.body}>{intl.formatMessage(i18n.noExtensions)}</p>
        ) : (
          sending.length > 0 && (
            <Disclosure
              variant="plain"
              testId="tool-bounds-all"
              open={showAll}
              onOpenChange={setShowAll}
              title={intl.formatMessage(showAll ? i18n.hideAll : i18n.showAll, {
                n: sending.length,
              })}
            >
              <div className="flex flex-col gap-2">
                <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.summary)}</p>
                <ul
                  data-testid="tool-bounds-ranking"
                  className={cx(
                    'flex flex-col divide-y divide-lz-border border-y',
                    SURFACE.hairline
                  )}
                >
                  {sending.map((extension) => (
                    <ExtensionRow
                      key={extension.name}
                      extension={extension}
                      testIdPrefix="tool-bounds"
                      largest={largest}
                      tooDeep={bounds.maxDepth != null && extension.depth > bounds.maxDepth}
                      busy={busy}
                      failure={failures[extension.name]}
                      onTurnOff={(name) => void turnOff(name)}
                    />
                  ))}
                  {ready.unowned.tools > 0 && (
                    <li data-testid="tool-bounds-row-unowned" className="flex flex-col gap-1 py-2">
                      <span className={TYPE.body}>{intl.formatMessage(i18n.unowned)}</span>
                      <span className={cx(TYPE.meta, TNUM)}>
                        {intl.formatMessage(i18n.rowSize, {
                          tools: ready.unowned.tools,
                          bytes: ready.unowned.bytes,
                        })}
                      </span>
                    </li>
                  )}
                </ul>
                <p className={TYPE.meta}>{intl.formatMessage(i18n.sizesNote)}</p>
              </div>
            </Disclosure>
          )
        )}
        {silent > 0 && (
          <p data-testid="tool-bounds-no-tools" className={TYPE.meta}>
            {intl.formatMessage(i18n.noTools, { n: silent })}
          </p>
        )}
      </div>
    );
  };

  return (
    <div
      data-testid="tool-bounds-notice"
      className={cx(SURFACE.card, SPACE.card, 'flex flex-col gap-3')}
    >
      <div className="flex items-start gap-2.5">
        <Wrench className={cx('mt-0.5 size-5 shrink-0', TONE_TEXT.err)} />
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <h3 className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.title)}</h3>
          {ready && bounds.maxBytes != null ? (
            <MeasureLines bounds={bounds} total={ready.total} />
          ) : ready ? (
            <>
              <MeasureLines bounds={bounds} total={ready.total} />
              {renderStated()}
            </>
          ) : (
            <>
              {!live && <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.summary)}</p>}
              {renderStated()}
            </>
          )}
        </div>
      </div>

      {renderMeasureState()}
      {renderPlan()}

      {fits === true && (
        <p data-testid="tool-bounds-within" className={cx(TYPE.body, TONE_TEXT.ok)}>
          {intl.formatMessage(i18n.within)}
        </p>
      )}

      {renderAll()}

      <p data-testid="tool-bounds-raw" className={cx(TYPE.meta, 'break-words font-mono')}>
        {bounds.raw}
      </p>

      <div className="flex flex-wrap items-center gap-2">
        {live && retryText != null && (
          <Button
            variant={fits === true ? 'primary' : 'secondary'}
            size="sm"
            icon={<RotateCcw />}
            disabled={busy != null}
            data-testid="tool-bounds-retry"
            onClick={() => onRetry(retryText)}
          >
            {intl.formatMessage(i18n.retry)}
          </Button>
        )}
        <Button
          variant="secondary"
          size="sm"
          icon={<Blocks />}
          data-testid="tool-bounds-open-mcps"
          onClick={() => navigate('/extensions')}
        >
          {intl.formatMessage(i18n.openMcps)}
        </Button>
      </div>
    </div>
  );
}
