import { useEffect, useRef, useState } from 'react';
import { ChevronDown, Loader2 } from 'lucide-react';
import {
  acpListProviderDetails,
  acpListProviderLiveModels,
  acpReadProviderConfig,
  acpRecheckProviderConnections,
} from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { Button, Combobox, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { isLocalEditionCloudProvider } from '../settings/models/leanzeroSelectorPolicy';
import { providerRowState } from '../leanzero-swarm/cloudProviderState';
import { OpenRouterHostPicker } from '../openrouter/OpenRouterHostPicker';
import CloudProviderSetupDialog from '../leanzero-swarm/CloudProviderSetupDialog';
import { openAiEndpointOverrides, type EndpointOverride } from '../leanzero-swarm/openaiEndpoint';
import { BENCHMARK_THINKING_LINE } from '../../benchModelFields';

/** A cloud provider whose settings are saved but which the form cannot offer, and why — the check's
 *  own words when it failed. 2026-10-03: an OPENROUTER_PARAMETERS pin failed OpenRouter's check with
 *  a 404 and the form said only "No configured providers", hiding the one line that named the cause. */
export function unusableProviderReasons(
  rows: readonly ProviderDetails[]
): { name: string; label: string; reason: string }[] {
  return rows.flatMap((row) => {
    if (row.is_configured || !isLocalEditionCloudProvider(row.name)) return [];
    const state = providerRowState(row);
    if (state === 'not-set-up') return [];
    const reason =
      state === 'failed'
        ? `its connection check failed: ${row.connection_error}`
        : state === 'not-running'
          ? `nothing is running at ${row.local_server_not_running} yet — start the local server, then press Refresh providers`
          : 'its key is saved but the connection check has not passed yet — press Refresh providers';
    return [{ name: row.name, label: row.metadata.display_name, reason }];
  });
}

/** What the selected provider serves, asked live with its saved key. A failed or absent listing is
 *  stated, and goose's catalog for the provider is offered under that statement — never an empty
 *  list that reads as "this provider has no models". */
export type ModelListRead =
  | { kind: 'idle' }
  | { kind: 'reading' }
  | { kind: 'read'; models: string[] }
  | { kind: 'unlisted'; catalog: string[] }
  | { kind: 'failed'; error: string; catalog: string[] };

function readError(err: unknown): string {
  if (err && typeof err === 'object' && 'data' in err && typeof err.data === 'string')
    return err.data;
  return err instanceof Error ? err.message : String(err);
}

/** The provider's default model leads, marked; then the provider's own order. */
export function modelOptions(read: ModelListRead, providerDefault: string | null) {
  const listed =
    read.kind === 'read'
      ? read.models
      : read.kind === 'unlisted' || read.kind === 'failed'
        ? read.catalog
        : [];
  const ordered =
    providerDefault && listed.includes(providerDefault)
      ? [providerDefault, ...listed.filter((m) => m !== providerDefault)]
      : listed;
  return ordered.map((value) => ({
    value,
    hint: value === providerDefault ? 'default' : undefined,
  }));
}

function ModelListStatus({ read, provider }: { read: ModelListRead; provider: string }) {
  switch (read.kind) {
    case 'idle':
      return <span className={TYPE.meta}>Choose a provider to list the models it serves.</span>;
    case 'reading':
      return (
        <span
          className={cx('flex items-center gap-2', TYPE.meta)}
          data-testid="bench-models-reading"
        >
          <Loader2 className="size-3 animate-spin" />
          Asking {provider} for its models… You can type an ID meanwhile.
        </span>
      );
    case 'read':
      return (
        <span className={TYPE.meta} data-testid="bench-models-read">
          {read.models.length} models {provider} serves with your key. An ID that is not listed is
          used exactly as typed.
        </span>
      );
    case 'unlisted':
      return (
        <span className={TYPE.meta} data-testid="bench-models-unlisted">
          {read.catalog.length > 0
            ? `${provider} has no model listing, so these are the ${read.catalog.length} models goose knows for it.`
            : `${provider} has no model listing and goose knows no models for it.`}{' '}
          Type the provider’s exact model ID if yours is not here.
        </span>
      );
    case 'failed':
      return (
        <span
          role="alert"
          className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.err)}
          data-testid="bench-models-failed"
        >
          {provider} did not list its models: {read.error}.{' '}
          {read.catalog.length > 0
            ? 'These are the models goose knows for it; any exact model ID can still be typed.'
            : 'Type the provider’s exact model ID.'}
        </span>
      );
  }
}

