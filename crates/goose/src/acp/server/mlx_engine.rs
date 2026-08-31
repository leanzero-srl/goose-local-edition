//! ACP surface for the in-house MLX engine: bridges the `mlxEngine` custom methods to
//! `goose_sidecar`'s engine manager and HuggingFace download tracker. Settings persist
//! under the `mlx_engine` config key; the sidecar crate stays config-agnostic, so every
//! handler syncs the persisted settings into the manager before acting.

use super::*;
use crate::config::ConfigError;
use goose_sidecar::engine::{
    expand_tilde, global_manager, EngineSettings, MlxEngineManager, ModelProfile,
};
use goose_sidecar::hf::{self, DownloadTracker};
use std::sync::LazyLock;

const MLX_ENGINE_CONFIG_KEY: &str = "mlx_engine";

static DOWNLOAD_TRACKER: LazyLock<DownloadTracker> = LazyLock::new(DownloadTracker::new);

/// Every settings load runs the legacy-flats → per-model-profile migration and persists
/// it the one time it changes anything, so all paths (status, mount, read, list) see
/// profile truth. Older configs kept sampling in flat engine-wide fields; profiles are
/// per model.
fn load_engine_settings() -> Result<EngineSettings, agent_client_protocol::Error> {
    let mut settings = match Config::global().get_param::<EngineSettings>(MLX_ENGINE_CONFIG_KEY) {
        Ok(settings) => settings,
        Err(ConfigError::NotFound(_)) => EngineSettings::default(),
        Err(e) => return Err(e).internal_err_ctx("reading mlx_engine config"),
    };
    if settings.migrate_legacy() {
        Config::global()
            .set_param(MLX_ENGINE_CONFIG_KEY, &settings)
            .internal_err_ctx("persisting migrated mlx_engine config")?;
    }
    Ok(settings)
}

fn synced_manager() -> Result<&'static MlxEngineManager, agent_client_protocol::Error> {
    let manager = global_manager();
    manager.set_settings(load_engine_settings()?);
    Ok(manager)
}

async fn huggingface_token() -> Option<String> {
    match crate::providers::huggingface_auth::resolve_token_async().await {
        Ok(token) => token,
        Err(e) => {
            warn!(error = %e, "HuggingFace token resolution failed; continuing unauthenticated");
            None
        }
    }
}

fn profile_to_dto(profile: ModelProfile) -> MlxModelProfileDto {
    MlxModelProfileDto {
        temperature: profile.temperature,
        top_p: profile.top_p,
        top_k: profile.top_k,
        min_p: profile.min_p,
        repetition_penalty: profile.repetition_penalty,
        presence_penalty: profile.presence_penalty,
        frequency_penalty: profile.frequency_penalty,
        context_limit: profile.context_limit,
    }
}

fn profile_from_dto(dto: MlxModelProfileDto) -> ModelProfile {
    ModelProfile {
        temperature: dto.temperature,
        top_p: dto.top_p,
        top_k: dto.top_k,
        min_p: dto.min_p,
        repetition_penalty: dto.repetition_penalty,
        presence_penalty: dto.presence_penalty,
        frequency_penalty: dto.frequency_penalty,
        context_limit: dto.context_limit,
    }
}

fn settings_to_dto(settings: EngineSettings) -> MlxEngineSettingsDto {
    MlxEngineSettingsDto {
        model_id: settings.model_id,
        models_dir: settings.models_dir,
        port: settings.port,
        context_limit: settings.context_limit,
        temperature: settings.temperature,
        top_p: settings.top_p,
        top_k: settings.top_k,
        min_p: settings.min_p,
        repetition_penalty: settings.repetition_penalty,
        presence_penalty: settings.presence_penalty,
        frequency_penalty: settings.frequency_penalty,
        served_model_name: settings.served_model_name,
        spawn_command: settings.spawn_command,
        model_profiles: settings
            .model_profiles
            .into_iter()
            .map(|(id, p)| (id, profile_to_dto(p)))
            .collect(),
    }
}

