//! MEMORY COMPACTION ("Make room"): ask macOS to reclaim memory on a node before a model is placed
//! there — never by quitting anything, only by raising real memory pressure to the kernel's WARN
//! level, where macOS compresses and swaps idle apps' pages and apps drop their caches on the
//! memory-pressure notification.
//!
//! The pressure source is Apple's own `/usr/bin/memory_pressure -l warn` (system_cmds
//! memory_pressure.c), chosen over an allocator of goose's own because it:
//! - checks `kern.memorystatus_vm_pressure_level` BEFORE EVERY PAGE it faults and stops
//!   allocating the instant the level is reached (`reached_or_bypassed_desired_result`);
//! - fills each page with a JPEG fragment (incompressible), so the compressor cannot absorb the
//!   ballast and the pressure lands on the idle apps, and a second thread keeps re-touching the
//!   ballast so it stays active while idle pages age out;
//! - frees pages itself if the level overshoots its target;
//! - ships on every macOS, so a peer needs no goose Python to compact.
//!
//! goose's own loop around it decides on LEVELS, never on a clock: the ballast is released (per
//! pid, never a group) the moment the level leaves NORMAL — WARN as intended, CRITICAL aborted at
//! once — or when the tool exits on its own. The settle after the release is progress-based:
//! it ends when available memory stopped rising (`settled`). A node with any MLX engine on it is
//! REFUSED: our own running engine is never pressured.
//!
//! MEASURED 2026-09-24 (M3 Ultra 96 GB, nothing loaded): available 77.30 GiB → the kernel raised
//! WARN after 33.4 s with the ballast at 32.4 GiB RSS and 3.73 GiB available → release → 78.90
//! immediately, 79.22 at the 3rd 2-second sample, flat ±0.3 after; an earlier run on the same
//! Mac: 72.4 → 78.0 (77.4 fifteen seconds later).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::exec::NodeExec;
use super::node_op::NodeOp;
use super::probe::{self, Pressure};
use super::supervisor::POLL_INTERVAL;
use crate::MemoryReading;

/// measured: the recorded M3 Ultra settle (2 s samples after the release) peaked at its 3rd
/// sample (78.90 → 78.90 → 79.23) and never rose again; 3 samples without a new high is "stopped
/// rising".
pub const SETTLE_SAMPLES: usize = 3;
/// measured: the same settle's plateau read 79.21 / 79.22 / 79.23 GiB on consecutive samples —
/// 0.01–0.02 GiB of jitter on 96 GiB. A rise below 0.1% of RAM is that jitter, not reclaim.
pub const SETTLE_RISE_RATIO: f64 = 0.001;

/// The ballast's process name, as it appears in `ps` (the tool itself: a peer's goosed and the
/// requester both recognize it by this path).
pub const BALLAST_TOOL: &str = "/usr/bin/memory_pressure";

/// The pressure phase, run ON the node as one script. The ballast's lifetime is tied to the script
/// by a guardian subshell (it releases the ballast within one of its polls if the script dies —
/// an ssh session cut, a dropped Link request), so no ballast survives its caller. `$$` is the
/// script's own shell in sh and zsh alike, subshells included.
pub fn compaction_script() -> String {
    format!(
        r#"echo; echo @@before; /usr/sbin/sysctl -n hw.memsize kern.memorystatus_vm_pressure_level; /usr/bin/vm_stat
level=$(/usr/sbin/sysctl -n kern.memorystatus_vm_pressure_level)
if [ "$level" != 1 ]; then echo; echo @@refused; echo "$level"; echo; echo @@end; exit 0; fi
{BALLAST_TOOL} -l warn >/dev/null 2>&1 &
ballast=$!
( while /bin/kill -0 $$ 2>/dev/null && /bin/kill -0 $ballast 2>/dev/null; do /bin/sleep 1; done; /bin/kill $ballast 2>/dev/null ) >/dev/null 2>&1 &
echo; echo @@ballast; echo $ballast
echo; echo @@levels
while :; do
  level=$(/usr/sbin/sysctl -n kern.memorystatus_vm_pressure_level)
  echo "$level"
  if [ "$level" != 1 ]; then break; fi
  if ! /bin/kill -0 $ballast 2>/dev/null; then echo exited; break; fi
  /bin/sleep 0.2
done
echo; echo @@peak; /usr/bin/vm_stat
/bin/kill $ballast 2>/dev/null
wait $ballast
echo; echo @@released; echo $?
echo; echo @@after; /usr/sbin/sysctl -n hw.memsize kern.memorystatus_vm_pressure_level; /usr/bin/vm_stat
echo; echo @@end
"#
    )
}

