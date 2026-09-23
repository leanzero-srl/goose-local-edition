//! Which physical path joins this node to a peer: a Thunderbolt cable, a shared LAN, or
//! nothing direct.
//!
//! A node reports its own [`InterfaceFact`]s (every IPv4 address on an interface macOS names
//! as a hardware port); the node that wants to move bytes compares both lists with
//! [`choose_path`]. Thunderbolt is recognised by the HARDWARE PORT name, not the device
//! name: `networksetup -listallhardwareports` names the TB ports "Thunderbolt N" (and a
//! bridge "Thunderbolt Bridge") while the device is an ordinary `enN` — measured on both
//! Macs of the 2026-09-23 TB5 pair (MacBook en3 = "Thunderbolt 3", Studio en3 =
//! "Thunderbolt 2"). Interfaces with no hardware port (utun VPNs — the personal Tailscale
//! among them — awdl, bridges of VMs) are never candidates.
//!
//! The negotiated link speed comes from `system_profiler SPThunderboltDataType -json`: the
//! bus whose `receptacle_id_key` equals the port number N of "Thunderbolt N" and whose
//! status is `receptacle_connected` carries `current_speed_key` ("80 Gb/s"). That port ↔
//! receptacle correspondence is a MEASURED pairing (both Macs, 2026-09-23: port 3 ↔
//! receptacle 3 connected at 80 Gb/s, port 2 ↔ receptacle 2 connected at 80 Gb/s), not an
//! Apple-documented one; when no receptacle matches, the speed is absent, never guessed.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    Thunderbolt,
    Ethernet,
    Wifi,
    Other,
}

/// One IPv4 address on one interface of a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceFact {
    pub device: String,
    /// The macOS hardware-port name ("Thunderbolt 3", "Wi-Fi"); `None` where the platform
    /// has no such registry (Linux), in which case `kind` comes from the device name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware_port: Option<String>,
    pub kind: InterfaceKind,
    pub ipv4: Ipv4Addr,
    pub prefix_len: u8,
    /// The negotiated Thunderbolt link speed as the OS reports it ("80 Gb/s"); `None` for
    /// non-TB interfaces and whenever it could not be attributed to this port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_speed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Thunderbolt,
    Network,
}

/// The direct path chosen between two nodes: `local` is this node's interface, `peer` the
/// address on the other node inside the same subnet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkPath {
    pub kind: LinkKind,
    pub local: InterfaceFact,
    pub peer: InterfaceFact,
}

/// Parse `networksetup -listallhardwareports` into device → hardware-port name.
pub fn parse_hardware_ports(text: &str) -> HashMap<String, String> {
    let mut ports = HashMap::new();
    let mut port: Option<&str> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("Hardware Port: ") {
            port = Some(name.trim());
        } else if let Some(device) = line.strip_prefix("Device: ") {
            if let Some(name) = port.take() {
                ports.insert(device.trim().to_string(), name.to_string());
            }
        }
    }
    ports
}

pub fn classify_hardware_port(port: &str) -> InterfaceKind {
    if port.starts_with("Thunderbolt") {
        InterfaceKind::Thunderbolt
    } else if port == "Wi-Fi" || port == "AirPort" {
        InterfaceKind::Wifi
    } else if port.contains("Ethernet") || port.contains("LAN") {
        InterfaceKind::Ethernet
    } else {
        InterfaceKind::Other
    }
}

/// Linux has no hardware-port registry; the `thunderbolt-net` driver names its interfaces
/// `thunderboltN`, and the predictable-name scheme gives `en*`/`eth*` to wired and `wl*` to
/// wireless NICs. Everything else (tun, wg, docker, veth, tailscale) is `Other`.
pub fn classify_device_name(device: &str) -> InterfaceKind {
    if device.starts_with("thunderbolt") {
        InterfaceKind::Thunderbolt
    } else if device.starts_with("wl") {
        InterfaceKind::Wifi
    } else if device.starts_with("en") || device.starts_with("eth") {
        InterfaceKind::Ethernet
    } else {
        InterfaceKind::Other
    }
}

#[derive(Deserialize)]
struct ProfilerRoot {
    #[serde(rename = "SPThunderboltDataType", default)]
    buses: Vec<serde_json::Map<String, serde_json::Value>>,
}

