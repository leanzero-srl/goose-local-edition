//! macOS local network privacy (Apple TN3179), NAMED where it bites.
//!
//! An app's process tree — goosed, the probe's `/sbin/ping`, the local rank — needs the Local
//! Network privilege for outgoing traffic to a Thunderbolt or LAN peer. A denied operation fails
//! with EHOSTUNREACH ("No route to host"), which on its own is indistinguishable from a dead
//! cable. The discriminator is the other side: the peer is probed over ssh, and TN3179 exempts
//! "command-line tools run from Terminal or over SSH, including any child processes they spawn",
//! so the peer's own ping back over the same link is the positive control. Tailscale (a VPN) and
//! loopback are not local-network addresses, so the ssh control path keeps working while the
//! data plane is refused.

use std::fmt::Write as _;

pub const CHECK_ID: &str = "localNetworkPermission";

pub const BLOCKED: &str = "macOS is blocking Goose Swarm from the local network — allow it in \
                           System Settings › Privacy & Security › Local Network";

/// Whether `text` (a ping error line, a rank's last output) names EHOSTUNREACH: the OS's own
/// strerror for it, `os error N`, or MLX's `(error: N)` (the spelling of the ring backend's
/// `Couldn't bind socket (error: 48)`, quoted by the portRange check).
pub fn names_host_unreachable(text: &str) -> bool {
    #[cfg(unix)]
    {
        let errno = libc::EHOSTUNREACH;
        let described = std::io::Error::from_raw_os_error(errno).to_string();
        let strerror = described
            .split(" (os error")
            .next()
            .unwrap_or(described.as_str());
        text.contains(strerror)
            || text.contains(&format!("os error {errno}"))
            || text.contains(&format!("(error: {errno})"))
    }
    #[cfg(not(unix))]
    {
        let _ = text;
        false
    }
}

/// One `@@ping` line: `<ip> ok`, `<ip> fail`, or `<ip> fail <ping's own error line>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingLine {
    pub ip: String,
    pub ok: bool,
    pub reason: Option<String>,
}

pub fn parse_ping_lines(text: &str) -> Vec<PingLine> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(3, ' ');
            let ip = parts.next()?.to_string();
            let ok = match parts.next()? {
                "ok" => true,
                "fail" => false,
                _ => return None,
            };
            let reason = parts
                .next()
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_string);
            Some(PingLine { ip, ok, reason })
        })
        .collect()
}

/// What preflight learned about one PEER of the node being diagnosed.
#[derive(Debug, Clone)]
pub struct PeerAnswer<'a> {
    pub name: &'a str,
    pub tb_ip: &'a str,
    /// Its probe answered over ssh (the control path works).
    pub answered: bool,
    /// Its own ping lines — run under ssh, exempt from the app's privilege.
    pub pings: &'a [PingLine],
}

/// The evidence that THIS Mac's app is refused the local network, or `None` when the facts do
/// not show it: every failed ping from here names EHOSTUNREACH, and at least one of those peers
/// answered over ssh AND reached this node's address over the same link from its side.
pub fn diagnose(local_tb_ip: &str, local: &[PingLine], peers: &[PeerAnswer]) -> Option<String> {
    let failed: Vec<&PingLine> = local.iter().filter(|p| !p.ok).collect();
    let unreachable: Vec<(&str, &str)> = failed
        .iter()
        .filter_map(|p| {
            p.reason
                .as_deref()
                .filter(|r| names_host_unreachable(r))
                .map(|r| (p.ip.as_str(), r))
        })
        .collect();
    if failed.is_empty() || unreachable.len() != failed.len() {
        return None;
    }
    let mut evidence = String::new();
    for (ip, reason) in unreachable {
        let Some(peer) = peers.iter().find(|p| p.tb_ip == ip) else {
            continue;
        };
        let pinged_back = peer.pings.iter().any(|p| p.ip == local_tb_ip && p.ok);
        if peer.answered && pinged_back {
            if !evidence.is_empty() {
                evidence.push_str("; ");
            }
            let _ = write!(
                evidence,
                "this Mac → {ip}: {reason}, while {} answered over ssh and reached {local_tb_ip} \
                 from its side",
                peer.name
            );
        }
    }
    (!evidence.is_empty()).then_some(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Measured 2026-09-24 on the MacBook (rank 0, 192.168.0.1) ↔ Mac Studio (192.168.0.2, ssh
    // `workhorse` over Tailscale): the app opened from Finder failed `ping` to 192.168.0.2 while
    // the Studio's probe (over ssh) answered `192.168.0.1 ok`.
    fn blocked_here() -> Vec<PingLine> {
        parse_ping_lines("192.168.0.2 fail ping: sendto: No route to host\n")
    }

    fn studio_pings() -> Vec<PingLine> {
        parse_ping_lines("192.168.0.1 ok\n")
    }

    #[test]
    fn ping_lines_keep_the_error_ping_printed() {
        assert_eq!(
            parse_ping_lines(
                "192.168.0.2 ok\n192.168.0.3 fail\n192.168.0.4 fail ping: sendto: Host is down\n"
            ),
            vec![
                PingLine {
                    ip: "192.168.0.2".into(),
                    ok: true,
                    reason: None
                },
                PingLine {
                    ip: "192.168.0.3".into(),
                    ok: false,
                    reason: None
                },
                PingLine {
                    ip: "192.168.0.4".into(),
                    ok: false,
                    reason: Some("ping: sendto: Host is down".into())
                },
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_signature_is_the_os_spelling_of_ehostunreach() {
        assert!(names_host_unreachable("ping: sendto: No route to host"));
        assert!(names_host_unreachable(
            "OSError: [Errno 65] No route to host"
        ));
        assert!(names_host_unreachable(
            "[ring] Couldn't connect (error: 65)"
        ));
        assert!(!names_host_unreachable("ping: sendto: Host is down"));
        assert!(!names_host_unreachable("Request timeout for icmp_seq 0"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refused_here_while_the_peer_reaches_us_over_the_same_link_is_named() {
        let studio = studio_pings();
        let peers = [PeerAnswer {
            name: "Work’s Mac Studio",
            tb_ip: "192.168.0.2",
            answered: true,
            pings: &studio,
        }];
        let evidence = diagnose("192.168.0.1", &blocked_here(), &peers).expect("named");
        assert!(evidence.contains("No route to host"), "{evidence}");
        assert!(
            evidence.contains("Work’s Mac Studio answered over ssh"),
            "{evidence}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_link_down_from_both_sides_is_not_blamed_on_the_privilege() {
        let studio = parse_ping_lines("192.168.0.1 fail ping: sendto: No route to host\n");
        let peers = [PeerAnswer {
            name: "studio",
            tb_ip: "192.168.0.2",
            answered: true,
            pings: &studio,
        }];
        assert_eq!(diagnose("192.168.0.1", &blocked_here(), &peers), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unreachable_peer_or_another_error_is_not_named() {
        let studio = studio_pings();
        let silent = [PeerAnswer {
            name: "studio",
            tb_ip: "192.168.0.2",
            answered: false,
            pings: &[],
        }];
        assert_eq!(diagnose("192.168.0.1", &blocked_here(), &silent), None);
        let answering = [PeerAnswer {
            name: "studio",
            tb_ip: "192.168.0.2",
            answered: true,
            pings: &studio,
        }];
        let timeout = parse_ping_lines("192.168.0.2 fail\n");
        assert_eq!(diagnose("192.168.0.1", &timeout, &answering), None);
        let passing = parse_ping_lines("192.168.0.2 ok\n");
        assert_eq!(diagnose("192.168.0.1", &passing, &answering), None);
    }
}
