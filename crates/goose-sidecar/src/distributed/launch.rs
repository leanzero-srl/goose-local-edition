//! The rank processes. goose launches the ranks itself instead of through `mlx.launch`, measured
//! on STEP1b for three reasons: `mlx.launch` exits 0 when a rank dies (the rank's own status is
//! the only truth), SIGTERM to it orphans both ranks (no handler), and its pump threads spin
//! more than one core for the launcher's whole life (`launch.py:192-201`, stdin always writable). What it
//! did is small and reproduced exactly: per rank, `MLX_RANK` plus the backend's env
//! (`MLX_IBV_DEVICES` + `MLX_JACCL_COORDINATOR`, or `MLX_HOSTFILE`), then exec the program — on a
//! peer through `ssh -tt`, whose pty makes the remote rank die with its session (measured
//! 2026-09-24: killing the local ssh client took the remote pid within 1 s).
//!
//! The program is embedded here and passed base64 on the command line, so a node needs nothing
//! installed beyond its interpreter: `rank_env.py` (the backend env, `emit`, the memory reporter —
//! every rank), `rank_live.py` (rank 0's live request table, Rapid-MLX's `/v1/status` shape),
//! then the runner's program — `rank_budget.py` + `rank_wrapper.py` (`mlx_lm.server`, tensor
//! split, under `NodeConfig::python`; the budget is what an absent max_tokens generates) or `pipeline_rank.py` (the fork's `pipeline_qwen4_serve`,
//! layer split, under `NodeConfig::pipeline_python`).

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use super::config::{Backend, DistributedConfig, NodeConfig};
use super::exec::{sh_quote, SSH_OPTIONS};
use super::plan::RankPlan;

/// The literal every goose rank carries on its command line, so `ps` can name a rank a previous
/// goosed left behind (and `stop` can reclaim it per-pid).
pub const RANK_MARKER: &str = "goose-distributed-rank";

/// The argument after the marker that names WHICH goose launched the rank: this install's owner
/// token (`RankSpec::owner`). The marker alone says "a goose rank" — a cargo test's stand-in rank
/// carries it too (Q-77: pid 9425 was `/usr/bin/python3` running the boot line, not the split's
/// py3.12 rank) — so only this token proves a leftover is this install's own, one preflight may
/// wait for or reclaim; any other rank stays a refusal.
pub const OWNER_ARG_PREFIX: &str = "goose-distributed-owner=";

const TENSOR_PROGRAM: &str = concat!(
    include_str!("rank_env.py"),
    include_str!("rank_live.py"),
    include_str!("rank_budget.py"),
    include_str!("rank_wrapper.py")
);
const PIPELINE_PROGRAM: &str = concat!(
    include_str!("rank_env.py"),
    include_str!("rank_live.py"),
    include_str!("pipeline_rank.py")
);
const BOOT: &str = "import base64,sys;exec(base64.b64decode(sys.argv[1]))";
const TAIL_LINES: usize = 200;

/// What a rank runs, with the fields only that program reads (flattened into the spec JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "program", rename_all = "camelCase")]
pub enum RankProgram {
    /// `mlx_lm.server` under `rank_wrapper.py` (tensor split). Its in-process memory and wired
    /// limits sit at the node's own GPU ceiling (`max_recommended_working_set_size`, read on the
    /// rank), the cache limit at the ceiling less the planned bytes. The prompt cache's two bounds
    /// are the same on every rank (see [`TensorLaunch`]).
    ///
    /// Tagged `mlxLmServerDoorbell` since the doorbell (Q-66): an idle worker parks in recv(1)
    /// instead of spinning in JACCL's all_sum, which needs every rank's wrapper to take part.
    /// A Link peer runs its OWN goosed's wrapper, and an older one would never join the
    /// doorbell's port all_sum (a hang). The new tag makes that peer's goosed refuse the spec at
    /// rank start instead (serde: unknown variant; see `older_peer_refusal`). `mlxLmServer`
    /// (an older requester's spec) still reads, with `doorbell` false: upstream waiting.
    #[serde(rename = "mlxLmServerDoorbell", alias = "mlxLmServer")]
    MlxLmServer {
        context_window: u64,
        /// `--prompt-cache-bytes` and the prompt cache's own `max_bytes`
        /// (`RankPlan::prompt_cache_limit_bytes`). Absent only in an older requester's spec.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_cache_limit_bytes: Option<u64>,
        /// `--prompt-cache-size` (`RankPlan::prompt_cache_entries`), beside the limit.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_cache_entries: Option<u64>,
        /// An older requester's spec: the one number its own rank 0's wrapper hands mlx_lm as the
        /// flag alone (no count, no max_bytes). A rank reading it runs that same policy, so both
        /// ranks evict alike (each runs its own LRU cache over the same requests); goose itself
        /// never writes it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_cache_bytes: Option<u64>,
        planned_bytes: u64,
        #[serde(default)]
        doorbell: bool,
    },
    /// The fork's `pipeline_qwen4_serve.serve` under `pipeline_rank.py` (layer split): the exact
    /// `pipeline_qwen4 serve` arguments, parsed on the rank by the fork's own parser.
    PipelineServe { serve_args: Vec<String> },
}

/// Everything one rank's program reads (base64 JSON, argv[2]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankSpec {
    pub rank: usize,
    pub size: usize,
    pub backend: Backend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ibv_devices: Option<Vec<Vec<Option<String>>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ring_hosts: Option<Vec<Vec<String>>>,
    pub served_id: String,
    pub model_dir: String,
    pub port: u16,
    pub memory_report_seconds: f64,
    /// The weights preflight planned on this rank (`RankPlan::weights_bytes`): with the rank's own
    /// MLX active bytes it is the load's progress — on this Mac and on a Link peer hosting it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_weight_bytes: Option<u64>,
    /// The launching install's owner token, written on the rank's command line after the marker
    /// (`OWNER_ARG_PREFIX`). A Link peer launches the requester's spec as given, so a hosted rank
    /// carries the REQUESTER's token. `None` = a goose that set none (an older goose, a test)
    /// launched it: its leftovers are never provably this install's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(flatten)]
    pub program: RankProgram,
}

