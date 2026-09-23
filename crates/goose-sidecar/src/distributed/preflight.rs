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
use super::plan::{self, RankPlan};
use super::probe::{self, Pressure};
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
    /// rdmaGid | portRange | ports | runner | plan
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
    if runner == Some(Runner::PipelineQwen4) {
        if let Some(python) = &node.pipeline_python {
            add(format!(
                "echo; echo @@pipeline; {} -m {PIPELINE_MODULE} --help 2>&1 | /usr/bin/head -3",
                sh_quote(python)
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
    add(format!(
        "echo; echo @@ping; for ip in {}; do if /sbin/ping -c 2 -t 3 \"$ip\" >/dev/null 2>&1; then echo \"$ip ok\"; else echo \"$ip fail\"; fi; done",
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

fn link_script(node: &NodeConfig, backend: Backend) -> String {
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
    model_files: Option<BTreeMap<String, u64>>,
    sums: BTreeMap<String, String>,
    mlx_version: Option<String>,
    link_speed: Option<String>,
    needs_repair: bool,
    services_text: Option<String>,
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
        model_files: None,
        sums: BTreeMap::new(),
        mlx_version: None,
        link_speed: None,
        needs_repair: false,
        services_text: None,
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
            .map(|h| format!("ssh {h} answered"))
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
            let failed: Vec<&str> = text
                .lines()
                .filter(|l| l.ends_with(" fail"))
                .map(|l| l.trim_end_matches(" fail"))
                .collect();
            if failed.is_empty() {
                answer.checks.push(Check::pass(
                    "ping",
                    format!("peers answer: {}", text.trim().replace('\n', ", ")),
                ));
            } else {
                answer.checks.push(Check::fail(
                    "ping",
                    format!("no ping answer over the TB link from {failed:?}"),
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
        answer
            .checks
            .push(pipeline_runner_check(node, sections.get("pipeline")));
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

/// The qwen4_exp split serves only if the fork's module offers a `serve` subcommand. Branch
/// lz/pipeline-qwen4 (7a9b622a5, 2026-09-24) offers `{plan,run}` — a planner and a one-shot
/// generator, no OpenAI server — so this check fails loudly with what the module does offer.
fn pipeline_runner_check(node: &NodeConfig, help: Option<&String>) -> Check {
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
    match offered {
        Some(commands) if commands.iter().any(|c| c == "serve") => {
            Check::pass("runner", format!("{PIPELINE_MODULE} offers {commands:?}"))
        }
        Some(commands) => Check::fail(
            "runner",
            format!(
                "{PIPELINE_MODULE} on {python} offers {commands:?} — no `serve` entry, so the \
                 qwen4_exp split cannot serve an OpenAI API (the plan below is a dry run only)"
            ),
        ),
        None => Check::fail(
            "runner",
            format!("{python} -m {PIPELINE_MODULE} --help: {}", help.trim()),
        ),
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
        let script = node_probe_script(config, runner, rank);
        let host = config.nodes[rank].ssh.clone();
        async move { exec.run(host.as_deref(), &script).await }
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
    let budgets: Vec<Option<(u64, u64, u64)>> = answers
        .iter()
        .map(|a| {
            a.memory.map(|(m, _)| {
                (
                    m.available_bytes,
                    m.total_bytes,
                    plan::budget_bytes(m.available_bytes, m.total_bytes),
                )
            })
        })
        .collect();
    let mut plans: Vec<Option<RankPlan>> = vec![None; config.size()];
    if let (Some(runner), true) = (runner, budgets.iter().all(Option::is_some)) {
        let budgets: Vec<(u64, u64, u64)> = budgets.iter().map(|b| b.unwrap()).collect();
        match runner {
            Runner::MlxLmTensor => {
                plan_tensor(config, &budgets, &mut report, &mut plans);
            }
            Runner::PipelineQwen4 => {
                plan_pipeline(config, &exec, &budgets, &mut report, &mut plans).await;
            }
        }
    } else if runner.is_some() {
        report.checks.push(Check::fail(
            "plan",
            "no plan: a node's memory could not be measured",
        ));
    }

    for (rank, mut answer) in answers.into_iter().enumerate() {
        let node = &config.nodes[rank];
        let plan = plans[rank].clone();
        if let (Some((reading, pressure)), Some(plan)) = (answer.memory, &plan) {
            let head = format!(
                "available {} of {} (pressure {}); budget {} = min(available × {:.2}, RAM × {:.2}); \
                 planned {} (weights {} + state {} + workspace {} + prompt cache {}) × {:.2} = {}",
                gib(reading.available_bytes),
                gib(reading.total_bytes),
                pressure.as_str(),
                gib(plan.budget_bytes),
                super::AVAILABLE_HEADROOM_RATIO,
                super::MEMORY_LIMIT_RATIO,
                gib(plan.planned_bytes),
                gib(plan.weights_bytes),
                gib(plan.state_bytes),
                gib(plan.workspace_bytes),
                gib(plan.prompt_cache_bytes),
                super::RUNTIME_OVERHEAD_RATIO,
                gib(plan.with_overhead_bytes),
            );
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
                        gib(plan.with_overhead_bytes - plan.budget_bytes)
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
            plan,
            link_speed: answer.link_speed,
            mlx_version: answer.mlx_version,
        });
    }
    report.ok = report.failures().is_empty() && report.nodes.iter().all(|n| n.plan.is_some());
    Ok(report)
}

fn plan_tensor(
    config: &DistributedConfig,
    budgets: &[(u64, u64, u64)],
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
        .map(|(_, _, budget)| facts.max_context(ranks, *budget))
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
    for (rank, (_, _, budget)) in budgets.iter().enumerate() {
        plans[rank] = Some(facts.rank_plan(ranks, rank as u64, context.max(1), *budget));
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

async fn plan_pipeline(
    config: &DistributedConfig,
    exec: &Arc<dyn NodeExec>,
    budgets: &[(u64, u64, u64)],
    report: &mut PreflightReport,
    plans: &mut [Option<RankPlan>],
) {
    let rank0 = &config.nodes[0];
    let Some(python) = &rank0.pipeline_python else {
        return;
    };
    let run_plan = |context: Option<u64>| {
        let mut script = format!(
            "{} -m {PIPELINE_MODULE} plan --model {}",
            sh_quote(python),
            sh_quote(&rank0.model_dir)
        );
        for (node, (available, total, _)) in config.nodes.iter().zip(budgets) {
            script.push_str(&format!(
                " --node {}",
                sh_quote(&plan::planner_node_arg(&node.name, *total, *available))
            ));
        }
        if let Some(context) = context {
            script.push_str(&format!(" --context {context}"));
        }
        script.push_str(" 2>&1");
        let exec = Arc::clone(exec);
        async move {
            let out = exec.run(None, &script).await?;
            plan::parse_pipeline_plan(&out.stdout)
                .with_context(|| format!("the fork planner answered (exit {:?})", out.status))
        }
    };
    let first = match run_plan(config.context).await {
        Ok(plan) => plan,
        Err(e) => {
            report.checks.push(Check::fail("plan", format!("{e:#}")));
            return;
        }
    };
    report.max_context_fits = first.max_context;
    let (planned, context) = match (config.context, first.max_context) {
        (Some(requested), _) => {
            report.context_source = Some("requested".to_string());
            (first, Some(requested))
        }
        (None, Some(ceiling)) => {
            report.context_source = Some("derived".to_string());
            match run_plan(Some(ceiling)).await {
                Ok(plan) => (plan, Some(ceiling)),
                Err(e) => {
                    report.checks.push(Check::fail("plan", format!("{e:#}")));
                    return;
                }
            }
        }
        (None, None) => {
            report.context_source = Some("derived".to_string());
            report.checks.push(Check::fail(
                "plan",
                "the fork planner found no context length that fits this split",
            ));
            (first, None)
        }
    };
    report.context_limit = context;
    if planned.stages.len() != config.size() {
        report.checks.push(Check::fail(
            "plan",
            format!(
                "the planner returned {} stages for {} nodes",
                planned.stages.len(),
                config.size()
            ),
        ));
        return;
    }
    for (rank, stage) in planned.stages.iter().enumerate() {
        plans[rank] = Some(stage.rank_plan(budgets[rank].2));
    }
    report.checks.push(Check::pass(
        "plan",
        format!(
            "pipeline split (fork planner): {}",
            planned
                .stages
                .iter()
                .map(|s| format!(
                    "rank {} layers [{}, {})",
                    s.rank, s.layer_start, s.layer_end
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ));
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
    let run = exec.run(host, &repair_script(node)).await;
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
        let Ok(out) = exec.run(host, &link_script(node, config.backend)).await else {
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

    #[test]
    fn the_pipeline_runner_without_a_serve_entry_is_refused_by_name() {
        let mut node = two_mac_config().nodes.remove(0);
        node.pipeline_python = Some("/p/.venv/bin/python".to_string());
        // The fork's real usage line on lz/pipeline-qwen4 7a9b622a5.
        let help = "usage: python -m rapid_mlx.distributed.pipeline_qwen4 [-h] {plan,run} ...\n"
            .to_string();
        let check = pipeline_runner_check(&node, Some(&help));
        assert_eq!(check.verdict, CheckVerdict::Fail);
        assert!(
            check.message.contains("[\"plan\", \"run\"]") && check.message.contains("no `serve`")
        );
        let with_serve = help.replace("{plan,run}", "{plan,run,serve}");
        assert_eq!(
            pipeline_runner_check(&node, Some(&with_serve)).verdict,
            CheckVerdict::Pass
        );
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
