import type {
  CanonicalModelInfoDto,
  CustomProviderCreateRequest_unstable,
  CustomProviderReadResponse_unstable,
  ProviderSecretDto,
  ProviderTemplateCatalogEntryDto,
  ProviderTemplateDto,
} from '@aaif/goose-sdk';
import type {
  ProviderDetails,
  ThinkingEffort,
  UpdateCustomProviderRequest,
} from '../types/providers';
import {
  CLOUD_PROVIDER_LABELS,
  isLocalEditionCloudProvider,
} from '../components/settings/models/leanzeroSelectorPolicy';
import { getAcpClient } from './acpConnection';

export type { CanonicalModelInfoDto, ProviderSecretDto };

function updateRequestToCreate(
  request: UpdateCustomProviderRequest
): CustomProviderCreateRequest_unstable {
  return {
    engine: request.engine,
    displayName: request.display_name,
    apiUrl: request.api_url,
    apiKey: request.api_key || null,
    models: request.models,
    supportsStreaming: request.supports_streaming ?? null,
    headers: request.headers ?? undefined,
    requiresAuth: request.requires_auth ?? true,
    catalogProviderId: request.catalog_provider_id ?? null,
    basePath: request.base_path ?? null,
    preservesThinking: request.preserves_thinking ?? null,
  };
}

const startupConnectionChecks = new WeakMap<object, Promise<unknown>>();

export async function acpRecheckProviderConnections(): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersConfigStatus_unstable({
    providerIds: Object.keys(CLOUD_PROVIDER_LABELS),
    checkConnections: true,
  });
}

export async function acpListProviderDetails(): Promise<ProviderDetails[]> {
  const client = await getAcpClient();
  let startupCheck = startupConnectionChecks.get(client);
  if (!startupCheck) {
    startupCheck = client.goose.providersConfigStatus_unstable({
      providerIds: Object.keys(CLOUD_PROVIDER_LABELS),
      checkConnections: true,
    });
    startupConnectionChecks.set(client, startupCheck);
    startupCheck.catch(() => startupConnectionChecks.delete(client));
  }
  await startupCheck;
  const { statuses } = await client.goose.providersConfigStatus_unstable({
    providerIds: Object.keys(CLOUD_PROVIDER_LABELS),
  });
  const { entries } = await client.goose.providersList_unstable({});
  return entries.map((entry) => ({
    name: entry.providerId,
    is_configured:
      entry.configured &&
      (!isLocalEditionCloudProvider(entry.providerId) ||
        statuses.some(
          (status) =>
            status.providerId === entry.providerId &&
            status.connectionChecked &&
            !status.connectionError
        )),
    credentials_saved: entry.configured,
    default_model: statuses.find((status) => status.providerId === entry.providerId)?.testModel,
    connection_error: statuses.find((status) => status.providerId === entry.providerId)
      ?.connectionError,
    connection_checked: statuses.find((status) => status.providerId === entry.providerId)
      ?.connectionChecked,
    provider_type: entry.providerType as ProviderDetails['provider_type'],
    metadata: {
      name: entry.providerId,
      display_name: CLOUD_PROVIDER_LABELS[entry.providerId] ?? entry.providerName,
      description: entry.description,
      default_model: entry.defaultModel,
      model_doc_link: '',
      model_selection_hint: entry.modelSelectionHint ?? null,
      config_keys: entry.configKeys
        .filter((key) => {
          if (entry.providerId === 'aws_bedrock')
            return ['AWS_BEARER_TOKEN_BEDROCK', 'AWS_REGION'].includes(key.name);
          if (entry.providerId === 'azure_openai') return key.name !== 'AZURE_OPENAI_AD_TOKEN';
          return true;
        })
        .map((key) => ({
          name: key.name,
          required:
            key.required ||
            ['AWS_BEARER_TOKEN_BEDROCK', 'AWS_REGION', 'AZURE_OPENAI_API_KEY'].includes(key.name),
          secret: key.secret,
          default: key.default ?? null,
          oauth_flow: key.oauthFlow ?? false,
          device_code_flow: key.deviceCodeFlow ?? false,
          primary: key.primary ?? false,
        })),
      known_models: entry.models.map((model) => ({
        name: model.id,
        context_limit: model.contextLimit ?? 0,
        reasoning: model.reasoning ?? undefined,
      })),
      setup_steps: entry.setupSteps,
    },
  }));
}

/** The models the saved key can see, asked of the provider RIGHT NOW (the setup dialog's default
 *  picker). An empty list means the provider has no listing endpoint, never a bad key — that is
 *  an error. */
export async function acpListProviderLiveModels(providerId: string): Promise<string[]> {
  const client = await getAcpClient();
  const { models } = await client.goose.providersSupportedModelsList_unstable({ providerId });
  return models;
}

/** Run the chosen model once and store it as the provider's default (the wire field keeps its
 *  pre-3.0.7 name `testModel`; the engine reads it as the default). */
export async function acpSaveProviderDefaultModel(
  providerId: string,
  model: string
): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersConfigSave_unstable({ providerId, fields: [], testModel: model });
}

export async function acpListProviderModels(providerId: string) {
  const client = await getAcpClient();
  const { entries } = await client.goose.providersList_unstable({ providerIds: [providerId] });
  return entries.find((e) => e.providerId === providerId)?.models ?? [];
}

export async function acpListProviderCatalogEntries(
  format?: string
): Promise<ProviderTemplateCatalogEntryDto[]> {
  const client = await getAcpClient();
  const { providers } = await client.goose.providersCatalogList_unstable(format ? { format } : {});
  return providers;
}

