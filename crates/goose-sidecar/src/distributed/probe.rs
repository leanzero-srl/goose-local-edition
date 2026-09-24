//! Parsers for what a node answers. Every parser is strict: an answer that does not have the
//! expected shape is an error naming what was missing, never a zero.

use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};

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
/// `pipeline_qwen4`, a goose rank) joins a group: it holds a coordinator port or an RDMA queue
/// pair and would collide with this launch.
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

/// [`foreign_engine_processes`] with each row's [`ForeignKind`].
pub fn classify_foreign_engines(text: &str, own: &[u32]) -> Vec<(u32, String, ForeignKind)> {
    const DISTRIBUTED: [&str; 3] = ["mlx.launch", "pipeline_qwen4", super::launch::RANK_MARKER];
    const SINGLE: [&str; 3] = ["mlx_lm.server", "mlx_lm/server", "rapid-mlx serve"];
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (pid, command) = line.split_once(char::is_whitespace)?;
            let pid: u32 = pid.parse().ok()?;
            let command = command.trim();
            let interpreter = command
                .split_whitespace()
                .next()
                .and_then(|argv0| argv0.rsplit('/').next())
                .is_some_and(|name| name.to_ascii_lowercase().starts_with("python"));
            if !interpreter || own.contains(&pid) {
                return None;
            }
            let kind = if DISTRIBUTED.iter().any(|m| command.contains(m)) {
                ForeignKind::Distributed
            } else if SINGLE.iter().any(|m| command.contains(m)) {
                ForeignKind::SingleServer
            } else {
                return None;
            };
            Some((pid, command.chars().take(160).collect(), kind))
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

    #[test]
    fn sections_split_on_markers_and_tolerate_carriage_returns() {
        let text = "@@a\r\none\r\n@@b\ntwo\nthree\n";
        let s = sections(text);
        assert_eq!(s["a"], "one\n");
        assert_eq!(s["b"], "two\nthree\n");
        assert!(section(&s, "c").is_err());
    }
}
