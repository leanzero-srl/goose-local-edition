//! HuggingFace Hub access for the MLX engine: MLX-only model search and paginated
//! browse, repo file listing, background snapshot downloads into a local models dir,
//! and local model inventory.
//!
//! Every network shape here was verified against the live API: `filter=mlx` is the
//! parameter that actually restricts results to MLX repos (`library=mlx` does not),
//! `expand[]` is required for `lastModified`/`downloads`/`likes`/`createdAt`/`tags` to
//! appear in hits, repeated `filter` params AND-combine, and pagination rides the Link
//! rel="next" header.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

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
    /// Files provably missing or unfinished: shards the safetensors index names that are
    /// absent/empty, plus `.part` leftovers not already counted (0 when complete).
    pub missing_files: u32,
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
    /// `config.model_type` as the server reports it (`expand[]=config`, exact) when
    /// present; else a DERIVED display fallback from tags/name. Not what the server
    /// filtered on unless the caller also set `BrowseParams::arch`.
    pub arch: Option<String>,
    /// ESTIMATE of the weight payload in bytes, computed from the server's
    /// `safetensors.parameters` dtype counts × dtype width (measured within 0.003% of
    /// the true safetensors byte sum on live repos, but it excludes tokenizer/config
    /// files and safetensors headers). Absent when the server reports no safetensors
    /// info or an unknown dtype appears — never a guess.
    pub size_bytes_estimate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowsePage {
    pub hits: Vec<BrowseHit>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiHitConfig {
    #[serde(default)]
    model_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiSafetensorsInfo {
    #[serde(default)]
    parameters: HashMap<String, u64>,
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
    #[serde(default)]
    config: Option<ApiHitConfig>,
    #[serde(default)]
    safetensors: Option<ApiSafetensorsInfo>,
}

/// safetensors dtype width in BITS (the spec's fixed widths). Unknown dtypes return
/// None so a size estimate is dropped whole rather than partially fabricated.
fn dtype_width_bits(dtype: &str) -> Option<u64> {
    Some(match dtype {
        "F64" | "I64" | "U64" => 64,
        "F32" | "I32" | "U32" => 32,
        "F16" | "BF16" | "I16" | "U16" => 16,
        "F8_E4M3" | "F8_E5M2" | "I8" | "U8" | "BOOL" => 8,
        _ => return None,
    })
}

fn size_estimate_from_safetensors(info: &ApiSafetensorsInfo) -> Option<u64> {
    if info.parameters.is_empty() {
        return None;
    }
    let mut total_bits: u64 = 0;
    for (dtype, count) in &info.parameters {
        total_bits = total_bits.checked_add(dtype_width_bits(dtype)?.checked_mul(*count)?)?;
    }
    Some(total_bits / 8)
}

/// DISPLAY-FALLBACK architecture tags, measured live on MLX repos (2026-08-31): every
/// entry returned hits for `filter=mlx&filter=<tag>`. Used only when a hit carries no
/// `config.model_type` (42 of 750 sampled repos) — the exact arch source is the
/// server's `expand[]=config` model_type, and the FILTER VOCABULARY comes from the
/// live crawl in `browse_filter_vocab`, never from this list.
/// Derivation picks the LONGEST entry that matches, so `qwen3_5` beats `qwen`.
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

/// Quant-shaped tag classifier, measured on 750 live MLX repos (2026-08-31): bit-width
/// tags (`1-bit`…`8-bit` observed), precision families with a numeric width
/// (bf16/fp16/fp32/int4/int8/mxfp4/nvfp4 observed), and `awq`. A SHAPE rule, not an
/// enumeration of values — the actual vocabulary values always come live from HF.
fn is_quant_tag(tag: &str) -> bool {
    if is_bit_tag(tag) {
        return true;
    }
    if tag == "awq" {
        return true;
    }
    ["mxfp", "nvfp", "bf", "fp", "int"].iter().any(|prefix| {
        tag.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Accepts `4`, `4bit`, `4-bit`, `4_bit` (any case) and yields the HF tag form `4-bit`;
/// non-bit-width quant tags (bf16, mxfp4, awq, …) pass through as literal tags — the
/// filter matches HF tags by design, so an untagged precision honestly narrows to
/// nothing. Anything else is refused loudly — a quant filter that cannot become a real
/// tag would silently return the unfiltered listing.
fn normalize_quant_filter(input: &str) -> Result<String> {
    let s = input.trim().to_ascii_lowercase();
    if !s.starts_with(|c: char| c.is_ascii_digit()) {
        ensure!(
            is_quant_tag(&s),
            "invalid quant filter '{input}': expected a bit width like '4-bit', or a precision tag like bf16/fp16/mxfp4/int8/awq"
        );
        return Ok(s);
    }
    let digits_end = s
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(s.len());
    let (digits, rest) = s.split_at(digits_end);
    ensure!(
        !digits.is_empty() && matches!(rest, "" | "bit" | "-bit" | "_bit"),
        "invalid quant filter '{input}': expected a bit width like '4-bit', or a precision tag like bf16/fp16/mxfp4/int8/awq"
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
            // config carries model_type (exact arch), safetensors carries dtype counts
            // (size estimate); both ride the same LIST call — measured, no per-row calls.
            for field in [
                "downloads",
                "likes",
                "createdAt",
                "lastModified",
                "tags",
                "config",
                "safetensors",
            ] {
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
            let arch = h
                .config
                .as_ref()
                .and_then(|c| c.model_type.clone())
                .or_else(|| derive_arch(&h.id, &h.tags));
            let size_bytes_estimate = h
                .safetensors
                .as_ref()
                .and_then(size_estimate_from_safetensors);
            let author = h.id.split('/').next().unwrap_or_default().to_string();
            BrowseHit {
                author,
                downloads: h.downloads,
                likes: h.likes,
                created_at: h.created_at,
                last_modified: h.last_modified,
                quant,
                arch,
                size_bytes_estimate,
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

/// Dynamic filter vocabularies for the browse UI, aggregated live from HF.
///
/// Source decision (measured 2026-08-31): `/api/models-tags-by-type` carries NO arch
/// bucket and only {4-bit, 8-bit} as quant-ish entries in `other` (negative control:
/// `qwen3_5` absent from all 576KB of it), while a bounded crawl of `filter=mlx`
/// listings with `expand[]=tags&expand[]=config` yielded 7 bit-width quants plus 7
/// precision families, 80 distinct `config.model_type` archs, and 175 authors — and
/// `model_type` was ALSO a tag on 100% of 708 sampled repos, so every arch entry is
/// genuinely server-side filterable via `filter=<arch>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseFilterVocab {
    /// Quant-shaped tags observed, ordered by observed frequency (desc, then name).
    pub quants: Vec<String>,
    /// Distinct `config.model_type` values observed (each proven filterable as a tag).
    pub archs: Vec<String>,
    /// Distinct repo publishers observed.
    pub authors: Vec<String>,
    /// Distinct repos the vocabulary was aggregated from — this is a TOP-N SAMPLE
    /// (downloads + newest sweeps), not an exhaustive census of every MLX repo.
    pub sampled_repos: u32,
    /// Unix epoch seconds when the crawl ran.
    pub computed_at_epoch_s: u64,
    /// Set when this vocabulary is served STALE because a TTL refresh failed; carries
    /// the refresh failure. Absent on a fresh vocabulary.
    pub refresh_error: Option<String>,
}

/// Request budget: the crawl is at most `10 + 5` LIST requests of 50 rows each, run
/// lazily on first demand and then at most once per TTL. The newest sweep exists
/// because it surfaced 14 archs the downloads sweep missed (measured 2026-08-31).
const VOCAB_PAGES_BY_DOWNLOADS: usize = 10;
const VOCAB_PAGES_BY_NEWEST: usize = 5;
const VOCAB_PAGE_LIMIT: u32 = 50;
const VOCAB_TTL: Duration = Duration::from_secs(60 * 60);

static VOCAB_CACHE: Mutex<Option<(Instant, BrowseFilterVocab)>> = Mutex::new(None);

#[derive(Debug, Deserialize)]
struct VocabApiHit {
    id: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    config: Option<ApiHitConfig>,
}

#[derive(Default)]
struct VocabAgg {
    quants: HashMap<String, u64>,
    archs: HashMap<String, u64>,
    authors: HashMap<String, u64>,
    repo_ids: HashSet<String>,
}

fn freq_sorted(map: HashMap<String, u64>) -> Vec<String> {
    let mut entries: Vec<(String, u64)> = map.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.into_iter().map(|(k, _)| k).collect()
}

async fn vocab_sweep(
    client: &reqwest::Client,
    token: Option<&str>,
    sort_params: &str,
    pages: usize,
    agg: &mut VocabAgg,
) -> Result<()> {
    let mut url = format!(
        "{HF_BASE}/api/models?filter=mlx&limit={VOCAB_PAGE_LIMIT}&expand[]=tags&expand[]=config&{sort_params}"
    );
    for _ in 0..pages {
        let mut req = client.get(&url);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.with_context(|| format!("GET {url}"))?;
        let next = next_page_url(resp.headers());
        let body = read_success_body(resp, "HuggingFace vocabulary crawl").await?;
        let hits: Vec<VocabApiHit> =
            serde_json::from_str(&body).context("parsing HuggingFace vocabulary page")?;
        for hit in hits {
            if let Some(author) = hit.id.split('/').next() {
                *agg.authors.entry(author.to_string()).or_default() += 1;
            }
            if let Some(mt) = hit.config.as_ref().and_then(|c| c.model_type.as_ref()) {
                *agg.archs.entry(mt.clone()).or_default() += 1;
            }
            for tag in &hit.tags {
                if is_quant_tag(tag) {
                    *agg.quants.entry(tag.clone()).or_default() += 1;
                }
            }
            agg.repo_ids.insert(hit.id);
        }
        match next {
            Some(next) => url = next,
            None => break,
        }
    }
    Ok(())
}

async fn crawl_filter_vocab(token: Option<&str>) -> Result<BrowseFilterVocab> {
    let client = api_client()?;
    let mut agg = VocabAgg::default();
    vocab_sweep(
        &client,
        token,
        "sort=downloads",
        VOCAB_PAGES_BY_DOWNLOADS,
        &mut agg,
    )
    .await?;
    vocab_sweep(
        &client,
        token,
        "sort=createdAt&direction=-1",
        VOCAB_PAGES_BY_NEWEST,
        &mut agg,
    )
    .await?;
    let computed_at_epoch_s = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(BrowseFilterVocab {
        quants: freq_sorted(agg.quants),
        archs: freq_sorted(agg.archs),
        authors: freq_sorted(agg.authors),
        sampled_repos: agg.repo_ids.len() as u32,
        computed_at_epoch_s,
        refresh_error: None,
    })
}

/// Serve the filter vocabulary from the in-process cache, refreshing at most once per
/// `VOCAB_TTL`. A failed refresh with a stale cache serves the stale vocabulary with
/// `refresh_error` set — an explicit signal, never a silent substitute; with no cache
/// at all the failure propagates.
pub async fn browse_filter_vocab(token: Option<&str>) -> Result<BrowseFilterVocab> {
    if let Some((at, vocab)) = VOCAB_CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < VOCAB_TTL {
            return Ok(vocab.clone());
        }
    }
    match crawl_filter_vocab(token).await {
        Ok(vocab) => {
            *VOCAB_CACHE.lock().unwrap() = Some((Instant::now(), vocab.clone()));
            Ok(vocab)
        }
        Err(e) => {
            if let Some((_, stale)) = VOCAB_CACHE.lock().unwrap().as_ref() {
                let mut vocab = stale.clone();
                vocab.refresh_error = Some(format!("{e:#}"));
                return Ok(vocab);
            }
            Err(e)
        }
    }
}

/// Everything the model-card modal needs. Request budget: exactly 3 bounded calls —
/// the single-model API (metadata), the tree listing (files+sizes, HF-paginated), and
/// the README resolve. A repo without a README yields `readme_markdown: None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCard {
    pub readme_markdown: Option<String>,
    /// True when the README exceeded `README_MAX_BYTES` and was cut.
    pub readme_truncated: bool,
    pub files: Vec<RepoFile>,
    pub total_bytes: u64,
    pub tags: Vec<String>,
    pub downloads: u64,
    pub likes: u64,
    pub license: Option<String>,
    pub created_at: Option<String>,
    pub last_modified: Option<String>,
}

const README_MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct ModelInfoApi {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default, rename = "createdAt")]
    created_at: Option<String>,
    #[serde(default, rename = "lastModified")]
    last_modified: Option<String>,
    #[serde(default, rename = "cardData")]
    card_data: Option<ModelCardData>,
}

#[derive(Debug, Deserialize)]
struct ModelCardData {
    #[serde(default)]
    license: Option<serde_json::Value>,
}

fn license_from(info: &ModelInfoApi) -> Option<String> {
    if let Some(serde_json::Value::String(s)) =
        info.card_data.as_ref().and_then(|c| c.license.as_ref())
    {
        return Some(s.clone());
    }
    info.tags
        .iter()
        .find_map(|t| t.strip_prefix("license:").map(str::to_string))
}

pub async fn model_card(repo_id: &str, token: Option<&str>) -> Result<ModelCard> {
    validate_model_id(repo_id)?;
    let client = api_client()?;

    let mut req = client.get(format!("{HF_BASE}/api/models/{repo_id}"));
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("GET api/models/{repo_id}"))?;
    let body = read_success_body(resp, &format!("HuggingFace model info for '{repo_id}'")).await?;
    let info: ModelInfoApi =
        serde_json::from_str(&body).context("parsing HuggingFace model info")?;

    let files = repo_files(repo_id, token).await?;
    let total_bytes = files.iter().map(|f| f.size).sum();

    let readme_url = format!("{HF_BASE}/{repo_id}/resolve/main/README.md");
    let mut req = client.get(&readme_url);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("GET {readme_url}"))?;
    let (readme_markdown, readme_truncated) = if resp.status() == reqwest::StatusCode::NOT_FOUND {
        (None, false)
    } else {
        let body = read_success_body(resp, &format!("README for '{repo_id}'")).await?;
        if body.len() > README_MAX_BYTES {
            let mut cut = README_MAX_BYTES;
            while !body.is_char_boundary(cut) {
                cut -= 1;
            }
            (Some(body[..cut].to_string()), true)
        } else {
            (Some(body), false)
        }
    };

    let license = license_from(&info);
    Ok(ModelCard {
        readme_markdown,
        readme_truncated,
        files,
        total_bytes,
        tags: info.tags,
        downloads: info.downloads,
        likes: info.likes,
        license,
        created_at: info.created_at,
        last_modified: info.last_modified,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Queued,
    Downloading,
    Paused,
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
    /// Files this attempt had to restart from zero because their on-disk `.part` or the
    /// server's range answer disagreed with the tree listing's size.
    pub restarted_files: Vec<String>,
    pub error: Option<String>,
}

/// Control words the download task polls between chunks.
const CTRL_RUN: u8 = 0;
const CTRL_PAUSE: u8 = 1;
const CTRL_CANCEL: u8 = 2;

struct DownloadEntry {
    progress: DownloadProgress,
    control: Arc<AtomicU8>,
}

#[derive(Default)]
pub struct DownloadTracker {
    downloads: Arc<Mutex<HashMap<String, DownloadEntry>>>,
}

enum DownloadOutcome {
    Done,
    Paused,
    Cancelled,
}

impl DownloadTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register and spawn a background snapshot download of `repo_id` into
    /// `{models_dir}/{repo_id}`. Errors if that repo already has an active or paused
    /// download (a paused one wants `resume` or `cancel`, not a second start).
    pub fn start_download(
        &self,
        repo_id: &str,
        models_dir: &Path,
        token: Option<String>,
    ) -> Result<()> {
        validate_model_id(repo_id)?;
        {
            let map = self.downloads.lock().unwrap();
            if let Some(existing) = map.get(repo_id) {
                match existing.progress.state {
                    DownloadState::Queued | DownloadState::Downloading => {
                        bail!("download already in progress for '{repo_id}'")
                    }
                    DownloadState::Paused => {
                        bail!("download for '{repo_id}' is paused; resume or cancel it")
                    }
                    DownloadState::Done | DownloadState::Failed | DownloadState::Cancelled => {}
                }
            }
        }
        self.spawn_task(repo_id, models_dir, token, 0, 0);
        Ok(())
    }

    /// Ask the running task for `repo_id` to stop cleanly between chunks, KEEPING every
    /// `.part` on disk. The state flips to `paused` when the task actually stops.
    pub fn pause(&self, repo_id: &str) -> Result<()> {
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
        entry.control.store(CTRL_PAUSE, Ordering::SeqCst);
        Ok(())
    }

    /// Resume a paused/failed/cancelled download — or one only present as partial files
    /// on disk from an earlier session. Spawns a fresh task that skips files whose
    /// on-disk size matches the tree listing and continues `.part` files via HTTP Range.
    pub fn resume(&self, repo_id: &str, models_dir: &Path, token: Option<String>) -> Result<()> {
        validate_model_id(repo_id)?;
        let (prev_total, prev_downloaded) = {
            let map = self.downloads.lock().unwrap();
            match map.get(repo_id) {
                Some(entry) => match entry.progress.state {
                    DownloadState::Queued | DownloadState::Downloading => {
                        bail!("download for '{repo_id}' is already active")
                    }
                    DownloadState::Done => {
                        bail!("download for '{repo_id}' is already complete")
                    }
                    DownloadState::Paused | DownloadState::Failed | DownloadState::Cancelled => {
                        (entry.progress.total_bytes, entry.progress.downloaded_bytes)
                    }
                },
                None => {
                    ensure!(
                        models_dir.join(repo_id).is_dir(),
                        "no download tracked for '{repo_id}' and no partial files on disk to resume"
                    );
                    (0, 0)
                }
            }
        };
        self.spawn_task(repo_id, models_dir, token, prev_total, prev_downloaded);
        Ok(())
    }

    fn spawn_task(
        &self,
        repo_id: &str,
        models_dir: &Path,
        token: Option<String>,
        prev_total: u64,
        prev_downloaded: u64,
    ) {
        let control = Arc::new(AtomicU8::new(CTRL_RUN));
        self.downloads.lock().unwrap().insert(
            repo_id.to_string(),
            DownloadEntry {
                progress: DownloadProgress {
                    state: DownloadState::Queued,
                    total_bytes: prev_total,
                    downloaded_bytes: prev_downloaded,
                    current_file: None,
                    restarted_files: Vec::new(),
                    error: None,
                },
                control: Arc::clone(&control),
            },
        );

        let downloads = Arc::clone(&self.downloads);
        let repo_id = repo_id.to_string();
        let models_dir = models_dir.to_path_buf();
        tokio::spawn(async move {
            let result = run_download(
                &repo_id,
                &models_dir,
                token.as_deref(),
                &downloads,
                &control,
            )
            .await;
            let mut map = downloads.lock().unwrap();
            let Some(entry) = map.get_mut(&repo_id) else {
                return;
            };
            entry.progress.current_file = None;
            match result {
                Ok(DownloadOutcome::Done) => entry.progress.state = DownloadState::Done,
                Ok(DownloadOutcome::Paused) => entry.progress.state = DownloadState::Paused,
                Ok(DownloadOutcome::Cancelled) => {
                    entry.progress.state = DownloadState::Cancelled;
                    entry.progress.downloaded_bytes = 0;
                }
                Err(e) => {
                    entry.progress.state = DownloadState::Failed;
                    entry.progress.error = Some(format!("{e:#}"));
                }
            }
        });
    }

    pub fn progress(&self, repo_id: &str) -> Option<DownloadProgress> {
        self.downloads
            .lock()
            .unwrap()
            .get(repo_id)
            .map(|e| e.progress.clone())
    }

    /// Cancel a download AND delete its on-disk claim: every `.part` and the whole
    /// partial `{models_dir}/{repo_id}` directory. For an active task the deletion runs
    /// in the task as it stops; for a paused/failed one it happens here, synchronously.
    pub fn cancel(&self, repo_id: &str, models_dir: &Path) -> Result<()> {
        let mut map = self.downloads.lock().unwrap();
        let Some(entry) = map.get_mut(repo_id) else {
            bail!("no download tracked for '{repo_id}'");
        };
        match entry.progress.state {
            DownloadState::Queued | DownloadState::Downloading => {
                entry.control.store(CTRL_CANCEL, Ordering::SeqCst);
                Ok(())
            }
            DownloadState::Paused | DownloadState::Failed => {
                remove_partial_repo(models_dir, repo_id)?;
                entry.progress.state = DownloadState::Cancelled;
                entry.progress.downloaded_bytes = 0;
                entry.progress.current_file = None;
                Ok(())
            }
            DownloadState::Done => {
                bail!("download for '{repo_id}' is already complete; delete the model instead")
            }
            DownloadState::Cancelled => bail!("download for '{repo_id}' is already cancelled"),
        }
    }

    #[cfg(test)]
    fn seed_state(&self, repo_id: &str, state: DownloadState) {
        self.downloads.lock().unwrap().insert(
            repo_id.to_string(),
            DownloadEntry {
                progress: DownloadProgress {
                    state,
                    total_bytes: 100,
                    downloaded_bytes: 50,
                    current_file: None,
                    restarted_files: Vec::new(),
                    error: None,
                },
                control: Arc::new(AtomicU8::new(CTRL_RUN)),
            },
        );
    }
}

/// Delete `{models_dir}/{repo_id}` with the same canonicalize guard as
/// `delete_local_model`; a directory that never got created counts as already deleted.
fn remove_partial_repo(models_dir: &Path, repo_id: &str) -> Result<()> {
    validate_model_id(repo_id)?;
    let target = models_dir.join(repo_id);
    if !target.exists() {
        return Ok(());
    }
    let canonical_root = models_dir
        .canonicalize()
        .with_context(|| format!("models dir {} does not resolve", models_dir.display()))?;
    let canonical_target = target
        .canonicalize()
        .with_context(|| format!("partial repo {} does not resolve", target.display()))?;
    ensure!(
        canonical_target.starts_with(&canonical_root),
        "refusing to delete '{repo_id}': {} resolves outside {}",
        canonical_target.display(),
        canonical_root.display()
    );
    std::fs::remove_dir_all(&canonical_target)
        .with_context(|| format!("deleting {}", canonical_target.display()))
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

enum FileOutcome {
    Done,
    Paused,
    Cancelled,
    /// The server refused or contradicted the requested byte range (HTTP 200/416, or a
    /// Content-Range total disagreeing with the tree listing) — restart from zero.
    RangeRejected,
}

async fn run_download(
    repo_id: &str,
    models_dir: &Path,
    token: Option<&str>,
    downloads: &Arc<Mutex<HashMap<String, DownloadEntry>>>,
    control: &AtomicU8,
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
        match control.load(Ordering::SeqCst) {
            CTRL_PAUSE => return Ok(DownloadOutcome::Paused),
            CTRL_CANCEL => {
                remove_partial_repo(models_dir, repo_id)?;
                return Ok(DownloadOutcome::Cancelled);
            }
            _ => {}
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

        // A .part shorter than the tree listing's size continues via HTTP Range
        // (measured: resolve URLs answer 206 + Content-Range through their redirects);
        // one at or past it contradicts the listing and restarts from zero.
        let mut resume_from = match tokio::fs::metadata(&part).await {
            Ok(meta) if meta.is_file() && meta.len() > 0 && meta.len() < file.size => meta.len(),
            Ok(meta) if meta.is_file() && meta.len() >= file.size => {
                update_progress(downloads, repo_id, |p| {
                    p.restarted_files.push(file.path.clone())
                });
                0
            }
            _ => 0,
        };

        loop {
            let mut file_bytes: u64 = resume_from;
            update_progress(downloads, repo_id, |p| {
                p.downloaded_bytes = downloaded + file_bytes;
            });
            let on_chunk = |written: u64| {
                file_bytes += written;
                update_progress(downloads, repo_id, |p| {
                    p.downloaded_bytes = downloaded + file_bytes;
                });
            };
            match download_one_file(
                &client,
                &url,
                &part,
                token,
                control,
                resume_from,
                file.size,
                on_chunk,
            )
            .await
            {
                Ok(FileOutcome::Paused) => return Ok(DownloadOutcome::Paused),
                Ok(FileOutcome::Cancelled) => {
                    remove_partial_repo(models_dir, repo_id)?;
                    return Ok(DownloadOutcome::Cancelled);
                }
                Ok(FileOutcome::RangeRejected) => {
                    update_progress(downloads, repo_id, |p| {
                        p.restarted_files.push(file.path.clone())
                    });
                    resume_from = 0;
                    continue;
                }
                Ok(FileOutcome::Done) => {
                    tokio::fs::rename(&part, &dest)
                        .await
                        .with_context(|| format!("renaming {} into place", part.display()))?;
                    downloaded += file.size;
                    update_progress(downloads, repo_id, |p| p.downloaded_bytes = downloaded);
                    break;
                }
                Err(e) => {
                    // Keep the .part: a later resume continues it instead of paying
                    // the bytes again. Only cancel removes partials.
                    return Err(e);
                }
            }
        }
    }
    Ok(DownloadOutcome::Done)
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

#[allow(clippy::too_many_arguments)]
async fn download_one_file(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    token: Option<&str>,
    control: &AtomicU8,
    resume_from: u64,
    expected_size: u64,
    mut on_chunk: impl FnMut(u64),
) -> Result<FileOutcome> {
    let mut req = client.get(url);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    if resume_from > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let mut resp = req.send().await.with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let appending = if resume_from > 0 {
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(FileOutcome::RangeRejected);
        }
        ensure!(
            status.is_success(),
            "GET {url} (range from {resume_from}) returned HTTP {status}"
        );
        if status != reqwest::StatusCode::PARTIAL_CONTENT {
            // 200: the server ignored the range and is sending the whole file.
            return Ok(FileOutcome::RangeRejected);
        }
        if content_range_total(resp.headers()) != Some(expected_size) {
            // 206 for a different total than the tree listed: the file changed upstream.
            return Ok(FileOutcome::RangeRejected);
        }
        true
    } else {
        ensure!(status.is_success(), "GET {url} returned HTTP {status}");
        false
    };

    let mut out = if appending {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part)
            .await
            .with_context(|| format!("opening {} for append", part.display()))?
    } else {
        tokio::fs::File::create(part)
            .await
            .with_context(|| format!("creating {}", part.display()))?
    };
    loop {
        match control.load(Ordering::SeqCst) {
            CTRL_PAUSE => {
                out.flush()
                    .await
                    .with_context(|| format!("flushing {}", part.display()))?;
                return Ok(FileOutcome::Paused);
            }
            CTRL_CANCEL => return Ok(FileOutcome::Cancelled),
            _ => {}
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
    Ok(FileOutcome::Done)
}

/// Two-level walk of `models_dir`: a model is any `publisher/name` directory containing
/// `config.json`. Completeness is judged against the model's OWN manifest when it has
/// one: with `model.safetensors.index.json`, every shard the index names must be present
/// with nonzero size and no `.part` may remain (a cancelled multi-shard download whose
/// first shards landed used to masquerade as complete). Single-file models keep the
/// older rule: at least one `*.safetensors` and no `.part`. A missing `models_dir` is
/// the legitimate nothing-downloaded-yet state.
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
            let assessment = assess_model_dir(&dir);
            models.push(LocalModel {
                id: format!("{publisher_name}/{}", entry.file_name().to_string_lossy()),
                size_bytes: assessment.size_bytes,
                complete: assessment.missing_files == 0,
                missing_files: assessment.missing_files,
            });
        }
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

struct ModelDirAssessment {
    size_bytes: u64,
    missing_files: u32,
}

const SAFETENSORS_INDEX: &str = "model.safetensors.index.json";

#[derive(Deserialize)]
struct SafetensorsIndex {
    weight_map: HashMap<String, String>,
}

fn assess_model_dir(dir: &Path) -> ModelDirAssessment {
    let mut size_bytes = 0u64;
    // Relative paths ('/'-joined) → size, plus the .part leftovers separately.
    let mut files: HashMap<String, u64> = HashMap::new();
    let mut parts: Vec<String> = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel_dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir.join(&rel_dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            let rel = rel_dir.join(entry.file_name());
            if meta.is_dir() {
                stack.push(rel);
            } else if meta.is_file() {
                size_bytes += meta.len();
                let rel = rel.to_string_lossy().into_owned();
                match rel.strip_suffix(".part") {
                    Some(_) => parts.push(rel),
                    None => {
                        files.insert(rel, meta.len());
                    }
                }
            }
        }
    }

    let missing_files = match std::fs::read(dir.join(SAFETENSORS_INDEX)) {
        Ok(bytes) => match serde_json::from_slice::<SafetensorsIndex>(&bytes) {
            Ok(index) => {
                let shards: HashSet<&String> = index.weight_map.values().collect();
                let missing: HashSet<&str> = shards
                    .iter()
                    .filter(|s| !matches!(files.get(s.as_str()), Some(size) if *size > 0))
                    .map(|s| s.as_str())
                    .collect();
                let stray_parts = parts
                    .iter()
                    .filter(|p| {
                        p.strip_suffix(".part")
                            .is_none_or(|target| !missing.contains(target))
                    })
                    .count();
                (missing.len() + stray_parts) as u32
            }
            // An unreadable index (e.g. itself a truncated download) cannot prove any
            // shard present: the index is the one provably broken file.
            Err(_) => 1 + parts.len() as u32,
        },
        Err(_) => {
            let has_safetensors = files.keys().any(|f| f.ends_with(".safetensors"));
            parts.len() as u32 + if has_safetensors { 0 } else { 1 }
        }
    };
    ModelDirAssessment {
        size_bytes,
        missing_files,
    }
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
        assert_eq!(by_id("pub/complete").missing_files, 0);
        assert!(!by_id("pub/partial").complete, ".part must mark incomplete");
        assert_eq!(by_id("pub/partial").missing_files, 1);
        assert!(
            !by_id("pub/no-weights").complete,
            "no safetensors must mark incomplete"
        );
        assert_eq!(by_id("pub/no-weights").missing_files, 1);
    }

    /// The owner's cancel test left multi-shard repos on disk whose finished shards
    /// masqueraded as complete once no `.part` remained. With the safetensors index
    /// present, every named shard must exist with nonzero size.
    #[test]
    fn sharded_model_completeness_follows_the_safetensors_index() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let index = r#"{"metadata":{"total_size":300},"weight_map":{
            "a.w":"model-00001-of-00003.safetensors",
            "b.w":"model-00002-of-00003.safetensors",
            "c.w":"model-00003-of-00003.safetensors"}}"#;

        // Cancelled after shard 1: no .part, one real shard — the old rule called this
        // complete.
        make_model(
            root,
            "pub/cancelled-midway",
            &[
                ("config.json", 10),
                ("model.safetensors.index.json", 0),
                ("model-00001-of-00003.safetensors", 100),
            ],
        );
        std::fs::write(
            root.join("pub/cancelled-midway/model.safetensors.index.json"),
            index,
        )
        .unwrap();

        // All three shards present.
        make_model(
            root,
            "pub/whole",
            &[
                ("config.json", 10),
                ("model.safetensors.index.json", 0),
                ("model-00001-of-00003.safetensors", 100),
                ("model-00002-of-00003.safetensors", 100),
                ("model-00003-of-00003.safetensors", 100),
            ],
        );
        std::fs::write(root.join("pub/whole/model.safetensors.index.json"), index).unwrap();

        // Zero-size shard is not a shard; a mid-shard .part must not double-count.
        make_model(
            root,
            "pub/zero-shard",
            &[
                ("config.json", 10),
                ("model.safetensors.index.json", 0),
                ("model-00001-of-00003.safetensors", 100),
                ("model-00002-of-00003.safetensors", 0),
                ("model-00002-of-00003.safetensors.part", 40),
            ],
        );
        std::fs::write(
            root.join("pub/zero-shard/model.safetensors.index.json"),
            index,
        )
        .unwrap();

        // A corrupt (truncated) index proves nothing about its shards.
        make_model(
            root,
            "pub/broken-index",
            &[
                ("config.json", 10),
                ("model-00001-of-00003.safetensors", 100),
            ],
        );
        std::fs::write(
            root.join("pub/broken-index/model.safetensors.index.json"),
            r#"{"metadata":{"total_size":300},"weight_ma"#,
        )
        .unwrap();

        let models = list_local_models(root).unwrap();
        let by_id = |id: &str| models.iter().find(|m| m.id == id).unwrap();
        assert!(
            !by_id("pub/cancelled-midway").complete,
            "one of three shards must not read as complete"
        );
        assert_eq!(by_id("pub/cancelled-midway").missing_files, 2);
        assert!(by_id("pub/whole").complete);
        assert_eq!(by_id("pub/whole").missing_files, 0);
        assert!(!by_id("pub/zero-shard").complete);
        assert_eq!(
            by_id("pub/zero-shard").missing_files,
            2,
            "shards 2 (zero + .part counted once) and 3"
        );
        assert!(!by_id("pub/broken-index").complete);
        assert_eq!(by_id("pub/broken-index").missing_files, 1);
    }

    #[test]
    fn download_control_state_machine_is_loud() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let tracker = DownloadTracker::new();

        assert!(tracker.pause("pub/none").is_err(), "pause unknown repo");
        assert!(
            tracker.cancel("pub/none", root).is_err(),
            "cancel unknown repo"
        );
        assert!(
            tracker.resume("pub/none", root, None).is_err(),
            "resume with nothing tracked and nothing on disk"
        );

        tracker.seed_state("pub/done", DownloadState::Done);
        assert!(tracker.pause("pub/done").is_err(), "pause a finished one");
        assert!(
            tracker.cancel("pub/done", root).is_err(),
            "cancel a finished one must point at model delete"
        );
        assert!(
            tracker.resume("pub/done", root, None).is_err(),
            "resume a finished one"
        );

        tracker.seed_state("pub/paused", DownloadState::Paused);
        assert!(
            tracker.start_download("pub/paused", root, None).is_err(),
            "start over a paused download must demand resume/cancel"
        );
    }

    #[test]
    fn cancel_of_inactive_download_deletes_the_partial_repo_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        make_model(
            root,
            "pub/halfway",
            &[
                ("config.json", 10),
                ("model-00001-of-00002.safetensors", 100),
                ("model-00002-of-00002.safetensors.part", 40),
            ],
        );
        let tracker = DownloadTracker::new();
        tracker.seed_state("pub/halfway", DownloadState::Paused);

        tracker.cancel("pub/halfway", root).unwrap();
        assert!(
            !root.join("pub/halfway").exists(),
            "cancel must delete the partial repo dir"
        );
        let progress = tracker.progress("pub/halfway").unwrap();
        assert_eq!(progress.state, DownloadState::Cancelled);
        assert_eq!(progress.downloaded_bytes, 0);

        assert!(
            tracker.cancel("pub/halfway", root).is_err(),
            "double cancel must be loud"
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
        // Precision families measured live on MLX repos (2026-08-31) pass through as
        // literal tags.
        for tag in ["bf16", "fp16", "fp32", "mxfp4", "nvfp4", "int8", "awq"] {
            assert_eq!(normalize_quant_filter(tag).unwrap(), tag, "input: {tag}");
        }
        assert_eq!(normalize_quant_filter("MXFP4").unwrap(), "mxfp4");
        for bad in [
            "", "bit", "four-bit", "4bits", "-4bit", "fp", "intx", "gguf",
        ] {
            assert!(
                normalize_quant_filter(bad).is_err(),
                "'{bad}' must be refused"
            );
        }
    }

    #[test]
    fn quant_tag_classifier_matches_measured_families_only() {
        for good in [
            "4-bit", "16-bit", "bf16", "fp32", "int4", "mxfp4", "nvfp4", "awq",
        ] {
            assert!(is_quant_tag(good), "'{good}' must classify as quant");
        }
        for bad in ["internvl", "fpga", "bfloat", "mlx", "gguf", "8bit", "bit-4"] {
            assert!(!is_quant_tag(bad), "'{bad}' must not classify as quant");
        }
    }

    #[test]
    fn size_estimate_uses_dtype_widths_and_refuses_unknown_dtypes() {
        // Real numbers from mlx-community/Qwen3.5-9B-MLX-4bit (measured 2026-08-31):
        // BF16 736,844,272 + U32 1,119,092,736 + F32 768 → 5,950,062,560 bytes,
        // within 0.003% of the repo's true safetensors byte sum 5,950,221,072.
        let info = ApiSafetensorsInfo {
            parameters: [
                ("BF16".to_string(), 736_844_272u64),
                ("U32".to_string(), 1_119_092_736u64),
                ("F32".to_string(), 768u64),
            ]
            .into_iter()
            .collect(),
        };
        assert_eq!(size_estimate_from_safetensors(&info), Some(5_950_062_560));

        let unknown = ApiSafetensorsInfo {
            parameters: [("BF16".to_string(), 100u64), ("MXFP4".to_string(), 100u64)]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            size_estimate_from_safetensors(&unknown),
            None,
            "an unknown dtype must drop the whole estimate, not fabricate part of it"
        );
        let empty = ApiSafetensorsInfo {
            parameters: HashMap::new(),
        };
        assert_eq!(size_estimate_from_safetensors(&empty), None);
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

    #[tokio::test]
    #[ignore = "hits huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_browse_hits_carry_size_estimates_and_arch() {
        let page = browse_mlx_models(
            &BrowseParams {
                limit: 10,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(page.hits.len(), 10);
        let with_size = page
            .hits
            .iter()
            .filter(|h| h.size_bytes_estimate.is_some())
            .count();
        let with_arch = page.hits.iter().filter(|h| h.arch.is_some()).count();
        assert!(with_size >= 8, "only {with_size}/10 hits carried a size");
        assert!(with_arch >= 8, "only {with_arch}/10 hits carried an arch");
        for h in &page.hits {
            if let Some(size) = h.size_bytes_estimate {
                assert!(size > 1_000_000, "{}: absurd size {size}", h.id);
            }
        }
    }

    #[tokio::test]
    #[ignore = "hits huggingface.co ~15 times; run with --features rustls-tls -- --ignored"]
    async fn live_browse_filter_vocab_covers_measured_reality() {
        let vocab = browse_filter_vocab(None).await.unwrap();
        assert!(vocab.refresh_error.is_none());
        assert!(
            vocab.sampled_repos >= 500,
            "sampled {}",
            vocab.sampled_repos
        );
        for q in ["4-bit", "8-bit", "6-bit"] {
            assert!(vocab.quants.iter().any(|v| v == q), "quants miss {q}");
        }
        assert!(vocab.quants.len() >= 5, "quants: {:?}", vocab.quants);
        assert!(vocab.archs.len() >= 30, "archs: {}", vocab.archs.len());
        assert!(
            vocab.archs.iter().any(|a| a.starts_with("qwen")),
            "archs miss the qwen family"
        );
        assert!(
            vocab.authors.iter().any(|a| a == "mlx-community"),
            "authors miss mlx-community"
        );
        // The cache must answer the second call without another crawl (instant).
        let started = Instant::now();
        let again = browse_filter_vocab(None).await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(50), "not cached");
        assert_eq!(again.computed_at_epoch_s, vocab.computed_at_epoch_s);
    }

    #[tokio::test]
    #[ignore = "hits huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_model_card_for_fixture_repo() {
        let card = model_card("mlx-community/Qwen3.5-9B-MLX-4bit", None)
            .await
            .unwrap();
        let readme = card.readme_markdown.expect("fixture repo has a README");
        assert!(
            readme.contains("Qwen"),
            "unexpected README head: {:.80}",
            readme
        );
        assert!(!card.readme_truncated);
        assert!(card.files.len() >= 5, "files: {}", card.files.len());
        assert!(
            card.total_bytes > 5_000_000_000,
            "total: {}",
            card.total_bytes
        );
        assert_eq!(card.license.as_deref(), Some("apache-2.0"));
        assert!(card.created_at.is_some());
        assert!(card.downloads > 0);
        assert!(!card.tags.is_empty());
    }

    /// Seeds a truncated `.part` of the repo's largest file, resumes, and proves the
    /// final bytes equal the served file with NO restart — i.e. reqwest carried the
    /// Range header through HF's resolve redirects and the append hit the right offset.
    #[tokio::test]
    #[ignore = "downloads ~20MB from huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_resume_continues_part_via_range() {
        let repo = "hf-internal-testing/tiny-random-gpt2";
        let tmp = tempfile::tempdir().unwrap();
        let models_dir = tmp.path();
        let dest_root = models_dir.join(repo);
        std::fs::create_dir_all(&dest_root).unwrap();

        let reference = reqwest::get(format!("{HF_BASE}/{repo}/resolve/main/tf_model.h5"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(reference.len(), 8_462_256, "fixture file changed upstream");
        std::fs::write(dest_root.join("tf_model.h5.part"), &reference[..3_000_000]).unwrap();

        let tracker = DownloadTracker::new();
        tracker.resume(repo, models_dir, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        let progress = loop {
            let progress = tracker.progress(repo).unwrap();
            match progress.state {
                DownloadState::Done => break progress,
                DownloadState::Failed => panic!("resume failed: {:?}", progress.error),
                _ => {
                    assert!(Instant::now() < deadline, "resume timed out: {progress:?}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        };
        assert!(
            progress.restarted_files.is_empty(),
            "range resume was rejected and restarted: {:?}",
            progress.restarted_files
        );
        assert_eq!(progress.downloaded_bytes, progress.total_bytes);
        let resumed = std::fs::read(dest_root.join("tf_model.h5")).unwrap();
        assert_eq!(resumed.len(), reference.len());
        assert_eq!(
            resumed,
            &reference[..],
            "appended bytes differ from the served file"
        );
        let models = list_local_models(models_dir).unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].complete, "{:?}", models[0]);
    }

    /// The owner's bug: cancel left partials on disk and the state never moved. Pauses
    /// a real multi-GB download, resumes it, then cancels and proves the repo dir is
    /// GONE. Network cost is bounded: it cancels within a few MB.
    #[tokio::test]
    #[ignore = "downloads a few MB from huggingface.co; run with --features rustls-tls -- --ignored"]
    async fn live_download_lifecycle_pause_resume_cancel() {
        let repo = "mlx-community/Qwen3.5-9B-MLX-4bit";
        let tmp = tempfile::tempdir().unwrap();
        let models_dir = tmp.path();
        let tracker = DownloadTracker::new();
        tracker.start_download(repo, models_dir, None).unwrap();

        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let p = tracker.progress(repo).unwrap();
            if p.state == DownloadState::Downloading && p.downloaded_bytes > 500_000 {
                break;
            }
            assert!(
                p.state != DownloadState::Failed,
                "download failed early: {:?}",
                p.error
            );
            assert!(
                Instant::now() < deadline,
                "never reached Downloading: {p:?}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        tracker.pause(repo).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let paused = loop {
            let p = tracker.progress(repo).unwrap();
            if p.state == DownloadState::Paused {
                break p;
            }
            assert!(Instant::now() < deadline, "never paused: {p:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        assert!(paused.total_bytes > 5_000_000_000, "{paused:?}");
        assert!(
            crate::dir_size_bytes(&models_dir.join(repo)) > 0,
            "pause must keep partial bytes on disk"
        );

        tracker.resume(repo, models_dir, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let p = tracker.progress(repo).unwrap();
            if p.state == DownloadState::Downloading
                && p.downloaded_bytes >= paused.downloaded_bytes
            {
                break;
            }
            assert!(
                p.state != DownloadState::Failed,
                "resume failed: {:?}",
                p.error
            );
            assert!(Instant::now() < deadline, "resume never progressed: {p:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        tracker.cancel(repo, models_dir).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let p = tracker.progress(repo).unwrap();
            if p.state == DownloadState::Cancelled {
                break;
            }
            assert!(Instant::now() < deadline, "never cancelled: {p:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            !models_dir.join(repo).exists(),
            "cancel must delete the whole partial repo dir"
        );
        assert_eq!(tracker.progress(repo).unwrap().downloaded_bytes, 0);
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
