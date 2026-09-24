import { Network } from 'lucide-react';
import type { MlxDistributedLinkDiscoveryDto, MlxDistributedLinkPeerDto } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, TONE_TEXT, TYPE, WEIGHT, cx, type Tone } from '../lz';
import { ToneBanner } from './studio';

/**
 * Setup's default: the same-account Macs on LeanZero Link, one row each. Picking one names it by
 * its Link host (`link:<node>`) — nothing is typed. A Mac that did not answer says why, in the
 * backend's words (its owner's switch is off, it is offline, …).
 */

const i18n = defineMessages({
  title: { id: 'mlxDistributedSetup.link.pick', defaultMessage: 'The other Mac' },
  notConnected: {
    id: 'mlxDistributedSetup.link.notConnected',
    defaultMessage: 'LeanZero Link is not connected',
  },
  none: {
    id: 'mlxDistributedSetup.link.none',
    defaultMessage: 'No other Mac of this account is on LeanZero Link right now.',
  },
  use: { id: 'mlxDistributedSetup.link.detectWith', defaultMessage: 'Detect with this Mac' },
  servingOff: {
    id: 'mlxDistributedSetup.link.servingOffMyMacs',
    defaultMessage:
      'Turn on “Let my other Macs use this Mac › Run part of a split model” on {name} (Providers › My Macs there).',
  },
  stateReady: { id: 'mlxDistributedSetup.link.state.ready', defaultMessage: 'Ready' },
  stateServingDisabled: {
    id: 'mlxDistributedSetup.link.state.servingDisabled',
    defaultMessage: 'Serving is off there',
  },
  stateNotServed: {
    id: 'mlxDistributedSetup.link.state.notServed',
    defaultMessage: 'Its goose cannot serve',
  },
  stateOffline: { id: 'mlxDistributedSetup.link.state.offline', defaultMessage: 'Offline' },
  stateUnreachable: {
    id: 'mlxDistributedSetup.link.state.unreachable',
    defaultMessage: 'Unreachable',
  },
  stateUnreadable: {
    id: 'mlxDistributedSetup.link.state.unreadable',
    defaultMessage: 'Answered, unreadable',
  },
});

const STATE_WORD: Record<string, (typeof i18n)['stateReady']> = {
  ready: i18n.stateReady,
  servingDisabled: i18n.stateServingDisabled,
  notServed: i18n.stateNotServed,
  offline: i18n.stateOffline,
  unreachable: i18n.stateUnreachable,
  unreadable: i18n.stateUnreadable,
};

function stateTone(state: string): Tone {
  if (state === 'ready') return 'ok';
  if (state === 'servingDisabled' || state === 'offline') return 'warn';
  return 'err';
}

/**
 * One Mac to pick, on one row: its name, whether it can take part, and the pick. What it holds —
 * memory, chip, models — is on My Macs and in the preflight, never repeated here.
 */
function PeerRow({
  peer,
  selected,
  disabled,
  onPick,
}: {
  peer: MlxDistributedLinkPeerDto;
  selected: boolean;
  disabled: boolean;
  onPick: (host: string) => void;
}) {
  const intl = useIntl();
  const name = peer.name ?? peer.hostname;
  const ready = peer.state === 'ready';
  const word = STATE_WORD[peer.state];
  return (
    <div
      data-testid="mlx-dist-link-peer"
      data-host={peer.host}
      data-state={peer.state}
      className="flex min-w-0 flex-wrap items-center gap-2"
    >
      <span className={cx(TYPE.body, WEIGHT.semibold)}>{name}</span>
      <Chip tone={stateTone(peer.state)}>{word ? intl.formatMessage(word) : peer.state}</Chip>
      {ready && (
        <Button
          size="sm"
          variant={selected ? 'primary' : 'secondary'}
          icon={<Network />}
          onClick={() => onPick(peer.host)}
          disabled={disabled}
        >
          {intl.formatMessage(i18n.use)}
        </Button>
      )}
      {peer.state === 'servingDisabled' && (
        <span className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.warn)}>
          {intl.formatMessage(i18n.servingOff, { name })}
        </span>
      )}
      {!ready && peer.state !== 'servingDisabled' && peer.detail && (
        <span className={cx('break-words', TYPE.meta)}>{peer.detail}</span>
      )}
    </div>
  );
}

export function DistributedLinkPeers({
  link,
  selectedHost,
  disabled,
  onPick,
}: {
  link: MlxDistributedLinkDiscoveryDto;
  selectedHost: string;
  disabled: boolean;
  onPick: (host: string) => void;
}) {
  const intl = useIntl();
  const peers = link.peers ?? [];
  return (
    <div data-testid="mlx-dist-link-peers" data-state={link.state} className="flex flex-col gap-2">
      <span className={TYPE.zone}>{intl.formatMessage(i18n.title)}</span>
      {link.state !== 'connected' ? (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.notConnected)}
          text={link.detail ?? link.state}
        />
      ) : peers.length === 0 ? (
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.none)}</p>
      ) : (
        <div className="flex flex-col gap-2">
          {peers.map((peer) => (
            <PeerRow
              key={peer.host}
              peer={peer}
              selected={selectedHost === peer.host}
              disabled={disabled}
              onPick={onPick}
            />
          ))}
        </div>
      )}
    </div>
  );
}
