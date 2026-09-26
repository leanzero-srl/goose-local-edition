//! Which road reaches a LeanZero server whose name is a Tailscale `*.ts.net` host — the
//! auth worker (`https://<node>.<tailnet>.ts.net/leanzero-link`) and the Headscale control
//! plane behind the same Funnel at `/`.
//!
//! A `*.ts.net` name has TWO doors. Devices OFF the tailnet reach it through Tailscale
//! Funnel's public relays (public DNS answers the relay IPs). Devices ON the same tailnet
//! reach the node itself at its tailnet address — the address MagicDNS answers.
//! <https://tailscale.com/kb/1223/funnel>: Funnel "routes traffic from the broader
//! internet to a local service"; <https://tailscale.com/kb/1081/magicdns>: every device on
//! the tailnet resolves `<machine>.<tailnet>.ts.net` through the MagicDNS resolver at
//! `100.100.100.100`; <https://tailscale.com/kb/1015/100.x-addresses>: tailnet IPv4
//! addresses are drawn from `100.64.0.0/10`.
//!
//! Why this module exists (Q-137, measured 2026-09-26): the Studio's Funnel ingress died
//! after a LAN change (every Funnel port failed the TLS handshake with SSL_ERROR_SYSCALL,
//! from both Macs) while the tailnet stayed healthy — `curl --resolve
//! worksmacstudio.tailfc4700.ts.net:443:100.122.51.13` answered from both Macs. Neither
//! Mac's SYSTEM resolver asks MagicDNS for the tailnet's domain (the Homebrew tailscaled
//! writes only `/etc/resolver/search.tailscale`, a search list with no nameserver, and
//! `scutil --dns` shows it "Not Reachable"), so the name always resolved to the Funnel
//! relays (185.40.234.x) and Link's whole control plane rode the one public door.
//! `dig @100.100.100.100 worksmacstudio.tailfc4700.ts.net` answered `100.122.51.13` on
//! both Macs in 0 ms, and `example.com` through the same resolver answered public IPs —
//! the negative control: an answer is a tailnet route ONLY when it is a tailnet address.
//!
//! The isolation invariant holds: this module never opens the personal Tailscale
//! daemon's socket, state or CLI. It sends one DNS query to the documented MagicDNS
//! address — the same packet the system resolver sends when MagicDNS is wired — and
//! dials the answered address with the ORIGINAL name kept for TLS SNI and `Host`.
//!
//! There is no silent fallback: every decision is a [`RoutePath`] with its reason,
//! recorded in a [`RouteSlot`] the Link tab shows and logged when it changes. A pinned
//! tailnet dial that fails is reported as that failure — never retried through Funnel.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

/// The MagicDNS resolver every tailnet device serves
/// (<https://tailscale.com/kb/1081/magicdns>).
pub const MAGICDNS_RESOLVER: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(100, 100, 100, 100)), 53);

/// Transport bound on one MagicDNS query. measured: `dig @100.100.100.100` answered in
/// 0 ms on both Macs (2026-09-26); the resolver lives inside the local tailscaled, so a
/// query that has not answered in a second is a Mac with no tailnet, not a slow one.
const MAGICDNS_TIMEOUT: Duration = Duration::from_secs(1);

/// The DNS suffix of every Tailscale-issued machine name
/// (<https://tailscale.com/kb/1217/tailnet-name>).
pub const TAILNET_DNS_SUFFIX: &str = ".ts.net";

pub fn is_tailnet_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host.len() > TAILNET_DNS_SUFFIX.len() && host.ends_with(TAILNET_DNS_SUFFIX)
}

/// `100.64.0.0/10` — the range Tailscale assigns tailnet IPv4 addresses from.
pub fn is_tailscale_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

/// The machine label of a tailnet name (`worksmacstudio` of
/// `worksmacstudio.tailfc4700.ts.net`) — how the node is named in a user-facing line.
pub fn machine_label(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}

/// The road one request took (or will take) to `host`. Serde tag `path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "path", rename_all = "snake_case")]
pub enum RoutePath {
    /// Dialed at the node's tailnet address; TLS SNI and `Host` stay the name.
    Tailnet { ip: Ipv4Addr },
    /// Resolved by the system resolver — for a `*.ts.net` name that is Tailscale Funnel's
    /// public relays. `reason` says why the tailnet road was not taken.
    Public { reason: String },
}

impl std::fmt::Display for RoutePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutePath::Tailnet { ip } => write!(f, "over the tailnet at {ip}"),
            RoutePath::Public { reason } => write!(f, "by public DNS ({reason})"),
        }
    }
}

/// The last road taken to one server and what happened on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteReport {
    pub host: String,
    #[serde(flatten)]
    pub path: RoutePath,
    pub decided_at: DateTime<Utc>,
    /// The failure of the last request on this road; `None` once one succeeds.
    #[serde(default)]
    pub last_failure: Option<String>,
}

