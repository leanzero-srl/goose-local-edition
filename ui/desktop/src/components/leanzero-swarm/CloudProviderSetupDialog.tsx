import { useCallback, useEffect, useMemo, useState } from 'react';
import { Loader2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Button, Chip, TYPE, cx } from '../lz';
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
import { ModelChoice, type ModelSource } from './ModelChoice';
import { officialEndpointReset, type EndpointOverride } from './openaiEndpoint';

const i18n = defineMessages({
  titleSetup: { id: 'cloudProviderSetup.titleSetup', defaultMessage: 'Connect {provider}' },
  titleModel: {
    id: 'cloudProviderSetup.titleModel',
    defaultMessage: '{provider} default model',
  },
  keyIntro: {
    id: 'cloudProviderSetup.keyIntro',
    defaultMessage:
      'Paste your key. {provider} is asked for its model list, then the key is proven when the default model you pick runs. Keys are encrypted into your goose secret store.',
  },
  keyIntroReplace: {
    id: 'cloudProviderSetup.keyIntroReplace',
    defaultMessage:
      'A key is already stored. Enter a new one to replace it — it is checked with {provider} before the old one is dropped.',
  },
  modelIntro: {
    id: 'cloudProviderSetup.modelIntro',
    defaultMessage:
      'Pick the model this provider starts with. Saving runs it once — that is what proves your key. It leads the list whenever you add a node and can be changed there per node.',
  },
  fieldRequired: { id: 'cloudProviderSetup.fieldRequired', defaultMessage: '{field} is required' },
  savedKey: { id: 'cloudProviderSetup.savedKey', defaultMessage: 'saved — leave blank to keep' },
  connect: { id: 'cloudProviderSetup.connect', defaultMessage: 'Connect' },
  connecting: { id: 'cloudProviderSetup.connecting', defaultMessage: 'Checking with {provider}…' },
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
  notOfficial: { id: 'cloudProviderSetup.notOfficial', defaultMessage: 'Not the official API' },
  notOfficialText: {
    id: 'cloudProviderSetup.notOfficialText',
    defaultMessage:
      'Saved settings send these requests to {where}, not api.openai.com. Connect resets them to the official API. To keep using that server, add it as an OpenAI-compatible endpoint.',
  },
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
  /** Saved endpoint fields that send this provider somewhere other than its official API (OpenAI's
   *  OPENAI_HOST & co.). Shown loudly on every step; Connect resets them to the official values. */
  endpointOverrides?: EndpointOverride[];
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
  endpointOverrides = [],
  onClose,
  onSaved,
}: CloudProviderSetupDialogProps) {
  const intl = useIntl();
  const label = provider.metadata.display_name;
  const deploymentOnly = DEPLOYMENT_PROVIDERS.has(provider.name);
  // A stored key — proven or still waiting on its default model — is what makes Replace/Remove real.
  const hasKey = provider.credentials_saved || provider.is_configured;
  // An endpoint that is not the official API opens on Connect: that is the step that resets it, and
  // a default saved first would be proven against the wrong server.
  const [step, setStep] = useState<Step>(
    hasKey && endpointOverrides.length === 0 ? 'model' : 'key'
  );
  const [values, setValues] = useState<Record<string, string>>({});
  const [serverValues, setServerValues] = useState<Record<string, string>>({});
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<'connect' | 'list' | 'save' | 'remove' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [models, setModels] = useState<string[] | null>(null);
  const [modelSource, setModelSource] = useState<ModelSource>('live');
  const [typed, setTyped] = useState('');
  const [chosen, setChosen] = useState<string | null>(provider.default_model ?? null);

  const fields = useMemo(
    () => provider.metadata.config_keys.filter((key) => !key.oauth_flow),
    [provider.metadata.config_keys]
  );

  useEffect(() => {
    if (!hasKey) return;
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
  }, [hasKey, provider.name, fields]);

  const listModels = useCallback(async () => {
    setBusy('list');
    setError(null);
    try {
      const live = await acpListProviderLiveModels(provider.name);
      if (live.length > 0) {
        setModels(live);
        setModelSource('live');
      } else {
        setModels(provider.metadata.known_models.map((m) => m.name));
        setModelSource('registry');
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
        if (field.required && !(field.secret && hasKey)) {
          errors[field.name] = intl.formatMessage(i18n.fieldRequired, { field: field.name });
        }
        continue;
      }
      submit.push({ key: field.name, value });
    }
    setFieldErrors(errors);
    if (Object.keys(errors).length > 0) return;
    submit.push(...officialEndpointReset(endpointOverrides));
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
                ? intl.formatMessage(hasKey ? i18n.keyIntroReplace : i18n.keyIntro, {
                    provider: label,
                  })
                : intl.formatMessage(i18n.modelIntro)}
          </DialogDescription>
        </DialogHeader>

        {endpointOverrides.length > 0 && step !== 'remove' && (
          <ToneBanner
            tone="err"
            testId="cloud-provider-not-official"
            label={intl.formatMessage(i18n.notOfficial)}
            text={intl.formatMessage(i18n.notOfficialText, {
              where: endpointOverrides.map((o) => `${o.key}=${o.value}`).join(', '),
            })}
          />
        )}

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
                  {field.secret && hasKey && (
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
              {hasKey ? (
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
              <ModelChoice
                label={label}
                models={models}
                source={modelSource}
                loading={busy === 'list'}
                currentDefault={provider.default_model}
                chosen={chosen}
                onChoose={(model) => {
                  setChosen(model);
                  setTyped('');
                }}
                typed={typed}
                onType={(model) => {
                  setTyped(model);
                  setChosen(null);
                }}
              />
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
              {hasKey && (
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
