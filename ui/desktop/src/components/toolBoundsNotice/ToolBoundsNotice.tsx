import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Blocks, Loader2, PowerOff, RotateCcw, Wrench } from 'lucide-react';
import { AppEvents } from '../../constants/events';
import { defineMessages, useIntl } from '../../i18n';
import { errorMessage } from '../../utils/conversionUtils';
import { removeFromAgent } from '../settings/extensions/agent-api';
import { formatExtensionName } from '../settings/extensions/subcomponents/ExtensionList';
import { Button, Chip, SURFACE, SPACE, TNUM, TONE_DOT, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { listEnvelopeBytes, withinBounds, type ToolBounds } from './toolSchemaBounds';
import { useSessionToolSizes } from './useSessionToolSizes';

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
  sessionSends: { id: 'toolBoundsNotice.sessionSends', defaultMessage: 'This session sends' },
  totalTools: {
    id: 'toolBoundsNotice.totalTools',
    defaultMessage: '{n, plural, one {# tool} other {# tools}}',
  },
  totalBytes: { id: 'toolBoundsNotice.totalBytes', defaultMessage: '{n, number} bytes' },
  totalDepth: { id: 'toolBoundsNotice.totalDepth', defaultMessage: 'depth {n, number}' },
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

/**
 * The engine's "tool schema exceeds grammar-compile bounds" refusal, as a notice instead of raw
 * text: the bounds the engine STATED (parsed from its words, never assumed), the session's
 * extensions ranked by the size of the tools they send in the engine's own units, a per-extension
 * "Turn off for this session" (the same session toggle the chat's extension menu uses), Retry and
 * Open MCPs. The engine's words stay verbatim beneath.
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
  const [turningOff, setTurningOff] = useState<string | null>(null);
  const [failures, setFailures] = useState<Record<string, string>>({});

  const turnOff = async (name: string) => {
    setTurningOff(name);
    try {
      await removeFromAgent(name, sessionId, true);
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_EXTENSIONS_LOADED, { detail: { sessionId } })
      );
      setFailures(({ [name]: _gone, ...rest }) => rest);
      await reload();
    } catch (error) {
      setFailures((prev) => ({ ...prev, [name]: errorMessage(error) }));
    } finally {
      setTurningOff(null);
    }
  };

  const fits = sizes.state === 'ready' ? withinBounds(bounds, sizes.total) : null;
  const statedBounds = [
    bounds.maxTools != null && intl.formatMessage(i18n.boundTools, { n: bounds.maxTools }),
    bounds.maxBytes != null && intl.formatMessage(i18n.boundBytes, { n: bounds.maxBytes }),
    bounds.maxDepth != null && intl.formatMessage(i18n.boundDepth, { n: bounds.maxDepth }),
  ].filter((b): b is string => typeof b === 'string');

  const renderTotals = () => {
    if (sizes.state !== 'ready') return null;
    const { total } = sizes;
    const listBytes = listEnvelopeBytes(total.tools, total.bytes);
    const facts: Array<{ key: string; text: string; over: boolean | null }> = [
      {
        key: 'tools',
        text: intl.formatMessage(i18n.totalTools, { n: total.tools }),
        over: bounds.maxTools != null ? total.tools > bounds.maxTools : null,
      },
      {
        key: 'bytes',
        text: intl.formatMessage(i18n.totalBytes, { n: listBytes }),
        over: bounds.maxBytes != null ? listBytes > bounds.maxBytes : null,
      },
      {
        key: 'depth',
        text: intl.formatMessage(i18n.totalDepth, { n: total.depth }),
        over: bounds.maxDepth != null ? total.depth > bounds.maxDepth : null,
      },
    ];
    return (
      <div data-testid="tool-bounds-total" className="flex flex-wrap items-center gap-1.5">
        <span className={cx(TYPE.body, WEIGHT.medium)}>
          {intl.formatMessage(i18n.sessionSends)}
        </span>
        {facts.map((fact) => (
          <Chip
            key={fact.key}
            tone={fact.over == null ? undefined : fact.over ? 'err' : 'ok'}
            className={TNUM}
          >
            {fact.text}
          </Chip>
        ))}
      </div>
    );
  };

  const renderRanking = () => {
    if (sizes.state === 'idle') return null;
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
    const largest = Math.max(1, ...sizes.extensions.map((e) => e.bytes), sizes.unowned.bytes);
    return (
      <div className="flex flex-col gap-2">
        {renderTotals()}
        {sizes.extensions.length === 0 ? (
          <p className={TYPE.body}>{intl.formatMessage(i18n.noExtensions)}</p>
        ) : (
          <ul
            data-testid="tool-bounds-ranking"
            className={cx('flex flex-col divide-y divide-lz-border border-y', SURFACE.hairline)}
          >
            {sizes.extensions.map((extension) => {
              const tooDeep = bounds.maxDepth != null && extension.depth > bounds.maxDepth;
              return (
                <li
                  key={extension.name}
                  data-testid={`tool-bounds-row-${extension.name}`}
                  className="flex items-center gap-3 py-2"
                >
                  <div className="flex min-w-0 flex-1 flex-col gap-1">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className={cx(TYPE.body, WEIGHT.medium, 'truncate')}>
                        {formatExtensionName(extension.name)}
                      </span>
                      {tooDeep && (
                        <Chip tone="err">
                          {intl.formatMessage(i18n.rowTooDeep, { n: extension.depth })}
                        </Chip>
                      )}
                    </div>
                    <span className={cx(TYPE.meta, TNUM)}>
                      {intl.formatMessage(i18n.rowSize, {
                        tools: extension.tools,
                        bytes: extension.bytes,
                      })}
                    </span>
                    <div className="h-1.5 w-full rounded-full bg-lz-surface-2">
                      <div
                        className={cx('h-1.5 rounded-full', TONE_DOT.accent)}
                        style={{ width: `${(extension.bytes / largest) * 100}%` }}
                      />
                    </div>
                    {failures[extension.name] && (
                      <p className={cx(TYPE.meta, TONE_TEXT.err, 'break-words')}>
                        {intl.formatMessage(i18n.turnOffFailed, {
                          error: failures[extension.name],
                        })}
                      </p>
                    )}
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={
                      turningOff === extension.name ? (
                        <Loader2 className="animate-spin" />
                      ) : (
                        <PowerOff />
                      )
                    }
                    disabled={turningOff != null}
                    data-testid={`tool-bounds-turn-off-${extension.name}`}
                    onClick={() => void turnOff(extension.name)}
                  >
                    {intl.formatMessage(
                      turningOff === extension.name ? i18n.turningOff : i18n.turnOff
                    )}
                  </Button>
                </li>
              );
            })}
            {sizes.unowned.tools > 0 && (
              <li data-testid="tool-bounds-row-unowned" className="flex flex-col gap-1 py-2">
                <span className={TYPE.body}>{intl.formatMessage(i18n.unowned)}</span>
                <span className={cx(TYPE.meta, TNUM)}>
                  {intl.formatMessage(i18n.rowSize, {
                    tools: sizes.unowned.tools,
                    bytes: sizes.unowned.bytes,
                  })}
                </span>
              </li>
            )}
          </ul>
        )}
        <p className={TYPE.meta}>{intl.formatMessage(i18n.sizesNote)}</p>
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
        <div className="flex min-w-0 flex-col gap-0.5">
          <h3 className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.title)}</h3>
          <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.summary)}</p>
        </div>
      </div>

      {statedBounds.length > 0 ? (
        <div data-testid="tool-bounds-stated" className="flex flex-wrap items-center gap-1.5">
          <span className={cx(TYPE.body, WEIGHT.medium)}>
            {intl.formatMessage(i18n.boundsLabel)}
          </span>
          {statedBounds.map((text) => (
            <Chip key={text} className={TNUM}>
              {text}
            </Chip>
          ))}
        </div>
      ) : (
        <p className={TYPE.body}>{intl.formatMessage(i18n.noBounds)}</p>
      )}

      {live && renderRanking()}

      <p data-testid="tool-bounds-raw" className={cx(TYPE.meta, 'break-words font-mono')}>
        {bounds.raw}
      </p>

      {live && fits === true && (
        <p data-testid="tool-bounds-within" className={cx(TYPE.body, TONE_TEXT.ok)}>
          {intl.formatMessage(i18n.within)}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {live && retryText != null && (
          <Button
            variant={fits === true ? 'primary' : 'secondary'}
            size="sm"
            icon={<RotateCcw />}
            disabled={turningOff != null}
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
