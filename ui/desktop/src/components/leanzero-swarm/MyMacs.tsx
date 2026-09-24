import { useState } from 'react';
import { Link2Off, Loader2, LogOut, RefreshCw } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Checkbox,
  Chip,
  Disclosure,
  KeyValue,
  StatusDot,
  SURFACE,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import { acpUpsertConfig } from '../../acp/config';
import type { LinkState } from '../../acp/leanzero-link';
import { useFeatures } from '../../contexts/FeaturesContext';
import { StudioSwitch, ToneBanner } from './studio';
import { formatGb } from './primitives';
import { mlxErrorMessage } from './mlxErrorMessage';
import { useMlxDistributedStatus } from './useMlxDistributedStatus';
import { PERMISSIONS, PERMISSION_KEY, allowsOf, type Mac, type Permission } from './macs';
import { macLine, macStateWord, summarizeMac } from './macSummary';
import { PERMISSION_LABEL, useMacs, type MacFacts } from './useMacs';

/**
 * MY MACS — one card per Mac on LeanZero Link, each called by the ONE name its owner gave it, its
 * state in the engine-phase palette and one line of what it runs; the switches that let the other
 * Macs use THIS Mac on its own card. Replaces the Link tab's device table and the old "Manage on"
 * picker: a Mac is looked at here, and its models live in the Models table beside the others.
 */

const i18n = defineMessages({
  title: { id: 'myMacs.title', defaultMessage: 'My Macs' },
  intro: {
    id: 'myMacs.intro',
    defaultMessage:
      'Every Mac signed in to your LeanZero Link account. Load models on any of them from the Models tab, and pick where a model runs from the Engine tab.',
  },
  thisMac: { id: 'myMacs.thisMac', defaultMessage: 'This Mac' },
  memory: { id: 'myMacs.memory', defaultMessage: 'Memory' },
  memoryFree: { id: 'myMacs.memoryFree', defaultMessage: '{free} GB free of {total} GB' },
  disk: { id: 'myMacs.disk', defaultMessage: 'Disk' },
  diskFree: { id: 'myMacs.diskFree', defaultMessage: '{free} free of {total}' },
  models: { id: 'myMacs.models', defaultMessage: 'Models' },
  chip: { id: 'myMacs.chip', defaultMessage: 'Chip' },
  chipCores: { id: 'myMacs.chipCores', defaultMessage: '{brand} · {cores}-core GPU' },
  cantRead: { id: 'myMacs.cantRead', defaultMessage: 'Can’t read: {reason}' },
  lets: { id: 'myMacs.lets', defaultMessage: 'Lets your other Macs' },
  letsUnknown: {
    id: 'myMacs.letsUnknown',
    defaultMessage: 'Its goose does not say what it allows — update goose there.',
  },
  on: { id: 'myMacs.on', defaultMessage: 'on' },
  off: { id: 'myMacs.off', defaultMessage: 'off' },
  master: { id: 'myMacs.master', defaultMessage: 'Let my other Macs use this Mac' },
  masterHint: {
    id: 'myMacs.masterHint',
    defaultMessage:
      'Only Macs signed in to your LeanZero Link account. Each switch is goose’s own operations — never an arbitrary command.',
  },
  manageHint: {
    id: 'myMacs.manageHint',
    defaultMessage: 'Your other Macs see, download, copy and delete the models on this Mac.',
  },
  chatHint: {
    id: 'myMacs.chatHint',
    defaultMessage: 'A model loaded here answers chat started on your other Macs.',
  },
  splitHint: {
    id: 'myMacs.splitHint',
    defaultMessage: 'This Mac holds part of a model too big for one Mac, run from another.',
  },
  applyOnReconnect: {
    id: 'myMacs.applyOnReconnect',
    defaultMessage:
      '“Load and download models” takes effect when this Mac reconnects to LeanZero Link.',
  },
  reconnect: { id: 'myMacs.reconnect', defaultMessage: 'Reconnect now' },
  saveFailed: { id: 'myMacs.saveFailed', defaultMessage: 'The switch was not saved' },
  details: { id: 'myMacs.details', defaultMessage: 'Details' },
  account: { id: 'myMacs.account', defaultMessage: 'Account' },
  mesh: { id: 'myMacs.mesh', defaultMessage: 'Mesh' },
  meshLine: {
    id: 'myMacs.meshLine',
    defaultMessage:
      '{state} · {online, select, true {online} other {offline}} · {count, plural, one {# Mac} other {# Macs}}',
  },
  meshIp: { id: 'myMacs.meshIp', defaultMessage: 'Mesh IP' },
  hostname: { id: 'myMacs.hostname', defaultMessage: 'Hostname' },
  sessions: { id: 'myMacs.sessions', defaultMessage: 'Sessions active' },
  lastPoll: { id: 'myMacs.lastPoll', defaultMessage: 'Last read failed' },
  disconnect: { id: 'myMacs.disconnect', defaultMessage: 'Disconnect' },
  logout: { id: 'myMacs.logout', defaultMessage: 'Log out / Switch account' },
  reconnecting: {
    id: 'myMacs.reconnecting',
    defaultMessage: 'Reconnecting… (lost contact with this Mac’s goose)',
  },
  none: {
    id: 'myMacs.none',
    defaultMessage:
      'No other Mac is on your LeanZero Link account yet — sign in on it to see it here.',
  },
});

