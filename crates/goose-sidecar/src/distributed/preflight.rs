//! Preflight: every node is asked the same questions in ONE script (one ssh round trip for a
//! peer), and every answer becomes a check with its numbers. A check that cannot be answered is
//! a FAIL naming why — never a pass by default.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::config::{Backend, DistributedConfig, NodeConfig, Runner};
use super::exec::{sh_quote, ExecOutput, NodeExec};
use super::link_control::link_peer;
use super::local_network::{self, PeerAnswer, PingLine};
use super::node_op::NodeOp;
use super::plan::{self, PipelinePlan, PipelineRatios, PipelineStage, RankPlan};
use super::probe::{self, Pressure};
use super::provision::{EnvSpec, PIPELINE_FORK_COMMIT};
use super::DERIVED_CONTEXT_MARGIN_RATIO;
use crate::GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckVerdict {
    Pass,
    Warn,
    Fail,
}

impl CheckVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckVerdict::Pass => "pass",
            CheckVerdict::Warn => "warn",
            CheckVerdict::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// reachable | foreignEngines | memory | model | modelManifest | python | tbIpv4 | ping |
    /// rdmaGid | portRange | ports | runner | plan | localNetworkPermission
    pub id: String,
    pub verdict: CheckVerdict,
    pub message: String,
}

impl Check {
    fn new(id: &str, verdict: CheckVerdict, message: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            verdict,
            message: message.into(),
        }
    }
    fn pass(id: &str, message: impl Into<String>) -> Self {
        Self::new(id, CheckVerdict::Pass, message)
    }
    fn warn(id: &str, message: impl Into<String>) -> Self {
        Self::new(id, CheckVerdict::Warn, message)
    }
    fn fail(id: &str, message: impl Into<String>) -> Self {
        Self::new(id, CheckVerdict::Fail, message)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePreflight {
    pub name: String,
    pub rank: usize,
    pub host: Option<String>,
    pub checks: Vec<Check>,
    pub available_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub pressure: Option<String>,
    pub plan: Option<RankPlan>,
    pub link_speed: Option<String>,
    pub mlx_version: Option<String>,
    /// The node's GPU ceiling: Metal's `max_recommended_working_set_size`, read on the node with
    /// its own mlx. `None` = it could not be read (a `gpuCeiling` FAIL says why).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_bytes: Option<u64>,
    /// `sysctl iogpu.wired_limit_mb` (0 = macOS's default wired ceiling), reported beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wired_limit_mb: Option<u64>,
    /// By how much this rank's plan exceeds its budget; `None` when it fits or has no plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_bytes: Option<u64>,
    /// The node's biggest apps by resident memory (what the owner could close), largest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_apps: Vec<probe::AppMemory>,
}

/// One node's measured memory figures and the budget the rule builds from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeFigures {
    pub available: u64,
    pub total: u64,
    pub ceiling: u64,
    pub budget: u64,
}

