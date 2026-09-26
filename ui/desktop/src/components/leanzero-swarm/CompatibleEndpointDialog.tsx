import { useCallback, useEffect, useRef, useState } from 'react';
import { Loader2, Plus, X } from 'lucide-react';
import type { CustomProviderConfigDto } from '@aaif/goose-sdk';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Button, Checkbox, Chip, TYPE, cx } from '../lz';
import { FIELD_LABEL, INPUT, ToneBanner } from './studio';
import {
  acpCreateCustomProviderFromRequest,
  acpDeleteCustomProvider,
  acpListProviderLiveModels,
  acpSaveProviderDefaultModel,
  acpUpdateCustomProviderFromRequest,
} from '../../acp/providers';
import type { ProviderDetails, UpdateCustomProviderRequest } from '../../types/providers';
import { errorMessage } from '../../utils/conversionUtils';
import { defineMessages, useIntl } from '../../i18n';
import { OPENAI_COMPATIBLE_TILE, ProviderTile } from './ProviderTile';
import { ModelChoice } from './ModelChoice';

const i18n = defineMessages({
  titleNew: {
    id: 'compatibleEndpoint.titleNew',
    defaultMessage: 'Add an OpenAI-compatible endpoint',
  },
  titleEdit: { id: 'compatibleEndpoint.titleEdit', defaultMessage: 'Edit {name}' },
  titleModel: { id: 'compatibleEndpoint.titleModel', defaultMessage: '{name} default model' },
  intro: {
    id: 'compatibleEndpoint.intro',
    defaultMessage:
      'Any server that speaks the OpenAI API — vLLM, llama.cpp, Together, Groq, a company gateway. Test connection saves it and asks the server for its models. For chat sessions: swarm nodes cannot use a compatible endpoint yet.',
  },
  modelIntro: {
    id: 'compatibleEndpoint.modelIntro',
    defaultMessage:
      'Pick the model chat starts with, or type one if the server does not list its models. Saving runs it once — that is what proves the endpoint.',
  },
  name: { id: 'compatibleEndpoint.name', defaultMessage: 'Name' },
  namePlaceholder: { id: 'compatibleEndpoint.namePlaceholder', defaultMessage: 'Team gateway' },
  baseUrl: { id: 'compatibleEndpoint.baseUrl', defaultMessage: 'Base URL' },
  baseUrlHint: {
    id: 'compatibleEndpoint.baseUrlHint',
    defaultMessage:
      'The server’s API root, usually ending in /v1 — goose adds /v1 when the address carries no version.',
  },
  apiKey: { id: 'compatibleEndpoint.apiKey', defaultMessage: 'API key' },
  apiKeyOptional: {
    id: 'compatibleEndpoint.apiKeyOptional',
    defaultMessage: 'optional — many local servers need none',
  },
  savedKey: { id: 'compatibleEndpoint.savedKey', defaultMessage: 'saved — leave blank to keep' },
  noKey: { id: 'compatibleEndpoint.noKey', defaultMessage: 'This server needs no key' },
  noKeyHint: {
    id: 'compatibleEndpoint.noKeyHint',
    defaultMessage: 'The saved key is deleted and requests go out without one.',
  },
  headers: { id: 'compatibleEndpoint.headers', defaultMessage: 'Custom headers' },
  headersHint: {
    id: 'compatibleEndpoint.headersHint',
    defaultMessage:
      'Sent with every request. Stored in plain text in the endpoint’s settings file.',
  },
  headerName: { id: 'compatibleEndpoint.headerName', defaultMessage: 'Header name' },
  headerValue: { id: 'compatibleEndpoint.headerValue', defaultMessage: 'Value' },
  addHeader: { id: 'compatibleEndpoint.addHeader', defaultMessage: 'Add header' },
  removeHeader: { id: 'compatibleEndpoint.removeHeader', defaultMessage: 'Remove header {name}' },
  required: { id: 'compatibleEndpoint.required', defaultMessage: '{field} is required' },
  test: { id: 'compatibleEndpoint.test', defaultMessage: 'Test connection' },
  testing: { id: 'compatibleEndpoint.testing', defaultMessage: 'Asking {name} for its models…' },
  listFailed: { id: 'compatibleEndpoint.listFailed', defaultMessage: 'No model list' },
  listFailedHint: {
    id: 'compatibleEndpoint.listFailedHint',
    defaultMessage:
      'Type the model id below if this server does not list its models; otherwise go back and fix the address or key.',
  },
  saveDefault: { id: 'compatibleEndpoint.saveDefault', defaultMessage: 'Save endpoint' },
  savingDefault: { id: 'compatibleEndpoint.savingDefault', defaultMessage: 'Running {model}…' },
  back: { id: 'compatibleEndpoint.back', defaultMessage: 'Back' },
  cancel: { id: 'compatibleEndpoint.cancel', defaultMessage: 'Cancel' },
  remove: { id: 'compatibleEndpoint.remove', defaultMessage: 'Remove' },
  removeConfirm: {
    id: 'compatibleEndpoint.removeConfirm',
    defaultMessage: 'Remove {name}, its key and its default model from this app?',
  },
  removeYes: { id: 'compatibleEndpoint.removeYes', defaultMessage: 'Yes, remove' },
  keep: { id: 'compatibleEndpoint.keep', defaultMessage: 'Keep' },
});

