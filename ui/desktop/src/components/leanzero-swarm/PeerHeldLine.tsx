import { useSyncExternalStore } from 'react';
import { X } from 'lucide-react';
import {
  latestMlxRemoteSingleStatus,
  remoteRouteUp,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { defineMessages, useIntl } from '../../i18n';
import { Button, TYPE, cx } from '../lz';
import { dismissPeerHeld, latestPeerHeld, subscribePeerHeld } from './routeSwitch';

const i18n = defineMessages({
  asking: {
    id: 'peerHeld.asking',
    defaultMessage: '{peer} still holds the model — goose asked it to free it',
  },
  held: {
    id: 'peerHeld.held',
    defaultMessage: '{peer} still holds the model — stop it on that Mac once it is back',
  },
  dismiss: { id: 'peerHeld.dismiss', defaultMessage: 'Dismiss' },
});

/**
 * The quiet line a switch off a route leaves when the linked Mac kept its model (routeSwitch.ts):
 * a secondary fact, not an error — chat already runs here. Gone once that Mac frees it, the route
 * to it is back up, or the user dismisses it. The peer's own words ride the title.
 */
export function PeerHeldLine() {
  const intl = useIntl();
  const held = useSyncExternalStore(subscribePeerHeld, latestPeerHeld);
  const route = useSyncExternalStore(subscribeMlxRemoteSingleStatus, latestMlxRemoteSingleStatus);
  if (!held) return null;
  if (remoteRouteUp(route) && route?.peer === held.peerNodeId) return null;
  return (
    <div
      role="status"
      data-testid="peer-held"
      data-phase={held.phase}
      title={held.phase === 'held' ? held.detail : undefined}
      className={cx('mb-2 flex items-center gap-2 px-1', TYPE.meta)}
    >
      <span className="min-w-0 flex-1 break-words">
        {intl.formatMessage(held.phase === 'asking' ? i18n.asking : i18n.held, {
          peer: held.peerName,
        })}
      </span>
      <Button
        size="sm"
        variant="ghost"
        icon={<X />}
        data-testid="peer-held-dismiss"
        onClick={dismissPeerHeld}
      >
        {intl.formatMessage(i18n.dismiss)}
      </Button>
    </div>
  );
}
