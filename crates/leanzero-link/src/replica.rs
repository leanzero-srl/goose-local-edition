//! Node-to-node model replication: the SENDING node offers a local model directory, the
//! RECEIVING node pulls it file by file over a direct path (a Thunderbolt cable or a shared
//! LAN, see [`crate::netpath`]), resuming with HTTP Range and verifying every file's size and
//! SHA-256 against the sender before it takes its final name.
//!
//! ## Wire (served by the sender, `GET`/`DELETE` only)
//! - `GET  /v1/swarm/replica/{publisher}/{name}/manifest` → [`ReplicaManifest`]
//! - `GET  /v1/swarm/replica/{publisher}/{name}/file?path=<rel>` → the bytes; `Range:
//!   bytes=N-` (or `N-M`) answers `206` + `Content-Range: bytes N-M/size`, a start at or past
//!   the size answers `416` + `Content-Range: bytes */size`.
//! - `GET  /v1/swarm/replica/{publisher}/{name}/sha256?path=<rel>` → [`FileDigest`], computed
//!   by the sender from its own bytes (once per file per offer).
//! - `DELETE /v1/swarm/replica/{publisher}/{name}/offer` → the receiver releases the offer.
//!
//! ## Auth: a per-offer capability, issued over Link's node auth
//! Every route requires `Authorization: Bearer <offer token>`, compared in constant time,
//! and refuses any request carrying `Origin` (a browser) with `403` — the control service's
//! rule. The offer token is 32 random bytes minted when the owner starts a copy, and it
//! reaches the receiver ONLY inside the node-token-authenticated `/v1/swarm/mlx/replicaPull`
//! call. It grants exactly one model's files, and the receiver releases it when the copy
//! ends. The account-wide node token is deliberately NOT accepted here: this listener binds
//! a physical interface (a TB cable, a LAN) with plain HTTP, and a LAN is sniffable — a
//! leaked offer token exposes one offered model until release; a leaked node token would
//! expose `/execute` on every node of the account.
//!
//! An unknown model and a wrong token are the same `401`, so the listener does not confirm
//! which models are on offer. The file set is FROZEN at offer time: a path outside the
//! manifest is `404` (no path is ever joined from the request), and a file whose size moved
//! since the offer is `409`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Extension, Json, Router};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::netpath::LinkKind;

pub const REPLICA_ROUTE_BASE: &str = "/v1/swarm/replica";

/// The on-disk suffix of a file still arriving — the same convention the HF downloader
/// (`goose_sidecar::hf`) uses, so a model dir mid-copy is incomplete to `list_local_models`
/// for the same reason a model mid-download is.
pub const PART_SUFFIX: &str = ".part";

/// Bytes per read on the serving side; tokio's `File` hands each read to the blocking pool
/// and caps one read at 2 MiB, so a larger buffer buys nothing.
const SERVE_CHUNK: usize = 2 * 1024 * 1024;

/// Chunks queued between the network loop and each of the writer/hasher threads — the
/// backpressure window (one reqwest chunk is typically well under 1 MiB).
const PIPE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaManifest {
    pub model_id: String,
    pub files: Vec<ReplicaFile>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDigest {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A manifest path is relative, `/`-separated, and made only of normal components. The
/// receiver re-checks every path it is handed; the sender only ever lists what it walked.
pub fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Walk `root` into a manifest: every regular file, `/`-joined and sorted, `.part`
/// leftovers excluded (they are not the model). A symlink or a non-UTF-8 name is refused
/// loudly — replication copies regular files, and a link would be served as whatever it
/// points at.
pub fn build_manifest(model_id: &str, root: &Path) -> Result<ReplicaManifest, String> {
    let mut files = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel_dir) = stack.pop() {
        let dir = root.join(&rel_dir);
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?;
            let rel = rel_dir.join(entry.file_name());
            let meta = std::fs::symlink_metadata(entry.path())
                .map_err(|e| format!("reading {}: {e}", entry.path().display()))?;
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "{} is a symlink; replication copies regular files only",
                    entry.path().display()
                ));
            }
            if meta.is_dir() {
                stack.push(rel);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let Some(rel) = rel.to_str() else {
                return Err(format!(
                    "{} has a non-UTF-8 name; it cannot be named on the wire",
                    entry.path().display()
                ));
            };
            let rel = rel.replace(std::path::MAIN_SEPARATOR, "/");
            if rel.ends_with(PART_SUFFIX) {
                continue;
            }
            files.push(ReplicaFile {
                path: rel,
                size: meta.len(),
            });
        }
    }
    if files.is_empty() {
        return Err(format!("{} holds no files to replicate", root.display()));
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let total_bytes = files.iter().map(|f| f.size).sum();
    Ok(ReplicaManifest {
        model_id: model_id.to_string(),
        files,
        total_bytes,
    })
}

/// SHA-256 of one file, read in 8 MiB blocks. BLOCKING.
pub fn sha256_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((total, hex(&hasher.finalize())))
}

// ---------------------------------------------------------------------------
// Serving side.
// ---------------------------------------------------------------------------

type DigestCell = Arc<tokio::sync::OnceCell<Result<String, String>>>;

