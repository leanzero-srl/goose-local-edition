import { AppWindow } from 'lucide-react';
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
});

export const turnElsewhereWords = i18n;

interface TurnElsewhereBridge {
  showTurnWindow?: (sessionId: string) => void;
  stopTurnElsewhere?: (sessionId: string) => void;
}

function bridge(): TurnElsewhereBridge | undefined {
  return (window as unknown as { electron?: TurnElsewhereBridge }).electron;
}

/** Stop this chat's turn in the window whose connection runs it (main relays it there). */
export function stopTurnElsewhere(sessionId: string): void {
  bridge()?.stopTurnElsewhere?.(sessionId);
}

/**
 * This chat's turn runs on ANOTHER window's connection and none on this one (Q-500): the window that
 * holds it, else null. The composer is busy then — Stop, and a send that visibly waits — exactly as
 * in the window that sent it; nothing here streams the answer, so this bar says where it is.
 */
export function useTurnElsewhere(sessionId: string | null | undefined, localTurn: boolean) {
  const { runningSince, turnWindow } = useActivityOf(sessionId ?? '');
  if (!sessionId || localTurn || turnWindow === undefined || runningSince === undefined) {
    return null;
  }
  return { window: turnWindow, since: runningSince };
}

export function TurnElsewhereBar({ sessionId, since }: { sessionId: string; since: string }) {
  const intl = useIntl();
  return (
    <div
      role="status"
      data-testid="turn-elsewhere"
      className="flex flex-wrap items-center gap-2 border-b border-lz-border pb-2 mb-2"
    >
      <RunningPill since={since} />
      <span className={cx(TYPE.body, 'font-lz-semibold')}>{intl.formatMessage(i18n.title)}</span>
      <span className={TYPE.meta}>{intl.formatMessage(i18n.detail)}</span>
      <Button
        size="sm"
        variant="secondary"
        icon={<AppWindow />}
        className="ml-auto"
        onClick={() => bridge()?.showTurnWindow?.(sessionId)}
      >
        {intl.formatMessage(i18n.show)}
      </Button>
    </div>
  );
}