/// How the pressure phase ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PhaseEnd {
    /// The kernel raised WARN; the ballast was released at once.
    Warn,
    /// The kernel jumped to CRITICAL; the ballast was released at once (reported loudly).
    Critical,
    /// The tool exited before any level change (it ran out of allocable memory).
    ToolExited,
}

impl PhaseEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            PhaseEnd::Warn => "warn",
            PhaseEnd::Critical => "critical",
            PhaseEnd::ToolExited => "toolExited",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PressurePhase {
    pub before: MemoryReading,
    pub ballast_pid: u32,
    /// Every level the loop read, in order.
    pub levels: Vec<Pressure>,
    pub end: PhaseEnd,
    /// Available memory at the moment the loop stopped (the kernel's WARN point, in goose's own
    /// available measure, when `end` is `Warn`).
    pub at_peak: MemoryReading,
    pub after: MemoryReading,
    pub after_level: Pressure,
}

/// What the script answered: the phase, or the node's refusal (its level was not NORMAL).
#[derive(Debug, Clone, PartialEq)]
pub enum PhaseAnswer {
    Ran(PressurePhase),
    NotNormal(Pressure),
}

fn reading(sections: &std::collections::BTreeMap<String, String>, name: &str) -> Result<(MemoryReading, Pressure)> {
    let text = probe::section(sections, name)?;
    let (sysctl, vm) = text
        .split_once("Mach Virtual Memory Statistics")
        .with_context(|| format!("`{name}` carries no vm_stat"))?;
    let values = probe::parse_sysctl_values(sysctl, 2)?;
    let reading = probe::parse_vm_stat(&format!("Mach Virtual Memory Statistics{vm}"), values[0])?;
    Ok((reading, Pressure::from_level(values[1])?))
}

/// Strict: an answer without every section is an error naming the missing one.
pub fn parse_pressure_phase(stdout: &str) -> Result<PhaseAnswer> {
    let sections = probe::sections(stdout);
    probe::section(&sections, "end").context("the compaction script did not finish")?;
    let (before, _) = reading(&sections, "before")?;
    if let Ok(refused) = probe::section(&sections, "refused") {
        let level = refused
            .trim()
            .parse::<u64>()
            .with_context(|| format!("refused level is not an integer: {refused}"))?;
        return Ok(PhaseAnswer::NotNormal(Pressure::from_level(level)?));
    }
    let ballast_pid = probe::section(&sections, "ballast")?
        .trim()
        .parse::<u32>()
        .context("the ballast pid is not an integer")?;
    let mut levels = Vec::new();
    let mut exited = false;
    for line in probe::section(&sections, "levels")?.lines().map(str::trim) {
        match line {
            "" => {}
            "exited" => exited = true,
            value => levels.push(Pressure::from_level(
                value
                    .parse()
                    .with_context(|| format!("level is not an integer: {value}"))?,
            )?),
        }
    }
    let last = *levels.last().context("the loop read no level")?;
    let end = match last {
        Pressure::Critical => PhaseEnd::Critical,
        Pressure::Warn => PhaseEnd::Warn,
        Pressure::Normal if exited => PhaseEnd::ToolExited,
        Pressure::Normal => anyhow::bail!("the loop stopped at NORMAL without the tool exiting"),
    };
    let at_peak = probe::parse_vm_stat(probe::section(&sections, "peak")?, before.total_bytes)?;
    let (after, after_level) = reading(&sections, "after")?;
    Ok(PhaseAnswer::Ran(PressurePhase {
        before,
        ballast_pid,
        levels,
        end,
        at_peak,
        after,
        after_level,
    }))
}

/// Available memory stopped rising: none of the last `SETTLE_SAMPLES` samples rose above the
/// highest sample before them by more than `SETTLE_RISE_RATIO` of RAM.
pub fn settled(samples: &[u64], total_bytes: u64) -> bool {
    if samples.len() <= SETTLE_SAMPLES {
        return false;
    }
    let (earlier, recent) = samples.split_at(samples.len() - SETTLE_SAMPLES);
    let high = earlier.iter().copied().max().unwrap_or(0);
    let rise = (total_bytes as f64 * SETTLE_RISE_RATIO) as u64;
    recent.iter().all(|s| *s <= high.saturating_add(rise))
}

