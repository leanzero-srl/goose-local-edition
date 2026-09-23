// This file is auto-generated — do not edit manually.

export interface ExtMethodProvider {
  extMethod(
    method: string,
    params: Record<string, unknown>,
  ): Promise<Record<string, unknown>>;
}

import type { Client } from "@agentclientprotocol/sdk";
import type {
  AddConfigExtensionRequest_unstable,
  AddSessionExtensionRequest_unstable,
  AnswerMemoryProposalRequest_unstable,
  AnswerMemoryProposalResponse_unstable,
  AppsExportRequest_unstable,
  AppsExportResponse_unstable,
  AppsImportRequest_unstable,
  AppsImportResponse_unstable,
  AppsListRequest_unstable,
  AppsListResponse_unstable,
  ArchiveSessionRequest_unstable,
  CanonicalModelInfoRequest_unstable,
  CanonicalModelInfoResponse_unstable,
  ConfigReadAllRequest_unstable,
  ConfigReadAllResponse_unstable,
  ConfigReadRequest_unstable,
  ConfigReadResponse_unstable,
  ConfigRemoveRequest_unstable,
  ConfigUpsertRequest_unstable,
  CreateRecipeRequest_unstable,
  CreateRecipeResponse_unstable,
  CreateScheduleRequest_unstable,
  CreateScheduleResponse_unstable,
  CreateSourceRequest_unstable,
  CreateSourceResponse_unstable,
  CustomProviderCreateRequest_unstable,
  CustomProviderCreateResponse_unstable,
  CustomProviderDeleteRequest_unstable,
  CustomProviderDeleteResponse_unstable,
  CustomProviderReadRequest_unstable,
  CustomProviderReadResponse_unstable,
  CustomProviderUpdateRequest_unstable,
  CustomProviderUpdateResponse_unstable,
  DecodeRecipeRequest_unstable,
  DecodeRecipeResponse_unstable,
  DefaultsClearRequest_unstable,
  DefaultsReadRequest_unstable,
  DefaultsReadResponse_unstable,
  DefaultsSaveRequest_unstable,
  DeleteRecipeRequest_unstable,
  DeleteScheduleRequest_unstable,
  DeleteSessionRequest,
  DeleteSourceRequest_unstable,
  DiagnosticsGetRequest_unstable,
  DiagnosticsGetResponse_unstable,
  DictationConfigRequest_unstable,
  DictationConfigResponse_unstable,
  DictationModelCancelRequest_unstable,
  DictationModelDeleteRequest_unstable,
  DictationModelDownloadProgressRequest_unstable,
  DictationModelDownloadProgressResponse_unstable,
  DictationModelDownloadRequest_unstable,
  DictationModelSelectRequest_unstable,
  DictationModelsListRequest_unstable,
  DictationModelsListResponse_unstable,
  DictationSecretDeleteRequest_unstable,
  DictationSecretSaveRequest_unstable,
  DictationTranscribeRequest_unstable,
  DictationTranscribeResponse_unstable,
  EncodeRecipeRequest_unstable,
  EncodeRecipeResponse_unstable,
  ExportSessionRequest_unstable,
  ExportSessionResponse_unstable,
  ExportSourceRequest_unstable,
  ExportSourceResponse_unstable,
  GetAvailableExtensionsRequest_unstable,
  GetAvailableExtensionsResponse_unstable,
  GetConfigExtensionsRequest_unstable,
  GetConfigExtensionsResponse_unstable,
  GetPromptRequest_unstable,
  GetPromptResponse_unstable,
  GetSessionExtensionsRequest_unstable,
  GetSessionExtensionsResponse_unstable,
  GetSessionInfoRequest_unstable,
  GetSessionInfoResponse_unstable,
  GetToolsRequest_unstable,
  GetToolsResponse_unstable,
  GooseSessionNotification_unstable,
  GooseToolCallRequest_unstable,
  GooseToolCallResponse_unstable,
  ImportSessionRequest_unstable,
  ImportSessionResponse_unstable,
  ImportSourcesRequest_unstable,
  ImportSourcesResponse_unstable,
  InspectConfigExtensionRequest_unstable,
  InspectConfigExtensionResponse_unstable,
  InspectRunningJobRequest_unstable,
  InspectRunningJobResponse_unstable,
  KillRunningJobRequest_unstable,
  KillRunningJobResponse_unstable,
  LeanzeroLinkConnectRequest_unstable,
  LeanzeroLinkHealthRequest_unstable,
  LeanzeroLinkHealthResponse_unstable,
  LeanzeroLinkLogoutRequest_unstable,
  LeanzeroLinkNodesRequest_unstable,
  LeanzeroLinkNodesResponse_unstable,
  LeanzeroLinkRemoteExecuteRequest_unstable,
  LeanzeroLinkRemoteExecuteResponse_unstable,
  LeanzeroLinkRequestCodeRequest_unstable,
  LeanzeroLinkRequestCodeResponse_unstable,
  LeanzeroLinkStateResponse_unstable,
  LeanzeroLinkStatusRequest_unstable,
  LeanzeroLinkVerifyRequest_unstable,
  LeanzeroLinkVerifyResponse_unstable,
  ListAgentMentionsRequest_unstable,
  ListAgentMentionsResponse_unstable,
  ListMemoryProposalsRequest_unstable,
  ListMemoryProposalsResponse_unstable,
  ListPromptsRequest_unstable,
  ListPromptsResponse_unstable,
  ListProvidersRequest_unstable,
  ListProvidersResponse_unstable,
  ListRecipesRequest_unstable,
  ListRecipesResponse_unstable,
  ListScheduleSessionsRequest_unstable,
  ListScheduleSessionsResponse_unstable,
  ListSchedulesRequest_unstable,
  ListSchedulesResponse_unstable,
  ListSlashCommandsRequest_unstable,
  ListSlashCommandsResponse_unstable,
  ListSourcesRequest_unstable,
  ListSourcesResponse_unstable,
  LocalInferenceBuiltinChatTemplatesListRequest_unstable,
  LocalInferenceBuiltinChatTemplatesListResponse_unstable,
  LocalInferenceHuggingFaceRepoVariantsRequest_unstable,
  LocalInferenceHuggingFaceRepoVariantsResponse_unstable,
  LocalInferenceHuggingFaceSearchRequest_unstable,
  LocalInferenceHuggingFaceSearchResponse_unstable,
  LocalInferenceModelDeleteRequest_unstable,
  LocalInferenceModelDownloadCancelRequest_unstable,
  LocalInferenceModelDownloadProgressRequest_unstable,
  LocalInferenceModelDownloadProgressResponse_unstable,
  LocalInferenceModelDownloadRequest_unstable,
  LocalInferenceModelDownloadResponse_unstable,
  LocalInferenceModelSettingsReadRequest_unstable,
  LocalInferenceModelSettingsReadResponse_unstable,
  LocalInferenceModelSettingsUpdateRequest_unstable,
  LocalInferenceModelSettingsUpdateResponse_unstable,
  LocalInferenceModelsListRequest_unstable,
  LocalInferenceModelsListResponse_unstable,
  MlxEngineBrowseFiltersRequest_unstable,
  MlxEngineBrowseFiltersResponse_unstable,
  MlxEngineBrowseRequest_unstable,
  MlxEngineBrowseResponse_unstable,
  MlxEngineDistributedConfigResponse_unstable,
  MlxEngineDistributedConfigUpdateRequest_unstable,
  MlxEngineDistributedDiscoverRequest_unstable,
  MlxEngineDistributedDiscoverResponse_unstable,
  MlxEngineDistributedPeerCandidatesRequest_unstable,
  MlxEngineDistributedPeerCandidatesResponse_unstable,
  MlxEngineDistributedPreflightRequest_unstable,
  MlxEngineDistributedPreflightResponse_unstable,
  MlxEngineDistributedProvisionRequest_unstable,
  MlxEngineDistributedProvisionResponse_unstable,
  MlxEngineDistributedStartRequest_unstable,
  MlxEngineDistributedStartResponse_unstable,
  MlxEngineDistributedStatusRequest_unstable,
  MlxEngineDistributedStatusResponse_unstable,
  MlxEngineDistributedStopRequest_unstable,
  MlxEngineDistributedStopResponse_unstable,
  MlxEngineDownloadCancelRequest_unstable,
  MlxEngineDownloadPauseRequest_unstable,
  MlxEngineDownloadProgressRequest_unstable,
  MlxEngineDownloadProgressResponse_unstable,
  MlxEngineDownloadRequest_unstable,
  MlxEngineDownloadResumeRequest_unstable,
  MlxEngineHfSearchRequest_unstable,
  MlxEngineHfSearchResponse_unstable,
  MlxEngineLinkFactsRequest_unstable,
  MlxEngineLinkFactsResponse_unstable,
  MlxEngineModelCardRequest_unstable,
  MlxEngineModelCardResponse_unstable,
  MlxEngineModelDeleteRequest_unstable,
  MlxEngineModelsListRequest_unstable,
  MlxEngineModelsListResponse_unstable,
  MlxEngineMountRequest_unstable,
  MlxEngineReplicaCancelRequest_unstable,
  MlxEngineReplicaProgressRequest_unstable,
  MlxEngineReplicaProgressResponse_unstable,
  MlxEngineReplicaPullRequest_unstable,
  MlxEngineReplicaTargetsRequest_unstable,
  MlxEngineReplicaTargetsResponse_unstable,
  MlxEngineReplicateRequest_unstable,
  MlxEngineReplicateResponse_unstable,
  MlxEngineSettingsReadRequest_unstable,
  MlxEngineSettingsResponse_unstable,
  MlxEngineSettingsUpdateRequest_unstable,
  MlxEngineStatusRequest_unstable,
  MlxEngineStatusResponse_unstable,
  MlxEngineUnmountRequest_unstable,
  OnboardingImportApplyRequest_unstable,
  OnboardingImportApplyResponse_unstable,
  OnboardingImportScanRequest_unstable,
  OnboardingImportScanResponse_unstable,
  ParseRecipeRequest_unstable,
  ParseRecipeResponse_unstable,
  PauseScheduleRequest_unstable,
  PreferencesReadRequest_unstable,
  PreferencesReadResponse_unstable,
  PreferencesRemoveRequest_unstable,
  PreferencesSaveRequest_unstable,
  PromptOperationResponse_unstable,
  ProviderCatalogListRequest_unstable,
  ProviderCatalogListResponse_unstable,
  ProviderCatalogTemplateRequest_unstable,
  ProviderCatalogTemplateResponse_unstable,
  ProviderConfigAuthenticateRequest_unstable,
  ProviderConfigChangeResponse_unstable,
  ProviderConfigDeleteRequest_unstable,
  ProviderConfigReadRequest_unstable,
  ProviderConfigReadResponse_unstable,
  ProviderConfigSaveRequest_unstable,
  ProviderConfigStatusRequest_unstable,
  ProviderConfigStatusResponse_unstable,
  ProviderSecretDeleteRequest_unstable,
  ProviderSecretsListRequest_unstable,
  ProviderSecretsListResponse_unstable,
  ProviderSetupCatalogListRequest_unstable,
  ProviderSetupCatalogListResponse_unstable,
  ProviderSupportedModelsListRequest_unstable,
  ProviderSupportedModelsListResponse_unstable,
  ReadResourceRequest_unstable,
  ReadResourceResponse_unstable,
  RecipeParamsResponse_unstable,
  RecipeToYamlRequest_unstable,
  RecipeToYamlResponse_unstable,
  RefreshProviderInventoryRequest_unstable,
  RefreshProviderInventoryResponse_unstable,
  RemoveConfigExtensionRequest_unstable,
  RemoveSessionExtensionRequest_unstable,
  RenameSessionRequest_unstable,
  RequestRecipeParams_unstable,
  ResetPromptRequest_unstable,
  RunScheduleNowRequest_unstable,
  RunScheduleNowResponse_unstable,
  SavePromptRequest_unstable,
  SaveRecipeRequest_unstable,
  SaveRecipeResponse_unstable,
  ScanRecipeRequest_unstable,
  ScanRecipeResponse_unstable,
  ScheduleRecipeRequest_unstable,
  SetConfigExtensionEnabledRequest_unstable,
  SetRecipeSlashCommandRequest_unstable,
  SetSessionSystemPromptRequest_unstable,
  SetToolPermissionsRequest_unstable,
  SetToolPermissionsResponse_unstable,
  ShareSessionNostrRequest_unstable,
  ShareSessionNostrResponse_unstable,
  SteerSessionRequest_unstable,
  SteerSessionResponse_unstable,
  TruncateSessionConversationRequest_unstable,
  UnarchiveSessionRequest_unstable,
  UnpauseScheduleRequest_unstable,
  UpdateScheduleRequest_unstable,
  UpdateScheduleResponse_unstable,
  UpdateSessionProjectRequest_unstable,
  UpdateSourceRequest_unstable,
  UpdateSourceResponse_unstable,
  UpdateWorkingDirRequest_unstable,
} from './types.gen.js';
import {
  zAnswerMemoryProposalResponse_unstable,
  zAppsExportResponse_unstable,
  zAppsImportResponse_unstable,
  zAppsListResponse_unstable,
  zCanonicalModelInfoResponse_unstable,
  zConfigReadAllResponse_unstable,
  zConfigReadResponse_unstable,
  zCreateRecipeResponse_unstable,
  zCreateScheduleResponse_unstable,
  zCreateSourceResponse_unstable,
  zCustomProviderCreateResponse_unstable,
  zCustomProviderDeleteResponse_unstable,
  zCustomProviderReadResponse_unstable,
  zCustomProviderUpdateResponse_unstable,
  zDecodeRecipeResponse_unstable,
  zDefaultsReadResponse_unstable,
  zDiagnosticsGetResponse_unstable,
  zDictationConfigResponse_unstable,
  zDictationModelDownloadProgressResponse_unstable,
  zDictationModelsListResponse_unstable,
  zDictationTranscribeResponse_unstable,
  zEncodeRecipeResponse_unstable,
  zExportSessionResponse_unstable,
  zExportSourceResponse_unstable,
  zGetAvailableExtensionsResponse_unstable,
  zGetConfigExtensionsResponse_unstable,
  zGetPromptResponse_unstable,
  zGetSessionExtensionsResponse_unstable,
  zGetSessionInfoResponse_unstable,
  zGetToolsResponse_unstable,
  zGooseSessionNotification_unstable,
  zGooseToolCallResponse_unstable,
  zImportSessionResponse_unstable,
  zImportSourcesResponse_unstable,
  zInspectConfigExtensionResponse_unstable,
  zInspectRunningJobResponse_unstable,
  zKillRunningJobResponse_unstable,
  zLeanzeroLinkHealthResponse_unstable,
  zLeanzeroLinkNodesResponse_unstable,
  zLeanzeroLinkRemoteExecuteResponse_unstable,
  zLeanzeroLinkRequestCodeResponse_unstable,
  zLeanzeroLinkStateResponse_unstable,
  zLeanzeroLinkVerifyResponse_unstable,
  zListAgentMentionsResponse_unstable,
  zListMemoryProposalsResponse_unstable,
  zListPromptsResponse_unstable,
  zListProvidersResponse_unstable,
  zListRecipesResponse_unstable,
  zListScheduleSessionsResponse_unstable,
  zListSchedulesResponse_unstable,
  zListSlashCommandsResponse_unstable,
  zListSourcesResponse_unstable,
  zLocalInferenceBuiltinChatTemplatesListResponse_unstable,
  zLocalInferenceHuggingFaceRepoVariantsResponse_unstable,
  zLocalInferenceHuggingFaceSearchResponse_unstable,
  zLocalInferenceModelDownloadProgressResponse_unstable,
  zLocalInferenceModelDownloadResponse_unstable,
  zLocalInferenceModelSettingsReadResponse_unstable,
  zLocalInferenceModelSettingsUpdateResponse_unstable,
  zLocalInferenceModelsListResponse_unstable,
  zMlxEngineBrowseFiltersResponse_unstable,
  zMlxEngineBrowseResponse_unstable,
  zMlxEngineDistributedConfigResponse_unstable,
  zMlxEngineDistributedDiscoverResponse_unstable,
  zMlxEngineDistributedPeerCandidatesResponse_unstable,
  zMlxEngineDistributedPreflightResponse_unstable,
  zMlxEngineDistributedProvisionResponse_unstable,
  zMlxEngineDistributedStartResponse_unstable,
  zMlxEngineDistributedStatusResponse_unstable,
  zMlxEngineDistributedStopResponse_unstable,
  zMlxEngineDownloadProgressResponse_unstable,
  zMlxEngineHfSearchResponse_unstable,
  zMlxEngineLinkFactsResponse_unstable,
  zMlxEngineModelCardResponse_unstable,
  zMlxEngineModelsListResponse_unstable,
  zMlxEngineReplicaProgressResponse_unstable,
  zMlxEngineReplicaTargetsResponse_unstable,
  zMlxEngineReplicateResponse_unstable,
  zMlxEngineSettingsResponse_unstable,
  zMlxEngineStatusResponse_unstable,
  zOnboardingImportApplyResponse_unstable,
  zOnboardingImportScanResponse_unstable,
  zParseRecipeResponse_unstable,
  zPreferencesReadResponse_unstable,
  zPromptOperationResponse_unstable,
  zProviderCatalogListResponse_unstable,
  zProviderCatalogTemplateResponse_unstable,
  zProviderConfigChangeResponse_unstable,
  zProviderConfigReadResponse_unstable,
  zProviderConfigStatusResponse_unstable,
  zProviderSecretsListResponse_unstable,
  zProviderSetupCatalogListResponse_unstable,
  zProviderSupportedModelsListResponse_unstable,
  zReadResourceResponse_unstable,
  zRecipeToYamlResponse_unstable,
  zRefreshProviderInventoryResponse_unstable,
  zRequestRecipeParams_unstable,
  zRunScheduleNowResponse_unstable,
  zSaveRecipeResponse_unstable,
  zScanRecipeResponse_unstable,
  zSetToolPermissionsResponse_unstable,
  zShareSessionNostrResponse_unstable,
  zSteerSessionResponse_unstable,
  zUpdateScheduleResponse_unstable,
  zUpdateSourceResponse_unstable,
} from './zod.gen.js';

