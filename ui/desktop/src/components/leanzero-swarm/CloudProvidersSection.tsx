import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { KeyRound, Loader2, Plus, RefreshCw } from 'lucide-react';
import type { CustomProviderConfigDto } from '@aaif/goose-sdk';
import {
  acpGetCustomProvider,
  acpListProviderDetails,
  acpReadProviderConfig,
  acpRecheckProviderConnections,
} from '../../acp/providers';
import type { ProviderDetails } from '../../types/providers';
import {
  isLocalEditionCloudProvider,
  isUserEndpoint,
} from '../settings/models/leanzeroSelectorPolicy';
import { Button, Chip, SURFACE, TYPE, cx } from '../lz';
import { ToneBanner } from './studio';
import { OPENAI_COMPATIBLE_TILE, ProviderTile } from './ProviderTile';
import CloudProviderSetupDialog from './CloudProviderSetupDialog';
import CompatibleEndpointDialog from './CompatibleEndpointDialog';
import { partitionProviderRows, providerRowState } from './cloudProviderState';
import { openAiEndpointOverrides, type EndpointOverride } from './openaiEndpoint';
import { errorMessage } from '../../utils/conversionUtils';
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
  notChecked: { id: 'cloudProviders.notChecked', defaultMessage: 'Not checked yet' },
  noDefault: { id: 'cloudProviders.noDefault', defaultMessage: 'no default model yet' },
  notSetUp: { id: 'cloudProviders.notSetUp', defaultMessage: 'Not set up' },
  setUp: { id: 'cloudProviders.setUp', defaultMessage: 'Set up' },
  change: { id: 'cloudProviders.change', defaultMessage: 'Change' },
  notOfficial: { id: 'cloudProviders.notOfficial', defaultMessage: 'Not the official API' },
  notOfficialText: {
    id: 'cloudProviders.notOfficialText',
    defaultMessage: 'Requests go to {where}, not api.openai.com.',
  },
  endpointUnread: {
    id: 'cloudProviders.endpointUnread',
    defaultMessage: 'Could not read where these requests go: {error}',
  },
  compatible: { id: 'cloudProviders.compatible', defaultMessage: 'OpenAI-compatible' },
  compatibleHint: {
    id: 'cloudProviders.compatibleHint',
    defaultMessage:
      'Any server that speaks the OpenAI API — vLLM, llama.cpp, a remote LM Studio, Together, Groq, a company gateway. Add as many as you need. Chat sessions can use them; swarm nodes cannot yet.',
  },
  chatOnly: { id: 'cloudProviders.chatOnly', defaultMessage: 'Chat only' },
  chatOnlyHint: {
    id: 'cloudProviders.chatOnlyHint',
    defaultMessage: 'Chat sessions can use this endpoint; swarm nodes cannot yet.',
  },
  addEndpoint: { id: 'cloudProviders.addEndpoint', defaultMessage: 'Add endpoint' },
});

type Open =
  | { kind: 'cloud'; id: string }
  | { kind: 'endpoint'; id: string }
  | { kind: 'new-endpoint' }
  | null;

/** What the section read about one endpoint's stored settings — the config, or why it is missing. */
type EndpointConfig = { config: CustomProviderConfigDto } | { error: string };

/** Where the official OpenAI tile's requests go — the overrides, or why they could not be read. */
type OpenAiEndpoint = { overrides: EndpointOverride[] } | { error: string };

function StateChip({ provider }: { provider: ProviderDetails }) {
  const intl = useIntl();
  switch (providerRowState(provider)) {
    case 'failed':
      return <Chip tone="err">{intl.formatMessage(i18n.checkFailed)}</Chip>;
    case 'connected':
      return <Chip tone="ok">{intl.formatMessage(i18n.connected)}</Chip>;
    case 'unchecked':
      return <Chip tone="warn">{intl.formatMessage(i18n.notChecked)}</Chip>;
    case 'not-set-up':
      return <Chip>{intl.formatMessage(i18n.notSetUp)}</Chip>;
  }
}

