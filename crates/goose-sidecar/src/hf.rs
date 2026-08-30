//! HuggingFace Hub access for the MLX engine: MLX-only model search, repo file listing,
//! background snapshot downloads into a local models dir, and local model inventory.
//!
//! Every network shape here was verified against the live API: `filter=mlx` is the
//! parameter that actually restricts results to MLX repos (`library=mlx` does not), and
//! `expand[]` is required for `lastModified`/`downloads`/`likes` to appear in search hits.

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
