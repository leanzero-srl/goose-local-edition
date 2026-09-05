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
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::{LazyLock, Mutex as StdMutex, OnceLock};

const MLX_ENGINE_CONFIG_KEY: &str = "mlx_engine";

/// True when the operator exported OMLX_HOST before goosed started — their value wins
/// forever. Otherwise goosed owns the variable and keeps it aligned to `mlx_engine.port`,
/// so the omlx declarative provider reaches the supervised engine without manual env glue
/// (live-caught 2026-08-31: chat silently pointed at the :8000 default while the engine
/// served on :8090).
static OMLX_HOST_USER_OWNED: OnceLock<bool> = OnceLock::new();

pub(super) fn align_omlx_host_env() {
    let user_owned = *OMLX_HOST_USER_OWNED.get_or_init(|| std::env::var_os("OMLX_HOST").is_some());
    if user_owned {
        return;
    }
    // No `mlx_engine` block = the default engine on the default port (honest: nothing was
    // configured). An UNREADABLE block is a different fact: pointing chat at the default port
    // would impersonate a configuration the operator did write and we could not read — the
    // 08-31 class in reverse — so the variable is left alone and the failure is named.
    let port = match Config::global().get_param::<EngineSettings>(MLX_ENGINE_CONFIG_KEY) {
        Ok(settings) => settings.port,
        Err(ConfigError::NotFound(_)) => EngineSettings::default().port,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "mlx_engine config is unreadable; OMLX_HOST left unset instead of pointing chat at the default port"
            );
            return;
        }
    };
    std::env::set_var("OMLX_HOST", format!("http://127.0.0.1:{port}"));
}

static LAST_ENGINE_STATE: StdMutex<String> = StdMutex::new(String::new());

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
    let launcher_moved = settings.migrate_launcher();
    if let Some(from) = &launcher_moved {
        tracing::warn!(
            from = from.join(" "),
            to = settings.spawn_command.join(" "),
            "mlx_engine spawn_command was a superseded default; it now follows the shipped launcher (an engine already running on the old pin reports restart_required)"
        );
    }
    if settings.migrate_legacy() || launcher_moved.is_some() {
        Config::global()
            .set_param(MLX_ENGINE_CONFIG_KEY, &settings)
            .internal_err_ctx("persisting migrated mlx_engine config")?;
    }
    align_omlx_host_env();
    Ok(settings)
}

/// `goose serve`'s exit path for the engine: stop the sidecar THIS goosed supervises (the
/// sidecar's own SIGTERM, grace window, proven group kill, then the port is released) and
/// nothing else — a listener on the port the manager does not supervise is somebody else's
/// at exit and is left alone; the explicit Unmount is the reclaim for that case. Gated on
/// the manager's reported state rather than on the port, so a foreign engine on the port
/// is never killed by goosed quitting.
pub(super) async fn shutdown_supervised_engine() -> String {
    let manager = global_manager();
    let status = manager.status().await;
    match status.state.as_str() {
        state @ ("running" | "mounting") => {
            let port = manager.settings().port;
            let model = status
                .model_id
                .as_deref()
                .unwrap_or("<model id not reported>");
            let pid = status
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "not yet spawned".to_string());
            manager.unmount().await;
            format!(
                "engine '{model}' ({state}, pid {pid}) on port {port}: SIGTERM, grace, proven group kill, port released"
            )
        }
        state => format!(
            "nothing supervised (state '{state}'); any listener on the engine port is not this goosed's and is left alone"
        ),
    }
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
        active_requests: status.active_requests,
        active_requests_error: status.active_requests_error,
        stray_listener_port: status.stray_listener_port,
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
        hf::DownloadState::Paused => "paused",
        hf::DownloadState::Done => "done",
        hf::DownloadState::Failed => "failed",
        hf::DownloadState::Cancelled => "cancelled",
    };
    MlxDownloadProgressDto {
        state: state.to_string(),
        total_bytes: progress.total_bytes,
        downloaded_bytes: progress.downloaded_bytes,
        current_file: progress.current_file,
        restarted_files: progress.restarted_files,
        error: progress.error,
    }
}

