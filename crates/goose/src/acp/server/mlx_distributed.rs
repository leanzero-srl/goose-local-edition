//! ACP surface for the DISTRIBUTED MLX engine (`goose_sidecar::distributed`): status, dry-run
//! preflight, start, stop and config persistence under the `mlx_distributed` config key. Local to
//! this goosed — the Mac that is rank 0 supervises the run — so no request carries a mesh `nodeId`.
//!
//! The single engine's handlers are untouched except for two guards that make "one engine owns a
//! Mac at a time" hold in both directions: `distributedStart` refuses (code
//! `singleEngineMounted`) while the single engine is mounted, and the single engine's `mount`
//! refuses while the distributed engine owns the Mac (`refuse_single_mount_while_distributed`).

use super::*;
use crate::config::ConfigError;
use goose_sidecar::distributed::{
    self, supervisor::Liveness, Backend, CheckVerdict, DistributedConfig, DistributedStatus,
    NodeConfig, PreflightReport, RankPlan, StartOutcome, StopReport,
};
use goose_sidecar::GIB;
use std::sync::Mutex as StdMutex;

const MLX_DISTRIBUTED_CONFIG_KEY: &str = "mlx_distributed";

static LAST_DISTRIBUTED_STATE: StdMutex<String> = StdMutex::new(String::new());

fn gib(bytes: u64) -> f64 {
    bytes as f64 / GIB as f64
}

fn config_from_dto(dto: MlxDistributedConfigDto) -> anyhow::Result<DistributedConfig> {
    let backend = match dto.backend.as_str() {
        "jaccl" => Backend::Jaccl,
        "ring" => Backend::Ring,
        other => anyhow::bail!("backend '{other}' is not \"jaccl\" or \"ring\""),
    };
    Ok(DistributedConfig {
        model_id: dto.model_id,
        backend,
        port: dto.port,
        coordinator_port: dto.coordinator_port,
        context: dto.context,
        restart_on_failure: dto.restart_on_failure,
        nodes: dto
            .nodes
            .into_iter()
            .map(|n| NodeConfig {
                name: n.name,
                ssh: n.ssh,
                tb_ip: n.tb_ip,
                tb_netmask: n.tb_netmask,
                tb_interface: n.tb_interface,
                tb_service: n.tb_service,
                rdma_device: n.rdma_device,
                python: n.python,
                pipeline_python: n.pipeline_python,
                model_dir: n.model_dir,
            })
            .collect(),
    })
}

fn config_to_dto(config: DistributedConfig) -> MlxDistributedConfigDto {
    MlxDistributedConfigDto {
        model_id: config.model_id,
        backend: config.backend.as_str().to_string(),
        port: config.port,
        coordinator_port: config.coordinator_port,
        context: config.context,
        restart_on_failure: config.restart_on_failure,
        nodes: config
            .nodes
            .into_iter()
            .map(|n| MlxDistributedNodeConfigDto {
                name: n.name,
                ssh: n.ssh,
                tb_ip: n.tb_ip,
                tb_netmask: n.tb_netmask,
                tb_interface: n.tb_interface,
                tb_service: n.tb_service,
                rdma_device: n.rdma_device,
                python: n.python,
                pipeline_python: n.pipeline_python,
                model_dir: n.model_dir,
            })
            .collect(),
    }
}

fn checks_to_dto(checks: Vec<distributed::Check>) -> Vec<MlxDistributedCheckDto> {
    checks
        .into_iter()
        .map(|c| MlxDistributedCheckDto {
            id: c.id,
            verdict: match c.verdict {
                CheckVerdict::Pass => "pass",
                CheckVerdict::Warn => "warn",
                CheckVerdict::Fail => "fail",
            }
            .to_string(),
            message: c.message,
        })
        .collect()
}

fn plan_to_dto(plan: RankPlan) -> MlxDistributedRankPlanDto {
    MlxDistributedRankPlanDto {
        layer_start: plan.layer_start,
        layer_end: plan.layer_end,
        shard_index: plan.shard_index,
        shard_count: plan.shard_count,
        weights_bytes: plan.weights_bytes,
        state_bytes: plan.state_bytes,
        workspace_bytes: plan.workspace_bytes,
        prompt_cache_bytes: plan.prompt_cache_bytes,
        planned_bytes: plan.planned_bytes,
        with_overhead_bytes: plan.with_overhead_bytes,
        budget_bytes: plan.budget_bytes,
        fits: plan.fits,
    }
}

