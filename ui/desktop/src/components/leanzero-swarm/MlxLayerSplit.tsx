import { Gauge } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import type { MlxDistributedPreflight } from '../../acp/mlx-distributed';
import {
  gbDecimal,
  layerSplitView,
  reservePercent,
  type LayerSplitView,
  type ReserveSource,
  type SplitRow,
} from './mlxDistributed';
import { ToneBanner } from './studio';

/**
 * The layer split goose plans itself for the mlx_lm pipeline runner (mimo_v2): which layers each
 * Mac holds, the reserve the plan kept free (and who set it), the planner's own sentence, and per
 * Mac its weights, budget and spare. Every figure is the backend's, in its decimal GB. On a
 * refusal that a settable reserve would fix, one press plans there.
 */

const i18n = defineMessages({
  title: { id: 'mlxDistributed.split.title', defaultMessage: 'Layer split' },
  layers: {
    id: 'mlxDistributed.split.layers',
    defaultMessage: '{split} layers',
  },
  reserve: {
    id: 'mlxDistributed.split.reserve',
    defaultMessage: 'reserve {pct}% of RAM',
  },
  sourceDefault: {
    id: 'mlxDistributed.split.sourceDefault',
    defaultMessage: 'goose’s default',
  },
  sourceModel: {
    id: 'mlxDistributed.split.sourceModel',
    defaultMessage: '{model}’s own reserve',
  },
  sourceModelUnnamed: {
    id: 'mlxDistributed.split.sourceModelUnnamed',
    defaultMessage: 'the model’s own reserve',
  },
  sourceUser: {
    id: 'mlxDistributed.split.sourceUser',
    defaultMessage: 'set by you',
  },
  margin: {
    id: 'mlxDistributed.split.margin',
    defaultMessage:
      'The safety margin is the reserve: goose sizes the context to fill each Mac’s budget, so spare reads about 0 GB by design.',
  },
  range: {
    id: 'mlxDistributed.split.range',
    defaultMessage: 'L{first}–{last} · {count} layers',
  },
  noRange: {
    id: 'mlxDistributed.split.noRange',
    defaultMessage: 'no layers planned',
  },
  rank: { id: 'mlxDistributed.split.rank', defaultMessage: 'rank {rank}' },
  weights: { id: 'mlxDistributed.split.weights', defaultMessage: 'weights' },
  budget: { id: 'mlxDistributed.split.budget', defaultMessage: 'budget' },
  spare: { id: 'mlxDistributed.split.spare', defaultMessage: 'spare' },
  gb: { id: 'mlxDistributed.split.gb', defaultMessage: '{value} GB' },
  fitsAtLabel: {
    id: 'mlxDistributed.split.fitsAtLabel',
    defaultMessage: 'Fits at {pct}%',
  },
  fitsAtText: {
    id: 'mlxDistributed.split.fitsAtText',
    defaultMessage:
      'No split fits at a reserve of {current}% of RAM; one fits at {pct}%. Planning there keeps less memory free on each Mac, still above the memory watchdog’s warning line.',
  },
  planAt: {
    id: 'mlxDistributed.split.planAt',
    defaultMessage: 'Plan at {pct}%',
  },
  noFitLabel: {
    id: 'mlxDistributed.split.noFitLabel',
    defaultMessage: 'No reserve fits',
  },
  noFitText: {
    id: 'mlxDistributed.split.noFitText',
    defaultMessage:
      'No reserve you can set makes a split fit: free memory on a Mac, or split a smaller model.',
  },
});

/** "Plan at 7.7%": provided by the section, which owns the config. */
export interface PlanAtControls {
  plan: (ratio: number) => void;
  disabled: boolean;
}

function sourceWord(
  intl: IntlShape,
  source: ReserveSource | null,
  modelType: string | null | undefined
): string | null {
  if (source === 'default') return intl.formatMessage(i18n.sourceDefault);
  if (source === 'user') return intl.formatMessage(i18n.sourceUser);
  if (source === 'model') {
    return modelType
      ? intl.formatMessage(i18n.sourceModel, { model: modelType })
      : intl.formatMessage(i18n.sourceModelUnnamed);
  }
  return null;
}

function Figure({ label, bytes, tone }: { label: string; bytes: number | null; tone?: string }) {
  const intl = useIntl();
  return (
    <span className="flex min-w-0 flex-col">
      <span className={TYPE.meta}>{label}</span>
      <span className={cx(TYPE.body, WEIGHT.semibold, TNUM, tone)}>
        {bytes == null ? '—' : intl.formatMessage(i18n.gb, { value: gbDecimal(bytes) })}
      </span>
    </span>
  );
}

