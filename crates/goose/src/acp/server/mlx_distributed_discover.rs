//! `distributedDiscover`: the distributed engine SETS ITSELF UP. This Mac and every peer answer ONE
//! read-only script (a peer over ssh, the same `/bin/sh` script, one round trip), and the answers
//! become a filled config in which every value carries the evidence it came from and every value
//! that could not be found is a NAMED gap — the field stays empty, nothing is guessed.
//!
//! What comes from where:
//! - node name — `scutil --get ComputerName` (and `hostname -s` beside it);
//! - the link — LeanZero Link's path detector (`netpath`): every node's addresses from `ifconfig`,
//!   hardware ports from `networksetup -listallhardwareports`, the TB speed from
//!   `system_profiler`, then `choose_path` (a Thunderbolt subnet shared by BOTH ends first); the
//!   netmask from the prefix, the network service from `-listnetworkserviceorder`;
//! - RDMA — `ibv_devinfo -v` (the `rdma_<iface>` device, its port state and its GID table);
//! - backend — JACCL when every node has an ACTIVE RDMA device on a Thunderbolt link, else ring,
//!   with the reason;
//! - models — every directory with a `config.json` under the goose models dir, `~/<dir>/models`
//!   and the HF cache, on every node; a model is on a peer when a directory of the same name holds
//!   the same loaded files at the same sizes (and SHA256SUMS, when rank 0 ships one, agrees);
//! - ports — the first ports free (`lsof`) above the single engine's own, the coordinator's below
//!   every node's ephemeral range;
//! - Python — the goose-managed env (`provision`), probed for the pinned versions, and the uv
//!   that builds it (the login shell's PATH first: the workhorse's Homebrew is in `.zprofile`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::Ipv4Addr;
use std::sync::Arc;

use goose_sdk_types::custom_requests::{
    MlxDistributedConfigDto, MlxDistributedDiscoveredEnvDto, MlxDistributedDiscoveredModelDto,
    MlxDistributedDiscoveredModelNodeDto, MlxDistributedDiscoveredNodeDto,
    MlxDistributedDiscoveryDto, MlxDistributedEvidenceDto, MlxDistributedGapDto,
    MlxDistributedNodeConfigDto,
};
use goose_sidecar::distributed::exec::{sh_quote, NodeExec};
use goose_sidecar::distributed::preflight::loaded_files;
use goose_sidecar::distributed::probe::{self, Pressure};
use goose_sidecar::distributed::provision::{self, EnvSpec};
use goose_sidecar::distributed::Runner;
use goose_sidecar::{MemoryReading, GIB};
use leanzero_link::netpath::{self, InterfaceFact, LinkKind, LinkPath};

/// Folders under `$HOME` never listed: macOS guards them (TCC), and a goosed reading one would
/// raise a privacy prompt for a model search. Models live elsewhere.
const PROTECTED_HOME_DIRS: [&str; 10] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Pictures",
    "Movies",
    "Music",
    "Public",
    "Applications",
    ".Trash",
];

/// The one script every node answers. `extra_roots` are model roots named by THIS goose (the
/// single engine's configured models dir, the persisted config's model dirs' parents).
pub(super) fn discover_script(extra_roots: &[String]) -> String {
    let env_probe = |section: &str, spec: &EnvSpec| {
        format!(
            "echo; echo @@{section}; P=\"$HOME\"/{}/{}/bin/python; if [ -x \"$P\" ]; then echo \"present $P\"; \"$P\" -c {} 2>&1 | /usr/bin/tail -1; else echo \"absent $P\"; fi",
            provision::ENVS_DIR,
            sh_quote(&spec.name),
            sh_quote(&spec.check)
        )
    };
    let protected: Vec<String> = PROTECTED_HOME_DIRS
        .iter()
        .map(|d| format!("\"$HOME/{d}\""))
        .collect();
    let extra: Vec<String> = extra_roots.iter().map(|r| sh_quote(r)).collect();
    let lines = [
        "echo; echo @@home; printf '%s\\n' \"$HOME\"".to_string(),
        "echo; echo @@names; /usr/sbin/scutil --get ComputerName 2>&1; /bin/hostname -s".to_string(),
        "echo; echo @@sysctl; /usr/sbin/sysctl -n hw.memsize kern.memorystatus_vm_pressure_level net.inet.ip.portrange.first".to_string(),
        "echo; echo @@vm; /usr/bin/vm_stat".to_string(),
        "echo; echo @@ifconfig; /sbin/ifconfig".to_string(),
        "echo; echo @@hwports; /usr/sbin/networksetup -listallhardwareports".to_string(),
        "echo; echo @@services; /usr/sbin/networksetup -listnetworkserviceorder".to_string(),
        "echo; echo @@tb; /usr/sbin/system_profiler SPThunderboltDataType -json 2>/dev/null".to_string(),
        "echo; echo @@ibv; /usr/bin/ibv_devinfo -v 2>&1".to_string(),
        "echo; echo @@listen; /usr/sbin/lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null".to_string(),
        format!("echo; echo @@uv; {}", provision::uv_candidates_script()),
        env_probe("env", &EnvSpec::tensor()),
        env_probe("envpipeline", &EnvSpec::pipeline()),
        "echo; echo @@models".to_string(),
        "model() { d=\"${1%/}\"; [ -f \"$d/config.json\" ] || return 0; echo \"M $d\"; echo \"T $(/usr/bin/plutil -extract model_type raw -o - \"$d/config.json\" 2>&1)\"; for f in \"$d\"/*; do [ -f \"$f\" ] && /usr/bin/stat -L -f 'F %z %N' \"$f\"; done; for s in \"$d\"/SHA256SUMS \"$d\"/SHA256SUMS.txt; do [ -f \"$s\" ] && echo \"S $(/usr/bin/shasum -a 256 \"$s\")\"; done; return 0; }".to_string(),
        "root() { r=\"${1%/}\"; [ -d \"$r\" ] || return 0; echo \"R $r\"; for a in \"$r\"/*/; do model \"$a\"; for b in \"$a\"*/; do model \"$b\"; done; done; }".to_string(),
        format!("for r in {} \"$HOME/.goose/models\"; do root \"$r\"; done", extra.join(" ")),
        format!(
            "for base in \"$HOME\"/*/ \"$HOME\"/.[!.]*/; do case \"${{base%/}}\" in {}|\"$HOME/.goose\") continue;; esac; [ -d \"${{base}}models\" ] && root \"${{base}}models\"; done",
            protected.join("|")
        ),
        "for s in \"$HOME\"/.cache/huggingface/hub/models--*/snapshots/*/; do model \"$s\"; done".to_string(),
        "echo; echo @@end".to_string(),
    ];
    lines.join("\n") + "\n"
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RdmaDevice {
    pub name: String,
    pub active: bool,
    pub gids: Vec<(u32, String)>,
}

/// `ibv_devinfo -v` (every device) as one entry per `hca_id`, its first port's state and GIDs.
pub(super) fn parse_rdma_devices(text: &str) -> Vec<RdmaDevice> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.trim().strip_prefix("hca_id:") {
            out.push((name.trim().to_string(), String::new()));
        } else if let Some((_, block)) = out.last_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    out.into_iter()
        .map(|(name, block)| RdmaDevice {
            active: block
                .lines()
                .find(|l| l.trim().starts_with("state:"))
                .is_some_and(|l| l.contains("PORT_ACTIVE")),
            gids: probe::parse_gid_table(&block),
            name,
        })
        .collect()
}