/// What preflight's plan hands one tensor rank's launch.
///
/// The prompt cache bounds must be identical on every rank: each tensor rank runs its own mlx_lm
/// LRU prompt cache over the same requests, and a rank that evicts differently reuses a different
/// prefix — its prefill then runs a different number of steps than its peers' and the collectives
/// no longer pair up. [`TensorLaunch::for_ranks`] refuses plans whose bounds differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorLaunch {
    pub planned_bytes: u64,
    pub prompt_cache_limit_bytes: u64,
    pub prompt_cache_entries: u64,
}

impl TensorLaunch {
    pub fn from_plan(plan: &RankPlan) -> Self {
        TensorLaunch {
            planned_bytes: plan.planned_bytes,
            prompt_cache_limit_bytes: plan.prompt_cache_limit_bytes(),
            prompt_cache_entries: plan.prompt_cache_entries,
        }
    }

    pub fn for_ranks(plans: &[&RankPlan]) -> Result<Vec<Self>> {
        let launches: Vec<Self> = plans.iter().map(|plan| Self::from_plan(plan)).collect();
        let bounds = |launch: &Self| (launch.prompt_cache_limit_bytes, launch.prompt_cache_entries);
        if let Some(first) = launches.first() {
            if let Some((rank, other)) = launches
                .iter()
                .enumerate()
                .find(|(_, launch)| bounds(launch) != bounds(first))
            {
                bail!(
                    "the ranks' prompt cache bounds differ (rank 0: {} bytes / {} entries, rank \
                     {rank}: {} bytes / {} entries): every tensor rank must evict identically",
                    first.prompt_cache_limit_bytes,
                    first.prompt_cache_entries,
                    other.prompt_cache_limit_bytes,
                    other.prompt_cache_entries
                );
            }
        }
        Ok(launches)
    }
}

/// The per-rank tensor specs. See [`base_specs`] for the backend env.
pub fn rank_specs(
    config: &DistributedConfig,
    served_id: &str,
    per_rank: &[TensorLaunch],
    context_window: u64,
    memory_report_seconds: f64,
) -> Vec<RankSpec> {
    base_specs(config, served_id, memory_report_seconds, |rank, _| {
        let launch = per_rank[rank];
        RankProgram::MlxLmServer {
            context_window,
            prompt_cache_limit_bytes: Some(launch.prompt_cache_limit_bytes),
            prompt_cache_entries: Some(launch.prompt_cache_entries),
            prompt_cache_bytes: None,
            planned_bytes: launch.planned_bytes,
            doorbell: true,
        }
    })
}

/// The `pipeline_qwen4 serve` arguments for one rank: this node's own model dir, the served id,
/// rank 0's loopback port, the context preflight allowed, the slots the plan was made for
/// (`--slots`: the fork re-plans at load for that many full-context sequences and admits requests
/// by that KV budget; `--max-batch` = the same count, the rows proven per batch), and the split
/// preflight approved (`--split` = ranks 1..N-1's starts), so the fork loads exactly that split
/// instead of re-balancing on its own load-time figures.
pub fn pipeline_serve_args(
    config: &DistributedConfig,
    node: &NodeConfig,
    served_id: &str,
    context: u64,
    split: &str,
) -> Vec<String> {
    [
        "--model",
        &node.model_dir,
        "--served-model-name",
        served_id,
        "--host",
        "127.0.0.1",
        "--port",
        &config.port.to_string(),
        "--context",
        &context.to_string(),
        "--slots",
        &config.slots().to_string(),
        "--max-batch",
        &config.slots().to_string(),
        "--split",
        split,
    ]
    .iter()
    .map(|a| a.to_string())
    .collect()
}

/// The per-rank pipeline specs for the plan preflight approved.
pub fn pipeline_rank_specs(
    config: &DistributedConfig,
    served_id: &str,
    context: u64,
    split: &str,
    memory_report_seconds: f64,
) -> Vec<RankSpec> {
    base_specs(config, served_id, memory_report_seconds, |_, node| {
        RankProgram::PipelineServe {
            serve_args: pipeline_serve_args(config, node, served_id, context, split),
        }
    })
}

/// The per-rank backend env, exactly as `mlx.launch` would have described the hostfile: JACCL's
/// device matrix has `null` on the diagonal and host i's own RDMA device elsewhere; ring's host
/// list gives rank r the port `coordinator_port + r` on its TB IP.
fn base_specs(
    config: &DistributedConfig,
    served_id: &str,
    memory_report_seconds: f64,
    program: impl Fn(usize, &NodeConfig) -> RankProgram,
) -> Vec<RankSpec> {
    let size = config.size();
    let ibv_devices: Vec<Vec<Option<String>>> = config
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            (0..size)
                .map(|j| (i != j).then(|| node.rdma_device.clone()))
                .collect()
        })
        .collect();
    let ring_hosts: Vec<Vec<String>> = config
        .nodes
        .iter()
        .enumerate()
        .map(|(rank, node)| {
            vec![format!(
                "{}:{}",
                node.tb_ip,
                config.coordinator_port as usize + rank
            )]
        })
        .collect();
    config
        .nodes
        .iter()
        .enumerate()
        .map(|(rank, node)| RankSpec {
            rank,
            size,
            backend: config.backend,
            ibv_devices: (config.backend == Backend::Jaccl).then(|| ibv_devices.clone()),
            coordinator: (config.backend == Backend::Jaccl)
                .then(|| format!("{}:{}", config.nodes[0].tb_ip, config.coordinator_port)),
            ring_hosts: (config.backend == Backend::Ring).then(|| ring_hosts.clone()),
            served_id: served_id.to_string(),
            model_dir: node.model_dir.clone(),
            port: config.port,
            memory_report_seconds,
            planned_weight_bytes: None,
            owner: None,
            program: program(rank, node),
        })
        .collect()
}

