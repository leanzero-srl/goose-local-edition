import { useCallback, useEffect, useMemo, useState } from 'react';
import { KeyRound, Loader2, RefreshCw } from 'lucide-react';
import { acpListProviderDetails, acpRecheckProviderConnections } from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import { isLocalEditionCloudProvider } from '../settings/models/leanzeroSelectorPolicy';
import { Button, Chip, SURFACE, TYPE, cx } from '../lz';
import { ToneBanner } from './studio';
import { ProviderTile } from './ProviderTile';
import CloudProviderSetupDialog from './CloudProviderSetupDialog';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  description: {
    id: 'cloudProviders.description',
    defaultMessage:
      'Credentials for the cloud providers this app can call — keys are encrypted into your goose secret store. Local backends (LM Studio, the MLX engine) need no credentials and live in the other tabs.',
  },
  recheck: { id: 'cloudProviders.recheck', defaultMessage: 'Recheck connections' },
  checking: { id: 'cloudProviders.checking', defaultMessage: 'Checking connections…' },
  loading: { id: 'cloudProviders.loading', defaultMessage: 'Loading providers…' },
  loadFailed: {
    id: 'cloudProviders.loadFailed',
    defaultMessage: 'Could not load the provider list.',
  },
  retry: { id: 'cloudProviders.retry', defaultMessage: 'Retry' },
  configured: { id: 'cloudProviders.configured', defaultMessage: 'Configured' },
  available: { id: 'cloudProviders.available', defaultMessage: 'Available to set up' },
  noneConfigured: {
    id: 'cloudProviders.noneConfigured',
    defaultMessage: 'No cloud providers configured yet. Choose one below to set it up.',
  },
  configuredCount: {
    id: 'cloudProviders.configuredCount',
    defaultMessage: '{configured} of {total} configured',
  },
  connected: { id: 'cloudProviders.connected', defaultMessage: 'Connected' },
  checkFailed: { id: 'cloudProviders.checkFailed', defaultMessage: 'Check failed' },
  noDefault: { id: 'cloudProviders.noDefault', defaultMessage: 'no default model yet' },
  notSetUp: { id: 'cloudProviders.notSetUp', defaultMessage: 'Not set up' },
  setUp: { id: 'cloudProviders.setUp', defaultMessage: 'Set up' },
  change: { id: 'cloudProviders.change', defaultMessage: 'Change' },
});

function ProviderRow({
  provider,
  onOpen,
}: {
  provider: ProviderDetails;
  onOpen: (provider: ProviderDetails) => void;
}) {
  const intl = useIntl();
  const label = provider.metadata.display_name;
  const failed = !!provider.connection_error;
  return (
    <div
      data-testid={`cloud-provider-${provider.name}`}
      className={cx(
        'flex items-center gap-3 px-3 py-2.5',
        SURFACE.card,
        failed && 'border-lz-err-solid'
      )}
    >
      <ProviderTile providerId={provider.name} label={label} size="md" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className={cx('truncate font-lz-semibold', TYPE.body)}>{label}</span>
          {failed ? (
            <Chip tone="err">{intl.formatMessage(i18n.checkFailed)}</Chip>
          ) : provider.is_configured ? (
            <Chip tone="ok">{intl.formatMessage(i18n.connected)}</Chip>
          ) : (
            <Chip>{intl.formatMessage(i18n.notSetUp)}</Chip>
          )}
        </div>
        {provider.is_configured && !failed && (
          <span className={cx('truncate', TYPE.mono)} title={provider.default_model ?? undefined}>
            {provider.default_model || (
              <span className="font-sans text-lz-warn">{intl.formatMessage(i18n.noDefault)}</span>
            )}
          </span>
        )}
        {failed && (
          <span role="alert" className="break-words text-lz-meta text-lz-err">
            {provider.connection_error}
          </span>
        )}
      </div>
      <Button
        size="sm"
        variant={provider.is_configured ? 'secondary' : 'primary'}
        icon={provider.is_configured ? undefined : <KeyRound />}
        onClick={() => onOpen(provider)}
        data-testid={`cloud-provider-open-${provider.name}`}
      >
        {intl.formatMessage(provider.is_configured ? i18n.change : i18n.setUp)}
      </Button>
    </div>
  );
}

