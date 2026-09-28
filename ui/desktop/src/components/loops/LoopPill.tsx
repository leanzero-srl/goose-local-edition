import { forwardRef } from 'react';
import { Repeat } from 'lucide-react';
import { useIntl } from '../../i18n';
import { FOCUS, MOTION, TNUM, TONE_FILL, cx, type Tone } from '../lz';
import { loopWords } from './loopWords';
import type { Words } from './loopView';

/**
 * The collapsed rail's loop pill (§8.3): a solid pill in the status's own fill with the status
 * label of §4.7. It sits first, beside the Changes pill, and opens the rail on the Loop tab.
 */
export const LoopPill = forwardRef<
  HTMLButtonElement,
  { tone: Tone; label: Words; panelId: string; onOpen: () => void }
>(function LoopPill({ tone, label, panelId, onOpen }, ref) {
  const intl = useIntl();
  const text = intl.formatMessage(label.message, label.values);
  return (
    <button
      ref={ref}
      type="button"
      data-testid="loop-rail-pill"
      data-tone={tone}
      aria-expanded={false}
      aria-controls={panelId}
      aria-label={intl.formatMessage(loopWords.pillOpen, { label: text })}
      title={text}
      onClick={onOpen}
      className={cx(
        'pointer-events-auto inline-flex h-8 min-w-0 max-w-[calc(100vw-2rem)] items-center gap-2 rounded-lz-pill px-3 text-xs font-lz-semibold shadow-lz-overlay dark:shadow-lz-overlay-dark hover:brightness-110',
        TONE_FILL[tone],
        TNUM,
        FOCUS,
        MOTION
      )}
    >
      <Repeat aria-hidden className="size-4 shrink-0" />
      <span className="truncate">{text}</span>
    </button>
  );
});
