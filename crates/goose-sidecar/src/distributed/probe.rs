//! Parsers for what a node answers. Every parser is strict: an answer that does not have the
//! expected shape is an error naming what was missing, never a zero.

use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::memory::{darwin_reading, VmPageCounts};
use crate::MemoryReading;

/// `kern.memorystatus_vm_pressure_level` as xnu publishes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    Normal,
    Warn,
    Critical,
}

impl Pressure {
    pub fn from_level(level: u64) -> Result<Self> {
        match level {
            1 => Ok(Pressure::Normal),
            2 => Ok(Pressure::Warn),
            4 => Ok(Pressure::Critical),
            other => bail!("kern.memorystatus_vm_pressure_level {other} is not 1, 2 or 4"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Pressure::Normal => "normal",
            Pressure::Warn => "warn",
            Pressure::Critical => "critical",
        }
    }
}

/// Split a multi-section answer. Each section starts with a line `@@<name>`.
pub fn sections(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(name) = line.strip_prefix("@@") {
            if let Some((name, body)) = current.take() {
                out.insert(name, body);
            }
            current = Some((name.trim().to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((name, body)) = current {
        out.insert(name, body);
    }
    out
}

pub fn section<'a>(sections: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    sections
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("the node's answer has no `{name}` section"))
}

/// `vm_stat` → the same page counts `memory::measure` reads from `host_statistics64`, fed to the
/// SAME `darwin_reading`. vm_stat prints "Pages free" as `free_count - speculative_count`, so the
/// raw `free_count` is rebuilt by adding speculative back before the shared arithmetic subtracts it.
pub fn parse_vm_stat(text: &str, total_bytes: u64) -> Result<MemoryReading> {
    let first = text.lines().next().context("vm_stat printed nothing")?;
    let page_size: u64 = first
        .split("page size of ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .with_context(|| format!("vm_stat header has no page size: {first}"))?;
    let count = |label: &str| -> Result<u64> {
        let line = text
            .lines()
            .find(|l| l.trim_start().starts_with(label))
            .with_context(|| format!("vm_stat has no `{label}` line"))?;
        let value = line
            .rsplit(':')
            .next()
            .map(|v| v.trim().trim_end_matches('.'))
            .unwrap_or_default();
        value
            .parse::<u64>()
            .with_context(|| format!("vm_stat `{label}` is not a count: {line}"))
    };
    let free_minus_speculative = count("Pages free")?;
    let speculative = count("Pages speculative")?;
    let counts = VmPageCounts {
        free: free_minus_speculative + speculative,
        speculative,
        external: count("File-backed pages")?,
        purgeable: count("Pages purgeable")?,
    };
    Ok(darwin_reading(counts, page_size, total_bytes))
}

/// The lines `sysctl -n` printed, as integers, in the order the names were asked.
pub fn parse_sysctl_values(text: &str, expected: usize) -> Result<Vec<u64>> {
    let values: Vec<u64> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.parse::<u64>()
                .with_context(|| format!("sysctl value is not an integer: {l}"))
        })
        .collect::<Result<_>>()?;
    ensure!(
        values.len() == expected,
        "sysctl printed {} values, {expected} were asked for",
        values.len()
    );
    Ok(values)
}

/// The `@@gpu` answer: Metal's `max_recommended_working_set_size` in bytes (its last line).
pub fn parse_gpu_ceiling(text: &str) -> Result<u64> {
    let line = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .context("the GPU probe printed nothing")?;
    let bytes: u64 = line
        .parse()
        .with_context(|| format!("the GPU probe did not print a byte count: {line}"))?;
    ensure!(bytes > 0, "the GPU probe reported a ceiling of 0 bytes");
    Ok(bytes)
}

/// One app's resident memory: every process inside the same outermost `.app` bundle, summed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMemory {
    pub name: String,
    pub rss_bytes: u64,
}

/// `ps -axo rss=,comm=` (RSS in KiB, then the executable path) → the `limit` biggest APPS the
/// owner could close, largest first. Only processes inside an `.app` bundle count (a daemon or a
/// CLI is not something to close); helpers are folded into their outermost app ("Google
/// Chrome.app/…/Google Chrome Helper (Renderer).app" is Google Chrome). goose's own app is left
/// out — it is the one doing this work.
pub fn top_apps_by_rss(text: &str, limit: usize) -> Vec<AppMemory> {
    let mut apps: BTreeMap<String, u64> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((rss, path)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(kib) = rss.parse::<u64>() else {
            continue;
        };
        let Some(app) = path
            .split('/')
            .find_map(|component| component.strip_suffix(".app"))
        else {
            continue;
        };
        if app.starts_with("Goose") {
            continue;
        }
        *apps.entry(app.to_string()).or_default() += kib * 1024;
    }
    let mut ranked: Vec<AppMemory> = apps
        .into_iter()
        .map(|(name, rss_bytes)| AppMemory { name, rss_bytes })
        .collect();
    ranked.sort_by(|a, b| b.rss_bytes.cmp(&a.rss_bytes).then(a.name.cmp(&b.name)));
    ranked.truncate(limit);
    ranked
}