/** Every supported cloud provider as one row: a solid tile, the connection state, the default model
 *  and one action. Setup runs in CloudProviderSetupDialog — key, then a default from the provider's
 *  own model list. */
export default function CloudProvidersSection() {
  const intl = useIntl();
  const [providers, setProviders] = useState<ProviderDetails[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [openId, setOpenId] = useState<string | null>(null);

  const loadProviders = useCallback(async () => {
    try {
      const result = await acpListProviderDetails();
      setProviders(result);
      setError(null);
    } catch (e) {
      // Failure twin: an unreachable agent must say so, never render an empty-but-clean list.
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  const cloudProviders = useMemo(
    () => (providers ?? []).filter((p) => isLocalEditionCloudProvider(p.name)),
    [providers]
  );
  const configured = cloudProviders.filter((p) => p.is_configured || p.connection_error);
  const available = cloudProviders.filter((p) => !p.is_configured && !p.connection_error);
  const configuredCount = cloudProviders.filter((p) => p.is_configured).length;
  const openProvider = openId ? (cloudProviders.find((p) => p.name === openId) ?? null) : null;

  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="cloud-providers-section">
      <div className="flex flex-wrap items-center gap-3">
        <p className={cx('max-w-[80ch]', TYPE.bodyMuted)}>
          {intl.formatMessage(i18n.description)}
        </p>
        {providers != null && (
          <Chip tone="accent" className="ml-auto">
            {intl.formatMessage(i18n.configuredCount, {
              configured: configuredCount,
              total: cloudProviders.length,
            })}
          </Chip>
        )}
      </div>

      <Button
        variant="secondary"
        className="self-start"
        disabled={checking || providers == null}
        icon={checking ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        onClick={async () => {
          setChecking(true);
          try {
            await acpRecheckProviderConnections();
            await loadProviders();
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          } finally {
            setChecking(false);
          }
        }}
      >
        {intl.formatMessage(checking ? i18n.checking : i18n.recheck)}
      </Button>

      {error != null ? (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.loadFailed)}
          text={error}
          action={
            <Button size="sm" variant="secondary" onClick={() => void loadProviders()}>
              {intl.formatMessage(i18n.retry)}
            </Button>
          }
        />
      ) : providers == null ? (
        <p className={TYPE.meta}>{intl.formatMessage(i18n.loading)}</p>
      ) : (
        <>
          <section aria-label={intl.formatMessage(i18n.configured)} className="flex flex-col gap-2">
            <h2 className={TYPE.zone}>{intl.formatMessage(i18n.configured)}</h2>
            {configured.length === 0 ? (
              <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.noneConfigured)}</p>
            ) : (
              <div className="grid gap-2 md:grid-cols-2">
                {configured.map((p) => (
                  <ProviderRow key={p.name} provider={p} onOpen={(pr) => setOpenId(pr.name)} />
                ))}
              </div>
            )}
          </section>
          <section aria-label={intl.formatMessage(i18n.available)} className="flex flex-col gap-2">
            <h2 className={TYPE.zone}>{intl.formatMessage(i18n.available)}</h2>
            <div className="grid gap-2 md:grid-cols-2">
              {available.map((p) => (
                <ProviderRow key={p.name} provider={p} onOpen={(pr) => setOpenId(pr.name)} />
              ))}
            </div>
          </section>
        </>
      )}

      {openProvider && (
        <CloudProviderSetupDialog
          key={openProvider.name}
          provider={openProvider}
          onClose={() => setOpenId(null)}
          onSaved={loadProviders}
        />
      )}
    </div>
  );
}