impl RankSpec {
    fn program_source(&self) -> &'static str {
        match self.program {
            RankProgram::MlxLmServer { .. } => TENSOR_PROGRAM,
            RankProgram::PipelineServe { .. } => PIPELINE_PROGRAM,
        }
    }

    /// The interpreter this rank's program needs on `node`: the tensor env, or the fork's.
    pub fn interpreter<'a>(&self, node: &'a NodeConfig) -> Result<&'a str> {
        match self.program {
            RankProgram::MlxLmServer { .. } => Ok(&node.python),
            RankProgram::PipelineServe { .. } => {
                node.pipeline_python.as_deref().with_context(|| {
                    format!(
                    "node '{}' has no pipeline_python: the qwen4_exp pipeline rank runs the fork's \
                     interpreter",
                    node.name
                )
                })
            }
        }
    }
}

/// The interpreter's arguments: the boot one-liner, the program, the spec, the marker, and the
/// owner token when the spec carries one (the rank programs read argv[1] and argv[2] only).
pub fn python_args(spec: &RankSpec) -> Result<Vec<String>> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut args = vec![
        "-c".to_string(),
        BOOT.to_string(),
        b64.encode(spec.program_source()),
        b64.encode(serde_json::to_vec(spec)?),
        RANK_MARKER.to_string(),
    ];
    if let Some(owner) = &spec.owner {
        args.push(format!("{OWNER_ARG_PREFIX}{owner}"));
    }
    Ok(args)
}

/// The peer-side command: print the shell's pid (which `exec` hands to the interpreter unchanged)
/// so the supervisor can signal and verify THAT pid over ssh, then become the rank.
pub fn remote_script(python: &str, args: &[String]) -> String {
    let quoted: Vec<String> = args.iter().map(|a| sh_quote(a)).collect();
    format!(
        "echo GOOSE_RANK_PID=$$; exec {} {}",
        sh_quote(python),
        quoted.join(" ")
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankMemory {
    pub active: u64,
    pub peak: u64,
    pub cache: u64,
}

/// What a rank has told us so far, from its own output.
#[derive(Debug, Default)]
pub struct RankLive {
    /// The rank's own pid (this Mac: the child; a peer: `GOOSE_RANK_PID=`).
    pub pid: Option<u32>,
    pub lines: u64,
    pub tail: VecDeque<String>,
    pub group_joined: bool,
    pub caps: Option<serde_json::Value>,
    pub memory: Option<RankMemory>,
    /// The rank printed its `GOOSE_READY` (the pipeline server, after its warm-up batch).
    pub ready: bool,
}

/// Where a rank's start is, from its own reports. Both rank programs report `RANK_CAPS` once the
/// weights are in (the fork after `load_stage`, the tensor wrapper as its server starts), so
/// before it the rank is loading; the pipeline server then runs a warm-up batch and prints
/// `READY`. The tensor wrapper prints no READY of its own — rank 0's readiness completion is its
/// warm-up, judged by the coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RankPhase {
    Loading,
    Warming,
    Ready,
}

impl RankPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            RankPhase::Loading => "loading",
            RankPhase::Warming => "warming",
            RankPhase::Ready => "ready",
        }
    }
}

impl RankLive {
    pub fn tail_text(&self) -> String {
        self.tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    pub fn phase(&self) -> RankPhase {
        if self.ready {
            RankPhase::Ready
        } else if self.caps.is_some() {
            RankPhase::Warming
        } else {
            RankPhase::Loading
        }
    }

    fn take_line(&mut self, line: &str) {
        self.lines += 1;
        if let Some(pid) = line.strip_prefix("GOOSE_RANK_PID=") {
            if let Ok(pid) = pid.trim().parse() {
                self.pid = Some(pid);
            }
        } else if line.starts_with("GOOSE_RANK_GROUP ") {
            self.group_joined = true;
        } else if let Some(caps) = line.strip_prefix("GOOSE_RANK_CAPS ") {
            self.caps = serde_json::from_str(caps).ok();
        } else if line.starts_with("GOOSE_READY ") {
            self.ready = true;
        } else if let Some(memory) = line.strip_prefix("GOOSE_RANK_MEM ") {
            if let Ok(memory) = serde_json::from_str::<RankMemory>(memory) {
                self.memory = Some(memory);
            }
            return;
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line.to_string());
    }
}

pub struct RankProcess {
    pub rank: usize,
    pub node: String,
    /// `None` = this Mac.
    pub host: Option<String>,
    /// The rank itself on this Mac; on a peer, the local ssh client carrying it.
    pub child: Child,
    pub live: Arc<StdMutex<RankLive>>,
    /// The owner token the rank was launched with (`RankSpec::owner`): a stop that must sweep a
    /// node for this rank signals only ranks carrying it.
    pub owner: Option<String>,
}

impl RankProcess {
    pub fn pid(&self) -> Option<u32> {
        self.live.lock().unwrap().pid
    }

    pub fn tail(&self) -> String {
        self.live.lock().unwrap().tail_text()
    }
}

fn read_lines<R: AsyncRead + Unpin + Send + 'static>(reader: R, live: Arc<StdMutex<RankLive>>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            live.lock().unwrap().take_line(line.trim_end_matches('\r'));
        }
    });
}

