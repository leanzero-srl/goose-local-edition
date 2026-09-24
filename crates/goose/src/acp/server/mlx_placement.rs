//! ACP surface for the placement planner (`goose_sidecar::placement`): `placementPlan` measures
//! this Mac and every configured peer (the distributed setup's nodes, over ssh or LeanZero Link —
//! the same probe `distributedDiscover` runs, which now names each node's chip and GPU ceiling),
//! reads the models' own files and the measurement store, and plans; `measureSpeed` runs the
//! token-counted benchmark on a RUNNING engine and records it; `speedHistory` reads the store.
//! Every fact that could not be read travels as a named `*Error` / gap, never a stand-in value.

use super::*;

#[cfg(unix)]
mod imp {
    use super::*;
    use goose_sidecar::distributed::plan::{self as dplan};
    use goose_sidecar::distributed::preflight::{loaded_files, run_fork_planner, NodeFigures};
    use goose_sidecar::distributed::probe::parse_gpu_ceiling;
    use goose_sidecar::distributed::{DistributedConfig, NodeExec, Runner, SystemExec};
    use goose_sidecar::engine::{expand_tilde, global_manager, EngineSettings};
    use goose_sidecar::hf;
    use goose_sidecar::placement::bench::{self, Workload};
    use goose_sidecar::placement::chip::{self, ChipIdentity};
    use goose_sidecar::placement::model::{read_model_facts, ModelFacts};
    use goose_sidecar::placement::planner::{
        self, ClusterInput, Goal, NodeInput, NodeMemory, PipelineFitInput, PlanInput,
    };
    use goose_sidecar::placement::predict::Calibration;
    use goose_sidecar::placement::store::{
        context_bucket, PlacementKey, PlacementKind, RecordSource, SpeedRecord,
    };
    use goose_sidecar::MemoryGate;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::super::mlx_distributed::persisted_config;
    use super::super::mlx_distributed_discover::{self as discover, ModelDir, NodeProbe};
    use super::super::mlx_distributed_link as link;
    use super::super::mlx_engine::load_engine_settings;

    pub(super) const LOCAL: &str = "local";

    use crate::providers::mlx_speed::speed_store;

    /// Shape-identical sidecar value → DTO. A mismatch is a defect in the mirror, named.
    fn mirror<T: Serialize, D: DeserializeOwned>(
        value: &T,
    ) -> Result<D, agent_client_protocol::Error> {
        serde_json::to_value(value)
            .and_then(serde_json::from_value)
            .internal_err_ctx("placement DTO mirror")
    }

    fn goal_of(dto: MlxPlacementGoalDto) -> Goal {
        match dto {
            MlxPlacementGoalDto::Chat => Goal::Chat,
            MlxPlacementGoalDto::LongDocuments => Goal::LongDocuments,
            MlxPlacementGoalDto::ManyRequests => Goal::ManyRequests,
        }
    }

    /// One Mac as measured, plus what the probe saw on its disk (peers only).
    pub(super) struct Measured {
        pub input: NodeInput,
        pub dto: MlxPlacementNodeDto,
        /// Peer model directories (`None` for this Mac: its models are the models folder).
        pub models: Option<Result<Vec<ModelDir>, String>>,
    }