/// Parse `system_profiler SPThunderboltDataType -json` into receptacle id → negotiated
/// speed, for CONNECTED receptacles only (an idle port reports "Up to 120 Gb/s", which is a
/// capability, not a negotiated link).
pub fn parse_thunderbolt_speeds(json: &str) -> Result<HashMap<String, String>, String> {
    let root: ProfilerRoot = serde_json::from_str(json)
        .map_err(|e| format!("system_profiler SPThunderboltDataType did not parse: {e}"))?;
    let mut speeds = HashMap::new();
    for bus in root.buses {
        for (key, value) in &bus {
            if !key.starts_with("receptacle_") {
                continue;
            }
            let field = |name: &str| value.get(name).and_then(|v| v.as_str());
            if field("receptacle_status_key") != Some("receptacle_connected") {
                continue;
            }
            if let (Some(id), Some(speed)) =
                (field("receptacle_id_key"), field("current_speed_key"))
            {
                speeds.insert(id.to_string(), speed.to_string());
            }
        }
    }
    Ok(speeds)
}

/// "Thunderbolt 3" → the negotiated speed of receptacle "3" (see the module doc for why
/// the pairing is a measured one).
fn speed_for_port(port: &str, speeds: &HashMap<String, String>) -> Option<String> {
    let number = port.strip_prefix("Thunderbolt ")?.trim();
    speeds.get(number).cloned()
}

/// Attach hardware ports, kinds and TB speeds to raw `(device, ipv4, prefix)` addresses.
/// `ports = None` means the platform has no hardware-port registry (device-name
/// classification); `Some(map)` keeps ONLY devices the registry names.
pub fn facts_from(
    addrs: &[(String, Ipv4Addr, u8)],
    ports: Option<&HashMap<String, String>>,
    speeds: &HashMap<String, String>,
) -> Vec<InterfaceFact> {
    addrs
        .iter()
        .filter_map(|(device, ipv4, prefix_len)| {
            let (hardware_port, kind) = match ports {
                Some(ports) => {
                    let port = ports.get(device)?;
                    (Some(port.clone()), classify_hardware_port(port))
                }
                None => (None, classify_device_name(device)),
            };
            let link_speed = match (&hardware_port, kind) {
                (Some(port), InterfaceKind::Thunderbolt) => speed_for_port(port, speeds),
                _ => None,
            };
            Some(InterfaceFact {
                device: device.clone(),
                hardware_port,
                kind,
                ipv4: *ipv4,
                prefix_len: *prefix_len,
                link_speed,
            })
        })
        .collect()
}

fn network_of(ip: Ipv4Addr, prefix_len: u8) -> Option<u32> {
    let mask = match prefix_len {
        0 => 0,
        1..=32 => u32::MAX << (32 - u32::from(prefix_len)),
        _ => return None,
    };
    Some(u32::from(ip) & mask)
}

/// Both addresses sit in one subnet as BOTH ends see it (same prefix, same network), and
/// they are different hosts.
fn same_subnet(a: &InterfaceFact, b: &InterfaceFact) -> bool {
    a.prefix_len == b.prefix_len
        && a.ipv4 != b.ipv4
        && network_of(a.ipv4, a.prefix_len).is_some()
        && network_of(a.ipv4, a.prefix_len) == network_of(b.ipv4, b.prefix_len)
}

fn usable(fact: &InterfaceFact) -> bool {
    // 169.254/16 is self-assigned on every idle link: two APIPA addresses "share" a subnet
    // on unrelated cables, so a match there proves nothing about a path.
    !fact.ipv4.is_loopback() && !fact.ipv4.is_link_local() && fact.kind != InterfaceKind::Other
}

fn preference(kind: InterfaceKind) -> u8 {
    match kind {
        InterfaceKind::Thunderbolt => 0,
        InterfaceKind::Ethernet => 1,
        InterfaceKind::Wifi => 2,
        InterfaceKind::Other => 3,
    }
}