function chipText(
  intl: ReturnType<typeof useIntl>,
  facts: MacFacts
): { text: string; error: boolean } | null {
  const chip = facts.status?.chip;
  if (chip) {
    return {
      text:
        chip.gpuCores != null
          ? intl.formatMessage(i18n.chipCores, { brand: chip.brand, cores: chip.gpuCores })
          : chip.brand,
      error: false,
    };
  }
  if (facts.status?.chipError) return { text: facts.status.chipError, error: true };
  return null;
}

function FactsGrid({ mac, facts }: { mac: Mac; facts: MacFacts }) {
  const intl = useIntl();
  const { describeError } = useMacs();
  const status = facts.status;
  const cantRead = (reason: string) => (
    <span className={cx('break-words', WEIGHT.semibold, TONE_TEXT.err)}>
      {intl.formatMessage(i18n.cantRead, { reason: describeError(mac, reason) })}
    </span>
  );
  const chip = chipText(intl, facts);
  const items = [
    {
      key: 'memory',
      label: intl.formatMessage(i18n.memory),
      value: status?.memoryError
        ? cantRead(status.memoryError)
        : status
          ? intl.formatMessage(i18n.memoryFree, {
              free: status.availableMemoryGb.toFixed(1),
              total: status.totalMemoryGb.toFixed(0),
            })
          : facts.statusError
            ? cantRead(facts.statusError)
            : '—',
    },
    {
      key: 'disk',
      label: intl.formatMessage(i18n.disk),
      value: facts.disk
        ? intl.formatMessage(i18n.diskFree, {
            free: formatGb(facts.disk.availableBytes),
            total: formatGb(facts.disk.totalBytes),
          })
        : facts.modelsError
          ? cantRead(facts.modelsError)
          : '—',
    },
    {
      key: 'models',
      label: intl.formatMessage(i18n.models),
      value:
        facts.models != null && !facts.modelsError ? (
          <span data-testid={`my-mac-models-${mac.key}`}>{facts.models.length}</span>
        ) : facts.modelsError ? (
          cantRead(facts.modelsError)
        ) : (
          '—'
        ),
    },
    {
      key: 'chip',
      label: intl.formatMessage(i18n.chip),
      value: chip ? (chip.error ? cantRead(chip.text) : chip.text) : '—',
    },
  ];
  return <KeyValue items={items} aria-label={mac.name} />;
}

/** What a peer lets the other Macs do, as it reported it — one chip per switch. */
function PeerLets({ mac }: { mac: Mac }) {
  const intl = useIntl();
  if (!mac.allows) {
    return <p className={TYPE.meta}>{intl.formatMessage(i18n.letsUnknown)}</p>;
  }
  return (
    <div className="flex flex-wrap items-center gap-1.5" data-testid={`my-mac-lets-${mac.key}`}>
      <span className={TYPE.meta}>{intl.formatMessage(i18n.lets)}</span>
      {PERMISSIONS.map((p) => {
        const on = allowsOf(mac, p) === true;
        return (
          <Chip key={p} tone={on ? 'ok' : undefined}>
            {intl.formatMessage(PERMISSION_LABEL[p])} ·{' '}
            {intl.formatMessage(on ? i18n.on : i18n.off)}
          </Chip>
        );
      })}
    </div>
  );
}

interface SwitchState {
  manage: boolean;
  chat: boolean;
  split: boolean;
}

function switchesOf(linkState: LinkState | null): SwitchState {
  return {
    manage: linkState?.remoteExecutionAllowed ?? false,
    chat: linkState?.chatServingAllowed ?? false,
    split: linkState?.distributedNodeAllowed ?? false,
  };
}

