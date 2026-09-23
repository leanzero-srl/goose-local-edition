import type { ReactNode } from 'react';
import { Segmented, SURFACE, TYPE, cx, type SegmentedOption } from '../lz';
import type { MlxLocalModel } from '../../acp/mlx-engine';
import type { NumericDrafts, ProfileDraftKey } from './MlxEngineView';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  thinking: { id: 'mlxThinking.thinking', defaultMessage: 'Thinking' },
  auto: { id: 'mlxThinking.auto', defaultMessage: 'Auto' },
  on: { id: 'mlxThinking.on', defaultMessage: 'On' },
  off: { id: 'mlxThinking.off', defaultMessage: 'Off' },
  autoNote: {
    id: 'mlxThinking.autoNote',
    defaultMessage: 'Auto = the engine decides (off when tools are used)',
  },
  effort: { id: 'mlxThinking.effort', defaultMessage: 'Effort' },
  modelDefault: { id: 'mlxThinking.modelDefault', defaultMessage: 'Model default' },
  modelDefaultNamed: {
    id: 'mlxThinking.modelDefaultNamed',
    defaultMessage: 'Model default ({level})',
  },
  effortNote: {
    id: 'mlxThinking.effortNote',
    defaultMessage:
      "The levels come from this model's chat template. A session keeps the level it started with; a change applies to new sessions.",
  },
  effortIdle: {
    id: 'mlxThinking.effortIdle',
    defaultMessage: 'Thinking is off, so the effort level changes nothing.',
  },
  notInTemplate: {
    id: 'mlxThinking.notInTemplate',
    defaultMessage: '{level} (not in the template)',
  },
  noControls: {
    id: 'mlxThinking.noControls',
    defaultMessage: "This model's chat template declares no thinking controls.",
  },
  unavailable: {
    id: 'mlxThinking.unavailable',
    defaultMessage: 'Thinking controls unavailable: {reason}',
  },
  notReported: {
    id: 'mlxThinking.notReported',
    defaultMessage: "The engine did not report this model's thinking controls.",
  },
  notLocal: {
    id: 'mlxThinking.notLocal',
    defaultMessage:
      "Thinking controls need the model's chat template, and this model is not in the models folder.",
  },
});

type ThinkingDraft = '' | 'on' | 'off';

function Row({ label, note, children }: { label: string; note: string; children: ReactNode }) {
  return (
    <div
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-center gap-4 border-t py-2',
        SURFACE.hairline
      )}
    >
      <span className={cx('truncate', TYPE.body)}>{label}</span>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">{children}</div>
        <span className={TYPE.meta}>{note}</span>
      </div>
    </div>
  );
}

function Notice({ text }: { text: string }) {
  return (
    <div className={cx('border-t py-2', SURFACE.hairline)}>
      <span className={TYPE.meta} data-testid="mlx-thinking-notice">
        {text}
      </span>
    </div>
  );
}

/**
 * The per-model thinking rows of the profile form. Only what the model's chat template declares
 * is offered: the on/off switch when it has one, the effort strip when it validates its own
 * levels. Auto and the model default are the absence of a choice — nothing is sent.
 */
export function MlxThinkingFields({
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
  const capabilities = model.thinking;
  if (!capabilities) {
    return (
      <Notice
        text={
          model.thinkingError
            ? intl.formatMessage(i18n.unavailable, { reason: model.thinkingError })
            : intl.formatMessage(i18n.notReported)
        }
      />
    );
  }
  const hasSwitch = capabilities.thinkingSwitch != null;
  const levels = capabilities.effortLevels;
  if (!hasSwitch && levels.length === 0) {
    return <Notice text={intl.formatMessage(i18n.noControls)} />;
  }

  const thinking = (['', 'on', 'off'] as const).find((v) => v === drafts.thinking.trim()) ?? '';
  const thinkingOptions: SegmentedOption<ThinkingDraft>[] = [
    { value: '', label: intl.formatMessage(i18n.auto), testId: 'mlx-thinking-auto' },
    { value: 'on', label: intl.formatMessage(i18n.on), testId: 'mlx-thinking-on' },
    { value: 'off', label: intl.formatMessage(i18n.off), testId: 'mlx-thinking-off' },
  ];

  const effort = drafts.reasoningEffort.trim();
  const effortOptions: SegmentedOption<string>[] = [
    {
      value: '',
      label: capabilities.defaultEffort
        ? intl.formatMessage(i18n.modelDefaultNamed, { level: capabilities.defaultEffort })
        : intl.formatMessage(i18n.modelDefault),
      testId: 'mlx-effort-default',
    },
    ...levels.map((level) => ({ value: level, label: level, testId: `mlx-effort-${level}` })),
  ];
  if (effort !== '' && !levels.includes(effort)) {
    effortOptions.push({
      value: effort,
      label: intl.formatMessage(i18n.notInTemplate, { level: effort }),
    });
  }
  const effortIdle = hasSwitch && thinking === 'off';

  return (
    <>
      {hasSwitch && (
        <Row label={intl.formatMessage(i18n.thinking)} note={intl.formatMessage(i18n.autoNote)}>
          <Segmented<ThinkingDraft>
            aria-label={intl.formatMessage(i18n.thinking)}
            options={thinkingOptions}
            value={thinking}
            onChange={(v) => setDraft('thinking', v)}
          />
        </Row>
      )}
      {levels.length > 0 && (
        <Row
          label={intl.formatMessage(i18n.effort)}
          note={intl.formatMessage(effortIdle ? i18n.effortIdle : i18n.effortNote)}
        >
          <Segmented<string>
            aria-label={intl.formatMessage(i18n.effort)}
            options={effortOptions}
            value={effort}
            onChange={(v) => setDraft('reasoningEffort', v)}
            disabled={effortIdle}
          />
        </Row>
      )}
    </>
  );
}