/// `ps -o time=` (macOS: `M:SS.cc`, minutes growing past 60; also accepts `[D-]H:MM:SS.cc`) as
/// centiseconds of CPU time.
pub fn parse_cpu_time(text: &str) -> Result<u64> {
    let text = text.trim();
    let (days, rest) = match text.split_once('-') {
        Some((d, rest)) => (d.parse::<u64>().context("ps time days")?, rest),
        None => (0, text),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    ensure!(
        (2..=3).contains(&parts.len()),
        "ps time '{text}' is not M:SS.cc or H:MM:SS.cc"
    );
    let seconds_part = parts[parts.len() - 1];
    let (secs, centis) = match seconds_part.split_once('.') {
        Some((s, c)) => (s.parse::<u64>()?, c.parse::<u64>()?),
        None => (seconds_part.parse::<u64>()?, 0),
    };
    let minutes = parts[parts.len() - 2].parse::<u64>()?;
    let hours = if parts.len() == 3 {
        parts[0].parse::<u64>()?
    } else {
        0
    };
    Ok((((days * 24 + hours) * 60 + minutes) * 60 + secs) * 100 + centis)
}

/// One rank process as `ps -o pid=,stat=,time= -p PID` shows it; `None` when ps printed no row
/// (the pid is gone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSample {
    pub pid: u32,
    pub stat: String,
    pub cpu_centis: u64,
}

impl ProcSample {
    /// `T` = stopped (SIGSTOP / a debugger): the soak's FROZEN rank.
    pub fn stopped(&self) -> bool {
        self.stat.contains('T')
    }

    /// `Z` = exited, not yet reaped.
    pub fn zombie(&self) -> bool {
        self.stat.contains('Z')
    }
}

/// A process's GPU time in ns: the sum of every `accumulatedGPUTime` in the `AppUsage` lines
/// `sample_script`'s `@@gputime` section printed for its Metal clients (none yet = 0).
pub fn parse_gpu_ns(text: &str) -> Result<u64> {
    text.split("\"accumulatedGPUTime\"=")
        .skip(1)
        .map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits
                .parse::<u64>()
                .with_context(|| format!("unreadable accumulatedGPUTime in: {text}"))
        })
        .sum()
}

pub fn parse_ps_row(text: &str) -> Result<Option<ProcSample>> {
    let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return Ok(None);
    };
    let mut fields = line.split_whitespace();
    let pid = fields
        .next()
        .and_then(|p| p.parse().ok())
        .with_context(|| format!("ps row has no pid: {line}"))?;
    let stat = fields
        .next()
        .with_context(|| format!("ps row has no stat: {line}"))?
        .to_string();
    let cpu_centis = parse_cpu_time(
        fields
            .next()
            .with_context(|| format!("ps row has no time: {line}"))?,
    )?;
    Ok(Some(ProcSample {
        pid,
        stat,
        cpu_centis,
    }))
}

/// What a foreign MLX process is. A SINGLE server (`mlx_lm.server`, `rapid-mlx serve`) is one
/// model behind one HTTP port — an independent engine whose resident memory the node's measured
/// `available` already excludes. A DISTRIBUTED process (`mlx.launch`, the fork's
/// `pipeline_qwen4 serve|run`, a goose rank) joins a group: it holds a coordinator port or an RDMA
/// queue pair and would collide with this launch. The fork's `plan` dry run joins nothing and is
/// neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForeignKind {
    SingleServer,
    Distributed,
}

/// `ps -axo pid=,command=` rows that ARE a distributed or MLX serving process: a python
/// interpreter (argv[0]) whose arguments name one. Only the interpreter counts — a shell, a
/// `pgrep` or an ssh client whose command line merely MENTIONS `mlx.launch` (a watcher loop, the
/// ssh session carrying a peer's rank) is not an engine (STEP1b trap: `pgrep -f` matched the
/// harness's own shell). `own` pids are left out.
pub fn foreign_engine_processes(text: &str, own: &[u32]) -> Vec<(u32, String)> {
    classify_foreign_engines(text, own)
        .into_iter()
        .map(|(pid, command, _)| (pid, command))
        .collect()
}