    fn node_dto(input: &NodeInput) -> MlxPlacementNodeDto {
        let (chip, chip_error) = match &input.chip {
            Ok(c) => (
                Some(MlxChipDto {
                    hw_model: c.hw_model.clone(),
                    brand: c.brand.clone(),
                    gpu_cores: c.gpu_cores,
                }),
                None,
            ),
            Err(e) => (None, Some(e.clone())),
        };
        let bandwidth = input.chip.as_ref().ok().map(chip::spec_bandwidth);
        let mut dto = MlxPlacementNodeDto {
            id: input.id.clone(),
            name: input.name.clone(),
            chip,
            chip_error,
            bandwidth_gbs: bandwidth
                .as_ref()
                .and_then(|b| b.as_ref().ok())
                .map(|b| b.gb_per_s),
            bandwidth_source: bandwidth
                .as_ref()
                .and_then(|b| b.as_ref().ok())
                .map(|b| b.source.clone()),
            bandwidth_error: bandwidth.and_then(|b| b.err()),
            ..Default::default()
        };
        match &input.memory {
            Ok(m) => {
                dto.total_bytes = Some(m.total_bytes);
                dto.available_bytes = Some(m.available_bytes);
                match &m.ceiling_bytes {
                    Ok(c) => dto.ceiling_bytes = Some(*c),
                    Err(e) => dto.ceiling_error = Some(e.clone()),
                }
            }
            Err(e) => dto.memory_error = Some(e.clone()),
        }
        dto
    }

