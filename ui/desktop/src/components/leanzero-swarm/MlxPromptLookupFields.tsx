import type { ReactNode } from 'react';
import { Chip, Segmented, SURFACE, TYPE, cx, type SegmentedOption } from '../lz';
import type { MlxLocalModel } from '../../acp/mlx-engine';
import { NumericField, type NumericDrafts, type ProfileDraftKey } from './MlxEngineView';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  promptLookup: { id: 'mlxPromptLookup.promptLookup', defaultMessage: 'Prompt lookup (n-gram)' },
  auto: { id: 'mlxPromptLookup.auto', defaultMessage: 'Auto' },
  on: { id: 'mlxPromptLookup.on', defaultMessage: 'On' },
  off: { id: 'mlxPromptLookup.off', defaultMessage: 'Off' },
  laneMtp: { id: 'mlxPromptLookup.laneMtp', defaultMessage: 'inside MTP' },
  laneSuffix: { id: 'mlxPromptLookup.laneSuffix', defaultMessage: 'suffix decoding' },
  mtpNote: {
    id: 'mlxPromptLookup.mtpNote',
    defaultMessage:
      "Copies a continuation the prompt already contains as a long draft inside MTP, verified like any draft, so the output stays the model's own. Auto keeps the engine's family default (on for Qwen3.5/3.6/3.8).",
  },
  mtpMeasured: {
    id: 'mlxPromptLookup.mtpMeasured',
    defaultMessage:
      'Measured on Qwen3.8-27B Q8: code edits 42.8 to 134.8 tok/s with 99% of copies accepted; summaries and chat gain nothing.',
  },
  suffixNote: {
    id: 'mlxPromptLookup.suffixNote',
    defaultMessage:
      'Served without MTP, so the lever drives the engine suffix decoder: greedy sampling only, one request at a time. Auto leaves it off; measured slower on Mellum, so turn it on only after comparing.',
  },
  unavailable: {
    id: 'mlxPromptLookup.unavailable',
    defaultMessage: 'Unavailable for this model: {reason}',
  },
  hybridReason: {
    id: 'mlxPromptLookup.hybridReason',
    defaultMessage:
      'it mixes linear-attention or SSM layers with attention and is served without an MTP head. The engine refuses suffix decoding on recurrent layers, and prompt lookup only runs inside MTP.',
  },
  unknownReason: {
    id: 'mlxPromptLookup.unknownReason',
    defaultMessage: 'goose cannot tell whether this model is pure attention ({error}).',
  },
  notLocal: {
    id: 'mlxPromptLookup.notLocal',
    defaultMessage:
      "Prompt lookup needs the model's folder to pick its lane, and this model is not in the models folder.",
  },
  minNgram: { id: 'mlxPromptLookup.minNgram', defaultMessage: 'Shortest n-gram' },
  maxNgram: { id: 'mlxPromptLookup.maxNgram', defaultMessage: 'Longest n-gram' },
  maxDraft: { id: 'mlxPromptLookup.maxDraft', defaultMessage: 'Longest draft (tokens)' },
  draftHint: {
    id: 'mlxPromptLookup.draftHint',
    defaultMessage:
      'Blank numbers keep the engine default. Inside MTP, 31, 63 or 95 fill whole 32-row verify tiles. Applies at the next mount.',
  },
});

type ModeDraft = '' | 'on' | 'off';
export type PromptLookupLane =
  | { kind: 'mtp' }
  | { kind: 'suffix' }
  | { kind: 'unavailable'; reason: string };

/**
 * Mirrors `prompt_lookup::lane()` in goose-sidecar: MTP served (auto or `mtp` with a head in the
 * folder) -> inside MTP; otherwise a pure-attention model takes suffix decoding and a hybrid has
 * no lane. `speculative` is the profile draft ('' | 'mtp' | 'off').
 */