struct Offer {
    token: String,
    root: PathBuf,
    manifest: ReplicaManifest,
    digests: StdMutex<HashMap<String, DigestCell>>,
}

/// The models this node currently offers, keyed by capability token. Cheap to clone; every
/// clone is the same registry.
#[derive(Clone, Default)]
pub struct ReplicaOffers {
    offers: Arc<StdMutex<Vec<Arc<Offer>>>>,
}

/// What the sender hands the receiver (inside the node-authenticated pull request).
#[derive(Debug, Clone)]
pub struct OfferTicket {
    pub token: String,
    pub manifest: ReplicaManifest,
}

impl ReplicaOffers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer `model_id`, whose files live in `root`. Each call mints a NEW token (two
    /// receivers copying the same model each hold and release their own).
    pub fn offer(&self, model_id: &str, root: &Path) -> Result<OfferTicket, String> {
        let manifest = build_manifest(model_id, root)?;
        let token = hex(&rand::random::<[u8; 32]>());
        self.offers.lock().unwrap().push(Arc::new(Offer {
            token: token.clone(),
            root: root.to_path_buf(),
            manifest: manifest.clone(),
            digests: StdMutex::new(HashMap::new()),
        }));
        Ok(OfferTicket { token, manifest })
    }

    /// Withdraw the offer `token` names. `false` when there was none.
    pub fn release(&self, token: &str) -> bool {
        let mut offers = self.offers.lock().unwrap();
        let before = offers.len();
        offers.retain(|offer| !bool::from(offer.token.as_bytes().ct_eq(token.as_bytes())));
        offers.len() != before
    }

    pub fn active(&self) -> usize {
        self.offers.lock().unwrap().len()
    }

    /// Constant-time scan: every held token is compared, whatever matches first.
    fn find(&self, candidate: &str) -> Option<Arc<Offer>> {
        let offers = self.offers.lock().unwrap();
        let mut found = None;
        for offer in offers.iter() {
            if bool::from(offer.token.as_bytes().ct_eq(candidate.as_bytes())) {
                found = Some(offer.clone());
            }
        }
        found
    }
}

async fn require_offer(
    State(offers): State<ReplicaOffers>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.headers().contains_key(header::ORIGIN) {
        return Err(StatusCode::FORBIDDEN);
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let offer = offers.find(token).ok_or(StatusCode::UNAUTHORIZED)?;
    request.extensions_mut().insert(offer);
    Ok(next.run(request).await)
}

/// The route's model must be the one the token was minted for — a token for model A never
/// reads model B, and the answer is the same `401` as a wrong token.
fn offer_for(offer: &Arc<Offer>, publisher: &str, name: &str) -> Result<(), StatusCode> {
    if offer.manifest.model_id == format!("{publisher}/{name}") {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn manifest_route(
    AxumPath((publisher, name)): AxumPath<(String, String)>,
    Extension(offer): Extension<Arc<Offer>>,
) -> Response {
    if let Err(status) = offer_for(&offer, &publisher, &name) {
        return status.into_response();
    }
    Json(offer.manifest.clone()).into_response()
}

fn manifest_entry<'a>(offer: &'a Offer, path: &str) -> Option<&'a ReplicaFile> {
    offer.manifest.files.iter().find(|f| f.path == path)
}

/// `bytes=N-` / `bytes=N-M` → the inclusive range, `Err(())` for anything unsatisfiable or
/// unparseable (a multi-range or suffix range is not something this client sends).
fn parse_range(value: &str, size: u64) -> Result<(u64, u64), ()> {
    let spec = value.trim().strip_prefix("bytes=").ok_or(())?;
    let (start, end) = spec.split_once('-').ok_or(())?;
    let start: u64 = start.trim().parse().map_err(|_| ())?;
    if start >= size {
        return Err(());
    }
    let end = match end.trim() {
        "" => size - 1,
        end => end.parse::<u64>().map_err(|_| ())?.min(size - 1),
    };
    if end < start {
        return Err(());
    }
    Ok((start, end))
}

fn file_stream(
    file: tokio::fs::File,
    len: u64,
) -> impl futures::Stream<Item = std::io::Result<Bytes>> + Send {
    futures::stream::try_unfold((file, len), |(mut file, remaining)| async move {
        if remaining == 0 {
            return Ok(None);
        }
        let want = remaining.min(SERVE_CHUNK as u64) as usize;
        let mut buf = BytesMut::zeroed(want);
        let n = file.read(&mut buf).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the file shrank while it was being served",
            ));
        }
        buf.truncate(n);
        Ok(Some((buf.freeze(), (file, remaining - n as u64))))
    })
}