/// The best direct path from `local` to `peer`: a Thunderbolt subnet shared by BOTH ends'
/// TB ports first, then a shared Ethernet/Wi-Fi subnet. `None` = no direct path at all —
/// the caller states that; it never invents one.
pub fn choose_path(local: &[InterfaceFact], peer: &[InterfaceFact]) -> Option<LinkPath> {
    let mut best: Option<(u8, LinkPath)> = None;
    for l in local.iter().filter(|f| usable(f)) {
        for p in peer.iter().filter(|f| usable(f)) {
            if !same_subnet(l, p) {
                continue;
            }
            let both_tb =
                l.kind == InterfaceKind::Thunderbolt && p.kind == InterfaceKind::Thunderbolt;
            let (rank, kind) = if both_tb {
                (0, LinkKind::Thunderbolt)
            } else {
                (
                    1 + preference(l.kind).max(preference(p.kind)),
                    LinkKind::Network,
                )
            };
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((
                    rank,
                    LinkPath {
                        kind,
                        local: l.clone(),
                        peer: p.clone(),
                    },
                ));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// This node's IPv4 addresses, as `(device, address, prefix length)`, for interfaces that
/// are UP and RUNNING and not loopback.
#[cfg(unix)]
pub fn raw_ipv4_addrs() -> Result<Vec<(String, Ipv4Addr, u8)>, String> {
    let mut out = Vec::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs allocates a linked list we only read and then free exactly once.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(format!(
            "getifaddrs failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut cursor = head;
    while !cursor.is_null() {
        // SAFETY: `cursor` walks the list getifaddrs returned; each node stays valid until
        // freeifaddrs below.
        let entry = unsafe { &*cursor };
        cursor = entry.ifa_next;
        let flags = entry.ifa_flags as libc::c_int;
        let up = flags & libc::IFF_UP != 0 && flags & libc::IFF_RUNNING != 0;
        if !up || flags & libc::IFF_LOOPBACK != 0 {
            continue;
        }
        if entry.ifa_addr.is_null() || entry.ifa_netmask.is_null() {
            continue;
        }
        // SAFETY: non-null sockaddr pointers from getifaddrs; the family is checked before
        // the reinterpretation as sockaddr_in.
        let family = unsafe { (*entry.ifa_addr).sa_family } as libc::c_int;
        if family != libc::AF_INET {
            continue;
        }
        let (addr, mask) = unsafe {
            let addr = &*(entry.ifa_addr as *const libc::sockaddr_in);
            let mask = &*(entry.ifa_netmask as *const libc::sockaddr_in);
            (
                u32::from_be(addr.sin_addr.s_addr),
                u32::from_be(mask.sin_addr.s_addr),
            )
        };
        // SAFETY: ifa_name is a NUL-terminated C string owned by the list.
        let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        out.push((name, Ipv4Addr::from(addr), mask.count_ones() as u8));
    }
    // SAFETY: `head` came from a successful getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(head) };
    Ok(out)
}

#[cfg(not(unix))]
pub fn raw_ipv4_addrs() -> Result<Vec<(String, Ipv4Addr, u8)>, String> {
    Err("interface discovery is implemented for unix platforms only".to_string())
}

fn run_text(program: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("running {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} {} exited {}: {}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// This node's interface facts. BLOCKING (runs `networksetup`, and `system_profiler` when a
/// Thunderbolt port carries an address — ~0.9 s measured); call from a blocking context.
///
/// A failed `system_profiler` read is not an error of the whole report: the TB interfaces
/// stay TB, their `link_speed` is absent, and the returned warning names why.
pub fn local_interface_facts() -> Result<(Vec<InterfaceFact>, Option<String>), String> {
    let addrs = raw_ipv4_addrs()?;
    if !cfg!(target_os = "macos") {
        return Ok((facts_from(&addrs, None, &HashMap::new()), None));
    }
    let ports = parse_hardware_ports(&run_text("networksetup", &["-listallhardwareports"])?);
    let tb_has_address = addrs.iter().any(|(device, _, _)| {
        ports
            .get(device)
            .is_some_and(|port| classify_hardware_port(port) == InterfaceKind::Thunderbolt)
    });
    let (speeds, warning) = if tb_has_address {
        match run_text("system_profiler", &["SPThunderboltDataType", "-json"])
            .and_then(|text| parse_thunderbolt_speeds(&text))
        {
            Ok(speeds) => (speeds, None),
            Err(e) => (
                HashMap::new(),
                Some(format!("Thunderbolt link speed unavailable: {e}")),
            ),
        }
    } else {
        (HashMap::new(), None)
    };
    Ok((facts_from(&addrs, Some(&ports), &speeds), warning))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MACBOOK_PORTS: &str = include_str!("../tests/fixtures/netpath/macbook-hardwareports.txt");
    const STUDIO_PORTS: &str = include_str!("../tests/fixtures/netpath/studio-hardwareports.txt");
    const MACBOOK_TB: &str = include_str!("../tests/fixtures/netpath/macbook-thunderbolt.json");
    const STUDIO_TB: &str = include_str!("../tests/fixtures/netpath/studio-thunderbolt.json");

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    /// The addresses `ifconfig` showed on each Mac on 2026-09-23 (the personal Tailscale's
    /// utun /32 and the Studio's APIPA en14 included on purpose: neither may be chosen).
    fn macbook_addrs() -> Vec<(String, Ipv4Addr, u8)> {
        vec![
            ("en3".into(), ip("192.168.0.1"), 30),
            ("en0".into(), ip("192.168.10.127"), 24),
            ("utun0".into(), ip("100.83.119.44"), 32),
        ]
    }

    fn studio_addrs() -> Vec<(String, Ipv4Addr, u8)> {
        vec![
            ("en3".into(), ip("192.168.0.2"), 30),
            ("en1".into(), ip("192.168.10.161"), 24),
            ("utun4".into(), ip("100.122.51.13"), 32),
            ("en14".into(), ip("169.254.250.146"), 16),
        ]
    }

    fn macbook() -> Vec<InterfaceFact> {
        let ports = parse_hardware_ports(MACBOOK_PORTS);
        facts_from(
            &macbook_addrs(),
            Some(&ports),
            &parse_thunderbolt_speeds(MACBOOK_TB).unwrap(),
        )
    }

    fn studio() -> Vec<InterfaceFact> {
        let ports = parse_hardware_ports(STUDIO_PORTS);
        facts_from(
            &studio_addrs(),
            Some(&ports),
            &parse_thunderbolt_speeds(STUDIO_TB).unwrap(),
        )
    }

    #[test]
    fn hardware_ports_name_the_thunderbolt_devices_on_both_macs() {
        let mb = parse_hardware_ports(MACBOOK_PORTS);
        assert_eq!(mb.get("en3").map(String::as_str), Some("Thunderbolt 3"));
        assert_eq!(mb.get("en0").map(String::as_str), Some("Wi-Fi"));
        assert_eq!(mb.get("utun0"), None, "a VPN tunnel is not a hardware port");
        let st = parse_hardware_ports(STUDIO_PORTS);
        assert_eq!(st.get("en3").map(String::as_str), Some("Thunderbolt 2"));
        assert_eq!(st.get("en0").map(String::as_str), Some("Ethernet"));
        assert_eq!(
            classify_hardware_port("Thunderbolt Bridge"),
            InterfaceKind::Thunderbolt
        );
        assert_eq!(
            classify_hardware_port("Ethernet Adapter (en4)"),
            InterfaceKind::Ethernet
        );
        assert_eq!(classify_hardware_port("iPhone USB"), InterfaceKind::Other);
    }

    #[test]
    fn only_connected_receptacles_report_a_speed() {
        let mb = parse_thunderbolt_speeds(MACBOOK_TB).unwrap();
        assert_eq!(
            mb,
            HashMap::from([("3".to_string(), "80 Gb/s".to_string())])
        );
        let st = parse_thunderbolt_speeds(STUDIO_TB).unwrap();
        assert_eq!(
            st,
            HashMap::from([("2".to_string(), "80 Gb/s".to_string())])
        );
        assert!(parse_thunderbolt_speeds("not json").is_err());
    }

    #[test]
    fn facts_keep_hardware_ports_only_and_attribute_the_tb_speed() {
        let facts = macbook();
        let devices: Vec<&str> = facts.iter().map(|f| f.device.as_str()).collect();
        assert_eq!(devices, ["en3", "en0"], "utun0 has no hardware port");
        assert_eq!(facts[0].kind, InterfaceKind::Thunderbolt);
        assert_eq!(facts[0].link_speed.as_deref(), Some("80 Gb/s"));
        assert_eq!(facts[1].kind, InterfaceKind::Wifi);
        assert_eq!(facts[1].link_speed, None);
    }

    #[test]
    fn the_tb5_pair_is_joined_by_thunderbolt_not_the_shared_wifi() {
        let path = choose_path(&macbook(), &studio()).expect("a direct path exists");
        assert_eq!(path.kind, LinkKind::Thunderbolt);
        assert_eq!(path.local.device, "en3");
        assert_eq!(path.peer.ipv4, ip("192.168.0.2"));
        assert_eq!(path.peer.hardware_port.as_deref(), Some("Thunderbolt 2"));
        // Symmetric from the Studio's side.
        let back = choose_path(&studio(), &macbook()).unwrap();
        assert_eq!(back.kind, LinkKind::Thunderbolt);
        assert_eq!(back.peer.ipv4, ip("192.168.0.1"));
    }

    /// NEGATIVE CONTROL: unplug the cable (drop both TB addresses) and the same pair must
    /// fall to the shared Wi-Fi LAN, labelled `network` — never still "thunderbolt".
    #[test]
    fn without_the_cable_the_pair_falls_to_the_lan() {
        let mb: Vec<_> = macbook()
            .into_iter()
            .filter(|f| f.device != "en3")
            .collect();
        let st: Vec<_> = studio().into_iter().filter(|f| f.device != "en3").collect();
        let path = choose_path(&mb, &st).unwrap();
        assert_eq!(path.kind, LinkKind::Network);
        assert_eq!(path.peer.ipv4, ip("192.168.10.161"));
    }

    #[test]
    fn no_shared_subnet_means_no_path_and_vpn_or_apipa_never_count() {
        let mb: Vec<_> = macbook()
            .into_iter()
            .filter(|f| f.kind == InterfaceKind::Thunderbolt)
            .collect();
        let lan_only: Vec<_> = studio()
            .into_iter()
            .filter(|f| f.kind == InterfaceKind::Wifi)
            .collect();
        assert_eq!(choose_path(&mb, &lan_only), None);

        let apipa = |dev: &str, addr: &str, kind| InterfaceFact {
            device: dev.into(),
            hardware_port: None,
            kind,
            ipv4: ip(addr),
            prefix_len: 16,
            link_speed: None,
        };
        assert_eq!(
            choose_path(
                &[apipa("en5", "169.254.1.1", InterfaceKind::Thunderbolt)],
                &[apipa("en6", "169.254.9.9", InterfaceKind::Thunderbolt)]
            ),
            None,
            "two self-assigned addresses prove no cable"
        );
        let vpn = |addr: &str| InterfaceFact {
            prefix_len: 24,
            ..apipa("utun3", addr, InterfaceKind::Other)
        };
        assert_eq!(choose_path(&[vpn("10.0.0.1")], &[vpn("10.0.0.2")]), None);
    }

    /// A TB port on one end facing an Ethernet port on the other in one subnet (a dock, a
    /// bridge) is a real path but not a Thunderbolt LINK: it is labelled `network`.
    #[test]
    fn a_link_is_thunderbolt_only_when_both_ends_are() {
        let mut st = studio();
        for fact in &mut st {
            if fact.device == "en3" {
                fact.kind = InterfaceKind::Ethernet;
            }
        }
        let path = choose_path(&macbook(), &st).unwrap();
        assert_eq!(path.kind, LinkKind::Network);
        assert_eq!(
            path.peer.ipv4,
            ip("192.168.0.2"),
            "the wired /30 still beats Wi-Fi"
        );
    }

    #[test]
    fn linux_names_classify_without_a_port_registry() {
        let facts = facts_from(
            &[
                ("thunderbolt0".into(), ip("10.9.0.1"), 30),
                ("tailscale0".into(), ip("100.64.0.3"), 32),
            ],
            None,
            &HashMap::new(),
        );
        assert_eq!(facts[0].kind, InterfaceKind::Thunderbolt);
        assert_eq!(facts[1].kind, InterfaceKind::Other);
    }

    #[cfg(unix)]
    #[test]
    fn getifaddrs_reports_no_loopback() {
        let addrs = raw_ipv4_addrs().unwrap();
        assert!(addrs.iter().all(|(_, a, _)| !a.is_loopback()));
    }
}
