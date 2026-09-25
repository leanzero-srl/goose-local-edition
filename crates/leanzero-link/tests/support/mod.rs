//! A fake tailnet for the fabric tests: a SOCKS5 server standing in for the goose-owned
//! tailscaled's `--socks5-server` listener. Peers get MESH-looking addresses
//! (`100.64.x.y`) that the host cannot reach directly; only a CONNECT through this proxy
//! reaches them — so a test that passes proves the call went through the mesh proxy,
//! not around it. An address that was never exposed (or whose local server is gone)
//! answers the CONNECT with a SOCKS failure, the way tailscaled does for an unreachable
//! peer.
//!
//! The server lives on its own thread + runtime for the whole test binary, because each
//! `#[tokio::test]` has a runtime of its own that dies with the test.
//!
//! [`FakeTailnet::kill`] reproduces a peer whose tailscaled was killed (R3 step 1,
//! r3-1.log 12:14:59): every connection already tunnelled to it freezes — the peer's side
//! closes (its loopback leg died with the daemon) while the requester's side stays open and
//! silent, no FIN, no RST — and every new CONNECT toward it hangs unanswered until the
//! caller's own connect timeout (r3-1's `502 … error sending request` at exactly 5.00 s).
//! [`FakeTailnet::revive`] points the same mesh address at a fresh server: the peer's Link
//! restarted, and connections frozen before it stay frozen, as a new netstack knows nothing
//! of the old one's streams.
#![allow(dead_code)] // shared by several test crates; each uses a subset

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use leanzero_link::peer_dial::MeshProxy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct FakeTailnet {
    proxy: MeshProxy,
    routes: Arc<StdMutex<HashMap<SocketAddr, SocketAddr>>>,
    connects: Arc<StdMutex<Vec<SocketAddr>>>,
    next_ip: AtomicU32,
    killed: Arc<Killed>,
}

/// The mesh IPs whose Link is dead, and a channel that wakes every tunnel when the set
/// changes (a `watch` works across the proxy's runtime and each test's).
struct Killed {
    ips: StdMutex<HashSet<IpAddr>>,
    changed: tokio::sync::watch::Sender<u64>,
}

impl Killed {
    fn contains(&self, ip: IpAddr) -> bool {
        self.ips.lock().unwrap().contains(&ip)
    }
}

pub fn fake_tailnet() -> &'static FakeTailnet {
    static TAILNET: OnceLock<FakeTailnet> = OnceLock::new();
    TAILNET.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake SOCKS5");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let routes = Arc::new(StdMutex::new(HashMap::new()));
        let connects = Arc::new(StdMutex::new(Vec::new()));
        let killed = Arc::new(Killed {
            ips: StdMutex::new(HashSet::new()),
            changed: tokio::sync::watch::channel(0).0,
        });
        let (thread_routes, thread_connects, thread_killed) =
            (routes.clone(), connects.clone(), killed.clone());
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = TcpListener::from_std(listener).unwrap();
                loop {
                    let Ok((client, _)) = listener.accept().await else {
                        continue;
                    };
                    let routes = thread_routes.clone();
                    let connects = thread_connects.clone();
                    let killed = thread_killed.clone();
                    tokio::spawn(async move {
                        let _ = serve_one(client, routes, connects, killed).await;
                    });
                }
            });
        });
        FakeTailnet {
            proxy: MeshProxy::socks5(addr).unwrap(),
            routes,
            connects,
            next_ip: AtomicU32::new(0),
            killed,
        }
    })
}

impl FakeTailnet {
    pub fn proxy(&self) -> MeshProxy {
        self.proxy
    }

    /// Give the loopback server on `port` a fresh mesh IP (same port) and return it.
    pub fn expose(&self, port: u16) -> String {
        let n = self.next_ip.fetch_add(1, Ordering::SeqCst);
        let ip = Ipv4Addr::new(100, 64, (n / 250) as u8, (n % 250 + 1) as u8);
        self.routes.lock().unwrap().insert(
            SocketAddr::new(IpAddr::V4(ip), port),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        );
        ip.to_string()
    }