async fn file_route(
    AxumPath((publisher, name)): AxumPath<(String, String)>,
    Query(query): Query<PathQuery>,
    Extension(offer): Extension<Arc<Offer>>,
    headers: HeaderMap,
) -> Response {
    if let Err(status) = offer_for(&offer, &publisher, &name) {
        return status.into_response();
    }
    let Some(entry) = manifest_entry(&offer, &query.path) else {
        return (StatusCode::NOT_FOUND, "not in the offered manifest").into_response();
    };
    let on_disk = offer.root.join(&entry.path);
    let mut file = match tokio::fs::File::open(&on_disk).await {
        Ok(file) => file,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("opening {}: {e}", entry.path),
            )
                .into_response()
        }
    };
    match file.metadata().await {
        Ok(meta) if meta.len() == entry.size => {}
        Ok(meta) => {
            return (
                StatusCode::CONFLICT,
                format!(
                    "{} is {} bytes now but was {} when offered",
                    entry.path,
                    meta.len(),
                    entry.size
                ),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("reading {}: {e}", entry.path),
            )
                .into_response()
        }
    }

    let size = entry.size;
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (status, start, len) = match range {
        None => (StatusCode::OK, 0, size),
        Some(value) => match parse_range(value, size) {
            Ok((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
            Err(()) => {
                return (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    [(header::CONTENT_RANGE, format!("bytes */{size}"))],
                )
                    .into_response()
            }
        },
    };
    if start > 0 {
        if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("seeking {}: {e}", entry.path),
            )
                .into_response();
        }
    }
    let mut response = Response::new(Body::from_stream(file_stream(file, len)));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if status == StatusCode::PARTIAL_CONTENT {
        if let Ok(value) =
            HeaderValue::from_str(&format!("bytes {start}-{}/{size}", start + len - 1))
        {
            headers.insert(header::CONTENT_RANGE, value);
        }
    }
    response
}

async fn sha256_route(
    AxumPath((publisher, name)): AxumPath<(String, String)>,
    Query(query): Query<PathQuery>,
    Extension(offer): Extension<Arc<Offer>>,
) -> Response {
    if let Err(status) = offer_for(&offer, &publisher, &name) {
        return status.into_response();
    }
    let Some(entry) = manifest_entry(&offer, &query.path).cloned() else {
        return (StatusCode::NOT_FOUND, "not in the offered manifest").into_response();
    };
    let cell = offer
        .digests
        .lock()
        .unwrap()
        .entry(entry.path.clone())
        .or_default()
        .clone();
    let on_disk = offer.root.join(&entry.path);
    let hashed = entry.clone();
    let result = cell
        .get_or_init(|| async move {
            match tokio::task::spawn_blocking(move || sha256_file(&on_disk)).await {
                Ok(Ok((size, digest))) if size == hashed.size => Ok(digest),
                Ok(Ok((size, _))) => Err(format!(
                    "{} hashed {size} bytes but was {} when offered",
                    hashed.path, hashed.size
                )),
                Ok(Err(e)) => Err(format!("hashing {}: {e}", hashed.path)),
                Err(e) => Err(format!("hashing {}: {e}", hashed.path)),
            }
        })
        .await
        .clone();
    match result {
        Ok(sha256) => Json(FileDigest {
            path: entry.path,
            size: entry.size,
            sha256,
        })
        .into_response(),
        Err(text) => (StatusCode::CONFLICT, text).into_response(),
    }
}

async fn release_route(
    AxumPath((publisher, name)): AxumPath<(String, String)>,
    Extension(offer): Extension<Arc<Offer>>,
    State(offers): State<ReplicaOffers>,
) -> Response {
    if let Err(status) = offer_for(&offer, &publisher, &name) {
        return status.into_response();
    }
    offers.release(&offer.token);
    StatusCode::NO_CONTENT.into_response()
}

/// The replica routes over `offers`, each behind the offer-token middleware.
pub fn replica_router(offers: ReplicaOffers) -> Router {
    Router::new()
        .route(
            "/v1/swarm/replica/{publisher}/{name}/manifest",
            get(manifest_route),
        )
        .route("/v1/swarm/replica/{publisher}/{name}/file", get(file_route))
        .route(
            "/v1/swarm/replica/{publisher}/{name}/sha256",
            get(sha256_route),
        )
        .route(
            "/v1/swarm/replica/{publisher}/{name}/offer",
            delete(release_route),
        )
        .layer(axum::middleware::from_fn_with_state(
            offers.clone(),
            require_offer,
        ))
        .with_state(offers)
}