/// A `ps` row's pid and command line (whole — a rank's marker sits after tens of KB of base64).
fn ps_rows(text: &str) -> impl Iterator<Item = (u32, &str)> {
    text.lines().filter_map(|line| {
        let (pid, command) = line.trim().split_once(char::is_whitespace)?;
        Some((pid.parse().ok()?, command.trim()))
    })
}

/// argv[0]'s file name, lowercased.
fn argv0_name(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .next()
        .and_then(|argv0| argv0.rsplit('/').next())
        .map(str::to_ascii_lowercase)
}

/// What a row shows, cut for display (the full line is a program's base64).
fn shown(command: &str) -> String {
    command.chars().take(160).collect()
}

/// Whether a row naming the fork's pipeline joins a distributed group. Of
/// `pipeline_qwen4 {plan,serve,run}` only `serve` and `run` call `mx.distributed.init` (fork
/// `_cmd_run` / `pipeline_qwen4_serve.serve`); `plan` is the dry run "from index/headers only"
/// goose itself runs on every pick and every preflight, and `--help` is the preflight's runner
/// probe — neither opens a coordinator socket or a queue pair (Q-126: two of goose's own
/// `plan --json` probes, gone seconds later, refused its own Run as a foreign split). A row that
/// names the pipeline some other way (`pipeline_qwen4_serve`, a wrapper) is not provably a
/// planner and stays a group member.
fn pipeline_joins_group(command: &str) -> bool {
    const GROUP_SUBCOMMANDS: [&str; 2] = ["serve", "run"];
    let args: Vec<&str> = command
        .split_whitespace()
        .map(|arg| arg.trim_matches(|c| c == '\'' || c == '"'))
        .collect();
    let program = args.iter().position(|arg| {
        *arg == "rapid_mlx.distributed.pipeline_qwen4"
            || arg.rsplit('/').next() == Some("pipeline_qwen4.py")
    });
    match program {
        Some(at) => args
            .get(at + 1)
            .is_some_and(|sub| GROUP_SUBCOMMANDS.contains(sub)),
        None => true,
    }
}

/// [`foreign_engine_processes`] with each row's [`ForeignKind`].
pub fn classify_foreign_engines(text: &str, own: &[u32]) -> Vec<(u32, String, ForeignKind)> {
    const GROUP_PROGRAMS: [&str; 2] = ["mlx.launch", super::launch::RANK_MARKER];
    const SINGLE: [&str; 3] = ["mlx_lm.server", "mlx_lm/server", "rapid-mlx serve"];
    ps_rows(text)
        .filter_map(|(pid, command)| {
            let interpreter = argv0_name(command).is_some_and(|name| name.starts_with("python"));
            if !interpreter || own.contains(&pid) {
                return None;
            }
            let joins_group = GROUP_PROGRAMS.iter().any(|m| command.contains(m))
                || (command.contains("pipeline_qwen4") && pipeline_joins_group(command));
            let kind = if joins_group {
                ForeignKind::Distributed
            } else if SINGLE.iter().any(|m| command.contains(m)) {
                ForeignKind::SingleServer
            } else {
                return None;
            };
            Some((pid, shown(command), kind))
        })
        .collect()
}

/// A goose rank as a node's `ps` shows it: the interpreter running one, or the ssh client on the
/// requester that carries one to a peer (that client's session is what keeps the peer's rank
/// alive, so a carrier a dead goose left is part of its leftover).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GooseRankProcess {
    pub pid: u32,
    /// Its parent, from the node's `pid ppid` listing; `None` = the node did not answer one (a
    /// peer on an older goose). 1 = the goose that launched it is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ppid: Option<u32>,
    /// The owner token on its command line (`launch::OWNER_ARG_PREFIX`); `None` = none written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default)]
    pub carrier: bool,
    /// The command line, cut for display.
    pub command: String,
}

impl GooseRankProcess {
    /// Its launcher is gone: nothing will ever stop it but a reclaim.
    pub fn orphaned(&self) -> bool {
        self.ppid == Some(1)
    }

    /// One line: what it is, what will end it, its command line.
    pub fn describe(&self) -> String {
        let what = if self.carrier {
            "the ssh session carrying a peer's rank"
        } else {
            "rank"
        };
        let state = match self.ppid {
            Some(1) => "orphaned: the goose that launched it is gone".to_string(),
            Some(parent) => format!("its parent pid {parent} still runs: shutting down"),
            None => "this node's goose reports no parent for it".to_string(),
        };
        format!("{what} pid {} ({state}) `{}`", self.pid, self.command)
    }
}

