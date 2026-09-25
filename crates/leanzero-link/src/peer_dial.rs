//! The ONE way this crate reaches a mesh peer: through the goose-owned `tailscaled`'s
//! loopback SOCKS5 listener.
//!
//! Why a proxy at all: the mesh daemon runs `--tun=userspace-networking` (no TUN device,
//! no root — the isolation law). In that mode the host kernel has NO route to tailnet
//! IPs; the daemon's netstack only carries traffic handed to it through its SOCKS5 or
//! outbound-HTTP proxy. <https://tailscale.com/kb/1112/userspace-networking>: "tailscaled
//! functions as a SOCKS5 or HTTP proxy which other processes in the container can
//! connect through". Measured 2026-09-23 on this Mac against a hermetic Headscale with
//! two userspace tailscaled 1.98.8 nodes: `route -n get 100.64.0.2` → `interface: en0`
//! (the LAN default route); a direct `curl http://100.64.0.2:<port>` from the host →
//! `Couldn't connect to server`; the same request with `--proxy socks5h://<A's
//! listener>` → the peer's loopback server answered 200; the same proxy to a port the
//! peer does not serve → SOCKS reply 1 (negative control). Inbound needs nothing: the
//! peer's netstack forwards tailnet TCP to its own loopback ([`crate::control::MeshBind`]).
//!
//! Every peer-dialing client in this crate is built HERE — the fabric's polls
//! ([`crate::state::PeerRegistry`]), its `/stream` WebSocket, the manager's
//! `/execute` and `/mlx/*` POSTs, and the chat relay's requests and in-flight looks
//! ([`crate::inference`]). A missing proxy is [`PeerDialError::NoMeshProxy`],
//! never a silent direct dial (which cannot reach the peer — and, on a machine whose
//! personal Tailscale holds a route for the same 100.x address, would reach a device on
//! the WRONG tailnet: this Mac's personal daemon installs per-peer /32s on utun0).

use std::net::SocketAddr;
use std::time::Duration;

use thiserror::Error;
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use tokio_socks::TargetAddr;
use tokio_tungstenite::WebSocketStream;
use url::{Host, Url};

/// The stderr line tailscaled prints for a kernel-chosen SOCKS5 port. Source:
/// `cmd/tailscaled/proxy.go` at v1.98.8, `outboundProxyListen`: when the
/// `--socks5-server` address ends in `:0` it runs `log.Printf("SOCKS5 listening on %v",
/// socksListener.Addr())` — "so integration tests can find it portably". Measured on the
/// Homebrew 1.98.8 binary under `--no-logs-no-support`:
/// `2026/09/23 23:03:35 SOCKS5 listening on 127.0.0.1:61235`.
pub const SOCKS5_LISTENING_MARKER: &str = "SOCKS5 listening on ";

#[derive(Debug, Error)]
pub enum PeerDialError {
    #[error(
        "no mesh proxy recorded for this node: its tailscaled runs in userspace-networking \
         mode, so the host has no route to mesh IPs and a peer is reachable only through \
         the daemon's SOCKS5 listener — refusing a direct dial"
    )]
    NoMeshProxy,
    #[error(
        "mesh proxy address {0} is not loopback — peer traffic is only ever routed through \
         this node's own tailscaled"
    )]
    NotLoopback(SocketAddr),
    #[error("cannot build the peer HTTP client over mesh proxy {proxy}: {source}")]
    Client {
        proxy: SocketAddr,
        source: reqwest::Error,
    },
    #[error("peer URL '{url}' has no {missing}")]
    BadUrl { url: String, missing: &'static str },
    #[error("SOCKS5 CONNECT to {target} through mesh proxy {proxy} failed: {source}")]
    Socks {
        proxy: SocketAddr,
        target: String,
        source: tokio_socks::Error,
    },
    #[error(
        "SOCKS5 CONNECT to {target} through mesh proxy {proxy} did not complete within {waited:?}"
    )]
    SocksTimeout {
        proxy: SocketAddr,
        target: String,
        waited: Duration,
    },
    #[error(
        "leanzero-link was built without a TLS backend: reqwest's SOCKS5 connector then \
         rejects every proxy connect — enable the `rustls-tls` or `native-tls` feature"
    )]
    NoTlsBackend,
    #[error("WebSocket handshake with {target} failed: {source}")]
    WebSocket {
        target: String,
        source: Box<tokio_tungstenite::tungstenite::Error>,
    },
}