export class GooseExtClient {
  constructor(private conn: ExtMethodProvider) {}

  async sessionExtensionsAdd_unstable(
    params: AddSessionExtensionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/session/extensions/add", params);
  }

  async sessionExtensionsRemove_unstable(
    params: RemoveSessionExtensionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/session/extensions/remove",
      params,
    );
  }

  async toolsList_unstable(
    params: GetToolsRequest_unstable,
  ): Promise<GetToolsResponse_unstable> {
    const raw = await this.conn.extMethod("_goose/unstable/tools/list", params);
    return zGetToolsResponse_unstable.parse(raw) as GetToolsResponse_unstable;
  }

  async toolsPermissionsSet_unstable(
    params: SetToolPermissionsRequest_unstable,
  ): Promise<SetToolPermissionsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/tools/permissions/set",
      params,
    );
    return zSetToolPermissionsResponse_unstable.parse(
      raw,
    ) as SetToolPermissionsResponse_unstable;
  }

  async toolsCall_unstable(
    params: GooseToolCallRequest_unstable,
  ): Promise<GooseToolCallResponse_unstable> {
    const raw = await this.conn.extMethod("_goose/unstable/tools/call", params);
    return zGooseToolCallResponse_unstable.parse(
      raw,
    ) as GooseToolCallResponse_unstable;
  }

  async resourcesRead_unstable(
    params: ReadResourceRequest_unstable,
  ): Promise<ReadResourceResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/resources/read",
      params,
    );
    return zReadResourceResponse_unstable.parse(
      raw,
    ) as ReadResourceResponse_unstable;
  }

  async appsList_unstable(
    params: AppsListRequest_unstable,
  ): Promise<AppsListResponse_unstable> {
    const raw = await this.conn.extMethod("_goose/unstable/apps/list", params);
    return zAppsListResponse_unstable.parse(raw) as AppsListResponse_unstable;
  }

  async appsExport_unstable(
    params: AppsExportRequest_unstable,
  ): Promise<AppsExportResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/apps/export",
      params,
    );
    return zAppsExportResponse_unstable.parse(
      raw,
    ) as AppsExportResponse_unstable;
  }

  async appsImport_unstable(
    params: AppsImportRequest_unstable,
  ): Promise<AppsImportResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/apps/import",
      params,
    );
    return zAppsImportResponse_unstable.parse(
      raw,
    ) as AppsImportResponse_unstable;
  }

  async sessionWorkingDirUpdate_unstable(
    params: UpdateWorkingDirRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/session/working-dir/update",
      params,
    );
  }

  async sessionSystemPromptSet_unstable(
    params: SetSessionSystemPromptRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/session/system-prompt/set",
      params,
    );
  }

  async sessionSteer_unstable(
    params: SteerSessionRequest_unstable,
  ): Promise<SteerSessionResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/session/steer",
      params,
    );
    return zSteerSessionResponse_unstable.parse(
      raw,
    ) as SteerSessionResponse_unstable;
  }

  async diagnosticsGet_unstable(
    params: DiagnosticsGetRequest_unstable,
  ): Promise<DiagnosticsGetResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/diagnostics/get",
      params,
    );
    return zDiagnosticsGetResponse_unstable.parse(
      raw,
    ) as DiagnosticsGetResponse_unstable;
  }

  async configPromptsList_unstable(
    params: ListPromptsRequest_unstable,
  ): Promise<ListPromptsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/prompts/list",
      params,
    );
    return zListPromptsResponse_unstable.parse(
      raw,
    ) as ListPromptsResponse_unstable;
  }

  async configPromptsGet_unstable(
    params: GetPromptRequest_unstable,
  ): Promise<GetPromptResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/prompts/get",
      params,
    );
    return zGetPromptResponse_unstable.parse(raw) as GetPromptResponse_unstable;
  }

  async configPromptsSave_unstable(
    params: SavePromptRequest_unstable,
  ): Promise<PromptOperationResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/prompts/save",
      params,
    );
    return zPromptOperationResponse_unstable.parse(
      raw,
    ) as PromptOperationResponse_unstable;
  }

  async configPromptsReset_unstable(
    params: ResetPromptRequest_unstable,
  ): Promise<PromptOperationResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/prompts/reset",
      params,
    );
    return zPromptOperationResponse_unstable.parse(
      raw,
    ) as PromptOperationResponse_unstable;
  }

  async sessionDelete(params: DeleteSessionRequest): Promise<void> {
    await this.conn.extMethod("session/delete", params);
  }

  async configExtensionsInspect_unstable(
    params: InspectConfigExtensionRequest_unstable,
  ): Promise<InspectConfigExtensionResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/extensions/inspect",
      params,
    );
    return zInspectConfigExtensionResponse_unstable.parse(
      raw,
    ) as InspectConfigExtensionResponse_unstable;
  }

  async configExtensionsList_unstable(
    params: GetConfigExtensionsRequest_unstable,
  ): Promise<GetConfigExtensionsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/extensions/list",
      params,
    );
    return zGetConfigExtensionsResponse_unstable.parse(
      raw,
    ) as GetConfigExtensionsResponse_unstable;
  }

  async extensionsAvailable_unstable(
    params: GetAvailableExtensionsRequest_unstable,
  ): Promise<GetAvailableExtensionsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/extensions/available",
      params,
    );
    return zGetAvailableExtensionsResponse_unstable.parse(
      raw,
    ) as GetAvailableExtensionsResponse_unstable;
  }

  async configExtensionsAdd_unstable(
    params: AddConfigExtensionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/config/extensions/add", params);
  }

  async configExtensionsRemove_unstable(
    params: RemoveConfigExtensionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/config/extensions/remove",
      params,
    );
  }

  async configExtensionsSetEnabled_unstable(
    params: SetConfigExtensionEnabledRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/config/extensions/set-enabled",
      params,
    );
  }

  async sessionExtensionsList_unstable(
    params: GetSessionExtensionsRequest_unstable,
  ): Promise<GetSessionExtensionsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/session/extensions/list",
      params,
    );
    return zGetSessionExtensionsResponse_unstable.parse(
      raw,
    ) as GetSessionExtensionsResponse_unstable;
  }

  async providersList_unstable(
    params: ListProvidersRequest_unstable,
  ): Promise<ListProvidersResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/list",
      params,
    );
    return zListProvidersResponse_unstable.parse(
      raw,
    ) as ListProvidersResponse_unstable;
  }

  async providersSupportedModelsList_unstable(
    params: ProviderSupportedModelsListRequest_unstable,
  ): Promise<ProviderSupportedModelsListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/supported-models/list",
      params,
    );
    return zProviderSupportedModelsListResponse_unstable.parse(
      raw,
    ) as ProviderSupportedModelsListResponse_unstable;
  }

  async providersCatalogList_unstable(
    params: ProviderCatalogListRequest_unstable,
  ): Promise<ProviderCatalogListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/catalog/list",
      params,
    );
    return zProviderCatalogListResponse_unstable.parse(
      raw,
    ) as ProviderCatalogListResponse_unstable;
  }

  async providersSetupCatalogList_unstable(
    params: ProviderSetupCatalogListRequest_unstable,
  ): Promise<ProviderSetupCatalogListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/setup/catalog/list",
      params,
    );
    return zProviderSetupCatalogListResponse_unstable.parse(
      raw,
    ) as ProviderSetupCatalogListResponse_unstable;
  }

  async providersCatalogTemplate_unstable(
    params: ProviderCatalogTemplateRequest_unstable,
  ): Promise<ProviderCatalogTemplateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/catalog/template",
      params,
    );
    return zProviderCatalogTemplateResponse_unstable.parse(
      raw,
    ) as ProviderCatalogTemplateResponse_unstable;
  }

  async providersCustomCreate_unstable(
    params: CustomProviderCreateRequest_unstable,
  ): Promise<CustomProviderCreateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/custom/create",
      params,
    );
    return zCustomProviderCreateResponse_unstable.parse(
      raw,
    ) as CustomProviderCreateResponse_unstable;
  }

  async providersCustomRead_unstable(
    params: CustomProviderReadRequest_unstable,
  ): Promise<CustomProviderReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/custom/read",
      params,
    );
    return zCustomProviderReadResponse_unstable.parse(
      raw,
    ) as CustomProviderReadResponse_unstable;
  }

  async providersCustomUpdate_unstable(
    params: CustomProviderUpdateRequest_unstable,
  ): Promise<CustomProviderUpdateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/custom/update",
      params,
    );
    return zCustomProviderUpdateResponse_unstable.parse(
      raw,
    ) as CustomProviderUpdateResponse_unstable;
  }

  async providersCustomDelete_unstable(
    params: CustomProviderDeleteRequest_unstable,
  ): Promise<CustomProviderDeleteResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/custom/delete",
      params,
    );
    return zCustomProviderDeleteResponse_unstable.parse(
      raw,
    ) as CustomProviderDeleteResponse_unstable;
  }

  async providersInventoryRefresh_unstable(
    params: RefreshProviderInventoryRequest_unstable,
  ): Promise<RefreshProviderInventoryResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/inventory/refresh",
      params,
    );
    return zRefreshProviderInventoryResponse_unstable.parse(
      raw,
    ) as RefreshProviderInventoryResponse_unstable;
  }

  async providersConfigRead_unstable(
    params: ProviderConfigReadRequest_unstable,
  ): Promise<ProviderConfigReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/config/read",
      params,
    );
    return zProviderConfigReadResponse_unstable.parse(
      raw,
    ) as ProviderConfigReadResponse_unstable;
  }

  async providersConfigStatus_unstable(
    params: ProviderConfigStatusRequest_unstable,
  ): Promise<ProviderConfigStatusResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/config/status",
      params,
    );
    return zProviderConfigStatusResponse_unstable.parse(
      raw,
    ) as ProviderConfigStatusResponse_unstable;
  }

  async providersConfigSave_unstable(
    params: ProviderConfigSaveRequest_unstable,
  ): Promise<ProviderConfigChangeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/config/save",
      params,
    );
    return zProviderConfigChangeResponse_unstable.parse(
      raw,
    ) as ProviderConfigChangeResponse_unstable;
  }

  async providersConfigDelete_unstable(
    params: ProviderConfigDeleteRequest_unstable,
  ): Promise<ProviderConfigChangeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/config/delete",
      params,
    );
    return zProviderConfigChangeResponse_unstable.parse(
      raw,
    ) as ProviderConfigChangeResponse_unstable;
  }

  async providersConfigAuthenticate_unstable(
    params: ProviderConfigAuthenticateRequest_unstable,
  ): Promise<ProviderConfigChangeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/config/authenticate",
      params,
    );
    return zProviderConfigChangeResponse_unstable.parse(
      raw,
    ) as ProviderConfigChangeResponse_unstable;
  }

  async providersSecretsList_unstable(
    params: ProviderSecretsListRequest_unstable,
  ): Promise<ProviderSecretsListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/secrets/list",
      params,
    );
    return zProviderSecretsListResponse_unstable.parse(
      raw,
    ) as ProviderSecretsListResponse_unstable;
  }

  async providersSecretsDelete_unstable(
    params: ProviderSecretDeleteRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/providers/secrets/delete",
      params,
    );
  }

  async providersCanonicalModelInfo_unstable(
    params: CanonicalModelInfoRequest_unstable,
  ): Promise<CanonicalModelInfoResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/providers/canonical-model-info",
      params,
    );
    return zCanonicalModelInfoResponse_unstable.parse(
      raw,
    ) as CanonicalModelInfoResponse_unstable;
  }

  async preferencesRead_unstable(
    params: PreferencesReadRequest_unstable,
  ): Promise<PreferencesReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/preferences/read",
      params,
    );
    return zPreferencesReadResponse_unstable.parse(
      raw,
    ) as PreferencesReadResponse_unstable;
  }

  async preferencesSave_unstable(
    params: PreferencesSaveRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/preferences/save", params);
  }

  async preferencesRemove_unstable(
    params: PreferencesRemoveRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/preferences/remove", params);
  }

  async configRead_unstable(
    params: ConfigReadRequest_unstable,
  ): Promise<ConfigReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/read",
      params,
    );
    return zConfigReadResponse_unstable.parse(
      raw,
    ) as ConfigReadResponse_unstable;
  }

  async configUpsert_unstable(
    params: ConfigUpsertRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/config/upsert", params);
  }

  async configRemove_unstable(
    params: ConfigRemoveRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/config/remove", params);
  }

  async configReadAll_unstable(
    params: ConfigReadAllRequest_unstable,
  ): Promise<ConfigReadAllResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/config/read-all",
      params,
    );
    return zConfigReadAllResponse_unstable.parse(
      raw,
    ) as ConfigReadAllResponse_unstable;
  }

  async defaultsRead_unstable(
    params: DefaultsReadRequest_unstable,
  ): Promise<DefaultsReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/defaults/read",
      params,
    );
    return zDefaultsReadResponse_unstable.parse(
      raw,
    ) as DefaultsReadResponse_unstable;
  }

  async defaultsSave_unstable(
    params: DefaultsSaveRequest_unstable,
  ): Promise<DefaultsReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/defaults/save",
      params,
    );
    return zDefaultsReadResponse_unstable.parse(
      raw,
    ) as DefaultsReadResponse_unstable;
  }

  async defaultsClear_unstable(
    params: DefaultsClearRequest_unstable,
  ): Promise<DefaultsReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/defaults/clear",
      params,
    );
    return zDefaultsReadResponse_unstable.parse(
      raw,
    ) as DefaultsReadResponse_unstable;
  }

  async onboardingImportScan_unstable(
    params: OnboardingImportScanRequest_unstable,
  ): Promise<OnboardingImportScanResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/onboarding/import/scan",
      params,
    );
    return zOnboardingImportScanResponse_unstable.parse(
      raw,
    ) as OnboardingImportScanResponse_unstable;
  }

  async onboardingImportApply_unstable(
    params: OnboardingImportApplyRequest_unstable,
  ): Promise<OnboardingImportApplyResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/onboarding/import/apply",
      params,
    );
    return zOnboardingImportApplyResponse_unstable.parse(
      raw,
    ) as OnboardingImportApplyResponse_unstable;
  }

  async sessionExport_unstable(
    params: ExportSessionRequest_unstable,
  ): Promise<ExportSessionResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/session/export",
      params,
    );
    return zExportSessionResponse_unstable.parse(
      raw,
    ) as ExportSessionResponse_unstable;
  }

  async sessionImport_unstable(
    params: ImportSessionRequest_unstable,
  ): Promise<ImportSessionResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/session/import",
      params,
    );
    return zImportSessionResponse_unstable.parse(
      raw,
    ) as ImportSessionResponse_unstable;
  }

  async sessionShareNostr_unstable(
    params: ShareSessionNostrRequest_unstable,
  ): Promise<ShareSessionNostrResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/session/share/nostr",
      params,
    );
    return zShareSessionNostrResponse_unstable.parse(
      raw,
    ) as ShareSessionNostrResponse_unstable;
  }

  async recipesEncode_unstable(
    params: EncodeRecipeRequest_unstable,
  ): Promise<EncodeRecipeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/encode",
      params,
    );
    return zEncodeRecipeResponse_unstable.parse(
      raw,
    ) as EncodeRecipeResponse_unstable;
  }

  async recipesDecode_unstable(
    params: DecodeRecipeRequest_unstable,
  ): Promise<DecodeRecipeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/decode",
      params,
    );
    return zDecodeRecipeResponse_unstable.parse(
      raw,
    ) as DecodeRecipeResponse_unstable;
  }

  async recipesScan_unstable(
    params: ScanRecipeRequest_unstable,
  ): Promise<ScanRecipeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/scan",
      params,
    );
    return zScanRecipeResponse_unstable.parse(
      raw,
    ) as ScanRecipeResponse_unstable;
  }

  async recipesList_unstable(
    params: ListRecipesRequest_unstable,
  ): Promise<ListRecipesResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/list",
      params,
    );
    return zListRecipesResponse_unstable.parse(
      raw,
    ) as ListRecipesResponse_unstable;
  }

  async recipesDelete_unstable(
    params: DeleteRecipeRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/recipes/delete", params);
  }

  async recipesSchedule_unstable(
    params: ScheduleRecipeRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/recipes/schedule", params);
  }

  async recipesSlashCommand_unstable(
    params: SetRecipeSlashCommandRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/recipes/slash-command", params);
  }

  async recipesSave_unstable(
    params: SaveRecipeRequest_unstable,
  ): Promise<SaveRecipeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/save",
      params,
    );
    return zSaveRecipeResponse_unstable.parse(
      raw,
    ) as SaveRecipeResponse_unstable;
  }

  async recipesCreate_unstable(
    params: CreateRecipeRequest_unstable,
  ): Promise<CreateRecipeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/create",
      params,
    );
    return zCreateRecipeResponse_unstable.parse(
      raw,
    ) as CreateRecipeResponse_unstable;
  }

  async recipesParse_unstable(
    params: ParseRecipeRequest_unstable,
  ): Promise<ParseRecipeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/parse",
      params,
    );
    return zParseRecipeResponse_unstable.parse(
      raw,
    ) as ParseRecipeResponse_unstable;
  }

  async recipesToYaml_unstable(
    params: RecipeToYamlRequest_unstable,
  ): Promise<RecipeToYamlResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/recipes/to-yaml",
      params,
    );
    return zRecipeToYamlResponse_unstable.parse(
      raw,
    ) as RecipeToYamlResponse_unstable;
  }

  async schedulesList_unstable(
    params: ListSchedulesRequest_unstable,
  ): Promise<ListSchedulesResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/list",
      params,
    );
    return zListSchedulesResponse_unstable.parse(
      raw,
    ) as ListSchedulesResponse_unstable;
  }

  async schedulesSessionsList_unstable(
    params: ListScheduleSessionsRequest_unstable,
  ): Promise<ListScheduleSessionsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/sessions/list",
      params,
    );
    return zListScheduleSessionsResponse_unstable.parse(
      raw,
    ) as ListScheduleSessionsResponse_unstable;
  }

  async schedulesCreate_unstable(
    params: CreateScheduleRequest_unstable,
  ): Promise<CreateScheduleResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/create",
      params,
    );
    return zCreateScheduleResponse_unstable.parse(
      raw,
    ) as CreateScheduleResponse_unstable;
  }

  async schedulesDelete_unstable(
    params: DeleteScheduleRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/schedules/delete", params);
  }

  async schedulesPause_unstable(
    params: PauseScheduleRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/schedules/pause", params);
  }

  async schedulesUnpause_unstable(
    params: UnpauseScheduleRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/schedules/unpause", params);
  }

  async schedulesUpdate_unstable(
    params: UpdateScheduleRequest_unstable,
  ): Promise<UpdateScheduleResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/update",
      params,
    );
    return zUpdateScheduleResponse_unstable.parse(
      raw,
    ) as UpdateScheduleResponse_unstable;
  }

  async schedulesRunNow_unstable(
    params: RunScheduleNowRequest_unstable,
  ): Promise<RunScheduleNowResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/run-now",
      params,
    );
    return zRunScheduleNowResponse_unstable.parse(
      raw,
    ) as RunScheduleNowResponse_unstable;
  }

  async schedulesRunningJobKill_unstable(
    params: KillRunningJobRequest_unstable,
  ): Promise<KillRunningJobResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/running-job/kill",
      params,
    );
    return zKillRunningJobResponse_unstable.parse(
      raw,
    ) as KillRunningJobResponse_unstable;
  }

  async schedulesRunningJobInspect_unstable(
    params: InspectRunningJobRequest_unstable,
  ): Promise<InspectRunningJobResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/schedules/running-job/inspect",
      params,
    );
    return zInspectRunningJobResponse_unstable.parse(
      raw,
    ) as InspectRunningJobResponse_unstable;
  }

  async sessionInfo_unstable(
    params: GetSessionInfoRequest_unstable,
  ): Promise<GetSessionInfoResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/session/info",
      params,
    );
    return zGetSessionInfoResponse_unstable.parse(
      raw,
    ) as GetSessionInfoResponse_unstable;
  }

  async sessionConversationTruncate_unstable(
    params: TruncateSessionConversationRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/session/conversation/truncate",
      params,
    );
  }

  async sessionProjectUpdate_unstable(
    params: UpdateSessionProjectRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/session/project/update", params);
  }

  async sessionRename_unstable(
    params: RenameSessionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/session/rename", params);
  }

  async sessionArchive_unstable(
    params: ArchiveSessionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/session/archive", params);
  }

  async sessionUnarchive_unstable(
    params: UnarchiveSessionRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/session/unarchive", params);
  }

  async sourcesCreate_unstable(
    params: CreateSourceRequest_unstable,
  ): Promise<CreateSourceResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/sources/create",
      params,
    );
    return zCreateSourceResponse_unstable.parse(
      raw,
    ) as CreateSourceResponse_unstable;
  }

  async sourcesList_unstable(
    params: ListSourcesRequest_unstable,
  ): Promise<ListSourcesResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/sources/list",
      params,
    );
    return zListSourcesResponse_unstable.parse(
      raw,
    ) as ListSourcesResponse_unstable;
  }

  async agentMentionsList_unstable(
    params: ListAgentMentionsRequest_unstable,
  ): Promise<ListAgentMentionsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/agent-mentions/list",
      params,
    );
    return zListAgentMentionsResponse_unstable.parse(
      raw,
    ) as ListAgentMentionsResponse_unstable;
  }

  async slashCommandsList_unstable(
    params: ListSlashCommandsRequest_unstable,
  ): Promise<ListSlashCommandsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/slash-commands/list",
      params,
    );
    return zListSlashCommandsResponse_unstable.parse(
      raw,
    ) as ListSlashCommandsResponse_unstable;
  }

  async sourcesUpdate_unstable(
    params: UpdateSourceRequest_unstable,
  ): Promise<UpdateSourceResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/sources/update",
      params,
    );
    return zUpdateSourceResponse_unstable.parse(
      raw,
    ) as UpdateSourceResponse_unstable;
  }

  async sourcesDelete_unstable(
    params: DeleteSourceRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/sources/delete", params);
  }

  async sourcesExport_unstable(
    params: ExportSourceRequest_unstable,
  ): Promise<ExportSourceResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/sources/export",
      params,
    );
    return zExportSourceResponse_unstable.parse(
      raw,
    ) as ExportSourceResponse_unstable;
  }

  async sourcesImport_unstable(
    params: ImportSourcesRequest_unstable,
  ): Promise<ImportSourcesResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/sources/import",
      params,
    );
    return zImportSourcesResponse_unstable.parse(
      raw,
    ) as ImportSourcesResponse_unstable;
  }

  async dictationTranscribe_unstable(
    params: DictationTranscribeRequest_unstable,
  ): Promise<DictationTranscribeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/dictation/transcribe",
      params,
    );
    return zDictationTranscribeResponse_unstable.parse(
      raw,
    ) as DictationTranscribeResponse_unstable;
  }

  async dictationConfig_unstable(
    params: DictationConfigRequest_unstable,
  ): Promise<DictationConfigResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/dictation/config",
      params,
    );
    return zDictationConfigResponse_unstable.parse(
      raw,
    ) as DictationConfigResponse_unstable;
  }

  async dictationSecretSave_unstable(
    params: DictationSecretSaveRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/dictation/secret/save", params);
  }

  async dictationSecretDelete_unstable(
    params: DictationSecretDeleteRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/dictation/secret/delete",
      params,
    );
  }

  async dictationModelsList_unstable(
    params: DictationModelsListRequest_unstable,
  ): Promise<DictationModelsListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/dictation/models/list",
      params,
    );
    return zDictationModelsListResponse_unstable.parse(
      raw,
    ) as DictationModelsListResponse_unstable;
  }

  async dictationModelsDownload_unstable(
    params: DictationModelDownloadRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/dictation/models/download",
      params,
    );
  }

  async dictationModelsDownloadProgress_unstable(
    params: DictationModelDownloadProgressRequest_unstable,
  ): Promise<DictationModelDownloadProgressResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/dictation/models/download/progress",
      params,
    );
    return zDictationModelDownloadProgressResponse_unstable.parse(
      raw,
    ) as DictationModelDownloadProgressResponse_unstable;
  }

  async dictationModelsCancel_unstable(
    params: DictationModelCancelRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/dictation/models/cancel",
      params,
    );
  }

  async dictationModelsDelete_unstable(
    params: DictationModelDeleteRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/dictation/models/delete",
      params,
    );
  }

  async dictationModelsSelect_unstable(
    params: DictationModelSelectRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/dictation/models/select",
      params,
    );
  }

  async localInferenceModelsList_unstable(
    params: LocalInferenceModelsListRequest_unstable,
  ): Promise<LocalInferenceModelsListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/models/list",
      params,
    );
    return zLocalInferenceModelsListResponse_unstable.parse(
      raw,
    ) as LocalInferenceModelsListResponse_unstable;
  }

  async localInferenceModelsDownload_unstable(
    params: LocalInferenceModelDownloadRequest_unstable,
  ): Promise<LocalInferenceModelDownloadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/models/download",
      params,
    );
    return zLocalInferenceModelDownloadResponse_unstable.parse(
      raw,
    ) as LocalInferenceModelDownloadResponse_unstable;
  }

  async localInferenceModelsDownloadProgress_unstable(
    params: LocalInferenceModelDownloadProgressRequest_unstable,
  ): Promise<LocalInferenceModelDownloadProgressResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/models/download/progress",
      params,
    );
    return zLocalInferenceModelDownloadProgressResponse_unstable.parse(
      raw,
    ) as LocalInferenceModelDownloadProgressResponse_unstable;
  }

  async localInferenceModelsDownloadCancel_unstable(
    params: LocalInferenceModelDownloadCancelRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/local-inference/models/download/cancel",
      params,
    );
  }

  async localInferenceModelsDelete_unstable(
    params: LocalInferenceModelDeleteRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/local-inference/models/delete",
      params,
    );
  }

  async localInferenceModelsSettingsRead_unstable(
    params: LocalInferenceModelSettingsReadRequest_unstable,
  ): Promise<LocalInferenceModelSettingsReadResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/models/settings/read",
      params,
    );
    return zLocalInferenceModelSettingsReadResponse_unstable.parse(
      raw,
    ) as LocalInferenceModelSettingsReadResponse_unstable;
  }

  async localInferenceModelsSettingsUpdate_unstable(
    params: LocalInferenceModelSettingsUpdateRequest_unstable,
  ): Promise<LocalInferenceModelSettingsUpdateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/models/settings/update",
      params,
    );
    return zLocalInferenceModelSettingsUpdateResponse_unstable.parse(
      raw,
    ) as LocalInferenceModelSettingsUpdateResponse_unstable;
  }

  async localInferenceHuggingfaceSearch_unstable(
    params: LocalInferenceHuggingFaceSearchRequest_unstable,
  ): Promise<LocalInferenceHuggingFaceSearchResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/huggingface/search",
      params,
    );
    return zLocalInferenceHuggingFaceSearchResponse_unstable.parse(
      raw,
    ) as LocalInferenceHuggingFaceSearchResponse_unstable;
  }

  async localInferenceHuggingfaceRepoVariants_unstable(
    params: LocalInferenceHuggingFaceRepoVariantsRequest_unstable,
  ): Promise<LocalInferenceHuggingFaceRepoVariantsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/huggingface/repo/variants",
      params,
    );
    return zLocalInferenceHuggingFaceRepoVariantsResponse_unstable.parse(
      raw,
    ) as LocalInferenceHuggingFaceRepoVariantsResponse_unstable;
  }

  async localInferenceChatTemplatesBuiltinList_unstable(
    params: LocalInferenceBuiltinChatTemplatesListRequest_unstable,
  ): Promise<LocalInferenceBuiltinChatTemplatesListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/local-inference/chat-templates/builtin/list",
      params,
    );
    return zLocalInferenceBuiltinChatTemplatesListResponse_unstable.parse(
      raw,
    ) as LocalInferenceBuiltinChatTemplatesListResponse_unstable;
  }

  async mlxEngineStatus_unstable(
    params: MlxEngineStatusRequest_unstable,
  ): Promise<MlxEngineStatusResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/status",
      params,
    );
    return zMlxEngineStatusResponse_unstable.parse(
      raw,
    ) as MlxEngineStatusResponse_unstable;
  }

  async mlxEngineMount_unstable(
    params: MlxEngineMountRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/mlxEngine/mount", params);
  }

  async mlxEngineUnmount_unstable(
    params: MlxEngineUnmountRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/mlxEngine/unmount", params);
  }

  async mlxEngineSettingsRead_unstable(
    params: MlxEngineSettingsReadRequest_unstable,
  ): Promise<MlxEngineSettingsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/settingsRead",
      params,
    );
    return zMlxEngineSettingsResponse_unstable.parse(
      raw,
    ) as MlxEngineSettingsResponse_unstable;
  }

  async mlxEngineSettingsUpdate_unstable(
    params: MlxEngineSettingsUpdateRequest_unstable,
  ): Promise<MlxEngineSettingsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/settingsUpdate",
      params,
    );
    return zMlxEngineSettingsResponse_unstable.parse(
      raw,
    ) as MlxEngineSettingsResponse_unstable;
  }

  async mlxEngineModelsList_unstable(
    params: MlxEngineModelsListRequest_unstable,
  ): Promise<MlxEngineModelsListResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/modelsList",
      params,
    );
    return zMlxEngineModelsListResponse_unstable.parse(
      raw,
    ) as MlxEngineModelsListResponse_unstable;
  }

  async mlxEngineModelDelete_unstable(
    params: MlxEngineModelDeleteRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/mlxEngine/modelDelete", params);
  }

  async mlxEngineHfSearch_unstable(
    params: MlxEngineHfSearchRequest_unstable,
  ): Promise<MlxEngineHfSearchResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/hfSearch",
      params,
    );
    return zMlxEngineHfSearchResponse_unstable.parse(
      raw,
    ) as MlxEngineHfSearchResponse_unstable;
  }

  async mlxEngineBrowse_unstable(
    params: MlxEngineBrowseRequest_unstable,
  ): Promise<MlxEngineBrowseResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/browse",
      params,
    );
    return zMlxEngineBrowseResponse_unstable.parse(
      raw,
    ) as MlxEngineBrowseResponse_unstable;
  }

  async mlxEngineDownload_unstable(
    params: MlxEngineDownloadRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/mlxEngine/download", params);
  }

  async mlxEngineDownloadProgress_unstable(
    params: MlxEngineDownloadProgressRequest_unstable,
  ): Promise<MlxEngineDownloadProgressResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/downloadProgress",
      params,
    );
    return zMlxEngineDownloadProgressResponse_unstable.parse(
      raw,
    ) as MlxEngineDownloadProgressResponse_unstable;
  }

  async mlxEngineBrowseFilters_unstable(
    params: MlxEngineBrowseFiltersRequest_unstable,
  ): Promise<MlxEngineBrowseFiltersResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/browseFilters",
      params,
    );
    return zMlxEngineBrowseFiltersResponse_unstable.parse(
      raw,
    ) as MlxEngineBrowseFiltersResponse_unstable;
  }

  async mlxEngineModelCard_unstable(
    params: MlxEngineModelCardRequest_unstable,
  ): Promise<MlxEngineModelCardResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/modelCard",
      params,
    );
    return zMlxEngineModelCardResponse_unstable.parse(
      raw,
    ) as MlxEngineModelCardResponse_unstable;
  }

  async mlxEngineDownloadPause_unstable(
    params: MlxEngineDownloadPauseRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/mlxEngine/downloadPause",
      params,
    );
  }

  async mlxEngineDownloadResume_unstable(
    params: MlxEngineDownloadResumeRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/mlxEngine/downloadResume",
      params,
    );
  }

  async mlxEngineDistributedStatus_unstable(
    params: MlxEngineDistributedStatusRequest_unstable,
  ): Promise<MlxEngineDistributedStatusResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedStatus",
      params,
    );
    return zMlxEngineDistributedStatusResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedStatusResponse_unstable;
  }

  async mlxEngineDistributedPreflight_unstable(
    params: MlxEngineDistributedPreflightRequest_unstable,
  ): Promise<MlxEngineDistributedPreflightResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedPreflight",
      params,
    );
    return zMlxEngineDistributedPreflightResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedPreflightResponse_unstable;
  }

  async mlxEngineDistributedStart_unstable(
    params: MlxEngineDistributedStartRequest_unstable,
  ): Promise<MlxEngineDistributedStartResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedStart",
      params,
    );
    return zMlxEngineDistributedStartResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedStartResponse_unstable;
  }

  async mlxEngineDistributedStop_unstable(
    params: MlxEngineDistributedStopRequest_unstable,
  ): Promise<MlxEngineDistributedStopResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedStop",
      params,
    );
    return zMlxEngineDistributedStopResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedStopResponse_unstable;
  }

  async mlxEngineDistributedPeerCandidates_unstable(
    params: MlxEngineDistributedPeerCandidatesRequest_unstable,
  ): Promise<MlxEngineDistributedPeerCandidatesResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedPeerCandidates",
      params,
    );
    return zMlxEngineDistributedPeerCandidatesResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedPeerCandidatesResponse_unstable;
  }

  async mlxEngineDistributedDiscover_unstable(
    params: MlxEngineDistributedDiscoverRequest_unstable,
  ): Promise<MlxEngineDistributedDiscoverResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedDiscover",
      params,
    );
    return zMlxEngineDistributedDiscoverResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedDiscoverResponse_unstable;
  }

  async mlxEngineDistributedProvision_unstable(
    params: MlxEngineDistributedProvisionRequest_unstable,
  ): Promise<MlxEngineDistributedProvisionResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedProvision",
      params,
    );
    return zMlxEngineDistributedProvisionResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedProvisionResponse_unstable;
  }

  async mlxEngineDistributedConfigUpdate_unstable(
    params: MlxEngineDistributedConfigUpdateRequest_unstable,
  ): Promise<MlxEngineDistributedConfigResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/distributedConfigUpdate",
      params,
    );
    return zMlxEngineDistributedConfigResponse_unstable.parse(
      raw,
    ) as MlxEngineDistributedConfigResponse_unstable;
  }

  async mlxEngineDownloadCancel_unstable(
    params: MlxEngineDownloadCancelRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/mlxEngine/downloadCancel",
      params,
    );
  }

  async mlxEngineLinkFacts_unstable(
    params: MlxEngineLinkFactsRequest_unstable,
  ): Promise<MlxEngineLinkFactsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/linkFacts",
      params,
    );
    return zMlxEngineLinkFactsResponse_unstable.parse(
      raw,
    ) as MlxEngineLinkFactsResponse_unstable;
  }

  async mlxEngineReplicaTargets_unstable(
    params: MlxEngineReplicaTargetsRequest_unstable,
  ): Promise<MlxEngineReplicaTargetsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/replicaTargets",
      params,
    );
    return zMlxEngineReplicaTargetsResponse_unstable.parse(
      raw,
    ) as MlxEngineReplicaTargetsResponse_unstable;
  }

  async mlxEngineReplicate_unstable(
    params: MlxEngineReplicateRequest_unstable,
  ): Promise<MlxEngineReplicateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/replicate",
      params,
    );
    return zMlxEngineReplicateResponse_unstable.parse(
      raw,
    ) as MlxEngineReplicateResponse_unstable;
  }

  async mlxEngineReplicaPull_unstable(
    params: MlxEngineReplicaPullRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod("_goose/unstable/mlxEngine/replicaPull", params);
  }

  async mlxEngineReplicaProgress_unstable(
    params: MlxEngineReplicaProgressRequest_unstable,
  ): Promise<MlxEngineReplicaProgressResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/mlxEngine/replicaProgress",
      params,
    );
    return zMlxEngineReplicaProgressResponse_unstable.parse(
      raw,
    ) as MlxEngineReplicaProgressResponse_unstable;
  }

  async mlxEngineReplicaCancel_unstable(
    params: MlxEngineReplicaCancelRequest_unstable,
  ): Promise<void> {
    await this.conn.extMethod(
      "_goose/unstable/mlxEngine/replicaCancel",
      params,
    );
  }

  async leanzeroLinkHealth_unstable(
    params: LeanzeroLinkHealthRequest_unstable,
  ): Promise<LeanzeroLinkHealthResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/health",
      params,
    );
    return zLeanzeroLinkHealthResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkHealthResponse_unstable;
  }

  async leanzeroLinkRequestCode_unstable(
    params: LeanzeroLinkRequestCodeRequest_unstable,
  ): Promise<LeanzeroLinkRequestCodeResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/requestCode",
      params,
    );
    return zLeanzeroLinkRequestCodeResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkRequestCodeResponse_unstable;
  }

  async leanzeroLinkVerify_unstable(
    params: LeanzeroLinkVerifyRequest_unstable,
  ): Promise<LeanzeroLinkVerifyResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/verify",
      params,
    );
    return zLeanzeroLinkVerifyResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkVerifyResponse_unstable;
  }

  async leanzeroLinkConnect_unstable(
    params: LeanzeroLinkConnectRequest_unstable,
  ): Promise<LeanzeroLinkStateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/connect",
      params,
    );
    return zLeanzeroLinkStateResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkStateResponse_unstable;
  }

  async leanzeroLinkStatus_unstable(
    params: LeanzeroLinkStatusRequest_unstable,
  ): Promise<LeanzeroLinkStateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/status",
      params,
    );
    return zLeanzeroLinkStateResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkStateResponse_unstable;
  }

  async leanzeroLinkLogout_unstable(
    params: LeanzeroLinkLogoutRequest_unstable,
  ): Promise<LeanzeroLinkStateResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/logout",
      params,
    );
    return zLeanzeroLinkStateResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkStateResponse_unstable;
  }

  async leanzeroLinkNodes_unstable(
    params: LeanzeroLinkNodesRequest_unstable,
  ): Promise<LeanzeroLinkNodesResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/nodes",
      params,
    );
    return zLeanzeroLinkNodesResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkNodesResponse_unstable;
  }

  async memoryProposalsList_unstable(
    params: ListMemoryProposalsRequest_unstable,
  ): Promise<ListMemoryProposalsResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/memory_proposals/list",
      params,
    );
    return zListMemoryProposalsResponse_unstable.parse(
      raw,
    ) as ListMemoryProposalsResponse_unstable;
  }

  async memoryProposalsAnswer_unstable(
    params: AnswerMemoryProposalRequest_unstable,
  ): Promise<AnswerMemoryProposalResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/memory_proposals/answer",
      params,
    );
    return zAnswerMemoryProposalResponse_unstable.parse(
      raw,
    ) as AnswerMemoryProposalResponse_unstable;
  }

  async leanzeroLinkRemoteExecute_unstable(
    params: LeanzeroLinkRemoteExecuteRequest_unstable,
  ): Promise<LeanzeroLinkRemoteExecuteResponse_unstable> {
    const raw = await this.conn.extMethod(
      "_goose/unstable/leanzeroLink/remoteExecute",
      params,
    );
    return zLeanzeroLinkRemoteExecuteResponse_unstable.parse(
      raw,
    ) as LeanzeroLinkRemoteExecuteResponse_unstable;
  }
}

