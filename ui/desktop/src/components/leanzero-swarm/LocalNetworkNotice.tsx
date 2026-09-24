import { Settings } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../lz';
import { ToneBanner } from './studio';

/**
 * macOS local network privacy, named. The engine decides (preflight's `localNetworkPermission`
 * check, the supervisor's `localNetworkBlocked` event, a copy's `localNetworkBlocked` flag); this
 * view only says it in the reader's language and offers the one click that fixes it on THIS Mac.
 */

export const LOCAL_NETWORK_CHECK = 'localNetworkPermission';
export const LOCAL_NETWORK_EVENT = 'localNetworkBlocked';

const i18n = defineMessages({
  label: { id: 'localNetwork.label', defaultMessage: 'Local Network' },
  blocked: {
    id: 'localNetwork.blocked',
    defaultMessage:
      'macOS is blocking Goose Swarm from the local network — allow it in System Settings › Privacy & Security › Local Network',
  },
  blockedOn: {
    id: 'localNetwork.blockedOn',
    defaultMessage:
      'macOS on {host} is blocking Goose Swarm from the local network — allow it on {host} in System Settings › Privacy & Security › Local Network',
  },
  open: { id: 'localNetwork.open', defaultMessage: 'Open Privacy & Security' },
});

/**
 * Brings up the system's Local Network alert from the app's main process (see localNetwork.ts)
 * before a feature reaches another Mac: Detect, Preflight, Start, a model copy. The outcomes are
 * data for main; whether the network was refused is decided by the engine's named checks.
 */
export async function touchLocalNetwork(): Promise<void> {
  await window.electron.touchLocalNetwork();
}

/** `host` names another Mac: the fix is there, so no button opens THIS Mac's settings. */
export function LocalNetworkNotice({ host }: { host?: string }) {
  const intl = useIntl();
  return (
    <ToneBanner
      tone="err"
      testId="local-network-blocked"
      label={intl.formatMessage(i18n.label)}
      text={host ? intl.formatMessage(i18n.blockedOn, { host }) : intl.formatMessage(i18n.blocked)}
      action={
        host ? undefined : (
          <Button
            size="sm"
            variant="secondary"
            icon={<Settings />}
            data-testid="local-network-open-settings"
            onClick={() => void window.electron.openLocalNetworkSettings()}
          >
            {intl.formatMessage(i18n.open)}
          </Button>
        )
      }
    />
  );
}
