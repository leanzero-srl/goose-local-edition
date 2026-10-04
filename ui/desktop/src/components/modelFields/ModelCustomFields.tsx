import { useEffect, useRef, useState } from 'react';
import { ChevronDown, Loader2 } from 'lucide-react';
import type { ModelFieldDto } from '@aaif/goose-sdk';
import {
  acpListModelFields,
  acpSaveModelFields,
  type ModelFieldsListing,
  type ModelFieldValue,
  type ModelFieldValues,
} from '../../acp/modelFields';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { Disclosure, FOCUS, MOTION, RADIUS, SURFACE, Segmented, TYPE, cx } from '../lz';
import { INPUT } from '../leanzero-swarm/studio';
import { defineMessages, useIntl } from '../../i18n';
import { FieldNotice, FieldRow } from './FieldRow';
import {
  SEGMENTED_MAX_CHOICES,
  parseNumberDraft,
  splitFields,
  withFieldValue,
  type NumberDraft,
} from './modelFieldsLogic';

const i18n = defineMessages({
  heading: { id: 'modelCustomFields.heading', defaultMessage: 'Model settings' },
  scope: {
    id: 'modelCustomFields.scope',
    defaultMessage:
      'Saved for this model. New chats and benchmark runs use them; an open chat keeps the values it started with.',
  },
  default: { id: 'modelCustomFields.default', defaultMessage: 'Default' },
  modelDefault: {
    id: 'modelCustomFields.modelDefault',
    defaultMessage: 'Model default ({value})',
  },
  numberPlaceholder: { id: 'modelCustomFields.numberPlaceholder', defaultMessage: 'default' },
  sampling: { id: 'modelCustomFields.sampling', defaultMessage: 'Sampling ({count})' },
  loading: { id: 'modelCustomFields.loading', defaultMessage: 'Reading this model’s settings…' },
  saving: { id: 'modelCustomFields.saving', defaultMessage: 'Saving…' },
  loadFailed: {
    id: 'modelCustomFields.loadFailed',
    defaultMessage: 'This model’s settings could not be read: {error}',
  },
  saveFailed: {
    id: 'modelCustomFields.saveFailed',
    defaultMessage: 'Not saved: {error}',
  },
  unlisted: {
    id: 'modelCustomFields.unlisted',
    defaultMessage:
      'The provider’s model listing has no model “{model}”, so it declares no settings for it. Check the model ID.',
  },
  pinned: { id: 'modelCustomFields.pinned', defaultMessage: 'Pinned for this run: {note}' },
  notANumber: { id: 'modelCustomFields.notANumber', defaultMessage: 'Enter a number.' },
  notWhole: { id: 'modelCustomFields.notWhole', defaultMessage: 'Enter a whole number.' },
  below: { id: 'modelCustomFields.below', defaultMessage: 'The lowest value is {min}.' },
  above: { id: 'modelCustomFields.above', defaultMessage: 'The highest value is {max}.' },
});

/** The wait after the last keystroke in a typed model ID before its fields are read. */
const LOOKUP_SETTLE_MS = 400;

type Load =
  | { state: 'idle' }
  | { state: 'loading' }
  | { state: 'ready'; listing: ModelFieldsListing }
  | { state: 'error'; error: string };

function errorText(err: unknown): string {
  if (err && typeof err === 'object' && 'data' in err && typeof err.data === 'string')
    return err.data;
  return err instanceof Error ? err.message : String(err);
}

function defaultLabel(field: ModelFieldDto, intl: ReturnType<typeof useIntl>): string {
  const value = field.modelDefault;
  return value == null || value === ''
    ? intl.formatMessage(i18n.default)
    : intl.formatMessage(i18n.modelDefault, { value: String(value) });
}