/**
 * The ONE master switch and its three parts, each a goose config key its route reads. Off writes all
 * three off; on writes all three on, and each can then be unticked. The model-management switch is
 * built into the running mesh service, so a change to it applies at the next connect — said, with
 * the reconnect one click away.
 */
function LetOthersUseThisMac({
  linkState,
  onChanged,
  onReconnect,
  reconnecting,
}: {
  linkState: LinkState | null;
  onChanged: () => Promise<void>;
  onReconnect: () => void;
  reconnecting: boolean;
}) {
  const intl = useIntl();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [optimistic, setOptimistic] = useState<SwitchState | null>(null);
  const configured = switchesOf(linkState);
  const shown = optimistic ?? configured;
  const master = shown.manage || shown.chat || shown.split;

  const write = async (next: SwitchState) => {
    setSaving(true);
    setError(null);
    setOptimistic(next);
    try {
      for (const p of PERMISSIONS) {
        if (next[p] !== configured[p]) await acpUpsertConfig(PERMISSION_KEY[p], next[p]);
      }
    } catch (e) {
      setError(mlxErrorMessage(e, String(e)));
    } finally {
      await onChanged();
      setOptimistic(null);
      setSaving(false);
    }
  };

  const pendingManage =
    linkState?.auth.state === 'connected' &&
    linkState.remoteExecutionAllowedLive != null &&
    linkState.remoteExecutionAllowedLive !== configured.manage;
  const hint: Record<Permission, string> = {
    manage: intl.formatMessage(i18n.manageHint),
    chat: intl.formatMessage(i18n.chatHint),
    split: intl.formatMessage(i18n.splitHint),
  };

  return (
    <div data-testid="my-mac-permissions" className="flex flex-col gap-2">
      <span className="flex items-center gap-2">
        <StudioSwitch
          checked={master}
          disabled={saving}
          aria-label={intl.formatMessage(i18n.master)}
          onChange={(on) => void write({ manage: on, chat: on, split: on })}
        />
        <span className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.master)}</span>
      </span>
      <p className={TYPE.meta}>{intl.formatMessage(i18n.masterHint)}</p>
      <div className="flex flex-col gap-1 pl-11">
        {PERMISSIONS.map((p) => (
          <Checkbox
            key={p}
            testId={`my-mac-permission-${p}`}
            checked={shown[p]}
            disabled={saving || !master}
            label={intl.formatMessage(PERMISSION_LABEL[p])}
            description={hint[p]}
            onChange={(on) => void write({ ...shown, [p]: on })}
          />
        ))}
      </div>
      {pendingManage && (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(PERMISSION_LABEL.manage)}
          text={intl.formatMessage(i18n.applyOnReconnect)}
          testId="my-mac-apply-on-reconnect"
          action={
            <Button
              size="sm"
              variant="secondary"
              icon={reconnecting ? <Loader2 className="animate-spin" /> : <RefreshCw />}
              disabled={reconnecting}
              onClick={onReconnect}
            >
              {intl.formatMessage(i18n.reconnect)}
            </Button>
          }
        />
      )}
      {error && <ToneBanner tone="err" label={intl.formatMessage(i18n.saveFailed)} text={error} />}
    </div>
  );
}

interface MyMacsProps {
  email: string;
  linkState: LinkState;
  stale: boolean;
  disconnecting: boolean;
  reconnecting: boolean;
  onDisconnect: () => void;
  onLogout: () => void;
  onReconnect: () => void;
  onLinkChanged: () => Promise<void>;
}