fn settings_from_dto(dto: MlxEngineSettingsDto) -> EngineSettings {
    EngineSettings {
        model_id: dto.model_id,
        models_dir: dto.models_dir,
        port: dto.port,
        context_limit: dto.context_limit,
        temperature: dto.temperature,
        top_p: dto.top_p,
        top_k: dto.top_k,
        min_p: dto.min_p,
        repetition_penalty: dto.repetition_penalty,
        presence_penalty: dto.presence_penalty,
        frequency_penalty: dto.frequency_penalty,
        served_model_name: dto.served_model_name,
        spawn_command: dto.spawn_command,
        model_profiles: dto
            .model_profiles
            .into_iter()
            .map(|(id, p)| (id, profile_from_dto(p)))
            .collect(),
    }
}

fn status_to_dto(status: goose_sidecar::engine::EngineStatus) -> MlxEngineStatusDto {
    MlxEngineStatusDto {
        state: status.state,
        model_id: status.model_id,
        base_url: status.base_url,
        pid: status.pid,
        context_window: status.context_window,
        tool_call_parser: status.tool_call_parser,
        served_model_id: status.served_model_id,
        probe_error: status.probe_error,
        gate_message: status.gate_message,
        gate_verdict: status.gate_verdict,
        available_memory_gb: status.available_memory_gb,
        total_memory_gb: status.total_memory_gb,
        restart_required: status.restart_required,
        last_error: status.last_error,
    }
}

fn progress_to_dto(progress: hf::DownloadProgress) -> MlxDownloadProgressDto {
    let state = match progress.state {
        hf::DownloadState::Queued => "queued",
        hf::DownloadState::Downloading => "downloading",
        hf::DownloadState::Done => "done",
        hf::DownloadState::Failed => "failed",
        hf::DownloadState::Cancelled => "cancelled",
    };
    MlxDownloadProgressDto {
        state: state.to_string(),
        total_bytes: progress.total_bytes,
        downloaded_bytes: progress.downloaded_bytes,
        current_file: progress.current_file,
        error: progress.error,
    }
}

impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_status(
        &self,
        _req: MlxEngineStatusRequest,
    ) -> Result<MlxEngineStatusResponse, agent_client_protocol::Error> {
        let manager = synced_manager()?;
        Ok(MlxEngineStatusResponse {
            status: status_to_dto(manager.status().await),
        })
    }

    pub(super) async fn on_mlx_engine_mount(
        &self,
        req: MlxEngineMountRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let manager = synced_manager()?;
        manager.mount(&req.model_id).await.invalid_params_err()?;
        Ok(EmptyResponse {})
    }

    pub(super) async fn on_mlx_engine_unmount(
        &self,
        _req: MlxEngineUnmountRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        global_manager().unmount().await;
        Ok(EmptyResponse {})
    }

    pub(super) async fn on_mlx_engine_settings_read(
        &self,
        _req: MlxEngineSettingsReadRequest,
    ) -> Result<MlxEngineSettingsResponse, agent_client_protocol::Error> {
        Ok(MlxEngineSettingsResponse {
            settings: settings_to_dto(load_engine_settings()?),
        })
    }

    pub(super) async fn on_mlx_engine_settings_update(
        &self,
        req: MlxEngineSettingsUpdateRequest,
    ) -> Result<MlxEngineSettingsResponse, agent_client_protocol::Error> {
        let mut settings = settings_from_dto(req.settings);
        // A legacy UI state may still send flat sampling fields — same migration,
        // so what persists is always profile truth.
        settings.migrate_legacy();
        Config::global()
            .set_param(MLX_ENGINE_CONFIG_KEY, &settings)
            .internal_err_ctx("persisting mlx_engine config")?;
        global_manager().set_settings(settings.clone());
        Ok(MlxEngineSettingsResponse {
            settings: settings_to_dto(settings),
        })
    }

    pub(super) async fn on_mlx_engine_models_list(
        &self,
        _req: MlxEngineModelsListRequest,
    ) -> Result<MlxEngineModelsListResponse, agent_client_protocol::Error> {
        let settings = load_engine_settings()?;
        let models = hf::list_local_models(&expand_tilde(&settings.models_dir)).internal_err()?;
        Ok(MlxEngineModelsListResponse {
            models: models
                .into_iter()
                .map(|m| MlxLocalModelDto {
                    id: m.id,
                    size_bytes: m.size_bytes,
                    complete: m.complete,
                })
                .collect(),
        })
    }

    pub(super) async fn on_mlx_engine_model_delete(
        &self,
        req: MlxEngineModelDeleteRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let settings = load_engine_settings()?;
        hf::delete_local_model(&expand_tilde(&settings.models_dir), &req.model_id)
            .invalid_params_err()?;
        Ok(EmptyResponse {})
    }

    pub(super) async fn on_mlx_engine_hf_search(
        &self,
        req: MlxEngineHfSearchRequest,
    ) -> Result<MlxEngineHfSearchResponse, agent_client_protocol::Error> {
        let token = huggingface_token().await;
        let hits = hf::search_mlx_models(&req.query, req.limit.unwrap_or(20), token.as_deref())
            .await
            .internal_err()?;
        Ok(MlxEngineHfSearchResponse {
            hits: hits
                .into_iter()
                .map(|h| MlxHfModelHitDto {
                    id: h.id,
                    downloads: h.downloads,
                    likes: h.likes,
                    updated_at: h.updated_at,
                })
                .collect(),
        })
    }

    pub(super) async fn on_mlx_engine_browse(
        &self,
        req: MlxEngineBrowseRequest,
    ) -> Result<MlxEngineBrowseResponse, agent_client_protocol::Error> {
        let sort = match req.sort.as_str() {
            "downloads" => hf::BrowseSort::Downloads,
            "newest" => hf::BrowseSort::Newest,
            other => {
                return Err(anyhow::anyhow!(
                    "invalid sort '{other}': expected \"downloads\" or \"newest\""
                ))
                .invalid_params_err()
            }
        };
        let params = hf::BrowseParams {
            query: req.query,
            author: req.author,
            quant: req.quant,
            arch: req.arch,
            sort,
            cursor: req.cursor,
            limit: req.limit.unwrap_or(20),
        };
        let token = huggingface_token().await;
        let page = hf::browse_mlx_models(&params, token.as_deref())
            .await
            .internal_err()?;
        Ok(MlxEngineBrowseResponse {
            hits: page
                .hits
                .into_iter()
                .map(|h| MlxBrowseHitDto {
                    id: h.id,
                    author: h.author,
                    downloads: h.downloads,
                    likes: h.likes,
                    created_at: h.created_at,
                    last_modified: h.last_modified,
                    tags: h.tags,
                    quant: h.quant,
                    arch: h.arch,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    pub(super) async fn on_mlx_engine_download(
        &self,
        req: MlxEngineDownloadRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let settings = load_engine_settings()?;
        let token = huggingface_token().await;
        DOWNLOAD_TRACKER
            .start_download(&req.repo_id, &expand_tilde(&settings.models_dir), token)
            .invalid_params_err()?;
        Ok(EmptyResponse {})
    }

    pub(super) async fn on_mlx_engine_download_progress(
        &self,
        req: MlxEngineDownloadProgressRequest,
    ) -> Result<MlxEngineDownloadProgressResponse, agent_client_protocol::Error> {
        Ok(MlxEngineDownloadProgressResponse {
            progress: DOWNLOAD_TRACKER.progress(&req.repo_id).map(progress_to_dto),
        })
    }

    pub(super) async fn on_mlx_engine_download_cancel(
        &self,
        req: MlxEngineDownloadCancelRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        DOWNLOAD_TRACKER.cancel(&req.repo_id).invalid_params_err()?;
        Ok(EmptyResponse {})
    }
}