/// A replica listener bound to ONE address (the TB or LAN interface the copy uses — never
/// `0.0.0.0`). Dropping it stops serving.
pub struct ReplicaListener {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl ReplicaListener {
    pub async fn bind(bind: SocketAddr, offers: ReplicaOffers) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind).await?;
        let addr = listener.local_addr()?;
        let router = replica_router(offers);
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                tracing::error!(%error, "replica listener failed");
            }
        });
        tracing::info!(%addr, "replica listener serving offered models");
        Ok(Self { addr, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for ReplicaListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// ---------------------------------------------------------------------------
// Receiving side.
// ---------------------------------------------------------------------------

/// Everything the receiver needs to pull one model: where from, with which capability, and
/// over which kind of path (reported back in progress, never re-derived here).
#[derive(Debug, Clone)]
pub struct PullSpec {
    pub model_id: String,
    pub source_url: String,
    pub offer_token: String,
    pub link: LinkKind,
    /// Human-readable path description the sender chose ("en3 192.168.0.1 → 192.168.0.2,
    /// 80 Gb/s").
    pub link_detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaState {
    Queued,
    Copying,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaPhase {
    Transferring,
    /// The file's bytes are in; its digest is being compared with the sender's.
    Verifying,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplicaProgress {
    pub state: ReplicaState,
    pub source_url: String,
    pub link: LinkKind,
    pub link_detail: String,
    pub total_bytes: u64,
    /// Bytes of the model that are on disk AND accounted for: finished files, skipped
    /// (already present, verified) files, and the running file's bytes so far.
    pub copied_bytes: u64,
    pub files_total: u32,
    pub files_done: u32,
    pub current_file: Option<String>,
    pub phase: Option<ReplicaPhase>,
    /// Files this attempt continued from an on-disk `.part` via HTTP Range.
    pub resumed_files: Vec<String>,
    /// Files restarted from zero: a `.part` at or past the manifest size, or a sender that
    /// refused/contradicted the range.
    pub restarted_files: Vec<String>,
    /// Files already present at full size, verified in place against the sender's digest.
    pub skipped_files: Vec<String>,
    /// Bytes that crossed the wire in this attempt, and the milliseconds spent receiving
    /// them (request sent → last byte, summed per file): the link's measured rate.
    pub wire_bytes: u64,
    pub wire_millis: u64,
    /// Wall milliseconds since the job started (manifest, transfer, verification).
    pub elapsed_millis: u64,
    pub error: Option<String>,
    /// The pull failed because macOS refused THIS node's app the local network (EHOSTUNREACH on
    /// the direct path to a sender whose offer just arrived over Link): the owner's click in
    /// System Settings › Privacy & Security › Local Network, not a dead cable.
    #[serde(default)]
    pub local_network_blocked: bool,
    /// The offer could not be released at the sender; it lapses when the sender exits.
    pub release_error: Option<String>,
}

/// Called with the fetched manifest before any byte is written (goose checks disk space
/// here). An `Err` fails the job with that text.
pub type Preflight = Arc<dyn Fn(&ReplicaManifest) -> Result<(), String> + Send + Sync>;

struct Job {
    progress: ReplicaProgress,
    cancel: Arc<AtomicBool>,
}

/// Receiving-side jobs, one per model id. Cheap to clone.
#[derive(Clone, Default)]
pub struct ReplicaTracker {
    jobs: Arc<StdMutex<HashMap<String, Job>>>,
}

enum JobEnd {
    Done,
    Cancelled,
}

impl ReplicaTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self, model_id: &str) -> bool {
        self.jobs.lock().unwrap().get(model_id).is_some_and(|job| {
            matches!(
                job.progress.state,
                ReplicaState::Queued | ReplicaState::Copying
            )
        })
    }

    pub fn progress(&self, model_id: &str) -> Option<ReplicaProgress> {
        self.jobs
            .lock()
            .unwrap()
            .get(model_id)
            .map(|job| job.progress.clone())
    }

    /// Start pulling `spec.model_id` into `models_dir/<model_id>`. Refused while a job for
    /// the same model is queued or copying. A previous attempt's `.part` files are resumed.
    pub fn start(
        &self,
        spec: PullSpec,
        models_dir: &Path,
        preflight: Preflight,
    ) -> Result<(), String> {
        if !is_safe_relative_path(&spec.model_id) || spec.model_id.split('/').count() != 2 {
            return Err(format!(
                "invalid model id '{}': expected 'publisher/name'",
                spec.model_id
            ));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().unwrap();
            if self.is_active_locked(&jobs, &spec.model_id) {
                return Err(format!(
                    "a copy of '{}' is already running on this node",
                    spec.model_id
                ));
            }
            jobs.insert(
                spec.model_id.clone(),
                Job {
                    progress: ReplicaProgress {
                        state: ReplicaState::Queued,
                        source_url: spec.source_url.clone(),
                        link: spec.link,
                        link_detail: spec.link_detail.clone(),
                        total_bytes: 0,
                        copied_bytes: 0,
                        files_total: 0,
                        files_done: 0,
                        current_file: None,
                        phase: None,
                        resumed_files: Vec::new(),
                        restarted_files: Vec::new(),
                        skipped_files: Vec::new(),
                        wire_bytes: 0,
                        wire_millis: 0,
                        elapsed_millis: 0,
                        error: None,
                        local_network_blocked: false,
                        release_error: None,
                    },
                    cancel: cancel.clone(),
                },
            );
        }
        let tracker = self.clone();
        let models_dir = models_dir.to_path_buf();
        tokio::spawn(async move {
            let started = Instant::now();
            let client = match reqwest::Client::builder()
                // Transport only: a dead path fails fast; a slow large file is never cut.
                .connect_timeout(Duration::from_secs(10))
                .build()
            {
                Ok(client) => client,
                Err(e) => {
                    tracker.finish(
                        &spec.model_id,
                        started,
                        Err(format!("http client: {e}").into()),
                    );
                    return;
                }
            };
            let result = tracker
                .run(&client, &spec, &models_dir, &preflight, &cancel)
                .await;
            let release_error = release_offer(&client, &spec).await.err();
            tracker.update(&spec.model_id, |p| p.release_error = release_error);
            tracker.finish(&spec.model_id, started, result);
        });
        Ok(())
    }

    fn is_active_locked(&self, jobs: &HashMap<String, Job>, model_id: &str) -> bool {
        jobs.get(model_id).is_some_and(|job| {
            matches!(
                job.progress.state,
                ReplicaState::Queued | ReplicaState::Copying
            )
        })
    }

    /// Stop the running copy between chunks and delete its on-disk claim (the partial
    /// `models_dir/<model_id>`), exactly as a cancelled download does.
    pub fn cancel(&self, model_id: &str) -> Result<(), String> {
        let jobs = self.jobs.lock().unwrap();
        let Some(job) = jobs.get(model_id) else {
            return Err(format!("no copy of '{model_id}' is tracked on this node"));
        };
        if !matches!(
            job.progress.state,
            ReplicaState::Queued | ReplicaState::Copying
        ) {
            return Err(format!(
                "the copy of '{model_id}' is not running (state: {:?})",
                job.progress.state
            ));
        }
        job.cancel.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn update(&self, model_id: &str, apply: impl FnOnce(&mut ReplicaProgress)) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(model_id) {
            apply(&mut job.progress);
        }
    }

    fn finish(&self, model_id: &str, started: Instant, result: Result<JobEnd, PullError>) {
        self.update(model_id, |p| {
            p.elapsed_millis = started.elapsed().as_millis() as u64;
            p.current_file = None;
            p.phase = None;
            match result {
                Ok(JobEnd::Done) => p.state = ReplicaState::Done,
                Ok(JobEnd::Cancelled) => {
                    p.state = ReplicaState::Cancelled;
                    p.copied_bytes = 0;
                }
                Err(error) => {
                    p.state = ReplicaState::Failed;
                    p.error = Some(error.message);
                    p.local_network_blocked = error.local_network_blocked;
                }
            }
        });
    }

    async fn run(
        &self,
        client: &reqwest::Client,
        spec: &PullSpec,
        models_dir: &Path,
        preflight: &Preflight,
        cancel: &AtomicBool,
    ) -> Result<JobEnd, PullError> {
        let model = &spec.model_id;
        let manifest: ReplicaManifest = get_json(
            client,
            &route_url(spec, "manifest", None),
            &spec.offer_token,
            &format!("the manifest of '{model}' from {}", spec.source_url),
        )
        .await?;
        if &manifest.model_id != model {
            return Err(format!(
                "the sender answered with the manifest of '{}', not '{model}'",
                manifest.model_id
            ).into());
        }
        if let Some(bad) = manifest
            .files
            .iter()
            .find(|f| !is_safe_relative_path(&f.path))
        {
            return Err(format!(
                "the sender's manifest lists an unsafe path '{}'",
                bad.path
            ).into());
        }
        preflight(&manifest)?;

        let dest_root = models_dir.join(model);
        tokio::fs::create_dir_all(&dest_root)
            .await
            .map_err(|e| format!("creating {}: {e}", dest_root.display()))?;
        self.update(model, |p| {
            p.state = ReplicaState::Copying;
            p.total_bytes = manifest.total_bytes;
            p.files_total = manifest.files.len() as u32;
        });

        let mut done_bytes = 0u64;
        for (index, file) in manifest.files.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                remove_partial(models_dir, model)?;
                return Ok(JobEnd::Cancelled);
            }
            self.update(model, |p| {
                p.current_file = Some(file.path.clone());
                p.phase = Some(ReplicaPhase::Transferring);
            });
            let outcome = self
                .copy_file(client, spec, &dest_root, file, done_bytes, cancel)
                .await?;
            if matches!(outcome, FileEnd::Cancelled) {
                remove_partial(models_dir, model)?;
                return Ok(JobEnd::Cancelled);
            }
            done_bytes += file.size;
            self.update(model, |p| {
                p.copied_bytes = done_bytes;
                p.files_done = index as u32 + 1;
                if matches!(outcome, FileEnd::AlreadyPresent) {
                    p.skipped_files.push(file.path.clone());
                }
            });
        }
        Ok(JobEnd::Done)
    }

    async fn copy_file(
        &self,
        client: &reqwest::Client,
        spec: &PullSpec,
        dest_root: &Path,
        file: &ReplicaFile,
        done_bytes: u64,
        cancel: &AtomicBool,
    ) -> Result<FileEnd, PullError> {
        let model = &spec.model_id;
        let dest = dest_root.join(&file.path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        // The sender hashes its copy while ours arrives. Aborted on drop, so a cancelled or
        // failed file does not leave the request (and the sender's read) running.
        let mut sender_digest = AbortOnDrop({
            let client = client.clone();
            let url = route_url(spec, "sha256", Some(&file.path));
            let token = spec.offer_token.clone();
            let what = format!("the sha256 of {} from the sender", file.path);
            tokio::spawn(async move { get_json::<FileDigest>(&client, &url, &token, &what).await })
        });

        if let Ok(meta) = tokio::fs::metadata(&dest).await {
            if meta.is_file() && meta.len() == file.size {
                self.update(model, |p| p.phase = Some(ReplicaPhase::Verifying));
                let path = dest.clone();
                let local = tokio::task::spawn_blocking(move || sha256_file(&path))
                    .await
                    .map_err(|e| format!("hashing {}: {e}", file.path))?
                    .map_err(|e| format!("hashing {}: {e}", file.path))?;
                let sender = join_digest(&mut sender_digest, file).await?;
                if local.1 != sender.sha256 {
                    return Err(format!(
                        "{} is already on this node at the right size but its content differs \
                         from the sender's (sha256 {} here, {} there); delete the model on this \
                         node and copy again",
                        file.path, local.1, sender.sha256
                    ).into());
                }
                return Ok(FileEnd::AlreadyPresent);
            }
        }

        let part = PathBuf::from(format!("{}{PART_SUFFIX}", dest.display()));
        let mut resume_from = match tokio::fs::metadata(&part).await {
            Ok(meta) if meta.is_file() && meta.len() > 0 && meta.len() < file.size => {
                self.update(model, |p| p.resumed_files.push(file.path.clone()));
                meta.len()
            }
            Ok(meta) if meta.is_file() && meta.len() >= file.size => {
                self.update(model, |p| p.restarted_files.push(file.path.clone()));
                0
            }
            _ => 0,
        };

        let url = route_url(spec, "file", Some(&file.path));
        loop {
            match self
                .transfer(
                    client,
                    spec,
                    &url,
                    &part,
                    file,
                    resume_from,
                    done_bytes,
                    cancel,
                )
                .await?
            {
                Transfer::Cancelled => return Ok(FileEnd::Cancelled),
                Transfer::RangeRejected => {
                    self.update(model, |p| p.restarted_files.push(file.path.clone()));
                    resume_from = 0;
                }
                Transfer::Received { local_sha256 } => {
                    self.update(model, |p| p.phase = Some(ReplicaPhase::Verifying));
                    let sender = join_digest(&mut sender_digest, file).await?;
                    if sender.size != file.size || sender.sha256 != local_sha256 {
                        // A bad prefix would fail every resume the same way: drop it so the
                        // next attempt starts this file clean.
                        let _ = tokio::fs::remove_file(&part).await;
                        return Err(format!(
                            "{} failed verification: the sender's sha256 is {} ({} bytes), the \
                             received file's is {local_sha256}; the partial file was removed",
                            file.path, sender.sha256, sender.size
                        ).into());
                    }
                    tokio::fs::rename(&part, &dest)
                        .await
                        .map_err(|e| format!("renaming {} into place: {e}", part.display()))?;
                    return Ok(FileEnd::Copied);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn transfer(
        &self,
        client: &reqwest::Client,
        spec: &PullSpec,
        url: &str,
        part: &Path,
        file: &ReplicaFile,
        resume_from: u64,
        done_bytes: u64,
        cancel: &AtomicBool,
    ) -> Result<Transfer, PullError> {
        let model = &spec.model_id;
        let began = Instant::now();
        let mut request = client.get(url).bearer_auth(&spec.offer_token);
        if resume_from > 0 {
            request = request.header(header::RANGE, format!("bytes={resume_from}-"));
        }
        let mut response = request.send().await.map_err(|e| {
            request_failure(format!("GET {} from {}", file.path, spec.source_url), &e)
        })?;
        let status = response.status();
        if resume_from > 0 {
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE
                || status == reqwest::StatusCode::OK
            {
                return Ok(Transfer::RangeRejected);
            }
            if status == reqwest::StatusCode::PARTIAL_CONTENT
                && content_range_total(response.headers()) != Some(file.size)
            {
                return Ok(Transfer::RangeRejected);
            }
        }
        if !status.is_success() {
            let body = match response.text().await {
                Ok(body) => body,
                Err(e) => format!("<error body unreadable: {e}>"),
            };
            return Err(format!(
                "GET {} from {} returned HTTP {status}: {}",
                file.path,
                spec.source_url,
                body.chars().take(300).collect::<String>()
            ).into());
        }

        let sinks = FileSinks::open(part, resume_from)?;
        let mut received = 0u64;
        let outcome: Result<bool, String> = async {
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Ok(false);
                }
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        received += chunk.len() as u64;
                        sinks.send(chunk).await?;
                        let so_far = done_bytes + resume_from + received;
                        self.update(model, |p| p.copied_bytes = so_far);
                    }
                    Ok(None) => return Ok(true),
                    Err(e) => {
                        return Err(format!(
                            "receiving {} from {} after {received} bytes: {e}",
                            file.path, spec.source_url
                        ))
                    }
                }
            }
        }
        .await;
        let wire_millis = began.elapsed().as_millis() as u64;
        self.update(model, |p| {
            p.wire_bytes += received;
            p.wire_millis += wire_millis;
        });
        // The writer is joined on EVERY path, so whatever arrived is on disk for a resume.
        let finished = sinks.finish().await;
        match outcome {
            Ok(false) => Ok(Transfer::Cancelled),
            Err(e) => {
                finished?;
                Err(e.into())
            }
            Ok(true) => {
                let (length, local_sha256) = finished?;
                if length != file.size {
                    return Err(format!(
                        "{} ended at {length} bytes but the manifest says {}; the partial file \
                         is kept for a resume",
                        file.path, file.size
                    ).into());
                }
                Ok(Transfer::Received { local_sha256 })
            }
        }
    }
}

enum FileEnd {
    Copied,
    AlreadyPresent,
    Cancelled,
}

enum Transfer {
    Received { local_sha256: String },
    RangeRejected,
    Cancelled,
}

/// Two blocking threads fed from the network loop: one appends to the `.part`, one hashes
/// (the resumed prefix first, then every new chunk), so disk, hash and socket overlap.
struct FileSinks {
    write_tx: tokio::sync::mpsc::Sender<Bytes>,
    hash_tx: tokio::sync::mpsc::Sender<Bytes>,
    writer: JoinHandle<Result<u64, String>>,
    hasher: JoinHandle<Result<String, String>>,
}

impl FileSinks {
    fn open(part: &Path, resume_from: u64) -> Result<Self, String> {
        let mut out = if resume_from > 0 {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(part)
                .map_err(|e| format!("opening {} for append: {e}", part.display()))?;
            // A `.part` longer than the resume point (not expected, but possible after a
            // crash mid-write) is cut back so the append lands exactly at `resume_from`.
            file.set_len(resume_from)
                .map_err(|e| format!("truncating {}: {e}", part.display()))?;
            file
        } else {
            std::fs::File::create(part).map_err(|e| format!("creating {}: {e}", part.display()))?
        };
        use std::io::Seek;
        out.seek(std::io::SeekFrom::Start(resume_from))
            .map_err(|e| format!("seeking {}: {e}", part.display()))?;

        let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<Bytes>(PIPE_DEPTH);
        let (hash_tx, mut hash_rx) = tokio::sync::mpsc::channel::<Bytes>(PIPE_DEPTH);
        let part_for_writer = part.to_path_buf();
        let writer = tokio::task::spawn_blocking(move || -> Result<u64, String> {
            let mut out = std::io::BufWriter::with_capacity(8 * 1024 * 1024, out);
            let mut length = resume_from;
            while let Some(chunk) = write_rx.blocking_recv() {
                out.write_all(&chunk)
                    .map_err(|e| format!("writing {}: {e}", part_for_writer.display()))?;
                length += chunk.len() as u64;
            }
            out.flush()
                .map_err(|e| format!("flushing {}: {e}", part_for_writer.display()))?;
            Ok(length)
        });
        let part_for_hasher = part.to_path_buf();
        let hasher = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let mut hasher = Sha256::new();
            if resume_from > 0 {
                let prefix = std::fs::File::open(&part_for_hasher)
                    .map_err(|e| format!("reading {}: {e}", part_for_hasher.display()))?;
                let mut prefix = prefix.take(resume_from);
                let mut buf = vec![0u8; 8 * 1024 * 1024];
                loop {
                    let n = prefix
                        .read(&mut buf)
                        .map_err(|e| format!("reading {}: {e}", part_for_hasher.display()))?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
            }
            while let Some(chunk) = hash_rx.blocking_recv() {
                hasher.update(&chunk);
            }
            Ok(hex(&hasher.finalize()))
        });
        Ok(Self {
            write_tx,
            hash_tx,
            writer,
            hasher,
        })
    }

    async fn send(&self, chunk: Bytes) -> Result<(), String> {
        let (a, b) = tokio::join!(self.write_tx.send(chunk.clone()), self.hash_tx.send(chunk));
        if a.is_err() || b.is_err() {
            return Err("the file writer stopped early".to_string());
        }
        Ok(())
    }

    /// Close both pipes and join the threads: `(bytes now in the .part, its sha256)`.
    async fn finish(self) -> Result<(u64, String), String> {
        drop(self.write_tx);
        drop(self.hash_tx);
        let length = self
            .writer
            .await
            .map_err(|e| format!("file writer: {e}"))??;
        let digest = self.hasher.await.map_err(|e| format!("hasher: {e}"))??;
        Ok((length, digest))
    }
}

fn content_range_total(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit('/')
        .next()?
        .parse()
        .ok()
}

fn route_url(spec: &PullSpec, leaf: &str, path: Option<&str>) -> String {
    let base = format!(
        "{}{REPLICA_ROUTE_BASE}/{}/{leaf}",
        spec.source_url.trim_end_matches('/'),
        spec.model_id
    );
    match path {
        Some(path) => {
            let query: String = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("path", path)
                .finish();
            format!("{base}?{query}")
        }
        None => base,
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    what: &str,
) -> Result<T, PullError> {
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| request_failure(format!("fetching {what}"), &e))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("fetching {what}: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "fetching {what}: HTTP {status}: {}",
            body.chars().take(300).collect::<String>()
        )
        .into());
    }
    serde_json::from_str(&body)
        .map_err(|e| format!("fetching {what}: the body did not parse: {e}").into())
}