// ============================================================================
// Local operation cores.
//
// Each `core_*` runs one mlxEngine op against THIS node's engine/tracker with the exact
// logic the ACP handler used to inline. Two callers share them, byte-for-byte: the ACP
// handler's local branch (below) and `GoosedMlxControl` (the mesh proxy's executing side).
// Only `status`'s provider-inventory refresh stays in the handler (it needs the agent);
// everything else is here so there is a single implementation to reuse, never a second.
// ============================================================================

async fn core_status() -> Result<MlxEngineStatusResponse, agent_client_protocol::Error> {
    let manager = synced_manager()?;
    Ok(MlxEngineStatusResponse {
        status: status_to_dto(manager.status().await),
    })
}

async fn core_mount(
    req: MlxEngineMountRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    let manager = synced_manager()?;
    manager.mount(&req.model_id).await.invalid_params_err()?;
    Ok(EmptyResponse {})
}

async fn core_unmount(
    _req: MlxEngineUnmountRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    global_manager().unmount().await;
    Ok(EmptyResponse {})
}

async fn core_settings_read(
    _req: MlxEngineSettingsReadRequest,
) -> Result<MlxEngineSettingsResponse, agent_client_protocol::Error> {
    Ok(MlxEngineSettingsResponse {
        settings: settings_to_dto(load_engine_settings()?),
    })
}

async fn core_settings_update(
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

async fn core_models_list(
    _req: MlxEngineModelsListRequest,
) -> Result<MlxEngineModelsListResponse, agent_client_protocol::Error> {
    let settings = load_engine_settings()?;
    let models_dir = expand_tilde(&settings.models_dir);
    let models = hf::list_local_models(&models_dir).internal_err()?;
    let (disk_available_bytes, disk_total_bytes) =
        goose_sidecar::disk_space(&models_dir).internal_err()?;
    Ok(MlxEngineModelsListResponse {
        models: models
            .into_iter()
            .map(|m| MlxLocalModelDto {
                id: m.id,
                size_bytes: m.size_bytes,
                complete: m.complete,
                missing_files: m.missing_files,
            })
            .collect(),
        disk_available_bytes,
        disk_total_bytes,
    })
}

async fn core_model_delete(
    req: MlxEngineModelDeleteRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    let settings = load_engine_settings()?;
    hf::delete_local_model(&expand_tilde(&settings.models_dir), &req.model_id)
        .invalid_params_err()?;
    Ok(EmptyResponse {})
}

async fn core_hf_search(
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

async fn core_browse(
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
    // invalid_params carries the anyhow chain to the client (the mount idiom); a
    // flattened "Internal error" hid a refused quant filter from the UI (shot 19).
    let page = hf::browse_mlx_models(&params, token.as_deref())
        .await
        .invalid_params_err()?;
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
                size_bytes_estimate: h.size_bytes_estimate,
            })
            .collect(),
        next_cursor: page.next_cursor,
    })
}

async fn core_browse_filters(
    _req: MlxEngineBrowseFiltersRequest,
) -> Result<MlxEngineBrowseFiltersResponse, agent_client_protocol::Error> {
    let token = huggingface_token().await;
    let vocab = hf::browse_filter_vocab(token.as_deref())
        .await
        .internal_err()?;
    Ok(MlxEngineBrowseFiltersResponse {
        quants: vocab.quants,
        archs: vocab.archs,
        authors: vocab.authors,
        sampled_repos: vocab.sampled_repos,
        computed_at: vocab.computed_at_epoch_s,
        refresh_error: vocab.refresh_error,
    })
}

