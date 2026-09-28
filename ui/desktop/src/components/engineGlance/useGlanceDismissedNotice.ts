import { useEffect } from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { toastSuccess } from '../../toasts';
import { ENGINE_GLANCE_DISMISSED_CHANNEL, isGlanceDismissedNotice } from '../../utils/engineGlance';

const i18n = defineMessages({
  title: {
    id: 'engineGlance.dismissed.title',
    defaultMessage: 'Floating window hidden for this session',
  },
  wayTray: {
    id: 'engineGlance.dismissed.wayTray',
    defaultMessage:
      'Bring it back from the goose icon in the menu bar or Settings › App, where you can also turn it off.',
  },
  way: {
    id: 'engineGlance.dismissed.way',
    defaultMessage: 'Bring it back from Settings › App, where you can also turn it off.',
  },
});

/** Long enough to read the way back once; the toast pauses while the pointer is on it. */
const NOTICE_MS = 8000;

/**
 * The person closed the desktop engine window from its X (Q-426): main tells the goose window in
 * front, once a session, and it says so with the ways back, in the app's own toast. The menu-bar
 * icon is named only while there is one (Settings › App can turn it off).
 */
export function useGlanceDismissedNotice(): void {
  const intl = useIntl();
  useEffect(() => {
    const electron = window.electron;
    const onDismissed = (_event: unknown, notice: unknown) => {
      const tray = isGlanceDismissedNotice(notice) && notice.tray;
      toastSuccess({
        title: intl.formatMessage(i18n.title),
        msg: intl.formatMessage(tray ? i18n.wayTray : i18n.way),
        toastOptions: { autoClose: NOTICE_MS, toastId: ENGINE_GLANCE_DISMISSED_CHANNEL },
      });
    };
    electron.on(ENGINE_GLANCE_DISMISSED_CHANNEL, onDismissed);
    return () => electron.off(ENGINE_GLANCE_DISMISSED_CHANNEL, onDismissed);
  }, [intl]);
}