impl NodeFigures {
    pub fn new(available: u64, total: u64, ceiling: u64) -> Self {
        Self {
            available,
            total,
            ceiling,
            budget: plan::budget_bytes(available, total, ceiling),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightReport {
    pub ok: bool,
    pub ran_at_ms: u64,
    pub backend: Backend,
    pub runner: Option<Runner>,
    pub model_type: Option<String>,
    /// The context this launch allows (goose-side bookkeeping: mlx_lm.server has no context flag).
    pub context_limit: Option<u64>,
    /// "requested" (the config named it) | "derived" (the largest that fits every rank).
    pub context_source: Option<String>,
    /// The largest context every rank fits on the measured memory.
    pub max_context_fits: Option<u64>,
    /// The pipeline split the fork's planner approved (each rank's first layer); the launch pins
    /// it with `--split`. `None` for the tensor runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_starts: Option<Vec<u32>>,
    /// Pipeline only: the full-context sequences every rank's plan (state + workspace) was made
    /// for — the planner's `slots`, the server's `--slots`. `None` for the tensor runner, which
    /// has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    /// Cluster-wide checks (runner, cross-node model/version agreement, the plan).
    pub checks: Vec<Check>,
    pub nodes: Vec<NodePreflight>,
    /// Link repairs performed during this preflight, each with its before/after.
    pub repairs: Vec<String>,
}

impl PreflightReport {
    pub fn failures(&self) -> Vec<String> {
        let cluster = self
            .checks
            .iter()
            .filter(|c| c.verdict == CheckVerdict::Fail)
            .map(|c| format!("{}: {}", c.id, c.message));
        let nodes = self.nodes.iter().flat_map(|n| {
            n.checks
                .iter()
                .filter(|c| c.verdict == CheckVerdict::Fail)
                .map(move |c| format!("{} {}: {}", n.name, c.id, c.message))
        });
        cluster.chain(nodes).collect()
    }

    /// (planned bytes, prompt cache bytes) per rank for the launch.
    pub fn launch_bytes(&self) -> Option<Vec<(u64, u64)>> {
        self.nodes
            .iter()
            .map(|n| {
                n.plan
                    .as_ref()
                    .map(|p| (p.planned_bytes, p.prompt_cache_bytes))
            })
            .collect()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn gib(bytes: u64) -> String {
    format!("{:.2} GiB", bytes as f64 / GIB as f64)
}

const PIPELINE_MODULE: &str = "rapid_mlx.distributed.pipeline_qwen4";

/// Prints the node's Metal working-set ceiling in bytes (mlx 0.32's `mx.device_info()`).
pub const GPU_CEILING_PROBE: &str =
    "import mlx.core as mx; print(mx.device_info()['max_recommended_working_set_size'])";

/// How many of a node's biggest apps a short node names.
const TOP_APPS: usize = 3;

/// The ports a node's launch binds: rank 0 the API port (and JACCL's coordinator); under ring every
/// rank its `coordinator_port + rank`.
fn ports_for(config: &DistributedConfig, rank: usize) -> Vec<u16> {
    let mut ports = Vec::new();
    if rank == 0 {
        ports.push(config.port);
    }
    match config.backend {
        Backend::Jaccl if rank == 0 => ports.push(config.coordinator_port),
        Backend::Ring => ports.push(config.coordinator_port + rank as u16),
        Backend::Jaccl => {}
    }
    ports
}

pub(crate) fn node_probe_script(
    config: &DistributedConfig,
    runner: Option<Runner>,
    rank: usize,
) -> String {
    let node = &config.nodes[rank];
    let dir = sh_quote(&node.model_dir);
    let mut script = String::new();
    let mut add = |line: String| {
        script.push_str(&line);
        script.push('\n');
    };
    add("echo; echo @@self; echo $$".into());
    add("echo; echo @@vm; /usr/bin/vm_stat".into());
    add("echo; echo @@sysctl; /usr/sbin/sysctl -n hw.memsize kern.memorystatus_vm_pressure_level net.inet.ip.portrange.first".into());
    add("echo; echo @@ps; /bin/ps -axo pid=,command=".into());
    add(format!(
        "echo; echo @@ifconfig; /sbin/ifconfig {} 2>&1",
        sh_quote(&node.tb_interface)
    ));
    if config.backend == Backend::Jaccl {
        add(format!(
            "echo; echo @@gid; /usr/bin/ibv_devinfo -v -d {} 2>&1",
            sh_quote(&node.rdma_device)
        ));
    }
    add("echo; echo @@hwports; /usr/sbin/networksetup -listallhardwareports".into());
    add("echo; echo @@services; /usr/sbin/networksetup -listnetworkserviceorder".into());
    add(
        "echo; echo @@tb; /usr/sbin/system_profiler SPThunderboltDataType -json 2>/dev/null".into(),
    );
    add(format!(
        "echo; echo @@model; /usr/bin/find -L {dir} -maxdepth 1 -type f -exec /usr/bin/stat -L -f '%z %N' {{}} + 2>&1"
    ));
    add(format!(
        "echo; echo @@sums; for f in {dir}/SHA256SUMS {dir}/SHA256SUMS.txt; do if [ -f \"$f\" ]; then /usr/bin/shasum -a 256 \"$f\"; fi; done"
    ));
    add(format!(
        "echo; echo @@python; {} -c 'import mlx.core as mx, mlx_lm; print(mx.__version__, mlx_lm.__version__)' 2>&1",
        sh_quote(&node.python)
    ));
    add(format!(
        "echo; echo @@gpu; {} -c {} 2>&1",
        sh_quote(&node.python),
        sh_quote(GPU_CEILING_PROBE)
    ));
    add("echo; echo @@wiredlimit; /usr/sbin/sysctl -n iogpu.wired_limit_mb 2>&1".into());
    add("echo; echo @@rss; /bin/ps -axo rss=,comm=".into());
    if runner == Some(Runner::PipelineQwen4) {
        if let Some(python) = &node.pipeline_python {
            add(format!(
                "echo; echo @@pipeline; {} -m {PIPELINE_MODULE} --help 2>&1 | /usr/bin/head -3",
                sh_quote(python)
            ));
            add(format!(
                "echo; echo @@pipelineenv; {} -c {} 2>&1 | /usr/bin/tail -1",
                sh_quote(python),
                sh_quote(&EnvSpec::pipeline().check)
            ));
        }
    }
    let peers: Vec<String> = config
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != rank)
        .map(|(_, n)| sh_quote(&n.tb_ip))
        .collect();
    // A failure keeps ping's own error line (`ping: sendto: …`): EHOSTUNREACH there is how a
    // local network privacy refusal shows (see `local_network`).
    add(format!(
        "echo; echo @@ping; for ip in {}; do out=$(/sbin/ping -c 2 -t 3 \"$ip\" 2>&1); if [ $? -eq 0 ]; then echo \"$ip ok\"; else echo \"$ip fail $(printf '%s\\n' \"$out\" | /usr/bin/grep -m1 '^ping:')\"; fi; done",
        peers.join(" ")
    ));
    for port in ports_for(config, rank) {
        add(format!(
            "echo; echo @@listen{port}; /usr/sbin/lsof -nP -iTCP:{port} -sTCP:LISTEN -t 2>/dev/null"
        ));
    }
    add("echo; echo @@end".into());
    script
}

pub(crate) fn link_script(node: &NodeConfig, backend: Backend) -> String {
    let mut script = format!(
        "echo; echo @@ifconfig; /sbin/ifconfig {} 2>&1\n",
        sh_quote(&node.tb_interface)
    );
    if backend == Backend::Jaccl {
        script.push_str(&format!(
            "echo; echo @@gid; /usr/bin/ibv_devinfo -v -d {} 2>&1\n",
            sh_quote(&node.rdma_device)
        ));
    }
    script.push_str("echo; echo @@end\n");
    script
}

/// The documented repair for the soak's `RTR failed errno 96` class (and a missing TB IPv4):
/// toggle the node's TB network service and re-apply the manual /30 — no sudo needed.
pub(crate) fn repair_script(node: &NodeConfig) -> String {
    let service = sh_quote(&node.tb_service);
    format!(
        "/usr/sbin/networksetup -setnetworkserviceenabled {service} off && \
         /usr/sbin/networksetup -setnetworkserviceenabled {service} on && \
         /usr/sbin/networksetup -setmanual {service} {} {}",
        sh_quote(&node.tb_ip),
        sh_quote(&node.tb_netmask)
    )
}

/// The proof that licenses the repair: the configured service exists, sits on a hardware port
/// named "Thunderbolt N", and that port's device is the configured interface. Anything else is
/// not a TB service and is never toggled.
pub(crate) fn repair_licence(node: &NodeConfig, services_text: &str) -> Result<(), String> {
    let services = probe::parse_service_order(services_text);
    let Some((port, device, _)) = services.get(&node.tb_service) else {
        return Err(format!(
            "network service '{}' does not exist on this node",
            node.tb_service
        ));
    };
    if !port.starts_with("Thunderbolt ") {
        return Err(format!(
            "network service '{}' is on hardware port '{port}', not a Thunderbolt port — never toggled",
            node.tb_service
        ));
    }
    if device != &node.tb_interface {
        return Err(format!(
            "network service '{}' is on device {device}, the config names {}",
            node.tb_service, node.tb_interface
        ));
    }
    Ok(())
}

/// JACCL's link state on a node: the TB IPv4 present and its IPv4-mapped GID at index 1 (the
/// soak: a restored IPv4 at GID index 2 made the next start fail `RTR ... errno 96`).
fn link_checks(
    node: &NodeConfig,
    backend: Backend,
    sections: &BTreeMap<String, String>,
) -> (Vec<Check>, bool) {
    let mut checks = Vec::new();
    let mut needs_repair = false;
    match probe::section(sections, "ifconfig") {
        Ok(text) => {
            let addrs = probe::parse_ifconfig_ipv4(text);
            if addrs.iter().any(|a| a == &node.tb_ip) {
                checks.push(Check::pass(
                    "tbIpv4",
                    format!("{} carries {}", node.tb_interface, node.tb_ip),
                ));
            } else {
                needs_repair = true;
                checks.push(Check::fail(
                    "tbIpv4",
                    format!(
                        "{} does not carry {} (IPv4 present: {:?}); JACCL needs it for the \
                         IPv4-mapped GID, ring for its socket",
                        node.tb_interface, node.tb_ip, addrs
                    ),
                ));
            }
        }
        Err(e) => checks.push(Check::fail("tbIpv4", format!("{e:#}"))),
    }
    if backend == Backend::Jaccl {
        match probe::section(sections, "gid") {
            Ok(text) => {
                let table = probe::parse_gid_table(text);
                let want = probe::ipv4_gid(&node.tb_ip);
                let at = table.iter().find(|(_, g)| *g == want).map(|(i, _)| *i);
                match at {
                    Some(1) => checks.push(Check::pass(
                        "rdmaGid",
                        format!("{} GID[1] = {want}", node.rdma_device),
                    )),
                    Some(index) => {
                        needs_repair = true;
                        checks.push(Check::fail(
                            "rdmaGid",
                            format!(
                                "{} carries {want} at GID[{index}], not GID[1] — the next JACCL \
                                 start fails `Changing queue pair to RTR failed with errno 96` \
                                 (STEP1b); repair: toggle '{}' off/on and re-apply the manual IP",
                                node.rdma_device, node.tb_service
                            ),
                        ));
                    }
                    None => {
                        needs_repair = true;
                        checks.push(Check::fail(
                            "rdmaGid",
                            format!(
                                "{} has no IPv4-mapped GID {want} (`No IPv4-mapped GID for this \
                                 device` class); table: {table:?}",
                                node.rdma_device
                            ),
                        ));
                    }
                }
            }
            Err(e) => checks.push(Check::fail("rdmaGid", format!("{e:#}"))),
        }
    }
    (checks, needs_repair)
}

/// What one node answered, parsed.
struct NodeAnswer {
    checks: Vec<Check>,
    memory: Option<(crate::MemoryReading, Pressure)>,
    ceiling: Option<u64>,
    wired_limit_mb: Option<u64>,
    top_apps: Vec<probe::AppMemory>,
    model_files: Option<BTreeMap<String, u64>>,
    sums: BTreeMap<String, String>,
    mlx_version: Option<String>,
    link_speed: Option<String>,
    needs_repair: bool,
    services_text: Option<String>,
    pings: Vec<PingLine>,
}

fn read_answer(
    config: &DistributedConfig,
    runner: Option<Runner>,
    rank: usize,
    output: Result<ExecOutput>,
) -> NodeAnswer {
    let node = &config.nodes[rank];
    let mut answer = NodeAnswer {
        checks: Vec::new(),
        memory: None,
        ceiling: None,
        wired_limit_mb: None,
        top_apps: Vec::new(),
        model_files: None,
        sums: BTreeMap::new(),
        mlx_version: None,
        link_speed: None,
        needs_repair: false,
        services_text: None,
        pings: Vec::new(),
    };
    let output = match output {
        Ok(output) if output.ssh_failed() => {
            answer.checks.push(Check::fail(
                "reachable",
                format!(
                    "ssh {} failed: {}",
                    node.host().unwrap_or_default(),
                    output.stderr.trim()
                ),
            ));
            return answer;
        }
        Ok(output) => output,
        Err(e) => {
            answer
                .checks
                .push(Check::fail("reachable", format!("{e:#}")));
            return answer;
        }
    };
    let sections = probe::sections(&output.stdout);
    if !sections.contains_key("end") {
        answer.checks.push(Check::fail(
            "reachable",
            format!(
                "the probe script did not finish (exit {:?}): {}",
                output.status,
                output.stderr.trim()
            ),
        ));
        return answer;
    }
    answer.checks.push(Check::pass(
        "reachable",
        node.host()
            .map(|h| match link_peer(Some(h)) {
                Some(peer) => format!("LeanZero Link: {peer} answered through its own goose"),
                None => format!("ssh {h} answered"),
            })
            .unwrap_or_else(|| "this Mac".to_string()),
    ));

    let sysctl = probe::section(&sections, "sysctl").and_then(|t| probe::parse_sysctl_values(t, 3));
    let memory = sysctl.as_ref().map_err(|e| format!("{e:#}")).and_then(|v| {
        let reading = probe::section(&sections, "vm")
            .and_then(|t| probe::parse_vm_stat(t, v[0]))
            .map_err(|e| format!("{e:#}"))?;
        let pressure = Pressure::from_level(v[1]).map_err(|e| format!("{e:#}"))?;
        Ok((reading, pressure))
    });
    match memory {
        Ok(memory) => answer.memory = Some(memory),
        Err(e) => answer
            .checks
            .push(Check::fail("memory", format!("memory probe failed: {e}"))),
    }

    match probe::section(&sections, "gpu").and_then(probe::parse_gpu_ceiling) {
        Ok(ceiling) => answer.ceiling = Some(ceiling),
        Err(e) => answer.checks.push(Check::fail(
            "gpuCeiling",
            format!(
                "the GPU ceiling (Metal max_recommended_working_set_size) could not be read with \
                 {}: {e:#} — the budget is built from it, so there is no plan without it",
                node.python
            ),
        )),
    }
    answer.wired_limit_mb = sections
        .get("wiredlimit")
        .and_then(|t| t.trim().parse().ok());
    answer.top_apps = sections
        .get("rss")
        .map(|t| probe::top_apps_by_rss(t, TOP_APPS))
        .unwrap_or_default();

    let own_pid: Vec<u32> = probe::section(&sections, "self")
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .into_iter()
        .collect();
    match probe::section(&sections, "ps") {
        Ok(text) => answer
            .checks
            .push(foreign_engines_check(&probe::classify_foreign_engines(
                text, &own_pid,
            ))),
        Err(e) => answer
            .checks
            .push(Check::fail("foreignEngines", format!("{e:#}"))),
    }

    let (link, needs_repair) = link_checks(node, config.backend, &sections);
    answer.checks.extend(link);
    answer.needs_repair = needs_repair;
    answer.services_text = sections.get("services").cloned();

    if let (Ok(ports), Ok(tb)) = (
        probe::section(&sections, "hwports"),
        probe::section(&sections, "tb"),
    ) {
        if let Some(port) = probe::hardware_port_for_device(ports, &node.tb_interface) {
            answer.link_speed = probe::thunderbolt_speed(tb, &port).ok().flatten();
        }
    }

    match probe::section(&sections, "ping") {
        Ok(text) => {
            answer.pings = local_network::parse_ping_lines(text);
            let failed: Vec<String> = answer
                .pings
                .iter()
                .filter(|p| !p.ok)
                .map(|p| match &p.reason {
                    Some(reason) => format!("{} ({reason})", p.ip),
                    None => p.ip.clone(),
                })
                .collect();
            if failed.is_empty() {
                answer.checks.push(Check::pass(
                    "ping",
                    format!("peers answer: {}", text.trim().replace('\n', ", ")),
                ));
            } else {
                answer.checks.push(Check::fail(
                    "ping",
                    format!("no ping answer over the TB link from {}", failed.join(", ")),
                ));
            }
        }
        Err(e) => answer.checks.push(Check::fail("ping", format!("{e:#}"))),
    }

    match probe::section(&sections, "model") {
        Ok(text) if text.contains("No such file") => answer
            .checks
            .push(Check::fail("model", missing_model_message(node, rank))),
        Ok(text) => answer.model_files = Some(probe::parse_file_sizes(text)),
        Err(e) => answer.checks.push(Check::fail("model", format!("{e:#}"))),
    }
    answer.sums = sections
        .get("sums")
        .map(|t| probe::parse_shasums(t))
        .unwrap_or_default();

    match probe::section(&sections, "python") {
        Ok(text) => {
            let line = text
                .lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty())
                .unwrap_or_default();
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2
                && parts
                    .iter()
                    .all(|p| p.chars().next().is_some_and(|c| c.is_ascii_digit()))
            {
                answer.mlx_version = Some(format!("mlx {} · mlx_lm {}", parts[0], parts[1]));
                answer.checks.push(Check::pass(
                    "python",
                    format!(
                        "{} imports mlx {} and mlx_lm {}",
                        node.python, parts[0], parts[1]
                    ),
                ));
            } else {
                answer.checks.push(Check::fail(
                    "python",
                    format!(
                        "{} cannot import mlx + mlx_lm: {}",
                        node.python,
                        text.trim()
                    ),
                ));
            }
        }
        Err(e) => answer.checks.push(Check::fail("python", format!("{e:#}"))),
    }

    if let Ok(values) = &sysctl {
        let first_ephemeral = values[2];
        let last_port = config.coordinator_port as u64 + config.size() as u64 - 1;
        if last_port < first_ephemeral {
            answer.checks.push(Check::pass(
                "portRange",
                format!(
                    "coordinator ports {}..={last_port} sit below the ephemeral range ({first_ephemeral})",
                    config.coordinator_port
                ),
            ));
        } else {
            answer.checks.push(Check::fail(
                "portRange",
                format!(
                    "coordinator port {last_port} is inside the ephemeral range (net.inet.ip.portrange.first \
                     = {first_ephemeral}): a rank's outgoing connect can take it (`[ring] Couldn't bind \
                     socket (error: 48)`)"
                ),
            ));
        }
    }

    let busy: Vec<String> = ports_for(config, rank)
        .into_iter()
        .filter_map(|port| {
            let pids = sections.get(&format!("listen{port}"))?.trim().to_string();
            (!pids.is_empty()).then(|| format!("{port} (pid {})", pids.replace('\n', ",")))
        })
        .collect();
    let ports = ports_for(config, rank);
    if ports.is_empty() {
        answer.checks.push(Check::pass(
            "ports",
            "this rank binds no port (a JACCL worker)",
        ));
    } else if busy.is_empty() {
        answer
            .checks
            .push(Check::pass("ports", format!("{ports:?} free")));
    } else {
        answer.checks.push(Check::fail(
            "ports",
            format!("already listening: {}", busy.join("; ")),
        ));
    }

    if runner == Some(Runner::PipelineQwen4) {
        answer.checks.push(pipeline_runner_check(
            node,
            sections.get("pipeline"),
            sections.get("pipelineenv"),
        ));
    }
    answer
}

/// A foreign DISTRIBUTED process (mlx.launch, the fork's pipeline, a goose rank another goosed
/// left) holds a coordinator port or an RDMA queue pair: FAIL. A foreign SINGLE server (the
/// owner's `rapid-mlx serve` on its own port) is an independent engine: its resident memory is
/// already out of the `available` figure the memory check plans against, so it is a WARN naming
/// the pid and the cost it does carry (GPU contention: decode on this node slows while it works).
fn foreign_engines_check(foreign: &[(u32, String, probe::ForeignKind)]) -> Check {
    let list = |kind: probe::ForeignKind| {
        foreign
            .iter()
            .filter(|(_, _, k)| *k == kind)
            .map(|(pid, cmd, _)| format!("pid {pid} `{cmd}`"))
            .collect::<Vec<_>>()
    };
    let distributed = list(probe::ForeignKind::Distributed);
    let single = list(probe::ForeignKind::SingleServer);
    if !distributed.is_empty() {
        return Check::fail(
            "foreignEngines",
            format!(
                "another distributed MLX process runs on this node (it holds a coordinator port or \
                 an RDMA queue pair this launch needs): {}",
                distributed.join("; ")
            ),
        );
    }
    if !single.is_empty() {
        return Check::warn(
            "foreignEngines",
            format!(
                "a single MLX server shares this node: {} — its memory is already outside the \
                 available figure the plan is measured against; it contends for the GPU, so \
                 decode here slows while it serves",
                single.join("; ")
            ),
        );
    }
    Check::pass("foreignEngines", "no other MLX engine runs here")
}

/// The qwen4_exp split serves through the fork's `pipeline_qwen4 serve` (the rank program calls
/// its `serve()`): the module must offer that subcommand, and a goose-managed fork env must be
/// the pinned commit — an env built from an earlier pin could offer `serve` with another
/// contract. An operator's own interpreter on another commit is a WARN naming both.
fn pipeline_runner_check(
    node: &NodeConfig,
    help: Option<&String>,
    env_answer: Option<&String>,
) -> Check {
    let Some(python) = &node.pipeline_python else {
        return Check::fail(
            "runner",
            "the qwen4_exp pipeline runner needs pipeline_python (the fork's interpreter) on every node",
        );
    };
    let Some(help) = help else {
        return Check::fail(
            "runner",
            format!("{python} did not answer `{PIPELINE_MODULE} --help`"),
        );
    };
    let offered = help
        .split('{')
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
    let commands = match offered {
        Some(commands) if commands.iter().any(|c| c == "serve") => commands,
        Some(commands) => {
            return Check::fail(
                "runner",
                format!(
                    "{PIPELINE_MODULE} on {python} offers {commands:?}, not `serve` — this env \
                     predates the pinned fork ({PIPELINE_FORK_COMMIT}); provision it again"
                ),
            )
        }
        None => {
            return Check::fail(
                "runner",
                format!("{python} -m {PIPELINE_MODULE} --help: {}", help.trim()),
            )
        }
    };
    let pinned = EnvSpec::pipeline().expect;
    let answer = env_answer.map(|a| a.trim()).unwrap_or_default();
    if answer == pinned {
        return Check::pass(
            "runner",
            format!("{PIPELINE_MODULE} offers {commands:?}; {python} imports {answer}"),
        );
    }
    let what = format!(
        "{python} imports '{answer}', the pinned fork is '{pinned}' (mlx, mlx_lm, fork commit)"
    );
    if EnvSpec::managed_by(python).is_some() {
        Check::fail(
            "runner",
            format!("{what} — the goose-managed env is stale; provision it again"),
        )
    } else {
        Check::warn(
            "runner",
            format!("{what} — an operator's own interpreter, used as configured"),
        )
    }
}

/// A model directory absent on a node. The fix that exists is the Thunderbolt copy (Models tab ›
/// Downloaded › "Copy to <device> · Thunderbolt", `mlx_replica`): its control messages ride the
/// LeanZero Link mesh, so it needs both Macs signed in to Link — without that, copy the directory
/// over the cable yourself (rsync to the peer's TB address).
fn missing_model_message(node: &NodeConfig, rank: usize) -> String {
    if rank == 0 {
        return format!("{} does not exist on this Mac", node.model_dir);
    }
    format!(
        "{} does not exist on {} — copy it over Thunderbolt from this Mac's Models tab \
         (Downloaded › Copy to {}; that copy needs both Macs signed in to LeanZero Link), then \
         Detect again",
        node.model_dir, node.name, node.name
    )
}

/// The files a rank loads: config, tokenizer, the index and every `model*.safetensors` (what
/// mlx_lm's loader globs). Compared by name AND size across nodes.
pub fn loaded_files(files: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    files
        .iter()
        .filter(|(name, _)| {
            name.as_str() == "config.json"
                || name.starts_with("tokenizer")
                || name.as_str() == "model.safetensors.index.json"
                || (name.starts_with("model") && name.ends_with(".safetensors"))
        })
        .map(|(n, s)| (n.clone(), *s))
        .collect()
}

pub async fn run_preflight(
    config: &DistributedConfig,
    exec: Arc<dyn NodeExec>,
    repair_link: bool,
) -> Result<PreflightReport> {
    config.validate()?;
    let mut report = PreflightReport {
        ok: false,
        ran_at_ms: now_ms(),
        backend: config.backend,
        runner: None,
        model_type: None,
        context_limit: None,
        context_source: None,
        max_context_fits: None,
        pipeline_starts: None,
        slots: None,
        checks: Vec::new(),
        nodes: Vec::new(),
        repairs: Vec::new(),
    };
    let rank0_dir = Path::new(&config.nodes[0].model_dir);
    let runner = match plan::read_model_type(rank0_dir) {
        Ok(model_type) => {
            report.model_type = Some(model_type.clone());
            match Runner::for_model_type(&model_type) {
                Ok(runner) => Some(runner),
                Err(e) => {
                    report.checks.push(Check::fail("runner", format!("{e:#}")));
                    None
                }
            }
        }
        Err(e) => {
            report.checks.push(Check::fail("runner", format!("{e:#}")));
            None
        }
    };
    report.runner = runner;

    let answers = futures::future::join_all((0..config.size()).map(|rank| {
        let exec = Arc::clone(&exec);
        let op = NodeOp::Probe {
            config: config.clone(),
            runner,
            rank,
        };
        let host = config.nodes[rank].ssh.clone();
        async move { exec.run_op(host.as_deref(), &op).await }
    }))
    .await;
    let mut answers: Vec<NodeAnswer> = answers
        .into_iter()
        .enumerate()
        .map(|(rank, output)| read_answer(config, runner, rank, output))
        .collect();

    for (rank, answer) in answers.iter_mut().enumerate() {
        if !answer.needs_repair {
            continue;
        }
        let node = &config.nodes[rank];
        let licence = answer
            .services_text
            .as_deref()
            .ok_or_else(|| "the node did not list its network services".to_string())
            .and_then(|text| repair_licence(node, text));
        let note = match (&licence, repair_link) {
            (Err(why), _) => format!("repair not possible on {}: {why}", node.name),
            (Ok(()), false) => format!(
                "repair available on {} (toggle '{}' + re-apply {}/{}); not performed — a dry run changes no network setting",
                node.name, node.tb_service, node.tb_ip, node.tb_netmask
            ),
            (Ok(()), true) => {
                let (checks, summary) = repair_node(config, &exec, rank).await;
                report.repairs.push(summary);
                answer
                    .checks
                    .retain(|c| c.id != "tbIpv4" && c.id != "rdmaGid");
                answer.checks.extend(checks);
                continue;
            }
        };
        answer.checks.push(Check::warn("linkRepair", note));
    }

    local_network_checks(config, &mut answers);

    // Cross-node agreement: the same files at the same sizes, the same manifest, the same mlx.
    let reference = answers[0].model_files.as_ref().map(loaded_files);
    for (rank, answer) in answers.iter_mut().enumerate() {
        let node = &config.nodes[rank];
        let Some(files) = answer.model_files.as_ref().map(loaded_files) else {
            continue;
        };
        if !files.contains_key("config.json") || !files.keys().any(|n| n.ends_with(".safetensors"))
        {
            answer.checks.push(Check::fail(
                "model",
                format!(
                    "{} holds no config.json + model*.safetensors",
                    node.model_dir
                ),
            ));
            continue;
        }
        let total: u64 = files.values().sum();
        match &reference {
            Some(reference) if rank > 0 && &files != reference => {
                let differing: Vec<String> = reference
                    .iter()
                    .filter(|(n, s)| files.get(*n) != Some(s))
                    .map(|(n, s)| {
                        format!(
                            "{n} ({} here vs {s} on rank 0)",
                            files
                                .get(n)
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "absent".to_string())
                        )
                    })
                    .chain(
                        files
                            .keys()
                            .filter(|n| !reference.contains_key(*n))
                            .map(|n| format!("{n} (not on rank 0)")),
                    )
                    .collect();
                answer.checks.push(Check::fail(
                    "model",
                    format!(
                        "{} differs from rank 0's copy: {}",
                        node.model_dir,
                        differing.join(", ")
                    ),
                ));
            }
            _ => answer.checks.push(Check::pass(
                "model",
                format!(
                    "{} files, {} at {}",
                    files.len(),
                    gib(total),
                    node.model_dir
                ),
            )),
        }
    }
    let rank0_sums = answers[0].sums.clone();
    for answer in answers.iter_mut() {
        if rank0_sums.is_empty() {
            answer.checks.push(Check::pass(
                "modelManifest",
                "no SHA256SUMS manifest on rank 0; files compared by name and size",
            ));
        } else if answer.sums == rank0_sums {
            answer.checks.push(Check::pass(
                "modelManifest",
                format!("manifest identical to rank 0's: {:?}", answer.sums),
            ));
        } else {
            answer.checks.push(Check::fail(
                "modelManifest",
                format!(
                    "SHA256SUMS differs from rank 0's ({:?} vs {rank0_sums:?})",
                    answer.sums
                ),
            ));
        }
    }
    let versions: Vec<&Option<String>> = answers.iter().map(|a| &a.mlx_version).collect();
    if versions.iter().all(|v| v.is_some()) && versions.windows(2).any(|w| w[0] != w[1]) {
        report.checks.push(Check::fail(
            "python",
            format!("the nodes run different mlx/mlx_lm versions: {versions:?}"),
        ));
    }

    // The plan, per rank, against each node's measured budget.
    let budgets: Vec<Option<NodeFigures>> = answers
        .iter()
        .map(|a| {
            let (memory, _) = a.memory?;
            Some(NodeFigures::new(
                memory.available_bytes,
                memory.total_bytes,
                a.ceiling?,
            ))
        })
        .collect();
    let mut plans: Vec<Option<RankPlan>> = vec![None; config.size()];
    let mut pipeline_ratios = None;
    if let (Some(runner), true) = (runner, budgets.iter().all(Option::is_some)) {
        let budgets: Vec<NodeFigures> = budgets.iter().map(|b| b.unwrap()).collect();
        match runner {
            Runner::MlxLmTensor => {
                plan_tensor(config, &budgets, &mut report, &mut plans);
            }
            Runner::PipelineQwen4 => {
                pipeline_ratios =
                    plan_pipeline(config, &exec, &budgets, &mut report, &mut plans).await;
            }
        }
    } else if runner.is_some() {
        report.checks.push(Check::fail(
            "plan",
            "no plan: a node's memory or GPU ceiling could not be measured (its memory / \
             gpuCeiling check says why)",
        ));
    }

    for (rank, mut answer) in answers.into_iter().enumerate() {
        let node = &config.nodes[rank];
        let plan = plans[rank].clone();
        if let (Some((reading, pressure)), Some(plan)) = (answer.memory, &plan) {
            let measured = format!(
                "available {} of {} (pressure {})",
                gib(reading.available_bytes),
                gib(reading.total_bytes),
                pressure.as_str(),
            );
            let head = match &pipeline_ratios {
                Some(ratios) => format!(
                    "{measured}; {}",
                    stage_line(plan, ratios, reading.available_bytes, answer.ceiling)
                ),
                None => format!(
                    "{measured}; budget {} = min(available − RAM × {:.2}, GPU ceiling {}); \
                     planned {} (weights {} + state {} + workspace {} + prompt cache {}) × {:.2} = {}",
                    gib(plan.budget_bytes),
                    super::AVAILABLE_MARGIN_RATIO,
                    answer.ceiling.map(gib).unwrap_or_else(|| "unread".to_string()),
                    gib(plan.planned_bytes),
                    gib(plan.weights_bytes),
                    gib(plan.state_bytes),
                    gib(plan.workspace_bytes),
                    gib(plan.prompt_cache_bytes),
                    super::RUNTIME_OVERHEAD_RATIO,
                    gib(plan.with_overhead_bytes),
                ),
            };
            let check = if pressure == Pressure::Critical {
                Check::fail(
                    "memory",
                    format!("{head} — kernel memory pressure is CRITICAL"),
                )
            } else if !plan.fits {
                Check::fail(
                    "memory",
                    format!(
                        "{head} — exceeds the budget by {}",
                        gib(plan.with_overhead_bytes.saturating_sub(plan.budget_bytes))
                    ),
                )
            } else if pressure == Pressure::Warn {
                Check::warn(
                    "memory",
                    format!("{head} — fits, but kernel memory pressure is WARN"),
                )
            } else {
                Check::pass(
                    "memory",
                    format!(
                        "{head} — fits ({:.0}% of budget)",
                        100.0 * plan.with_overhead_bytes as f64 / plan.budget_bytes as f64
                    ),
                )
            };
            answer.checks.push(check);
        }
        report.nodes.push(NodePreflight {
            name: node.name.clone(),
            rank,
            host: node.ssh.clone(),
            checks: answer.checks,
            available_bytes: answer.memory.map(|(m, _)| m.available_bytes),
            total_bytes: answer.memory.map(|(m, _)| m.total_bytes),
            pressure: answer.memory.map(|(_, p)| p.as_str().to_string()),
            short_bytes: plan
                .as_ref()
                .filter(|p| !p.fits)
                .map(|p| p.with_overhead_bytes.saturating_sub(p.budget_bytes)),
            plan,
            link_speed: answer.link_speed,
            mlx_version: answer.mlx_version,
            ceiling_bytes: answer.ceiling,
            wired_limit_mb: answer.wired_limit_mb,
            top_apps: answer.top_apps,
        });
    }
    report.ok = report.failures().is_empty() && report.nodes.iter().all(|n| n.plan.is_some());
    Ok(report)
}

/// An APP's process tree is subject to its Local Network privilege: this Mac's, and a LeanZero Link
/// node's (its goosed runs the probe); a peer probed over ssh is exempt.
fn local_network_checks(config: &DistributedConfig, answers: &mut [NodeAnswer]) {
    let answered: Vec<bool> = answers
        .iter()
        .map(|a| {
            a.checks
                .iter()
                .any(|c| c.id == "reachable" && c.verdict == CheckVerdict::Pass)
        })
        .collect();
    // A LeanZero Link node's probe runs in its goosed — an app, like this Mac's.
    let findings: Vec<(usize, String)> = config
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.ssh.is_none() || link_peer(node.host()).is_some())
        .filter_map(|(rank, node)| {
            let peers: Vec<PeerAnswer> = config
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != rank)
                .map(|(i, peer)| PeerAnswer {
                    name: &peer.name,
                    tb_ip: &peer.tb_ip,
                    answered: answered[i],
                    pings: &answers[i].pings,
                })
                .collect();
            match node.ssh {
                None => local_network::diagnose(&node.tb_ip, &answers[rank].pings, &peers)
                    .map(|evidence| (rank, format!("{} ({evidence})", local_network::BLOCKED))),
                Some(_) => local_network::diagnose_on(
                    &node.name,
                    &node.tb_ip,
                    &answers[rank].pings,
                    &peers,
                )
                .map(|evidence| {
                    (
                        rank,
                        format!("{} ({evidence})", local_network::blocked_on(&node.name)),
                    )
                }),
            }
        })
        .collect();
    for (rank, message) in findings {
        answers[rank]
            .checks
            .push(Check::fail(local_network::CHECK_ID, message));
    }
}