/// A pull's failure; `local_network_blocked` names macOS local network privacy.
#[derive(Debug)]
struct PullError {
    message: String,
    local_network_blocked: bool,
}

impl From<String> for PullError {
    fn from(message: String) -> Self {
        Self {
            message,
            local_network_blocked: false,
        }
    }
}

const LOCAL_NETWORK_BLOCKED: &str = "macOS is blocking Goose Swarm from the local network on this \
                                     node — allow it in System Settings › Privacy & Security › \
                                     Local Network";

/// reqwest's Display stops at "error sending request"; the cause (`No route to host (os error
/// 65)`) is in the source chain, so the message carries the whole chain. EHOSTUNREACH on the
/// direct path to a sender that just made its offer over Link is the refused Local Network
/// privilege (Apple TN3179: denied local network operations fail as if there were no route).
fn request_failure(what: String, error: &reqwest::Error) -> PullError {
    let mut chain = error.to_string();
    let mut blocked = false;
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !chain.contains(&text) {
            chain.push_str(": ");
            chain.push_str(&text);
        }
        if cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::HostUnreachable)
        {
            blocked = true;
        }
        source = cause.source();
    }
    PullError {
        message: if blocked {
            format!("{what}: {chain} — {LOCAL_NETWORK_BLOCKED}")
        } else {
            format!("{what}: {chain}")
        },
        local_network_blocked: blocked,
    }
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn join_digest(
    handle: &mut AbortOnDrop<Result<FileDigest, PullError>>,
    file: &ReplicaFile,
) -> Result<FileDigest, PullError> {
    (&mut handle.0)
        .await
        .map_err(|e| format!("the sha256 request for {} ended: {e}", file.path))?
}

