import { Minus, Plus, X } from 'lucide-react';
import { Button, TNUM, TONE_TEXT, TYPE, cx } from '../lz';
import { FieldRow } from '../modelFields/FieldRow';
import { INPUT, STEP_BUTTON } from './studio';
import { defineMessages, useIntl } from '../../i18n';

/**
 * goose-sidecar's `PREFIX_CACHE_SHARE_OF_FREE`: the share a profile that names none mounts with.
 * MlxPrefixCacheField.test.ts reads it out of engine.rs, so the two cannot drift apart.
 */
export const PREFIX_CACHE_SHARE_DEFAULT = 0.35;

const STEP = 0.05;

export type PrefixCacheShareDraft =
  | { kind: 'default' }
  | { kind: 'share'; share: number }
  | { kind: 'invalid' };

/** The backend's own range (engine.rs `prefix_cache_share`): a finite number, 0 < share < 1. */
export function readPrefixCacheShare(text: string): PrefixCacheShareDraft {
  const trimmed = text.trim();
  if (trimmed === '') return { kind: 'default' };
  const share = Number(trimmed);
  if (!Number.isFinite(share) || share <= 0 || share >= 1) return { kind: 'invalid' };
  return { kind: 'share', share };
}

const i18n = defineMessages({
  label: { id: 'mlxPrefixCache.label', defaultMessage: 'Prefix cache share' },
  decrease: { id: 'mlxPrefixCache.decrease', defaultMessage: 'Decrease prefix cache share' },
  increase: { id: 'mlxPrefixCache.increase', defaultMessage: 'Increase prefix cache share' },
  clear: { id: 'mlxPrefixCache.clear', defaultMessage: 'Clear' },
  clearTitle: {
    id: 'mlxPrefixCache.clearTitle',
    defaultMessage: "Clear — fall back to goose's default share",
  },
  defaultValue: { id: 'mlxPrefixCache.defaultValue', defaultMessage: 'default {percent}' },
  defaultNote: {
    id: 'mlxPrefixCache.defaultNote',
    defaultMessage: 'Blank = the default: {percent} of the memory free after the model loads.',
  },
  setNote: {
    id: 'mlxPrefixCache.setNote',
    defaultMessage:
      'This model caches in {percent} of the memory free after it loads (default {default}).',
  },
  explain: {
    id: 'mlxPrefixCache.explain',
    defaultMessage:
      'A long single session (a benchmark run) keeps its history cached only while one snapshot of it fits here; raising the share trades memory for re-use. A change restarts the engine: it applies at the next mount.',
  },
  invalid: {
    id: 'mlxPrefixCache.invalid',
    defaultMessage:
      'Enter a share between 0 and 1 (for example 0.45), or clear it for the default. Save stays off until then.',
  },
});

/**
 * The per-model prefix-cache share row of the profile form, in the same −/value/+ shape as the
 * sampling numbers: blank = goose's default share, otherwise a fraction of the memory free after the
 * weights load, refused here with the range the mount itself enforces.
 */
export function MlxPrefixCacheField({
  text,
  onText,
}: {
  text: string;
  onText: (v: string) => void;
}) {
  const intl = useIntl();
  const percent = (share: number) =>
    intl.formatNumber(share, { style: 'percent', maximumFractionDigits: 1 });
  const draft = readPrefixCacheShare(text);
  const label = intl.formatMessage(i18n.label);
  const bump = (dir: 1 | -1) => {
    const base = draft.kind === 'share' ? draft.share : PREFIX_CACHE_SHARE_DEFAULT;
    // Float steps accumulate representation noise (0.35 + 0.05 = 0.39999999999999997).
    const next = Number((base + dir * STEP).toFixed(4));
    if (next > 0 && next < 1) onText(String(next));
  };
  const state =
    draft.kind === 'share'
      ? intl.formatMessage(i18n.setNote, {
          percent: percent(draft.share),
          default: percent(PREFIX_CACHE_SHARE_DEFAULT),
        })
      : intl.formatMessage(i18n.defaultNote, { percent: percent(PREFIX_CACHE_SHARE_DEFAULT) });

  return (
    <FieldRow
      label={label}
      testId="mlx-prefix-cache"
      note={
        <span className="flex flex-col gap-1">
          {draft.kind === 'invalid' ? (
            <span
              className={cx('text-lz-meta', TONE_TEXT.err)}
              role="alert"
              data-testid="mlx-prefix-cache-invalid"
            >
              {intl.formatMessage(i18n.invalid)}
            </span>
          ) : (
            <span data-testid="mlx-prefix-cache-state">{state}</span>
          )}
          <span>{intl.formatMessage(i18n.explain)}</span>
        </span>
      }
    >
      <button
        type="button"
        className={STEP_BUTTON}
        aria-label={intl.formatMessage(i18n.decrease)}
        onClick={() => bump(-1)}
      >
        <Minus />
      </button>
      <input
        type="number"
        step={STEP}
        value={text}
        onChange={(e) => onText(e.target.value)}
        className={cx(INPUT, 'w-36 text-right', TNUM)}
        aria-label={label}
        aria-invalid={draft.kind === 'invalid'}
      />
      <button
        type="button"
        className={STEP_BUTTON}
        aria-label={intl.formatMessage(i18n.increase)}
        onClick={() => bump(1)}
      >
        <Plus />
      </button>
      {text.trim() !== '' ? (
        <Button
          size="sm"
          variant="ghost"
          icon={<X />}
          onClick={() => onText('')}
          title={intl.formatMessage(i18n.clearTitle)}
        >
          {intl.formatMessage(i18n.clear)}
        </Button>
      ) : (
        <span className={TYPE.meta} data-testid="mlx-prefix-cache-default">
          {intl.formatMessage(i18n.defaultValue, {
            percent: percent(PREFIX_CACHE_SHARE_DEFAULT),
          })}
        </span>
      )}
    </FieldRow>
  );
}
