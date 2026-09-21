import { useEffect, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { acpListProviderDetails } from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { Button, TYPE } from '../lz';
import { isLocalEditionCloudProvider } from '../settings/models/leanzeroSelectorPolicy';

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
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    void acpListProviderDetails()
      .then((rows) => {
        if (!alive) return;
        setProviders(
          rows.filter((row) => row.is_configured && isLocalEditionCloudProvider(row.name))
        );
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
      {!loading && !error && providers.length === 0 && (
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
    </div>
  );
}