fn preflight_to_dto(report: PreflightReport) -> MlxDistributedPreflightDto {
    MlxDistributedPreflightDto {
        ok: report.ok,
        ran_at_ms: report.ran_at_ms,
        backend: report.backend.as_str().to_string(),
        runner: report.runner.map(|r| r.as_str().to_string()),
        model_type: report.model_type,
        context_limit: report.context_limit,
        context_source: report.context_source,
        max_context_fits: report.max_context_fits,
        checks: checks_to_dto(report.checks),
        nodes: report
            .nodes
            .into_iter()
            .map(|n| MlxDistributedNodePreflightDto {
                name: n.name,
                rank: n.rank as u32,
                host: n.host,
                checks: checks_to_dto(n.checks),
                available_bytes: n.available_bytes,
                total_bytes: n.total_bytes,
                pressure: n.pressure,
                plan: n.plan.map(plan_to_dto),
                link_speed: n.link_speed,
                mlx_version: n.mlx_version,
            })
            .collect(),
        repairs: report.repairs,
    }
}

fn liveness_to_dto(liveness: Liveness) -> MlxDistributedLivenessDto {
    MlxDistributedLivenessDto {
        samples: liveness.samples as u32,
        median_ms: liveness.median_ms,
        bound_ms: liveness.bound_ms,
        silent_ms: liveness.silent_ms,
    }
}

fn status_to_dto(
    status: DistributedStatus,
    persisted: Option<DistributedConfig>,
) -> MlxDistributedStatusDto {
    MlxDistributedStatusDto {
        mode: status.mode().to_string(),
        state: status.state.as_str().to_string(),
        backend: status.backend.map(|b| b.as_str().to_string()),
        runner: status.runner.map(|r| r.as_str().to_string()),
        model_id: status.model_id,
        base_url: status.base_url,
        context_limit: status.context_limit,
        admission_open: status.admission_open,
        inflight: status.inflight,
        liveness: status.liveness.map(liveness_to_dto),
        nodes: status
            .nodes
            .into_iter()
            .map(|n| MlxDistributedNodeStatusDto {
                name: n.name,
                rank: n.rank as u32,
                role: n.role,
                host: n.host,
                state: n.state.as_str().to_string(),
                pid: n.pid,
                layer_start: n.layer_start,
                layer_end: n.layer_end,
                shard_index: n.shard_index,
                shard_count: n.shard_count,
                available_memory_gb: n.available_bytes.map(gib),
                total_memory_gb: n.total_bytes.map(gib),
                pressure: n.pressure,
                memory_error: n.memory_error,
                active_memory_gb: n.active_bytes.map(gib),
                peak_memory_gb: n.peak_bytes.map(gib),
                planned_memory_gb: n.planned_bytes.map(gib),
                memory_limit_gb: n.memory_limit_bytes.map(gib),
                wired_limit_gb: n.wired_limit_bytes.map(gib),
                cache_limit_gb: n.cache_limit_bytes.map(gib),
                link: MlxDistributedLinkDto {
                    backend: n.backend.as_str().to_string(),
                    tb_ip: n.tb_ip,
                    interface: n.tb_interface,
                    speed: n.link_speed,
                },
            })
            .collect(),
        last_preflight: status.last_preflight.map(preflight_to_dto),
        events: status
            .events
            .into_iter()
            .map(|e| MlxDistributedEventDto {
                at_ms: e.at_ms,
                kind: e.kind.as_str().to_string(),
                node: e.node,
                message: e.message,
            })
            .collect(),
        restarts: status.restarts,
        last_error: status.last_error,
        config: status.config.or(persisted).map(config_to_dto),
    }
}

fn stop_to_dto(report: StopReport) -> MlxDistributedStopReportDto {
    MlxDistributedStopReportDto {
        steps: report.steps,
        verified: report.verified,
    }
}

