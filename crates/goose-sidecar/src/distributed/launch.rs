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
//! every rank) followed by the runner's program — `rank_wrapper.py` (`mlx_lm.server`, tensor
//! split, under `NodeConfig::python`) or `pipeline_rank.py` (the fork's `pipeline_qwen4_serve`,
//! layer split, under `NodeConfig::pipeline_python`).

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use super::config::{Backend, DistributedConfig, NodeConfig};
use super::exec::{sh_quote, SSH_OPTIONS};
use super::{MEMORY_LIMIT_RATIO, PIPELINE_MAX_BATCH, WIRED_LIMIT_RATIO};

/// The literal every goose rank carries on its command line, so `ps` can name a rank a previous
/// goosed left behind (and `stop` can reclaim it per-pid).
pub const RANK_MARKER: &str = "goose-distributed-rank";

const TENSOR_PROGRAM: &str = concat!(include_str!("rank_env.py"), include_str!("rank_wrapper.py"));
const PIPELINE_PROGRAM: &str = concat!(
    include_str!("rank_env.py"),
    include_str!("pipeline_rank.py")
);
const BOOT: &str = "import base64,sys;exec(base64.b64decode(sys.argv[1]))";
const TAIL_LINES: usize = 200;

/// What a rank runs, with the fields only that program reads (flattened into the spec JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "program", rename_all = "camelCase")]
pub enum RankProgram {
    /// `mlx_lm.server` under `rank_wrapper.py` (tensor split).
    MlxLmServer {
        context_window: u64,
        prompt_cache_bytes: u64,
        planned_bytes: u64,
        memory_limit_ratio: f64,
        wired_limit_ratio: f64,
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
    #[serde(flatten)]
    pub program: RankProgram,
}

/// The per-rank tensor specs. See [`base_specs`] for the backend env.
pub fn rank_specs(
    config: &DistributedConfig,
    served_id: &str,
    per_rank: &[(u64, u64)],
    context_window: u64,
    memory_report_seconds: f64,
) -> Vec<RankSpec> {
    base_specs(config, served_id, memory_report_seconds, |rank, _| {
        let (planned_bytes, prompt_cache_bytes) = per_rank[rank];
        RankProgram::MlxLmServer {
            context_window,
            prompt_cache_bytes,
            planned_bytes,
            memory_limit_ratio: MEMORY_LIMIT_RATIO,
            wired_limit_ratio: WIRED_LIMIT_RATIO,
        }
    })
}

/// The `pipeline_qwen4 serve` arguments for one rank: this node's own model dir, the served id,
/// rank 0's loopback port, the context preflight allowed, the batch the plan was made for, and the
/// split preflight approved (`--split` = ranks 1..N-1's starts), so the fork loads exactly that
/// split instead of re-balancing on its own load-time figures.
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
        "--max-batch",
        &PIPELINE_MAX_BATCH.to_string(),
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

/// The interpreter's arguments: the boot one-liner, the program, the spec, the marker.
pub fn python_args(spec: &RankSpec) -> Result<Vec<String>> {
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(vec![
        "-c".to_string(),
        BOOT.to_string(),
        b64.encode(spec.program_source()),
        b64.encode(serde_json::to_vec(spec)?),
        RANK_MARKER.to_string(),
    ])
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
}

impl RankLive {
    pub fn tail_text(&self) -> String {
        self.tail.iter().cloned().collect::<Vec<_>>().join("\n")
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::config::tests::two_mac_config;

    #[test]
    fn jaccl_specs_reproduce_the_proven_hostfile() {
        let config = two_mac_config();
        let specs = rank_specs(&config, "node-alias", &[(20, 1), (21, 2)], 65_536, 2.0);
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
                prompt_cache_bytes: 2,
                context_window: 65_536,
                ..
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
        let specs = rank_specs(&config, "node-alias", &[(1, 1), (1, 1)], 8_192, 2.0);
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
        let spec = &rank_specs(&config, "node-alias", &[(1, 1), (1, 1)], 8_192, 2.0)[1];
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
        assert!(program.starts_with(include_str!("rank_env.py")));
        assert!(program.ends_with(include_str!("pipeline_rank.py")));
        let script = remote_script(specs[1].interpreter(&config.nodes[1]).unwrap(), &args);
        assert!(script.starts_with(
            "echo GOOSE_RANK_PID=$$; exec '/Users/workhorse/.goose/distributed/fork/bin/python' '-c' "
        ));

        let mut tensor_only = config.clone();
        tensor_only.nodes[1].pipeline_python = None;
        let err = specs[1]
            .interpreter(&tensor_only.nodes[1])
            .unwrap_err()
            .to_string();
        assert!(err.contains("pipeline_python"), "{err}");
        let tensor = &rank_specs(&config, "node-alias", &[(1, 1), (1, 1)], 8_192, 2.0)[1];
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
             def add_arguments(parser):\n\
             \x20   parser.add_argument('--model', required=True)\n\
             \x20   parser.add_argument('--served-model-name', required=True)\n\
             \x20   parser.add_argument('--host', default='127.0.0.1')\n\
             \x20   parser.add_argument('--port', type=int, required=True)\n\
             \x20   parser.add_argument('--context', type=int, required=True)\n\
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
        assert_eq!(ready["max_batch"], 2);
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
