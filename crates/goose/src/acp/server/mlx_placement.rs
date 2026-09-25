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
    use goose_sidecar::distributed::exec::ExecOutput;
    use goose_sidecar::distributed::link_control::{
        self, DiscoverRequest, ExecAnswer, LinkCallError, LinkOp,
    };
    use goose_sidecar::distributed::plan::{self as dplan};
    use goose_sidecar::distributed::preflight::{loaded_files, run_fork_planner, NodeFigures};
    use goose_sidecar::distributed::probe::parse_gpu_ceiling;
    use goose_sidecar::distributed::{
        DistributedConfig, NodeExec, Runner, SystemExec, PIPELINE_DEFAULT_SLOTS,
    };
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
    use leanzero_link::state::MlxOp;
    use leanzero_link::wire::NodeStatus;
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

    /// A size the way the model picker writes it (`formatGb`): GiB labelled GB, whole from 10 up,
    /// one decimal below — the Run it note said "31.1 GiB" beside the picker's "31 GB" (Q-46).
    pub(super) fn gb_words(bytes: u64) -> String {
        let gb = bytes as f64 / goose_sidecar::GIB as f64;
        if gb >= 10.0 {
            format!("{gb:.0} GB")
        } else {
            format!("{gb:.1} GB")
        }
    }

    /// Why this Mac's figures count another mounted model's memory as free, in plain words.
    pub(super) fn mounted_here_note(mounted: &str, bytes: u64) -> String {
        format!(
            "Starting it here frees the {} {mounted} holds on this Mac: Run replaces that model, it \
             never adds a second one",
            gb_words(bytes)
        )
    }

    /// Why the linked Mac's figures count this model's memory there as free, in plain words.
    pub(super) fn moved_from_peer_note(mac: &str, bytes: u64) -> String {
        format!(
            "Moving it frees its {} on {mac}: Run moves the model, it never adds a second copy",
            gb_words(bytes)
        )
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
            split_refusal: None,
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
            split_refusal: None,
        };
        Measured {
            dto: node_dto(&input),
            input,
            models,
        }
    }

    /// A Link peer whose "Let my other Macs use this Mac › Run part of a split model" is off answers no
    /// discovery probe — but its goose still answers `mlxEngine/status` over the mesh, which
    /// carries its memory and GPU ceiling. The split stays planned (the model may need both Macs
    /// whatever the switch says) and names the switch as the step before it can start.
    async fn switched_off_peer(host: &str, node_id: &str, name: &str, why: &str) -> Measured {
        let status: Result<MlxEngineStatusResponse, String> = async {
            let manager = super::super::link::existing_link_manager()
                .ok_or_else(|| "LeanZero Link is not running in this goose".to_string())?;
            let body = serde_json::to_value(MlxEngineStatusRequest::default())
                .map_err(|e| e.to_string())?;
            let value = manager
                .mlx_proxy(node_id, MlxOp::Status, body)
                .await
                .map_err(|e| format!("its engine status over LeanZero Link: {e}"))?;
            serde_json::from_value(value).map_err(|e| format!("its engine status: {e}"))
        }
        .await;
        let gib = |v: f64| (v * goose_sidecar::GIB as f64) as u64;
        let memory = status.and_then(|s| {
            let s = s.status;
            if let Some(e) = s.memory_error {
                return Err(e);
            }
            Ok(NodeMemory {
                total_bytes: gib(s.total_memory_gb),
                available_bytes: gib(s.available_memory_gb),
                ceiling_bytes: s.gpu_ceiling_bytes.ok_or_else(|| {
                    s.gpu_ceiling_error.unwrap_or_else(|| {
                        format!(
                            "{name}'s goose does not report its GPU ceiling — update goose there"
                        )
                    })
                }),
            })
        });
        let unprobed = format!("not probed — {why}");
        let input = NodeInput {
            id: host.to_string(),
            name: name.to_string(),
            chip: Err(format!("{name}'s chip was {unprobed}")),
            memory,
            has_model: Err("not checked yet".to_string()),
            remote_single: Ok(()),
            split_refusal: Some(format!(
                "turn on \"Let my other Macs use this Mac › Run part of a split model\" on {name} \
                 (Providers › My Macs there)"
            )),
        };
        Measured {
            dto: node_dto(&input),
            input,
            models: Some(Err(format!("{name}'s models folder was {unprobed}"))),
        }
    }

    /// A Link peer, probed by its own goose (`LinkOp::Discover`); a switched-off peer is read
    /// through its engine status instead.
    async fn link_peer_measured(
        host: &str,
        node_id: &str,
        name: &str,
        roots: &[String],
    ) -> Measured {
        let answer: Result<ExecAnswer, LinkCallError> = link_control::call_typed(
            node_id,
            LinkOp::Discover,
            &DiscoverRequest {
                extra_roots: roots.to_vec(),
            },
        )
        .await;
        match answer {
            Ok(answer) => {
                let out: ExecOutput = answer.into();
                let probe = discover::parse_node(&out.stdout);
                let name = match &probe {
                    Ok(p) if !p.computer_name.is_empty() => p.computer_name.clone(),
                    _ => name.to_string(),
                };
                peer_measured(host, &name, probe)
            }
            Err(LinkCallError::Disabled(why)) => switched_off_peer(host, node_id, name, &why).await,
            Err(e) => peer_measured(
                host,
                name,
                Err(format!("discovery over LeanZero Link: {e}")),
            ),
        }
    }

    /// With no distributed setup on this Mac, the Macs it reaches over LeanZero Link are the ones
    /// a split would use — the badge must say "needs both Macs" from either side. `Err` = why
    /// there are none to ask.
    async fn link_peers() -> Result<Vec<(String, String)>, String> {
        link::ensure_link_transport();
        let manager = super::super::link::existing_link_manager()
            .ok_or_else(|| "LeanZero Link has not started in this goose".to_string())?;
        let registry = manager
            .active_registry()
            .await
            .ok_or_else(|| "this Mac is not connected to LeanZero Link".to_string())?;
        let self_id = super::super::link::stable_node_id();
        Ok(registry
            .peer_nodes()
            .into_iter()
            .filter(|p| p.node_id != self_id && p.status != NodeStatus::Offline)
            .map(|p| (link_control::link_host(&p.node_id), p.hostname))
            .collect())
    }

    /// This Mac and every configured peer — or, with no distributed setup here, every Mac on
    /// LeanZero Link — measured in parallel. The second value says why no peer could be asked.
    pub(super) async fn measure_nodes(
        config: Option<&DistributedConfig>,
    ) -> (Vec<Measured>, Option<String>) {
        let (peers, peer_gap): (Vec<(String, String)>, Option<String>) = match config {
            Some(c) => (
                c.nodes
                    .iter()
                    .filter_map(|n| n.ssh.clone().map(|h| (h, n.name.clone())))
                    .collect(),
                None,
            ),
            None => match link_peers().await {
                Ok(peers) => (peers, None),
                Err(why) => (Vec::new(), Some(why)),
            },
        };
        if peers.iter().any(|(h, _)| h.starts_with("link:")) {
            link::ensure_link_transport();
        }
        let ssh_peers: Vec<&(String, String)> = peers
            .iter()
            .filter(|(h, _)| link_control::link_peer(Some(h)).is_none())
            .collect();
        let hosts: Vec<Option<String>> = ssh_peers.iter().map(|(h, _)| Some(h.clone())).collect();
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
        let link_measures = futures::future::join_all(peers.iter().filter_map(|(host, name)| {
            let node_id = link_control::link_peer(Some(host))?;
            let roots = roots.get(&Some(host.clone())).cloned().unwrap_or_default();
            Some(async move { link_peer_measured(host, node_id, name, &roots).await })
        }));
        let (local, probes, linked) = tokio::join!(
            measure_local(config),
            discover::probe_nodes(exec, &hosts, &roots),
            link_measures
        );
        let mut by_ssh = ssh_peers
            .iter()
            .zip(probes)
            .map(|((host, name), probe)| peer_measured(host, name, probe));
        let mut by_link = linked.into_iter();
        let measured = std::iter::once(local)
            .chain(peers.iter().filter_map(|(host, _)| {
                if link_control::link_peer(Some(host)).is_some() {
                    by_link.next()
                } else {
                    by_ssh.next()
                }
            }))
            .collect();
        (measured, peer_gap)
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

    /// What is serving on this Mac right now, so the planner neither calls the running model
    /// "short" (its memory is in use) nor forgets that every other placement gets that memory back.
    pub(super) struct Serving {
        /// (model id, candidate id, context window) of the running placement.
        pub running: Vec<(String, String, Option<u64>)>,
        /// The single engine's resident bytes when a model is mounted here, and whose.
        pub single_footprint: Option<(String, u64)>,
        /// The Metal memory the engine on the linked Mac serving this Mac's chat holds:
        /// (its placement node id, model id, bytes, the Mac's name). Every other placement of that
        /// model on that Mac gets it back, because Run switches rather than adds a copy.
        pub peer_footprint: Option<(String, String, u64, String)>,
        pub notes: Vec<String>,
    }

    async fn serving(settings: &EngineSettings) -> Serving {
        let mut out = Serving {
            running: Vec::new(),
            single_footprint: None,
            peer_footprint: None,
            notes: Vec::new(),
        };
        let single = global_manager().status().await;
        if let (true, Some(model)) = (single.state == "running", single.model_id.clone()) {
            out.running.push((
                model.clone(),
                PlacementKey::single(LOCAL).id(),
                single.context_window,
            ));
            match goose_sidecar::placement::engine_resident_bytes(settings.port).await {
                Ok(bytes) => out.single_footprint = Some((model, bytes)),
                Err(e) => out.notes.push(format!(
                    "{model} is mounted here but its memory could not be read ({e:#}); this Mac's \
                     figures do not count it as free for other placements"
                )),
            }
        }
        let dist = goose_sidecar::distributed::global_manager().status();
        if dist.state.owns_the_mac() {
            if let (Some(config), Some(model), Some(runner)) =
                (dist.config.clone(), dist.model_id.clone(), dist.runner)
            {
                let key = PlacementKey {
                    kind: match runner {
                        Runner::MlxLmTensor => PlacementKind::Tensor,
                        Runner::PipelineQwen4 => PlacementKind::Pipeline,
                    },
                    nodes: config
                        .nodes
                        .iter()
                        .map(|n| n.ssh.clone().unwrap_or_else(|| LOCAL.to_string()))
                        .collect(),
                    link: Some(backend_name(config.backend).to_string()),
                };
                out.running
                    .push((model.clone(), key.id(), dist.context_limit));
                out.notes.push(format!(
                    "the distributed engine is running {model}; its ranks' memory is not counted as \
                     free for other placements"
                ));
            }
        }
        if let Some(route) = crate::providers::mlx_remote::read().live() {
            let node = format!("link:{}", route.peer);
            match super::super::mlx_remote_single::peer_engine_held_bytes(&route.base_url).await {
                Ok(bytes) => {
                    out.peer_footprint = Some((
                        node.clone(),
                        route.model_id.clone(),
                        bytes,
                        route.peer_name().to_string(),
                    ))
                }
                Err(e) => out.notes.push(format!(
                    "{} is served from {} but its memory could not be read ({e}); that Mac's \
                     figures do not count it as free for other placements",
                    route.model_id,
                    route.peer_name()
                )),
            }
            out.running
                .push((route.model_id, PlacementKey::single(&node).id(), None));
        }
        out
    }

    fn backend_name(backend: goose_sidecar::distributed::Backend) -> &'static str {
        match backend {
            goose_sidecar::distributed::Backend::Jaccl => "jaccl",
            goose_sidecar::distributed::Backend::Ring => "ring",
        }
    }

    pub(super) struct Context {
        pub serving: Serving,
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
        let ((mut measured, peer_gap), mut serving) =
            tokio::join!(measure_nodes(config.as_ref()), serving(&settings));
        if let Some(why) = peer_gap {
            serving
                .notes
                .push(format!("no other Mac could be asked: {why}"));
        }
        if let Some((_, bytes)) = &serving.single_footprint {
            if let Some(local) = measured.iter_mut().find(|m| m.input.id == LOCAL) {
                if let Ok(memory) = local.input.memory.as_mut() {
                    memory.available_bytes += bytes;
                }
            }
        }
        let read = speed_store()
            .read()
            .map_err(|e| agent_client_protocol::Error::internal_error().data(format!("{e:#}")))?;
        Ok(Context {
            serving,
            settings,
            models_dir,
            config,
            measured,
            records: read.records,
            store_errors: read.unreadable,
        })
    }

    /// The planner's own answer for one model; `Err` = its files could not be read.
    async fn plan_raw(
        ctx: &Context,
        model_id: &str,
        bytes_on_disk: u64,
        goal: Goal,
        context: Option<u64>,
        calibration: &Calibration,
    ) -> Result<planner::Plan, String> {
        let dir = ctx.models_dir.join(model_id);
        let facts: ModelFacts = read_model_facts(&dir).map_err(|e| format!("{e:#}"))?;
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
        // The linked Mac's copy of THIS model is what Run replaces there (a switch, never a second
        // copy), so every placement of it — the fork planner's included — sees that memory free.
        if let Some((node_id, model, bytes, _)) = &ctx.serving.peer_footprint {
            if model == model_id {
                if let Some(Ok(memory)) = nodes
                    .iter_mut()
                    .find(|n| &n.id == node_id)
                    .map(|n| n.memory.as_mut())
                {
                    memory.available_bytes += bytes;
                }
            }
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
        let cluster = match &ctx.config {
            Some(c) => Some(ClusterInput {
                link: Some(backend_name(c.backend).to_string()),
                slots: c.slots(),
                config_model_id: Some(c.model_id.clone()),
            }),
            None if nodes.len() >= 2 => Some(ClusterInput {
                link: None,
                slots: PIPELINE_DEFAULT_SLOTS,
                config_model_id: None,
            }),
            None => None,
        };
        Ok(planner::plan(&PlanInput {
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
            running: ctx
                .serving
                .running
                .iter()
                .find(|(model, _, _)| model == model_id)
                .map(|(_, id, context)| (id.clone(), *context)),
        }))
    }

    async fn plan_model(
        ctx: &Context,
        model_id: &str,
        bytes_on_disk: u64,
        goal_dto: MlxPlacementGoalDto,
        context: Option<u64>,
        calibration: &Calibration,
    ) -> Result<MlxPlacementPlanDto, agent_client_protocol::Error> {
        let plan = match plan_raw(
            ctx,
            model_id,
            bytes_on_disk,
            goal_of(goal_dto),
            context,
            calibration,
        )
        .await
        {
            Ok(plan) => plan,
            Err(error) => {
                return Ok(MlxPlacementPlanDto {
                    model_id: model_id.to_string(),
                    goal: goal_dto,
                    candidates: Vec::new(),
                    best: None,
                    best_available: None,
                    badge: None,
                    notes: Vec::new(),
                    error: Some(error),
                })
            }
        };
        let mut dto: MlxPlacementPlanDto = mirror(&plan)?;
        if let Some((mounted, bytes)) = &ctx.serving.single_footprint {
            if mounted != model_id {
                dto.notes.push(mounted_here_note(mounted, *bytes));
            }
        }
        if let Some((_, served, bytes, mac)) = &ctx.serving.peer_footprint {
            if served == model_id {
                dto.notes.push(moved_from_peer_note(mac, *bytes));
            }
        }
        dto.notes.extend(ctx.serving.notes.iter().cloned());
        Ok(dto)
    }

    /// What the single engine's mount refusal offers instead: the planner's placement for this
    /// model that is not this Mac alone (`planner::alternative_to_this_mac`), and the model's badge.
    pub(in crate::acp::server) async fn mount_alternative(
        model_id: &str,
        bytes_on_disk: u64,
    ) -> Result<
        (
            Option<MlxPlacementCandidateDto>,
            Option<MlxPlacementBadgeDto>,
        ),
        String,
    > {
        let ctx = context()
            .await
            .map_err(|e| format!("measuring the Macs for a placement: {}", e.message))?;
        let calibration = Calibration::fit(&BTreeMap::new());
        let plan = plan_raw(
            &ctx,
            model_id,
            bytes_on_disk,
            Goal::Chat,
            None,
            &calibration,
        )
        .await?;
        let candidate = planner::alternative_to_this_mac(&plan.candidates)
            .map(mirror)
            .transpose()
            .map_err(|e| e.message.to_string())?;
        let badge = mirror(&plan.badge).map_err(|e| e.message.to_string())?;
        Ok((candidate, Some(badge)))
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
            let (measured, _) = measure_nodes(config.as_ref()).await;
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
        let (measured, _) = measure_nodes(Some(&config_run)).await;
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

#[cfg(unix)]
pub(super) use imp::mount_alternative;

#[cfg(not(unix))]
pub(super) async fn mount_alternative(
    _model_id: &str,
    _bytes_on_disk: u64,
) -> Result<
    (
        Option<MlxPlacementCandidateDto>,
        Option<MlxPlacementBadgeDto>,
    ),
    String,
> {
    Err("the MLX placement planner requires macOS".to_string())
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

    /// Q-46: "runs on a linked Mac now: its 31.1 GiB there count as free for this model's other
    /// placements, because Run moves it rather than adding a copy" — jargon, and GiB beside the
    /// picker's "31 GB".
    #[test]
    fn the_run_it_notes_are_plain_words_in_the_pickers_gb() {
        let bytes = (31.1 * goose_sidecar::GIB as f64) as u64;
        assert_eq!(imp::gb_words(bytes), "31 GB");
        assert_eq!(imp::gb_words(goose_sidecar::GIB / 2), "0.5 GB");
        assert_eq!(
            imp::moved_from_peer_note("Work's Mac Studio", bytes),
            "Moving it frees its 31 GB on Work's Mac Studio: Run moves the model, it never adds \
             a second copy"
        );
        let here = imp::mounted_here_note("Qwen3.8-Flash", 18 * goose_sidecar::GIB);
        assert_eq!(
            here,
            "Starting it here frees the 18 GB Qwen3.8-Flash holds on this Mac: Run replaces that \
             model, it never adds a second one"
        );
        assert!(!here.contains("GiB") && !here.contains("count as free"));
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
            Badge::NeedsBothMacs { needs: None },
            Badge::NeedsBothMacs {
                needs: Some("allow it".into()),
            },
            Badge::TooBig { short_bytes: 5 },
            Badge::Unknown { reason: "r".into() },
        ] {
            let _: MlxPlacementBadgeDto = mirror(&badge);
        }
        assert_eq!(
            serde_json::to_value(Badge::TooBig { short_bytes: 5 }).unwrap(),
            serde_json::json!({"kind": "tooBig", "shortBytes": 5})
        );
        // A 3.0.25 desktop reads `{"kind": "needsBothMacs"}` — unchanged while nothing is needed.
        assert_eq!(
            serde_json::to_value(Badge::NeedsBothMacs { needs: None }).unwrap(),
            serde_json::json!({"kind": "needsBothMacs"})
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