    async fn computer_name() -> Result<String, String> {
        let out = tokio::process::Command::new("/usr/sbin/scutil")
            .args(["--get", "ComputerName"])
            .output()
            .await
            .map_err(|e| format!("scutil: {e}"))?;
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if name.is_empty() {
            return Err(format!(
                "scutil printed no ComputerName: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(name)
    }

    async fn measure_local(config: Option<&DistributedConfig>) -> Measured {
        let name = match config {
            Some(c) => c.nodes[0].name.clone(),
            None => computer_name()
                .await
                .unwrap_or_else(|e| format!("This Mac ({e})")),
        };
        let memory = goose_sidecar::measure()
            .map(|m| NodeMemory {
                total_bytes: m.total_bytes,
                available_bytes: m.available_bytes,
                ceiling_bytes: chip::local_gpu_ceiling().map_err(|e| format!("{e:#}")),
            })
            .map_err(|e| format!("{e:#}"));
        let input = NodeInput {
            id: LOCAL.to_string(),
            name,
            chip: chip::local_chip().await.map_err(|e| format!("{e:#}")),
            memory,
            has_model: Ok(true),
            remote_single: Ok(()),
        };
        Measured {
            dto: node_dto(&input),
            input,
            models: None,
        }
    }

    fn peer_measured(host: &str, name: &str, probe: Result<NodeProbe, String>) -> Measured {
        let older = format!(
            "{name}'s goose did not report it (it predates the placement planner — update goose there)"
        );
        let (chip, memory, models) = match probe {
            Err(e) => (Err(e.clone()), Err(e.clone()), Some(Err(e))),
            Ok(probe) => {
                let chip = match &probe.chip {
                    None => Err(older.clone()),
                    Some(text) => chip::parse_chip(text).map_err(|e| format!("{e:#}")),
                };
                let ceiling = match &probe.gpu {
                    None => Err(older.clone()),
                    Some(text) if text.trim_start().starts_with("absent") => Err(format!(
                        "{} (Set up → Save and provision installs one)",
                        text.trim()
                    )),
                    Some(text) => parse_gpu_ceiling(text).map_err(|e| format!("{e:#}")),
                };
                let memory = Ok(NodeMemory {
                    total_bytes: probe.memory.total_bytes,
                    available_bytes: probe.memory.available_bytes,
                    ceiling_bytes: ceiling,
                });
                (chip, memory, Some(Ok(probe.models)))
            }
        };
        let input = NodeInput {
            id: host.to_string(),
            name: name.to_string(),
            chip,
            memory,
            has_model: Err("not checked yet".to_string()),
            remote_single: if host.starts_with("link:") {
                Ok(())
            } else {
                Err(format!(
                    "{name} is set up over ssh; a single engine on another Mac runs over LeanZero Link"
                ))
            },
        };
        Measured {
            dto: node_dto(&input),
            input,
            models,
        }
    }

    /// This Mac and every configured peer, measured in parallel.
    pub(super) async fn measure_nodes(config: Option<&DistributedConfig>) -> Vec<Measured> {
        let peers: Vec<(String, String)> = config
            .map(|c| {
                c.nodes
                    .iter()
                    .filter_map(|n| n.ssh.clone().map(|h| (h, n.name.clone())))
                    .collect()
            })
            .unwrap_or_default();
        if peers.iter().any(|(h, _)| h.starts_with("link:")) {
            link::ensure_link_transport();
        }
        let hosts: Vec<Option<String>> = peers.iter().map(|(h, _)| Some(h.clone())).collect();
        let mut roots: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
        for node in config.iter().flat_map(|c| c.nodes.iter()) {
            if let Some(parent) = Path::new(&node.model_dir).parent() {
                roots
                    .entry(node.ssh.clone())
                    .or_default()
                    .push(parent.display().to_string());
            }
        }
        let exec: Arc<dyn NodeExec> = Arc::new(SystemExec);
        let (local, probes) = tokio::join!(
            measure_local(config),
            discover::probe_nodes(exec, &hosts, &roots)
        );
        std::iter::once(local)
            .chain(
                peers
                    .iter()
                    .zip(probes)
                    .map(|((host, name), probe)| peer_measured(host, name, probe)),
            )
            .collect()
    }

    fn local_loaded_files(dir: &Path) -> Result<BTreeMap<String, u64>, String> {
        let mut files = BTreeMap::new();
        for entry in
            std::fs::read_dir(dir).map_err(|e| format!("listing {}: {e}", dir.display()))?
        {
            let entry = entry.map_err(|e| e.to_string())?;
            let meta = std::fs::metadata(entry.path()).map_err(|e| e.to_string())?;
            if meta.is_file() {
                files.insert(entry.file_name().to_string_lossy().into_owned(), meta.len());
            }
        }
        Ok(loaded_files(&files))
    }

    /// Does a peer hold this model? A directory of the same name with the same loaded files at the
    /// same sizes (discover's rule).
    fn peer_has(
        models: &Result<Vec<ModelDir>, String>,
        local_dir: &Path,
        local_files: &Result<BTreeMap<String, u64>, String>,
    ) -> Result<bool, String> {
        let models = models.as_ref().map_err(Clone::clone)?;
        let files = local_files.as_ref().map_err(Clone::clone)?;
        let name = local_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{} has no directory name", local_dir.display()))?;
        Ok(models.iter().any(|m| {
            Path::new(&m.dir)
                .file_name()
                .is_some_and(|n| n.to_string_lossy() == name)
                && &loaded_files(&m.files) == files
        }))
    }

    fn peer_model_dir(
        models: &Option<Result<Vec<ModelDir>, String>>,
        local_dir: &Path,
        local_files: &Result<BTreeMap<String, u64>, String>,
    ) -> Option<String> {
        let models = models.as_ref()?.as_ref().ok()?;
        let files = local_files.as_ref().ok()?;
        let name = local_dir.file_name()?.to_string_lossy().into_owned();
        models
            .iter()
            .find(|m| {
                Path::new(&m.dir)
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy() == name)
                    && &loaded_files(&m.files) == files
            })
            .map(|m| m.dir.clone())
    }

    /// The fork's planner for a qwen4_exp model on these Macs: its fixed-point walk when no
    /// context is asked (preflight's rule), else one plan at the asked context.
    async fn fork_plan(
        config: &DistributedConfig,
        local_dir: &Path,
        peer_dirs: &[Option<String>],
        nodes: &[NodeInput],
        context: Option<u64>,
    ) -> Option<Result<PipelineFitInput, String>> {
        let python = config.nodes[0].pipeline_python.clone()?;
        let mut figures = Vec::new();
        for node in nodes {
            let memory = match &node.memory {
                Ok(m) => m,
                Err(e) => return Some(Err(format!("{}: {e}", node.name))),
            };
            let ceiling = match &memory.ceiling_bytes {
                Ok(c) => *c,
                Err(e) => return Some(Err(format!("{}: GPU ceiling unknown — {e}", node.name))),
            };
            figures.push(NodeFigures::new(
                memory.available_bytes,
                memory.total_bytes,
                ceiling,
            ));
        }
        let mut config = config.clone();
        config.nodes[0].model_dir = local_dir.display().to_string();
        for (node, dir) in config.nodes.iter_mut().skip(1).zip(peer_dirs) {
            if let Some(dir) = dir {
                node.model_dir = dir.clone();
            }
        }
        let exec: Arc<dyn NodeExec> = Arc::new(SystemExec);
        let mut plan = match run_fork_planner(&config, &exec, &python, &figures, context).await {
            Ok(p) => p,
            Err(e) => return Some(Err(format!("{e:#}"))),
        };
        if context.is_none() {
            let mut tried = std::collections::BTreeSet::from([plan.context]);
            while let Some(ceiling) = plan
                .max_context
                .filter(|c| (*c > plan.context || !plan.fits) && tried.insert(*c))
            {
                plan = match run_fork_planner(&config, &exec, &python, &figures, Some(ceiling))
                    .await
                {
                    Ok(p) => p,
                    Err(e) => return Some(Err(format!("{e:#}"))),
                };
            }
        }
        let layers: f64 = plan
            .stages
            .iter()
            .map(|s| (s.layer_end - s.layer_start) as f64)
            .sum::<f64>()
            .max(1.0);
        Some(Ok(PipelineFitInput {
            fits: plan.fits,
            context: plan.fits.then_some(plan.context),
            stages: plan
                .stages
                .iter()
                .map(|s| (s.total_bytes, s.budget_bytes))
                .collect(),
            layer_shares: plan
                .stages
                .iter()
                .map(|s| (s.layer_end - s.layer_start) as f64 / layers)
                .collect(),
        }))
    }

    pub(super) struct Context {
        pub settings: EngineSettings,
        pub models_dir: PathBuf,
        pub config: Option<DistributedConfig>,
        pub measured: Vec<Measured>,
        pub records: Vec<SpeedRecord>,
        pub store_errors: Vec<String>,
    }

    pub(super) async fn context() -> Result<Context, agent_client_protocol::Error> {
        let settings = load_engine_settings()?;
        let models_dir = expand_tilde(&settings.models_dir);
        let config = persisted_config()?;
        let measured = measure_nodes(config.as_ref()).await;
        let read = speed_store()
            .read()
            .map_err(|e| agent_client_protocol::Error::internal_error().data(format!("{e:#}")))?;
        Ok(Context {
            settings,
            models_dir,
            config,
            measured,
            records: read.records,
            store_errors: read.unreadable,
        })
    }

    async fn plan_model(
        ctx: &Context,
        model_id: &str,
        bytes_on_disk: u64,
        goal_dto: MlxPlacementGoalDto,
        context: Option<u64>,
        calibration: &Calibration,
    ) -> Result<MlxPlacementPlanDto, agent_client_protocol::Error> {
        let goal = goal_of(goal_dto);
        let dir = ctx.models_dir.join(model_id);
        let failed = |error: String| MlxPlacementPlanDto {
            model_id: model_id.to_string(),
            goal: goal_dto,
            candidates: Vec::new(),
            best: None,
            best_available: None,
            badge: None,
            notes: Vec::new(),
            error: Some(error),
        };
        let facts: ModelFacts = match read_model_facts(&dir) {
            Ok(f) => f,
            Err(e) => return Ok(failed(format!("{e:#}"))),
        };
        let local_files = local_loaded_files(&dir);
        let mut nodes: Vec<NodeInput> = Vec::new();
        let mut peer_dirs = Vec::new();
        for m in &ctx.measured {
            let mut input = m.input.clone();
            if let Some(models) = &m.models {
                input.has_model = peer_has(models, &dir, &local_files);
                peer_dirs.push(peer_model_dir(&m.models, &dir, &local_files));
            }
            nodes.push(input);
        }
        let runner = Runner::for_model_type(&facts.model_type).ok();
        let tensor = (runner == Some(Runner::MlxLmTensor))
            .then(|| dplan::read_tensor_facts(&dir).map_err(|e| format!("{e:#}")));
        let pipeline = match (&ctx.config, runner) {
            (Some(config), Some(Runner::PipelineQwen4)) if nodes.len() >= 2 => {
                fork_plan(config, &dir, &peer_dirs, &nodes, context).await
            }
            _ => None,
        };
        let cluster = ctx.config.as_ref().map(|c| ClusterInput {
            link: match c.backend {
                goose_sidecar::distributed::Backend::Jaccl => "jaccl".to_string(),
                goose_sidecar::distributed::Backend::Ring => "ring".to_string(),
            },
            slots: c.slots(),
            config_model_id: c.model_id.clone(),
        });
        let plan = planner::plan(&PlanInput {
            model_id,
            model: &facts,
            bytes_on_disk,
            kv_mode: ctx
                .settings
                .model_profiles
                .get(model_id)
                .and_then(|p| p.kv_cache),
            nodes: &nodes,
            cluster: cluster.as_ref(),
            tensor: tensor.as_ref(),
            pipeline: pipeline.as_ref(),
            goal,
            context,
            records: &ctx.records,
            calibration,
            gate: &MemoryGate::default(),
        });
        mirror(&plan)
    }

    pub(super) async fn placement_plan(
        req: MlxEnginePlacementPlanRequest,
    ) -> Result<MlxEnginePlacementPlanResponse, agent_client_protocol::Error> {
        let started = std::time::Instant::now();
        let ctx = context().await?;
        let local = hf::list_local_models(&ctx.models_dir)
            .map_err(|e| agent_client_protocol::Error::internal_error().data(format!("{e:#}")))?;
        let chosen: Vec<&hf::LocalModel> = match &req.model_id {
            Some(id) => {
                let model = local.iter().find(|m| &m.id == id).ok_or_else(|| {
                    agent_client_protocol::Error::invalid_params().data(format!(
                        "{id} is not in the models folder {}",
                        ctx.models_dir.display()
                    ))
                })?;
                vec![model]
            }
            None => local.iter().filter(|m| m.complete).collect(),
        };
        let calibration = Calibration::fit(&BTreeMap::new());
        let mut plans = Vec::new();
        for model in chosen {
            plans.push(
                plan_model(
                    &ctx,
                    &model.id,
                    model.size_bytes,
                    req.goal,
                    req.context,
                    &calibration,
                )
                .await?,
            );
        }
        Ok(MlxEnginePlacementPlanResponse {
            plans,
            nodes: ctx.measured.iter().map(|m| m.dto.clone()).collect(),
            store_errors: ctx.store_errors,
            probe_ms: started.elapsed().as_millis() as u64,
        })
    }

    fn now_ms() -> u64 {
        goose_sidecar::distributed::preflight::now_ms()
    }

    /// Where the benchmark talks to: the RUNNING engine of the placement, with the id it serves.
    struct Target {
        base_url: String,
        served: String,
        key: PlacementKey,
        node_names: Vec<String>,
        chips: Vec<Option<ChipIdentity>>,
        kv_cache: Option<String>,
    }

    async fn target(
        model_id: &str,
        placement_id: &str,
        settings: &EngineSettings,
    ) -> Result<Target, agent_client_protocol::Error> {
        let refuse = |why: String| agent_client_protocol::Error::invalid_params().data(why);
        let config = persisted_config()?;
        if placement_id == PlacementKey::single(LOCAL).id() {
            let status = global_manager().status().await;
            if status.state != "running" || status.model_id.as_deref() != Some(model_id) {
                return Err(refuse(format!(
                    "the single engine on this Mac is not running {model_id} (state {}, model {}) — [Use this] mounts it first",
                    status.state,
                    status.model_id.as_deref().unwrap_or("none")
                )));
            }
            let base_url = status.base_url.ok_or_else(|| {
                refuse("the running single engine reports no base URL".to_string())
            })?;
            let served = status.served_model_id.ok_or_else(|| {
                refuse(format!(
                    "the single engine has not answered /v1/models yet ({})",
                    status.probe_error.unwrap_or_default()
                ))
            })?;
            let local = measure_local(config.as_ref()).await;
            return Ok(Target {
                base_url,
                served,
                key: PlacementKey::single(LOCAL),
                node_names: vec![local.input.name.clone()],
                chips: vec![local.input.chip.ok()],
                kv_cache: settings
                    .model_profiles
                    .get(model_id)
                    .and_then(|p| p.kv_cache)
                    .map(|m| m.engine_dtype().to_string()),
            });
        }
        if let Some(host) = placement_id.strip_prefix("single:") {
            let Some(peer) = host.strip_prefix("link:") else {
                return Err(refuse(format!(
                    "{host} is set up over ssh; only a LeanZero Link peer's single engine can be measured from here"
                )));
            };
            let route = crate::providers::mlx_remote::read().live().ok_or_else(|| {
                refuse(format!(
                    "no single engine on {host} is routed to this Mac — [Use this] starts it first"
                ))
            })?;
            if route.peer != peer || route.model_id != model_id {
                return Err(refuse(format!(
                    "the remote engine serves {} on {}, not {model_id} on {peer}",
                    route.model_id, route.peer
                )));
            }
            let measured = measure_nodes(config.as_ref()).await;
            let node = measured.iter().find(|m| m.input.id == host);
            return Ok(Target {
                base_url: route.base_url,
                served: route.served_model_id,
                key: PlacementKey::single(host),
                node_names: vec![node.map_or(route.peer_hostname.clone(), |n| n.input.name.clone())],
                chips: vec![node.and_then(|n| n.input.chip.clone().ok())],
                kv_cache: None,
            });
        }
        let status = goose_sidecar::distributed::global_manager().status();
        let Some(config_run) = status.config.clone() else {
            return Err(refuse(
                "the distributed engine is not running on this Mac".to_string(),
            ));
        };
        if !matches!(
            status.state,
            goose_sidecar::distributed::supervisor::RunState::Ready
                | goose_sidecar::distributed::supervisor::RunState::Serving
        ) || status.model_id.as_deref() != Some(model_id)
        {
            return Err(refuse(format!(
                "the distributed engine is not serving {model_id} (state {:?}, model {})",
                status.state,
                status.model_id.as_deref().unwrap_or("none")
            )));
        }
        let kind = match status.runner {
            Some(Runner::MlxLmTensor) => PlacementKind::Tensor,
            Some(Runner::PipelineQwen4) => PlacementKind::Pipeline,
            None => {
                return Err(refuse(
                    "the distributed engine reports no runner".to_string(),
                ))
            }
        };
        let key = PlacementKey {
            kind,
            nodes: config_run
                .nodes
                .iter()
                .map(|n| n.ssh.clone().unwrap_or_else(|| LOCAL.to_string()))
                .collect(),
            link: Some(
                match config_run.backend {
                    goose_sidecar::distributed::Backend::Jaccl => "jaccl",
                    goose_sidecar::distributed::Backend::Ring => "ring",
                }
                .to_string(),
            ),
        };
        if key.id() != placement_id {
            return Err(refuse(format!(
                "the distributed engine runs {}, not {placement_id}",
                key.id()
            )));
        }
        let measured = measure_nodes(Some(&config_run)).await;
        Ok(Target {
            base_url: status
                .base_url
                .ok_or_else(|| refuse("the distributed engine reports no base URL".to_string()))?,
            served: status
                .served_model_id
                .ok_or_else(|| refuse("the distributed engine reports no served id".to_string()))?,
            key,
            node_names: measured.iter().map(|m| m.input.name.clone()).collect(),
            chips: measured.iter().map(|m| m.input.chip.clone().ok()).collect(),
            kv_cache: None,
        })
    }

    pub(super) async fn measure_speed(
        req: MlxEngineMeasureSpeedRequest,
    ) -> Result<MlxEngineMeasureSpeedResponse, agent_client_protocol::Error> {
        let settings = load_engine_settings()?;
        let target = target(&req.model_id, &req.placement_id, &settings).await?;
        let mut workloads = vec![Workload::Chat];
        if req.long_document {
            workloads.push(Workload::LongDocument);
        }
        let store = speed_store();
        let mut records = Vec::new();
        for workload in workloads {
            let started_ms = now_ms();
            let nonce = format!("{started_ms:x}-{}", workload.as_str());
            let sample = bench::run_workload(&target.base_url, &target.served, workload, &nonce)
                .await
                .map_err(|e| {
                    agent_client_protocol::Error::internal_error().data(format!(
                        "measuring {} ({}): {e:#}",
                        req.placement_id,
                        workload.as_str()
                    ))
                })?;
            let record = SpeedRecord {
                model_id: req.model_id.clone(),
                placement: target.key.clone(),
                node_names: target.node_names.clone(),
                chips: target.chips.clone(),
                backend: planner::backend_of(target.key.kind).to_string(),
                context_bucket: context_bucket(sample.prompt_tokens),
                prompt_tokens: sample.prompt_tokens,
                completion_tokens: sample.completion_tokens,
                prefill_tps: Some(sample.prefill_tps),
                decode_tps: sample.decode_tps,
                ttft_ms: Some(sample.ttft_ms),
                recorded_at_ms: now_ms(),
                source: RecordSource::Benchmark,
                workload: Some(workload.as_str().to_string()),
                kv_cache: target.kv_cache.clone(),
            };
            store.append(&record).map_err(|e| {
                agent_client_protocol::Error::internal_error().data(format!("{e:#}"))
            })?;
            records.push(mirror(&record)?);
        }
        Ok(MlxEngineMeasureSpeedResponse { records })
    }

    pub(super) fn speed_history(
        req: MlxEngineSpeedHistoryRequest,
    ) -> Result<MlxEngineSpeedHistoryResponse, agent_client_protocol::Error> {
        let store = speed_store();
        let read = store
            .read()
            .map_err(|e| agent_client_protocol::Error::internal_error().data(format!("{e:#}")))?;
        let records = read
            .records
            .iter()
            .filter(|r| req.model_id.as_ref().is_none_or(|m| &r.model_id == m))
            .map(mirror)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MlxEngineSpeedHistoryResponse {
            records,
            store_errors: read.unreadable,
            path: store.path().display().to_string(),
        })
    }
}

#[cfg(unix)]
impl GooseAcpAgent {
    pub(super) async fn on_mlx_engine_placement_plan(
        &self,
        req: MlxEnginePlacementPlanRequest,
    ) -> Result<MlxEnginePlacementPlanResponse, agent_client_protocol::Error> {
        imp::placement_plan(req).await
    }