/// `lsof -nP -iTCP -sTCP:LISTEN` → the listening ports.
pub(super) fn parse_listening_ports(text: &str) -> BTreeSet<u16> {
    text.lines()
        .filter(|l| l.contains("(LISTEN)"))
        .filter_map(|l| {
            let name = l.split_whitespace().rev().nth(1)?;
            name.rsplit(':').next()?.parse().ok()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ModelDir {
    pub dir: String,
    pub model_type: String,
    pub files: BTreeMap<String, u64>,
    pub sums: BTreeMap<String, String>,
}

/// The `@@models` section: `R <root>` per root listed, then per model `M <dir>`, `T <model_type>`,
/// `F <size> <path>` per file and `S <digest> <path>` per manifest.
pub(super) fn parse_model_dirs(text: &str) -> (Vec<String>, Vec<ModelDir>) {
    let mut roots = Vec::new();
    let mut models: Vec<ModelDir> = Vec::new();
    for line in text.lines() {
        let Some((tag, rest)) = line.split_once(' ') else {
            continue;
        };
        match tag {
            "R" => roots.push(rest.to_string()),
            "M" => models.push(ModelDir {
                dir: rest.to_string(),
                model_type: String::new(),
                files: BTreeMap::new(),
                sums: BTreeMap::new(),
            }),
            "T" => {
                if let Some(m) = models.last_mut() {
                    m.model_type = rest.trim().to_string();
                }
            }
            "F" => {
                if let Some(m) = models.last_mut() {
                    m.files.extend(probe::parse_file_sizes(rest));
                }
            }
            "S" => {
                if let Some(m) = models.last_mut() {
                    m.sums.extend(probe::parse_shasums(rest));
                }
            }
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    models.retain(|m| seen.insert(m.dir.clone()));
    (roots, models)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum EnvProbe {
    Present { python: String, answer: String },
    Absent { python: String },
}

fn parse_env(text: &str) -> Option<EnvProbe> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let head = lines.next()?;
    if let Some(python) = head.strip_prefix("present ") {
        return Some(EnvProbe::Present {
            python: python.to_string(),
            answer: lines.next_back().unwrap_or_default().to_string(),
        });
    }
    head.strip_prefix("absent ").map(|python| EnvProbe::Absent {
        python: python.to_string(),
    })
}

/// Everything one node answered, parsed.
#[derive(Debug, Clone)]
pub(super) struct NodeProbe {
    pub home: String,
    pub computer_name: String,
    pub hostname: String,
    pub memory: MemoryReading,
    pub pressure: Pressure,
    pub ephemeral_first: u16,
    pub facts: Vec<InterfaceFact>,
    pub services: BTreeMap<String, (String, String, bool)>,
    pub rdma: Vec<RdmaDevice>,
    pub listening: BTreeSet<u16>,
    pub uv: Option<(String, String)>,
    pub env: Option<EnvProbe>,
    pub pipeline_env: Option<EnvProbe>,
    pub models: Vec<ModelDir>,
    pub roots: Vec<String>,
}

pub(super) fn parse_node(text: &str) -> Result<NodeProbe, String> {
    let sections = probe::sections(text);
    let get = |name: &str| probe::section(&sections, name).map_err(|e| format!("{e:#}"));
    get("end").map_err(|_| "the probe script did not finish".to_string())?;
    let home = get("home")?.trim().to_string();
    let mut names = get("names")?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty());
    let computer_name = names.next().unwrap_or_default().to_string();
    let hostname = names.next().unwrap_or_default().to_string();
    let sysctl = probe::parse_sysctl_values(get("sysctl")?, 3).map_err(|e| format!("{e:#}"))?;
    let memory = probe::parse_vm_stat(get("vm")?, sysctl[0]).map_err(|e| format!("{e:#}"))?;
    let pressure = Pressure::from_level(sysctl[1]).map_err(|e| format!("{e:#}"))?;
    let ephemeral_first = u16::try_from(sysctl[2])
        .map_err(|_| format!("net.inet.ip.portrange.first {} is not a port", sysctl[2]))?;
    let ports = netpath::parse_hardware_ports(get("hwports")?);
    let speeds = netpath::parse_thunderbolt_speeds(get("tb")?).unwrap_or_default();
    let facts = netpath::facts_from(
        &netpath::parse_ifconfig_addrs(get("ifconfig")?),
        Some(&ports),
        &speeds,
    );
    let uv = get("uv")?.lines().find_map(|l| {
        let (path, version) = l.split_once('|')?;
        Some((path.trim().to_string(), version.trim().to_string()))
    });
    let (roots, models) = parse_model_dirs(get("models")?);
    Ok(NodeProbe {
        home,
        computer_name,
        hostname,
        memory,
        pressure,
        ephemeral_first,
        facts,
        services: probe::parse_service_order(get("services")?),
        rdma: parse_rdma_devices(get("ibv")?),
        listening: parse_listening_ports(get("listen")?),
        uv,
        env: parse_env(get("env")?),
        pipeline_env: sections.get("envpipeline").and_then(|t| parse_env(t)),
        models,
        roots,
    })
}

/// What the probe could not know from the nodes alone.
pub(super) struct Context {
    /// The single engine's port on this Mac: the distributed API never takes it.
    pub single_port: u16,
    /// The single engine's models dir on this Mac (ids are relative to it).
    pub goose_models_dir: String,
    /// Prefer this model when it is on every node.
    pub preferred_model: Option<String>,
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

fn netmask(prefix_len: u8) -> String {
    let mask = match prefix_len {
        0 => 0,
        n => u32::MAX << (32 - u32::from(n.min(32))),
    };
    Ipv4Addr::from(mask).to_string()
}

fn model_id(dir: &str, goose_models_dir: &str) -> String {
    let root = format!("{}/", goose_models_dir.trim_end_matches('/'));
    if let Some(rel) = dir.strip_prefix(&root) {
        return rel.to_string();
    }
    let parts: Vec<&str> = dir.split('/').collect();
    if let Some(pos) = parts.iter().position(|p| *p == "snapshots") {
        if let Some(repo) = pos
            .checked_sub(1)
            .and_then(|i| parts[i].strip_prefix("models--"))
        {
            return repo.replacen("--", "/", 1);
        }
    }
    parts.last().copied().unwrap_or(dir).to_string()
}

fn model_key(id_or_dir: &str) -> &str {
    id_or_dir.rsplit('/').next().unwrap_or(id_or_dir)
}

/// A peer's directory for a model named `key`: the same loaded files at the same sizes. The
/// peer's goose models dir is preferred, then the order the node listed them.
fn peer_copies<'a>(peer: &'a NodeProbe, key: &str, hf_key: &str) -> Vec<&'a ModelDir> {
    let goose_root = format!("{}/.goose/models/", peer.home);
    let mut copies: Vec<&ModelDir> = peer
        .models
        .iter()
        .filter(|m| {
            let id = model_id(&m.dir, "");
            model_key(&id) == key || model_key(&id) == hf_key
        })
        .collect();
    copies.sort_by_key(|m| !m.dir.starts_with(&goose_root));
    copies
}

struct Composer {
    evidence: Vec<MlxDistributedEvidenceDto>,
    gaps: Vec<MlxDistributedGapDto>,
}

impl Composer {
    fn found(&mut self, node: Option<usize>, field: &str, value: &str, evidence: String) {
        self.evidence.push(MlxDistributedEvidenceDto {
            node: node.map(|n| n as u32),
            field: field.to_string(),
            value: value.to_string(),
            evidence,
        });
    }
    fn gap(&mut self, node: Option<usize>, field: &str, reason: String) {
        self.gaps.push(MlxDistributedGapDto {
            node: node.map(|n| n as u32),
            field: field.to_string(),
            reason,
        });
    }
}

/// Everything discovery concludes from the probes. `probes[0]` is this Mac; `hosts[i]` is node
/// i's ssh alias (`None` for this Mac).
pub(super) fn compose(
    hosts: &[Option<String>],
    probes: &[Result<NodeProbe, String>],
    context: &Context,
) -> MlxDistributedDiscoveryDto {
    let mut c = Composer {
        evidence: Vec::new(),
        gaps: Vec::new(),
    };
    let size = probes.len();
    let mut nodes: Vec<MlxDistributedNodeConfigDto> = hosts
        .iter()
        .map(|h| MlxDistributedNodeConfigDto {
            ssh: h.clone(),
            ..Default::default()
        })
        .collect();
    let mut discovered: Vec<MlxDistributedDiscoveredNodeDto> = Vec::new();

    // Names, memory, reachability.
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for (rank, probe) in probes.iter().enumerate() {
        let host = hosts[rank].clone();
        let label = host.clone().unwrap_or_else(|| "this Mac".to_string());
        match probe {
            Ok(p) => {
                let mut name = if p.computer_name.is_empty() {
                    p.hostname.clone()
                } else {
                    p.computer_name.clone()
                };
                if name.is_empty() {
                    c.gap(
                        Some(rank),
                        "name",
                        format!("{label} reported no ComputerName and no hostname"),
                    );
                } else {
                    if !taken.insert(name.clone()) {
                        name = format!("{name} ({label})");
                        taken.insert(name.clone());
                    }
                    c.found(
                        Some(rank),
                        "name",
                        &name,
                        format!(
                            "scutil --get ComputerName on {label} (hostname -s: {})",
                            p.hostname
                        ),
                    );
                    nodes[rank].name = name.clone();
                }
                discovered.push(MlxDistributedDiscoveredNodeDto {
                    rank: rank as u32,
                    name,
                    host,
                    reachable: true,
                    home: Some(p.home.clone()),
                    total_bytes: Some(p.memory.total_bytes),
                    available_bytes: Some(p.memory.available_bytes),
                    pressure: Some(p.pressure.as_str().to_string()),
                    uv: p.uv.as_ref().map(|(path, v)| format!("{path} · {v}")),
                    env: None,
                    link_speed: None,
                });
            }
            Err(why) => {
                c.gap(Some(rank), "reachable", format!("{label}: {why}"));
                nodes[rank].name = label.clone();
                discovered.push(MlxDistributedDiscoveredNodeDto {
                    rank: rank as u32,
                    name: label,
                    host,
                    ..Default::default()
                });
            }
        }
    }
    let ok: Vec<Option<&NodeProbe>> = probes.iter().map(|p| p.as_ref().ok()).collect();

    // The link: rank 0 to every peer by the Link path detector, then every peer pair.
    let mut paths: Vec<Option<LinkPath>> = vec![None; size];
    if let Some(local) = ok[0] {
        for rank in 1..size {
            let Some(peer) = ok[rank] else { continue };
            match netpath::choose_path(&local.facts, &peer.facts) {
                Some(path) => paths[rank] = Some(path),
                None => c.gap(
                    Some(rank),
                    "tbIp",
                    format!(
                        "{} and this Mac share no subnet on any Thunderbolt, Ethernet or Wi-Fi port \
                         (this Mac: {}; {}: {}) — connect the Thunderbolt cable and give both ends \
                         an address in one subnet",
                        nodes[rank].name,
                        describe_facts(&local.facts),
                        nodes[rank].name,
                        describe_facts(&peer.facts)
                    ),
                ),
            }
        }
    }
    let local_devices: BTreeSet<&str> = paths
        .iter()
        .flatten()
        .map(|p| p.local.device.as_str())
        .collect();
    let mut selected: Vec<Option<InterfaceFact>> = vec![None; size];
    if local_devices.len() > 1 {
        c.gap(
            Some(0),
            "tbInterface",
            format!(
                "this Mac reaches its peers over different interfaces ({local_devices:?}); a node \
                 carries one link address in the config"
            ),
        );
    } else if let Some(first) = paths.iter().flatten().next() {
        selected[0] = Some(first.local.clone());
    }
    for rank in 1..size {
        if let Some(path) = &paths[rank] {
            selected[rank] = Some(path.peer.clone());
        }
    }
    let mut all_thunderbolt = size > 1
        && (1..size).all(|r| {
            paths[r]
                .as_ref()
                .is_some_and(|p| p.kind == LinkKind::Thunderbolt)
        });
    for a in 1..size {
        for b in (a + 1)..size {
            let (Some(pa), Some(pb)) = (ok[a], ok[b]) else {
                continue;
            };
            let pair = netpath::choose_path(&pa.facts, &pb.facts);
            let joined = pair.as_ref().is_some_and(|p| {
                Some(&p.local.device) == selected[a].as_ref().map(|f| &f.device)
                    && Some(&p.peer.device) == selected[b].as_ref().map(|f| &f.device)
            });
            if !joined {
                all_thunderbolt = false;
                c.gap(
                    Some(b),
                    "tbIp",
                    format!(
                        "{} and {} share no direct path on the interfaces they use to reach this Mac",
                        nodes[a].name, nodes[b].name
                    ),
                );
            } else if pair.is_some_and(|p| p.kind != LinkKind::Thunderbolt) {
                all_thunderbolt = false;
            }
        }
    }
    for rank in 0..size {
        let (Some(fact), Some(p)) = (&selected[rank], ok[rank]) else {
            if ok[rank].is_some() && rank == 0 && size > 1 && paths.iter().all(Option::is_none) {
                c.gap(
                    Some(0),
                    "tbIp",
                    "this Mac has no direct path to any peer".to_string(),
                );
            }
            continue;
        };
        let port = fact.hardware_port.clone().unwrap_or_default();
        let other = if rank == 0 {
            paths.iter().flatten().next().map(|p| &p.peer)
        } else {
            paths[rank].as_ref().map(|p| &p.local)
        };
        let shared = other
            .map(|o| format!(", sharing its /{} with {}", fact.prefix_len, o.ipv4))
            .unwrap_or_default();
        nodes[rank].tb_ip = fact.ipv4.to_string();
        nodes[rank].tb_interface = fact.device.clone();
        nodes[rank].tb_netmask = netmask(fact.prefix_len);
        c.found(
            Some(rank),
            "tbInterface",
            &fact.device,
            format!("networksetup: {} is hardware port '{port}'", fact.device),
        );
        c.found(
            Some(rank),
            "tbIp",
            &nodes[rank].tb_ip.clone(),
            format!(
                "ifconfig {}: inet {}/{}{shared}",
                fact.device, fact.ipv4, fact.prefix_len
            ),
        );
        c.found(
            Some(rank),
            "tbNetmask",
            &nodes[rank].tb_netmask.clone(),
            format!("prefix /{} on {}", fact.prefix_len, fact.device),
        );
        discovered[rank].link_speed = fact.link_speed.clone();
        match p
            .services
            .iter()
            .find(|(_, (_, device, _))| device == &fact.device)
        {
            Some((service, (hw, _, enabled))) => {
                nodes[rank].tb_service = service.clone();
                c.found(
                    Some(rank),
                    "tbService",
                    service,
                    format!(
                        "networksetup -listnetworkserviceorder: '{service}' on {hw} ({}), {}",
                        fact.device,
                        if *enabled { "enabled" } else { "DISABLED" }
                    ),
                );
            }
            None => c.gap(
                Some(rank),
                "tbService",
                format!(
                    "no network service sits on {} (networksetup -listnetworkserviceorder)",
                    fact.device
                ),
            ),
        }
    }

    // RDMA per node, then the backend.
    let mut rdma_ok = vec![false; size];
    for rank in 0..size {
        let (Some(fact), Some(p)) = (&selected[rank], ok[rank]) else {
            continue;
        };
        let want = format!("rdma_{}", fact.device);
        let Some(device) = p.rdma.iter().find(|d| d.name == want) else {
            let known: Vec<&str> = p.rdma.iter().map(|d| d.name.as_str()).collect();
            c.gap(
                Some(rank),
                "rdmaDevice",
                format!(
                    "ibv_devinfo lists no {want} (devices: {known:?}) — RDMA over Thunderbolt is off on \
                     {}; enable it with `rdma_ctl enable` from recovery, or run over ring",
                    nodes[rank].name
                ),
            );
            continue;
        };
        let gid = probe::ipv4_gid(&fact.ipv4.to_string());
        let gid_note = match device.gids.iter().find(|(_, g)| *g == gid) {
            Some((1, _)) => format!("GID[1] = {gid}"),
            Some((i, _)) => format!(
                "{gid} sits at GID[{i}], not GID[1] — the next JACCL start would fail `RTR errno 96`; \
                 preflight's link repair re-seats it"
            ),
            None => format!("no IPv4-mapped GID {gid} yet — preflight's link repair re-applies the address"),
        };
        if !device.active {
            c.gap(
                Some(rank),
                "rdmaDevice",
                format!("{want} is present but its port is not PORT_ACTIVE (is the cable in?)"),
            );
            continue;
        }
        rdma_ok[rank] = true;
        nodes[rank].rdma_device = want.clone();
        c.found(
            Some(rank),
            "rdmaDevice",
            &want,
            format!("ibv_devinfo -v: {want} PORT_ACTIVE, {gid_note}"),
        );
    }
    let jaccl = all_thunderbolt && rdma_ok.iter().all(|b| *b);
    let names = |f: &dyn Fn(usize) -> String| (0..size).map(f).collect::<Vec<_>>().join(", ");
    let backend_reason = if jaccl {
        format!(
            "JACCL: every node has an active RDMA device on the shared Thunderbolt link ({})",
            names(&|r| format!(
                "{} {} {}",
                nodes[r].name,
                nodes[r].rdma_device,
                discovered[r]
                    .link_speed
                    .clone()
                    .unwrap_or_else(|| "speed not reported".to_string())
            ))
        )
    } else if size > 1 && selected.iter().all(Option::is_some) {
        let why = if !all_thunderbolt {
            "the nodes are not joined by Thunderbolt on both ends".to_string()
        } else {
            format!(
                "no active RDMA device on {}",
                (0..size)
                    .filter(|r| !rdma_ok[*r])
                    .map(|r| nodes[r].name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        format!("ring (TCP): JACCL needs RDMA on both ends of a Thunderbolt link, and {why}")
    } else {
        String::new()
    };
    let backend = if jaccl {
        "jaccl"
    } else if backend_reason.is_empty() {
        c.gap(
            None,
            "backend",
            "no direct link between the nodes was found".to_string(),
        );
        ""
    } else {
        "ring"
    };
    if !backend.is_empty() {
        c.found(None, "backend", backend, backend_reason.clone());
    }
    if backend != "jaccl" {
        for node in &mut nodes {
            node.rdma_device.clear();
        }
        c.gaps.retain(|g| g.field != "rdmaDevice");
    }

    // Python: the goose-managed env on every node.
    for rank in 0..size {
        let Some(p) = ok[rank] else { continue };
        let spec = EnvSpec::tensor();
        let python = spec.python(&p.home);
        let uv = p.uv.as_ref().map(|(path, v)| format!("{path} ({v})"));
        let (state, detail) = match &p.env {
            Some(EnvProbe::Present { answer, .. }) if *answer == spec.expect => (
                "ready",
                format!(
                    "imports mlx {} and mlx_lm {}",
                    provision::MLX_VERSION,
                    provision::MLX_LM_VERSION
                ),
            ),
            Some(EnvProbe::Present { answer, .. }) => (
                "broken",
                format!(
                    "present but answers '{answer}', not '{}' — Save re-provisions it",
                    spec.expect
                ),
            ),
            _ => match &uv {
                Some(uv) => (
                    "absent",
                    format!("not provisioned yet — Save builds it with {uv}"),
                ),
                None => (
                    "noUv",
                    format!(
                        "not provisioned, and no uv on this node (looked: {})",
                        provision::UV_LOOKED
                    ),
                ),
            },
        };
        discovered[rank].env = Some(MlxDistributedDiscoveredEnvDto {
            python: python.clone(),
            state: state.to_string(),
            detail: detail.clone(),
        });
        if state == "noUv" {
            c.gap(
                Some(rank),
                "python",
                format!(
                    "{}: {detail} — install uv there (`brew install uv`), or set its Python under Advanced",
                    nodes[rank].name
                ),
            );
        } else {
            nodes[rank].python = python.clone();
            c.found(
                Some(rank),
                "python",
                &python,
                format!(
                    "goose-managed env (uv, Python {}): {detail}",
                    provision::PYTHON_VERSION
                ),
            );
        }
    }

    // Models: every splittable model on this Mac, and where it is on each peer.
    let mut models: Vec<MlxDistributedDiscoveredModelDto> = Vec::new();
    if let Some(local) = ok[0] {
        for m in &local.models {
            let Ok(runner) = Runner::for_model_type(&m.model_type) else {
                continue;
            };
            let id = model_id(&m.dir, &context.goose_models_dir);
            let key = model_key(&id).to_string();
            let hf_key = key.clone();
            let loaded = loaded_files(&m.files);
            if loaded.is_empty() || !loaded.keys().any(|n| n.ends_with(".safetensors")) {
                continue;
            }
            let mut per_node = vec![MlxDistributedDiscoveredModelNodeDto {
                rank: 0,
                state: "match".to_string(),
                dir: Some(m.dir.clone()),
                detail: format!("{} files, {}", loaded.len(), gib(loaded.values().sum())),
            }];
            let mut manifest = if m.sums.is_empty() { "absent" } else { "agree" };
            for (rank, peer) in ok.iter().enumerate().skip(1) {
                let Some(peer) = peer else {
                    per_node.push(MlxDistributedDiscoveredModelNodeDto {
                        rank: rank as u32,
                        state: "absent".to_string(),
                        dir: None,
                        detail: "node not reachable".to_string(),
                    });
                    continue;
                };
                let copies = peer_copies(peer, &key, &hf_key);
                let matching: Vec<&&ModelDir> = copies
                    .iter()
                    .filter(|c| loaded_files(&c.files) == loaded)
                    .collect();
                if let Some(found) = matching.first() {
                    let others: Vec<&str> = matching[1..].iter().map(|c| c.dir.as_str()).collect();
                    let sums_note = if m.sums.is_empty() {
                        String::new()
                    } else if found.sums == m.sums {
                        "; SHA256SUMS identical".to_string()
                    } else {
                        manifest = "differ";
                        "; SHA256SUMS DIFFERS".to_string()
                    };
                    per_node.push(MlxDistributedDiscoveredModelNodeDto {
                        rank: rank as u32,
                        state: "match".to_string(),
                        dir: Some(found.dir.clone()),
                        detail: format!(
                            "same {} loaded files at the same sizes{sums_note}{}",
                            loaded.len(),
                            if others.is_empty() {
                                String::new()
                            } else {
                                format!("; identical copies also at {}", others.join(", "))
                            }
                        ),
                    });
                } else if let Some(near) = copies.first() {
                    let theirs = loaded_files(&near.files);
                    let differing: Vec<String> = loaded
                        .iter()
                        .filter(|(n, s)| theirs.get(*n) != Some(s))
                        .map(|(n, s)| {
                            format!(
                                "{n} {} vs {s}",
                                theirs
                                    .get(n)
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "absent".to_string())
                            )
                        })
                        .collect();
                    per_node.push(MlxDistributedDiscoveredModelNodeDto {
                        rank: rank as u32,
                        state: "differs".to_string(),
                        dir: Some(near.dir.clone()),
                        detail: format!("files differ from this Mac's: {}", differing.join(", ")),
                    });
                } else {
                    per_node.push(MlxDistributedDiscoveredModelNodeDto {
                        rank: rank as u32,
                        state: "absent".to_string(),
                        dir: None,
                        detail: format!(
                            "no directory named {key} under {} — copy it over Thunderbolt from this \
                             Mac's Models tab (needs both Macs signed in to LeanZero Link)",
                            if peer.roots.is_empty() {
                                "any model root".to_string()
                            } else {
                                peer.roots.join(", ")
                            }
                        ),
                    });
                }
            }
            let on_every_node = per_node.iter().all(|n| n.state == "match");
            models.push(MlxDistributedDiscoveredModelDto {
                id,
                model_type: m.model_type.clone(),
                runner: runner.as_str().to_string(),
                weights_bytes: loaded.values().sum(),
                on_every_node,
                manifest: manifest.to_string(),
                nodes: per_node,
            });
        }
    }
    let everywhere: Vec<&MlxDistributedDiscoveredModelDto> = models
        .iter()
        .filter(|m| m.on_every_node && m.manifest != "differ")
        .collect();
    let chosen = context
        .preferred_model
        .as_ref()
        .and_then(|want| everywhere.iter().find(|m| &m.id == want).copied())
        .or_else(|| (everywhere.len() == 1).then(|| everywhere[0]));
    let mut model_id_value = String::new();
    match chosen {
        Some(model) => {
            model_id_value = model.id.clone();
            c.found(
                None,
                "modelId",
                &model.id,
                format!(
                    "{} ({}, {} loaded) is on every node{}",
                    model.id,
                    model.model_type,
                    gib(model.weights_bytes),
                    if everywhere.len() > 1 {
                        format!(" — the one asked for, of {} candidates", everywhere.len())
                    } else {
                        String::new()
                    }
                ),
            );
            for n in &model.nodes {
                let rank = n.rank as usize;
                if let Some(dir) = &n.dir {
                    nodes[rank].model_dir = dir.clone();
                    c.found(Some(rank), "modelDir", dir, n.detail.clone());
                }
            }
            if model.runner == Runner::PipelineQwen4.as_str() {
                for rank in 0..size {
                    if let Some(p) = ok[rank] {
                        let spec = EnvSpec::pipeline();
                        let ready = matches!(&p.pipeline_env, Some(EnvProbe::Present { answer, .. }) if *answer == spec.expect);
                        let python = spec.python(&p.home);
                        nodes[rank].pipeline_python = Some(python.clone());
                        c.found(
                            Some(rank),
                            "pipelinePython",
                            &python,
                            if ready {
                                "goose-managed fork env: imports rapid_mlx.distributed.pipeline_qwen4".to_string()
                            } else {
                                format!("goose-managed fork env ({}): not provisioned yet — Save builds it", provision::PIPELINE_FORK)
                            },
                        );
                    }
                }
            }
        }
        None if everywhere.is_empty() => c.gap(
            None,
            "modelId",
            if models.is_empty() {
                "no splittable model (qwen3_5 tensor, qwen4_exp pipeline) is on this Mac".to_string()
            } else {
                format!(
                    "none of this Mac's {} splittable models is on every node with the same files — \
                     copy one over Thunderbolt (Models tab; needs LeanZero Link sign-in on both Macs)",
                    models.len()
                )
            },
        ),
        None => c.gap(
            None,
            "modelId",
            format!(
                "{} models are on every node: {} — pick one",
                everywhere.len(),
                everywhere.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
            ),
        ),
    }

    // Ports: the API above the single engine's, the coordinator below every ephemeral range.
    let mut port = 0u16;
    let mut coordinator_port = 0u16;
    if let Some(local) = ok[0] {
        match (context.single_port.saturating_add(1)..u16::MAX)
            .find(|p| !local.listening.contains(p))
        {
            Some(p) => {
                port = p;
                c.found(
                    None,
                    "port",
                    &p.to_string(),
                    format!(
                        "the first port above the single engine's {} with no listener on this Mac (lsof)",
                        context.single_port
                    ),
                );
            }
            None => c.gap(
                None,
                "port",
                format!("no free port above {}", context.single_port),
            ),
        }
        let ceiling = ok.iter().flatten().map(|p| p.ephemeral_first).min();
        let all_probed = ok.iter().all(Option::is_some);
        if port != 0 && all_probed {
            let ceiling = ceiling.unwrap_or(0);
            let ring = backend == "ring";
            let free = |c: u16| {
                (0..size).all(|r| {
                    let binds = if ring {
                        Some(c + r as u16)
                    } else {
                        (r == 0).then_some(c)
                    };
                    binds.is_none_or(|b| {
                        b != port && !ok[r].is_some_and(|p| p.listening.contains(&b))
                    })
                })
            };
            match (port + 1..ceiling)
                .find(|c| (*c as usize) + size - 1 < ceiling as usize && free(*c))
            {
                Some(found) => {
                    coordinator_port = found;
                    c.found(
                        None,
                        "coordinatorPort",
                        &found.to_string(),
                        format!(
                            "{}free on {} (lsof), below every node's ephemeral range (net.inet.ip.portrange.first = {ceiling})",
                            if ring {
                                format!("{found}..={} (one per rank) ", found as usize + size - 1)
                            } else {
                                String::new()
                            },
                            if ring { "every node".to_string() } else { "this Mac".to_string() }
                        ),
                    );
                }
                None => c.gap(
                    None,
                    "coordinatorPort",
                    format!(
                        "no free port between {} and the ephemeral range {ceiling}",
                        port + 1
                    ),
                ),
            }
        } else if !all_probed {
            c.gap(
                None,
                "coordinatorPort",
                "a node could not be probed for its listening ports".to_string(),
            );
        }
    }

    MlxDistributedDiscoveryDto {
        config: MlxDistributedConfigDto {
            model_id: model_id_value,
            backend: backend.to_string(),
            port,
            coordinator_port,
            context: None,
            restart_on_failure: false,
            hang_ratio_only: false,
            watchdog_warn_ratio: None,
            watchdog_critical_ratio: None,
            nodes,
        },
        backend_reason,
        evidence: c.evidence,
        gaps: c.gaps,
        nodes: discovered,
        models,
        probe_ms: 0,
    }
}

fn describe_facts(facts: &[InterfaceFact]) -> String {
    if facts.is_empty() {
        return "no addressed hardware port".to_string();
    }
    facts
        .iter()
        .map(|f| {
            format!(
                "{} {}/{} ({})",
                f.device,
                f.ipv4,
                f.prefix_len,
                f.hardware_port.as_deref().unwrap_or("?")
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Run the probe on this Mac and every peer (in parallel) and compose.
pub(super) async fn discover(
    exec: Arc<dyn NodeExec>,
    peers: &[String],
    extra_roots: &BTreeMap<Option<String>, Vec<String>>,
    context: &Context,
) -> MlxDistributedDiscoveryDto {
    let started = std::time::Instant::now();
    let hosts: Vec<Option<String>> = std::iter::once(None)
        .chain(peers.iter().map(|p| Some(p.clone())))
        .collect();
    let probes = futures::future::join_all(hosts.iter().map(|host| {
        let exec = Arc::clone(&exec);
        let roots = extra_roots.get(host).cloned().unwrap_or_default();
        let script = discover_script(&roots);
        async move {
            let script = match host {
                None => script,
                Some(_) => format!("/bin/sh -c {}", sh_quote(&script)),
            };
            match exec.run(host.as_deref(), &script).await {
                Ok(out) if out.ssh_failed() => Err(format!("ssh failed: {}", out.stderr.trim())),
                Ok(out) => parse_node(&out.stdout),
                Err(e) => Err(format!("{e:#}")),
            }
        }
    }))
    .await;
    let mut result = compose(&hosts, &probes, context);
    result.probe_ms = started.elapsed().as_millis() as u64;
    result
}

/// `Host` aliases in an ssh config (no wildcards, no negations), in file order.
pub(super) fn ssh_config_hosts(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("Host ")
            .or_else(|| line.strip_prefix("Host\t"))
        else {
            continue;
        };
        for alias in rest.split_whitespace() {
            if alias.contains(['*', '?', '!']) || out.iter().any(|a| a == alias) {
                continue;
            }
            out.push(alias.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MACBOOK: &str =
        include_str!("../../../tests/fixtures/mlx-distributed-discover/macbook.txt");
    const WORKHORSE: &str =
        include_str!("../../../tests/fixtures/mlx-distributed-discover/workhorse.txt");

    fn context() -> Context {
        Context {
            single_port: 8090,
            goose_models_dir: "/Users/mihaiperdum/.goose/models".to_string(),
            preferred_model: None,
        }
    }

    fn hosts() -> Vec<Option<String>> {
        vec![None, Some("workhorse".to_string())]
    }

    fn real() -> MlxDistributedDiscoveryDto {
        compose(
            &hosts(),
            &[parse_node(MACBOOK), parse_node(WORKHORSE)],
            &context(),
        )
    }

    fn evidence<'a>(d: &'a MlxDistributedDiscoveryDto, node: Option<u32>, field: &str) -> &'a str {
        &d.evidence
            .iter()
            .find(|e| e.node == node && e.field == field)
            .unwrap_or_else(|| panic!("no evidence for {node:?} {field}: {:#?}", d.evidence))
            .evidence
    }

    #[test]
    fn the_two_real_macs_parse_whole() {
        let mb = parse_node(MACBOOK).unwrap();
        assert_eq!(mb.home, "/Users/mihaiperdum");
        assert_eq!(mb.computer_name, "Mihai Macbook");
        assert_eq!(mb.memory.total_bytes, 137_438_953_472);
        assert_eq!(mb.ephemeral_first, 49152);
        assert!(mb.listening.contains(&8090), "the single engine listens");
        let en3 = mb.rdma.iter().find(|d| d.name == "rdma_en3").unwrap();
        assert!(en3.active);
        assert!(en3.gids.contains(&(1, "::ffff:192.168.0.1".to_string())));
        assert!(mb.rdma.iter().any(|d| d.name == "rdma_en1" && !d.active));
        assert_eq!(mb.uv.as_ref().unwrap().0, "/opt/homebrew/bin/uv");

        let wh = parse_node(WORKHORSE).unwrap();
        assert_eq!(wh.home, "/Users/workhorse");
        assert_eq!(wh.memory.total_bytes, 103_079_215_104);
        assert!(wh
            .roots
            .contains(&"/Users/workhorse/jaccl-smoke/models".to_string()));
        assert!(wh.models.iter().any(|m| m
            .dir
            .ends_with("jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx")
            && m.model_type == "qwen3_5"));
    }

    #[test]
    fn detect_fills_the_recorded_two_mac_setup_from_evidence() {
        let d = real();
        let c = &d.config;
        assert_eq!(c.backend, "jaccl", "{}", d.backend_reason);
        assert!(
            d.backend_reason.contains("rdma_en3"),
            "{}",
            d.backend_reason
        );
        let (mb, wh) = (&c.nodes[0], &c.nodes[1]);
        assert_eq!(mb.name, "Mihai Macbook");
        assert_eq!(wh.name, "Work’s Mac Studio");
        assert_eq!(wh.ssh.as_deref(), Some("workhorse"));
        assert_eq!(
            (mb.tb_ip.as_str(), wh.tb_ip.as_str()),
            ("192.168.0.1", "192.168.0.2")
        );
        assert_eq!(mb.tb_netmask, "255.255.255.252");
        assert_eq!(
            (mb.tb_interface.as_str(), wh.tb_interface.as_str()),
            ("en3", "en3")
        );
        assert_eq!(mb.tb_service, "EXO Thunderbolt 3");
        assert_eq!(wh.tb_service, "EXO Thunderbolt 2");
        assert_eq!(
            (mb.rdma_device.as_str(), wh.rdma_device.as_str()),
            ("rdma_en3", "rdma_en3")
        );
        assert!(evidence(&d, Some(1), "rdmaDevice").contains("GID[1] = ::ffff:192.168.0.2"));
        assert!(evidence(&d, Some(1), "tbIp").contains("192.168.0.1"));
        assert_eq!(d.nodes[1].link_speed.as_deref(), Some("80 Gb/s"));
        assert_eq!(
            wh.python,
            "/Users/workhorse/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12/bin/python"
        );
        assert_eq!(d.nodes[1].env.as_ref().unwrap().state, "absent");
        assert!(d.nodes[1]
            .uv
            .as_ref()
            .unwrap()
            .starts_with("/opt/homebrew/bin/uv"));
        // Ports: the single engine's 8090 is taken; 8091 is the first free above it.
        assert_eq!(c.port, 8091);
        assert!(c.coordinator_port > c.port && c.coordinator_port < 49152);
    }

    /// Two splittable models are on both Macs with the same loaded files — the 27B (in the
    /// workhorse's ~/jaccl-smoke/models, the LM Studio copy with the same weights and another
    /// README named as an identical alternative) and the Flash — so Detect names the choice as a
    /// gap instead of picking; asked for the 27B, it fills both model folders.
    #[test]
    fn two_models_are_on_both_macs_so_the_choice_is_named_until_one_is_asked_for() {
        let d = real();
        let id = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";
        assert_eq!(d.config.model_id, "");
        let gap = d.gaps.iter().find(|g| g.field == "modelId").unwrap();
        assert!(
            gap.reason.contains(id) && gap.reason.contains("Flash"),
            "{}",
            gap.reason
        );
        let model = d.models.iter().find(|m| m.id == id).unwrap();
        assert!(model.on_every_node);
        assert_eq!(model.runner, "mlxLmTensor");
        assert!(
            model.nodes[1].detail.contains(".lmstudio"),
            "{}",
            model.nodes[1].detail
        );
        let flash = d
            .models
            .iter()
            .find(|m| m.id.ends_with("Qwen3.8-Flash-Next-4bit"))
            .unwrap();
        assert_eq!(flash.runner, "pipelineQwen4");
        assert_eq!(flash.manifest, "agree");
        assert_eq!(
            d.models.len(),
            2,
            "the HF snapshots without safetensors are not models"
        );

        // The workhorse without the 27B: absent there, the Thunderbolt copy named, no pick.
        let mut wh = parse_node(WORKHORSE).unwrap();
        wh.models
            .retain(|m| !m.dir.ends_with("Qwen3.8-27B-Atlassian-Q8-mlx"));
        let without = compose(
            &hosts(),
            &[parse_node(MACBOOK), Ok(wh)],
            &Context {
                preferred_model: Some(id.to_string()),
                ..context()
            },
        );
        let missing = without.models.iter().find(|m| m.id == id).unwrap();
        assert_eq!(missing.nodes[1].state, "absent");
        assert!(
            missing.nodes[1].detail.contains("Thunderbolt"),
            "{}",
            missing.nodes[1].detail
        );
        assert!(
            missing.nodes[1].detail.contains("jaccl-smoke/models"),
            "the roots looked in are named"
        );
        assert!(
            without.config.model_id.ends_with("Qwen3.8-Flash-Next-4bit"),
            "the one model left on every node is the only candidate"
        );

        let asked = compose(
            &hosts(),
            &[parse_node(MACBOOK), parse_node(WORKHORSE)],
            &Context {
                preferred_model: Some(id.to_string()),
                ..context()
            },
        );
        assert_eq!(asked.config.model_id, id);
        assert!(asked.gaps.is_empty(), "{:#?}", asked.gaps);
        assert_eq!(
            asked.config.nodes[0].model_dir,
            "/Users/mihaiperdum/.goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx"
        );
        assert_eq!(
            asked.config.nodes[1].model_dir,
            "/Users/workhorse/jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx"
        );
        assert!(asked
            .config
            .nodes
            .iter()
            .all(|n| n.pipeline_python.is_none()));
    }

    /// NEGATIVE: no cable (both TB addresses gone) → the Wi-Fi LAN path, ring with its reason and
    /// no RDMA device; a peer that did not answer is a named gap, never a filled node.
    #[test]
    fn without_the_cable_it_is_ring_and_an_unreachable_peer_is_a_gap() {
        let mut mb = parse_node(MACBOOK).unwrap();
        let mut wh = parse_node(WORKHORSE).unwrap();
        mb.facts.retain(|f| f.device != "en3");
        wh.facts.retain(|f| f.device != "en3");
        let d = compose(&hosts(), &[Ok(mb), Ok(wh)], &context());
        assert_eq!(d.config.backend, "ring", "{}", d.backend_reason);
        assert!(
            d.backend_reason.contains("not joined by Thunderbolt"),
            "{}",
            d.backend_reason
        );
        assert_eq!(d.config.nodes[1].tb_ip, "192.168.10.161");
        assert!(d.config.nodes.iter().all(|n| n.rdma_device.is_empty()));

        let d = compose(
            &hosts(),
            &[
                parse_node(MACBOOK),
                Err("ssh failed: Connection refused".to_string()),
            ],
            &context(),
        );
        assert!(d
            .gaps
            .iter()
            .any(|g| g.node == Some(1) && g.field == "reachable" && g.reason.contains("refused")));
        assert!(d.config.nodes[1].tb_ip.is_empty());
        assert!(d.config.nodes[1].python.is_empty());
        assert_eq!(d.config.backend, "");
        assert_eq!(d.config.coordinator_port, 0);
    }

    #[test]
    fn no_uv_on_a_peer_is_a_loud_gap_not_a_guessed_python() {
        let mut wh = parse_node(WORKHORSE).unwrap();
        wh.uv = None;
        let d = compose(&hosts(), &[parse_node(MACBOOK), Ok(wh)], &context());
        assert!(d.config.nodes[1].python.is_empty());
        let gap = d
            .gaps
            .iter()
            .find(|g| g.node == Some(1) && g.field == "python")
            .unwrap();
        assert!(gap.reason.contains("no uv"), "{}", gap.reason);
        assert_eq!(d.nodes[1].env.as_ref().unwrap().state, "noUv");
    }

    #[test]
    fn ids_follow_the_goose_dir_the_hf_cache_or_the_folder_name() {
        let root = "/Users/me/.goose/models";
        assert_eq!(model_id("/Users/me/.goose/models/org/m", root), "org/m");
        assert_eq!(
            model_id(
                "/Users/me/.cache/huggingface/hub/models--mlx-community--Q-4bit/snapshots/abc",
                root
            ),
            "mlx-community/Q-4bit"
        );
        assert_eq!(model_id("/Users/w/jaccl-smoke/models/M", root), "M");
        assert_eq!(netmask(30), "255.255.255.252");
        assert_eq!(netmask(24), "255.255.255.0");
    }

    #[test]
    fn listening_ports_and_ssh_hosts_are_read() {
        let lsof = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
python3 1 me 5u IPv4 0x1 0t0 TCP 127.0.0.1:8090 (LISTEN)
rapportd 2 me 10u IPv6 0x2 0t0 TCP *:49159 (LISTEN)
x 3 me 1u IPv6 0x3 0t0 TCP [::1]:5000 (LISTEN)
";
        assert_eq!(
            parse_listening_ports(lsof),
            BTreeSet::from([8090, 49159, 5000])
        );
        let cfg = "Host workhorse\n  HostName 192.168.8.220\nHost *\n  IdentitiesOnly yes\nHost a b !c\nHost workhorse\n";
        assert_eq!(ssh_config_hosts(cfg), vec!["workhorse", "a", "b"]);
    }

    /// Re-captures the fixtures from the real pair: this Mac and the `workhorse` alias.
    /// `cargo test -p goose --lib capture_discover_fixtures -- --ignored`
    #[tokio::test]
    #[ignore = "probes this Mac and ssh workhorse; rewrites the fixtures"]
    async fn capture_discover_fixtures() {
        use goose_sidecar::distributed::SystemExec;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mlx-distributed-discover");
        let script = discover_script(&[]);
        for (host, file) in [(None, "macbook.txt"), (Some("workhorse"), "workhorse.txt")] {
            let script = match host {
                None => script.clone(),
                Some(_) => format!("/bin/sh -c {}", sh_quote(&script)),
            };
            let out = SystemExec.run(host, &script).await.unwrap();
            assert!(out.success(), "{}", out.stderr);
            parse_node(&out.stdout).unwrap();
            std::fs::write(dir.join(file), out.stdout).unwrap();
        }
    }

    /// Writes what Detect returns for the real pair (model not chosen, then the 27B asked for) as
    /// the desktop's fixture, so the UI tests read the backend's own output.
    /// `cargo test -p goose --lib export_discovery_ui_fixture -- --ignored`
    #[test]
    #[ignore = "rewrites ui/desktop's discovery fixture"]
    fn export_discovery_ui_fixture() {
        let asked = compose(
            &hosts(),
            &[parse_node(MACBOOK), parse_node(WORKHORSE)],
            &Context {
                preferred_model: Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx".to_string()),
                ..context()
            },
        );
        let json = serde_json::json!({ "unchosen": real(), "chosen27b": asked });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../ui/desktop/src/components/leanzero-swarm/mlxDistributedDiscovery.fixture.json",
        );
        std::fs::write(path, serde_json::to_string_pretty(&json).unwrap() + "\n").unwrap();
    }

    #[test]
    fn the_script_never_lists_a_protected_folder_and_ends_with_its_marker() {
        let script = discover_script(&["/opt/models x".to_string()]);
        assert!(script.contains("\"$HOME/Documents\""));
        assert!(script.contains("'/opt/models x'"));
        assert!(script.trim_end().ends_with("echo @@end"));
    }
}
