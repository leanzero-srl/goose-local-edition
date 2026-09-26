//! Q-137: the road the worker client takes to a `*.ts.net` worker. A fake MagicDNS on
//! loopback stands in for 100.100.100.100 and a wiremock server stands in for the node's
//! tailnet address; the accepted-address predicate is widened to loopback so the pin is
//! testable. The worker name (`studio.tailtest.ts.net`) exists in no public DNS, so a
//! request that succeeds proves the tailnet pin carried it — and the name reached the
//! server as the `Host`, exactly as `curl --resolve` did on 2026-09-26.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use leanzero_link::tailnet_route::{is_tailscale_ipv4, RoutePath, TailnetResolver};
use leanzero_link::worker_client::{WorkerClient, WorkerError};
use serde_json::json;
use tokio::net::UdpSocket;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WORKER_HOST: &str = "studio.tailtest.ts.net";

/// A loopback DNS server answering every A query with `addrs`.
async fn fake_magicdns(addrs: Vec<Ipv4Addr>) -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    tokio::spawn(async move {
        let mut buf = [0u8; 512];
        while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
            let mut out = buf[..n].to_vec();
            out[2] |= 0x80;
            out[3] = 0x80;
            out[6..8].copy_from_slice(&(addrs.len() as u16).to_be_bytes());
            for ip in &addrs {
                out.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
                out.extend_from_slice(&ip.octets());
            }
            let _ = socket.send_to(&out, peer).await;
        }
    });
    addr
}

fn loopback_is_tailnet(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
}

async fn health_ok(server: &MockServer, port: u16) {
    Mock::given(method("GET"))
        .and(path("/leanzero-link/v1/health"))
        .and(header("host", format!("{WORKER_HOST}:{port}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "version": "0.1.0",
            "capabilities": { "mail": true, "audience": false, "mesh": true },
            "meshProvider": "headscale"
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_tailnet_worker_is_reached_at_its_tailnet_address_with_its_name_as_host() {
    let server = MockServer::start().await;
    let port = server.address().port();
    health_ok(&server, port).await;
    let dns = fake_magicdns(vec![Ipv4Addr::LOCALHOST]).await;
    let client = WorkerClient::new(format!("http://{WORKER_HOST}:{port}/leanzero-link"))
        .unwrap()
        .with_resolver(TailnetResolver::with_server(
            dns,
            Duration::from_millis(500),
            loopback_is_tailnet,
        ));

    let health = client
        .health()
        .await
        .expect("the tailnet pin reaches the worker");
    assert!(health.capabilities.mesh);
    let route = client.last_route().expect("the road is recorded");
    assert_eq!(route.host, WORKER_HOST);
    assert_eq!(
        route.path,
        RoutePath::Tailnet {
            ip: Ipv4Addr::LOCALHOST
        }
    );
    assert_eq!(route.last_failure, None);
}

/// The negative control: MagicDNS forwards a name it does not own and answers PUBLIC
/// addresses (the Funnel relays, 185.40.234.x on 2026-09-26). That is not a tailnet
/// road; the request goes by public DNS, fails, and the error says it was Funnel — with
/// the reason the tailnet was not used — instead of "error sending request".
#[tokio::test]
async fn a_public_answer_goes_through_funnel_and_a_dead_funnel_is_named() {
    let dns = fake_magicdns(vec![Ipv4Addr::new(185, 40, 234, 198)]).await;
    let client = WorkerClient::with_timeout(
        format!("https://{WORKER_HOST}/leanzero-link"),
        Duration::from_secs(5),
    )
    .unwrap()
    .with_resolver(TailnetResolver::with_server(
        dns,
        Duration::from_millis(500),
        is_tailscale_ipv4,
    ));

    let err = client
        .join_key("token")
        .await
        .expect_err("no public road exists");
    let WorkerError::FunnelUnreachable {
        what,
        host,
        why_public,
        ..
    } = &err
    else {
        panic!("expected FunnelUnreachable, got {err:?}");
    };
    assert_eq!(*what, "join-key");
    assert_eq!(host, WORKER_HOST);
    assert!(why_public.contains("185.40.234.198"), "{why_public}");
    let text = err.to_string();
    assert!(
        text.starts_with("the public Tailscale Funnel address of studio (studio.tailtest.ts.net)"),
        "{text}"
    );
    assert!(text.contains("restart Tailscale on studio"), "{text}");
    let route = client.last_route().unwrap();
    assert!(matches!(route.path, RoutePath::Public { .. }));
    assert!(route.last_failure.is_some());
}

/// A pinned tailnet road that fails is reported as THAT failure — never retried
/// through Funnel, never worded as a Funnel outage.
#[tokio::test]
async fn a_failed_tailnet_road_is_not_retried_through_funnel() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let dns = fake_magicdns(vec![Ipv4Addr::LOCALHOST]).await;
    let client = WorkerClient::new(format!("http://{WORKER_HOST}:{port}/leanzero-link"))
        .unwrap()
        .with_resolver(TailnetResolver::with_server(
            dns,
            Duration::from_millis(500),
            loopback_is_tailnet,
        ));

    let err = client
        .health()
        .await
        .expect_err("nothing listens on the pinned port");
    let WorkerError::Transport { route, detail, .. } = &err else {
        panic!("expected Transport, got {err:?}");
    };
    assert_eq!(route, "over the tailnet at 127.0.0.1");
    assert!(!detail.is_empty());
    assert!(!err.to_string().contains("Funnel"), "{err}");
}

/// A worker that is not a Tailscale name has one road: nothing is asked of MagicDNS and
/// no road is recorded (the mock-server tests in worker_client.rs rely on this).
#[tokio::test]
async fn a_non_tailnet_worker_asks_no_magicdns() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "version": "0.1.0",
            "capabilities": { "mail": false, "audience": false, "mesh": false }
        })))
        .mount(&server)
        .await;
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client =
        WorkerClient::new(server.uri())
            .unwrap()
            .with_resolver(TailnetResolver::with_server(
                silent.local_addr().unwrap(),
                Duration::from_secs(30),
                loopback_is_tailnet,
            ));
    let started = std::time::Instant::now();
    client.health().await.expect("the mock answers");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(client.last_route(), None);
}

/// Live: the production worker's `GET /v1/health` (no account, no mail) from a Mac on the
/// worker node's tailnet travels over the tailnet — the road that stayed up while the
/// node's Funnel was dead on 2026-09-26. Read-only; run explicitly.
#[tokio::test]
#[ignore = "calls the live LeanZero Link worker's /v1/health from a Mac on its tailnet"]
async fn live_default_worker_health_goes_over_the_tailnet() {
    let client = WorkerClient::new(leanzero_link::worker_client::DEFAULT_WORKER_BASE_URL).unwrap();
    let result = client.health().await;
    let route = client.last_route().expect("the road is recorded");
    eprintln!("route: {route:?}\nresult: {result:?}");
    assert!(matches!(route.path, RoutePath::Tailnet { .. }), "{route:?}");
    assert!(result.expect("the worker answers over the tailnet").ok);
}