async fn core_model_card(
    req: MlxEngineModelCardRequest,
) -> Result<MlxEngineModelCardResponse, agent_client_protocol::Error> {
    let token = huggingface_token().await;
    // invalid_params carries the anyhow chain (the mount idiom): a malformed repo id
    // must reach the UI as itself, not as "Internal error".
    let card = hf::model_card(&req.repo_id, token.as_deref())
        .await
        .invalid_params_err()?;
    Ok(MlxEngineModelCardResponse {
        readme_markdown: card.readme_markdown,
        readme_truncated: card.readme_truncated,
        files: card
            .files
            .into_iter()
            .map(|f| MlxRepoFileDto {
                path: f.path,
                size_bytes: f.size,
            })
            .collect(),
        total_bytes: card.total_bytes,
        tags: card.tags,
        downloads: card.downloads,
        likes: card.likes,
        license: card.license,
        created_at: card.created_at,
        last_modified: card.last_modified,
    })
}

async fn core_download(
    req: MlxEngineDownloadRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    let settings = load_engine_settings()?;
    let token = huggingface_token().await;
    DOWNLOAD_TRACKER
        .start_download(&req.repo_id, &expand_tilde(&settings.models_dir), token)
        .invalid_params_err()?;
    Ok(EmptyResponse {})
}

async fn core_download_progress(
    req: MlxEngineDownloadProgressRequest,
) -> Result<MlxEngineDownloadProgressResponse, agent_client_protocol::Error> {
    Ok(MlxEngineDownloadProgressResponse {
        progress: DOWNLOAD_TRACKER.progress(&req.repo_id).map(progress_to_dto),
    })
}

async fn core_download_cancel(
    req: MlxEngineDownloadCancelRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    let settings = load_engine_settings()?;
    DOWNLOAD_TRACKER
        .cancel(&req.repo_id, &expand_tilde(&settings.models_dir))
        .invalid_params_err()?;
    Ok(EmptyResponse {})
}

async fn core_download_pause(
    req: MlxEngineDownloadPauseRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    DOWNLOAD_TRACKER.pause(&req.repo_id).invalid_params_err()?;
    Ok(EmptyResponse {})
}

async fn core_download_resume(
    req: MlxEngineDownloadResumeRequest,
) -> Result<EmptyResponse, agent_client_protocol::Error> {
    let settings = load_engine_settings()?;
    let token = huggingface_token().await;
    DOWNLOAD_TRACKER
        .resume(&req.repo_id, &expand_tilde(&settings.models_dir), token)
        .invalid_params_err()?;
    Ok(EmptyResponse {})
}

impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_status(
        &self,
        req: MlxEngineStatusRequest,
    ) -> Result<MlxEngineStatusResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::Status, &req).await;
        }
        let response = core_status().await?;
        let entered_running = {
            let mut last = LAST_ENGINE_STATE.lock().unwrap();
            let entered = response.status.state == "running" && *last != "running";
            *last = response.status.state.clone();
            entered
        };
        if entered_running {
            // The engine just came up serving a (possibly new) model id; without a refresh
            // the provider inventory rejects it on the next model switch. This local-only
            // step is a desktop-chat concern for THIS node — the mesh proxy's `core_status`
            // deliberately skips it (a remote poller does not drive the peer's chat).
            if let Err(e) = self
                .start_provider_inventory_refresh(&["omlx".to_string()])
                .await
            {
                warn!(error = ?e, "omlx inventory refresh after engine start failed");
            }
        }
        Ok(response)
    }

    pub(super) async fn on_mlx_engine_mount(
        &self,
        req: MlxEngineMountRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::Mount, &req).await;
        }
        core_mount(req).await
    }

    pub(super) async fn on_mlx_engine_unmount(
        &self,
        req: MlxEngineUnmountRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::Unmount, &req).await;
        }
        core_unmount(req).await
    }

    pub(super) async fn on_mlx_engine_settings_read(
        &self,
        req: MlxEngineSettingsReadRequest,
    ) -> Result<MlxEngineSettingsResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::SettingsRead, &req)
                .await;
        }
        core_settings_read(req).await
    }

    pub(super) async fn on_mlx_engine_settings_update(
        &self,
        req: MlxEngineSettingsUpdateRequest,
    ) -> Result<MlxEngineSettingsResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::SettingsUpdate, &req)
                .await;
        }
        core_settings_update(req).await
    }

    pub(super) async fn on_mlx_engine_models_list(
        &self,
        req: MlxEngineModelsListRequest,
    ) -> Result<MlxEngineModelsListResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::ModelsList, &req).await;
        }
        core_models_list(req).await
    }

    pub(super) async fn on_mlx_engine_model_delete(
        &self,
        req: MlxEngineModelDeleteRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::ModelDelete, &req).await;
        }
        core_model_delete(req).await
    }

    pub(super) async fn on_mlx_engine_hf_search(
        &self,
        req: MlxEngineHfSearchRequest,
    ) -> Result<MlxEngineHfSearchResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::HfSearch, &req).await;
        }
        core_hf_search(req).await
    }

    pub(super) async fn on_mlx_engine_browse(
        &self,
        req: MlxEngineBrowseRequest,
    ) -> Result<MlxEngineBrowseResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::Browse, &req).await;
        }
        core_browse(req).await
    }

    pub(super) async fn on_mlx_engine_browse_filters(
        &self,
        req: MlxEngineBrowseFiltersRequest,
    ) -> Result<MlxEngineBrowseFiltersResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::BrowseFilters, &req)
                .await;
        }
        core_browse_filters(req).await
    }

    pub(super) async fn on_mlx_engine_model_card(
        &self,
        req: MlxEngineModelCardRequest,
    ) -> Result<MlxEngineModelCardResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::ModelCard, &req).await;
        }
        core_model_card(req).await
    }

    pub(super) async fn on_mlx_engine_download(
        &self,
        req: MlxEngineDownloadRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self.mlx_engine_relay(&node, MlxOp::Download, &req).await;
        }
        core_download(req).await
    }

    pub(super) async fn on_mlx_engine_download_progress(
        &self,
        req: MlxEngineDownloadProgressRequest,
    ) -> Result<MlxEngineDownloadProgressResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::DownloadProgress, &req)
                .await;
        }
        core_download_progress(req).await
    }

    pub(super) async fn on_mlx_engine_download_cancel(
        &self,
        req: MlxEngineDownloadCancelRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::DownloadCancel, &req)
                .await;
        }
        core_download_cancel(req).await
    }

    pub(super) async fn on_mlx_engine_download_pause(
        &self,
        req: MlxEngineDownloadPauseRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::DownloadPause, &req)
                .await;
        }
        core_download_pause(req).await
    }

    pub(super) async fn on_mlx_engine_download_resume(
        &self,
        req: MlxEngineDownloadResumeRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if let Some(node) = self.mlx_engine_remote_target(req.node_id.as_deref()) {
            return self
                .mlx_engine_relay(&node, MlxOp::DownloadResume, &req)
                .await;
        }
        core_download_resume(req).await
    }
}

// ============================================================================
// GoosedMlxControl — the executing side of the mesh MLX proxy.
//
// goose implements the `leanzero-link` `MlxControl` seam over the SAME local cores the ACP
// handlers call, so an op reached via `POST /v1/swarm/mlx/<op>` from a peer runs against
// this node's `goose_sidecar` engine byte-for-byte identically to a local ACP call. It is
// stateless (the engine manager + download tracker are process globals it shares with the
// ACP handlers), so a download started locally is visible over the proxy and vice versa.
// ============================================================================

/// The local MLX-engine executor injected into the LeanZero Link control service at boot
/// (`set_mlx_control`). Runs a peer's forwarded op against this node's engine.
#[derive(Debug, Default, Clone, Copy)]
pub struct GoosedMlxControl;