/// `ps -axo pid=,ppid=` as pid → parent.
pub fn parse_parents(text: &str) -> BTreeMap<u32, u32> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect()
}

/// Every goose rank on a node's `ps -axo pid=,command=` — judged on the WHOLE command line (the
/// marker and the owner token follow the program's base64). A rank's argv[0] is its interpreter;
/// a carrier's is `ssh` with the remote script (the rank's quoted argv) as its argument. A shell
/// or a `pgrep` that merely mentions the marker is neither. `own` pids are left out.
pub fn goose_rank_processes(
    text: &str,
    parents: Option<&BTreeMap<u32, u32>>,
    own: &[u32],
) -> Vec<GooseRankProcess> {
    ps_rows(text)
        .filter(|(pid, command)| !own.contains(pid) && command.contains(super::launch::RANK_MARKER))
        .filter_map(|(pid, command)| {
            let name = argv0_name(command)?;
            let carrier = match name.as_str() {
                "ssh" => true,
                n if n.starts_with("python") => false,
                _ => return None,
            };
            let owner = command
                .split_whitespace()
                .map(|arg| arg.trim_matches(|c| c == '\'' || c == '"'))
                .find_map(|arg| arg.strip_prefix(super::launch::OWNER_ARG_PREFIX))
                .map(str::to_string);
            Some(GooseRankProcess {
                pid,
                ppid: parents.and_then(|p| p.get(&pid).copied()),
                owner,
                carrier,
                command: shown(command),
            })
        })
        .collect()
}

/// Every `inet A.B.C.D` on an `ifconfig <iface>` answer.
pub fn parse_ifconfig_ipv4(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("inet "))
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// `ibv_devinfo -v -d <dev>`'s GID table as (index, gid).
pub fn parse_gid_table(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("GID[")?;
            let (index, rest) = rest.split_once(']')?;
            let gid = rest.trim_start_matches(':').trim();
            Some((index.trim().parse().ok()?, gid.to_string()))
        })
        .collect()
}

/// The IPv4-mapped GID JACCL reads for `ip`.
pub fn ipv4_gid(ip: &str) -> String {
    format!("::ffff:{ip}")
}

/// `networksetup -listnetworkserviceorder` as service name → (hardware port, device, enabled).
pub fn parse_service_order(text: &str) -> BTreeMap<String, (String, String, bool)> {
    let mut out = BTreeMap::new();
    let mut pending: Option<(String, bool)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("(Hardware Port: ") {
            if let Some((name, enabled)) = pending.take() {
                let rest = rest.trim_end_matches(')');
                if let Some((port, device)) = rest.split_once(", Device: ") {
                    out.insert(name, (port.to_string(), device.to_string(), enabled));
                }
            }
        } else if let Some(rest) = line.strip_prefix('(') {
            if let Some((marker, name)) = rest.split_once(") ") {
                pending = Some((name.to_string(), marker != "*"));
            }
        }
    }
    out
}

/// `networksetup -listallhardwareports`: the hardware port name that owns `device`.
pub fn hardware_port_for_device(text: &str, device: &str) -> Option<String> {
    let mut port: Option<&str> = None;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("Hardware Port: ") {
            port = Some(name);
        } else if line.strip_prefix("Device: ") == Some(device) {
            return port.map(str::to_string);
        }
    }
    None
}

/// The live speed of the receptacle behind hardware port "Thunderbolt N", from
/// `system_profiler SPThunderboltDataType -json` (receptacle N ↔ port "Thunderbolt N", measured on
/// both Macs 2026-09-23). `None` when the port is not a Thunderbolt one or nothing is connected.
pub fn thunderbolt_speed(profiler_json: &str, hardware_port: &str) -> Result<Option<String>> {
    let Some(receptacle) = hardware_port.strip_prefix("Thunderbolt ") else {
        return Ok(None);
    };
    let value: serde_json::Value =
        serde_json::from_str(profiler_json).context("system_profiler JSON")?;
    fn walk<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(tag) = map.get("receptacle_1_tag") {
                    out.push(tag);
                }
                map.values().for_each(|v| walk(v, out));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut tags = Vec::new();
    walk(&value, &mut tags);
    Ok(tags
        .into_iter()
        .find(|tag| tag.get("receptacle_id_key").and_then(|v| v.as_str()) == Some(receptacle))
        .filter(|tag| {
            tag.get("receptacle_status_key").and_then(|v| v.as_str())
                == Some("receptacle_connected")
        })
        .and_then(|tag| tag.get("current_speed_key").and_then(|v| v.as_str()))
        .map(str::to_string))
}