/// One node's compaction, as the UI and the events report it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionReport {
    pub node: String,
    pub at_ms: u64,
    pub total_bytes: u64,
    pub before_available_bytes: u64,
    /// Available memory when the loop stopped: the kernel's WARN point in goose's measure.
    pub peak_available_bytes: u64,
    pub end: PhaseEnd,
    pub ballast_pid: u32,
    /// The settled figure: the last sample, taken once available stopped rising.
    pub settled_available_bytes: u64,
    /// Settled minus before; negative when the node had less after than before.
    pub gained_bytes: i64,
    pub settle_samples: usize,
}

impl CompactionReport {
    pub fn summary(&self) -> String {
        let gib = |b: u64| b as f64 / crate::GIB as f64;
        let gained = self.gained_bytes as f64 / crate::GIB as f64;
        format!(
            "{}: {} {gained:.1} GiB — available {:.1} → {:.1} GiB (kernel {} at {:.1} GiB available; settled after {} samples)",
            self.node,
            if self.gained_bytes >= 0 { "freed" } else { "lost" },
            gib(self.before_available_bytes),
            gib(self.settled_available_bytes),
            match self.end {
                PhaseEnd::Warn => "reached WARN",
                PhaseEnd::Critical => "jumped to CRITICAL — ballast released at once",
                PhaseEnd::ToolExited => "never left NORMAL — the tool exited",
            },
            gib(self.peak_available_bytes),
            self.settle_samples,
        )
    }
}

/// A compaction that did not run, with the reason the caller shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionRefusal {
    /// "engineLoaded" | "notNormal"
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompactionOutcome {
    Compacted(CompactionReport),
    Refused(CompactionRefusal),
}

/// Every MLX engine process in a `ps -axo pid=,command=` answer (single servers, distributed
/// ranks, the fork's pipeline) — a node holding any of them is never pressured.
pub fn engines_in(process_list: &str) -> Vec<(u32, String)> {
    probe::classify_foreign_engines(process_list, &[])
        .into_iter()
        .map(|(pid, command, _)| (pid, command))
        .collect()
}

pub fn engine_refusal(node: &str, engines: &[(u32, String)]) -> Option<CompactionRefusal> {
    (!engines.is_empty()).then(|| CompactionRefusal {
        code: "engineLoaded".to_string(),
        message: format!(
            "{node} runs an MLX engine ({}); compaction pressures every process on the Mac, so it \
             never runs beside a loaded model",
            engines
                .iter()
                .map(|(pid, command)| format!("pid {pid}: {command}"))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    })
}

fn sample(output: &super::ExecOutput) -> Result<u64> {
    anyhow::ensure!(
        output.success(),
        "the memory sample exited {:?}: {}",
        output.status,
        output.stderr.trim()
    );
    let sections = probe::sections(&output.stdout);
    let values = probe::parse_sysctl_values(probe::section(&sections, "sysctl")?, 2)?;
    Ok(probe::parse_vm_stat(probe::section(&sections, "vm")?, values[0])?.available_bytes)
}

/// Compact the node `host` (`None` = this Mac): refuse beside an engine, run the pressure phase,
/// then sample until available stopped rising.
pub async fn compact_node(
    exec: &dyn NodeExec,
    host: Option<&str>,
    node: &str,
) -> Result<CompactionOutcome> {
    let listing = exec.run_op(host, &NodeOp::ProcessList).await?;
    anyhow::ensure!(
        listing.success(),
        "listing {node}'s processes exited {:?}: {}",
        listing.status,
        listing.stderr.trim()
    );
    if let Some(refusal) = engine_refusal(node, &engines_in(&listing.stdout)) {
        return Ok(CompactionOutcome::Refused(refusal));
    }
    let answer = exec.run_op(host, &NodeOp::Compact).await?;
    let phase = match parse_pressure_phase(&answer.stdout).with_context(|| {
        format!(
            "{node}'s compaction answer (exit {:?}, stderr {})",
            answer.status,
            answer.stderr.trim()
        )
    })? {
        PhaseAnswer::Ran(phase) => phase,
        PhaseAnswer::NotNormal(level) => {
            return Ok(CompactionOutcome::Refused(CompactionRefusal {
                code: "notNormal".to_string(),
                message: format!(
                    "{node}'s kernel memory pressure is already {} — macOS is reclaiming on its \
                     own; compaction runs only from NORMAL",
                    level.as_str()
                ),
            }))
        }
    };
    let total = phase.before.total_bytes;
    let mut samples = vec![phase.after.available_bytes];
    while !settled(&samples, total) {
        tokio::time::sleep(POLL_INTERVAL).await;
        let output = exec.run_op(host, &NodeOp::Sample { pid: None }).await?;
        samples.push(sample(&output).with_context(|| format!("sampling {node} after release"))?);
    }
    let settled_bytes = *samples.last().expect("settle needs samples");
    Ok(CompactionOutcome::Compacted(CompactionReport {
        node: node.to_string(),
        at_ms: super::preflight::now_ms(),
        total_bytes: total,
        before_available_bytes: phase.before.available_bytes,
        peak_available_bytes: phase.at_peak.available_bytes,
        end: phase.end,
        ballast_pid: phase.ballast_pid,
        settled_available_bytes: settled_bytes,
        gained_bytes: settled_bytes as i64 - phase.before.available_bytes as i64,
        settle_samples: samples.len(),
    }))
}

/// One node's latest compaction as the status carries it: exactly one of `report`, `refusal`,
/// `error` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCompaction {
    pub node: String,
    pub at_ms: u64,
    /// "automatic" (a preflight found the node short) | "manual" ("Make room").
    pub trigger: String,
    pub report: Option<CompactionReport>,
    pub refusal: Option<CompactionRefusal>,
    pub error: Option<String>,
}