fn plan_tensor(
    config: &DistributedConfig,
    budgets: &[NodeFigures],
    report: &mut PreflightReport,
    plans: &mut [Option<RankPlan>],
) {
    let ranks = config.size() as u64;
    let facts = match plan::read_tensor_facts(Path::new(&config.nodes[0].model_dir)) {
        Ok(facts) => facts,
        Err(e) => {
            report.checks.push(Check::fail("plan", format!("{e:#}")));
            return;
        }
    };
    if let Err(e) = facts.check_divisible(ranks) {
        report.checks.push(Check::fail("plan", format!("{e:#}")));
        return;
    }
    let ceiling = budgets
        .iter()
        .map(|figures| facts.max_context(ranks, figures.budget))
        .min()
        .unwrap_or(0);
    report.max_context_fits = Some(ceiling);
    let context = match config.context {
        Some(requested) => {
            report.context_source = Some("requested".to_string());
            if requested > facts.max_position {
                report.checks.push(Check::fail(
                    "plan",
                    format!(
                        "requested context {requested} exceeds the model's max_position_embeddings {}",
                        facts.max_position
                    ),
                ));
            }
            requested
        }
        None => {
            report.context_source = Some("derived".to_string());
            if ceiling == 0 {
                report.checks.push(Check::fail(
                    "plan",
                    "no context fits: a rank's weights alone exceed its budget",
                ));
            }
            ceiling
        }
    };
    report.context_limit = Some(context);
    for (rank, figures) in budgets.iter().enumerate() {
        plans[rank] = Some(facts.rank_plan(ranks, rank as u64, context.max(1), figures.budget));
    }
    report.checks.push(Check::pass(
        "plan",
        format!(
            "tensor split over {ranks} ranks: {} per rank weights ({} sharded / {ranks} + {} embed+lm_head whole), \
             KV {} per token per rank, context {context} ({}), largest that fits {ceiling}",
            gib(facts.weights_per_rank(ranks)),
            gib(facts.sharded_bytes),
            gib(facts.replicated_bytes),
            facts.kv_bytes_per_token(ranks),
            report.context_source.as_deref().unwrap_or_default(),
        ),
    ));
}