function ProviderRow({
  provider,
  tileId,
  detail,
  warning,
  onOpen,
}: {
  provider: ProviderDetails;
  tileId: string;
  /** A line under the name (an endpoint's URL, a chat-only note). */
  detail?: ReactNode;
  /** A loud line that must not be missed (the official tile pointing elsewhere). */
  warning?: string | null;
  onOpen: () => void;
}) {
  const intl = useIntl();
  const label = provider.metadata.display_name;
  const state = providerRowState(provider);
  const listed = state !== 'not-set-up';
  return (
    <div
      data-testid={`cloud-provider-${provider.name}`}
      data-state={state}
      className={cx(
        'flex items-center gap-3 px-3 py-2.5',
        SURFACE.card,
        (state === 'failed' || warning) && 'border-lz-err-solid'
      )}
    >
      <ProviderTile providerId={tileId} label={label} size="md" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className={cx('truncate font-lz-semibold', TYPE.body)}>{label}</span>
          <StateChip provider={provider} />
        </div>
        {detail}
        {listed && state !== 'failed' && (
          <span className={cx('truncate', TYPE.mono)} title={provider.default_model ?? undefined}>
            {provider.default_model || (
              <span className="font-sans text-lz-warn">{intl.formatMessage(i18n.noDefault)}</span>
            )}
          </span>
        )}
        {state === 'failed' && (
          <span role="alert" className="break-words text-lz-meta text-lz-err">
            {provider.connection_error}
          </span>
        )}
        {warning && (
          <span
            role="alert"
            data-testid={`cloud-provider-warning-${provider.name}`}
            className="break-words text-lz-meta font-lz-semibold text-lz-err"
          >
            {warning}
          </span>
        )}
      </div>
      <Button
        size="sm"
        variant={listed ? 'secondary' : 'primary'}
        icon={listed ? undefined : <KeyRound />}
        onClick={onOpen}
        data-testid={`cloud-provider-open-${provider.name}`}
      >
        {intl.formatMessage(listed ? i18n.change : i18n.setUp)}
      </Button>
    </div>
  );
}

/** Every supported cloud provider as one row — plus the OpenAI-compatible endpoints the person added,
 *  each its own row. One rule (cloudProviderState.ts) decides a row's chip, its list and the header
 *  count. Setup runs in CloudProviderSetupDialog (key, then a default from the provider's own model
 *  list) or CompatibleEndpointDialog (address, optional key and headers, then a default). */