function ChoiceControl({
  field,
  value,
  disabled,
  onChange,
}: {
  field: ModelFieldDto;
  value: ModelFieldValue | undefined;
  disabled: boolean;
  onChange: (value: string | null) => void;
}) {
  const intl = useIntl();
  if (field.kind.type !== 'select') return null;
  const options = field.kind.options;
  const current = typeof value === 'string' ? value : '';
  const choices = [
    { value: '', label: defaultLabel(field, intl), testId: `model-field-${field.id}-default` },
    ...options.map((option) => ({
      value: option,
      label: option,
      testId: `model-field-${field.id}-${option}`,
    })),
  ];
  if (choices.length <= SEGMENTED_MAX_CHOICES) {
    return (
      <Segmented<string>
        aria-label={field.label}
        size="sm"
        options={choices}
        value={current}
        disabled={disabled}
        onChange={(next) => onChange(next === '' ? null : next)}
      />
    );
  }
  const selected = choices.find((choice) => choice.value === current) ?? choices[0];
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={field.label}
          disabled={disabled}
          data-testid={`model-field-${field.id}-trigger`}
          className={cx(
            'flex h-8 min-w-36 items-center justify-between gap-3 bg-lz-surface px-3 text-lz-body text-lz-ink',
            SURFACE.outline,
            RADIUS.control,
            FOCUS,
            MOTION
          )}
        >
          {selected.label}
          <ChevronDown className="size-4" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="max-h-64 overflow-y-auto bg-lz-surface text-lz-ink">
        {choices.map((choice) => (
          <DropdownMenuItem
            key={choice.value || 'default'}
            data-testid={choice.testId}
            onSelect={() => onChange(choice.value === '' ? null : choice.value)}
            className={cx(choice.value === current && SURFACE.selected)}
          >
            {choice.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function draftProblem(draft: NumberDraft, field: ModelFieldDto, intl: ReturnType<typeof useIntl>) {
  if (draft.kind !== 'invalid' || field.kind.type !== 'number') return null;
  switch (draft.reason) {
    case 'not-whole':
      return intl.formatMessage(i18n.notWhole);
    case 'below':
      return intl.formatMessage(i18n.below, { min: field.kind.min ?? '' });
    case 'above':
      return intl.formatMessage(i18n.above, { max: field.kind.max ?? '' });
    default:
      return intl.formatMessage(i18n.notANumber);
  }
}

function NumberControl({
  field,
  value,
  disabled,
  onCommit,
}: {
  field: ModelFieldDto;
  value: ModelFieldValue | undefined;
  disabled: boolean;
  onCommit: (value: number | null) => void;
}) {
  const intl = useIntl();
  const saved = typeof value === 'number' ? String(value) : '';
  const [text, setText] = useState(saved);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => setText(saved), [saved]);
  const commit = () => {
    const draft = parseNumberDraft(text, field);
    const reason = draftProblem(draft, field, intl);
    setProblem(reason);
    if (draft.kind === 'clear') {
      if (saved !== '') onCommit(null);
    } else if (draft.kind === 'value' && String(draft.value) !== saved) {
      onCommit(draft.value);
    }
  };
  const placeholder =
    field.modelDefault != null
      ? String(field.modelDefault)
      : intl.formatMessage(i18n.numberPlaceholder);
  return (
    <>
      <input
        aria-label={field.label}
        data-testid={`model-field-${field.id}`}
        inputMode="decimal"
        value={text}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') commit();
        }}
        className={cx(INPUT, 'w-28')}
      />
      {problem && <span className="text-lz-meta text-lz-err">{problem}</span>}
    </>
  );
}

/**
 * The custom fields of `modelId` on `providerId` — reasoning effort, verbosity, sampling — as the
 * provider's own metadata declares them, each saved the moment it changes. Renders nothing for a
 * model with no fields. Self-contained: mount it wherever a model is chosen.
 *
 * `pinned` maps a field id to why the caller fixes it (a benchmark tier that runs every entrant at
 * one effort): the row shows the reason and cannot be changed there. `onValuesChange` hears the
 * saved values whenever they load or change, for a caller that records them (a benchmark run).
 */
export function ModelCustomFields({
  providerId,
  modelId,
  disabled = false,
  pinned,
  onValuesChange,
}: {
  providerId: string;
  modelId: string;
  disabled?: boolean;
  pinned?: Readonly<Record<string, string>>;
  onValuesChange?: (values: ModelFieldValues) => void;
}) {
  const intl = useIntl();
  const model = modelId.trim();
  const [load, setLoad] = useState<Load>({ state: 'idle' });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const request = useRef(0);
  const report = useRef(onValuesChange);
  report.current = onValuesChange;

  useEffect(() => {
    const ticket = ++request.current;
    setSaveError(null);
    if (!providerId || !model) {
      setLoad({ state: 'idle' });
      report.current?.({});
      return;
    }
    setLoad({ state: 'loading' });
    const timer = setTimeout(() => {
      acpListModelFields(providerId, model)
        .then((listing) => {
          if (ticket !== request.current) return;
          setLoad({ state: 'ready', listing });
          report.current?.(listing.values);
        })
        .catch((err: unknown) => {
          if (ticket !== request.current) return;
          setLoad({ state: 'error', error: errorText(err) });
          report.current?.({});
        });
    }, LOOKUP_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [providerId, model]);

  if (load.state === 'idle') return null;
  if (load.state === 'loading') {
    return (
      <div
        className={cx('flex items-center gap-2', TYPE.meta)}
        data-testid="model-custom-fields-loading"
      >
        <Loader2 className="size-3 animate-spin" />
        {intl.formatMessage(i18n.loading)}
      </div>
    );
  }
  if (load.state === 'error') {
    return (
      <p role="alert" className="text-lz-meta text-lz-err" data-testid="model-custom-fields-error">
        {intl.formatMessage(i18n.loadFailed, { error: load.error })}
      </p>
    );
  }
  const { listing } = load;
  if (listing.source === 'unlisted_model') {
    return (
      <FieldNotice
        testId="model-custom-fields-unlisted"
        text={intl.formatMessage(i18n.unlisted, { model })}
      />
    );
  }
  if (listing.fields.length === 0) return null;

  const save = (id: string, value: ModelFieldValue | null) => {
    const before = listing.values;
    const next = withFieldValue(before, id, value);
    const ticket = request.current;
    setLoad({ state: 'ready', listing: { ...listing, values: next } });
    setSaving(true);
    setSaveError(null);
    acpSaveModelFields(providerId, model, next)
      .then((saved) => {
        if (ticket !== request.current) return;
        setLoad({ state: 'ready', listing: { ...listing, values: saved } });
        report.current?.(saved);
      })
      .catch((err: unknown) => {
        if (ticket !== request.current) return;
        setLoad({ state: 'ready', listing: { ...listing, values: before } });
        setSaveError(errorText(err));
      })
      .finally(() => {
        if (ticket === request.current) setSaving(false);
      });
  };

  const { choices, numbers } = splitFields(listing.fields);
  const row = (field: ModelFieldDto) => {
    const pin = pinned?.[field.id];
    if (pin != null) {
      return (
        <FieldRow key={field.id} label={field.label} testId={`model-field-row-${field.id}`}>
          <span className={TYPE.body} data-testid={`model-field-${field.id}-pinned`}>
            {intl.formatMessage(i18n.pinned, { note: pin })}
          </span>
        </FieldRow>
      );
    }
    return (
      <FieldRow
        key={field.id}
        label={field.label}
        testId={`model-field-row-${field.id}`}
        note={field.description}
      >
        {field.kind.type === 'select' ? (
          <ChoiceControl
            field={field}
            value={listing.values[field.id]}
            disabled={disabled}
            onChange={(value) => save(field.id, value)}
          />
        ) : (
          <NumberControl
            field={field}
            value={listing.values[field.id]}
            disabled={disabled}
            onCommit={(value) => save(field.id, value)}
          />
        )}
      </FieldRow>
    );
  };

  return (
    <section className="flex flex-col" data-testid="model-custom-fields">
      <div className="flex items-center justify-between gap-3 pb-1">
        <span className={TYPE.zone}>{intl.formatMessage(i18n.heading)}</span>
        {saving && <span className={TYPE.meta}>{intl.formatMessage(i18n.saving)}</span>}
      </div>
      <span className={cx('pb-1', TYPE.meta)}>{intl.formatMessage(i18n.scope)}</span>
      {choices.map(row)}
      {numbers.length > 0 && (
        <Disclosure
          variant="plain"
          className="border-t border-lz-border py-1"
          testId="model-custom-fields-sampling"
          title={
            <span className={TYPE.body}>
              {intl.formatMessage(i18n.sampling, { count: numbers.length })}
            </span>
          }
        >
          {numbers.map(row)}
        </Disclosure>
      )}
      {saveError && (
        <p
          role="alert"
          className="pt-1 text-lz-meta text-lz-err"
          data-testid="model-custom-fields-save-error"
        >
          {intl.formatMessage(i18n.saveFailed, { error: saveError })}
        </p>
      )}
    </section>
  );
}