export interface GooseExtNotifications {
  unstable_sessionUpdate?: (
    notification: GooseSessionNotification_unstable,
  ) => Promise<void>;
}

export interface GooseExtAgentRequests {
  unstable_sessionRecipeRequestParams?: (
    request: RequestRecipeParams_unstable,
  ) => Promise<RecipeParamsResponse_unstable>;
}

export type GooseClientCallbacks = Omit<
  Client,
  "extNotification" | "extMethod"
> &
  Partial<Pick<Client, "extNotification" | "extMethod">> &
  GooseExtNotifications &
  GooseExtAgentRequests;

export function installGooseExtNotificationDispatcher(
  callbacks: GooseClientCallbacks,
): Client {
  const dispatcher: Pick<Client, "extNotification"> = {
    extNotification: async (method, params) => {
      switch (method) {
        case "_goose/unstable/session/update": {
          const parsed = zGooseSessionNotification_unstable.parse(
            params,
          ) as GooseSessionNotification_unstable;
          await callbacks.unstable_sessionUpdate?.(parsed);
          return;
        }
        default:
          await callbacks.extNotification?.(method, params);
          return;
      }
    },
  };
  return new Proxy(callbacks, {
    get(target, property) {
      if (property === "extNotification") {
        return dispatcher.extNotification;
      }

      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  }) as Client;
}

export function installGooseExtAgentRequestDispatcher(
  callbacks: GooseClientCallbacks,
): Client {
  const dispatcher: Pick<Client, "extMethod"> = {
    extMethod: async (method, params) => {
      switch (method) {
        case "_goose/unstable/session/recipe/request-params": {
          if (callbacks.unstable_sessionRecipeRequestParams) {
            const parsed = zRequestRecipeParams_unstable.parse(
              params,
            ) as RequestRecipeParams_unstable;
            return await callbacks.unstable_sessionRecipeRequestParams(parsed);
          }
          if (callbacks.extMethod) {
            return await callbacks.extMethod(method, params);
          }
          throw new Error(`unhandled ext method: ${method}`);
        }
        default:
          if (callbacks.extMethod) {
            return await callbacks.extMethod(method, params);
          }
          throw new Error(`unhandled ext method: ${method}`);
      }
    },
  };
  return new Proxy(callbacks, {
    get(target, property) {
      if (property === "extMethod") {
        return dispatcher.extMethod;
      }

      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  }) as Client;
}
