import { useContext, useId, useState } from 'react';
import { Check, ChevronDown, Copy, Repeat } from 'lucide-react';
import { useIntl } from '../../i18n';
import type { LoopTickMetadata, Message } from '../../types/message';
import { getTextAndImageContent } from '../../types/message';
import { FOCUS, MOTION, SURFACE, TNUM, TONE_FILL, cx } from '../lz';
import { loopWords as w } from './loopWords';
import { cadenceWords, hm, tickMarkerDomId } from './loopView';
import { fmtTime } from './model';
import { LoopSessionContext } from './startLoopRequest';

/**
 * A tick's prompt in the transcript (§8.5): a full-width divider, not a user bubble — "⟳ Loop tick 5
 * · 22:40 · every 10 min" — with the exact text goose was sent one click away (the tick prompt's
 * transparency, §4.4). A yielded tick says on a second line why it stopped. Its only hover action is
 * Copy prompt: a marker is never edited, forked or looped.
 */
export function TickMarker({ message, tick }: { message: Message; tick: LoopTickMetadata }) {
  const intl = useIntl();
  const session = useContext(LoopSessionContext);
  const [showPrompt, setShowPrompt] = useState(false);
  const promptId = useId();
  const [copied, setCopied] = useState(false);
  const { textContent } = getTextAndImageContent(message);
  const time = hm(fmtTime(message.created * 1000)) ?? '';
  const loop = session?.loop?.id === tick.loopId ? session.loop : null;
  const cadence = loop ? cadenceWords(loop.cadence) : null;
  const record = loop?.ticks?.find((t) => t.n === tick.n);
  const yielded = record?.outcome?.kind === 'yielded' ? record.outcome : null;
  const title = cadence
    ? intl.formatMessage(w.markerTitleCadence, {
        n: tick.n,
        time,
        cadence: intl.formatMessage(cadence.message, cadence.values),
      })
    : intl.formatMessage(w.markerTitle, { n: tick.n, time });

  return (
    <div
      id={tickMarkerDomId(tick.messageId)}
      data-testid="loop-tick-marker"
      data-tick={tick.n}
      className="group w-full mt-[16px] scroll-mt-16"
    >
      <div className="flex items-center gap-3">
        <span aria-hidden className={cx('h-0 flex-1 border-t', SURFACE.hairline)} />
        <span
          className={cx(
            'inline-flex h-6 shrink-0 items-center gap-1.5 rounded-lz-pill px-2.5 text-xs font-lz-semibold',
            TONE_FILL.accent,
            TNUM
          )}
        >
          <Repeat aria-hidden className="size-3.5" />
          {title}
        </span>
        <span aria-hidden className={cx('h-0 flex-1 border-t', SURFACE.hairline)} />
        <button
          type="button"
          data-testid="loop-tick-marker-toggle"
          aria-expanded={showPrompt}
          aria-controls={promptId}
          onClick={() => setShowPrompt(!showPrompt)}
          className={cx(
            'inline-flex h-6 shrink-0 items-center gap-1 rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-xs font-lz-medium text-lz-ink hover:bg-lz-surface-2',
            FOCUS,
            MOTION
          )}
        >
          {intl.formatMessage(showPrompt ? w.markerHidePrompt : w.markerShowPrompt)}
          <ChevronDown aria-hidden className={cx('size-3.5', showPrompt && 'rotate-180')} />
        </button>
      </div>
      {yielded && record?.endedAt && (
        <p
          data-testid="loop-tick-marker-yielded"
          className="mt-1 text-center text-xs text-lz-ink-2"
        >
          {intl.formatMessage(w.markerYielded, {
            time: hm(record.endedAt) ?? '',
            chat: yielded.toChat,
          })}
        </p>
      )}
      {showPrompt && (
        <pre
          id={promptId}
          data-testid="loop-tick-prompt"
          className={cx(
            'mt-2 max-h-80 overflow-auto whitespace-pre-wrap rounded-lz-control border p-3 font-mono text-xs text-lz-ink bg-lz-surface-2',
            SURFACE.hairline
          )}
        >
          {textContent}
        </pre>
      )}
      <div className="mt-1 flex h-[22px] justify-end opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
        <button
          type="button"
          data-testid="loop-tick-copy-prompt"
          onClick={() =>
            void navigator.clipboard.writeText(textContent).then(() => setCopied(true))
          }
          onBlur={() => setCopied(false)}
          className={cx(
            'inline-flex items-center gap-1 rounded text-xs text-lz-ink-2 hover:text-lz-ink',
            FOCUS,
            MOTION
          )}
        >
          {copied ? (
            <Check aria-hidden className="size-3" />
          ) : (
            <Copy aria-hidden className="size-3" />
          )}
          {intl.formatMessage(copied ? w.markerCopied : w.markerCopyPrompt)}
        </button>
      </div>
    </div>
  );
}