/// One pipeline rank's plan line: its layers and the fork's bytes against the fork's budget, with
/// the available figure and GPU ceiling the fork built that budget from.
fn pipeline_stage_line(stage: &PipelineStage, ratios: &PipelineRatios) -> String {
    stage_line(
        &stage.rank_plan(),
        ratios,
        stage.available_bytes,
        Some(stage.ceiling_bytes),
    )
}

fn stage_line(
    plan: &RankPlan,
    ratios: &PipelineRatios,
    available: u64,
    ceiling: Option<u64>,
) -> String {
    format!(
        "layers [{}, {}): weights {} + state {} + workspace {} = {} of budget {} = \
         min(available {} − RAM × {:.2}, GPU ceiling {}) (the fork's plan, {:.0}%) → {}",
        plan.layer_start,
        plan.layer_end,
        gib(plan.weights_bytes),
        gib(plan.state_bytes),
        gib(plan.workspace_bytes),
        gib(plan.with_overhead_bytes),
        gib(plan.budget_bytes),
        gib(available),
        ratios.available_margin,
        ceiling.map(gib).unwrap_or_else(|| "unread".to_string()),
        100.0 * plan.with_overhead_bytes as f64 / plan.budget_bytes.max(1) as f64,
        if plan.fits { "fits" } else { "DOES NOT FIT" },
    )
}