function MacCard({ mac, props }: { mac: Mac; props: MyMacsProps }) {
  const intl = useIntl();
  const macsCtx = useMacs();
  const { mlxDistributed } = useFeatures();
  const distributed = useMlxDistributedStatus(mlxDistributed && mac.isSelf);
  const facts = macsCtx.factsOf(mac.key);
  const summary = summarizeMac(mac, {
    status: facts.status,
    statusError: facts.statusError,
    activity: facts.activity,
    decodeTps: facts.decodeTps,
    distributed: distributed.status,
  });
  const line = macLine(intl, summary);
  const word = macStateWord(intl, summary.state);
  const { linkState } = props;
  const details = mac.isSelf
    ? [
        { key: 'account', label: intl.formatMessage(i18n.account), value: props.email },
        {
          key: 'mesh',
          label: intl.formatMessage(i18n.mesh),
          value: intl.formatMessage(i18n.meshLine, {
            state: linkState.mesh?.backendState ?? '—',
            online: String(linkState.mesh?.online === true),
            count: linkState.nodeCount,
          }),
        },
        { key: 'ip', label: intl.formatMessage(i18n.meshIp), value: mac.meshIp ?? '—', mono: true },
        { key: 'host', label: intl.formatMessage(i18n.hostname), value: mac.hostname, mono: true },
        { key: 'sessions', label: intl.formatMessage(i18n.sessions), value: mac.sessionsActive },
      ]
    : [
        { key: 'ip', label: intl.formatMessage(i18n.meshIp), value: mac.meshIp ?? '—', mono: true },
        { key: 'host', label: intl.formatMessage(i18n.hostname), value: mac.hostname, mono: true },
        { key: 'sessions', label: intl.formatMessage(i18n.sessions), value: mac.sessionsActive },
        ...(mac.pollError
          ? [
              {
                key: 'poll',
                label: intl.formatMessage(i18n.lastPoll),
                value: mac.pollError,
                tone: 'err' as const,
              },
            ]
          : []),
      ];

  return (
    <section
      aria-label={mac.name}
      data-testid={`my-mac-${mac.key}`}
      data-state={summary.state}
      className={cx('flex min-w-0 flex-col gap-3 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={TYPE.h2} data-testid="my-mac-name">
          {mac.name}
        </span>
        {mac.isSelf && <Chip>{intl.formatMessage(i18n.thisMac)}</Chip>}
        {summary.phase ? (
          <Chip phase={summary.phase}>
            <span data-testid="my-mac-state" data-phase={summary.phase}>
              {word}
            </span>
          </Chip>
        ) : (
          <Chip tone={summary.state === 'off' ? 'warn' : 'stopped'}>
            <span data-testid="my-mac-state">{word}</span>
          </Chip>
        )}
      </div>
      {line && (
        <p data-testid="my-mac-line" className={cx(TYPE.body, TNUM, 'break-words')}>
          {line}
        </p>
      )}
      {summary.state === 'off' && (
        <p
          data-testid="my-mac-off"
          className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.warn)}
        >
          {macsCtx.offText(mac, 'manage')}
        </p>
      )}
      {summary.state === 'unreadable' && summary.detail && (
        <p
          data-testid="my-mac-unreadable"
          className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
        >
          {macsCtx.describeError(mac, summary.detail)}
        </p>
      )}
      {mac.online && summary.state !== 'off' && <FactsGrid mac={mac} facts={facts} />}
      {mac.isSelf ? (
        <LetOthersUseThisMac
          linkState={linkState}
          onChanged={props.onLinkChanged}
          onReconnect={props.onReconnect}
          reconnecting={props.reconnecting}
        />
      ) : (
        <PeerLets mac={mac} />
      )}
      <Disclosure variant="plain" title={intl.formatMessage(i18n.details)} testId="my-mac-details">
        <div className="flex flex-col gap-3">
          <KeyValue items={details} aria-label={intl.formatMessage(i18n.details)} />
          {mac.isSelf && (
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="secondary"
                data-testid="link-disconnect"
                disabled={props.disconnecting}
                onClick={props.onDisconnect}
                icon={props.disconnecting ? <Loader2 className="animate-spin" /> : <Link2Off />}
              >
                {intl.formatMessage(i18n.disconnect)}
              </Button>
              <Button
                size="sm"
                variant="secondary"
                data-testid="link-logout"
                onClick={props.onLogout}
                icon={<LogOut />}
              >
                {intl.formatMessage(i18n.logout)}
              </Button>
            </div>
          )}
        </div>
      </Disclosure>
    </section>
  );
}

export function MyMacs(props: MyMacsProps) {
  const intl = useIntl();
  const { macs } = useMacs();
  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="link-connected">
      {props.stale && (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.title)}
          text={intl.formatMessage(i18n.reconnecting)}
          live
          testId="link-reconnecting"
        />
      )}
      {props.linkState.lastError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.mesh)}
          text={props.linkState.lastError}
        />
      )}
      <div className="flex items-center gap-2">
        <StatusDot tone="ok" label={intl.formatMessage(i18n.mesh)} />
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.intro)}</p>
      </div>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2" data-testid="my-macs">
        {macs.map((mac) => (
          <MacCard key={mac.key} mac={mac} props={props} />
        ))}
      </div>
      {macs.length === 1 && (
        <p className={TYPE.bodyMuted} data-testid="link-peers-empty">
          {intl.formatMessage(i18n.none)}
        </p>
      )}
    </div>
  );
}