impl NodeCompaction {
    pub fn refused(node: &str, trigger: &str, refusal: CompactionRefusal) -> Self {
        Self {
            node: node.to_string(),
            at_ms: super::preflight::now_ms(),
            trigger: trigger.to_string(),
            report: None,
            refusal: Some(refusal),
            error: None,
        }
    }
}

/// [`compact_node`], folded into the record the status keeps.
pub async fn compact_recorded(
    exec: &dyn NodeExec,
    host: Option<&str>,
    node: &str,
    trigger: &str,
) -> NodeCompaction {
    let mut record = NodeCompaction {
        node: node.to_string(),
        at_ms: super::preflight::now_ms(),
        trigger: trigger.to_string(),
        report: None,
        refusal: None,
        error: None,
    };
    match compact_node(exec, host, node).await {
        Ok(CompactionOutcome::Compacted(report)) => record.report = Some(report),
        Ok(CompactionOutcome::Refused(refusal)) => record.refusal = Some(refusal),
        Err(e) => record.error = Some(format!("{e:#}")),
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GIB;

    const VM_77: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                  4000000.
Pages active:                                1000000.
Pages inactive:                              1000000.
Pages speculative:                            100000.
Pages throttled:                                   0.
Pages wired down:                             500000.
Pages purgeable:                               10000.
File-backed pages:                            900000.
Anonymous pages:                             1000000.
";

    fn vm(free: u64, file: u64) -> String {
        VM_77
            .replace("4000000", &free.to_string())
            .replace("900000", &file.to_string())
    }

    fn answer(levels: &str) -> String {
        format!(
            "\n@@before\n103079215104\n1\n{}\n@@ballast\n16716\n\n@@levels\n{levels}\n@@peak\n{}\n@@released\n143\n\n@@after\n103079215104\n1\n{}\n@@end\n",
            vm(4_000_000, 900_000),
            vm(200_000, 30_000),
            vm(4_300_000, 950_000),
        )
    }

    #[test]
    fn the_phase_stops_at_warn_and_reads_the_kernels_warn_point() {
        let PhaseAnswer::Ran(phase) = parse_pressure_phase(&answer("1\n1\n1\n2\n")).unwrap() else {
            panic!("a normal node runs the phase");
        };
        assert_eq!(phase.end, PhaseEnd::Warn);
        assert_eq!(phase.ballast_pid, 16716);
        assert_eq!(phase.levels.len(), 4);
        assert_eq!(phase.after_level, Pressure::Normal);
        // vm_stat's "Pages free" already excludes speculative: (free + file + purgeable) × 16 KiB.
        assert_eq!(
            phase.at_peak.available_bytes,
            (200_000 + 30_000 + 10_000) * 16_384
        );
        assert_eq!(phase.before.total_bytes, 103_079_215_104);
        assert!(phase.after.available_bytes > phase.before.available_bytes);
    }

    #[test]
    fn critical_and_a_tool_that_exits_are_named_never_read_as_warn() {
        let PhaseAnswer::Ran(critical) = parse_pressure_phase(&answer("1\n4\n")).unwrap() else {
            panic!()
        };
        assert_eq!(critical.end, PhaseEnd::Critical);
        let PhaseAnswer::Ran(exited) = parse_pressure_phase(&answer("1\n1\nexited\n")).unwrap()
        else {
            panic!()
        };
        assert_eq!(exited.end, PhaseEnd::ToolExited);
        assert!(parse_pressure_phase(&answer("1\n3\n")).is_err(), "3 is no xnu level");
        assert!(parse_pressure_phase(&answer("1\n1\n")).is_err());
    }

    #[test]
    fn a_node_already_under_pressure_is_refused_by_the_script() {
        let text = format!(
            "\n@@before\n103079215104\n2\n{}\n@@refused\n2\n\n@@end\n",
            vm(1, 1)
        );
        assert_eq!(
            parse_pressure_phase(&text).unwrap(),
            PhaseAnswer::NotNormal(Pressure::Warn)
        );
        assert!(parse_pressure_phase("@@before\n1\n1\n").is_err(), "no @@end");
    }

    /// The recorded M3 Ultra settle (2026-09-24, 2 s cadence after the release, GiB).
    const RECORDED: [f64; 12] = [
        78.90, 78.90, 79.23, 78.91, 79.22, 79.22, 79.21, 79.22, 79.21, 79.21, 79.21, 79.21,
    ];

    #[test]
    fn the_recorded_settle_ends_once_available_stopped_rising() {
        let total = 96 * GIB;
        let bytes: Vec<u64> = RECORDED.iter().map(|g| (g * GIB as f64) as u64).collect();
        let end = (1..=bytes.len())
            .find(|n| settled(&bytes[..*n], total))
            .unwrap();
        // 78.90, 78.90, 79.23 (the high), then three samples at or under it.
        assert_eq!(end, 6);
        assert!((bytes[end - 1] as f64 / GIB as f64 - 79.22).abs() < 0.005);
        // A series still climbing never settles.
        let climbing: Vec<u64> = (0..10).map(|i| 70 * GIB + i * GIB).collect();
        assert!(!settled(&climbing, total));
        // Jitter below 0.1% of RAM is not a rise.
        let flat: Vec<u64> = [0u64, 50, 20, 90]
            .iter()
            .map(|mib| 79 * GIB + mib * 1024 * 1024)
            .collect();
        assert!(settled(&flat, total));
    }

    #[test]
    fn an_engine_on_the_node_refuses_by_name() {
        let ps = "  101 /usr/bin/python3 -m mlx_lm.server --model x\n  202 /bin/zsh\n  303 /x/bin/python /x/bin/rapid-mlx serve /m --port 8093\n  404 /usr/bin/memory_pressure -l warn\n";
        let engines = engines_in(ps);
        assert_eq!(
            engines.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
            [101, 303]
        );
        let refusal = engine_refusal("MacBook", &engines).unwrap();
        assert_eq!(refusal.code, "engineLoaded");
        assert!(refusal.message.contains("pid 303"), "{}", refusal.message);
        assert!(engine_refusal("MacBook", &engines_in("  202 /bin/zsh\n")).is_none());
    }

    #[tokio::test]
    async fn the_script_parses_its_own_refusal_path_on_this_mac() {
        // Runs only the NORMAL check's refusal branch: a level forced to "2" by rewriting the
        // sysctl read — never the ballast.
        let script = compaction_script().replacen(
            "level=$(/usr/sbin/sysctl -n kern.memorystatus_vm_pressure_level)",
            "level=2",
            1,
        );
        let out = super::super::SystemExec.run(None, &script).await.unwrap();
        assert!(out.success(), "{}", out.stderr);
        assert_eq!(
            parse_pressure_phase(&out.stdout).unwrap(),
            PhaseAnswer::NotNormal(Pressure::Warn)
        );
    }
}