/// Run the fork's planner once: `plan --json` over every node's RAM and goose's measured
/// available memory, at the batch the ranks are launched with. Exit 0 = fits, 2 = does not fit
/// (JSON either way); anything else is the planner failing, named with its own last words.
pub async fn run_fork_planner(
    config: &DistributedConfig,
    exec: &Arc<dyn NodeExec>,
    python: &str,
    budgets: &[NodeFigures],
    context: Option<u64>,
) -> Result<PipelinePlan> {
    let rank0 = &config.nodes[0];
    let nodes: Vec<String> = config
        .nodes
        .iter()
        .zip(budgets)
        .map(|(node, figures)| {
            plan::planner_node_arg(
                &node.name,
                figures.total,
                figures.available,
                figures.ceiling,
            )
        })
        .collect();
    let mut script = format!(
        "{} -m {PIPELINE_MODULE} plan --json --model {}",
        sh_quote(python),
        sh_quote(&rank0.model_dir)
    );
    for node in &nodes {
        script.push_str(&format!(" --node {}", sh_quote(node)));
    }
    script.push_str(&format!(" --batch {}", config.slots()));
    if let Some(context) = context {
        script.push_str(&format!(" --context {context}"));
    }
    let out = exec.run(None, &script).await?;
    let failed = || {
        let words: Vec<&str> = out
            .stderr
            .lines()
            .chain(out.stdout.lines())
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        format!(
            "the fork planner exited {:?} for nodes {nodes:?} (RAM GiB : available GiB : GPU ceiling GiB): {}",
            out.status,
            words[words.len().saturating_sub(3)..].join(" | ")
        )
    };
    let verdict = match out.status {
        Some(0) => true,
        Some(2) => false,
        _ => anyhow::bail!(failed()),
    };
    let plan = plan::parse_pipeline_plan(&out.stdout).with_context(failed)?;
    anyhow::ensure!(
        plan.fits == verdict,
        "the fork planner exited {:?} but its JSON says fits = {}",
        out.status,
        plan.fits
    );
    Ok(plan)
}