/// A shared cell holding the last [`RouteReport`] for one server. Clones share the cell,
/// so a config template cloned per connect still reports into the manager's view.
#[derive(Debug, Clone, Default)]
pub struct RouteSlot(Arc<StdMutex<Option<RouteReport>>>);

impl RouteSlot {
    pub fn get(&self) -> Option<RouteReport> {
        self.0.lock().unwrap().clone()
    }

    /// Record a decision; logs only when the road differs from the last one recorded.
    pub fn record(&self, what: &'static str, host: &str, path: &RoutePath) {
        let mut slot = self.0.lock().unwrap();
        let changed = slot
            .as_ref()
            .is_none_or(|last| last.host != host || last.path != *path);
        if changed {
            match path {
                RoutePath::Tailnet { ip } => {
                    tracing::info!(server = what, host, %ip, "leanzero-link: route over the tailnet")
                }
                RoutePath::Public { reason } => {
                    tracing::info!(server = what, host, reason = %reason, "leanzero-link: route by public DNS")
                }
            }
        }
        let last_failure = if changed {
            None
        } else {
            slot.as_ref().and_then(|last| last.last_failure.clone())
        };
        *slot = Some(RouteReport {
            host: host.to_string(),
            path: path.clone(),
            decided_at: Utc::now(),
            last_failure,
        });
    }

    pub fn record_outcome(&self, failure: Option<String>) {
        if let Some(report) = self.0.lock().unwrap().as_mut() {
            report.last_failure = failure;
        }
    }
}

/// Asks MagicDNS for a `*.ts.net` name and decides the road. The resolver address, the
/// timeout and the accepted-address predicate are injectable so tests run a fake DNS
/// server on loopback; production uses [`TailnetResolver::default`].
#[derive(Debug, Clone, Copy)]
pub struct TailnetResolver {
    server: SocketAddr,
    timeout: Duration,
    is_tailnet_addr: fn(Ipv4Addr) -> bool,
}

impl Default for TailnetResolver {
    fn default() -> Self {
        Self {
            server: MAGICDNS_RESOLVER,
            timeout: MAGICDNS_TIMEOUT,
            is_tailnet_addr: is_tailscale_ipv4,
        }
    }
}

impl TailnetResolver {
    pub fn with_server(
        server: SocketAddr,
        timeout: Duration,
        is_tailnet_addr: fn(Ipv4Addr) -> bool,
    ) -> Self {
        Self {
            server,
            timeout,
            is_tailnet_addr,
        }
    }

