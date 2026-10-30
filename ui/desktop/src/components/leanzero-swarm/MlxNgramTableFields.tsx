// The per-model "Keep n-gram table on disk" row (goose-sidecar `ple_offload`): offered only for a
// checkpoint that carries an n-gram (PLE) table, today Qwen3.8-Flash-Next.
import { Chip, Segmented, SURFACE, TYPE, cx, type SegmentedOption } from '../lz';
import type { MlxLocalModel } from '../../acp/mlx-engine';
import type { NumericDrafts, ProfileDraftKey } from './MlxEngineView';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  label: { id: 'mlxNgramTable.label', defaultMessage: 'Keep n-gram table on disk' },
  auto: { id: 'mlxNgramTable.auto', defaultMessage: 'Auto' },
  on: { id: 'mlxNgramTable.on', defaultMessage: 'On' },
  off: { id: 'mlxNgramTable.off', defaultMessage: 'Off' },
  autoOn: { id: 'mlxNgramTable.autoOn', defaultMessage: 'auto: on for this Mac' },
  autoOff: { id: 'mlxNgramTable.autoOff', defaultMessage: 'auto: off for this Mac' },
  note: {
    id: 'mlxNgramTable.note',
    defaultMessage:
      'The model reads the few rows of its 32 GB n-gram table each token needs from the SSD instead of holding the table in memory. Auto turns it on when the model would not otherwise fit this Mac. Applies at the next mount.',
  },
  measured: {
    id: 'mlxNgramTable.measured',
    defaultMessage:
      'Measured on Qwen3.8-Flash-Next (M5 Max 128 GB): 104 GB to 72 GB held, decode speed within a few percent, the same answers.',
  },
});

type ChoiceDraft = '' | 'true' | 'false';

export function MlxNgramTableFields({
  model,
  drafts,
  setDraft,
}: {
  model: MlxLocalModel | undefined;
  drafts: NumericDrafts;
  setDraft: (key: ProfileDraftKey, text: string) => void;
}) {
  const intl = useIntl();
  if (!model?.hasNgramTable) return null;
  const label = intl.formatMessage(i18n.label);
  const value =
    (['', 'true', 'false'] as const).find((v) => v === drafts.pleOffload.trim()) ?? '';
  const options: SegmentedOption<ChoiceDraft>[] = (['', 'true', 'false'] as const).map((v) => ({
    value: v,
    label: intl.formatMessage(v === '' ? i18n.auto : v === 'true' ? i18n.on : i18n.off),
    testId: `mlx-ngram-${v === '' ? 'auto' : v === 'true' ? 'on' : 'off'}`,
  }));
  return (
    <div
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-center gap-4 border-t py-2',
        SURFACE.hairline
      )}
    >
      <span className={cx('truncate', TYPE.body)}>{label}</span>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <Segmented<ChoiceDraft>
            aria-label={label}
            options={options}
            value={value}
            onChange={(v) => setDraft('pleOffload', v)}
          />
          {value === '' && (
            <Chip tone="accent">
              {intl.formatMessage(model.ngramTableAutoOn ? i18n.autoOn : i18n.autoOff)}
            </Chip>
          )}
        </div>
        <span className={TYPE.meta}>{intl.formatMessage(i18n.note)}</span>
        <span className={TYPE.meta}>{intl.formatMessage(i18n.measured)}</span>
      </div>
    </div>
  );
}
