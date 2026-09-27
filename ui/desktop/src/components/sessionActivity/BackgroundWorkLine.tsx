import { LoaderCircle } from 'lucide-react';
import { useIntl } from '../../i18n';
import { Chip } from '../lz';
import { backgroundWorkLabel } from './backgroundWorkText';
import { sessionStates, useActivityOf } from './sessionActivityStore';

/**
 * Under the finished reply, while goose still works for this chat with no turn running — the fact
 * check, a title (Q-185). The same state the session's row shows as its quiet pill and the Engine
 * card names, from the same store; it goes when the work ends, and a "goose check" finding (if the
 * check found one) lands in its place.
 */
export function BackgroundWorkLine({ sessionId }: { sessionId: string }) {
  const intl = useIntl();
  const activity = useActivityOf(sessionId);
  if (!activity.background || !sessionStates(activity).includes('background')) return null;
  return (
    <div
      data-testid="chat-background-work"
      data-work={activity.background}
      className="flex items-center gap-2 py-2 text-left"
    >
      <Chip tone="secondary" icon={<LoaderCircle className="animate-spin" />}>
        {backgroundWorkLabel(intl, activity.background)}
      </Chip>
    </div>
  );
}