export default function CloudProvidersSection() {
  const intl = useIntl();
  const [providers, setProviders] = useState<ProviderDetails[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [open, setOpen] = useState<Open>(null);
  const [endpointConfigs, setEndpointConfigs] = useState<Record<string, EndpointConfig>>({});
  const [openAiEndpoint, setOpenAiEndpoint] = useState<OpenAiEndpoint>({ overrides: [] });

  const loadProviders = useCallback(async () => {
    try {
      const result = await acpListProviderDetails();
      const endpoints = result.filter(isUserEndpoint);
      const [configs, openai] = await Promise.all([
        Promise.all(
          endpoints.map(async (p): Promise<[string, EndpointConfig]> => {
            try {
              return [p.name, { config: (await acpGetCustomProvider(p.name)).provider }];
            } catch (e) {
              return [p.name, { error: errorMessage(e) }];
            }
          })
        ),
        acpReadProviderConfig('openai').then(
          (fields): OpenAiEndpoint => ({ overrides: openAiEndpointOverrides(fields) }),
          (e): OpenAiEndpoint => ({ error: errorMessage(e) })
        ),
      ]);
      setProviders(result);
      setEndpointConfigs(Object.fromEntries(configs));
      setOpenAiEndpoint(openai);
      setError(null);
    } catch (e) {
      // Failure twin: an unreachable agent must say so, never render an empty-but-clean list.
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void loadProviders();
  }, [loadProviders]);

  const rows = useMemo(
    () =>
      (providers ?? [])
        .filter((p) => isLocalEditionCloudProvider(p.name) || isUserEndpoint(p))
        .sort((a, b) => a.metadata.display_name.localeCompare(b.metadata.display_name)),
    [providers]
  );
  const { configured, available, count } = partitionProviderRows(rows);
  const endpointIds = rows.filter(isUserEndpoint).map((p) => p.name);

  const openAiWarning =
    'error' in openAiEndpoint
      ? intl.formatMessage(i18n.endpointUnread, { error: openAiEndpoint.error })
      : openAiEndpoint.overrides.length > 0
        ? `${intl.formatMessage(i18n.notOfficial)} — ${intl.formatMessage(i18n.notOfficialText, {
            where: openAiEndpoint.overrides.map((o) => `${o.key}=${o.value}`).join(', '),
          })}`
        : null;

  const renderRow = (p: ProviderDetails) => {
    if (isUserEndpoint(p)) {
      const read = endpointConfigs[p.name];
      return (
        <ProviderRow
          key={p.name}
          provider={p}
          tileId={OPENAI_COMPATIBLE_TILE}
          detail={
            <>
              {read && 'config' in read && (
                <span className={cx('truncate', TYPE.mono)} title={read.config.apiUrl}>
                  {read.config.apiUrl}
                </span>
              )}
              <span className="flex min-w-0 items-center gap-2">
                <Chip>{intl.formatMessage(i18n.compatible)}</Chip>
                <Chip tone="warn" title={intl.formatMessage(i18n.chatOnlyHint)}>
                  {intl.formatMessage(i18n.chatOnly)}
                </Chip>
              </span>
            </>
          }
          warning={
            read && 'error' in read
              ? intl.formatMessage(i18n.endpointUnread, { error: read.error })
              : null
          }
          onOpen={() =>
            read && 'config' in read
              ? setOpen({ kind: 'endpoint', id: p.name })
              : void loadProviders()
          }
        />
      );
    }
    return (
      <ProviderRow
        key={p.name}
        provider={p}
        tileId={p.name}
        warning={p.name === 'openai' ? openAiWarning : null}
        onOpen={() => setOpen({ kind: 'cloud', id: p.name })}
      />
    );
  };

  const openCloud = open?.kind === 'cloud' ? (rows.find((p) => p.name === open.id) ?? null) : null;
  const openEndpoint = open?.kind === 'endpoint' ? rows.find((p) => p.name === open.id) : undefined;
  const openEndpointRead = openEndpoint ? endpointConfigs[openEndpoint.name] : undefined;

  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="cloud-providers-section">
      <div className="flex flex-wrap items-center gap-3">
        <p className={cx('max-w-[80ch]', TYPE.bodyMuted)}>{intl.formatMessage(i18n.description)}</p>
        {providers != null && (
          <Chip tone="accent" className="ml-auto">
            {intl.formatMessage(i18n.configuredCount, count)}
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
            await acpRecheckProviderConnections(endpointIds);
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
              <div className="grid gap-2 md:grid-cols-2">{configured.map(renderRow)}</div>
            )}
          </section>
          <section aria-label={intl.formatMessage(i18n.available)} className="flex flex-col gap-2">
            <h2 className={TYPE.zone}>{intl.formatMessage(i18n.available)}</h2>
            <div className="grid gap-2 md:grid-cols-2">
              <div
                data-testid="cloud-provider-add-compatible"
                className={cx('flex items-center gap-3 px-3 py-2.5', SURFACE.card)}
              >
                <ProviderTile
                  providerId={OPENAI_COMPATIBLE_TILE}
                  label={intl.formatMessage(i18n.compatible)}
                  size="md"
                />
                <div className="flex min-w-0 flex-1 flex-col gap-1">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className={cx('truncate font-lz-semibold', TYPE.body)}>
                      {intl.formatMessage(i18n.compatible)}
                    </span>
                    <Chip tone="warn" title={intl.formatMessage(i18n.chatOnlyHint)}>
                      {intl.formatMessage(i18n.chatOnly)}
                    </Chip>
                  </div>
                  <span className={TYPE.meta}>{intl.formatMessage(i18n.compatibleHint)}</span>
                </div>
                <Button
                  size="sm"
                  variant="primary"
                  icon={<Plus />}
                  onClick={() => setOpen({ kind: 'new-endpoint' })}
                  data-testid="cloud-provider-add-compatible-open"
                >
                  {intl.formatMessage(i18n.addEndpoint)}
                </Button>
              </div>
              {available.map(renderRow)}
            </div>
          </section>
        </>
      )}

      {openCloud && (
        <CloudProviderSetupDialog
          key={openCloud.name}
          provider={openCloud}
          endpointOverrides={
            openCloud.name === 'openai' && 'overrides' in openAiEndpoint
              ? openAiEndpoint.overrides
              : []
          }
          onClose={() => setOpen(null)}
          onSaved={loadProviders}
        />
      )}
      {open?.kind === 'new-endpoint' && (
        <CompatibleEndpointDialog onClose={() => setOpen(null)} onSaved={loadProviders} />
      )}
      {openEndpoint && openEndpointRead && 'config' in openEndpointRead && (
        <CompatibleEndpointDialog
          key={openEndpoint.name}
          endpoint={{ provider: openEndpoint, config: openEndpointRead.config }}
          onClose={() => setOpen(null)}
          onSaved={loadProviders}
        />
      )}
    </div>
  );
}
