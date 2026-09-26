//! The goose-owned `tailscaled`'s road to a Headscale control plane at a `*.ts.net`
//! name: a loopback HTTP CONNECT proxy handed to the daemon as `HTTPS_PROXY`, which dials
//! the control host at its tailnet address when this Mac is on that node's tailnet
//! ([`crate::tailnet_route`]) and by public DNS (Funnel) otherwise.
//!
//! Why a proxy: the daemon resolves its `--login-server` with the SYSTEM resolver, and
//! on both Macs that resolver never asks MagicDNS for `*.ts.net` (Q-137, measured
//! 2026-09-26) — so a dead Funnel took the mesh's control plane down even though
//! `https://worksmacstudio.tailfc4700.ts.net/key?v=116` answered 200 over the tailnet.
//! A CONNECT proxy keeps TLS end to end: the daemon still verifies the certificate for
//! the NAME (SNI), only the TCP destination changes.
//!
//! tailscaled honours `HTTPS_PROXY` on its control and DERP dials — measured in source
//! at v1.98.8: `control/controlhttp/client.go` `tryURLUpgrade` sets
//! `tr.Proxy = a.getProxyFunc()` (= `feature.HookProxyFromEnvironment`, registered by
//! `feature/useproxy` as `tshttpproxy.ProxyFromEnvironment`, which reads the environment
//! through `httpproxy.FromEnvironment`), `control/controlclient/direct.go` sets the same
//! hook on its HTTP transport, and `derp/derphttp/derphttp_client.go` `dialNode` CONNECTs
//! through it. Every CONNECT that is not for the control host is relayed to its target
//! exactly as asked (a direct dial — what the daemon would have done with no proxy).

use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::tailnet_route::{RoutePath, RouteSlot, TailnetResolver};

/// Largest CONNECT request head accepted (request line + headers) — a guard against a
/// client that never ends its head; Go's `http.Transport` sends well under 1 KiB.
const MAX_HEAD_BYTES: usize = 16 * 1024; // ratio: 1/64 of Go's http.DefaultMaxHeaderBytes (1 MiB)

/// A running proxy; dropping it stops the accept loop.
#[derive(Debug)]
pub struct ControlProxy {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for ControlProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ControlProxy {
    /// Bind `127.0.0.1:0` (the kernel picks the port, held from here on so nothing can
    /// take it before the daemon starts) and serve CONNECTs.
    pub async fn start(
        control_host: String,
        resolver: TailnetResolver,
        slot: RouteSlot,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let task = tokio::spawn(async move {
            loop {
                let stream = match listener.accept().await {
                    Ok((stream, _)) => stream,
                    Err(err) => {
                        tracing::warn!(error = %err, "leanzero-link: control proxy accept failed");
                        tokio::task::yield_now().await;
                        continue;
                    }
                };
                let control_host = control_host.clone();
                let slot = slot.clone();
                tokio::spawn(async move {
                    serve(stream, &control_host, resolver, &slot).await;
                });
            }
        });
        Ok(Self { addr, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The value for the daemon's `HTTPS_PROXY`.
    pub fn proxy_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

async fn serve(
    mut client: TcpStream,
    control_host: &str,
    resolver: TailnetResolver,
    slot: &RouteSlot,
) {
    let (target, leftover) = match read_connect_head(&mut client).await {
        Ok(parsed) => parsed,
        Err(status) => {
            let _ = client
                .write_all(format!("HTTP/1.1 {status}\r\n\r\n").as_bytes())
                .await;
            return;
        }
    };
    let Some((host, port)) = split_host_port(&target) else {
        let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        return;
    };
    let control_route = if host.eq_ignore_ascii_case(control_host) {
        let route = resolver.route(&host).await;
        slot.record("control", &host, &route);
        Some(route)
    } else {
        None
    };
    let dialed = match &control_route {
        Some(RoutePath::Tailnet { ip }) => TcpStream::connect((*ip, port)).await,
        _ => TcpStream::connect((host.as_str(), port)).await,
    };
    if let (Some(route), Err(err)) = (&control_route, &dialed) {
        slot.record_outcome(Some(format!("could not open {target} {route}: {err}")));
    }
    let mut upstream = match dialed {
        Ok(upstream) => upstream,
        Err(_) => {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            return;
        }
    };
    if client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .is_err()
    {
        return;
    }
    if !leftover.is_empty() && upstream.write_all(&leftover).await.is_err() {
        return;
    }
    let copied = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    if let Some(route) = control_route {
        // A TCP connect is not an answer: Funnel's dead ingress ACCEPTED connections and
        // then closed them mid-handshake (curl: SSL_ERROR_SYSCALL). Zero bytes back from
        // the server is that failure, and it is named, not counted as a success.
        let outcome = match copied {
            Ok((_, 0)) => Some(format!(
                "{target} {route} accepted the connection and closed it without sending \
                 a byte (a TLS handshake that got no answer)"
            )),
            Ok(_) => None,
            Err(err) => Some(format!("relay to {target} {route} failed: {err}")),
        };
        slot.record_outcome(outcome);
    }
}

/// Read the CONNECT head; returns the target and any bytes read past the head.
async fn read_connect_head(client: &mut TcpStream) -> Result<(String, Vec<u8>), &'static str> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        if let Some(pos) = find_head_end(&buf) {
            break pos;
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err("431 Request Header Fields Too Large");
        }
        let n = client
            .read(&mut chunk)
            .await
            .map_err(|_| "400 Bad Request")?;
        if n == 0 {
            return Err("400 Bad Request");
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]);
    let request_line = head.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err("400 Bad Request");
    };
    if !method.eq_ignore_ascii_case("CONNECT") {
        return Err("405 Method Not Allowed");
    }
    Ok((target.to_string(), buf[head_end + 4..].to_vec()))
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// `host:port` or `[v6]:port` → (host without brackets, port).
pub fn split_host_port(target: &str) -> Option<(String, u16)> {
    let (host, port) = target.rsplit_once(':')?;
    let port = port.parse().ok()?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    (!host.is_empty()).then(|| (host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::time::Duration;

    #[test]
    fn connect_targets_split() {
        assert_eq!(
            split_host_port("worksmacstudio.tailfc4700.ts.net:443"),
            Some(("worksmacstudio.tailfc4700.ts.net".to_string(), 443))
        );
        assert_eq!(split_host_port("[::1]:80"), Some(("::1".to_string(), 80)));
        assert_eq!(split_host_port("nohost"), None);
        assert_eq!(split_host_port(":443"), None);
    }

    /// A loopback DNS server answering every A query with `ip`.
    async fn fake_dns(ip: Ipv4Addr) -> SocketAddr {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
                let mut out = buf[..n].to_vec();
                out[2] |= 0x80;
                out[3] = 0x80;
                out[6..8].copy_from_slice(&1u16.to_be_bytes());
                out.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
                out.extend_from_slice(&ip.octets());
                let _ = socket.send_to(&out, peer).await;
            }
        });
        addr
    }

