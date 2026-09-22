import { useCallback, useEffect, useMemo, useState } from 'react';
import { Check, Loader2, Search } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Button, Chip, SURFACE, TYPE, cx } from '../lz';
import { INPUT, ToneBanner } from './studio';
import {
  acpDeleteProviderConfig,
  acpListProviderLiveModels,
  acpReadProviderConfig,
  acpSaveProviderConfig,
  acpSaveProviderDefaultModel,
} from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import { errorMessage } from '../../utils/conversionUtils';
import { defineMessages, useIntl } from '../../i18n';
import { ProviderTile } from './ProviderTile';

const i18n = defineMessages({
  titleSetup: { id: 'cloudProviderSetup.titleSetup', defaultMessage: 'Connect {provider}' },
  titleModel: {
    id: 'cloudProviderSetup.titleModel',
    defaultMessage: '{provider} default model',
  },
  keyIntro: {
    id: 'cloudProviderSetup.keyIntro',
    defaultMessage:
      'Paste your key. {provider} is asked for the models it can run, and the key is stored (encrypted, in your goose secret store) only when it answers.',
  },
  keyIntroReplace: {
    id: 'cloudProviderSetup.keyIntroReplace',
    defaultMessage:
      'A key is already stored. Enter a new one to replace it — it is checked with {provider} before the old one is dropped.',
  },
  modelIntro: {
    id: 'cloudProviderSetup.modelIntro',
    defaultMessage:
      'Pick the model this provider starts with. It leads the list whenever you add a node and can be changed there per node.',
  },
  fieldRequired: { id: 'cloudProviderSetup.fieldRequired', defaultMessage: '{field} is required' },
  savedKey: { id: 'cloudProviderSetup.savedKey', defaultMessage: 'saved — leave blank to keep' },
  connect: { id: 'cloudProviderSetup.connect', defaultMessage: 'Connect' },
  connecting: { id: 'cloudProviderSetup.connecting', defaultMessage: 'Checking with {provider}…' },
  listing: { id: 'cloudProviderSetup.listing', defaultMessage: 'Asking {provider} for its models…' },
  filter: { id: 'cloudProviderSetup.filter', defaultMessage: 'Filter models' },
  modelsLive: {
    id: 'cloudProviderSetup.modelsLive',
    defaultMessage: '{count, plural, one {# model} other {# models}} your key can run',
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
  saveDefault: { id: 'cloudProviderSetup.saveDefault', defaultMessage: 'Save default model' },
  savingDefault: { id: 'cloudProviderSetup.savingDefault', defaultMessage: 'Running {model}…' },
  replaceKey: { id: 'cloudProviderSetup.replaceKey', defaultMessage: 'Replace key' },
  remove: { id: 'cloudProviderSetup.remove', defaultMessage: 'Remove' },
  removeConfirm: {
    id: 'cloudProviderSetup.removeConfirm',
    defaultMessage: 'Remove the {provider} key and its default model from this app?',
  },
  removeYes: { id: 'cloudProviderSetup.removeYes', defaultMessage: 'Yes, remove' },
  keep: { id: 'cloudProviderSetup.keep', defaultMessage: 'Keep' },
  cancel: { id: 'cloudProviderSetup.cancel', defaultMessage: 'Cancel' },
  back: { id: 'cloudProviderSetup.back', defaultMessage: 'Back' },
  retryList: { id: 'cloudProviderSetup.retryList', defaultMessage: 'Ask again' },
  azureDone: {
    id: 'cloudProviderSetup.azureDone',
    defaultMessage: 'Azure runs the deployment you named; there is no separate model to choose.',
  },
});

type Step = 'key' | 'model' | 'remove';

/** Azure has no model listing — the deployment name IS the model. */
const DEPLOYMENT_PROVIDERS = new Set(['azure_openai']);

export interface CloudProviderSetupDialogProps {
  provider: ProviderDetails;
  onClose: () => void;
  /** The provider list changed (a key landed, a default was saved, a config was removed). */
  onSaved: () => Promise<void>;
}

/**
 * Two steps, never a typed "test model": (1) the key — verified by asking the provider for its
 * models, and (2) the default model — chosen from that answer and run once before it is saved.
 * A configured provider opens at step 2 with its current default selected.
 */
export default function CloudProviderSetupDialog({
  provider,
  onClose,
  onSaved,
}: CloudProviderSetupDialogProps) {
  const intl = useIntl();
  const label = provider.metadata.display_name;
  const deploymentOnly = DEPLOYMENT_PROVIDERS.has(provider.name);
  const [step, setStep] = useState<Step>(provider.is_configured ? 'model' : 'key');
  const [values, setValues] = useState<Record<string, string>>({});
  const [serverValues, setServerValues] = useState<Record<string, string>>({});
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<'connect' | 'list' | 'save' | 'remove' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [models, setModels] = useState<string[] | null>(null);
  const [liveListing, setLiveListing] = useState(true);
  const [filter, setFilter] = useState('');
  const [typed, setTyped] = useState('');
  const [chosen, setChosen] = useState<string | null>(provider.default_model ?? null);

  const fields = useMemo(
    () => provider.metadata.config_keys.filter((key) => !key.oauth_flow),
    [provider.metadata.config_keys]
  );

  useEffect(() => {
    if (!provider.is_configured) return;
    let cancelled = false;
    acpReadProviderConfig(provider.name)
      .then((saved) => {
        if (cancelled) return;
        const next: Record<string, string> = {};
        for (const field of saved) {
          const key = fields.find((f) => f.name === field.key);
          if (key && !key.secret && typeof field.value === 'string') next[field.key] = field.value;
        }
        setServerValues(next);
      })
      .catch(() => {
        // the saved non-secret values only prefill; a failed read leaves the inputs empty and the
        // provider's own answer decides on Connect
      });
    return () => {
      cancelled = true;
    };
  }, [provider.is_configured, provider.name, fields]);

  const listModels = useCallback(async () => {
    setBusy('list');
    setError(null);
    try {
      const live = await acpListProviderLiveModels(provider.name);
      if (live.length > 0) {
        setModels(live);
        setLiveListing(true);
      } else {
        setModels(provider.metadata.known_models.map((m) => m.name));
        setLiveListing(false);
      }
    } catch (e) {
      setError(errorMessage(e));
      setModels((current) => current ?? []);
    } finally {
      setBusy(null);
    }
  }, [provider.name, provider.metadata.known_models]);

  useEffect(() => {
    if (step === 'model' && models == null && !deploymentOnly) void listModels();
  }, [step, models, deploymentOnly, listModels]);

  const connect = async () => {
    const errors: Record<string, string> = {};
    const submit: { key: string; value: string }[] = [];
    for (const field of fields) {
      const entered = values[field.name]?.trim() ?? '';
      const kept = serverValues[field.name]?.trim() ?? '';
      const value = entered || kept;
      if (!value) {
        if (field.required && !(field.secret && provider.is_configured)) {
          errors[field.name] = intl.formatMessage(i18n.fieldRequired, { field: field.name });
        }
        continue;
      }
      submit.push({ key: field.name, value });
    }
    setFieldErrors(errors);
    if (Object.keys(errors).length > 0) return;
    setBusy('connect');
    setError(null);
    try {
      await acpSaveProviderConfig(provider.name, submit);
      await onSaved();
      if (deploymentOnly) {
        onClose();
        return;
      }
      setModels(null);
      setStep('model');
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const saveDefault = async () => {
    const model = (chosen ?? typed).trim();
    if (!model) return;
    setBusy('save');
    setError(null);
    try {
      await acpSaveProviderDefaultModel(provider.name, model);
      await onSaved();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    setBusy('remove');
    setError(null);
    try {
      await acpDeleteProviderConfig(provider.name);
      await onSaved();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const shown = useMemo(() => {
    const list = models ?? [];
    const q = filter.trim().toLowerCase();
    const ordered =
      chosen && list.includes(chosen) ? [chosen, ...list.filter((m) => m !== chosen)] : list;
    return q ? ordered.filter((m) => m.toLowerCase().includes(q)) : ordered;
  }, [models, filter, chosen]);

  const selection = (chosen ?? typed).trim();

  return (
    <Dialog open onOpenChange={(open) => !open && busy == null && onClose()}>
      <DialogContent className="sm:max-w-[600px]" data-testid="cloud-provider-setup">
        <DialogHeader>
          <DialogTitle className={cx('flex items-center gap-3', TYPE.h1)}>
            <ProviderTile providerId={provider.name} label={label} size="md" />
            {intl.formatMessage(step === 'key' ? i18n.titleSetup : i18n.titleModel, {
              provider: label,
            })}
          </DialogTitle>
          <DialogDescription className={TYPE.bodyMuted}>
            {step === 'remove'
              ? intl.formatMessage(i18n.removeConfirm, { provider: label })
              : step === 'key'
                ? intl.formatMessage(
                    provider.is_configured ? i18n.keyIntroReplace : i18n.keyIntro,
                    { provider: label }
                  )
                : intl.formatMessage(i18n.modelIntro)}
          </DialogDescription>
        </DialogHeader>

        {step === 'key' && (
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              void connect();
            }}
          >
            {fields.map((field) => (
              <label key={field.name} className="flex flex-col gap-1">
                <span className={cx(TYPE.meta, 'flex items-center gap-2')}>
                  {field.name}
                  {field.required && <span className="text-lz-err">*</span>}
                  {field.secret && provider.is_configured && (
                    <Chip tone="ok">{intl.formatMessage(i18n.savedKey)}</Chip>
                  )}
                </span>
                <input
                  className={INPUT}
                  type={field.secret ? 'password' : 'text'}
                  autoComplete="off"
                  aria-label={field.name}
                  disabled={busy != null}
                  value={values[field.name] ?? serverValues[field.name] ?? ''}
                  onChange={(e) => setValues((v) => ({ ...v, [field.name]: e.target.value }))}
                />
                {fieldErrors[field.name] && (
                  <span className="text-lz-meta text-lz-err">{fieldErrors[field.name]}</span>
                )}
              </label>
            ))}
            {error && <ToneBanner tone="err" label={label} text={error} />}
            <DialogFooter>
              {provider.is_configured ? (
                <Button variant="ghost" type="button" onClick={() => setStep('model')}>
                  {intl.formatMessage(i18n.back)}
                </Button>
              ) : (
                <Button variant="ghost" type="button" onClick={onClose} disabled={busy != null}>
                  {intl.formatMessage(i18n.cancel)}
                </Button>
              )}
              <Button
                variant="primary"
                type="submit"
                disabled={busy != null}
                icon={busy === 'connect' ? <Loader2 className="animate-spin" /> : undefined}
                data-testid="cloud-provider-connect"
              >
                {busy === 'connect'
                  ? intl.formatMessage(i18n.connecting, { provider: label })
                  : intl.formatMessage(i18n.connect)}
              </Button>
            </DialogFooter>
          </form>
        )}

        {step === 'model' && (
          <div className="flex flex-col gap-3">
            {deploymentOnly ? (
              <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.azureDone)}</p>
            ) : (
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
                  {models != null && busy !== 'list' && (
                    <Chip tone={liveListing ? 'ok' : 'warn'}>
                      {liveListing
                        ? intl.formatMessage(i18n.modelsLive, { count: models.length })
                        : label}
                    </Chip>
                  )}
                </div>
                {!liveListing && models != null && (
                  <p className={TYPE.meta}>
                    {intl.formatMessage(i18n.modelsRegistry, { provider: label })}
                  </p>
                )}
                {busy === 'list' || models == null ? (
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
                            onClick={() => {
                              setChosen(model);
                              setTyped('');
                            }}
                            className={cx(
                              'flex h-8 w-full items-center gap-2 px-3 text-left font-mono text-lz-mono',
                              selected ? SURFACE.selected : cx('text-lz-ink', SURFACE.hover)
                            )}
                          >
                            <span className="size-4 shrink-0">
                              {selected && <Check className="size-4" />}
                            </span>
                            <span className="min-w-0 flex-1 truncate">{model}</span>
                            {model === provider.default_model && (
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
                  <span className={cx(TYPE.meta, 'shrink-0')}>
                    {intl.formatMessage(i18n.typeModel)}
                  </span>
                  <input
                    className={cx(INPUT, 'flex-1 font-mono')}
                    aria-label={intl.formatMessage(i18n.typeModel)}
                    value={typed}
                    onChange={(e) => {
                      setTyped(e.target.value);
                      setChosen(null);
                    }}
                    autoComplete="off"
                    spellCheck={false}
                  />
                </label>
              </>
            )}
            {error && (
              <ToneBanner
                tone="err"
                label={label}
                text={error}
                action={
                  !deploymentOnly && busy == null ? (
                    <Button size="sm" variant="secondary" onClick={() => void listModels()}>
                      {intl.formatMessage(i18n.retryList)}
                    </Button>
                  ) : undefined
                }
              />
            )}
            <DialogFooter className="items-center">
              {provider.is_configured && (
                <>
                  <Button
                    variant="ghost"
                    onClick={() => setStep('remove')}
                    disabled={busy != null}
                    className="mr-auto text-lz-err"
                  >
                    {intl.formatMessage(i18n.remove)}
                  </Button>
                  <Button variant="ghost" onClick={() => setStep('key')} disabled={busy != null}>
                    {intl.formatMessage(i18n.replaceKey)}
                  </Button>
                </>
              )}
              {deploymentOnly ? (
                <Button variant="primary" onClick={onClose}>
                  {intl.formatMessage(i18n.keep)}
                </Button>
              ) : (
                <Button
                  variant="primary"
                  disabled={busy != null || !selection}
                  onClick={() => void saveDefault()}
                  icon={busy === 'save' ? <Loader2 className="animate-spin" /> : undefined}
                  data-testid="cloud-provider-save-default"
                >
                  {busy === 'save'
                    ? intl.formatMessage(i18n.savingDefault, { model: selection })
                    : intl.formatMessage(i18n.saveDefault)}
                </Button>
              )}
            </DialogFooter>
          </div>
        )}

        {step === 'remove' && (
          <div className="flex flex-col gap-3">
            {error && <ToneBanner tone="err" label={label} text={error} />}
            <DialogFooter>
              <Button variant="ghost" onClick={() => setStep('model')} disabled={busy != null}>
                {intl.formatMessage(i18n.keep)}
              </Button>
              <Button
                variant="destructive"
                onClick={() => void remove()}
                disabled={busy != null}
                icon={busy === 'remove' ? <Loader2 className="animate-spin" /> : undefined}
                data-testid="cloud-provider-remove"
              >
                {intl.formatMessage(i18n.removeYes)}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