impl GoosedMlxControl {
    pub fn new() -> Self {
        Self
    }
}

/// Deserialize a proxied request body into the op's ACP request type. A malformed body is a
/// `BadRequest` (the local `invalid_params` class), never a silent default.
fn mlx_req_from_value<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, MlxControlError> {
    serde_json::from_value(value)
        .map_err(|e| MlxControlError::BadRequest(format!("invalid mlx request body: {e}")))
}

/// Serialize an op's ACP response into opaque JSON for the proxy, or map its ACP error to a
/// [`MlxControlError`] that preserves the local error CLASS: an `invalid_params`-class error
/// (mount memory-gate BLOCK, malformed repo id) → `BadRequest`; anything else → `Failed`.
/// The verbatim cause text (the handler's `.data`, else its message) rides through so node A
/// re-raises the peer's failure as itself.
fn mlx_response_to_value<T: Serialize>(
    result: Result<T, agent_client_protocol::Error>,
) -> Result<serde_json::Value, MlxControlError> {
    match result {
        Ok(response) => serde_json::to_value(response)
            .map_err(|e| MlxControlError::Failed(format!("serializing mlx response: {e}"))),
        Err(error) => Err(acp_error_to_mlx_control(error)),
    }
}

fn acp_error_to_mlx_control(error: agent_client_protocol::Error) -> MlxControlError {
    let text = error
        .data
        .as_ref()
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| error.message.to_string());
    if error.code == agent_client_protocol::Error::invalid_params().code {
        MlxControlError::BadRequest(text)
    } else {
        MlxControlError::Failed(text)
    }
}

#[async_trait::async_trait]
impl MlxControl for GoosedMlxControl {
    async fn dispatch(
        &self,
        op: MlxOp,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, MlxControlError> {
        match op {
            MlxOp::Status => mlx_response_to_value(core_status().await),
            MlxOp::Mount => mlx_response_to_value(core_mount(mlx_req_from_value(request)?).await),
            MlxOp::Unmount => {
                mlx_response_to_value(core_unmount(mlx_req_from_value(request)?).await)
            }
            MlxOp::SettingsRead => {
                mlx_response_to_value(core_settings_read(mlx_req_from_value(request)?).await)
            }
            MlxOp::SettingsUpdate => {
                mlx_response_to_value(core_settings_update(mlx_req_from_value(request)?).await)
            }
            MlxOp::ModelsList => {
                mlx_response_to_value(core_models_list(mlx_req_from_value(request)?).await)
            }
            MlxOp::ModelDelete => {
                mlx_response_to_value(core_model_delete(mlx_req_from_value(request)?).await)
            }
            MlxOp::HfSearch => {
                mlx_response_to_value(core_hf_search(mlx_req_from_value(request)?).await)
            }
            MlxOp::Browse => mlx_response_to_value(core_browse(mlx_req_from_value(request)?).await),
            MlxOp::BrowseFilters => {
                mlx_response_to_value(core_browse_filters(mlx_req_from_value(request)?).await)
            }
            MlxOp::ModelCard => {
                mlx_response_to_value(core_model_card(mlx_req_from_value(request)?).await)
            }
            MlxOp::Download => {
                mlx_response_to_value(core_download(mlx_req_from_value(request)?).await)
            }
            MlxOp::DownloadProgress => {
                mlx_response_to_value(core_download_progress(mlx_req_from_value(request)?).await)
            }
            MlxOp::DownloadPause => {
                mlx_response_to_value(core_download_pause(mlx_req_from_value(request)?).await)
            }
            MlxOp::DownloadResume => {
                mlx_response_to_value(core_download_resume(mlx_req_from_value(request)?).await)
            }
            MlxOp::DownloadCancel => {
                mlx_response_to_value(core_download_cancel(mlx_req_from_value(request)?).await)
            }
        }
    }
}
