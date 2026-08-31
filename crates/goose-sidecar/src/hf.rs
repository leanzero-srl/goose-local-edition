//! HuggingFace Hub access for the MLX engine: MLX-only model search and paginated
//! browse, repo file listing, background snapshot downloads into a local models dir,
//! and local model inventory.
//!
//! Every network shape here was verified against the live API: `filter=mlx` is the
//! parameter that actually restricts results to MLX repos (`library=mlx` does not),
//! `expand[]` is required for `lastModified`/`downloads`/`likes`/`createdAt`/`tags` to
//! appear in hits, repeated `filter` params AND-combine, and pagination rides the Link
//! rel="next" header.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

const HF_BASE: &str = "https://huggingface.co";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfModelHit {
    pub id: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub likes: u64,
    #[serde(rename = "lastModified")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    pub id: String,
    pub size_bytes: u64,
    pub complete: bool,
}

/// `publisher/name`, each segment starting alphanumeric and continuing with `[A-Za-z0-9._-]`.
pub fn validate_model_id(id: &str) -> Result<()> {
    let mut parts = id.split('/');
    let (Some(publisher), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        bail!("invalid model id '{id}': expected exactly two segments 'publisher/name'");
    };
    for segment in [publisher, name] {
        let mut chars = segment.chars();
        let valid = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        ensure!(
            valid,
            "invalid model id '{id}': segment '{segment}' must start alphanumeric and use only [A-Za-z0-9._-]"
        );
    }
    Ok(())
}

fn api_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("goose-sidecar")
        .timeout(Duration::from_secs(30))
        .build()
        .context("building HuggingFace API client")
}

fn download_client() -> Result<reqwest::Client> {
    // No overall timeout: multi-GiB safetensors legitimately take a long time.
    // Stalls surface as chunk errors; connect failures are bounded below.
    reqwest::Client::builder()
        .user_agent("goose-sidecar")
        .connect_timeout(Duration::from_secs(30))
        .build()
        .context("building HuggingFace download client")
}

async fn read_success_body(resp: reqwest::Response, what: &str) -> Result<String> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .with_context(|| format!("reading response body from {what}"))?;
    ensure!(
        status.is_success(),
        "{what} returned HTTP {status}: {}",
        body.chars().take(500).collect::<String>()
    );
    Ok(body)
}

