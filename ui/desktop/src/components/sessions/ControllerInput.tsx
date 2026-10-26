import { useMemo, useState } from 'react';
import type { InputOption, PendingInput } from '../../acp/controllerTypes';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { StudioSelect } from '../leanzero-swarm/studio';
import {
  controllerForm,
  formAnswer,
  objectValue,
  UNSUPPORTED_FORM,
  type ControllerFormField,
} from './controllerForm';

const messages = defineMessages({
  unsupported: {
    id: 'controller.input.unsupported',
    defaultMessage: 'This request cannot be answered here. Open it on the owner Mac.',
  },
  choose: { id: 'controller.input.choose', defaultMessage: 'Choose a value' },
  yes: { id: 'controller.input.yes', defaultMessage: 'Yes' },
  no: { id: 'controller.input.no', defaultMessage: 'No' },
  answer: { id: 'controller.input.answer', defaultMessage: 'Your answer' },
});

interface Props {
  input: PendingInput;
  disabled: boolean;
  onAnswer: (option: InputOption, value: string | null) => Promise<void>;
}
export default function ControllerInput({ input, disabled, onAnswer }: Props) {
  const intl = useIntl();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [answer, setAnswer] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const parsed = useMemo(() => {
    if (input.payload?.kind !== 'form') return { fields: [] as ControllerFormField[], error: null };
    try {
      return { fields: controllerForm(input.payload.schema), error: null };
    } catch (failure) {
      return { fields: [], error: failure instanceof Error ? failure.message : UNSUPPORTED_FORM };
    }
  }, [input.payload]);
  const permission = input.payload?.kind === 'permission' ? input.payload : null;
  const question = input.payload?.kind === 'question' ? input.payload : null;
  const supported = permission
    ? Boolean(
        permission.request_id &&
        permission.tool_name &&
        objectValue(permission.arguments) &&
        input.options.every((option) => !option.accepts_value)
      )
    : question
      ? Boolean(question.item_id)
      : input.payload?.kind === 'form' && !parsed.error;

  const submit = async (option: InputOption) => {
    if (!supported || disabled || sending) return;
    setError(null);
    try {
      const value = option.accepts_value
        ? question
          ? answer
          : formAnswer(parsed.fields, drafts)
        : null;
      setSending(true);
      await onAnswer(option, value);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setSending(false);
    }
  };

  return (
    <section
      className="rounded-xl border border-border-primary bg-background-secondary p-4 space-y-3"
      aria-label={input.prompt}
    >
      <p className="font-medium text-text-primary whitespace-pre-wrap">{input.prompt}</p>
      {permission && (
        <>
          <p className="font-mono text-sm break-all">{permission.tool_name}</p>
          <pre className="text-xs whitespace-pre-wrap break-all overflow-auto max-h-64 p-3 bg-background-primary rounded-lg">
            {JSON.stringify(permission.arguments, null, 2)}
          </pre>
        </>
      )}
      {question && (
        <>
          {question.why && <p className="text-sm whitespace-pre-wrap">{question.why}</p>}
          {question.recommended_answer && (
            <p className="text-sm whitespace-pre-wrap">{question.recommended_answer}</p>
          )}
          {input.options.some((option) => option.accepts_value) && (
            <Input
              aria-label={intl.formatMessage(messages.answer)}
              value={answer}
              onChange={(event) => setAnswer(event.target.value)}
              disabled={disabled || sending}
            />
          )}
        </>
      )}
      {supported &&
        parsed.fields.map((field) => {
          const options =
            field.enum?.map((value, index) => ({ value: String(index), label: String(value) })) ??
            (field.type === 'boolean'
              ? [
                  { value: 'true', label: intl.formatMessage(messages.yes) },
                  { value: 'false', label: intl.formatMessage(messages.no) },
                ]
              : null);
          return (
            <div key={field.name} className="space-y-1">
              <p className="text-sm font-medium">
                {field.title}
                {field.required ? ' *' : ''}
              </p>
              {field.description && (
                <p className="text-sm text-text-secondary">{field.description}</p>
              )}
              {options ? (
                <StudioSelect
                  aria-label={field.title}
                  options={options}
                  value={options.find((option) => option.value === drafts[field.name]) ?? null}
                  placeholder={intl.formatMessage(messages.choose)}
                  disabled={disabled || sending}
                  onChange={(option) => {
                    if (option)
                      setDrafts((current) => ({ ...current, [field.name]: option.value }));
                  }}
                />
              ) : (
                <Input
                  aria-label={field.title}
                  value={drafts[field.name] ?? ''}
                  disabled={disabled || sending}
                  type={field.type === 'string' ? 'text' : 'number'}
                  step={field.type === 'integer' ? '1' : 'any'}
                  onChange={(event) =>
                    setDrafts((current) => ({ ...current, [field.name]: event.target.value }))
                  }
                />
              )}
            </div>
          );
        })}
      {!supported && (
        <p role="alert" className="text-text-danger">
          {parsed.error ?? intl.formatMessage(messages.unsupported)}
        </p>
      )}
      {error && (
        <p role="alert" className="text-text-danger">
          {error}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {input.options.map((option) => (
          <Button
            key={option.option_id}
            disabled={!supported || disabled || sending}
            onClick={() => void submit(option)}
            variant="outline"
          >
            {option.label}
          </Button>
        ))}
      </div>
    </section>
  );
}