    /// The peer at `ip` loses its Link: tunnels to it freeze silently, new CONNECTs hang.
    pub fn kill(&self, ip: &str) {
        let ip: IpAddr = ip.parse().expect("a mesh IP");
        self.killed.ips.lock().unwrap().insert(ip);
        self.killed.changed.send_modify(|n| *n += 1);
    }

    /// The peer's Link is back at the same mesh address, served now by the loopback server
    /// on `local_port` (a fresh process). Tunnels frozen by [`Self::kill`] stay frozen.
    pub fn revive(&self, ip: &str, mesh_port: u16, local_port: u16) {
        let ip: IpAddr = ip.parse().expect("a mesh IP");
        self.routes.lock().unwrap().insert(
            SocketAddr::new(ip, mesh_port),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), local_port),
        );
        self.killed.ips.lock().unwrap().remove(&ip);
        self.killed.changed.send_modify(|n| *n += 1);
    }

    /// How many CONNECTs this proxy has been asked for toward `ip`.
    pub fn connects_to(&self, ip: &str) -> usize {
        self.connects
            .lock()
            .unwrap()
            .iter()
            .filter(|target| target.ip().to_string() == ip)
            .count()
    }
}

/// RFC 1928, no-auth CONNECT. The fabric only dials IP literals; `socks5h` clients may
/// still send one as a DOMAIN (atyp 3), so that form is accepted when it parses as an IP.
async fn serve_one(
    mut client: TcpStream,
    routes: Arc<StdMutex<HashMap<SocketAddr, SocketAddr>>>,
    connects: Arc<StdMutex<Vec<SocketAddr>>>,
    killed: Arc<Killed>,
) -> std::io::Result<()> {
    let mut head = [0u8; 2];
    client.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    client.read_exact(&mut methods).await?;
    client.write_all(&[5, 0]).await?;

    let mut request = [0u8; 4];
    client.read_exact(&mut request).await?;
    let ip = match request[3] {
        1 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await?;
            IpAddr::from(octets)
        }
        4 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await?;
            IpAddr::from(octets)
        }
        3 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            client.read_exact(&mut name).await?;
            match String::from_utf8_lossy(&name).parse::<IpAddr>() {
                Ok(ip) => ip,
                Err(_) => {
                    client.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
                    return Ok(());
                }
            }
        }
        _ => {
            client.write_all(&[5, 8, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Ok(());
        }
    };
    let mut port = [0u8; 2];
    client.read_exact(&mut port).await?;
    let target = SocketAddr::new(ip, u16::from_be_bytes(port));
    connects.lock().unwrap().push(target);

    if killed.contains(ip) {
        // A dead peer never answers the CONNECT: the caller's connect timeout decides.
        return hold_silently(client).await;
    }
    let route = routes.lock().unwrap().get(&target).copied();
    let upstream = match route {
        Some(local) => TcpStream::connect(local).await.ok(),
        None => None,
    };
    let Some(mut upstream) = upstream else {
        // 4 = host unreachable.
        client.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    };
    client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    let mut changed = killed.changed.subscribe();
    tokio::select! {
        result = tokio::io::copy_bidirectional(&mut client, &mut upstream) => {
            result?;
            Ok(())
        }
        _ = async {
            while !killed.contains(ip) {
                if changed.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        } => {
            drop(upstream);
            hold_silently(client).await
        }
    }
}

/// Keep the requester's socket open and silent — read and discard whatever it sends, never
/// answer, close only when it does.
async fn hold_silently(mut client: TcpStream) -> std::io::Result<()> {
    let mut sink = [0u8; 4096];
    while client.read(&mut sink).await? > 0 {}
    Ok(())
}