pub fn spawn_rank(node: &NodeConfig, spec: &RankSpec) -> Result<RankProcess> {
    let python = spec.interpreter(node)?;
    let args = python_args(spec)?;
    let mut cmd = match node.host() {
        None => {
            let mut cmd = Command::new(python);
            cmd.args(&args)
                .env("PATH", crate::engine::sidecar_spawn_path())
                .env("DO_NOT_TRACK", "1");
            cmd
        }
        Some(alias) => {
            let mut cmd = Command::new("/usr/bin/ssh");
            cmd.arg("-tt")
                .args(SSH_OPTIONS)
                .args(["-o", "LogLevel=QUIET"])
                .arg(alias)
                .arg(remote_script(python, &args));
            cmd
        }
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::subprocess::configure_subprocess(&mut cmd);
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "spawning rank {} on {} ({})",
            spec.rank,
            node.name,
            node.host().unwrap_or("this Mac")
        )
    })?;
    let live = Arc::new(StdMutex::new(RankLive {
        pid: if node.is_local() { child.id() } else { None },
        ..Default::default()
    }));
    if let Some(stdout) = child.stdout.take() {
        read_lines(stdout, Arc::clone(&live));
    }
    if let Some(stderr) = child.stderr.take() {
        read_lines(stderr, Arc::clone(&live));
    }
    Ok(RankProcess {
        rank: spec.rank,
        node: node.name.clone(),
        host: node.ssh.clone(),
        child,
        live,
        owner: spec.owner.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;

    fn launch(planned_bytes: u64, prompt_cache_limit_bytes: u64) -> TensorLaunch {
        TensorLaunch {
            planned_bytes,
            prompt_cache_limit_bytes,
            prompt_cache_entries: 3,
        }
    }

    #[test]
    fn jaccl_specs_reproduce_the_proven_hostfile() {
        let config = two_mac_config();
        let specs = rank_specs(
            &config,
            "node-alias",
            &[launch(20, 2), launch(21, 2)],
            65_536,
            2.0,
        );
        // hostfile-jaccl.json: rdma [null,"rdma_en3"] / ["rdma_en3",null], coordinator hosts[0].ips[0].
        let devices = specs[0].ibv_devices.as_ref().unwrap();
        assert_eq!(devices[0], vec![None, Some("rdma_en3".to_string())]);
        assert_eq!(devices[1], vec![Some("rdma_en3".to_string()), None]);
        assert_eq!(specs[1].coordinator.as_deref(), Some("192.168.0.1:32323"));
        assert_eq!(specs[1].model_dir, config.nodes[1].model_dir);
        assert!(matches!(
            specs[1].program,
            RankProgram::MlxLmServer {
                planned_bytes: 21,
                prompt_cache_limit_bytes: Some(2),
                prompt_cache_entries: Some(3),
                prompt_cache_bytes: None,
                context_window: 65_536,
                doorbell: true,
            }
        ));
        assert!(specs[0].ring_hosts.is_none());
        assert!(
            specs.iter().all(|s| s.served_id == "node-alias"),
            "every rank serves the id it was given, never the HF id {}",
            config.model_id
        );
    }

    #[test]
    fn ring_specs_give_each_rank_its_own_port() {
        let mut config = two_mac_config();
        config.backend = Backend::Ring;
        let specs = rank_specs(
            &config,
            "node-alias",
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        );
        assert_eq!(
            specs[0].ring_hosts.as_ref().unwrap(),
            &vec![
                vec!["192.168.0.1:32323".to_string()],
                vec!["192.168.0.2:32324".to_string()]
            ]
        );
        assert!(specs[0].ibv_devices.is_none() && specs[0].coordinator.is_none());
    }

    #[test]
    fn the_remote_command_prints_the_pid_then_execs_the_marked_rank() {
        let config = two_mac_config();
        let spec = &rank_specs(
            &config,
            "node-alias",
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )[1];
        let args = python_args(spec).unwrap();
        let script = remote_script(&config.nodes[1].python, &args);
        assert!(script
            .starts_with("echo GOOSE_RANK_PID=$$; exec '/tmp/jaccl-smoke/.venv/bin/python' '-c' "));
        assert!(script.ends_with(&format!("'{RANK_MARKER}'")));
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&args[3])
            .unwrap();
        let back: RankSpec = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(&back, spec);
    }

    /// The owner token rides after the marker — on this Mac's rank and inside the ssh carrier's
    /// remote script alike — and `ps` reads it back from both; a spec without one (an older
    /// requester's) still reads, and writes no token.
    #[test]
    fn the_owner_token_follows_the_marker_and_ps_reads_it_back() {
        let config = two_mac_config();
        let mut spec = rank_specs(
            &config,
            "node-alias",
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )
        .remove(1);
        let bare = python_args(&spec).unwrap();
        assert_eq!(bare.last().map(String::as_str), Some(RANK_MARKER));

        spec.owner = Some("0123abcd".to_string());
        let args = python_args(&spec).unwrap();
        assert_eq!(args[args.len() - 2], RANK_MARKER);
        assert_eq!(args[args.len() - 1], format!("{OWNER_ARG_PREFIX}0123abcd"));
        let local = format!("11 {} {}", config.nodes[1].python, args.join(" "));
        let carrier = format!(
            "12 /usr/bin/ssh -tt workhorse {}",
            remote_script(&config.nodes[1].python, &args)
        );
        let ranks = crate::distributed::probe::goose_rank_processes(
            &format!("{local}\n{carrier}\n"),
            None,
            &[],
        );
        let read: Vec<(u32, Option<&str>, bool)> = ranks
            .iter()
            .map(|r| (r.pid, r.owner.as_deref(), r.carrier))
            .collect();
        assert_eq!(
            read,
            vec![(11, Some("0123abcd"), false), (12, Some("0123abcd"), true)]
        );

        let mut older = serde_json::to_value(&spec).unwrap();
        older.as_object_mut().unwrap().remove("owner");
        let back: RankSpec = serde_json::from_value(older).unwrap();
        assert_eq!(back.owner, None);
    }

    #[tokio::test]
    async fn the_boot_line_runs_the_wrapper_it_is_given() {
        // The boot one-liner, fed a stand-in wrapper, sees the spec as argv[2] and the marker as
        // argv[3] — the positions rank_wrapper.py reads.
        let b64 = base64::engine::general_purpose::STANDARD;
        let stand_in = "import sys,base64,json;print(json.loads(base64.b64decode(sys.argv[2]))['rank'], sys.argv[3])";
        let out = tokio::process::Command::new("/usr/bin/python3")
            .args([
                "-c",
                BOOT,
                &b64.encode(stand_in),
                &b64.encode(r#"{"rank":1}"#),
                RANK_MARKER,
            ])
            .output()
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("1 {RANK_MARKER}")
        );
    }

    fn pipeline_config() -> DistributedConfig {
        let mut config = two_mac_config();
        config.nodes[0].pipeline_python =
            Some("/Users/me/.goose/distributed/fork/bin/python".into());
        config.nodes[1].pipeline_python =
            Some("/Users/workhorse/.goose/distributed/fork/bin/python".into());
        config
    }

    #[test]
    fn a_pipeline_rank_serves_the_approved_split_under_the_forks_interpreter() {
        let config = pipeline_config();
        let plan = crate::distributed::plan::parse_pipeline_plan(
            crate::distributed::plan::tests::FLASH_PLAN_32K,
        )
        .unwrap();
        let specs = pipeline_rank_specs(&config, "node-alias", 32_768, &plan.split_arg(), 2.0);
        for (rank, spec) in specs.iter().enumerate() {
            let RankProgram::PipelineServe { serve_args } = &spec.program else {
                panic!("{spec:?}");
            };
            assert_eq!(
                serve_args,
                &[
                    "--model",
                    config.nodes[rank].model_dir.as_str(),
                    "--served-model-name",
                    "node-alias",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    "8190",
                    "--context",
                    "32768",
                    "--slots",
                    "2",
                    "--max-batch",
                    "2",
                    "--split",
                    "19",
                ]
            );
            assert_eq!(
                spec.interpreter(&config.nodes[rank]).unwrap(),
                config.nodes[rank].pipeline_python.as_deref().unwrap()
            );
        }
        assert_eq!(specs[1].coordinator.as_deref(), Some("192.168.0.1:32323"));
        let args = python_args(&specs[1]).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let program = String::from_utf8(b64.decode(&args[2]).unwrap()).unwrap();
        assert_eq!(
            program,
            concat!(
                include_str!("rank_env.py"),
                include_str!("rank_live.py"),
                include_str!("pipeline_rank.py")
            )
        );
        let script = remote_script(specs[1].interpreter(&config.nodes[1]).unwrap(), &args);
        assert!(script.starts_with(
            "echo GOOSE_RANK_PID=$$; exec '/Users/workhorse/.goose/distributed/fork/bin/python' '-c' "
        ));

        let mut four = config.clone();
        four.slots = Some(4);
        let RankProgram::PipelineServe { serve_args } =
            &pipeline_rank_specs(&four, "node-alias", 8_192, "19", 2.0)[0].program
        else {
            unreachable!()
        };
        let joined = serve_args.join(" ");
        assert!(
            joined.contains("--slots 4 --max-batch 4 --split 19"),
            "{joined}"
        );

        let mut tensor_only = config.clone();
        tensor_only.nodes[1].pipeline_python = None;
        let err = specs[1]
            .interpreter(&tensor_only.nodes[1])
            .unwrap_err()
            .to_string();
        assert!(err.contains("pipeline_python"), "{err}");
        let tensor = &rank_specs(
            &config,
            "node-alias",
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )[1];
        assert_eq!(
            tensor.interpreter(&config.nodes[1]).unwrap(),
            config.nodes[1].python
        );
    }

    /// The REAL pipeline program (prelude + pipeline_rank.py), booted exactly as a rank is, against
    /// stand-in `mlx.core` and `pipeline_qwen4_serve` modules: it sets the JACCL env, parses goose's
    /// argv with the fork's parser, starts the memory reporter, hands serve() goose's emit, and
    /// exits with serve()'s code — without ever calling mx.distributed.init itself.
    #[tokio::test]
    async fn the_pipeline_program_runs_serve_with_the_parsed_args_and_exits_with_its_code() {
        let root = tempfile::tempdir().unwrap();
        let site = root.path();
        std::fs::create_dir_all(site.join("mlx")).unwrap();
        std::fs::create_dir_all(site.join("rapid_mlx/distributed")).unwrap();
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx/core.py"),
            "class _D:\n    @staticmethod\n    def init(*a, **k):\n        raise SystemExit('the wrapper called mx.distributed.init')\n\
             distributed = _D()\n\
             def get_active_memory(): return 11\n\
             def get_peak_memory(): return 22\n\
             def get_cache_memory(): return 3\n",
        )
        .unwrap();
        std::fs::write(site.join("rapid_mlx/__init__.py"), "").unwrap();
        std::fs::write(site.join("rapid_mlx/distributed/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("rapid_mlx/distributed/pipeline_qwen4_serve.py"),
            "import os, time\n\
             from dataclasses import dataclass\n\
             @dataclass\n\
             class _Job:\n\
             \x20   row: object\n\
             \x20   produced: int = 0\n\
             def run_batch(stage, guard, rows, prefill_step, on_tokens=None, control_fn=None): pass\n\
             def _step(stage, out, cache, rows, guard, control, *, sample): pass\n\
             def _build_app(state, tokenizer, eos_ids, vision=None): pass\n\
             def add_arguments(parser):\n\
             \x20   parser.add_argument('--model', required=True)\n\
             \x20   parser.add_argument('--served-model-name', required=True)\n\
             \x20   parser.add_argument('--host', default='127.0.0.1')\n\
             \x20   parser.add_argument('--port', type=int, required=True)\n\
             \x20   parser.add_argument('--context', type=int, required=True)\n\
             \x20   parser.add_argument('--slots', type=int, default=2)\n\
             \x20   parser.add_argument('--max-batch', type=int, default=2)\n\
             \x20   parser.add_argument('--prefill-step', type=int)\n\
             \x20   parser.add_argument('--split')\n\
             def serve(options, emit=None):\n\
             \x20   time.sleep(0.3)\n\
             \x20   emit('READY', dict(vars(options), rank=int(os.environ['MLX_RANK']), \
             coordinator=os.environ.get('MLX_JACCL_COORDINATOR'), devices=open(os.environ['MLX_IBV_DEVICES']).read(), \
             hostfile=os.environ.get('MLX_HOSTFILE'), offline=os.environ.get('HF_HUB_OFFLINE')))\n\
             \x20   return 7\n",
        )
        .unwrap();
        let mut config = pipeline_config();
        config.slots = Some(3);
        config.nodes[1].pipeline_python = Some("/usr/bin/python3".into());
        let mut spec = pipeline_rank_specs(&config, "node-alias", 32_768, "19", 0.05).remove(1);
        spec.memory_report_seconds = 0.05;
        let out = tokio::process::Command::new(spec.interpreter(&config.nodes[1]).unwrap())
            .args(python_args(&spec).unwrap())
            .env("PYTHONPATH", site)
            .output()
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(7),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut live = RankLive::default();
        stdout.lines().for_each(|l| live.take_line(l));
        assert_eq!(
            live.memory,
            Some(RankMemory {
                active: 11,
                peak: 22,
                cache: 3
            }),
            "the reporter ran while serve() did: {stdout}"
        );
        let ready: serde_json::Value = serde_json::from_str(
            stdout
                .lines()
                .find_map(|l| l.strip_prefix("GOOSE_READY "))
                .unwrap_or_else(|| panic!("{stdout}")),
        )
        .unwrap();
        assert_eq!(ready["model"], config.nodes[1].model_dir.as_str());
        assert_eq!(ready["served_model_name"], "node-alias");
        assert_eq!(ready["port"], 8190);
        assert_eq!(ready["context"], 32_768);
        assert_eq!(
            ready["slots"], 3,
            "the configured slots, not the fork's default"
        );
        assert_eq!(ready["max_batch"], 3);
        assert_eq!(ready["split"], "19");
        assert_eq!(ready["prefill_step"], serde_json::Value::Null);
        assert_eq!(ready["rank"], 1);
        assert_eq!(ready["coordinator"], "192.168.0.1:32323");
        assert_eq!(
            ready["devices"],
            r#"[[null, "rdma_en3"], ["rdma_en3", null]]"#
        );
        assert_eq!(
            ready["hostfile"],
            serde_json::Value::Null,
            "one backend's env only"
        );
        assert_eq!(ready["offline"], "1");
    }

    /// rank_live.py's two functions under a real interpreter, on the instants the 2-rank 27B
    /// tensor split measured (2026-09-24: prefill from 0.52 s, 2,048 tokens reported at 13.53 s,
    /// the first token at 21.069 s, the third at 21.354 s).
    #[test]
    fn the_live_request_table_reads_prefill_and_generation_apart() {
        let checks = r#"
queued = live_request("r", 0.0, 0.5, max_tokens=60)
assert (queued["phase"], queued["status"]) == ("queued", "waiting"), queued
assert queued["prompt_tokens_per_second"] is None and queued["tokens_per_second"] is None
reading = live_request("r", 0.0, 13.53, prompt_tokens=3249, prefill_started=0.52, prefilled=2048)
assert (reading["phase"], reading["status"]) == ("prefill", "running"), reading
assert reading["prompt_tokens_per_second"] == round(2048 / 13.01, 2), reading
assert reading["tokens_per_second"] is None
writing = live_request("r", 0.0, 21.354, prompt_tokens=3249, prefill_started=0.52,
                       prefilled=3249, first_token=21.069, last_token=21.354, completion=3)
assert writing["phase"] == "generation", writing
assert writing["prompt_tokens_per_second"] == round(3249 / (21.069 - 0.52), 2), writing
assert writing["tokens_per_second"] == round(2 / (21.354 - 21.069), 2), writing
assert writing["ttft_s"] == 21.069
one = live_request("r", 0.0, 21.1, first_token=21.069, last_token=21.069, completion=1)
assert one["tokens_per_second"] is None, "one token has no decode span"
busy = live_status({"num_running": 1, "num_waiting": 0}, [writing, queued])
assert busy["status"] == "generating" and busy["generation_tps"] == writing["tokens_per_second"]
assert busy["num_running"] == 1 and len(busy["requests"]) == 2
idle = live_status({"num_running": 0, "num_waiting": 0}, [])
assert (idle["status"], idle["generation_tps"], idle["requests"]) == ("idle", None, [])
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_live.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// rank_budget.py under a real interpreter, on Q-65's turn (2026-09-25, the 2-rank 27B split
    /// launched for 262,144 tokens): a 49,939-token prompt with no max_tokens stopped at exactly
    /// 512 — mlx_lm's `--max-tokens` default. The budget is the window's room instead.
    #[test]
    fn a_request_without_max_tokens_runs_to_the_windows_room_not_512() {
        let checks = r#"
assert generation_budget(262144, 49939, None) == 262144 - 49939
assert generation_budget(262144, 49939, None) > 512
assert generation_budget(262144, 49939, 4096) == 4096, "a client's own max_tokens stands"
assert generation_budget(262144, 262000, 4096) == 144, "never past the window"
assert generation_budget(262144, 262143, None) == 1
for full in (262144, 300000):
    try:
        generation_budget(262144, full, None)
    except ContextFull as refusal:
        assert "262144" in str(refusal) and str(full) in str(refusal), refusal
        assert "maximum context length" in str(refusal), "goose reads it as context-exceeded"
    else:
        raise AssertionError(f"a {full}-token prompt has no room")
print("ok")
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-c")
            .arg(format!("{}{checks}", include_str!("rank_budget.py")))
            .output()
            .expect("/usr/bin/python3 runs the rank programs' pure half");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
    }

    /// The tensor program a rank receives carries the budget ahead of the wrapper that calls it,
    /// and the spec hands it the launch's window under the key the wrapper reads
    /// (`spec["context_window"]`).
    #[test]
    fn the_tensor_program_carries_the_budget_and_the_launch_window() {
        let config = two_mac_config();
        let specs = rank_specs(
            &config,
            "node-alias",
            &[launch(1, 2), launch(3, 2)],
            262_144,
            2.0,
        );
        let args = python_args(&specs[1]).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD;
        let program = String::from_utf8(b64.decode(&args[2]).unwrap()).unwrap();
        assert_eq!(
            program,
            concat!(
                include_str!("rank_env.py"),
                include_str!("rank_live.py"),
                include_str!("rank_budget.py"),
                include_str!("rank_wrapper.py")
            )
        );
        let spec: serde_json::Value =
            serde_json::from_slice(&b64.decode(&args[3]).unwrap()).unwrap();
        assert_eq!(spec["context_window"], 262_144);
    }

    /// Q-66's doorbell needs every rank's wrapper to take part, so the spec says so twice: the
    /// `doorbell` flag the wrapper reads, and a program tag an older peer's goosed cannot parse
    /// (it refuses at rank start instead of joining a launch it would hang).
    #[test]
    fn the_tensor_spec_asks_for_the_doorbell_and_an_older_peer_refuses_it() {
        let config = two_mac_config();
        let spec = rank_specs(
            &config,
            "node-alias",
            &[launch(1, 2), launch(3, 2)],
            8_192,
            2.0,
        )
        .remove(1);
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["program"], "mlxLmServerDoorbell");
        assert_eq!(json["doorbell"], true);

        // An older requester's spec (no doorbell, the old tag, its one prompt cache number)
        // still starts a rank here, waiting the upstream way.
        let mut older = json.clone();
        older["program"] = "mlxLmServer".into();
        let fields = older.as_object_mut().unwrap();
        fields.remove("doorbell");
        fields.remove("prompt_cache_limit_bytes");
        fields.remove("prompt_cache_entries");
        fields.insert("prompt_cache_bytes".into(), 4_715_872_256u64.into());
        let read: RankSpec = serde_json::from_value(older).unwrap();
        assert!(matches!(
            read.program,
            RankProgram::MlxLmServer {
                doorbell: false,
                prompt_cache_bytes: Some(4_715_872_256),
                prompt_cache_limit_bytes: None,
                prompt_cache_entries: None,
                ..
            }
        ));

        // An older peer's goosed (its enum has no doorbell tag) cannot read this spec, and the
        // requester says what to do about it.
        #[derive(Debug, Deserialize)]
        #[serde(tag = "program", rename_all = "camelCase")]
        #[allow(dead_code)]
        enum OlderProgram {
            MlxLmServer {
                context_window: u64,
                prompt_cache_bytes: u64,
                planned_bytes: u64,
            },
            PipelineServe {
                serve_args: Vec<String>,
            },
        }
        let refused = serde_json::from_value::<OlderProgram>(json).unwrap_err();
        let why = link_control_refusal(&refused.to_string());
        assert!(why.is_some_and(|w| w.contains("update goose")), "{refused}");
    }

    fn link_control_refusal(error: &str) -> Option<&'static str> {
        super::super::link_control::older_peer_refusal(error)
    }

    /// The REAL tensor program, booted as a rank is (doorbell off: the stand-in has no peer),
    /// against stand-in `mlx.core` and `mlx_lm` modules whose `run` builds its prompt cache
    /// exactly as mlx_lm 0.31.3's does (`LRUPromptCache(cli_args.prompt_cache_size)`,
    /// server.py:1743). Returns what that `run` saw: argv, the cache's max_size / max_bytes.
    async fn boot_against_stand_ins(mut spec: RankSpec) -> serde_json::Value {
        let root = tempfile::tempdir().unwrap();
        let site = root.path();
        std::fs::create_dir_all(site.join("mlx")).unwrap();
        std::fs::create_dir_all(site.join("mlx_lm")).unwrap();
        std::fs::write(site.join("mlx/__init__.py"), "").unwrap();
        std::fs::write(
            site.join("mlx/core.py"),
            "import os\n\
             __version__ = '0.32.2'\n\
             class _G:\n\
             \x20   def rank(self): return int(os.environ['MLX_RANK'])\n\
             \x20   def size(self): return 2\n\
             class _D:\n\
             \x20   @staticmethod\n\
             \x20   def init(strict=False, backend='any'): return _G()\n\
             distributed = _D()\n\
             def device_info(): return {'memory_size': 128, 'max_recommended_working_set_size': 100}\n\
             def set_memory_limit(n): pass\n\
             def set_wired_limit(n): pass\n\
             def set_cache_limit(n): pass\n\
             def get_active_memory(): return 1\n\
             def get_peak_memory(): return 1\n\
             def get_cache_memory(): return 0\n",
        )
        .unwrap();
        std::fs::write(site.join("mlx_lm/__init__.py"), "__version__ = '0.31.3'\n").unwrap();
        std::fs::write(
            site.join("mlx_lm/server.py"),
            "import argparse, json, sys\n\
             class LRUPromptCache:\n\
             \x20   def __init__(self, max_size=10, max_bytes=1 << 63):\n\
             \x20       self.max_size, self.max_bytes = max_size, max_bytes\n\
             class ResponseGenerator:\n\
             \x20   def _next_request(self, timeout=None): pass\n\
             \x20   def generate(self, request, args, progress_callback=None): pass\n\
             \x20   def _tokenize(self, tokenizer, request, args): pass\n\
             \x20   def _share_request(self, request): pass\n\
             \x20   def _generate(self): pass\n\
             class APIHandler:\n\
             \x20   def do_GET(self): pass\n\
             \x20   def do_POST(self): pass\n\
             \x20   def validate_model_parameters(self): pass\n\
             \x20   def _set_completion_headers(self, status): pass\n\
             class ModelProvider:\n\
             \x20   def __init__(self, cli_args): self.cli_args, self._model_map = cli_args, {}\n\
             \x20   def load(self, *a): pass\n\
             def run(host, port, model_provider):\n\
             \x20   cache = LRUPromptCache(model_provider.cli_args.prompt_cache_size)\n\
             \x20   print('GOOSE_STANDIN ' + json.dumps({'argv': sys.argv[1:], 'max_size': cache.max_size, \
             'max_bytes': cache.max_bytes, 'served': model_provider._model_map}), flush=True)\n\
             def main():\n\
             \x20   p = argparse.ArgumentParser()\n\
             \x20   p.add_argument('--model'); p.add_argument('--host'); p.add_argument('--port', type=int)\n\
             \x20   p.add_argument('--prompt-cache-size', type=int, default=10)\n\
             \x20   p.add_argument('--prompt-cache-bytes', type=int)\n\
             \x20   args = p.parse_args()\n\
             \x20   run(args.host, args.port, ModelProvider(args))\n",
        )
        .unwrap();
        spec.memory_report_seconds = 0.05;
        if let RankProgram::MlxLmServer { doorbell, .. } = &mut spec.program {
            *doorbell = false;
        }
        let out = tokio::process::Command::new("/usr/bin/python3")
            .args(python_args(&spec).unwrap())
            .env("PYTHONPATH", site)
            .output()
            .await
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_str(
            stdout
                .lines()
                .find_map(|l| l.strip_prefix("GOOSE_STANDIN "))
                .unwrap_or_else(|| panic!("{stdout}")),
        )
        .unwrap()
    }

    fn flag(served: &serde_json::Value, name: &str) -> Option<String> {
        let argv: Vec<String> = serde_json::from_value(served["argv"].clone()).unwrap();
        let at = argv.iter().position(|a| a == name)?;
        Some(argv[at + 1].clone())
    }

    /// The server receives both flags from the spec (E2E #1's figures), and the cache it builds is
    /// bounded by the same bytes, which mlx_lm itself never passes.
    #[tokio::test]
    async fn the_tensor_program_hands_mlx_lm_both_prompt_cache_bounds() {
        let config = two_mac_config();
        let e2e = TensorLaunch {
            planned_bytes: 27_456_216_576,
            prompt_cache_limit_bytes: 9_431_744_512,
            prompt_cache_entries: 110,
        };
        let spec = rank_specs(&config, "node-alias", &[e2e, e2e], 141_568, 2.0).remove(1);
        let served = boot_against_stand_ins(spec).await;
        assert_eq!(flag(&served, "--prompt-cache-size").as_deref(), Some("110"));
        assert_eq!(
            flag(&served, "--prompt-cache-bytes").as_deref(),
            Some("9431744512")
        );
        assert_eq!(served["max_size"], 110);
        assert_eq!(
            served["max_bytes"], 9_431_744_512u64,
            "the cache mlx_lm builds is bounded between admissions too"
        );
        assert_eq!(
            served["served"]["node-alias"],
            config.nodes[1].model_dir.as_str()
        );
    }

    /// An older requester's spec runs the policy its own rank 0 runs — its one number as the flag,
    /// mlx_lm's default count, no max_bytes — so the two ranks evict alike.
    #[tokio::test]
    async fn an_older_requesters_spec_runs_its_own_prompt_cache_policy() {
        let config = two_mac_config();
        let mut spec = rank_specs(
            &config,
            "node-alias",
            &[launch(1, 1), launch(1, 1)],
            8_192,
            2.0,
        )
        .remove(1);
        if let RankProgram::MlxLmServer {
            prompt_cache_limit_bytes,
            prompt_cache_entries,
            prompt_cache_bytes,
            ..
        } = &mut spec.program
        {
            *prompt_cache_limit_bytes = None;
            *prompt_cache_entries = None;
            *prompt_cache_bytes = Some(4_715_872_256);
        }
        let served = boot_against_stand_ins(spec).await;
        assert_eq!(
            flag(&served, "--prompt-cache-bytes").as_deref(),
            Some("4715872256")
        );
        assert_eq!(flag(&served, "--prompt-cache-size"), None);
        assert_eq!(served["max_size"], 10);
        assert_eq!(served["max_bytes"], serde_json::json!(1u64 << 63));
    }

    /// The phases a pipeline rank walks, from its own lines (the 27B's figures as a reporter thread
    /// read them mid-`mx.eval`, 2026-09-24): loading until RANK_CAPS, warming until READY.
    #[test]
    fn a_ranks_phase_is_its_own_reports() {
        let mut live = RankLive::default();
        assert_eq!(live.phase(), RankPhase::Loading);
        live.take_line("GOOSE_RANK_GROUP {\"rank\": 1, \"size\": 2}");
        live.take_line(
            "GOOSE_RANK_MEM {\"active\": 21861391624, \"peak\": 21861391624, \"cache\": 0}",
        );
        assert_eq!(live.phase(), RankPhase::Loading);
        assert_eq!(live.memory.map(|m| m.active), Some(21_861_391_624));
        live.take_line("GOOSE_RANK_CAPS {\"memory_limit\": 1, \"planned\": 2}");
        assert_eq!(live.phase(), RankPhase::Warming);
        live.take_line("GOOSE_READY {\"rank\": 1, \"pid\": 7, \"layers\": [20, 48]}");
        assert_eq!(live.phase(), RankPhase::Ready);
    }

    #[test]
    fn rank_output_markers_are_read_and_memory_lines_stay_out_of_the_tail() {
        let mut live = RankLive::default();
        live.take_line("GOOSE_RANK_PID=4242");
        live.take_line("GOOSE_RANK_GROUP {\"rank\": 1, \"size\": 2}");
        live.take_line("GOOSE_RANK_MEM {\"active\": 10, \"peak\": 20, \"cache\": 1}");
        live.take_line("Traceback (most recent call last):");
        assert_eq!(live.pid, Some(4242));
        assert!(live.group_joined);
        assert_eq!(live.memory.unwrap().peak, 20);
        assert!(!live.tail_text().contains("GOOSE_RANK_MEM"));
        assert!(live.tail_text().contains("Traceback"));
    }
}
