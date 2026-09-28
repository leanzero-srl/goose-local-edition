import { Repeat } from 'lucide-react';
import { useIntl } from '../../i18n';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import { Button, FOCUS, MOTION, TNUM, TONE_FILL, cx } from '../lz';
import { composerLoopSlot } from './composerLoop';
import { requestOpenLoopRail, useEndedSeen } from './loopRailRequest';
import { statusTicks } from './loopView';
import { loopWords } from './loopWords';
import { requestStartLoop } from './startLoopRequest';
import { composerWords as cw } from './startLoopWords';
import { useNow } from './useNow';
import type { SessionLoop } from './useSessionLoop';

/**
 * The composer's Loop slot (DESIGN-SESSION-LOOPS §7.1, §8.1), where "Recipes & loops" was. With no
 * loop it is the Loop button, which opens the Start dialog; a swarm-build chat shows it disabled
 * with the refusal as its tooltip. With a loop it is the loop's status as a solid chip — the rail
 * pill's own words — that opens the rail on Loop. Its own component so the once-a-second clock of a
 * running tick re-renders only this chip, never the whole composer.
 */
export function ComposerLoopSlot({
  sessionId,
  state,
  swarmBuild,
}: {
  sessionId: string;
  state: SessionLoop;
  swarmBuild: boolean;
}) {
  const intl = useIntl();
  const loopId = state.kind === 'loop' ? state.loop.id : null;
  const endedSeen = useEndedSeen(loopId);
  const nowMs = useNow(state.kind === 'loop' && statusTicks(state.status));
  const slot = composerLoopSlot(state, { swarmBuild, endedSeen, nowMs });

  if (slot.kind === 'none') return null;

  if (slot.kind === 'button') {
    const title = intl.formatMessage(
      slot.swarmBuild ? cw.loopButtonSwarmBuild : cw.loopButtonTitle
    );
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex" data-testid="composer-loop-trigger">
            <Button
              variant="ghost"
              size="sm"
              icon={<Repeat />}
              disabled={slot.swarmBuild}
              aria-label={title}
              onClick={() => requestStartLoop({ sessionId, mode: 'start' })}
              data-testid="composer-loop-button"
            >
              {intl.formatMessage(cw.loopButton)}
            </Button>
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-xs">{title}</TooltipContent>
      </Tooltip>
    );
  }

  const text = intl.formatMessage(slot.view.label.message, slot.view.label.values);
  return (
    <button
      type="button"
      data-testid="composer-loop-chip"
      data-tone={slot.view.tone}
      aria-label={intl.formatMessage(loopWords.pillOpen, { label: text })}
      title={text}
      onClick={() => requestOpenLoopRail(sessionId)}
      className={cx(
        'inline-flex h-7 min-w-0 max-w-[16rem] shrink items-center gap-1.5 rounded-lz-pill px-2.5 text-[12px] font-lz-semibold hover:brightness-110',
        TONE_FILL[slot.view.tone],
        TNUM,
        FOCUS,
        MOTION
      )}
    >
      <Repeat aria-hidden className="size-3.5 shrink-0" />
      <span className="truncate">{text}</span>
    </button>
  );
}