export async function acpGetProviderTemplate(providerId: string): Promise<ProviderTemplateDto> {
  const client = await getAcpClient();
  const { template } = await client.goose.providersCatalogTemplate_unstable({ providerId });
  return template;
}

export async function acpGetCustomProvider(
  providerId: string
): Promise<CustomProviderReadResponse_unstable> {
  const client = await getAcpClient();
  return client.goose.providersCustomRead_unstable({ providerId });
}

export async function acpCreateCustomProviderFromRequest(
  request: UpdateCustomProviderRequest
): Promise<{ provider_name: string }> {
  const client = await getAcpClient();
  const response = await client.goose.providersCustomCreate_unstable(
    updateRequestToCreate(request)
  );
  return { provider_name: response.providerId };
}

export async function acpUpdateCustomProviderFromRequest(
  providerId: string,
  request: UpdateCustomProviderRequest
): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersCustomUpdate_unstable({
    providerId,
    ...updateRequestToCreate(request),
  });
}

export async function acpDeleteCustomProvider(providerId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersCustomDelete_unstable({ providerId });
}

export async function acpReadProviderConfig(providerId: string) {
  const client = await getAcpClient();
  const { fields } = await client.goose.providersConfigRead_unstable({ providerId });
  return fields;
}

export async function acpDeleteProviderConfig(providerId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersConfigDelete_unstable({ providerId });
}

/** Save credential fields; the engine verifies the key by listing the provider's models (or by
 *  running the saved default when one exists) before the new values stick. */
export async function acpSaveProviderConfig(
  providerId: string,
  fields: { key: string; value: string }[]
): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersConfigSave_unstable({ providerId, fields });
}

export async function acpAuthenticateProvider(providerId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersConfigAuthenticate_unstable({ providerId });
}

export async function acpListProviderSecrets(): Promise<ProviderSecretDto[]> {
  const client = await getAcpClient();
  const { secrets } = await client.goose.providersSecretsList_unstable({});
  return secrets;
}

export async function acpDeleteProviderSecret(id: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.providersSecretsDelete_unstable({ id });
}

export async function acpGetCanonicalModelInfo(
  provider: string,
  model: string
): Promise<CanonicalModelInfoDto | null> {
  const client = await getAcpClient();
  const { modelInfo } = await client.goose.providersCanonicalModelInfo_unstable({
    provider,
    model,
  });
  return modelInfo ?? null;
}

export async function acpReadDefaults(): Promise<{
  providerId: string | null;
  modelId: string | null;
}> {
  const client = await getAcpClient();
  const response = await client.goose.defaultsRead_unstable({});
  return {
    providerId: response.providerId ?? null,
    modelId: response.modelId ?? null,
  };
}

export async function acpSaveDefaults(providerId: string, modelId?: string | null): Promise<void> {
  const client = await getAcpClient();
  await client.goose.defaultsSave_unstable({ providerId, modelId: modelId ?? null });
}

export async function acpClearDefaults(): Promise<void> {
  const client = await getAcpClient();
  await client.goose.defaultsClear_unstable({});
}

export async function acpReadThinkingEffort(): Promise<ThinkingEffort | null> {
  const client = await getAcpClient();
  const response = await client.goose.preferencesRead_unstable({ keys: ['gooseThinkingEffort'] });
  const value = response.values.find((v) => v.key === 'gooseThinkingEffort')?.value;
  return typeof value === 'string' ? (value as ThinkingEffort) : null;
}

export async function acpSaveThinkingEffort(effort: ThinkingEffort): Promise<void> {
  const client = await getAcpClient();
  await client.goose.preferencesSave_unstable({
    values: [{ key: 'gooseThinkingEffort', value: effort }],
  });
}

export type AppliedSessionProviderModel = {
  providerId?: string;
  modelId?: string;
};

function extractAppliedSessionProviderModel(configOptions: unknown): AppliedSessionProviderModel {
  if (!Array.isArray(configOptions)) {
    return {};
  }

  const applied: AppliedSessionProviderModel = {};

  for (const option of configOptions) {
    if (!option || typeof option !== 'object') {
      continue;
    }

    const id = 'id' in option ? option.id : undefined;
    if (id !== 'provider' && id !== 'model') {
      continue;
    }

    const currentValue = selectCurrentValue(option);
    if (typeof currentValue !== 'string') {
      continue;
    }

    if (id === 'provider') {
      applied.providerId = currentValue;
    } else {
      applied.modelId = currentValue;
    }
  }

  return applied;
}

function selectCurrentValue(kind: unknown): unknown {
  if (!kind || typeof kind !== 'object') {
    return undefined;
  }

  if ('type' in kind && kind.type === 'select' && 'currentValue' in kind) {
    return kind.currentValue;
  }

  return undefined;
}

/**
 * Switch the provider (and model) for an active session via ACP config options.
 *
 * Changing the provider on the server resets the session's model, so the model
 * is applied as a follow-up step when supplied.
 */
export async function acpSetSessionProviderModel(
  sessionId: string,
  providerId: string,
  modelId?: string | null,
  thinkingEffort?: ThinkingEffort | null
): Promise<AppliedSessionProviderModel> {
  const client = await getAcpClient();
  let response = await client.setSessionConfigOption({
    sessionId,
    configId: 'provider',
    value: providerId,
  });
  if (modelId) {
    response = await client.setSessionConfigOption({
      sessionId,
      configId: 'model',
      value: modelId,
    });
  }
  if (thinkingEffort != null) {
    response = await client.setSessionConfigOption({
      sessionId,
      configId: 'thinking_effort',
      value: thinkingEffort,
    });
  }

  return extractAppliedSessionProviderModel(response.configOptions);
}
