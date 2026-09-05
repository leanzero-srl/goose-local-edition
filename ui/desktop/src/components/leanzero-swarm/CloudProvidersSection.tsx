import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import ProviderGrid from '../settings/providers/ProviderGrid';
import { acpListProviderDetails } from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import { isLocalEditionCloudProvider } from '../settings/models/leanzeroSelectorPolicy';
import { createNavigationHandler } from '../../utils/navigationUtils';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  description: {
    id: 'cloudProviders.description',
    defaultMessage:
      'Credentials for the cloud providers this app can call — keys are encrypted into your goose secret store. Local backends (LM Studio, the MLX engine) need no credentials and live in the other tabs.',
  },
  loading: { id: 'cloudProviders.loading', defaultMessage: 'Loading providers…' },
  loadFailed: {
    id: 'cloudProviders.loadFailed',
    defaultMessage: 'Could not load the provider list.',
  },
  retry: { id: 'cloudProviders.retry', defaultMessage: 'Retry' },
  configuredCount: {
    id: 'cloudProviders.configuredCount',
    defaultMessage: '{configured} of {total} configured',
  },
});

/**
 * The Cloud Providers tab — the provider-credential experience relocated from Settings, filtered
 * to the swarm's FOUR cloud families (`isLocalEditionCloudProvider`, joined on registry id: the
 * only cloud providers this edition can chat through; Swarm itself needs no credentials). REUSES ProviderGrid wholesale — the cards, ProviderConfigurationModal
 * (key entry / authenticate / delete over the acp provider-config surface) and the custom-provider
 * form are the exact components Settings used; nothing is forked.
 */
export default function CloudProvidersSection() {
  const intl = useIntl();
  const navigate = useNavigate();
  const [providers, setProviders] = useState<ProviderDetails[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const initialLoadDone = useRef(false);

  const setView = useMemo(() => createNavigationHandler(navigate), [navigate]);

  const loadProviders = useCallback(async () => {
    try {
      const result = await acpListProviderDetails();
      setProviders(result);
      setError(null);
      initialLoadDone.current = true;
    } catch (e) {
      // Failure twin: an unreachable agent must say so, never render an empty-but-clean grid.
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  const refreshProviders = useCallback(async () => {
    if (!initialLoadDone.current) return;
    try {
      const result = await acpListProviderDetails();
      setProviders(result);
    } catch {
      // keep the last real list; the banner from the initial load already covers a dead agent
    }
  }, []);

  const cloudProviders = useMemo(
    () => (providers ?? []).filter((p) => isLocalEditionCloudProvider(p.name)),
    [providers]
  );
  const configuredCount = cloudProviders.filter((p) => p.is_configured).length;

  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="cloud-providers-section">
      <div className="flex flex-wrap items-center gap-3">
        <p className="max-w-[80ch] text-sm text-text-secondary">
          {intl.formatMessage(i18n.description)}
        </p>
        {providers != null && (
          <span
            className="ml-auto shrink-0 rounded px-2 py-0.5 text-xs font-bold text-white"
            style={{ backgroundColor: '#2e8bff' }}
          >
            {intl.formatMessage(i18n.configuredCount, {
              configured: configuredCount,
              total: cloudProviders.length,
            })}
          </span>
        )}
      </div>

      {error != null ? (
        <div
          className="flex items-center gap-3 rounded px-4 py-3 text-sm font-semibold text-white"
          style={{ backgroundColor: '#e5484d' }}
          role="alert"
        >
          <span className="min-w-0 flex-1 break-words">
            {intl.formatMessage(i18n.loadFailed)} {error}
          </span>
          <button
            type="button"
            onClick={() => void loadProviders()}
            className="shrink-0 rounded bg-white/20 px-2.5 py-1 text-xs font-bold hover:bg-white/30"
          >
            {intl.formatMessage(i18n.retry)}
          </button>
        </div>
      ) : providers == null ? (
        <div className="text-sm text-text-secondary">{intl.formatMessage(i18n.loading)}</div>
      ) : (
        <ProviderGrid
          providers={cloudProviders}
          isOnboarding={false}
          refreshProviders={() => void refreshProviders()}
          setView={setView}
          allowCustomProvider={false}
        />
      )}
    </div>
  );
}
