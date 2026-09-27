import { useEffect } from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { toastSuccess } from '../../toasts';
import { ENGINE_GLANCE_TURNED_OFF_CHANNEL } from '../../utils/engineGlance';

const i18n = defineMessages({
  title: {
    id: 'engineGlance.turnedOff.title',
    defaultMessage: 'Floating window turned off',
  },
  way: {
    id: 'engineGlance.turnedOff.way',
    defaultMessage: 'Turn it back on in Settings › App.',
  },
});

/** Long enough to read the way back once; the toast pauses while the pointer is on it. */
const NOTICE_MS = 6000;

/**
 * The person turned the desktop engine window off from the window itself (Q-224): main tells the
 * goose window in front, and it says so with the way back, in the app's own toast.
 */
export function useGlanceTurnedOffNotice(): void {
  const intl = useIntl();
  useEffect(() => {
    const electron = window.electron;
    const onTurnedOff = () => {
      toastSuccess({
        title: intl.formatMessage(i18n.title),
        msg: intl.formatMessage(i18n.way),
        toastOptions: { autoClose: NOTICE_MS, toastId: ENGINE_GLANCE_TURNED_OFF_CHANNEL },
      });
    };
    electron.on(ENGINE_GLANCE_TURNED_OFF_CHANNEL, onTurnedOff);
    return () => electron.off(ENGINE_GLANCE_TURNED_OFF_CHANNEL, onTurnedOff);
  }, [intl]);
}