function SplitRowView({ row }: { row: SplitRow }) {
  const intl = useIntl();
  const spareTone =
    row.spareBytes == null ? undefined : row.spareBytes < 0 ? TONE_TEXT.err : undefined;
  return (
    <li
      data-testid="mlx-dist-split-row"
      data-node={row.name}
      className={cx('grid grid-cols-2 gap-3 p-3 md:grid-cols-5', SURFACE.card)}
    >
      <span className="col-span-2 flex min-w-0 flex-col md:col-span-2">
        <span className="flex min-w-0 flex-wrap items-center gap-2">
          <span className={cx('min-w-0 truncate', TYPE.body, WEIGHT.semibold)}>{row.name}</span>
          <Chip>{intl.formatMessage(i18n.rank, { rank: row.rank })}</Chip>
        </span>
        <span data-testid="mlx-dist-split-range" className={cx(TYPE.mono, TNUM)}>
          {row.first != null && row.last != null && row.count != null
            ? intl.formatMessage(i18n.range, {
                first: row.first,
                last: row.last,
                count: row.count,
              })
            : intl.formatMessage(i18n.noRange)}
        </span>
      </span>
      <Figure label={intl.formatMessage(i18n.weights)} bytes={row.weightsBytes} />
      <Figure label={intl.formatMessage(i18n.budget)} bytes={row.budgetBytes} />
      <Figure label={intl.formatMessage(i18n.spare)} bytes={row.spareBytes} tone={spareTone} />
    </li>
  );
}

function RefusalBanner({ view, planAt }: { view: LayerSplitView; planAt?: PlanAtControls }) {
  const intl = useIntl();
  const fitting = view.fittingReserveRatio;
  if (fitting == null) {
    return (
      <ToneBanner
        tone="err"
        testId="mlx-dist-split-nofit"
        label={intl.formatMessage(i18n.noFitLabel)}
        text={intl.formatMessage(i18n.noFitText)}
      />
    );
  }
  const pct = reservePercent(fitting);
  return (
    <ToneBanner
      tone="warn"
      testId="mlx-dist-split-fits-at"
      label={intl.formatMessage(i18n.fitsAtLabel, { pct })}
      text={intl.formatMessage(i18n.fitsAtText, {
        pct,
        current: view.reserveRatio != null ? reservePercent(view.reserveRatio) : '—',
      })}
      action={
        planAt ? (
          <Button
            size="sm"
            variant="primary"
            icon={<Gauge />}
            onClick={() => planAt.plan(fitting)}
            disabled={planAt.disabled}
            data-testid="mlx-dist-plan-at"
          >
            {intl.formatMessage(i18n.planAt, { pct })}
          </Button>
        ) : undefined
      }
    />
  );
}

/** The layer split block of a preflight report; nothing for runners goose does not split itself. */
export function MlxLayerSplit({
  report,
  planAt,
}: {
  report: MlxDistributedPreflight;
  planAt?: PlanAtControls;
}) {
  const intl = useIntl();
  const view = layerSplitView(report);
  if (!view) return null;
  const source = sourceWord(intl, view.source, report.modelType);
  return (
    <div
      data-testid="mlx-dist-layer-split"
      data-refused={view.refused}
      className="flex flex-col gap-3"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={TYPE.zone}>{intl.formatMessage(i18n.title)}</span>
        {view.layers && view.layers.length > 0 && (
          <span data-testid="mlx-dist-split-layers">
            <Chip tone="accent">
              {intl.formatMessage(i18n.layers, { split: view.layers.join(' / ') })}
            </Chip>
          </span>
        )}
        {view.reserveRatio != null && (
          <span data-testid="mlx-dist-split-reserve">
            <Chip tone={view.source === 'user' ? 'secondary' : undefined}>
              {intl.formatMessage(i18n.reserve, { pct: reservePercent(view.reserveRatio) })}
            </Chip>
          </span>
        )}
        {source && (
          <span
            data-testid="mlx-dist-split-source"
            data-source={view.source ?? undefined}
            className={TYPE.meta}
          >
            {source}
          </span>
        )}
      </div>
      {view.refused ? (
        <RefusalBanner view={view} planAt={planAt} />
      ) : (
        <p className={TYPE.meta}>{intl.formatMessage(i18n.margin)}</p>
      )}
      {view.reason && (
        <p data-testid="mlx-dist-split-reason" className={cx('break-words', TYPE.body, TNUM)}>
          {view.reason}
        </p>
      )}
      {view.rows.some((r) => r.first != null) && (
        <ul className="flex flex-col gap-2">
          {view.rows.map((row) => (
            <SplitRowView key={`${row.rank}|${row.name}`} row={row} />
          ))}
        </ul>
      )}
    </div>
  );
}