    pub(super) async fn on_mlx_engine_measure_speed(
        &self,
        req: MlxEngineMeasureSpeedRequest,
    ) -> Result<MlxEngineMeasureSpeedResponse, agent_client_protocol::Error> {
        imp::measure_speed(req).await
    }

    pub(super) async fn on_mlx_engine_speed_history(
        &self,
        req: MlxEngineSpeedHistoryRequest,
    ) -> Result<MlxEngineSpeedHistoryResponse, agent_client_protocol::Error> {
        imp::speed_history(req)
    }
}

#[cfg(not(unix))]
impl GooseAcpAgent {
    fn placement_unsupported<T>() -> Result<T, agent_client_protocol::Error> {
        Err(agent_client_protocol::Error::invalid_request()
            .data("placementUnsupported: the MLX placement planner requires macOS"))
    }

    pub(super) async fn on_mlx_engine_placement_plan(
        &self,
        _req: MlxEnginePlacementPlanRequest,
    ) -> Result<MlxEnginePlacementPlanResponse, agent_client_protocol::Error> {
        Self::placement_unsupported()
    }

    pub(super) async fn on_mlx_engine_measure_speed(
        &self,
        _req: MlxEngineMeasureSpeedRequest,
    ) -> Result<MlxEngineMeasureSpeedResponse, agent_client_protocol::Error> {
        Self::placement_unsupported()
    }