/// Which timeout a peer HTTP client carries. The fabric's small polls are bounded in
/// TOTAL (slowness is the signal); the `/execute` and `/mlx/*` proxy POSTs are bounded
/// only in CONNECT, because the peer's work runs as long as it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerTimeout {
    Total(Duration),
    ConnectOnly(Duration),
    /// A LIVENESS LOOK (the chat relay's in-flight watch): bounded in TOTAL — the dial and
    /// the peer's in-memory answer — and never over a pooled connection. A kept-alive
    /// connection to a peer whose Link died stays open and silent through the local
    /// daemon (r3-1.log 12:14:59: the request in flight hung 120 s), so only a fresh dial
    /// can tell whether the peer is still there.
    FreshTotal(Duration),
}

/// The goose-owned tailscaled's SOCKS5 listener — loopback by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshProxy {
    socks5: SocketAddr,
}

impl MeshProxy {
    pub fn socks5(addr: SocketAddr) -> Result<Self, PeerDialError> {
        if !addr.ip().is_loopback() {
            return Err(PeerDialError::NotLoopback(addr));
        }
        Ok(Self { socks5: addr })
    }

    /// The listener address from one tailscaled stderr line, when the line is the
    /// [`SOCKS5_LISTENING_MARKER`] report (any log prefix before it is ignored).
    pub fn from_tailscaled_log_line(line: &str) -> Option<Result<Self, PeerDialError>> {
        let (_, rest) = line.split_once(SOCKS5_LISTENING_MARKER)?;
        let addr: SocketAddr = rest.trim().parse().ok()?;
        Some(Self::socks5(addr))
    }

    pub fn addr(&self) -> SocketAddr {
        self.socks5
    }

    /// `socks5h`: the target name is resolved by tailscaled (MagicDNS-capable), never by
    /// the host resolver. For the IP-literal URLs the fabric builds the two are identical.
    pub fn proxy_url(&self) -> String {
        format!("socks5h://{}", self.socks5)
    }

    /// THE peer HTTP client constructor. Every request goes through the mesh proxy; the
    /// environment's `HTTP_PROXY`/`ALL_PROXY` are not consulted (an explicit proxy turns
    /// reqwest's system-proxy lookup off), so nothing can divert a peer call.
    pub fn http_client(&self, timeout: PeerTimeout) -> Result<reqwest::Client, PeerDialError> {
        if !cfg!(any(feature = "rustls-tls", feature = "native-tls")) {
            return Err(PeerDialError::NoTlsBackend);
        }
        let client_err = |source| PeerDialError::Client {
            proxy: self.socks5,
            source,
        };
        let proxy = reqwest::Proxy::all(self.proxy_url()).map_err(client_err)?;
        let builder = reqwest::Client::builder().proxy(proxy);
        let builder = match timeout {
            PeerTimeout::Total(limit) => builder.timeout(limit),
            PeerTimeout::ConnectOnly(limit) => builder.connect_timeout(limit),
            PeerTimeout::FreshTotal(limit) => builder.timeout(limit).pool_max_idle_per_host(0),
        };
        builder.build().map_err(client_err)
    }