pub async fn search_mlx_models(
    query: &str,
    limit: u32,
    token: Option<&str>,
) -> Result<Vec<HfModelHit>> {
    let client = api_client()?;
    let mut req = client.get(format!("{HF_BASE}/api/models")).query(&[
        ("search", query),
        ("filter", "mlx"),
        ("sort", "downloads"),
        ("limit", &limit.to_string()),
        ("expand[]", "lastModified"),
        ("expand[]", "downloads"),
        ("expand[]", "likes"),
    ]);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .context("GET huggingface.co/api/models (search)")?;
    let body = read_success_body(resp, "HuggingFace model search").await?;
    serde_json::from_str(&body).context("parsing HuggingFace search response")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowseSort {
    Downloads,
    Newest,
}

#[derive(Debug, Clone)]
pub struct BrowseParams {
    pub query: Option<String>,
    pub author: Option<String>,
    pub quant: Option<String>,
    pub arch: Option<String>,
    pub sort: BrowseSort,
    /// Opaque continuation from a previous page's `next_cursor` (the Link rel="next"
    /// URL). When set, every other parameter is already baked into it.
    pub cursor: Option<String>,
    pub limit: u32,
}

impl Default for BrowseParams {
    fn default() -> Self {
        Self {
            query: None,
            author: None,
            quant: None,
            arch: None,
            sort: BrowseSort::Downloads,
            cursor: None,
            limit: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseHit {
    pub id: String,
    /// The `publisher` prefix of `id`.
    pub author: String,
    pub downloads: u64,
    pub likes: u64,
    pub created_at: Option<String>,
    pub last_modified: Option<String>,
    pub tags: Vec<String>,
    /// DERIVED display field: the repo's `N-bit` tag when present, else parsed from
    /// the repo name (`(\d+)[-_]?bit`). Not what the server filtered on unless the
    /// caller also set `BrowseParams::quant`.
    pub quant: Option<String>,
    /// DERIVED display field: the most specific measured architecture tag present,
    /// else a boundary-checked keyword match against the repo name.
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowsePage {
    pub hits: Vec<BrowseHit>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowseApiHit {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default, rename = "createdAt")]
    created_at: Option<String>,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Architecture (`config.model_type`) tags measured live on MLX repos (2026-08-31):
/// every entry returned hits for `filter=mlx&filter=<tag>`, sampled from 250 real tag
/// arrays plus per-family probes. `baichuan` returned zero MLX hits and is deliberately
/// absent. Derivation picks the LONGEST entry that matches, so `qwen3_5` beats `qwen`.
const MEASURED_ARCH_TAGS: &[&str] = &[
    "qwen3_5_moe",
    "qwen3_5",
    "qwen3_moe",
    "qwen3_vl",
    "qwen4_exp",
    "qwen3",
    "qwen2",
    "qwen",
    "gemma4_unified",
    "gemma4",
    "gemma3",
    "glm4_moe",
    "glm4v",
    "glm4",
    "deepseek_v3",
    "deepseek_v2",
    "kimi_k25",
    "kimi_k2",
    "lfm2_moe",
    "lfm2",
    "gpt_oss",
    "smollm3",
    "starcoder2",
    "ernie4_5",
    "mixtral",
    "mistral",
    "phi3",
    "phi",
    "llama",
    "granite",
    "olmo2",
    "minimax",
    "mamba",
    "cohere",
    "nemotron",
    "exaone",
    "whisper",
    "internvl",
];

fn is_bit_tag(tag: &str) -> bool {
    tag.strip_suffix("-bit")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `(\d+)[-_]?bit` over the lowercased id; first digit-backed occurrence wins,
/// normalized to the tag form `N-bit`.
fn quant_from_name(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for (i, _) in lower.match_indices("bit") {
        let mut end = i;
        if end > 0 && matches!(bytes[end - 1], b'-' | b'_') {
            end -= 1;
        }
        let mut start = end;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start < end {
            return Some(format!("{}-bit", &lower[start..end]));
        }
    }
    None
}

pub fn derive_quant(id: &str, tags: &[String]) -> Option<String> {
    tags.iter()
        .find(|t| is_bit_tag(t))
        .cloned()
        .or_else(|| quant_from_name(id))
}

/// Keyword occurs in the `[.-/]→_` normalized name at a token boundary: nothing
/// alphanumeric immediately before it, no letter immediately after (a trailing digit
/// is allowed so `llama31` still reads as llama).
fn name_contains_keyword(norm: &str, keyword: &str) -> bool {
    let bytes = norm.as_bytes();
    let mut from = 0;
    while let Some(pos) = norm[from..].find(keyword) {
        let start = from + pos;
        let end = start + keyword.len();
        let before_ok = start == 0
            || !(bytes[start - 1].is_ascii_lowercase() || bytes[start - 1].is_ascii_digit());
        let after_ok = end == norm.len() || !bytes[end].is_ascii_lowercase();
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

pub fn derive_arch(id: &str, tags: &[String]) -> Option<String> {
    if let Some(best) = MEASURED_ARCH_TAGS
        .iter()
        .filter(|a| tags.iter().any(|t| t == **a))
        .max_by_key(|a| a.len())
    {
        return Some((*best).to_string());
    }
    let norm: String = id
        .to_ascii_lowercase()
        .chars()
        .map(|c| if matches!(c, '.' | '-' | '/') { '_' } else { c })
        .collect();
    MEASURED_ARCH_TAGS
        .iter()
        .filter(|a| name_contains_keyword(&norm, a))
        .max_by_key(|a| a.len())
        .map(|a| a.to_string())
}

/// Accepts `4`, `4bit`, `4-bit`, `4_bit` (any case) and yields the HF tag form `4-bit`.
/// Anything else is refused loudly — a quant filter that cannot become a real tag would
/// silently return the unfiltered listing.
fn normalize_quant_filter(input: &str) -> Result<String> {
    let s = input.trim().to_ascii_lowercase();
    let digits_end = s
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(s.len());
    let (digits, rest) = s.split_at(digits_end);
    ensure!(
        !digits.is_empty() && matches!(rest, "" | "bit" | "-bit" | "_bit"),
        "invalid quant filter '{input}': expected a bit width like '4-bit'"
    );
    Ok(format!("{digits}-bit"))
}

/// Paginated MLX model browse. Every filter is applied SERVER-SIDE, each shape measured
/// live (2026-08-31): repeated `filter` params AND-combine (`filter=4-bit` alone returns
/// non-MLX repos; combined with `filter=mlx`, only repos carrying both tags),
/// `author`/`search` restrict as documented, `sort=createdAt&direction=-1` with
/// `expand[]=createdAt` pages in strictly descending creation order, and the Link
/// rel="next" header carries the whole query (HF rewrites `expand[]=` to `expand=`
/// there; the API honors both). The quant/arch filters match HF *tags*, so repos whose
/// bit width appears only in the NAME (18 of the top 50 mlx-community repos) are not
/// returned by a quant filter — honest under-inclusion, never wrong pagination.
pub async fn browse_mlx_models(params: &BrowseParams, token: Option<&str>) -> Result<BrowsePage> {
    let client = api_client()?;
    let mut req = match &params.cursor {
        Some(cursor) => {
            let prefix = format!("{HF_BASE}/api/models?");
            ensure!(
                cursor.starts_with(&prefix),
                "invalid browse cursor: expected a previous page's '{prefix}…' URL"
            );
            client.get(cursor)
        }
        None => {
            let mut query: Vec<(&str, String)> = vec![("filter", "mlx".to_string())];
            if let Some(quant) = &params.quant {
                query.push(("filter", normalize_quant_filter(quant)?));
            }
            if let Some(arch) = &params.arch {
                let arch = arch.trim().to_ascii_lowercase();
                ensure!(!arch.is_empty(), "arch filter is empty");
                query.push(("filter", arch));
            }
            if let Some(search) = params.query.as_deref().map(str::trim) {
                if !search.is_empty() {
                    query.push(("search", search.to_string()));
                }
            }
            if let Some(author) = params.author.as_deref().map(str::trim) {
                if !author.is_empty() {
                    query.push(("author", author.to_string()));
                }
            }
            match params.sort {
                BrowseSort::Downloads => query.push(("sort", "downloads".to_string())),
                BrowseSort::Newest => {
                    query.push(("sort", "createdAt".to_string()));
                    query.push(("direction", "-1".to_string()));
                }
            }
            query.push(("limit", params.limit.clamp(1, 50).to_string()));
            for field in ["downloads", "likes", "createdAt", "lastModified", "tags"] {
                query.push(("expand[]", field.to_string()));
            }
            client.get(format!("{HF_BASE}/api/models")).query(&query)
        }
    };
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .context("GET huggingface.co/api/models (browse)")?;
    let next_cursor = next_page_url(resp.headers());
    let body = read_success_body(resp, "HuggingFace model browse").await?;
    let raw: Vec<BrowseApiHit> =
        serde_json::from_str(&body).context("parsing HuggingFace browse response")?;
    let hits = raw
        .into_iter()
        .map(|h| {
            let quant = derive_quant(&h.id, &h.tags);
            let arch = derive_arch(&h.id, &h.tags);
            let author = h.id.split('/').next().unwrap_or_default().to_string();
            BrowseHit {
                author,
                downloads: h.downloads,
                likes: h.likes,
                created_at: h.created_at,
                last_modified: h.last_modified,
                quant,
                arch,
                tags: h.tags,
                id: h.id,
            }
        })
        .collect();
    Ok(BrowsePage { hits, next_cursor })
}

#[derive(Debug, Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
}

fn next_page_url(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let link = headers.get(reqwest::header::LINK)?.to_str().ok()?;
    link.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.trim().eq_ignore_ascii_case("rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

pub async fn repo_files(repo_id: &str, token: Option<&str>) -> Result<Vec<RepoFile>> {
    validate_model_id(repo_id)?;
    let client = api_client()?;
    let mut url = format!("{HF_BASE}/api/models/{repo_id}/tree/main?recursive=true");
    let mut files = Vec::new();
    loop {
        let mut req = client.get(&url);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.with_context(|| format!("GET {url}"))?;
        let next = next_page_url(resp.headers());
        let body =
            read_success_body(resp, &format!("HuggingFace tree listing for '{repo_id}'")).await?;
        let entries: Vec<TreeEntry> =
            serde_json::from_str(&body).context("parsing HuggingFace tree response")?;
        files.extend(
            entries
                .into_iter()
                .filter(|e| e.kind == "file")
                .map(|e| RepoFile {
                    path: e.path,
                    size: e.size,
                }),
        );
        match next {
            Some(next) => url = next,
            None => break,
        }
    }
    Ok(files)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Downloading,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub state: DownloadState,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub current_file: Option<String>,
    pub error: Option<String>,
}

struct DownloadEntry {
    progress: DownloadProgress,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct DownloadTracker {
    downloads: Arc<Mutex<HashMap<String, DownloadEntry>>>,
}

enum DownloadOutcome {
    Done,
    Cancelled,
}

impl DownloadTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register and spawn a background snapshot download of `repo_id` into
    /// `{models_dir}/{repo_id}`. Errors if that repo already has an active download.
    pub fn start_download(
        &self,
        repo_id: &str,
        models_dir: &Path,
        token: Option<String>,
    ) -> Result<()> {
        validate_model_id(repo_id)?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut map = self.downloads.lock().unwrap();
            if let Some(existing) = map.get(repo_id) {
                if matches!(
                    existing.progress.state,
                    DownloadState::Queued | DownloadState::Downloading
                ) {
                    bail!("download already in progress for '{repo_id}'");
                }
            }
            map.insert(
                repo_id.to_string(),
                DownloadEntry {
                    progress: DownloadProgress {
                        state: DownloadState::Queued,
                        total_bytes: 0,
                        downloaded_bytes: 0,
                        current_file: None,
                        error: None,
                    },
                    cancel: Arc::clone(&cancel),
                },
            );
        }

        let downloads = Arc::clone(&self.downloads);
        let repo_id = repo_id.to_string();
        let models_dir = models_dir.to_path_buf();
        tokio::spawn(async move {
            let result =
                run_download(&repo_id, &models_dir, token.as_deref(), &downloads, &cancel).await;
            let mut map = downloads.lock().unwrap();
            let Some(entry) = map.get_mut(&repo_id) else {
                return;
            };
            entry.progress.current_file = None;
            match result {
                Ok(DownloadOutcome::Done) => entry.progress.state = DownloadState::Done,
                Ok(DownloadOutcome::Cancelled) => entry.progress.state = DownloadState::Cancelled,
                Err(e) => {
                    entry.progress.state = DownloadState::Failed;
                    entry.progress.error = Some(format!("{e:#}"));
                }
            }
        });
        Ok(())
    }

    pub fn progress(&self, repo_id: &str) -> Option<DownloadProgress> {
        self.downloads
            .lock()
            .unwrap()
            .get(repo_id)
            .map(|e| e.progress.clone())
    }

    pub fn cancel(&self, repo_id: &str) -> Result<()> {
        let map = self.downloads.lock().unwrap();
        let Some(entry) = map.get(repo_id) else {
            bail!("no download tracked for '{repo_id}'");
        };
        ensure!(
            matches!(
                entry.progress.state,
                DownloadState::Queued | DownloadState::Downloading
            ),
            "download for '{repo_id}' is not active (state: {:?})",
            entry.progress.state
        );
        entry.cancel.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn update_progress(
    downloads: &Arc<Mutex<HashMap<String, DownloadEntry>>>,
    repo_id: &str,
    apply: impl FnOnce(&mut DownloadProgress),
) {
    if let Some(entry) = downloads.lock().unwrap().get_mut(repo_id) {
        apply(&mut entry.progress);
    }
}

async fn run_download(
    repo_id: &str,
    models_dir: &Path,
    token: Option<&str>,
    downloads: &Arc<Mutex<HashMap<String, DownloadEntry>>>,
    cancel: &AtomicBool,
) -> Result<DownloadOutcome> {
    let files = repo_files(repo_id, token).await?;
    ensure!(!files.is_empty(), "repo '{repo_id}' lists no files");
    for file in &files {
        let path = Path::new(&file.path);
        ensure!(
            path.is_relative()
                && !path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "repo '{repo_id}' lists unsafe file path '{}'",
            file.path
        );
    }

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    let dest_root = models_dir.join(repo_id);
    tokio::fs::create_dir_all(&dest_root)
        .await
        .with_context(|| format!("creating {}", dest_root.display()))?;
    update_progress(downloads, repo_id, |p| {
        p.state = DownloadState::Downloading;
        p.total_bytes = total_bytes;
    });

    let client = download_client()?;
    let mut downloaded: u64 = 0;
    for file in &files {
        if cancel.load(Ordering::SeqCst) {
            return Ok(DownloadOutcome::Cancelled);
        }
        let dest = dest_root.join(&file.path);
        if let Ok(meta) = tokio::fs::metadata(&dest).await {
            if meta.is_file() && meta.len() == file.size {
                downloaded += file.size;
                update_progress(downloads, repo_id, |p| p.downloaded_bytes = downloaded);
                continue;
            }
        }
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        update_progress(downloads, repo_id, |p| {
            p.current_file = Some(file.path.clone());
        });

        let part = PathBuf::from(format!("{}.part", dest.display()));
        let url = format!("{HF_BASE}/{repo_id}/resolve/main/{}", file.path);
        let mut file_bytes: u64 = 0;
        let on_chunk = |written: u64| {
            file_bytes += written;
            update_progress(downloads, repo_id, |p| {
                p.downloaded_bytes = downloaded + file_bytes;
            });
        };
        match download_one_file(&client, &url, &part, token, cancel, on_chunk).await {
            Ok(DownloadOutcome::Cancelled) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Ok(DownloadOutcome::Cancelled);
            }
            Ok(DownloadOutcome::Done) => {
                tokio::fs::rename(&part, &dest)
                    .await
                    .with_context(|| format!("renaming {} into place", part.display()))?;
                downloaded += file.size;
                update_progress(downloads, repo_id, |p| p.downloaded_bytes = downloaded);
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
        }
    }
    Ok(DownloadOutcome::Done)
}

async fn download_one_file(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    token: Option<&str>,
    cancel: &AtomicBool,
    mut on_chunk: impl FnMut(u64),
) -> Result<DownloadOutcome> {
    let mut req = client.get(url);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let mut resp = req.send().await.with_context(|| format!("GET {url}"))?;
    ensure!(
        resp.status().is_success(),
        "GET {url} returned HTTP {}",
        resp.status()
    );
    let mut out = tokio::fs::File::create(part)
        .await
        .with_context(|| format!("creating {}", part.display()))?;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(DownloadOutcome::Cancelled);
        }
        match resp
            .chunk()
            .await
            .with_context(|| format!("streaming {url}"))?
        {
            Some(chunk) => {
                out.write_all(&chunk)
                    .await
                    .with_context(|| format!("writing {}", part.display()))?;
                on_chunk(chunk.len() as u64);
            }
            None => break,
        }
    }
    out.flush()
        .await
        .with_context(|| format!("flushing {}", part.display()))?;
    Ok(DownloadOutcome::Done)
}

/// Two-level walk of `models_dir`: a model is any `publisher/name` directory containing
/// `config.json`; it is complete when no `*.part` remains and at least one `*.safetensors`
/// exists. A missing `models_dir` is the legitimate nothing-downloaded-yet state.
pub fn list_local_models(models_dir: &Path) -> Result<Vec<LocalModel>> {
    if !models_dir.exists() {
        return Ok(Vec::new());
    }
    let mut models = Vec::new();
    let publishers = std::fs::read_dir(models_dir)
        .with_context(|| format!("reading {}", models_dir.display()))?;
    for publisher in publishers {
        let publisher = publisher?;
        if !publisher.file_type()?.is_dir() {
            continue;
        }
        let publisher_name = publisher.file_name().to_string_lossy().into_owned();
        let entries = std::fs::read_dir(publisher.path())
            .with_context(|| format!("reading {}", publisher.path().display()))?;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let dir = entry.path();
            if !dir.join("config.json").is_file() {
                continue;
            }
            let (has_part, has_safetensors) = scan_model_dir(&dir);
            models.push(LocalModel {
                id: format!("{publisher_name}/{}", entry.file_name().to_string_lossy()),
                size_bytes: crate::dir_size_bytes(&dir),
                complete: !has_part && has_safetensors,
            });
        }
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

fn scan_model_dir(dir: &Path) -> (bool, bool) {
    let mut has_part = false;
    let mut has_safetensors = false;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(ext) = path.extension() {
                if ext == "part" {
                    has_part = true;
                } else if ext == "safetensors" {
                    has_safetensors = true;
                }
            }
        }
    }
    (has_part, has_safetensors)
}

pub fn delete_local_model(models_dir: &Path, id: &str) -> Result<()> {
    validate_model_id(id)?;
    let canonical_root = models_dir
        .canonicalize()
        .with_context(|| format!("models dir {} does not resolve", models_dir.display()))?;
    let target = models_dir.join(id);
    let canonical_target = target
        .canonicalize()
        .with_context(|| format!("model '{id}' not found under {}", models_dir.display()))?;
    ensure!(
        canonical_target.starts_with(&canonical_root),
        "refusing to delete '{id}': {} resolves outside {}",
        canonical_target.display(),
        canonical_root.display()
    );
    std::fs::remove_dir_all(&canonical_target)
        .with_context(|| format!("deleting {}", canonical_target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_model(root: &Path, id: &str, files: &[(&str, usize)]) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, size) in files {
            std::fs::write(dir.join(name), vec![0u8; *size]).unwrap();
        }
    }

    #[test]
    fn model_id_validation_accepts_real_ids_and_rejects_bad_shapes() {
        for good in [
            "mlx-community/Qwen3.5-9B-MLX-4bit",
            "Qwen/Qwen3-0.6B",
            "a/b",
            "a1.b_c-d/e2.f_g-h",
        ] {
            validate_model_id(good).unwrap_or_else(|e| panic!("{good} rejected: {e}"));
        }
        for bad in [
            "",
            "single",
            "a/b/c",
            "../etc/passwd",
            "a/../b",
            ".hidden/model",
            "pub/.dotname",
            "pub/",
            "/name",
            "pub/na me",
            "pub/na~me",
        ] {
            assert!(validate_model_id(bad).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn list_local_models_flags_partials_and_requires_safetensors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        make_model(
            root,
            "pub/complete",
            &[("config.json", 10), ("model.safetensors", 100)],
        );
        make_model(
            root,
            "pub/partial",
            &[
                ("config.json", 10),
                ("model.safetensors", 100),
                ("model-2.safetensors.part", 50),
            ],
        );
        make_model(root, "pub/no-weights", &[("config.json", 10)]);
        make_model(root, "pub/not-a-model", &[("README.md", 5)]);

        let models = list_local_models(root).unwrap();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["pub/complete", "pub/no-weights", "pub/partial"]);

        let by_id = |id: &str| models.iter().find(|m| m.id == id).unwrap();
        assert!(by_id("pub/complete").complete);
        assert_eq!(by_id("pub/complete").size_bytes, 110);
        assert!(!by_id("pub/partial").complete, ".part must mark incomplete");
        assert!(
            !by_id("pub/no-weights").complete,
            "no safetensors must mark incomplete"
        );
    }

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Real tag arrays captured live from the HF API on 2026-08-31.
    #[test]
    fn quant_and_arch_derivation_on_real_hf_fixtures() {
        // Tagged 4-bit AND gpt_oss although the NAME says MXFP4-Q8 — tags must win.
        let gpt_oss = tags(&[
            "mlx",
            "safetensors",
            "gpt_oss",
            "vllm",
            "text-generation",
            "conversational",
            "base_model:openai/gpt-oss-20b",
            "base_model:quantized:openai/gpt-oss-20b",
            "license:apache-2.0",
            "4-bit",
            "region:us",
        ]);
        let id = "mlx-community/gpt-oss-20b-MXFP4-Q8";
        assert_eq!(derive_quant(id, &gpt_oss), Some("4-bit".to_string()));
        assert_eq!(derive_arch(id, &gpt_oss), Some("gpt_oss".to_string()));

        // No bit tag at all — quant must come from the NAME; arch must prefer the
        // model_type tag qwen2 over the looser qwen also present.
        let qwen25 = tags(&[
            "transformers",
            "safetensors",
            "qwen2",
            "text-generation",
            "code",
            "codeqwen",
            "chat",
            "qwen",
            "qwen-coder",
            "mlx",
            "conversational",
            "en",
            "base_model:Qwen/Qwen2.5-Coder-7B",
            "base_model:finetune:Qwen/Qwen2.5-Coder-7B",
            "license:apache-2.0",
            "text-generation-inference",
            "endpoints_compatible",
            "region:us",
        ]);
        let id = "mlx-community/Qwen2.5-Coder-7B-Instruct-4bit";
        assert_eq!(derive_quant(id, &qwen25), Some("4-bit".to_string()));
        assert_eq!(derive_arch(id, &qwen25), Some("qwen2".to_string()));

        // qwen3_5 model_type tag plus 4-bit tag.
        let qwen38 = tags(&[
            "mlx",
            "safetensors",
            "qwen3_5",
            "image-text-to-text",
            "conversational",
            "base_model:Qwen/Qwen3.8-27B",
            "base_model:quantized:Qwen/Qwen3.8-27B",
            "license:apache-2.0",
            "4-bit",
            "region:us",
        ]);
        let id = "mlx-community/Qwen3.8-27B-4bit";
        assert_eq!(derive_quant(id, &qwen38), Some("4-bit".to_string()));
        assert_eq!(derive_arch(id, &qwen38), Some("qwen3_5".to_string()));

        // Tag is kimi_k25 (measured, in the set); quant from tag.
        let kimi = tags(&[
            "mlx",
            "safetensors",
            "kimi_k25",
            "text-generation",
            "conversational",
            "custom_code",
            "base_model:moonshotai/Kimi-K2.5",
            "base_model:quantized:moonshotai/Kimi-K2.5",
            "license:other",
            "4-bit",
            "region:us",
        ]);
        let id = "mlx-community/Kimi-K2.5";
        assert_eq!(derive_quant(id, &kimi), Some("4-bit".to_string()));
        assert_eq!(derive_arch(id, &kimi), Some("kimi_k25".to_string()));

        // ASR model: no bit tag, no bit in name, no measured arch — both honestly None.
        let parakeet = tags(&[
            "mlx",
            "safetensors",
            "automatic-speech-recognition",
            "speech",
            "audio",
            "FastConformer",
            "Conformer",
            "Parakeet",
            "base_model:nvidia/parakeet-tdt-0.6b-v2",
            "base_model:finetune:nvidia/parakeet-tdt-0.6b-v2",
            "license:cc-by-4.0",
            "region:us",
        ]);
        let id = "mlx-community/parakeet-tdt-0.6b-v2";
        assert_eq!(derive_quant(id, &parakeet), None);
        assert_eq!(derive_arch(id, &parakeet), None);
    }

    #[test]
    fn quant_name_fallback_shapes() {
        let no_tags: Vec<String> = Vec::new();
        for (id, expect) in [
            ("pub/Model-4bit", Some("4-bit")),
            ("pub/Model-8-bit", Some("8-bit")),
            ("pub/Model_6_bit", Some("6-bit")),
            ("pub/Orbit-Explorer", None),
            ("pub/plain-model", None),
        ] {
            assert_eq!(
                derive_quant(id, &no_tags),
                expect.map(str::to_string),
                "id: {id}"
            );
        }
    }

    #[test]
    fn arch_name_fallback_respects_token_boundaries() {
        let no_tags: Vec<String> = Vec::new();
        for (id, expect) in [
            ("mlx-community/Qwen3.5-9B-MLX-4bit", Some("qwen3_5")),
            ("mlx-community/Llama-3.1-8B-Instruct-4bit", Some("llama")),
            ("mlx-community/Kimi-K2.5", Some("kimi_k2")),
            ("pub/Phi-3-mini", Some("phi")),
            // "phi" inside a longer word must not match.
            ("pub/Sapphire-Model", None),
            ("pub/Delphi-Tools", None),
        ] {
            assert_eq!(
                derive_arch(id, &no_tags),
                expect.map(str::to_string),
                "id: {id}"
            );
        }
    }

    #[test]
    fn quant_filter_normalization_is_loud_on_garbage() {
        for input in ["4", "4bit", "4-Bit", "4_bit", " 4-bit "] {
            assert_eq!(
                normalize_quant_filter(input).unwrap(),
                "4-bit",
                "input: {input}"
            );
        }
        assert_eq!(normalize_quant_filter("16bit").unwrap(), "16-bit");
        for bad in ["", "bit", "four-bit", "4bits", "mxfp4", "-4bit"] {
            assert!(
                normalize_quant_filter(bad).is_err(),
                "'{bad}' must be refused"
            );
        }
    }

    #[tokio::test]
    async fn browse_cursor_must_be_an_hf_models_url() {
        let params = BrowseParams {
            cursor: Some("https://evil.example/steal?token".to_string()),
            ..Default::default()
        };
        let err = browse_mlx_models(&params, Some("secret"))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid browse cursor"), "got: {err}");
    }

    // Live API tests: `cargo test -p goose-sidecar -- --ignored` runs them once against
    // the real HF API; the shapes they pin were measured with curl on 2026-08-31.

    #[tokio::test]
    #[ignore = "hits huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_browse_two_page_pagination_via_cursor() {
        let params = BrowseParams {
            author: Some("mlx-community".to_string()),
            limit: 5,
            ..Default::default()
        };
        let page1 = browse_mlx_models(&params, None).await.unwrap();
        assert_eq!(page1.hits.len(), 5);
        let cursor = page1
            .next_cursor
            .clone()
            .expect("page 1 must have a next cursor");

        let page2 = browse_mlx_models(
            &BrowseParams {
                cursor: Some(cursor),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(page2.hits.len(), 5);
        let ids1: std::collections::HashSet<_> = page1.hits.iter().map(|h| &h.id).collect();
        assert!(
            page2.hits.iter().all(|h| !ids1.contains(&h.id)),
            "pages overlap"
        );
        let min1 = page1.hits.iter().map(|h| h.downloads).min().unwrap();
        let max2 = page2.hits.iter().map(|h| h.downloads).max().unwrap();
        assert!(
            max2 <= min1,
            "page 2 must continue the downloads ordering: {max2} > {min1}"
        );
    }

    #[tokio::test]
    #[ignore = "hits huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_browse_author_filter_is_server_side() {
        let params = BrowseParams {
            author: Some("mlx-community".to_string()),
            limit: 10,
            ..Default::default()
        };
        let page = browse_mlx_models(&params, None).await.unwrap();
        assert!(!page.hits.is_empty());
        for hit in &page.hits {
            assert_eq!(hit.author, "mlx-community", "id: {}", hit.id);
            assert!(hit.id.starts_with("mlx-community/"), "id: {}", hit.id);
        }
    }

    #[tokio::test]
    #[ignore = "hits huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_browse_newest_returns_descending_created_at() {
        let params = BrowseParams {
            sort: BrowseSort::Newest,
            limit: 10,
            ..Default::default()
        };
        let page = browse_mlx_models(&params, None).await.unwrap();
        assert!(page.hits.len() >= 2);
        let dates: Vec<&String> = page
            .hits
            .iter()
            .map(|h| {
                h.created_at
                    .as_ref()
                    .expect("expand[]=createdAt must deliver createdAt")
            })
            .collect();
        assert!(
            dates.windows(2).all(|w| w[0] >= w[1]),
            "createdAt not descending: {dates:?}"
        );
    }

    #[test]
    fn list_local_models_on_missing_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let models = list_local_models(&tmp.path().join("nope")).unwrap();
        assert!(models.is_empty());
    }

    #[test]
    fn delete_guard_rejects_traversal_and_wrong_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("models");
        make_model(&root, "pub/model", &[("config.json", 10)]);
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();

        for bad in ["../outside", "pub/../../outside", "pub", "pub/a/b"] {
            assert!(
                delete_local_model(&root, bad).is_err(),
                "'{bad}' was not rejected"
            );
        }
        assert!(outside.exists(), "traversal escaped the models dir");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("pub").join("link")).unwrap();
            let err = delete_local_model(&root, "pub/link")
                .unwrap_err()
                .to_string();
            assert!(err.contains("outside"), "symlink escape allowed: {err}");
            assert!(outside.exists(), "symlink target was deleted");
        }

        delete_local_model(&root, "pub/model").unwrap();
        assert!(!root.join("pub/model").exists());
        assert!(
            delete_local_model(&root, "pub/model").is_err(),
            "double delete must be loud"
        );
    }
}
