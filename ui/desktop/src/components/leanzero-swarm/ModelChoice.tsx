import { useMemo, useState } from 'react';
import { Check, Loader2, Search } from 'lucide-react';
import { Chip, SURFACE, TYPE, cx } from '../lz';
import { INPUT } from './studio';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  titleModel: {
    id: 'cloudProviderSetup.titleModel',
    defaultMessage: '{provider} default model',
  },
  listing: {
    id: 'cloudProviderSetup.listing',
    defaultMessage: 'Asking {provider} for its models…',
  },
  filter: { id: 'cloudProviderSetup.filter', defaultMessage: 'Filter models' },
  modelsLive: {
    id: 'cloudProviderSetup.modelsLive',
    defaultMessage: '{count, plural, one {# model} other {# models}} listed by {provider}',
  },
  modelsRegistry: {
    id: 'cloudProviderSetup.modelsRegistry',
    defaultMessage:
      '{provider} has no model listing. These are the ids goose knows for it — access depends on your account.',
  },
  noMatch: { id: 'cloudProviderSetup.noMatch', defaultMessage: 'no model matches the filter' },
  typeModel: {
    id: 'cloudProviderSetup.typeModel',
    defaultMessage: 'Or type a model id',
  },
  current: { id: 'cloudProviderSetup.current', defaultMessage: 'current default' },
});

/** Where the offered ids came from: the provider's own listing, or goose's registry for a provider
 *  that has none. */
export type ModelSource = 'live' | 'registry';

export interface ModelChoiceProps {
  /** The provider's display name, used in every sentence. */
  label: string;
  models: string[] | null;
  source: ModelSource;
  loading: boolean;
  /** The saved default, marked in the list. */
  currentDefault?: string | null;
  chosen: string | null;
  onChoose: (model: string) => void;
  typed: string;
  onType: (model: string) => void;
}

/** The default-model picker every provider dialog shares: a filter, the listed ids as a listbox
 *  (the chosen one first), and a free-text id for a model the listing does not carry. Choosing from
 *  the list clears the typed id and typing clears the choice — one selection at a time. */
export function ModelChoice({
  label,
  models,
  source,
  loading,
  currentDefault,
  chosen,
  onChoose,
  typed,
  onType,
}: ModelChoiceProps) {
  const intl = useIntl();
  const [filter, setFilter] = useState('');

  const shown = useMemo(() => {
    const list = models ?? [];
    const q = filter.trim().toLowerCase();
    const ordered =
      chosen && list.includes(chosen) ? [chosen, ...list.filter((m) => m !== chosen)] : list;
    return q ? ordered.filter((m) => m.toLowerCase().includes(q)) : ordered;
  }, [models, filter, chosen]);

  return (
    <>
      <div className="flex items-center gap-2">
        <label className="relative flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-lz-ink-3" />
          <input
            className={cx(INPUT, 'w-full pl-8')}
            placeholder={intl.formatMessage(i18n.filter)}
            aria-label={intl.formatMessage(i18n.filter)}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            autoComplete="off"
          />
        </label>
        {models != null && models.length > 0 && !loading && (
          <Chip tone={source === 'live' ? 'ok' : 'warn'}>
            {source === 'live'
              ? intl.formatMessage(i18n.modelsLive, { count: models.length, provider: label })
              : label}
          </Chip>
        )}
      </div>
      {source === 'registry' && models != null && (
        <p className={TYPE.meta}>{intl.formatMessage(i18n.modelsRegistry, { provider: label })}</p>
      )}
      {!loading && models != null && models.length === 0 ? null : loading || models == null ? (
        <p className={cx('flex items-center gap-2', TYPE.meta)}>
          <Loader2 className="size-3 animate-spin" />
          {intl.formatMessage(i18n.listing, { provider: label })}
        </p>
      ) : (
        <div
          role="listbox"
          aria-label={intl.formatMessage(i18n.titleModel, { provider: label })}
          className={cx('max-h-64 overflow-y-auto', SURFACE.outline, 'rounded-lz-control')}
        >
          {shown.length === 0 ? (
            <p className={cx('px-3 py-2', TYPE.meta)}>{intl.formatMessage(i18n.noMatch)}</p>
          ) : (
            shown.map((model) => {
              const selected = model === chosen;
              return (
                <button
                  key={model}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  data-testid={`cloud-model-${model}`}
                  onClick={() => onChoose(model)}
                  className={cx(
                    'flex h-8 w-full items-center gap-2 px-3 text-left font-mono text-lz-mono',
                    selected ? SURFACE.selected : cx('text-lz-ink', SURFACE.hover)
                  )}
                >
                  <span className="size-4 shrink-0">
                    {selected && <Check className="size-4" />}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{model}</span>
                  {model === currentDefault && (
                    <Chip tone={selected ? 'secondary' : 'accent'}>
                      {intl.formatMessage(i18n.current)}
                    </Chip>
                  )}
                </button>
              );
            })
          )}
        </div>
      )}
      <label className="flex items-center gap-2">
        <span className={cx(TYPE.meta, 'shrink-0')}>{intl.formatMessage(i18n.typeModel)}</span>
        <input
          className={cx(INPUT, 'flex-1 font-mono')}
          aria-label={intl.formatMessage(i18n.typeModel)}
          value={typed}
          onChange={(e) => onType(e.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
      </label>
    </>
  );
}