fn persisted_config() -> Result<Option<DistributedConfig>, agent_client_protocol::Error> {
    match Config::global().get_param::<DistributedConfig>(MLX_DISTRIBUTED_CONFIG_KEY) {
        Ok(config) => Ok(Some(config)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(e) => Err(e).internal_err_ctx("reading mlx_distributed config"),
    }
}

fn persist_config(config: &DistributedConfig) -> Result<(), agent_client_protocol::Error> {
    Config::global()
        .set_param(MLX_DISTRIBUTED_CONFIG_KEY, config)
        .internal_err_ctx("persisting mlx_distributed config")
}

/// The request's config, else the persisted one; validated, and never on the single engine's port.
fn resolve_config(
    dto: Option<MlxDistributedConfigDto>,
) -> Result<DistributedConfig, agent_client_protocol::Error> {
    let config = match dto {
        Some(dto) => config_from_dto(dto).invalid_params_err()?,
        None => persisted_config()?.ok_or_else(|| {
            agent_client_protocol::Error::invalid_params().data(
                "no distributed config: send `config`, or save one with distributedConfigUpdate",
            )
        })?,
    };
    config.validate().invalid_params_err()?;
    let single_port = super::mlx_engine::load_engine_settings()?.port;
    if config.port == single_port {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "port {single_port} is the single MLX engine's port; the distributed engine serves on its own"
        )));
    }
    Ok(config)
}

/// The single engine's mount path calls this: while the distributed engine owns the Mac, a mount
/// is refused with a named reason (never a silent stop of either engine).
pub(super) fn refuse_single_mount_while_distributed() -> Result<(), agent_client_protocol::Error> {
    let status = distributed::global_manager().status();
    if status.state.owns_the_mac() {
        return Err(agent_client_protocol::Error::invalid_params().data(format!(
            "distributedEngineActive: the distributed MLX engine is {} with '{}' on this Mac; one \
             engine owns a Mac at a time — stop it before mounting",
            status.state.as_str(),
            status.model_id.as_deref().unwrap_or("<model not reported>")
        )));
    }
    Ok(())
}

/// `goose serve`'s exit path: stop a supervised distributed run (verified, per pid) so no 20 GB
/// rank outlives goosed on either Mac.
pub(super) async fn shutdown_distributed_engine() -> String {
    let manager = distributed::global_manager();
    let state = manager.status().state;
    if !state.owns_the_mac() {
        return format!("nothing supervised (state '{}')", state.as_str());
    }
    let report = manager.stop().await;
    format!(
        "{} ({}): {}",
        if report.verified {
            "stopped, verified"
        } else {
            "stopped, NOT verified"
        },
        state.as_str(),
        report.steps.join("; ")
    )
}

fn status_response() -> Result<MlxEngineDistributedStatusResponse, agent_client_protocol::Error> {
    let status = distributed::global_manager().status();
    let persisted = if status.config.is_none() {
        persisted_config()?
    } else {
        None
    };
    Ok(MlxEngineDistributedStatusResponse {
        status: status_to_dto(status, persisted),
    })
}

impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_distributed_status(
        &self,
        _req: MlxEngineDistributedStatusRequest,
    ) -> Result<MlxEngineDistributedStatusResponse, agent_client_protocol::Error> {
        super::mlx_engine::align_omlx_host_env();
        let response = status_response()?;
        let entered_serving = {
            let mut last = LAST_DISTRIBUTED_STATE.lock().unwrap();
            let serving = matches!(response.status.state.as_str(), "ready" | "serving");
            let was_serving = matches!(last.as_str(), "ready" | "serving");
            *last = response.status.state.clone();
            serving && !was_serving
        };
        if entered_serving {
            // The omlx provider now reaches the distributed engine (OMLX_HOST follows it); its
            // model inventory must list the distributed model id before the next model switch.
            if let Err(e) = self
                .start_provider_inventory_refresh(&["omlx".to_string()])
                .await
            {
                warn!(error = ?e, "omlx inventory refresh after the distributed engine came up failed");
            }
        }
        Ok(response)
    }

    pub(super) async fn on_mlx_engine_distributed_preflight(
        &self,
        req: MlxEngineDistributedPreflightRequest,
    ) -> Result<MlxEngineDistributedPreflightResponse, agent_client_protocol::Error> {
        let config = resolve_config(req.config)?;
        let report = distributed::global_manager()
            .preflight(&config, req.repair_link)
            .await
            .invalid_params_err()?;
        Ok(MlxEngineDistributedPreflightResponse {
            preflight: preflight_to_dto(report),
        })
    }

    pub(super) async fn on_mlx_engine_distributed_start(
        &self,
        req: MlxEngineDistributedStartRequest,
    ) -> Result<MlxEngineDistributedStartResponse, agent_client_protocol::Error> {
        let given = req.config.is_some();
        let config = resolve_config(req.config)?;
        if given {
            persist_config(&config)?;
        }
        let outcome = distributed::global_manager()
            .start(config)
            .await
            .invalid_params_err()?;
        super::mlx_engine::align_omlx_host_env();
        Ok(match outcome {
            StartOutcome::Started { preflight } => MlxEngineDistributedStartResponse {
                started: true,
                refusal: None,
                preflight: Some(preflight_to_dto(preflight)),
            },
            StartOutcome::Refused {
                code,
                message,
                preflight,
            } => MlxEngineDistributedStartResponse {
                started: false,
                refusal: Some(MlxDistributedRefusalDto {
                    code: code.as_str().to_string(),
                    message,
                }),
                preflight: preflight.map(preflight_to_dto),
            },
        })
    }

    pub(super) async fn on_mlx_engine_distributed_stop(
        &self,
        _req: MlxEngineDistributedStopRequest,
    ) -> Result<MlxEngineDistributedStopResponse, agent_client_protocol::Error> {
        let manager = distributed::global_manager();
        if manager.status().config.is_none() {
            manager.set_config(persisted_config()?);
        }
        let report = manager.stop().await;
        super::mlx_engine::align_omlx_host_env();
        Ok(MlxEngineDistributedStopResponse {
            stop: stop_to_dto(report),
            status: status_response()?.status,
        })
    }

    pub(super) async fn on_mlx_engine_distributed_config_update(
        &self,
        req: MlxEngineDistributedConfigUpdateRequest,
    ) -> Result<MlxEngineDistributedConfigResponse, agent_client_protocol::Error> {
        let config = resolve_config(Some(req.config))?;
        persist_config(&config)?;
        distributed::global_manager().set_config(Some(config.clone()));
        Ok(MlxEngineDistributedConfigResponse {
            config: config_to_dto(config),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto() -> MlxDistributedConfigDto {
        MlxDistributedConfigDto {
            model_id: "org/model".to_string(),
            backend: "jaccl".to_string(),
            port: 8190,
            coordinator_port: 32323,
            context: None,
            restart_on_failure: true,
            nodes: vec![
                MlxDistributedNodeConfigDto {
                    name: "MacBook Pro".to_string(),
                    ssh: None,
                    tb_ip: "192.168.0.1".to_string(),
                    tb_netmask: "255.255.255.252".to_string(),
                    tb_interface: "en3".to_string(),
                    tb_service: "EXO Thunderbolt 3".to_string(),
                    rdma_device: "rdma_en3".to_string(),
                    python: "/v/bin/python".to_string(),
                    pipeline_python: None,
                    model_dir: "/m/a".to_string(),
                },
                MlxDistributedNodeConfigDto {
                    name: "workhorse".to_string(),
                    ssh: Some("workhorse".to_string()),
                    tb_ip: "192.168.0.2".to_string(),
                    tb_netmask: "255.255.255.252".to_string(),
                    tb_interface: "en3".to_string(),
                    tb_service: "EXO Thunderbolt 2".to_string(),
                    rdma_device: "rdma_en3".to_string(),
                    python: "/v/bin/python".to_string(),
                    pipeline_python: Some("/f/bin/python".to_string()),
                    model_dir: "/m/b".to_string(),
                },
            ],
        }
    }

    #[test]
    fn the_config_round_trips_through_the_wire_shape() {
        let config = config_from_dto(dto()).unwrap();
        config.validate().unwrap();
        assert_eq!(config_to_dto(config), dto());
        let wire = serde_json::to_value(dto()).unwrap();
        assert_eq!(wire["coordinatorPort"], 32323);
        assert_eq!(wire["nodes"][1]["tbService"], "EXO Thunderbolt 2");
        assert_eq!(wire["nodes"][1]["pipelinePython"], "/f/bin/python");
        assert!(
            wire["nodes"][0].get("ssh").is_none(),
            "absent ssh = this Mac"
        );
    }

    #[test]
    fn an_unknown_backend_is_refused_by_name() {
        let mut bad = dto();
        bad.backend = "mpi".to_string();
        let err = config_from_dto(bad).unwrap_err().to_string();
        assert!(err.contains("mpi"), "{err}");
    }

    #[test]
    fn a_stopped_engine_reports_single_mode_and_the_persisted_config() {
        let status = status_to_dto(
            distributed::DistributedManager::default().status(),
            Some(config_from_dto(dto()).unwrap()),
        );
        assert_eq!(status.mode, "single");
        assert_eq!(status.state, "stopped");
        assert!(status.admission_open);
        assert_eq!(status.config.unwrap().model_id, "org/model");
        refuse_single_mount_while_distributed().unwrap();
    }
}
