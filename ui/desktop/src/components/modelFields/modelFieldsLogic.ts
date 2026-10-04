import type { ModelFieldDto } from '@aaif/goose-sdk';
import type { ModelFieldValue, ModelFieldValues } from '../../acp/modelFields';

/** A select with this many choices or fewer (the default included) shows as a segmented strip;
 *  a longer one as a dropdown. Six overflowed the provider dialog's 560px card (Verbosity's
 *  Default·low·medium·high·xhigh·max, fixture screenshot 2026-10-04). */
export const SEGMENTED_MAX_CHOICES = 5;

export type NumberDraft =
  | { kind: 'clear' }
  | { kind: 'value'; value: number }
  | { kind: 'invalid'; reason: 'not-a-number' | 'not-whole' | 'below' | 'above' };

/** A number field's typed text: empty clears it back to the model's default; anything else must
 *  be a number inside the field's declared range (the engine checks again on save). */
export function parseNumberDraft(text: string, field: ModelFieldDto): NumberDraft {
  const trimmed = text.trim();
  if (trimmed === '') return { kind: 'clear' };
  const value = Number(trimmed);
  if (!Number.isFinite(value)) return { kind: 'invalid', reason: 'not-a-number' };
  if (field.kind.type !== 'number') return { kind: 'invalid', reason: 'not-a-number' };
  if (field.kind.integer && !Number.isInteger(value))
    return { kind: 'invalid', reason: 'not-whole' };
  if (field.kind.min != null && value < field.kind.min) return { kind: 'invalid', reason: 'below' };
  if (field.kind.max != null && value > field.kind.max) return { kind: 'invalid', reason: 'above' };
  return { kind: 'value', value };
}

/** `values` with `id` set, or removed when `value` is null (back to the model's default). */
export function withFieldValue(
  values: ModelFieldValues,
  id: string,
  value: ModelFieldValue | null
): ModelFieldValues {
  const next = { ...values };
  if (value == null) delete next[id];
  else next[id] = value;
  return next;
}

/** Effort first, then the other choices, then the numbers — the order a person reads them in. */
export function splitFields(fields: readonly ModelFieldDto[]): {
  choices: ModelFieldDto[];
  numbers: ModelFieldDto[];
} {
  return {
    choices: fields.filter((field) => field.kind.type === 'select'),
    numbers: fields.filter((field) => field.kind.type === 'number'),
  };
}
