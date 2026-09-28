import { useEffect } from 'react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { restoreBrandTitle } from '../../contexts/EditionContext';
import { backgroundWorkShort } from './backgroundWorkText';
import { sessionStates, useActivityOf, type SessionActivity } from './sessionActivityStore';

/**
 * The window's title names the chat on screen and what it is doing (Q-318): "Jira Migration
 * Assessment — Running", read from the one session-activity store every session row reads, in the
 * sidebar's own words — so the Window menu, Mission Control and the app switcher say what the
 * sidebar says. Idle: the chat's name alone. No chat on screen: the brand title again.
 */
const i18n = defineMessages({
  withState: { id: 'windowTitle.withState', defaultMessage: '{session} — {state}' },
  running: { id: 'windowTitle.running', defaultMessage: 'Running' },
  needsYou: { id: 'sessionActivity.needsYou', defaultMessage: 'Needs you' },
  looping: { id: 'sessionActivity.looping', defaultMessage: 'Looping' },
  loopUnreadable: { id: 'sessionActivity.loopUnreadable', defaultMessage: 'Loop unreadable' },
  failed: { id: 'sessionActivity.failed', defaultMessage: 'Failed' },
  stopped: { id: 'sessionActivity.stopped', defaultMessage: 'Stopped' },
});

/** The most urgent state's word, as the session's pill says it; null when the session is idle. */
function stateWord(intl: IntlShape, activity: SessionActivity): string | null {
  switch (sessionStates(activity)[0]) {
    case 'needs-you':
      return intl.formatMessage(i18n.needsYou);
    case 'running':
      return intl.formatMessage(i18n.running);
    case 'background':
      return activity.background ? backgroundWorkShort(intl, activity.background) : null;
    case 'looping':
      return intl.formatMessage(
        activity.loopError !== undefined ? i18n.loopUnreadable : i18n.looping
      );
    case 'failed':
      return intl.formatMessage(i18n.failed);
    case 'stopped':
      return intl.formatMessage(i18n.stopped);
    case 'idle':
      return null;
  }
}

export function sessionWindowTitle(
  intl: IntlShape,
  session: string,
  activity: SessionActivity
): string {
  const state = stateWord(intl, activity);
  return state ? intl.formatMessage(i18n.withState, { session, state }) : session;
}

/** While `active` (this chat is the one on screen), the window's title is the chat's. */
export function useSessionWindowTitle(
  active: boolean,
  sessionId: string | undefined,
  name: string
): void {
  const intl = useIntl();
  const activity = useActivityOf(sessionId ?? '');
  const title = sessionId && name ? sessionWindowTitle(intl, name, activity) : null;
  useEffect(() => {
    if (!active || title == null) return;
    document.title = title;
    return restoreBrandTitle;
  }, [active, title]);
}