/// `stat -L -f '%z %N'` rows as basename → size.
pub fn parse_file_sizes(text: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (size, path) = l.trim().split_once(' ')?;
            let name = path.rsplit('/').next()?.to_string();
            Some((name, size.parse().ok()?))
        })
        .collect()
}

/// `shasum -a 256 <file>` rows as basename → digest.
pub fn parse_shasums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (digest, path) = l.trim().split_once(char::is_whitespace)?;
            let name = path.trim().rsplit('/').next()?.to_string();
            Some((name, digest.to_string()))
        })
        .collect()
}

/// A readiness/liveness stream's verdict. The soak's failure class (a): a rank that dies
/// mid-stream leaves an EOF with NO `data: [DONE]` under an HTTP 200 already sent — so a 200 is
/// not a completion, only the terminator is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseVerdict {
    Complete { chunks: usize },
    Truncated { chunks: usize },
}

pub fn sse_verdict(body: &str) -> SseVerdict {
    let mut chunks = 0;
    for line in body.lines().map(str::trim) {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        if data.trim() == "[DONE]" {
            return SseVerdict::Complete { chunks };
        }
        chunks += 1;
    }
    SseVerdict::Truncated { chunks }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded on this MacBook (M4 Max 128 GB) 2026-09-24 with `vm_stat`.
    const VM_STAT_MACBOOK: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                  3174400.
Pages active:                                1376185.
Pages inactive:                              2775337.
Pages speculative:                             37587.
Pages throttled:                                   0.
Pages wired down:                             315751.
Pages purgeable:                                9322.
\"Translation faults\":                     4063523096.
File-backed pages:                           3197112.
Anonymous pages:                              991997.
Pages stored in compressor:                  1475380.
Pages occupied by compressor:                 649325.
";

    #[test]
    fn vm_stat_is_read_with_the_single_engines_own_arithmetic() {
        let total = 137_438_953_472;
        let reading = parse_vm_stat(VM_STAT_MACBOOK, total).unwrap();
        let page = 16_384;
        assert_eq!(
            reading.available_bytes,
            (3_174_400 + 3_197_112 + 9_322) * page,
            "(free - speculative) + file-backed + purgeable"
        );
        assert_eq!(
            reading.reclaimable_cache_bytes,
            Some((3_197_112 + 9_322) * page)
        );
        let direct = darwin_reading(
            VmPageCounts {
                free: 3_174_400 + 37_587,
                speculative: 37_587,
                external: 3_197_112,
                purgeable: 9_322,
            },
            page,
            total,
        );
        assert_eq!(reading, direct);
    }

    /// The live parity: this Mac's vm_stat, parsed here, agrees with `memory::measure()`
    /// (host_statistics64) within what moves between the two reads.
    #[cfg(target_os = "macos")]
    #[test]
    fn vm_stat_agrees_with_host_statistics64_on_this_mac() {
        let out = std::process::Command::new("/usr/bin/vm_stat")
            .output()
            .unwrap();
        let measured = crate::measure().unwrap();
        let parsed =
            parse_vm_stat(&String::from_utf8_lossy(&out.stdout), measured.total_bytes).unwrap();
        let diff = parsed.available_bytes.abs_diff(measured.available_bytes);
        assert!(
            diff < crate::GIB,
            "vm_stat and host_statistics64 differ by {diff} bytes"
        );
    }

    #[test]
    fn a_vm_stat_without_file_backed_pages_is_an_error_not_a_zero() {
        let broken = VM_STAT_MACBOOK.replace("File-backed pages", "File pages");
        let err = parse_vm_stat(&broken, 1).unwrap_err().to_string();
        assert!(err.contains("File-backed pages"), "{err}");
    }

    #[test]
    fn pressure_levels_follow_xnu() {
        assert_eq!(Pressure::from_level(1).unwrap(), Pressure::Normal);
        assert_eq!(Pressure::from_level(2).unwrap(), Pressure::Warn);
        assert_eq!(Pressure::from_level(4).unwrap(), Pressure::Critical);
        assert!(Pressure::from_level(0).is_err());
    }

    #[test]
    fn cpu_time_parses_the_macos_minute_form_and_the_hour_form() {
        assert_eq!(parse_cpu_time("0:00.01").unwrap(), 1);
        assert_eq!(
            parse_cpu_time("10:52.48").unwrap(),
            (10 * 60 + 52) * 100 + 48
        );
        assert_eq!(
            parse_cpu_time("528:19.99").unwrap(),
            (528 * 60 + 19) * 100 + 99
        );
        assert_eq!(
            parse_cpu_time("1-02:03:04.05").unwrap(),
            (((24 + 2) * 60 + 3) * 60 + 4) * 100 + 5
        );
        assert!(parse_cpu_time("soon").is_err());
    }

    #[test]
    fn a_ps_row_names_a_stopped_rank() {
        let row = parse_ps_row("33675 Ts+    0:00.01\n").unwrap().unwrap();
        assert_eq!(row.pid, 33675);
        assert!(row.stopped());
        assert!(parse_ps_row("\n").unwrap().is_none());
    }

    #[test]
    fn foreign_engines_are_named_and_own_ranks_are_not() {
        let ps = "  101 /tmp/jaccl-smoke/.venv/bin/python /tmp/jaccl-smoke/.venv/bin/mlx_lm.server --model m
  102 /usr/bin/python3 -m http.server
  103 /Users/w/.cache/uv/x/bin/python /Users/w/.local/bin/rapid-mlx serve /m --port 8090
  104 /opt/py/bin/python3.12 -c import base64,sys;exec(base64.b64decode(sys.argv[1])) AAAA goose-distributed-rank
  105 /bin/zsh -c while pgrep -f 'mlx.launch|pipeline_qwen4'; do sleep 5; done
  106 /usr/bin/ssh -tt workhorse echo GOOSE_RANK_PID=$$; exec python -c x goose-distributed-rank
";
        let foreign = foreign_engine_processes(ps, &[104]);
        let pids: Vec<u32> = foreign.iter().map(|(p, _)| *p).collect();
        assert_eq!(pids, vec![101, 103]);
        let orphan = foreign_engine_processes(ps, &[]);
        assert!(orphan.iter().any(|(p, _)| *p == 104));
        let kinds: Vec<(u32, ForeignKind)> = classify_foreign_engines(ps, &[])
            .into_iter()
            .map(|(p, _, k)| (p, k))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (101, ForeignKind::SingleServer),
                (103, ForeignKind::SingleServer),
                (104, ForeignKind::Distributed)
            ]
        );
    }

    /// Q-126, measured on 3.0.48 at 11:41:50: goose's own `plan --json` probes (pids 52054 and
    /// 52057, exited seconds later) were named a foreign split and refused Run. The fork's `plan`
    /// and `--help` join no group; its `serve`/`run`, `mlx.launch`, and a row that names the
    /// pipeline some other way still do.
    #[test]
    fn the_forks_planner_joins_no_group_and_its_ranks_do() {
        let fork = "/Users/me/.goose/distributed/rapid-mlx-pipeline-qwen4-py3.12/bin/python";
        let ps = format!(
            "52054 {fork} -m rapid_mlx.distributed.pipeline_qwen4 plan --json --model /m/Qwen3.8-Flash-Next-4bit --node Mihai-Macbook:128.00:90.00:100.00 --batch 1
52057 {fork} -m rapid_mlx.distributed.pipeline_qwen4 plan --json --model /m/Qwen3.8-Flash-Next-4bit --node a:1:1:1 --context 65536
  401 {fork} -m rapid_mlx.distributed.pipeline_qwen4 --help
  402 {fork} -m rapid_mlx.distributed.pipeline_qwen4 serve --model /m --port 8090
  403 {fork} /x/rapid_mlx/distributed/pipeline_qwen4.py run --model /m --prompt hi --max-tokens 4
  404 {fork} '/x/pipeline_qwen4.py' plan --json --model /m
  405 {fork} -m rapid_mlx.distributed.pipeline_qwen4_serve --model /m
  406 {fork} /x/bin/mlx.launch --hosts a,b -- python -m rapid_mlx.distributed.pipeline_qwen4 serve
"
        );
        let kinds: Vec<(u32, ForeignKind)> = classify_foreign_engines(&ps, &[])
            .into_iter()
            .map(|(p, _, k)| (p, k))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (402, ForeignKind::Distributed),
                (403, ForeignKind::Distributed),
                (405, ForeignKind::Distributed),
                (406, ForeignKind::Distributed),
            ]
        );
    }

    /// Q-77's shapes. A real rank's marker and owner token sit after the program's base64, far
    /// past the 160 characters a displayed row keeps — the whole line is what is judged. pid 9425
    /// was `/usr/bin/python3` (ps names it by Xcode's Python.app, measured) running the boot line
    /// with the marker and NO owner token: a goose rank, but never provably this install's.
    #[test]
    fn goose_ranks_are_read_whole_with_their_owner_carrier_and_parent() {
        let b64 = "QUFB".repeat(100);
        let marker = super::super::launch::RANK_MARKER;
        let owner = format!("{}0123abcd", super::super::launch::OWNER_ARG_PREFIX);
        let ps = format!(
            "  201 /Users/me/.goose/distributed/mlx-py3.12/bin/python -c import base64,sys;exec(base64.b64decode(sys.argv[1])) {b64} {b64} {marker} {owner}
 9425 /Applications/Xcode.app/Contents/Developer/Library/Frameworks/Python3.framework/Versions/3.9/Resources/Python.app/Contents/MacOS/Python -c import base64,sys;exec(base64.b64decode(sys.argv[1])) {b64} {b64} {marker}
  203 /usr/bin/ssh -tt -o BatchMode=yes studio echo GOOSE_RANK_PID=$$; exec '/x/python' '-c' '{b64}' '{marker}' '{owner}'
  204 /bin/zsh -c pgrep -f {marker}
  205 /usr/bin/python3 -m http.server
"
        );
        let parents = parse_parents("  201 1\n 9425 88\n  203 4242\n");
        let ranks = goose_rank_processes(&ps, Some(&parents), &[]);
        let rows: Vec<(u32, Option<u32>, Option<&str>, bool)> = ranks
            .iter()
            .map(|r| (r.pid, r.ppid, r.owner.as_deref(), r.carrier))
            .collect();
        assert_eq!(
            rows,
            vec![
                (201, Some(1), Some("0123abcd"), false),
                (9425, Some(88), None, false),
                (203, Some(4242), Some("0123abcd"), true),
            ]
        );
        assert!(ranks[0].orphaned() && !ranks[1].orphaned());
        assert!(ranks.iter().all(|r| r.command.chars().count() <= 160));
        assert!(ranks[0].describe().contains("pid 201 (orphaned"));
        assert!(ranks[2]
            .describe()
            .starts_with("the ssh session carrying a peer's rank pid 203 (its parent pid 4242"));

        let unanswered = goose_rank_processes(&ps, None, &[201]);
        assert_eq!(unanswered.len(), 2, "own pids are left out");
        assert!(unanswered.iter().all(|r| r.ppid.is_none() && !r.orphaned()));
        assert!(unanswered[0].describe().contains("reports no parent"));
    }

    #[test]
    fn the_gid_table_and_the_rtr_errno_96_shape_are_read() {
        let healthy = "\t\t\tgid_tbl_len:\t\t1024\n\t\t\tGID[  0]:\t\tfe80::34e4:49ff:fec8:e348\n\t\t\tGID[  1]:\t\t::ffff:192.168.0.1\n";
        assert_eq!(
            parse_gid_table(healthy),
            vec![
                (0, "fe80::34e4:49ff:fec8:e348".to_string()),
                (1, ipv4_gid("192.168.0.1"))
            ]
        );
        // The soak's post-restore table: IPv4 back, but at index 2 — JACCL's RTR fails errno 96.
        let shifted = "GID[  0]:\t\tfe80::1\nGID[  2]:\t\t::ffff:192.168.0.2\n";
        let table = parse_gid_table(shifted);
        assert!(!table
            .iter()
            .any(|(i, g)| *i == 1 && *g == ipv4_gid("192.168.0.2")));
    }

    #[test]
    fn service_order_maps_services_to_their_hardware_port() {
        let text = "An asterisk (*) denotes that a network service is disabled.
(1) Wi-Fi
(Hardware Port: Wi-Fi, Device: en0)

(*) EXO Thunderbolt 1
(Hardware Port: Thunderbolt 1, Device: en1)

(2) EXO Thunderbolt 3
(Hardware Port: Thunderbolt 3, Device: en3)
";
        let services = parse_service_order(text);
        assert_eq!(
            services["EXO Thunderbolt 3"],
            ("Thunderbolt 3".to_string(), "en3".to_string(), true)
        );
        assert!(!services["EXO Thunderbolt 1"].2);
        assert_eq!(services["Wi-Fi"].0, "Wi-Fi");
    }

    #[test]
    fn thunderbolt_speed_follows_the_receptacle_of_the_hardware_port() {
        let json = r#"{"SPThunderboltDataType":[
            {"_name":"bus_2","receptacle_1_tag":{"current_speed_key":"80 Gb/s","receptacle_id_key":"3","receptacle_status_key":"receptacle_connected"}},
            {"_name":"bus_1","receptacle_1_tag":{"current_speed_key":"Up to 120 Gb/s","receptacle_id_key":"2","receptacle_status_key":"receptacle_no_devices_connected"}}]}"#;
        assert_eq!(
            thunderbolt_speed(json, "Thunderbolt 3").unwrap().as_deref(),
            Some("80 Gb/s")
        );
        assert_eq!(thunderbolt_speed(json, "Thunderbolt 2").unwrap(), None);
        assert_eq!(thunderbolt_speed(json, "Wi-Fi").unwrap(), None);
        let ports =
            "Hardware Port: Wi-Fi\nDevice: en0\n\nHardware Port: Thunderbolt 3\nDevice: en3\n";
        assert_eq!(
            hardware_port_for_device(ports, "en3").as_deref(),
            Some("Thunderbolt 3")
        );
    }

    #[test]
    fn the_no_done_rule() {
        let complete =
            "data: {\"choices\":[{\"delta\":{\"content\":\"pong\"}}]}\n\ndata: [DONE]\n\n";
        assert_eq!(sse_verdict(complete), SseVerdict::Complete { chunks: 1 });
        // The soak's rank-death shape: 54 chunks under a 200, then EOF — no terminator.
        let truncated: String = (0..54)
            .map(|i| format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"t{i}\"}}}}]}}\n\n"))
            .collect();
        assert_eq!(
            sse_verdict(&truncated),
            SseVerdict::Truncated { chunks: 54 }
        );
        assert_eq!(sse_verdict(""), SseVerdict::Truncated { chunks: 0 });
    }

    /// The AppUsage lines the sample printed for the 27B's rank 1 on the Studio (2026-09-26,
    /// mid-decode) and a client that has not submitted yet: every entry summed, an empty one is 0.
    #[test]
    fn gpu_time_sums_every_metal_client_of_the_rank() {
        let text = "      \"AppUsage\" = ({\"API\"=\"Metal\",\"lastSubmittedTime\"=0,\"accumulatedGPUTime\"=0},{\"API\"=\"Metal\",\"lastSubmittedTime\"=294351842493625,\"accumulatedGPUTime\"=79033999958})\n      \"AppUsage\" = ({\"API\"=\"Metal\",\"lastSubmittedTime\"=1,\"accumulatedGPUTime\"=42})\n      \"AppUsage\" = ()\n";
        assert_eq!(parse_gpu_ns(text).unwrap(), 79_033_999_958 + 42);
        assert_eq!(parse_gpu_ns("").unwrap(), 0);
    }

    #[test]
    fn sections_split_on_markers_and_tolerate_carriage_returns() {
        let text = "@@a\r\none\r\n@@b\ntwo\nthree\n";
        let s = sections(text);
        assert_eq!(s["a"], "one\n");
        assert_eq!(s["b"], "two\nthree\n");
        assert!(section(&s, "c").is_err());
    }

    #[test]
    fn the_biggest_apps_fold_helpers_and_skip_daemons_and_goose() {
        let ps = "  900000 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
  600000 /Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)
 1200000 /Applications/Slack.app/Contents/MacOS/Slack
 5000000 /usr/libexec/some-daemon
 4000000 /Applications/Goose Swarm.app/Contents/MacOS/Goose Swarm
  100000 /System/Applications/Mail.app/Contents/MacOS/Mail
   50000 /Applications/Notes.app/Contents/MacOS/Notes
";
        let top = top_apps_by_rss(ps, 3);
        assert_eq!(
            top,
            vec![
                AppMemory {
                    name: "Google Chrome".into(),
                    rss_bytes: 1_500_000 * 1024
                },
                AppMemory {
                    name: "Slack".into(),
                    rss_bytes: 1_200_000 * 1024
                },
                AppMemory {
                    name: "Mail".into(),
                    rss_bytes: 100_000 * 1024
                },
            ]
        );
    }

    #[test]
    fn the_gpu_ceiling_is_a_positive_byte_count_or_a_named_error() {
        assert_eq!(parse_gpu_ceiling("83494174720\n").unwrap(), 83_494_174_720);
        assert!(parse_gpu_ceiling("ModuleNotFoundError: No module named 'mlx'\n").is_err());
        assert!(parse_gpu_ceiling("").is_err());
        assert!(parse_gpu_ceiling("0").is_err());
    }
}