type Setup = { provider: ProviderDetails; overrides: EndpointOverride[] };

export function CloudEntrant({
  provider,
  model,
  disabled,
  onChange,
}: {
  provider: string;
  model: string;
  disabled: boolean;
  onChange: (provider: string, model: string) => void;
}) {
  const [cloudRows, setCloudRows] = useState<ProviderDetails[]>([]);
  const [providers, setProviders] = useState<ProviderDetails[]>([]);
  const [unusable, setUnusable] = useState<ReturnType<typeof unusableProviderReasons>>([]);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [reload, setReload] = useState(0);
  const [loading, setLoading] = useState(true);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [setupError, setSetupError] = useState<string | null>(null);
  const [models, setModels] = useState<ModelListRead>({ kind: 'idle' });
  const [listRevision, setListRevision] = useState(0);
  // A provider set up from this form is chosen as soon as the reload shows it configured.
  const selectAfterSetup = useRef<string | null>(null);
  const change = useRef(onChange);
  change.current = onChange;
  const current = useRef(provider);
  current.current = provider;
  useEffect(() => {
    let alive = true;
    setLoading(true);
    void (
      revision > 0
        ? acpRecheckProviderConnections().then(() => acpListProviderDetails())
        : acpListProviderDetails()
    )
      .then((rows) => {
        if (!alive) return;
        const usable = rows.filter(
          (row) => row.is_configured && isLocalEditionCloudProvider(row.name)
        );
        setCloudRows(rows.filter((row) => isLocalEditionCloudProvider(row.name)));
        setProviders(usable);
        setUnusable(unusableProviderReasons(rows));
        setError(null);
        // Every provider read re-asks the selected provider for its models: a key replaced in the
        // setup dialog (or a refresh) can change what the key sees.
        setListRevision((value) => value + 1);
        const pending = selectAfterSetup.current;
        if (pending && usable.some((row) => row.name === pending)) {
          selectAfterSetup.current = null;
          if (pending !== current.current) change.current(pending, '');
        }
      })
      .catch((err: unknown) => {
        if (alive)
          setError(
            `Provider configuration unavailable: ${err instanceof Error ? err.message : String(err)}`
          );
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [revision, reload]);
  const selected = providers.find((row) => row.name === provider);
  const selectedName = selected?.name ?? '';
  const catalogKey = selected?.metadata.known_models.map((m) => m.name).join('\n') ?? '';
  useEffect(() => {
    if (!selectedName) {
      setModels({ kind: 'idle' });
      return undefined;
    }
    let alive = true;
    const catalog = catalogKey ? catalogKey.split('\n') : [];
    setModels({ kind: 'reading' });
    acpListProviderLiveModels(selectedName)
      .then((live) => {
        if (!alive) return;
        // An empty answer means the provider has no listing endpoint (acp/providers.ts), never a
        // bad key — that arrives as an error.
        setModels(live.length > 0 ? { kind: 'read', models: live } : { kind: 'unlisted', catalog });
      })
      .catch((err: unknown) => {
        if (alive) setModels({ kind: 'failed', error: readError(err), catalog });
      });
    return () => {
      alive = false;
    };
  }, [selectedName, catalogKey, listRevision]);

  const openSetup = (row: ProviderDetails) => {
    setSetupError(null);
    if (row.name !== 'openai') {
      setSetup({ provider: row, overrides: [] });
      return;
    }
    // OpenAI's tile may point at another host; the dialog shows that and resets it on Connect,
    // exactly as Cloud Providers does — so the saved endpoint is read first.
    acpReadProviderConfig('openai').then(
      (fields) => setSetup({ provider: row, overrides: openAiEndpointOverrides(fields) }),
      (err: unknown) =>
        setSetupError(
          `OpenAI’s saved endpoint could not be read (${readError(err)}); set it up in Configure providers.`
        )
    );
  };
  const displayName = selected?.metadata.display_name ?? provider;
  const options = modelOptions(models, selected?.default_model || null);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label="Model provider"
              disabled={disabled || loading || !!error}
              className="flex items-center gap-3 rounded-lg border border-lz-border bg-lz-surface px-3 py-2 text-lz-ink"
            >
              {selected?.metadata.display_name ??
                (loading ? 'Loading providers…' : 'Choose configured provider')}
              <ChevronDown className="size-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="max-h-64 overflow-y-auto bg-lz-surface text-lz-ink">
            {providers.map((row) => (
              <DropdownMenuItem key={row.name} onSelect={() => onChange(row.name, '')}>
                {row.metadata.display_name}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          variant="secondary"
          disabled={disabled || loading}
          onClick={() => {
            onChange('', '');
            setRevision((value) => value + 1);
          }}
        >
          Refresh providers
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label="Set up a provider key"
              disabled={disabled || loading || !!error || cloudRows.length === 0}
              className="flex items-center gap-3 rounded-lg border border-lz-border bg-lz-surface px-3 py-2 text-lz-ink"
            >
              Set up a key
              <ChevronDown className="size-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="max-h-64 overflow-y-auto bg-lz-surface text-lz-ink">
            {cloudRows.map((row) => (
              <DropdownMenuItem key={row.name} onSelect={() => openSetup(row)}>
                {row.metadata.display_name}
                {row.is_configured ? ' · set up' : ''}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <a href="#/leanzero-swarm" className="text-lz-accent underline">
          Configure providers
        </a>
      </div>
      {error && (
        <p role="alert" className="text-lz-err">
          {error}
        </p>
      )}
      {setupError && (
        <p role="alert" className="text-lz-err">
          {setupError}
        </p>
      )}
      {!loading && !error && unusable.length > 0 && (
        <ul aria-label="Providers that cannot run" className="flex flex-col gap-1 text-lz-err">
          {unusable.map((row) => (
            <li key={row.name}>
              {row.label} is not offered: {row.reason}
            </li>
          ))}
        </ul>
      )}
      {!loading && !error && providers.length === 0 && unusable.length === 0 && (
        <p className={TYPE.bodyMuted}>
          No configured providers. Set up a key here or in Goose Swarm, then refresh.
        </p>
      )}
      <div className="flex flex-col gap-2 text-sm">
        <span>Model ID</span>
        <Combobox
          aria-label="Model ID"
          allowFreeText
          options={options}
          value={model}
          onChange={(next) => onChange(provider, next)}
          disabled={disabled || !selected || !!error}
          placeholder={selected ? `Search or type a ${displayName} model ID` : undefined}
          emptyText="Not in the list — the ID is used exactly as typed."
          className="max-w-xl"
        />
        <ModelListStatus read={models} provider={displayName} />
        <span className={TYPE.meta}>Uses this provider’s saved configuration.</span>
      </div>
      <p className={cx(TYPE.body, WEIGHT.semibold)} data-testid="bench-thinking-line">
        {BENCHMARK_THINKING_LINE}
      </p>
      {provider === 'openrouter' && selected && !error && (
        <OpenRouterHostPicker model={model} disabled={disabled} />
      )}
      {setup && (
        <CloudProviderSetupDialog
          key={setup.provider.name}
          provider={setup.provider}
          endpointOverrides={setup.overrides}
          onClose={() => setSetup(null)}
          onSaved={async () => {
            selectAfterSetup.current = setup.provider.name;
            setReload((value) => value + 1);
          }}
        />
      )}
    </div>
  );
}