/// The qwen4_exp plan, read from the fork. A requested context is planned as asked. A derived one
/// walks the planner's own ceiling to its fixed point: the first plan (at the model's full
/// context) names the largest context ITS split fits; re-planning at that ceiling re-balances the
/// split, whose ceiling is at least as large (the re-balanced split's worst rank is no worse), and
/// so on until the ceiling equals the planned context. The walk ends on progress — a context
/// already planned is never planned again — never on a count.
async fn plan_pipeline(
    config: &DistributedConfig,
    exec: &Arc<dyn NodeExec>,
    budgets: &[NodeFigures],
    report: &mut PreflightReport,
    plans: &mut [Option<RankPlan>],
) -> Option<PipelineRatios> {
    let python = config.nodes[0].pipeline_python.as_deref()?;
    report.context_source = Some(
        if config.context.is_some() {
            "requested"
        } else {
            "derived"
        }
        .to_string(),
    );
    let planned = if let Some(context) = config.context {
        match run_fork_planner(config, exec, python, budgets, Some(context)).await {
            Ok(plan) => plan,
            Err(e) => {
                report.checks.push(Check::fail("plan", format!("{e:#}")));
                return None;
            }
        }
    } else {
        // The walk runs on margin budgets (DERIVED_CONTEXT_MARGIN_RATIO); the chosen context is
        // then planned once more on the real ones, so the report and the pinned split carry the
        // measured budgets while the ranks keep room for what memory does between now and load.
        let margin: Vec<NodeFigures> = budgets
            .iter()
            .map(|figures| {
                let reserve = (figures.total as f64 * DERIVED_CONTEXT_MARGIN_RATIO) as u64;
                NodeFigures::new(
                    figures.available.saturating_sub(reserve),
                    figures.total,
                    figures.ceiling,
                )
            })
            .collect();
        let mut derived = match run_fork_planner(config, exec, python, &margin, None).await {
            Ok(plan) => plan,
            Err(e) => {
                report.checks.push(Check::fail("plan", format!("{e:#}")));
                return None;
            }
        };
        let mut tried = std::collections::BTreeSet::from([derived.context]);
        while let Some(ceiling) = derived
            .max_context
            .filter(|c| (*c > derived.context || !derived.fits) && tried.insert(*c))
        {
            derived = match run_fork_planner(config, exec, python, &margin, Some(ceiling)).await {
                Ok(plan) => plan,
                Err(e) => {
                    report.checks.push(Check::fail("plan", format!("{e:#}")));
                    return None;
                }
            };
        }
        if !derived.fits {
            derived
        } else {
            match run_fork_planner(config, exec, python, budgets, Some(derived.context)).await {
                Ok(plan) => plan,
                Err(e) => {
                    report.checks.push(Check::fail("plan", format!("{e:#}")));
                    return None;
                }
            }
        }
    };
    report.context_limit = Some(planned.context);
    report.max_context_fits = planned.max_context;
    if planned.stages.len() != config.size() {
        report.checks.push(Check::fail(
            "plan",
            format!(
                "the planner returned {} stages for {} nodes",
                planned.stages.len(),
                config.size()
            ),
        ));
        return None;
    }
    if planned.slots != config.slots() {
        report.checks.push(Check::fail(
            "plan",
            format!(
                "asked the planner for {} slots, it planned {}",
                config.slots(),
                planned.slots
            ),
        ));
        return None;
    }
    report.slots = Some(planned.slots);
    let lines: Vec<String> = planned
        .stages
        .iter()
        .map(|stage| {
            let rank_plan = stage.rank_plan();
            let line = format!(
                "rank {} ({}) {}",
                stage.rank,
                config.nodes[stage.rank as usize].name,
                pipeline_stage_line(stage, &planned.ratios)
            );
            plans[stage.rank as usize] = Some(rank_plan);
            line
        })
        .collect();
    let head = format!(
        "pipeline split (fork planner {}), context {} ({}), batch {slots} = {slots} full-context \
         slots (each rank's state and workspace are planned for {slots} sequences of the whole \
         context, so the context already accounts for the batch width), largest context this \
         split fits {}: {}",
        &PIPELINE_FORK_COMMIT[..9],
        planned.context,
        report.context_source.as_deref().unwrap_or_default(),
        planned
            .max_context
            .map(|c| c.to_string())
            .unwrap_or_else(|| "none".to_string()),
        lines.join("; "),
        slots = planned.slots,
    );
    if planned.fits {
        report.pipeline_starts = Some(planned.starts.clone());
        report.checks.push(Check::pass("plan", head));
    } else {
        report.checks.push(Check::fail(
            "plan",
            match planned.max_context {
                Some(_) if config.context.is_some() => {
                    format!("{head} — the requested context does not fit this split")
                }
                Some(_) => head,
                None => format!("{head} — no context length fits"),
            },
        ));
    }
    Some(planned.ratios)
}

