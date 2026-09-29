import { AppWindow } from 'lucide-react';
import { acpCancelPrompt } from '../../acp/prompt';
import { AppEvents } from '../../constants/events';
import { defineMessages, useIntl } from '../../i18n';
import { Button, TYPE, cx } from '../lz';
import { RunningPill } from '../sessionActivity/ActivityPills';
import { useActivityOf } from '../sessionActivity/sessionActivityStore';

const i18n = defineMessages({
  title: {
    id: 'turnElsewhere.title',
    defaultMessage: 'Running in another window',
  },
  detail: {
    id: 'turnElsewhere.detail',
    defaultMessage:
      'goose is answering this chat in another window. Stop ends it there; a message sent now waits until it finishes.',
  },
  show: {
    id: 'turnElsewhere.show',
    defaultMessage: 'Show that window',
  },
  stop: {
    id: 'turnElsewhere.stop',
    defaultMessage: 'Stop the turn running in another window',
  },
  backgroundTitle: {
    id: 'turnElsewhere.backgroundTitle',
    defaultMessage: 'Running in the background',
  },
  backgroundDetail: {
    id: 'turnElsewhere.backgroundDetail',
    defaultMessage:
      'goose is answering this chat outside any window — for an agent another chat started, or a linked Mac. Stop ends it; a message sent now waits until it finishes.',
  },
  backgroundStop: {
    id: 'turnElsewhere.backgroundStop',
    defaultMessage: 'Stop the turn goose is running in the background',
  },
});

export const turnElsewhereWords = i18n;

interface TurnElsewhereBridge {
  showTurnWindow?: (sessionId: string) => void;
  stopTurnElsewhere?: (sessionId: string) => void;
}

function bridge(): TurnElsewhereBridge | undefined {
  return (window as unknown as { electron?: TurnElsewhereBridge }).electron;
}

/**
 * A turn that runs on another window's connection, or — `window: null` — on goosed's process-wide
 * agents, which no window sent (Q-504: an orchestrator subagent's turn, a linked Mac's remote run).
 */
export interface TurnElsewhere {
  window: number | null;
  since: string;
}

/**
 * Stop this chat's turn where it runs. Another window's prompt is stopped by that window, which main
 * relays it to (Q-500). A turn no window holds is stopped through this window's own connection:
 * goosed's `session/cancel` reaches the process-wide agents for a turn no connection sent (Q-504).
 */
export function stopTurnElsewhere(sessionId: string, turn: TurnElsewhere): void {
  if (turn.window !== null) {
    bridge()?.stopTurnElsewhere?.(sessionId);
    return;
  }
  acpCancelPrompt(sessionId)
    .catch((error) => console.warn('Failed to stop the background turn:', error))
    .finally(() => window.dispatchEvent(new CustomEvent(AppEvents.SESSION_ACTIVITY_CHANGED)));
}

/**
 * This chat's turn runs on goosed but not in this window (Q-500, Q-504): on another window's
 * connection (the window that holds it) or on goosed's process-wide agents (`window: null`), else
 * null. The composer is busy then — Stop, and a send that visibly waits — exactly as in a window
 * whose own turn runs; nothing here streams the answer, so the bar says where it is.
 */
export function useTurnElsewhere(
  sessionId: string | null | undefined,
  localTurn: boolean
): TurnElsewhere | null {
  const { runningSince, turnWindow } = useActivityOf(sessionId ?? '');
  if (!sessionId || localTurn || runningSince === undefined) {
    return null;
  }
  return { window: turnWindow ?? null, since: runningSince };
}

export function TurnElsewhereBar({
  sessionId,
  turn,
}: {
  sessionId: string;
  turn: TurnElsewhere;
}) {
  const intl = useIntl();
  const background = turn.window === null;
  return (
    <div
      role="status"
      data-testid="turn-elsewhere"
      className="flex flex-wrap items-center gap-2 border-b border-lz-border pb-2 mb-2"
    >
      <RunningPill since={turn.since} />
      <span className={cx(TYPE.body, 'font-lz-semibold')}>
        {intl.formatMessage(background ? i18n.backgroundTitle : i18n.title)}
      </span>
      <span className={TYPE.meta}>
        {intl.formatMessage(background ? i18n.backgroundDetail : i18n.detail)}
      </span>
      {!background && (
        <Button
          size="sm"
          variant="secondary"
          icon={<AppWindow />}
          className="ml-auto"
          onClick={() => bridge()?.showTurnWindow?.(sessionId)}
        >
          {intl.formatMessage(i18n.show)}
        </Button>
      )}
    </div>
  );
}
