import type { ReactNode } from 'react';
import { Chip, Segmented, SURFACE, TYPE, cx, type SegmentedOption } from '../lz';
import type {
  MlxKvCacheFacts,
  MlxKvCacheMeasurement,
  MlxKvCacheMode,
  MlxLocalModel,
} from '../../acp/mlx-engine';
import type { NumericDrafts, ProfileDraftKey } from './MlxEngineView';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  kvCache: { id: 'mlxKvCache.kvCache', defaultMessage: 'KV cache' },
  off: { id: 'mlxKvCache.off', defaultMessage: 'Off (bf16)' },
  int8: { id: 'mlxKvCache.int8', defaultMessage: '8-bit' },
  int4: { id: 'mlxKvCache.int4', defaultMessage: '4-bit' },
  offNote: {
    id: 'mlxKvCache.offNote',
    defaultMessage: "Off: the engine's bf16 cache, {bytes} KiB per token of context.",
  },
  effect: {
    id: 'mlxKvCache.effect',
    defaultMessage:
      '{mode}: {bytes} KiB per token instead of {bf16} KiB — the same memory holds {ratio}× the context.',
  },
  measured: {
    id: 'mlxKvCache.measured',
    defaultMessage:
      'Measured on this model ({date}): {agreement}% token agreement with bf16, {identical} of {prompts} answers identical, the buried fact {retrieval}; bf16 against itself agreed {floor}%.',
  },
  measuredDecode: {
    id: 'mlxKvCache.measuredDecode',
    defaultMessage: 'Decode speed with {context} tokens of context: {ratio}% of bf16.',
  },
  retrievalFound: { id: 'mlxKvCache.retrievalFound', defaultMessage: 'found' },
  retrievalMissed: { id: 'mlxKvCache.retrievalMissed', defaultMessage: 'MISSED' },
  notMeasured: {
    id: 'mlxKvCache.notMeasured',
    defaultMessage:
      'Quality not measured on this model — compare its answers before relying on it.',
  },
  measurementUnreadable: {
    id: 'mlxKvCache.measurementUnreadable',
    defaultMessage: 'The measurement record cannot be read: {reason}',
  },
  scope: {
    id: 'mlxKvCache.scope',
    defaultMessage:
      'Only the {attention} full-attention layers grow with the context and are compressed; {state} linear-attention layers keep fixed-size state. Applies at the next mount. Distributed runs keep a bf16 cache.',
  },
  noGroup: {
    id: 'mlxKvCache.noGroup',
    defaultMessage:
      "This model's head size ({headDim}) fits none of the engine's quantization groups, so its KV cannot be compressed.",
  },
  noAttention: {
    id: 'mlxKvCache.noAttention',
    defaultMessage:
      'This model has no full-attention layers, so there is no growing KV to compress.',
  },
  unavailable: {
    id: 'mlxKvCache.unavailable',
    defaultMessage: 'KV cache compression unavailable: {reason}',
  },
  notLocal: {
    id: 'mlxKvCache.notLocal',
    defaultMessage:
      "KV cache compression needs the model's config.json, and this model is not in the models folder.",
  },
  measuredChip: {
    id: 'mlxKvCache.measuredChip',
    defaultMessage: '{agreement}% agreement',
  },
});

type KvDraft = '' | MlxKvCacheMode;

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

function Notice({ text }: { text: string }) {
  return (
    <div className={cx('border-t py-2', SURFACE.hairline)}>
      <span className={TYPE.meta} data-testid="mlx-kv-notice">
        {text}
      </span>
    </div>
  );
}

const kib = (bytes: number) => (bytes / 1024).toFixed(1).replace(/\.0$/, '');
const pct = (share: number) => (share * 100).toFixed(1);

export function kvBytesPerToken(facts: MlxKvCacheFacts, mode: KvDraft): number | null {
  if (mode === '') return facts.bf16BytesPerToken;
  return (mode === 'int8' ? facts.int8BytesPerToken : facts.int4BytesPerToken) ?? null;
}

function measurementFor(record: MlxKvCacheMeasurement | null | undefined, mode: MlxKvCacheMode) {
  return record ? (record[mode] ?? null) : null;
}