type Step = 'connection' | 'model' | 'remove';

interface HeaderRow {
  id: number;
  name: string;
  value: string;
}

export interface CompatibleEndpointDialogProps {
  /** An endpoint already saved (its registry row and its stored settings); absent to add a new one. */
  endpoint?: { provider: ProviderDetails; config: CustomProviderConfigDto };
  onClose: () => void;
  /** The provider list changed (an endpoint was created, updated or removed). */
  onSaved: () => Promise<void>;
}

/**
 * An OpenAI-compatible endpoint as a real goose provider (a declarative custom provider, engine
 * openai). Test connection saves the connection fields and asks THE ENGINE for the model list — the
 * same provider object chat will use, with the same URL, key and headers — then the chosen default is
 * run once (the proof) and written back as the endpoint's first model. A new endpoint that never gets
 * a default is removed again when the dialog closes, so an abandoned test leaves nothing behind.
 */
export default function CompatibleEndpointDialog({
  endpoint,
  onClose,
  onSaved,
}: CompatibleEndpointDialogProps) {
  const intl = useIntl();
  const saved = endpoint?.config;
  const [step, setStep] = useState<Step>(endpoint ? 'model' : 'connection');
  const [name, setName] = useState(saved?.displayName ?? '');
  const [baseUrl, setBaseUrl] = useState(saved?.apiUrl ?? '');
  const [apiKey, setApiKey] = useState('');
  // Only meaningful while a key is stored: ticking it switches auth off and deletes that key.
  const [noKey, setNoKey] = useState(false);
  const nextHeaderId = useRef(0);
  const [headers, setHeaders] = useState<HeaderRow[]>(() =>
    Object.entries(saved?.headers ?? {}).map(([headerName, value]) => ({
      id: nextHeaderId.current++,
      name: headerName,
      value,
    }))
  );
  const [fieldErrors, setFieldErrors] = useState<{ name?: string; baseUrl?: string }>({});
  const [providerId, setProviderId] = useState<string | null>(endpoint?.provider.name ?? null);
  // Created by THIS dialog and not yet given a default: closing removes it again.
  const [provisional, setProvisional] = useState(false);
  const [busy, setBusy] = useState<'test' | 'list' | 'save' | 'remove' | 'close' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [models, setModels] = useState<string[] | null>(saved ? null : []);
  const [chosen, setChosen] = useState<string | null>(endpoint?.provider.default_model ?? null);
  const [typed, setTyped] = useState('');

  const label = name.trim() || intl.formatMessage(i18n.titleNew);
  // Whether the engine holds a key for this endpoint right now — the saved one, or the one this
  // dialog just stored. A later write with a blank key must keep it, never switch auth off.
  const [keyStored, setKeyStored] = useState(!!saved?.apiKeySet && !!saved.requiresAuth);

  /** The full endpoint as the engine stores it; `models` and `api_key` vary per call. */
  const request = (models: string[], key: string): UpdateCustomProviderRequest => ({
    engine: saved?.engine ?? 'openai_compatible',
    display_name: name.trim(),
    api_url: baseUrl.trim(),
    api_key: key,
    models,
    headers: Object.fromEntries(
      headers.filter((h) => h.name.trim()).map((h) => [h.name.trim(), h.value.trim()])
    ),
    requires_auth: noKey ? false : key.length > 0 || keyStored,
    supports_streaming: saved?.supportsStreaming ?? null,
    base_path: saved?.basePath ?? null,
    catalog_provider_id: saved?.catalogProviderId ?? null,
    preserves_thinking: saved?.preservesThinking ?? null,
  });

  const listModels = useCallback(async (id: string) => {
    setBusy('list');
    setListError(null);
    try {
      const listed = await acpListProviderLiveModels(id);
      setModels(listed);
      // A saved default the server no longer lists stays visible as the typed id.
      setChosen((current) => {
        if (current && !listed.includes(current)) {
          setTyped(current);
          return null;
        }
        return current;
      });
    } catch (e) {
      setModels([]);
      setListError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }, []);

  const savedId = endpoint?.provider.name;
  useEffect(() => {
    if (savedId) void listModels(savedId);
  }, [savedId, listModels]);

  const testConnection = async () => {
    const errors: typeof fieldErrors = {};
    if (!name.trim()) {
      errors.name = intl.formatMessage(i18n.required, { field: intl.formatMessage(i18n.name) });
    }
    if (!baseUrl.trim()) {
      errors.baseUrl = intl.formatMessage(i18n.required, {
        field: intl.formatMessage(i18n.baseUrl),
      });
    }
    setFieldErrors(errors);
    if (Object.keys(errors).length > 0) return;
    setBusy('test');
    setError(null);
    try {
      const key = noKey ? '' : apiKey.trim();
      let id = providerId;
      if (id) {
        await acpUpdateCustomProviderFromRequest(id, request(saved?.models ?? [], key));
      } else {
        id = (await acpCreateCustomProviderFromRequest(request([], key))).provider_name;
        setProviderId(id);
        setProvisional(true);
      }
      setKeyStored(!noKey && (key.length > 0 || keyStored));
      setApiKey('');
      await onSaved();
      setStep('model');
      setBusy(null);
      await listModels(id);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(null);
    }
  };

  const selection = (chosen ?? typed).trim();

  const saveDefault = async () => {
    if (!providerId || !selection) return;
    setBusy('save');
    setError(null);
    try {
      await acpSaveProviderDefaultModel(providerId, selection);
      const rest = (models ?? []).filter((m) => m !== selection);
      await acpUpdateCustomProviderFromRequest(providerId, request([selection, ...rest], ''));
      setProvisional(false);
      await onSaved();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const remove = async (then: () => void) => {
    if (!providerId) return then();
    try {
      await acpDeleteCustomProvider(providerId);
      await onSaved();
      then();
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const close = async () => {
    if (busy != null) return;
    if (!provisional) return onClose();
    setBusy('close');
    await remove(onClose);
    setBusy(null);
  };

  return (
    <Dialog open onOpenChange={(open) => !open && void close()}>
      <DialogContent className="sm:max-w-[640px]" data-testid="compatible-endpoint-dialog">
        <DialogHeader>
          <DialogTitle className={cx('flex items-center gap-3', TYPE.h1)}>
            <ProviderTile providerId={OPENAI_COMPATIBLE_TILE} label={label} size="md" />
            {step === 'model'
              ? intl.formatMessage(i18n.titleModel, { name: label })
              : endpoint
                ? intl.formatMessage(i18n.titleEdit, { name: saved?.displayName ?? label })
                : intl.formatMessage(i18n.titleNew)}
          </DialogTitle>
          <DialogDescription className={TYPE.bodyMuted}>
            {step === 'remove'
              ? intl.formatMessage(i18n.removeConfirm, { name: label })
              : step === 'model'
                ? intl.formatMessage(i18n.modelIntro)
                : intl.formatMessage(i18n.intro)}
          </DialogDescription>
        </DialogHeader>

        {step === 'connection' && (
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              void testConnection();
            }}
          >
            <label className="flex flex-col gap-1">
              <span className={cx(FIELD_LABEL, 'flex items-center gap-2')}>
                {intl.formatMessage(i18n.name)}
                <span className="text-lz-err">*</span>
              </span>
              <input
                className={INPUT}
                aria-label={intl.formatMessage(i18n.name)}
                placeholder={intl.formatMessage(i18n.namePlaceholder)}
                autoComplete="off"
                disabled={busy != null}
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
              {fieldErrors.name && (
                <span className="text-lz-meta text-lz-err">{fieldErrors.name}</span>
              )}
            </label>
            <label className="flex flex-col gap-1">
              <span className={cx(FIELD_LABEL, 'flex items-center gap-2')}>
                {intl.formatMessage(i18n.baseUrl)}
                <span className="text-lz-err">*</span>
              </span>
              <input
                className={cx(INPUT, 'font-mono')}
                aria-label={intl.formatMessage(i18n.baseUrl)}
                placeholder="http://192.168.1.20:8000/v1"
                autoComplete="off"
                spellCheck={false}
                disabled={busy != null}
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
              />
              <span className={TYPE.meta}>{intl.formatMessage(i18n.baseUrlHint)}</span>
              {fieldErrors.baseUrl && (
                <span className="text-lz-meta text-lz-err">{fieldErrors.baseUrl}</span>
              )}
            </label>
            <label className="flex flex-col gap-1">
              <span className={cx(FIELD_LABEL, 'flex items-center gap-2')}>
                {intl.formatMessage(i18n.apiKey)}
                {keyStored && !noKey ? (
                  <Chip tone="ok">{intl.formatMessage(i18n.savedKey)}</Chip>
                ) : (
                  <span>{intl.formatMessage(i18n.apiKeyOptional)}</span>
                )}
              </span>
              <input
                className={INPUT}
                type="password"
                aria-label={intl.formatMessage(i18n.apiKey)}
                autoComplete="off"
                disabled={busy != null || noKey}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
              />
            </label>
            {(keyStored || noKey) && (
              <Checkbox
                checked={noKey}
                onChange={setNoKey}
                label={intl.formatMessage(i18n.noKey)}
                description={intl.formatMessage(i18n.noKeyHint)}
                disabled={busy != null}
                testId="compatible-endpoint-no-key"
              />
            )}
            <div className="flex flex-col gap-1">
              <span className={FIELD_LABEL}>{intl.formatMessage(i18n.headers)}</span>
              <span className={TYPE.meta}>{intl.formatMessage(i18n.headersHint)}</span>
              {headers.map((row) => (
                <div key={row.id} className="flex items-center gap-2">
                  <input
                    className={cx(INPUT, 'w-2/5 font-mono')}
                    aria-label={intl.formatMessage(i18n.headerName)}
                    placeholder={intl.formatMessage(i18n.headerName)}
                    autoComplete="off"
                    spellCheck={false}
                    disabled={busy != null}
                    value={row.name}
                    onChange={(e) =>
                      setHeaders((all) =>
                        all.map((h) => (h.id === row.id ? { ...h, name: e.target.value } : h))
                      )
                    }
                  />
                  <input
                    className={cx(INPUT, 'min-w-0 flex-1 font-mono')}
                    aria-label={intl.formatMessage(i18n.headerValue)}
                    placeholder={intl.formatMessage(i18n.headerValue)}
                    autoComplete="off"
                    spellCheck={false}
                    disabled={busy != null}
                    value={row.value}
                    onChange={(e) =>
                      setHeaders((all) =>
                        all.map((h) => (h.id === row.id ? { ...h, value: e.target.value } : h))
                      )
                    }
                  />
                  <Button
                    size="sm"
                    variant="ghost"
                    iconOnly
                    aria-label={intl.formatMessage(i18n.removeHeader, { name: row.name })}
                    disabled={busy != null}
                    onClick={() => setHeaders((all) => all.filter((h) => h.id !== row.id))}
                  >
                    <X />
                  </Button>
                </div>
              ))}
              <Button
                size="sm"
                variant="secondary"
                type="button"
                className="self-start"
                icon={<Plus />}
                disabled={busy != null}
                onClick={() =>
                  setHeaders((all) => [...all, { id: nextHeaderId.current++, name: '', value: '' }])
                }
              >
                {intl.formatMessage(i18n.addHeader)}
              </Button>
            </div>
            {error && <ToneBanner tone="err" label={label} text={error} />}
            <DialogFooter>
              <Button
                variant="ghost"
                type="button"
                disabled={busy != null}
                onClick={() => (providerId ? setStep('model') : void close())}
              >
                {intl.formatMessage(providerId ? i18n.back : i18n.cancel)}
              </Button>
              <Button
                variant="primary"
                type="submit"
                disabled={busy != null}
                icon={busy === 'test' ? <Loader2 className="animate-spin" /> : undefined}
                data-testid="compatible-endpoint-test"
              >
                {busy === 'test'
                  ? intl.formatMessage(i18n.testing, { name: label })
                  : intl.formatMessage(i18n.test)}
              </Button>
            </DialogFooter>
          </form>
        )}

        {step === 'model' && (
          <div className="flex flex-col gap-3">
            {listError && (
              <ToneBanner
                tone="warn"
                testId="compatible-endpoint-list-error"
                label={intl.formatMessage(i18n.listFailed)}
                text={`${listError} — ${intl.formatMessage(i18n.listFailedHint)}`}
              />
            )}
            <ModelChoice
              label={label}
              models={models}
              source="live"
              loading={busy === 'list'}
              currentDefault={endpoint?.provider.default_model}
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
            {error && <ToneBanner tone="err" label={label} text={error} />}
            <DialogFooter className="items-center">
              {endpoint && (
                <Button
                  variant="ghost"
                  onClick={() => setStep('remove')}
                  disabled={busy != null}
                  className="mr-auto text-lz-err"
                >
                  {intl.formatMessage(i18n.remove)}
                </Button>
              )}
              <Button variant="ghost" onClick={() => setStep('connection')} disabled={busy != null}>
                {intl.formatMessage(i18n.back)}
              </Button>
              <Button
                variant="primary"
                disabled={busy != null || !selection}
                onClick={() => void saveDefault()}
                icon={busy === 'save' ? <Loader2 className="animate-spin" /> : undefined}
                data-testid="compatible-endpoint-save"
              >
                {busy === 'save'
                  ? intl.formatMessage(i18n.savingDefault, { model: selection })
                  : intl.formatMessage(i18n.saveDefault)}
              </Button>
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
                onClick={async () => {
                  setBusy('remove');
                  await remove(onClose);
                  setBusy(null);
                }}
                disabled={busy != null}
                icon={busy === 'remove' ? <Loader2 className="animate-spin" /> : undefined}
                data-testid="compatible-endpoint-remove"
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