    pub async fn route(&self, host: &str) -> RoutePath {
        if !is_tailnet_hostname(host) {
            return RoutePath::Public {
                reason: format!("{host} is not a Tailscale *.ts.net name"),
            };
        }
        let answer = match tokio::time::timeout(self.timeout, query_a(self.server, host)).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(err)) => {
                return RoutePath::Public {
                    reason: format!(
                        "this Mac's MagicDNS ({}) could not be asked for {host}: {err} — \
                         this Mac is not on a tailnet",
                        self.server.ip()
                    ),
                }
            }
            Err(_) => {
                return RoutePath::Public {
                    reason: format!(
                        "this Mac's MagicDNS ({}) did not answer for {host} within {:?} — \
                         this Mac is not on a tailnet, or its Tailscale is off",
                        self.server.ip(),
                        self.timeout
                    ),
                }
            }
        };
        match answer {
            DnsAnswer::NxDomain => RoutePath::Public {
                reason: format!(
                    "MagicDNS knows no node named {host} — this Mac is on a different tailnet"
                ),
            },
            DnsAnswer::Addrs(addrs) => match addrs.iter().find(|ip| (self.is_tailnet_addr)(**ip)) {
                Some(ip) => RoutePath::Tailnet { ip: *ip },
                None if addrs.is_empty() => RoutePath::Public {
                    reason: format!("MagicDNS returned no IPv4 address for {host}"),
                },
                None => RoutePath::Public {
                    reason: format!(
                        "MagicDNS answered {} for {host} — not tailnet addresses, so {host} \
                         is not a node on this Mac's tailnet",
                        addrs
                            .iter()
                            .map(Ipv4Addr::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsAnswer {
    Addrs(Vec<Ipv4Addr>),
    NxDomain,
}

async fn query_a(server: SocketAddr, host: &str) -> Result<DnsAnswer, String> {
    let id: u16 = rand::random();
    let query = build_a_query(id, host)?;
    let socket = UdpSocket::bind(SocketAddr::new(
        std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        0,
    ))
    .await
    .map_err(|err| format!("bind: {err}"))?;
    socket
        .connect(server)
        .await
        .map_err(|err| format!("connect: {err}"))?;
    socket
        .send(&query)
        .await
        .map_err(|err| format!("send: {err}"))?;
    let mut buf = [0u8; 1500];
    loop {
        let n = socket
            .recv(&mut buf)
            .await
            .map_err(|err| format!("recv: {err}"))?;
        match parse_a_response(id, &buf[..n]) {
            // A datagram for another id is a stray; keep waiting for ours.
            Err(ParseError::WrongId) => continue,
            Err(ParseError::Malformed(reason)) => return Err(reason),
            Ok(answer) => return Ok(answer),
        }
    }
}

/// An RFC 1035 §4.1 query: one question, `QTYPE=A`, `QCLASS=IN`, recursion desired.
pub fn build_a_query(id: u16, host: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(18 + host.len());
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("'{host}' is not a valid DNS name"));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&[0, 1, 0, 1]);
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    WrongId,
    Malformed(String),
}

pub fn parse_a_response(id: u16, msg: &[u8]) -> Result<DnsAnswer, ParseError> {
    let malformed = |what: &str| ParseError::Malformed(format!("malformed DNS answer: {what}"));
    if msg.len() < 12 {
        return Err(malformed("shorter than a header"));
    }
    if u16::from_be_bytes([msg[0], msg[1]]) != id {
        return Err(ParseError::WrongId);
    }
    if msg[2] & 0x80 == 0 {
        return Err(malformed("not a response"));
    }
    let rcode = msg[3] & 0x0f;
    if rcode == 3 {
        return Ok(DnsAnswer::NxDomain);
    }
    if rcode != 0 {
        return Err(ParseError::Malformed(format!(
            "DNS server answered RCODE {rcode}"
        )));
    }
    let qdcount = u16::from_be_bytes([msg[4], msg[5]]);
    let ancount = u16::from_be_bytes([msg[6], msg[7]]);
    let mut pos = 12;
    for _ in 0..qdcount {
        pos = skip_name(msg, pos).ok_or_else(|| malformed("question name"))? + 4;
    }
    let mut addrs = Vec::new();
    for _ in 0..ancount {
        pos = skip_name(msg, pos).ok_or_else(|| malformed("answer name"))?;
        let fixed = msg
            .get(pos..pos + 10)
            .ok_or_else(|| malformed("answer header"))?;
        let rtype = u16::from_be_bytes([fixed[0], fixed[1]]);
        let rclass = u16::from_be_bytes([fixed[2], fixed[3]]);
        let rdlen = u16::from_be_bytes([fixed[8], fixed[9]]) as usize;
        pos += 10;
        let rdata = msg
            .get(pos..pos + rdlen)
            .ok_or_else(|| malformed("rdata"))?;
        if rtype == 1 && rclass == 1 && rdlen == 4 {
            addrs.push(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]));
        }
        pos += rdlen;
    }
    Ok(DnsAnswer::Addrs(addrs))
}

/// The offset just past the (possibly compressed) name starting at `pos`.
fn skip_name(msg: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *msg.get(pos)?;
        match len {
            0 => return Some(pos + 1),
            l if l & 0xc0 == 0xc0 => {
                msg.get(pos + 1)?;
                return Some(pos + 2);
            }
            l => pos += 1 + l as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canned response: the query echoed back with QR set, then A records whose name
    /// is a compression pointer to the question (offset 12), as real resolvers answer.
    fn a_response(query: &[u8], rcode: u8, addrs: &[Ipv4Addr]) -> Vec<u8> {
        let mut out = query.to_vec();
        out[2] |= 0x80;
        out[3] = 0x80 | rcode;
        out[6..8].copy_from_slice(&(addrs.len() as u16).to_be_bytes());
        for ip in addrs {
            out.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
            out.extend_from_slice(&ip.octets());
        }
        out
    }

    #[test]
    fn tailnet_names_and_addresses() {
        assert!(is_tailnet_hostname("worksmacstudio.tailfc4700.ts.net"));
        assert!(is_tailnet_hostname("WorksMacStudio.tailfc4700.ts.net."));
        assert!(!is_tailnet_hostname("ts.net"));
        assert!(!is_tailnet_hostname("example.com"));
        assert!(!is_tailnet_hostname("127.0.0.1"));
        assert!(is_tailscale_ipv4(Ipv4Addr::new(100, 122, 51, 13)));
        assert!(is_tailscale_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_tailscale_ipv4(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_tailscale_ipv4(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_tailscale_ipv4(Ipv4Addr::new(185, 40, 234, 198)));
        assert_eq!(
            machine_label("worksmacstudio.tailfc4700.ts.net"),
            "worksmacstudio"
        );
    }

    #[test]
    fn query_wire_format_matches_rfc1035() {
        let q = build_a_query(0xabcd, "a.bc.ts.net").unwrap();
        assert_eq!(&q[..12], &[0xab, 0xcd, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[12..], b"\x01a\x02bc\x02ts\x03net\x00\x00\x01\x00\x01");
        assert!(build_a_query(1, "a..b").is_err());
    }

    #[test]
    fn response_parsing_reads_a_records_and_rcodes() {
        let q = build_a_query(7, "worksmacstudio.tailfc4700.ts.net").unwrap();
        let ip = Ipv4Addr::new(100, 122, 51, 13);
        assert_eq!(
            parse_a_response(7, &a_response(&q, 0, &[ip])),
            Ok(DnsAnswer::Addrs(vec![ip]))
        );
        assert_eq!(
            parse_a_response(7, &a_response(&q, 3, &[])),
            Ok(DnsAnswer::NxDomain)
        );
        assert_eq!(
            parse_a_response(8, &a_response(&q, 0, &[ip])),
            Err(ParseError::WrongId)
        );
        assert!(matches!(
            parse_a_response(7, &a_response(&q, 2, &[])),
            Err(ParseError::Malformed(_))
        ));
        let mut truncated = a_response(&q, 0, &[ip]);
        truncated.truncate(truncated.len() - 2);
        assert!(matches!(
            parse_a_response(7, &truncated),
            Err(ParseError::Malformed(_))
        ));
    }

    /// A loopback DNS server answering every query with `rcode` and `addrs`.
    async fn fake_dns(rcode: u8, addrs: Vec<Ipv4Addr>) -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
                let reply = a_response(&buf[..n], rcode, &addrs);
                let _ = socket.send_to(&reply, peer).await;
            }
        });
        addr
    }

    fn resolver(server: SocketAddr) -> TailnetResolver {
        TailnetResolver::with_server(server, Duration::from_millis(500), is_tailscale_ipv4)
    }

    #[tokio::test]
    async fn a_tailnet_answer_pins_the_tailnet_road() {
        let server = fake_dns(0, vec![Ipv4Addr::new(100, 122, 51, 13)]).await;
        assert_eq!(
            resolver(server)
                .route("worksmacstudio.tailfc4700.ts.net")
                .await,
            RoutePath::Tailnet {
                ip: Ipv4Addr::new(100, 122, 51, 13)
            }
        );
    }

    /// The negative control measured on 2026-09-26: MagicDNS forwards names it does not
    /// own upstream, so a PUBLIC answer (the Funnel relays) must never become a pin.
    #[tokio::test]
    async fn a_public_answer_is_not_a_tailnet_road() {
        let server = fake_dns(0, vec![Ipv4Addr::new(185, 40, 234, 198)]).await;
        let path = resolver(server).route("other.tailzzzz.ts.net").await;
        let RoutePath::Public { reason } = path else {
            panic!("a public answer pinned a tailnet road: {path:?}");
        };
        assert!(reason.contains("185.40.234.198"), "{reason}");
        assert!(
            reason.contains("not a node on this Mac's tailnet"),
            "{reason}"
        );
    }

    #[tokio::test]
    async fn nxdomain_silence_and_non_tailnet_names_each_say_why() {
        let nx = fake_dns(3, vec![]).await;
        let RoutePath::Public { reason } = resolver(nx).route("gone.tailfc4700.ts.net").await
        else {
            panic!("NXDOMAIN pinned a road");
        };
        assert!(reason.contains("knows no node named"), "{reason}");

        // A bound socket that never answers: the query times out.
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let RoutePath::Public { reason } = resolver(silent.local_addr().unwrap())
            .route("studio.tailfc4700.ts.net")
            .await
        else {
            panic!("silence pinned a road");
        };
        assert!(reason.contains("did not answer"), "{reason}");

        // Not a ts.net name: no query is sent at all (the silent server would time out).
        let RoutePath::Public { reason } = resolver(silent.local_addr().unwrap())
            .route("127.0.0.1")
            .await
        else {
            panic!("an IP literal pinned a road");
        };
        assert!(reason.contains("not a Tailscale"), "{reason}");
    }

    #[test]
    fn a_route_record_keeps_the_failure_until_the_road_changes() {
        let slot = RouteSlot::default();
        let tailnet = RoutePath::Tailnet {
            ip: Ipv4Addr::new(100, 122, 51, 13),
        };
        slot.record("worker", "h.t.ts.net", &tailnet);
        slot.record_outcome(Some("tls: reset".to_string()));
        slot.record("worker", "h.t.ts.net", &tailnet);
        assert_eq!(
            slot.get().unwrap().last_failure.as_deref(),
            Some("tls: reset")
        );
        slot.record(
            "worker",
            "h.t.ts.net",
            &RoutePath::Public {
                reason: "x".to_string(),
            },
        );
        assert_eq!(slot.get().unwrap().last_failure, None);
    }
}