/**
 * The per-model KV-cache row of the profile form: off / 8-bit / 4-bit, and underneath, what the
 * choice buys on THIS model — bytes per token from its config.json, how much more context the
 * same memory holds, and the measured quality when a measurement record sits in its folder.
 */
export function MlxKvCacheFields({
  model,
  drafts,
  setDraft,
}: {
  model: MlxLocalModel | undefined;
  drafts: NumericDrafts;
  setDraft: (key: ProfileDraftKey, text: string) => void;
}) {
  const intl = useIntl();
  if (!model) return <Notice text={intl.formatMessage(i18n.notLocal)} />;
  const facts = model.kvCache;
  if (!facts) {
    return (
      <Notice
        text={intl.formatMessage(i18n.unavailable, {
          reason: model.kvCacheError ?? '—',
        })}
      />
    );
  }

  const blocker =
    facts.attentionLayers === 0
      ? intl.formatMessage(i18n.noAttention)
      : facts.groupSize == null
        ? intl.formatMessage(i18n.noGroup, { headDim: facts.headDim })
        : null;
  const value = (['', 'int8', 'int4'] as const).find((v) => v === drafts.kvCache.trim()) ?? '';
  const record = model.kvCacheMeasurement;
  const chip = (mode: MlxKvCacheMode) => {
    const m = measurementFor(record, mode);
    return m ? intl.formatMessage(i18n.measuredChip, { agreement: pct(m.agreement) }) : null;
  };
  const options: SegmentedOption<KvDraft>[] = [
    { value: '', label: intl.formatMessage(i18n.off), testId: 'mlx-kv-off' },
    ...(['int8', 'int4'] as const).map((mode) => ({
      value: mode,
      label: intl.formatMessage(i18n[mode]),
      testId: `mlx-kv-${mode}`,
      disabled: blocker != null && value !== mode,
      title: blocker ?? chip(mode) ?? undefined,
    })),
  ];

  const lines: string[] = [];
  if (value === '') {
    lines.push(intl.formatMessage(i18n.offNote, { bytes: kib(facts.bf16BytesPerToken) }));
  } else {
    const bytes = kvBytesPerToken(facts, value);
    if (bytes != null) {
      lines.push(
        intl.formatMessage(i18n.effect, {
          mode: intl.formatMessage(i18n[value]),
          bytes: kib(bytes),
          bf16: kib(facts.bf16BytesPerToken),
          ratio: (facts.bf16BytesPerToken / bytes).toFixed(1),
        })
      );
    }
    if (model.kvCacheMeasurementError) {
      lines.push(
        intl.formatMessage(i18n.measurementUnreadable, { reason: model.kvCacheMeasurementError })
      );
    } else {
      const measured = measurementFor(record, value);
      if (record && measured) {
        lines.push(
          intl.formatMessage(i18n.measured, {
            date: record.measuredAt,
            agreement: pct(measured.agreement),
            identical: measured.identicalAnswers,
            prompts: record.prompts,
            retrieval: intl.formatMessage(
              measured.retrievalFound ? i18n.retrievalFound : i18n.retrievalMissed
            ),
            floor: pct(record.noiseFloor.agreement),
          })
        );
        if (measured.decodeTpsRatio != null && measured.decodeContextTokens != null) {
          lines.push(
            intl.formatMessage(i18n.measuredDecode, {
              ratio: (measured.decodeTpsRatio * 100).toFixed(0),
              context: intl.formatNumber(measured.decodeContextTokens),
            })
          );
        }
      } else {
        lines.push(intl.formatMessage(i18n.notMeasured));
      }
    }
  }
  if (blocker) lines.push(blocker);
  lines.push(
    intl.formatMessage(i18n.scope, {
      attention: facts.attentionLayers,
      state: facts.stateLayers,
    })
  );

  const selectedChip = value === '' ? null : chip(value);
  return (
    <Row label={intl.formatMessage(i18n.kvCache)}>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <Segmented<KvDraft>
          aria-label={intl.formatMessage(i18n.kvCache)}
          options={options}
          value={value}
          onChange={(v) => setDraft('kvCache', v)}
        />
        {selectedChip && <Chip tone="accent">{selectedChip}</Chip>}
      </div>
      {lines.map((line) => (
        <span key={line} className={TYPE.meta} data-testid="mlx-kv-effect">
          {line}
        </span>
      ))}
    </Row>
  );
}