export function promptLookupLane(
  model: Pick<MlxLocalModel, 'hasMtpHead' | 'hybrid' | 'hybridError'>,
  speculative: string
): PromptLookupLane {
  const servesMtp = speculative.trim().toLowerCase() !== 'off' && model.hasMtpHead === true;
  if (servesMtp) return { kind: 'mtp' };
  if (model.hybrid === false) return { kind: 'suffix' };
  if (model.hybrid === true) return { kind: 'unavailable', reason: 'hybrid' };
  return { kind: 'unavailable', reason: model.hybridError ?? '—' };
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-center gap-4 border-t py-2',
        SURFACE.hairline
      )}
    >
      <span className={cx('truncate', TYPE.body)}>{label}</span>
      <div className="flex min-w-0 flex-col gap-1">{children}</div>
    </div>
  );
}

/**
 * The per-model prompt-lookup row: Auto / On / Off, the lane it drives on THIS model under the
 * profile's speculative setting, and the draft sizes. A hybrid served without MTP shows the
 * lever locked with the reason; Auto stays reachable so a stale choice can be cleared.
 */
export function MlxPromptLookupFields({
  model,
  drafts,
  setDraft,
}: {
  model: MlxLocalModel | undefined;
  drafts: NumericDrafts;
  setDraft: (key: ProfileDraftKey, text: string) => void;
}) {
  const intl = useIntl();
  const label = intl.formatMessage(i18n.promptLookup);
  if (!model) {
    return (
      <Row label={label}>
        <span className={TYPE.meta} data-testid="mlx-pl-note">
          {intl.formatMessage(i18n.notLocal)}
        </span>
      </Row>
    );
  }
  const lane = promptLookupLane(model, drafts.speculative);
  const value =
    (['', 'on', 'off'] as const).find((v) => v === drafts.promptLookupMode.trim()) ?? '';
  const locked = lane.kind === 'unavailable';
  const options: SegmentedOption<ModeDraft>[] = (['', 'on', 'off'] as const).map((mode) => ({
    value: mode,
    label: intl.formatMessage(mode === '' ? i18n.auto : i18n[mode]),
    testId: `mlx-pl-${mode || 'auto'}`,
    disabled: locked && mode !== '' && mode !== value,
  }));

  const notes: string[] = [];
  if (lane.kind === 'mtp') {
    notes.push(intl.formatMessage(i18n.mtpNote), intl.formatMessage(i18n.mtpMeasured));
  } else if (lane.kind === 'suffix') {
    notes.push(intl.formatMessage(i18n.suffixNote));
  } else {
    notes.push(
      intl.formatMessage(i18n.unavailable, {
        reason:
          lane.reason === 'hybrid'
            ? intl.formatMessage(i18n.hybridReason)
            : intl.formatMessage(i18n.unknownReason, { error: lane.reason }),
      })
    );
  }

  const numbers: { key: ProfileDraftKey; label: string; step: number }[] = [];
  if (!locked) {
    if (lane.kind === 'mtp') {
      numbers.push({
        key: 'promptLookupMinNgram',
        label: intl.formatMessage(i18n.minNgram),
        step: 1,
      });
    }
    numbers.push(
      { key: 'promptLookupMaxNgram', label: intl.formatMessage(i18n.maxNgram), step: 1 },
      { key: 'promptLookupMaxDraft', label: intl.formatMessage(i18n.maxDraft), step: 1 }
    );
  }

  return (
    <>
      <Row label={label}>
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Segmented<ModeDraft>
            aria-label={label}
            options={options}
            value={value}
            onChange={(v) => setDraft('promptLookupMode', v)}
          />
          {!locked && (
            <Chip tone="accent">
              {intl.formatMessage(lane.kind === 'mtp' ? i18n.laneMtp : i18n.laneSuffix)}
            </Chip>
          )}
        </div>
        {notes.map((line) => (
          <span key={line} className={TYPE.meta} data-testid="mlx-pl-note">
            {line}
          </span>
        ))}
      </Row>
      {numbers.map((spec) => (
        <NumericField
          key={spec.key}
          spec={{ ...spec, integer: true }}
          text={drafts[spec.key]}
          onText={(v) => setDraft(spec.key, v)}
        />
      ))}
      {numbers.length > 0 && (
        <span className={cx('pb-2', TYPE.meta)} data-testid="mlx-pl-hint">
          {intl.formatMessage(i18n.draftHint)}
        </span>
      )}
    </>
  );
}