/// Toggle the node's TB service and re-apply its /30, then re-read the link until it reports the
/// repaired state or the crate's grace window ends. Returns the node's new link checks and the
/// repair's summary line (before → after).
async fn repair_node(
    config: &DistributedConfig,
    exec: &Arc<dyn NodeExec>,
    rank: usize,
) -> (Vec<Check>, String) {
    let node = &config.nodes[rank];
    let host = node.host();
    let run = exec
        .run_op(host, &NodeOp::Repair { node: node.clone() })
        .await;
    let applied = match run {
        Ok(out) if out.success() => "applied".to_string(),
        Ok(out) => format!(
            "networksetup exited {:?}: {}",
            out.status,
            format!("{}{}", out.stdout, out.stderr).trim()
        ),
        Err(e) => format!("{e:#}"),
    };
    let mut last = Vec::new();
    for _ in 0..crate::GRACE_TICKS / 5 {
        tokio::time::sleep(crate::GRACE_TICK * 5).await;
        let link = NodeOp::Link {
            node: node.clone(),
            backend: config.backend,
        };
        let Ok(out) = exec.run_op(host, &link).await else {
            continue;
        };
        let sections = probe::sections(&out.stdout);
        let (checks, needs_repair) = link_checks(node, config.backend, &sections);
        last = checks;
        if !needs_repair {
            break;
        }
    }
    let verdict = if last.iter().all(|c| c.verdict == CheckVerdict::Pass) && !last.is_empty() {
        "link healthy after repair"
    } else {
        "link still failing after repair"
    };
    let summary = format!(
        "{}: toggled '{}' off/on and re-applied {}/{} ({applied}); {verdict}",
        node.name, node.tb_service, node.tb_ip, node.tb_netmask
    );
    (last, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;

    #[test]
    fn the_repair_is_licensed_only_on_the_configured_thunderbolt_service() {
        let node = two_mac_config().nodes.remove(1);
        let services = "(1) Ethernet\n(Hardware Port: Ethernet, Device: en0)\n\n(3) EXO Thunderbolt 2\n(Hardware Port: Thunderbolt 2, Device: en3)\n";
        repair_licence(&node, services).unwrap();

        let mut wifi = node.clone();
        wifi.tb_service = "Ethernet".to_string();
        let err = repair_licence(&wifi, services).unwrap_err();
        assert!(err.contains("not a Thunderbolt port"), "{err}");

        let mut wrong_device = node.clone();
        wrong_device.tb_interface = "en5".to_string();
        assert!(repair_licence(&wrong_device, services).is_err());

        let mut absent = node.clone();
        absent.tb_service = "EXO Thunderbolt 9".to_string();
        assert!(repair_licence(&absent, services).is_err());
    }

    #[test]
    fn the_repair_toggles_only_the_named_service_and_reapplies_the_manual_ip() {
        let node = two_mac_config().nodes.remove(1);
        assert_eq!(
            repair_script(&node),
            "/usr/sbin/networksetup -setnetworkserviceenabled 'EXO Thunderbolt 2' off && \
             /usr/sbin/networksetup -setnetworkserviceenabled 'EXO Thunderbolt 2' on && \
             /usr/sbin/networksetup -setmanual 'EXO Thunderbolt 2' '192.168.0.2' '255.255.255.252'"
        );
    }

    #[test]
    fn a_gid_at_index_two_fails_with_the_documented_repair() {
        let node = two_mac_config().nodes.remove(1);
        let mut sections = BTreeMap::new();
        sections.insert(
            "ifconfig".to_string(),
            "\tinet 192.168.0.2 netmask 0xfffffffc broadcast 192.168.0.3\n".to_string(),
        );
        sections.insert(
            "gid".to_string(),
            "GID[  0]:\t\tfe80::1\nGID[  2]:\t\t::ffff:192.168.0.2\n".to_string(),
        );
        let (checks, needs_repair) = link_checks(&node, Backend::Jaccl, &sections);
        assert!(needs_repair);
        let gid = checks.iter().find(|c| c.id == "rdmaGid").unwrap();
        assert_eq!(gid.verdict, CheckVerdict::Fail);
        assert!(gid.message.contains("GID[2]") && gid.message.contains("errno 96"));
        // Ring does not read the GID table at all.
        let (checks, needs_repair) = link_checks(&node, Backend::Ring, &sections);
        assert!(!needs_repair && checks.iter().all(|c| c.id != "rdmaGid"));
    }

    fn pipeline_node(python: &str) -> NodeConfig {
        let mut node = two_mac_config().nodes.remove(0);
        node.pipeline_python = Some(python.to_string());
        node
    }

    #[test]
    fn the_pipeline_runner_passes_on_the_pinned_serve_and_fails_by_name_without_it() {
        let managed = EnvSpec::pipeline().python("/Users/me");
        let node = pipeline_node(&managed);
        // The fork's real usage at 272cb0643 (argparse wraps the subcommands to line 2).
        let help = "usage: python -m rapid_mlx.distributed.pipeline_qwen4 [-h]\n                                                      {plan,serve,run} ...\n".to_string();
        let pinned = EnvSpec::pipeline().expect;
        let check = pipeline_runner_check(&node, Some(&help), Some(&pinned));
        assert_eq!(check.verdict, CheckVerdict::Pass, "{}", check.message);
        assert!(check.message.contains("[\"plan\", \"serve\", \"run\"]"));

        // The fork before `serve` existed (lz/pipeline-qwen4 7a9b622a5): refused by name.
        let old = "usage: python -m rapid_mlx.distributed.pipeline_qwen4 [-h] {plan,run} ...\n"
            .to_string();
        let check = pipeline_runner_check(&node, Some(&old), Some(&pinned));
        assert_eq!(check.verdict, CheckVerdict::Fail);
        assert!(
            check.message.contains("[\"plan\", \"run\"], not `serve`")
                && check.message.contains(PIPELINE_FORK_COMMIT),
            "{}",
            check.message
        );

        // `serve` offered, but a goose-managed env on another commit is stale: FAIL; an
        // operator's own interpreter on another commit is used as configured: WARN.
        let stale = "0.32.2 0.31.3 e7d49b355fe2692d54b04332c19fc541e5e120fd".to_string();
        let check = pipeline_runner_check(&node, Some(&help), Some(&stale));
        assert_eq!(check.verdict, CheckVerdict::Fail);
        assert!(check.message.contains("stale"), "{}", check.message);
        let own = pipeline_node("/Users/me/Projects/Rapid-MLX/.venv/bin/python");
        let check = pipeline_runner_check(&own, Some(&help), Some(&stale));
        assert_eq!(check.verdict, CheckVerdict::Warn, "{}", check.message);
    }

    /// Answers the fork planner by the `--context` it was given; records every script.
    struct ScriptedPlanner {
        seen: std::sync::Mutex<Vec<String>>,
        answer: fn(Option<u64>) -> ExecOutput,
    }

    impl NodeExec for ScriptedPlanner {
        fn run<'a>(
            &'a self,
            host: Option<&'a str>,
            script: &'a str,
        ) -> crate::distributed::exec::BoxFuture<'a, Result<ExecOutput>> {
            assert!(host.is_none(), "the planner runs on this Mac");
            self.seen.lock().unwrap().push(script.to_string());
            let context = script
                .split(" --context ")
                .nth(1)
                .map(|c| c.trim().parse().unwrap());
            let out = (self.answer)(context);
            Box::pin(async move { Ok(out) })
        }
    }

    /// The real 32k answer with its context, ceiling and verdict replaced.
    fn flash_answer(context: u64, max_context: u64, fits: bool) -> ExecOutput {
        let json = plan::tests::FLASH_PLAN_32K
            .replacen("\"context\": 32768", &format!("\"context\": {context}"), 1)
            .replace(
                "\"max_context\": 262144",
                &format!("\"max_context\": {max_context}"),
            )
            .replace("\"fits\": true", &format!("\"fits\": {fits}"));
        ExecOutput {
            status: Some(if fits { 0 } else { 2 }),
            stdout: json + "\n",
            stderr: String::new(),
        }
    }

    fn pipeline_config() -> DistributedConfig {
        let mut config = two_mac_config();
        for node in &mut config.nodes {
            node.pipeline_python = Some("/fork/bin/python".to_string());
        }
        config
    }

    fn budgets() -> Vec<NodeFigures> {
        vec![
            NodeFigures::new(90 * GIB, 128 * GIB, plan::tests::M4_MAX_CEILING),
            NodeFigures::new(67 * GIB, 96 * GIB, plan::tests::M3_ULTRA_CEILING),
        ]
    }

    #[tokio::test]
    async fn a_derived_pipeline_context_walks_the_forks_ceiling_to_its_fixed_point() {
        // The planner's measured shape with no context (2026-09-24, 128:90 / 96:67 at batch 2):
        // full context 262,144 does not fit and ITS split fits 16,308; re-balanced at 16,308 the
        // split fits 73,216; at 73,216 the ceiling is the context itself.
        let exec: Arc<dyn NodeExec> = Arc::new(ScriptedPlanner {
            seen: Default::default(),
            answer: |context| match context {
                None => flash_answer(262_144, 16_308, false),
                Some(16_308) => flash_answer(16_308, 73_216, true),
                Some(73_216) => flash_answer(73_216, 73_216, true),
                other => panic!("unexpected context {other:?}"),
            },
        });
        let config = pipeline_config();
        let mut report = empty_report(&config);
        let mut plans = vec![None; 2];
        let ratios = plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        assert!(ratios.is_some());
        assert_eq!(report.context_limit, Some(73_216));
        assert_eq!(report.context_source.as_deref(), Some("derived"));
        assert_eq!(report.max_context_fits, Some(73_216));
        assert_eq!(report.pipeline_starts, Some(vec![0, 19]));
        let check = &report.checks[0];
        assert_eq!(check.verdict, CheckVerdict::Pass, "{}", check.message);
        assert!(
            check.message.contains("rank 1 (workhorse) layers [19, 48)")
                && check.message.contains("→ fits"),
            "{}",
            check.message
        );
        assert_eq!(plans[1].as_ref().unwrap().budget_bytes, 62_354_335_204);
        assert!(
            check.message.contains("GPU ceiling 77.76 GiB"),
            "{}",
            check.message
        );
    }

    #[tokio::test]
    async fn a_derived_context_is_walked_on_margin_budgets_and_reported_on_the_real_ones() {
        let planner = Arc::new(ScriptedPlanner {
            seen: Default::default(),
            answer: |context| match context {
                None => flash_answer(262_144, 16_308, false),
                Some(16_308) => flash_answer(16_308, 73_216, true),
                Some(73_216) => flash_answer(73_216, 73_216, true),
                other => panic!("unexpected context {other:?}"),
            },
        });
        let exec: Arc<dyn NodeExec> = planner.clone();
        let config = pipeline_config();
        let mut report = empty_report(&config);
        let mut plans = vec![None; 2];
        plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        let seen = planner.seen.lock().unwrap();
        // 2% of 128 GiB = 2.56 GiB and of 96 GiB = 1.92 GiB off each node's available figure;
        // the GPU ceilings go through unchanged.
        let margin = "--node 'MacBook-Pro:128.0000:87.4400:107.5200' --node 'workhorse:96.0000:65.0800:77.7600'";
        let real = "--node 'MacBook-Pro:128.0000:90.0000:107.5200' --node 'workhorse:96.0000:67.0000:77.7600'";
        let (last, walk) = seen.split_last().unwrap();
        assert_eq!(walk.len(), 3, "{seen:?}");
        assert!(
            walk.iter().all(|script| script.contains(margin)),
            "{seen:?}"
        );
        assert!(
            last.contains(real) && last.ends_with("--context 73216"),
            "{last}"
        );
        assert_eq!(report.context_limit, Some(73_216));
    }

    #[tokio::test]
    async fn the_planner_is_asked_with_goose_figures_at_the_launch_batch() {
        let planner = Arc::new(ScriptedPlanner {
            seen: Default::default(),
            answer: |_| flash_answer(32_768, 73_216, true),
        });
        let exec: Arc<dyn NodeExec> = planner.clone();
        let mut config = pipeline_config();
        config.context = Some(32_768);
        let mut report = empty_report(&config);
        let mut plans = vec![None; 2];
        plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        let seen = planner.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "a requested context is planned once");
        assert_eq!(
            seen[0],
            "'/fork/bin/python' -m rapid_mlx.distributed.pipeline_qwen4 plan --json --model \
             '/Users/me/.goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx' \
             --node 'MacBook-Pro:128.0000:90.0000:107.5200' --node \
             'workhorse:96.0000:67.0000:77.7600' --batch 2 --context 32768"
        );
        assert_eq!(report.context_source.as_deref(), Some("requested"));
        assert_eq!(report.pipeline_starts, Some(vec![0, 19]));
    }

    #[tokio::test]
    async fn the_planner_is_asked_for_the_configured_slots_and_must_plan_them() {
        let planner = Arc::new(ScriptedPlanner {
            seen: Default::default(),
            answer: |_| flash_answer(32_768, 73_216, true),
        });
        let exec: Arc<dyn NodeExec> = planner.clone();
        let mut config = pipeline_config();
        config.context = Some(32_768);
        let mut report = empty_report(&config);
        let mut plans = vec![None; 2];
        plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        assert_eq!(report.slots, Some(2));
        assert!(
            report.checks[0]
                .message
                .contains("batch 2 = 2 full-context slots"),
            "{}",
            report.checks[0].message
        );

        // 4 slots asked, the (scripted) planner answered for 2: refused by name, no split.
        config.slots = Some(4);
        let mut report = empty_report(&config);
        plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        assert!(planner
            .seen
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains(" --batch 4 "));
        assert_eq!(report.checks[0].verdict, CheckVerdict::Fail);
        assert!(
            report.checks[0]
                .message
                .contains("asked the planner for 4 slots, it planned 2"),
            "{}",
            report.checks[0].message
        );
        assert_eq!(report.pipeline_starts, None);
    }

    #[tokio::test]
    async fn a_plan_that_does_not_fit_or_a_crashed_planner_is_a_named_failure() {
        let exec: Arc<dyn NodeExec> = Arc::new(ScriptedPlanner {
            seen: Default::default(),
            answer: |_| {
                let mut out = flash_answer(32_768, 8_192, true);
                out.stdout = out.stdout.replacen(
                    "\"budget_bytes\": 62354335204, \"available_bytes\": 71940702208, \"ceiling_bytes\": 83494164234, \"ram_bytes\": 103079215104, \"budget_source\": \"free + GPU ceiling given\", \"fits\": true",
                    "\"budget_bytes\": 40000000000, \"available_bytes\": 71940702208, \"ceiling_bytes\": 83494164234, \"ram_bytes\": 103079215104, \"budget_source\": \"free + GPU ceiling given\", \"fits\": false",
                    1,
                ).replacen("\"fits\": true, \"vision\"", "\"fits\": false, \"vision\"", 1);
                out.status = Some(2);
                out
            },
        });
        let mut config = pipeline_config();
        config.context = Some(32_768);
        let mut report = empty_report(&config);
        let mut plans = vec![None; 2];
        plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        assert_eq!(report.checks[0].verdict, CheckVerdict::Fail);
        assert!(
            report.checks[0].message.contains("DOES NOT FIT")
                && report.checks[0]
                    .message
                    .contains("requested context does not fit"),
            "{}",
            report.checks[0].message
        );
        assert!(!plans[1].as_ref().unwrap().fits && plans[0].as_ref().unwrap().fits);
        assert_eq!(
            report.pipeline_starts, None,
            "a split that does not fit is never approved"
        );

        // Measured 2026-09-24 (before 9f861d9e1): a node below the fork's budget floor crashed the
        // planner (budget 0 → ZeroDivisionError, exit 1, no JSON); any crash stays a named FAIL.
        let exec: Arc<dyn NodeExec> = Arc::new(ScriptedPlanner {
            seen: Default::default(),
            answer: |_| {
                ExecOutput {
                status: Some(1),
                stdout: String::new(),
                stderr: "Traceback (most recent call last):\n    return self.total_bytes / self.node.budget_bytes\nZeroDivisionError: division by zero\n".to_string(),
            }
            },
        });
        let mut report = empty_report(&config);
        let ratios = plan_pipeline(&config, &exec, &budgets(), &mut report, &mut plans).await;
        assert!(ratios.is_none());
        let message = &report.checks[0].message;
        assert!(
            message.contains("exited Some(1)")
                && message.contains("ZeroDivisionError")
                && message.contains("workhorse:96.0000:67.0000:77.7600"),
            "{message}"
        );
    }

    fn empty_report(config: &DistributedConfig) -> PreflightReport {
        PreflightReport {
            ok: false,
            ran_at_ms: 0,
            backend: config.backend,
            runner: Some(Runner::PipelineQwen4),
            model_type: Some("qwen4_exp".to_string()),
            context_limit: None,
            context_source: None,
            max_context_fits: None,
            pipeline_starts: None,
            slots: None,
            checks: Vec::new(),
            nodes: Vec::new(),
            repairs: Vec::new(),
        }
    }

    #[test]
    fn the_probe_asks_the_ports_each_rank_binds() {
        let config = two_mac_config();
        let rank0 = node_probe_script(&config, Some(Runner::MlxLmTensor), 0);
        assert!(rank0.contains("@@listen8190") && rank0.contains("@@listen32323"));
        assert!(rank0.contains("ibv_devinfo -v -d 'rdma_en3'"));
        let rank1 = node_probe_script(&config, Some(Runner::MlxLmTensor), 1);
        assert!(!rank1.contains("@@listen"));
        assert!(rank1.contains("'192.168.0.1'"), "rank 1 pings rank 0");
    }
}