    /// A TCP stream to `url`'s host:port tunnelled through the mesh proxy, bounded by
    /// `connect_timeout` (transport only: reaching the peer, never the peer's work).
    pub async fn tcp_stream(
        &self,
        url: &Url,
        connect_timeout: Duration,
    ) -> Result<TcpStream, PeerDialError> {
        let port = url
            .port_or_known_default()
            .ok_or_else(|| PeerDialError::BadUrl {
                url: redacted(url),
                missing: "port",
            })?;
        let target: TargetAddr<'static> = match url.host() {
            Some(Host::Ipv4(ip)) => TargetAddr::Ip(SocketAddr::new(ip.into(), port)),
            Some(Host::Ipv6(ip)) => TargetAddr::Ip(SocketAddr::new(ip.into(), port)),
            Some(Host::Domain(name)) => TargetAddr::Domain(name.to_string().into(), port),
            None => {
                return Err(PeerDialError::BadUrl {
                    url: redacted(url),
                    missing: "host",
                })
            }
        };
        let target_text = target_text(&target);
        match tokio::time::timeout(connect_timeout, Socks5Stream::connect(self.socks5, target))
            .await
        {
            Err(_) => Err(PeerDialError::SocksTimeout {
                proxy: self.socks5,
                target: target_text,
                waited: connect_timeout,
            }),
            Ok(Err(source)) => Err(PeerDialError::Socks {
                proxy: self.socks5,
                target: target_text,
                source,
            }),
            Ok(Ok(stream)) => Ok(stream.into_inner()),
        }
    }

    /// A `ws://` client connection to a peer through the mesh proxy.
    pub async fn websocket(
        &self,
        url: &str,
        connect_timeout: Duration,
    ) -> Result<WebSocketStream<TcpStream>, PeerDialError> {
        let parsed = Url::parse(url).map_err(|_| PeerDialError::BadUrl {
            url: "<unparseable>".to_string(),
            missing: "valid syntax",
        })?;
        let stream = self.tcp_stream(&parsed, connect_timeout).await?;
        let (ws, _response) =
            tokio_tungstenite::client_async(url, stream)
                .await
                .map_err(|source| PeerDialError::WebSocket {
                    target: redacted(&parsed),
                    source: Box::new(source),
                })?;
        Ok(ws)
    }
}

/// [`MeshProxy::http_client`] over an optional proxy: absence is
/// [`PeerDialError::NoMeshProxy`], never a client that dials directly.
pub fn peer_http_client(
    proxy: Option<MeshProxy>,
    timeout: PeerTimeout,
) -> Result<reqwest::Client, PeerDialError> {
    proxy
        .ok_or(PeerDialError::NoMeshProxy)?
        .http_client(timeout)
}

/// `scheme://host:port` only — the `/stream` URL carries the node token in its query.
fn redacted(url: &Url) -> String {
    format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap_or("<no host>"),
        url.port().map(|p| format!(":{p}")).unwrap_or_default()
    )
}

fn target_text(target: &TargetAddr<'_>) -> String {
    match target {
        TargetAddr::Ip(addr) => addr.to_string(),
        TargetAddr::Domain(name, port) => format!("{name}:{port}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listener_line_tailscaled_prints_is_parsed() {
        let proxy = MeshProxy::from_tailscaled_log_line(
            "2026/09/23 23:03:35 SOCKS5 listening on 127.0.0.1:61235",
        )
        .expect("the marker line")
        .expect("a loopback address");
        assert_eq!(proxy.addr(), "127.0.0.1:61235".parse().unwrap());
        assert_eq!(proxy.proxy_url(), "socks5h://127.0.0.1:61235");
    }

    #[test]
    fn other_lines_are_not_a_listener_report() {
        for line in [
            "2026/09/23 23:03:35 logtail started",
            "2026/09/23 23:03:35 HTTP proxy listening on 127.0.0.1:61236",
            "2026/09/23 23:03:35 SOCKS5 listening on not-an-address",
        ] {
            assert!(
                MeshProxy::from_tailscaled_log_line(line).is_none(),
                "{line}"
            );
        }
    }

    #[test]
    fn a_non_loopback_listener_is_refused() {
        let err = MeshProxy::from_tailscaled_log_line("SOCKS5 listening on 0.0.0.0:1055")
            .expect("the marker line")
            .unwrap_err();
        assert!(matches!(err, PeerDialError::NotLoopback(_)), "{err}");
        assert!(MeshProxy::socks5("192.168.1.10:1055".parse().unwrap()).is_err());
        assert!(MeshProxy::socks5("[::1]:1055".parse().unwrap()).is_ok());
    }

    #[test]
    fn no_proxy_is_a_named_error_not_a_direct_client() {
        let err = peer_http_client(None, PeerTimeout::Total(Duration::from_secs(1))).unwrap_err();
        assert!(matches!(err, PeerDialError::NoMeshProxy), "{err}");
        assert!(err.to_string().contains("refusing a direct dial"));
    }

    #[test]
    fn redaction_drops_the_query_token() {
        let url = Url::parse("ws://100.64.0.2:41226/v1/swarm/stream?token=secret").unwrap();
        assert_eq!(redacted(&url), "ws://100.64.0.2:41226");
    }
}
