import { useState } from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { TYPE, cx } from '../lz';
import { acpUpsertConfig } from '../../acp/config';
import { ALLOW_DISTRIBUTED_NODE_KEY, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { hostingSummary } from './mlxDistributed';
import { formatMlxMode } from './mlxModeLabel';
import { mlxErrorMessage } from './mlxErrorMessage';
import { StudioSwitch, ToneBanner } from './studio';

/**
 * THIS Mac as a node of another Mac's distributed engine over LeanZero Link: the owner's switch
 * (off by default; the backend reads it on every request, so it applies at once) and, while a rank
 * is served here, what it is and for whom — the same sentence the tile and the tray say.
 */

const i18n = defineMessages({
  serve: {
    id: 'mlxDistributed.serveNode',
    defaultMessage: 'Allow this Mac to serve as a distributed node',
  },
  serveHint: {
    id: 'mlxDistributed.serveNodeHint',
    defaultMessage:
      "Your other Macs on LeanZero Link can then run a rank of their distributed engine here — goose's own rank programs and probes only, never an arbitrary command. Off by default.",
  },
  serveError: {
    id: 'mlxDistributed.serveNodeError',
    defaultMessage: 'Could not change the setting',
  },
  hostingLabel: { id: 'mlxDistributed.hosting.label', defaultMessage: 'This Mac serves a rank' },
  hostingText: {
    id: 'mlxDistributed.hosting.text',
    defaultMessage:
      '{line} — rank pid {pid}. The single engine and this Mac’s own distributed engine are refused until {requester} stops the run.',
  },
});

export function DistributedNodeServing({
  status,
  onChanged,
}: {
  status: MlxDistributedStatus;
  onChanged: () => void | Promise<void>;
}) {
  const intl = useIntl();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const hosting = hostingSummary(status);
  const allowed = status.allowDistributedNode ?? false;

  const toggle = async (next: boolean) => {
    setSaving(true);
    setError(null);
    try {
      await acpUpsertConfig(ALLOW_DISTRIBUTED_NODE_KEY, next);
    } catch (e) {
      setError(mlxErrorMessage(e, String(e)));
    } finally {
      setSaving(false);
      await onChanged();
    }
  };

  return (
    <div data-testid="mlx-dist-node-serving" className="flex flex-col gap-2">
      {hosting && (
        <ToneBanner
          tone="accent"
          live
          label={intl.formatMessage(i18n.hostingLabel)}
          text={intl.formatMessage(i18n.hostingText, {
            line: formatMlxMode(intl, hosting, null),
            pid: status.hosting?.pid ?? '—',
            requester: hosting.requester,
          })}
          testId="mlx-dist-hosting"
        />
      )}
      <span className="flex items-center gap-2">
        <StudioSwitch
          checked={allowed}
          onChange={(next) => void toggle(next)}
          aria-label={intl.formatMessage(i18n.serve)}
          disabled={saving}
        />
        <span className={cx(TYPE.body)}>{intl.formatMessage(i18n.serve)}</span>
      </span>
      <p className={TYPE.meta}>{intl.formatMessage(i18n.serveHint)}</p>
      {error && (
        <ToneBanner tone="err" label={intl.formatMessage(i18n.serveError)} text={error} />
      )}
    </div>
  );
}
