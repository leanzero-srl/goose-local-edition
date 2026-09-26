import { useMemo, useState } from 'react';
import { Chip, Combobox, TONE_TEXT, TYPE, cx } from '../lz';
import type { MlxEngineSettings, MlxLocalModel } from '../../acp/mlx-engine';
import type { MlxServingIntent } from '../../acp/mlx-serving-intent';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { chatNodeOf, servedRepo, shortModelName } from '../noNodeNotice/mlxMount';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  pickAria: { id: 'nodeModelCell.pickAria', defaultMessage: 'Model for {node}' },
  servingNow: { id: 'nodeModelCell.servingNow', defaultMessage: 'serving now' },
  chatHere: { id: 'nodeModelCell.chatHere', defaultMessage: 'Chat: {model}' },
  chatFollows: {
    id: 'nodeModelCell.chatFollows',
    defaultMessage: 'Chat follows your Run: {model}',
  },
  chatFollowsTitle: {
    id: 'nodeModelCell.chatFollowsTitle',
    defaultMessage:
      'You started {served} on this Mac, so chat goes to it on this node. The node stays set to {set} for swarm builds and benchmarks.',
  },
  engineServes: { id: 'nodeModelCell.engineServes', defaultMessage: 'Engine serves {model}' },
  noModels: { id: 'nodeModelCell.noModels', defaultMessage: 'No model on this Mac matches.' },
  saveFailed: { id: 'nodeModelCell.saveFailed', defaultMessage: 'Not saved: {error}' },
});

/**
 * A local LeanZero MLX node's Model cell: what the node is set to — changeable from the models on
 * this Mac through the one node-model writer — and what this Mac's engine serves right now, with
 * whether chat goes to it on this node (the router's rule, `chatNodeOf`). A node that follows the
 * owner's own Run says so; its setting is never rewritten behind the user's back.
 */
export function NodeModelCell({
  device,
  devices,
  settings,
  intent,
  served,
  models,
  onPick,
}: {
  device: SwarmDeviceRow;
  devices: SwarmDeviceRow[];
  settings: MlxEngineSettings | null;
  intent: MlxServingIntent | null;
  /** The id this Mac's MLX engine answers under now; null = nothing answers. */
  served: string | null;
  models: readonly MlxLocalModel[];
  onPick: (modelId: string) => Promise<void>;
}) {
  const intl = useIntl();
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const chat =
    served != null && settings != null ? chatNodeOf(devices, settings, intent, served) : null;
  const options = useMemo(
    () =>
      models
        .filter((m) => m.complete)
        .map((m) => ({
          value: m.id,
          label: shortModelName(m.id),
          hint:
            served != null && settings != null && servedRepo(settings, served) === m.id
              ? intl.formatMessage(i18n.servingNow)
              : undefined,
        })),
    [models, served, settings, intl]
  );

  const pick = (modelId: string) => {
    if (modelId === device.model_id) return;
    setSaving(true);
    setError(null);
    onPick(modelId)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setSaving(false));
  };

  return (
    <div
      className="flex min-w-0 max-w-[36ch] flex-col gap-1"
      data-testid={`node-model-${device.id}`}
    >
      <Combobox
        options={options}
        value={device.model_id}
        onChange={pick}
        disabled={saving}
        aria-label={intl.formatMessage(i18n.pickAria, { node: device.id })}
        emptyText={intl.formatMessage(i18n.noModels)}
        className="font-mono text-lz-mono"
      />
      {served != null && chat?.nodeId === device.id && (
        <span data-testid={`node-model-chat-${device.id}`}>
          <Chip
            tone={chat.follows ? 'warn' : 'ok'}
            title={
              chat.follows
                ? intl.formatMessage(i18n.chatFollowsTitle, { served, set: device.model_id })
                : served
            }
          >
            {intl.formatMessage(chat.follows ? i18n.chatFollows : i18n.chatHere, {
              model: shortModelName(served),
            })}
          </Chip>
        </span>
      )}
      {served != null && chat?.nodeId !== device.id && (
        <span
          data-testid={`node-model-serves-${device.id}`}
          className={cx(TYPE.meta, 'truncate')}
          title={served}
        >
          {intl.formatMessage(i18n.engineServes, { model: shortModelName(served) })}
        </span>
      )}
      {error && (
        <span className={cx(TYPE.meta, TONE_TEXT.err, 'break-words')}>
          {intl.formatMessage(i18n.saveFailed, { error })}
        </span>
      )}
    </div>
  );
}