async fn release_offer(client: &reqwest::Client, spec: &PullSpec) -> Result<(), String> {
    let response = client
        .delete(route_url(spec, "offer", None))
        .bearer_auth(&spec.offer_token)
        .send()
        .await
        .map_err(|e| format!("releasing the offer at {}: {e}", spec.source_url))?;
    match response.status() {
        status if status.is_success() => Ok(()),
        status => Err(format!(
            "releasing the offer at {}: HTTP {status}",
            spec.source_url
        )),
    }
}

/// Delete `models_dir/<model_id>` after a cancel, with the canonicalize guard the model
/// delete uses. A directory never created counts as already gone.
fn remove_partial(models_dir: &Path, model_id: &str) -> Result<(), String> {
    let target = models_dir.join(model_id);
    if !target.exists() {
        return Ok(());
    }
    let root = models_dir
        .canonicalize()
        .map_err(|e| format!("models dir {}: {e}", models_dir.display()))?;
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("{}: {e}", target.display()))?;
    if !canonical.starts_with(&root) || canonical == root {
        return Err(format!(
            "refusing to delete {}: it resolves outside {}",
            canonical.display(),
            root.display()
        ));
    }
    std::fs::remove_dir_all(&canonical)
        .map_err(|e| format!("deleting {}: {e}", canonical.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing_covers_open_closed_and_unsatisfiable() {
        assert_eq!(parse_range("bytes=0-", 10), Ok((0, 9)));
        assert_eq!(parse_range("bytes=4-", 10), Ok((4, 9)));
        assert_eq!(parse_range("bytes=4-6", 10), Ok((4, 6)));
        assert_eq!(parse_range("bytes=4-99", 10), Ok((4, 9)));
        assert_eq!(parse_range("bytes=10-", 10), Err(()));
        assert_eq!(parse_range("bytes=6-4", 10), Err(()));
        assert_eq!(parse_range("bytes=-4", 10), Err(()));
        assert_eq!(parse_range("items=0-", 10), Err(()));
    }

    #[test]
    fn unsafe_manifest_paths_are_refused() {
        assert!(is_safe_relative_path("model.safetensors"));
        assert!(is_safe_relative_path("sub/dir/tokenizer.json"));
        for bad in ["", "../etc/passwd", "/abs", "a/../../b", "a\\b", "./x"] {
            assert!(!is_safe_relative_path(bad), "{bad}");
        }
    }

    #[test]
    fn manifest_lists_regular_files_skips_parts_and_refuses_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("config.json"), b"{}").unwrap();
        std::fs::write(dir.path().join("sub/a.bin"), vec![1u8; 5]).unwrap();
        std::fs::write(dir.path().join("model.safetensors.part"), b"xx").unwrap();
        let manifest = build_manifest("pub/m", dir.path()).unwrap();
        let paths: Vec<_> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["config.json", "sub/a.bin"]);
        assert_eq!(manifest.total_bytes, 7);

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hosts", dir.path().join("link")).unwrap();
            let err = build_manifest("pub/m", dir.path()).unwrap_err();
            assert!(err.contains("symlink"), "{err}");
        }
    }

    /// The digest oracle is an independent vector (SHA-256 of "abc", FIPS 180-2 B.1), so a
    /// wrong hex order or a hasher fed twice fails against a value this code did not make.
    #[test]
    fn sha256_file_matches_the_fips_vector() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            (
                3,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string()
            )
        );
    }

    #[test]
    fn offers_are_per_token_and_release_is_exact() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), b"{}").unwrap();
        let offers = ReplicaOffers::new();
        let a = offers.offer("pub/m", dir.path()).unwrap();
        let b = offers.offer("pub/m", dir.path()).unwrap();
        assert_ne!(a.token, b.token);
        assert_eq!(a.token.len(), 64);
        assert!(offers.find(&a.token).is_some());
        assert!(offers.find("0".repeat(64).as_str()).is_none());
        assert!(offers.release(&a.token));
        assert!(!offers.release(&a.token));
        assert!(offers.find(&a.token).is_none());
        assert!(offers.find(&b.token).is_some(), "releasing A keeps B");
    }
}
