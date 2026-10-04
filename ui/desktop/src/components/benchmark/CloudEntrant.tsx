import { useEffect, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { acpListProviderDetails, acpRecheckProviderConnections } from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { Button, TYPE } from '../lz';
import { isLocalEditionCloudProvider } from '../settings/models/leanzeroSelectorPolicy';
import { providerRowState } from '../leanzero-swarm/cloudProviderState';
import { OpenRouterHostPicker } from '../openrouter/OpenRouterHostPicker';

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
        : 'its key is saved but the connection check has not passed yet — press Refresh providers';
    return [{ name: row.name, label: row.metadata.display_name, reason }];
  });
}

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
  const [providers, setProviders] = useState<ProviderDetails[]>([]);
  const [unusable, setUnusable] = useState<ReturnType<typeof unusableProviderReasons>>([]);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [loading, setLoading] = useState(true);
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
        setProviders(
          rows.filter((row) => row.is_configured && isLocalEditionCloudProvider(row.name))
        );
        setUnusable(unusableProviderReasons(rows));
        setError(null);
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
  }, [revision]);
  const selected = providers.find((row) => row.name === provider);
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
        <a href="#/leanzero-swarm" className="text-lz-accent underline">
          Configure providers
        </a>
      </div>
      {error && (
        <p role="alert" className="text-lz-err">
          {error}
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
          No configured providers. Add a provider in Goose Swarm, then return and refresh.
        </p>
      )}
      <label className="flex flex-col gap-2 text-sm">
        Model ID
        <input
          aria-label="Model ID"
          value={model}
          onChange={(event) => onChange(provider, event.target.value)}
          disabled={disabled || !selected || !!error}
          className="rounded-lg border border-lz-border bg-lz-surface px-3 py-2 text-lz-ink"
        />
        <span className={TYPE.meta}>
          Uses this provider’s saved configuration. Enter the provider’s exact model ID.
        </span>
      </label>
      {provider === 'openrouter' && selected && !error && (
        <OpenRouterHostPicker model={model} disabled={disabled} />
      )}
    </div>
  );
}