    pub(super) async fn on_mlx_engine_speed_history(
        &self,
        _req: MlxEngineSpeedHistoryRequest,
    ) -> Result<MlxEngineSpeedHistoryResponse, agent_client_protocol::Error> {
        Self::placement_unsupported()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use goose_sidecar::placement::planner::{Action, Badge, Outcome};
    use goose_sidecar::placement::store::{PlacementKey, RecordSource, SpeedRecord};

    fn mirror<T: serde::Serialize, D: serde::de::DeserializeOwned>(value: &T) -> D {
        serde_json::from_value(serde_json::to_value(value).unwrap()).unwrap_or_else(|e| {
            panic!(
                "{} does not mirror: {e}",
                serde_json::to_value(value).unwrap()
            )
        })
    }

    #[test]
    fn every_planner_variant_mirrors_onto_its_wire_dto() {
        for action in [
            Action::MountHere,
            Action::StartSplit {
                setup_matches: true,
            },
            Action::RemoteSingle,
            Action::Unavailable { reason: "r".into() },
        ] {
            let dto: MlxPlacementActionDto = mirror(&action);
            let wire = serde_json::to_value(&dto).unwrap();
            assert_eq!(wire, serde_json::to_value(&action).unwrap());
        }
        let split = serde_json::to_value(Action::StartSplit {
            setup_matches: true,
        })
        .unwrap();
        assert_eq!(
            split,
            serde_json::json!({"kind": "startSplit", "setupMatches": true})
        );
        for outcome in [
            Outcome::Best,
            Outcome::BestAvailableNow,
            Outcome::NotSupported { reason: "r".into() },
            Outcome::DoesNotFit,
            Outcome::FitUnknown { reason: "r".into() },
            Outcome::NoFigure { reason: "r".into() },
            Outcome::Slower {
                mine: 1.0,
                best: 2.0,
            },
            Outcome::TiedNeedsMoreMacs {
                mine: 1.0,
                best: 1.1,
            },
        ] {
            let _: MlxPlacementOutcomeDto = mirror(&outcome);
        }
        let tied = serde_json::to_value(Outcome::TiedNeedsMoreMacs {
            mine: 1.0,
            best: 1.1,
        })
        .unwrap();
        assert_eq!(tied["code"], "tiedNeedsMoreMacs");
        for badge in [
            Badge::FitsThisMac,
            Badge::FitsPeer { name: "n".into() },
            Badge::NeedsBothMacs,
            Badge::TooBig { short_bytes: 5 },
            Badge::Unknown { reason: "r".into() },
        ] {
            let _: MlxPlacementBadgeDto = mirror(&badge);
        }
        assert_eq!(
            serde_json::to_value(Badge::TooBig { short_bytes: 5 }).unwrap(),
            serde_json::json!({"kind": "tooBig", "shortBytes": 5})
        );
        let record = SpeedRecord {
            model_id: "m".into(),
            placement: PlacementKey::single("local"),
            node_names: vec!["Mac".into()],
            chips: vec![None],
            backend: "rapid-mlx".into(),
            context_bucket: 2048,
            prompt_tokens: 1900,
            completion_tokens: 256,
            prefill_tps: Some(1.0),
            decode_tps: None,
            ttft_ms: Some(2.0),
            recorded_at_ms: 3,
            source: RecordSource::Benchmark,
            workload: Some("chat".into()),
            kv_cache: None,
        };
        let dto: MlxSpeedRecordDto = mirror(&record);
        assert_eq!(dto.source, "benchmark");
        assert_eq!(dto.placement.kind, MlxPlacementKindDto::Single);
    }
}