    async fn connect_through(proxy: SocketAddr, target: &str) -> (TcpStream, String) {
        let mut stream = TcpStream::connect(proxy).await.unwrap();
        stream
            .write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while find_head_end(&head).is_none() {
            stream.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
        }
        (stream, String::from_utf8(head).unwrap())
    }

    /// The control host is dialed at the address MagicDNS answered — here loopback, which
    /// the injected predicate accepts in place of 100.64/10 — and bytes flow both ways.
    #[tokio::test]
    async fn the_control_host_is_dialed_at_its_tailnet_address() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = upstream.accept().await.unwrap();
            let mut buf = [0u8; 4];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(b"pong").await.unwrap();
        });
        let dns = fake_dns(Ipv4Addr::LOCALHOST).await;
        let resolver =
            TailnetResolver::with_server(dns, Duration::from_millis(500), |ip| ip.is_loopback());
        let slot = RouteSlot::default();
        let proxy = ControlProxy::start("hs.tailtest.ts.net".to_string(), resolver, slot.clone())
            .await
            .unwrap();
        // The name does not exist in public DNS: only the tailnet pin can reach it.
        let (mut stream, head) =
            connect_through(proxy.addr(), &format!("hs.tailtest.ts.net:{port}")).await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        stream.write_all(b"ping").await.unwrap();
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pong");
        let report = slot.get().expect("the control route is recorded");
        assert_eq!(report.host, "hs.tailtest.ts.net");
        assert_eq!(
            report.path,
            RoutePath::Tailnet {
                ip: Ipv4Addr::LOCALHOST
            }
        );
    }

    /// Funnel's measured failure shape: the connection is accepted, then closed with no
    /// byte back. The proxy names it on the route instead of calling the dial a success.
    #[tokio::test]
    async fn a_server_that_closes_without_answering_is_a_named_failure() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = upstream.accept().await.unwrap();
            let mut buf = [0u8; 4];
            let _ = s.read_exact(&mut buf).await;
        });
        let dns = fake_dns(Ipv4Addr::LOCALHOST).await;
        let resolver =
            TailnetResolver::with_server(dns, Duration::from_millis(500), |ip| ip.is_loopback());
        let slot = RouteSlot::default();
        let proxy = ControlProxy::start("hs.tailtest.ts.net".to_string(), resolver, slot.clone())
            .await
            .unwrap();
        let (mut stream, head) =
            connect_through(proxy.addr(), &format!("hs.tailtest.ts.net:{port}")).await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        stream.write_all(b"\x16\x03\x01\x00").await.unwrap();
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest).await;
        drop(stream);
        let mut failure = None;
        for _ in 0..50 {
            failure = slot.get().and_then(|r| r.last_failure);
            if failure.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let failure = failure.expect("the silent close is recorded");
        assert!(failure.contains("without sending a byte"), "{failure}");
    }

    /// Any other host (a DERP server) is relayed to exactly the target asked for; nothing
    /// is recorded on the control route and MagicDNS is never asked.
    #[tokio::test]
    async fn other_hosts_are_relayed_untouched() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = upstream.accept().await.unwrap();
            s.write_all(b"derp").await.unwrap();
        });
        let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let resolver = TailnetResolver::with_server(
            silent.local_addr().unwrap(),
            Duration::from_millis(200),
            |ip| ip.is_loopback(),
        );
        let slot = RouteSlot::default();
        let proxy = ControlProxy::start("hs.tailtest.ts.net".to_string(), resolver, slot.clone())
            .await
            .unwrap();
        let (mut stream, head) = connect_through(proxy.addr(), &format!("127.0.0.1:{port}")).await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"derp");
        assert_eq!(slot.get(), None);
    }

    #[tokio::test]
    async fn only_connect_is_served() {
        let proxy = ControlProxy::start(
            "hs.tailtest.ts.net".to_string(),
            TailnetResolver::default(),
            RouteSlot::default(),
        )
        .await
        .unwrap();
        let mut stream = TcpStream::connect(proxy.addr()).await.unwrap();
        stream
            .write_all(b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
            .await
            .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 405"), "{reply}");
    }
}
