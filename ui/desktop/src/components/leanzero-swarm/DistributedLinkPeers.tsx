import { Network } from 'lucide-react';
import type {
  MlxDistributedLinkDiscoveryDto,
  MlxDistributedLinkPeerDto,
} from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx, type Tone } from '../lz';
import { gb1, gib } from './mlxDistributed';
import { ToneBanner } from './studio';

/**
 * Setup's FIRST offer: the same-account Macs on LeanZero Link, each as its own goosed described
 * itself over the mesh (name, memory, the Thunderbolt path, the RDMA device, the models it holds).
 * Picking one names it by its Link host (`link:<node>`) — nothing is typed. A Mac that did not
 * answer says why, in the backend's words (its owner's switch is off, it is offline, …).
 */

const i18n = defineMessages({
  title: { id: 'mlxDistributedSetup.link.title', defaultMessage: 'Your Macs on LeanZero Link' },
  intro: {
    id: 'mlxDistributedSetup.link.intro',
    defaultMessage:
      'Signed in to the same account. Each one described itself over the mesh; pick one and goose drives it through its own goose — no ssh.',
  },
  notConnected: {
    id: 'mlxDistributedSetup.link.notConnected',
    defaultMessage: 'LeanZero Link is not connected',
  },
  none: {
    id: 'mlxDistributedSetup.link.none',
    defaultMessage: 'No other Mac of this account is on LeanZero Link right now.',
  },
  use: { id: 'mlxDistributedSetup.link.use', defaultMessage: 'Use this Mac' },
  memory: {
    id: 'mlxDistributedSetup.link.memory',
    defaultMessage: '{available} of {total} GiB available',
  },
  noThunderbolt: {
    id: 'mlxDistributedSetup.link.noThunderbolt',
    defaultMessage: 'No Thunderbolt address',
  },
  rdmaActive: { id: 'mlxDistributedSetup.link.rdmaActive', defaultMessage: 'active' },
  rdmaDown: { id: 'mlxDistributedSetup.link.rdmaDown', defaultMessage: 'port down' },
  gid: { id: 'mlxDistributedSetup.link.gid', defaultMessage: 'IPv4 GID {index}' },
  noGid: { id: 'mlxDistributedSetup.link.noGid', defaultMessage: 'no IPv4 GID' },
  noRdma: { id: 'mlxDistributedSetup.link.noRdma', defaultMessage: 'No RDMA device' },
  models: {
    id: 'mlxDistributedSetup.link.models',
    defaultMessage: '{count, plural, one {# model} other {# models}}',
  },
  servingOff: {
    id: 'mlxDistributedSetup.link.servingOff',
    defaultMessage:
      'Turn on “Allow this Mac to serve as a distributed node” on {name} (Providers › LeanZero MLX › Engine › Distributed).',
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

function PeerCard({
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
  const thunderbolt = peer.thunderbolt ?? [];
  const rdma = peer.rdma ?? [];
  const models = peer.models ?? [];
  return (
    <div
      data-testid="mlx-dist-link-peer"
      data-host={peer.host}
      data-state={peer.state}
      className={cx('flex min-w-0 flex-col gap-2 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={TYPE.h2}>{name}</span>
        <Chip>{peer.hostname}</Chip>
        <Chip tone={stateTone(peer.state)}>{word ? intl.formatMessage(word) : peer.state}</Chip>
      </div>
      {ready && peer.availableBytes != null && peer.totalBytes != null && (
        <span className={cx(TYPE.body, TNUM)}>
          {intl.formatMessage(i18n.memory, {
            available: gb1(gib(peer.availableBytes)),
            total: gb1(gib(peer.totalBytes)),
          })}
        </span>
      )}
      {ready && (
        <span data-testid="mlx-dist-link-peer-tb" className={cx('break-all', TYPE.mono)}>
          {thunderbolt.length > 0
            ? thunderbolt
                .map((p) =>
                  [`${p.device} ${p.ipv4}/${p.prefixLen}`, p.hardwarePort, p.speed]
                    .filter(Boolean)
                    .join(' · ')
                )
                .join(', ')
            : intl.formatMessage(i18n.noThunderbolt)}
        </span>
      )}
      {ready && (
        <span data-testid="mlx-dist-link-peer-rdma" className={cx('break-all', TYPE.mono)}>
          {rdma.length > 0
            ? rdma
                .map((d) =>
                  [
                    d.device,
                    intl.formatMessage(d.active ? i18n.rdmaActive : i18n.rdmaDown),
                    d.ipv4GidIndex != null
                      ? intl.formatMessage(i18n.gid, { index: d.ipv4GidIndex })
                      : intl.formatMessage(i18n.noGid),
                  ].join(' · ')
                )
                .join(', ')
            : intl.formatMessage(i18n.noRdma)}
        </span>
      )}
      {ready && (
        <div className="flex flex-col gap-0.5">
          <span className={TYPE.meta}>
            {intl.formatMessage(i18n.models, { count: models.length })}
          </span>
          {models.map((m) => (
            <span key={m.dir} className={cx('break-all', TYPE.mono)}>
              {`${m.dir.split('/').pop() ?? m.dir} · ${m.modelType} · ${gb1(gib(m.weightsBytes))} GiB`}
            </span>
          ))}
        </div>
      )}
      {peer.state === 'servingDisabled' && (
        <span className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.warn)}>
          {intl.formatMessage(i18n.servingOff, { name })}
        </span>
      )}
      {!ready && peer.detail && (
        <span className={cx('break-words', TYPE.meta)}>{peer.detail}</span>
      )}
      {ready && (
        <div>
          <Button
            size="sm"
            variant={selected ? 'primary' : 'secondary'}
            icon={<Network />}
            onClick={() => onPick(peer.host)}
            disabled={disabled}
          >
            {intl.formatMessage(i18n.use)}
          </Button>
        </div>
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
      <div className="flex flex-col gap-0.5">
        <span className={TYPE.zone}>{intl.formatMessage(i18n.title)}</span>
        <span className={TYPE.bodyMuted}>{intl.formatMessage(i18n.intro)}</span>
      </div>
      {link.state !== 'connected' ? (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.notConnected)}
          text={link.detail ?? link.state}
        />
      ) : peers.length === 0 ? (
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.none)}</p>
      ) : (
        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          {peers.map((peer) => (
            <PeerCard
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
