//! The CLOUD PROVIDER ROSTER: which cloud providers a swarm node may come from, the exact
//! goose provider-registry key each maps to, and the validate-by-LISTING seam that proves a key
//! by fetching what it can actually run.
//!
//! Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases) — a mechanical move out of swarm.rs,
//! visibility only, tests included, paying for the restream-abort wiring in the root. The
//! dispatch bug the doc below pins (`bedrock` stored, `aws_bedrock` in the registry) is why the
//! name/registry split is load-bearing rather than cosmetic.

use anyhow::{anyhow, Result};

/// The cloud providers the swarm can add nodes from. `name` is what `SwarmDevice.provider`
/// stores and what the CLI/desktop present; `registry` is the goose provider factory's EXACT
/// key (the two differ — storing "bedrock" while the registry says "aws_bedrock" was a live
/// dispatch bug the provider-recon audit caught before any run hit it).
pub(super) struct CloudDef {
    pub(super) name: &'static str,
    pub(super) registry: &'static str,
    pub(super) secret_key: &'static str,
    pub(super) needs_region: bool,
    pub(super) label: &'static str,
}

const CLOUD_DEFS: &[CloudDef] = &[
    CloudDef {
        name: "bedrock",
        registry: "aws_bedrock",
        secret_key: "AWS_BEARER_TOKEN_BEDROCK",
        needs_region: true,
        label: "Amazon Bedrock",
    },
    CloudDef {
        name: "zai",
        registry: "zai",
        secret_key: "ZHIPU_API_KEY",
        needs_region: false,
        label: "Z.ai",
    },
    CloudDef {
        name: "google",
        registry: "google",
        secret_key: "GOOGLE_API_KEY",
        needs_region: false,
        label: "Google Gemini",
    },
    CloudDef {
        name: "deepseek",
        registry: "custom_deepseek",
        secret_key: "DEEPSEEK_API_KEY",
        needs_region: false,
        label: "DeepSeek",
    },
];

pub(super) fn cloud_def(name: &str) -> Option<&'static CloudDef> {
    let lower = name.to_lowercase();
    CLOUD_DEFS.iter().find(|d| d.name == lower)
}

/// goose provider-registry key for a swarm cloud provider name; identity for anything unmapped
/// (the local "lmstudio" and forward-compat names pass through).
pub(super) fn cloud_registry_name(name: &str) -> &str {
    cloud_def(name).map(|d| d.registry).unwrap_or(name)
}

/// The stored/ambient key for a cloud provider: env first (a harness override), then the goose
/// secret store — the same precedence the provider's own from_env uses, so a key that validates
/// here is exactly the key the dispatcher's provider will read.
pub(super) fn cloud_stored_key(def: &CloudDef) -> Option<String> {
    std::env::var(def.secret_key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            goose::config::Config::global()
                .get_secret::<String>(def.secret_key)
                .ok()
        })
}

/// The provider's usable model ids, fetched with ONLY the API key — the shared
/// validate-by-listing seam. A rejection is a bad key; a listing is both proof and roster.
pub(super) async fn cloud_roster(provider: &str, key: &str, region: &str) -> Result<Vec<String>> {
    match provider {
        "bedrock" => bedrock_roster(key, region).await,
        // OpenAI-shaped /models listings (Authorization: Bearer).
        "zai" => openai_style_roster("https://api.z.ai/api/paas/v4/models", key, "Z.ai").await,
        "deepseek" => openai_style_roster("https://api.deepseek.com/models", key, "DeepSeek").await,
        "google" => google_roster(key).await,
        other => anyhow::bail!(
            "unknown cloud provider '{other}' — one of: bedrock, zai, google, deepseek"
        ),
    }
}

/// GET an OpenAI-shaped model listing (`{"data":[{"id":…}]}`) with a bearer key. Pure transport;
/// 401/403 = the key is bad, anything non-2xx is reported verbatim.
async fn openai_style_roster(url: &str, key: &str, label: &str) -> Result<Vec<String>> {
    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| anyhow!("cannot reach {label}: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        anyhow::bail!("{label} REJECTED the API key (HTTP {status}) — bad, expired, or revoked");
    }
    if !status.is_success() {
        anyhow::bail!("{label} answered HTTP {status} listing models");
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow!("{label} model listing was not JSON: {e}"))?;
    let mut ids: Vec<String> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        anyhow::bail!("{label} accepted the key but returned no model ids");
    }
    Ok(ids)
}

/// Gemini's model listing with the provider's own auth scheme (x-goog-api-key), paginated —
/// the in-repo provider never paginates and the catalog exceeds one page. Only models that can
/// generateContent are runnable as swarm nodes. Google answers a BAD key with HTTP 400
/// (API_KEY_INVALID), not 401 — treated as rejection.
async fn google_roster(key: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let mut ids: Vec<String> = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let url = match &token {
            Some(t) => format!(
                "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000&pageToken={t}"
            ),
            None => "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000"
                .to_string(),
        };
        let resp = client
            .get(&url)
            .header("x-goog-api-key", key)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|e| anyhow!("cannot reach Google Gemini: {e}"))?;
        let status = resp.status();
        if matches!(status.as_u16(), 400 | 401 | 403) {
            anyhow::bail!(
                "Google Gemini REJECTED the API key (HTTP {status}) — bad, expired, or restricted"
            );
        }
        if !status.is_success() {
            anyhow::bail!("Google Gemini answered HTTP {status} listing models");
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| anyhow!("Gemini model listing was not JSON: {e}"))?;
        ids.extend(google_ids_from_models(&v));
        token = v["nextPageToken"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        anyhow::bail!("the key authenticates but no generateContent-capable models came back");
    }
    Ok(ids)
}

/// generateContent-capable model ids from a Gemini ListModels page ("models/x" -> "x").
/// Pure/testable.
fn google_ids_from_models(v: &serde_json::Value) -> Vec<String> {
    v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            m["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x.as_str() == Some("generateContent")))
        })
        .filter_map(|m| {
            m["name"]
                .as_str()
                .map(|s| s.strip_prefix("models/").unwrap_or(s).to_string())
        })
        .collect()
}

pub(super) fn bedrock_stored_region() -> Option<String> {
    std::env::var("AWS_REGION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            goose::config::Config::global()
                .get_param::<String>("AWS_REGION")
                .ok()
        })
}

/// Validate the key by LISTING what it can use: the region's system-defined inference profiles
/// (the ids cross-region models must be invoked by) plus on-demand streaming text models. A 200
/// proves the key; 401/403 is a bad/expired/mis-region key, reported as such. Sorted, deduped.
async fn bedrock_roster(key: &str, region: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::new();
    let base = format!("https://bedrock.{region}.amazonaws.com");
    let fetch = |path: String| {
        let client = client.clone();
        let base = base.clone();
        let key = key.to_string();
        async move {
            let resp = client
                .get(format!("{base}{path}"))
                .bearer_auth(&key)
                .timeout(std::time::Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| anyhow!("cannot reach Bedrock in {region}: {e}"))?;
            let status = resp.status();
            if status.as_u16() == 401 || status.as_u16() == 403 {
                anyhow::bail!(
                    "Bedrock in {region} REJECTED the API key (HTTP {status}) — bad, expired, or \
                     issued for a different region"
                );
            }
            if !status.is_success() {
                anyhow::bail!("Bedrock {region}{path} answered HTTP {status}");
            }
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| anyhow!("Bedrock {path} answer was not JSON: {e}"))
        }
    };
    let mut ids: Vec<String> = Vec::new();
    // Cross-region inference profiles: what modern Anthropic/Meta models must be called by.
    let mut token: Option<String> = None;
    loop {
        let q = match &token {
            Some(t) => format!(
                "/inference-profiles?maxResults=1000&typeEquals=SYSTEM_DEFINED&nextToken={t}"
            ),
            None => "/inference-profiles?maxResults=1000&typeEquals=SYSTEM_DEFINED".to_string(),
        };
        let v = fetch(q).await?;
        ids.extend(bedrock_ids_from_profiles(&v));
        token = v["nextToken"].as_str().map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    // On-demand streaming text models (older/regional ids callable directly).
    let v = fetch("/foundation-models".to_string()).await?;
    ids.extend(bedrock_ids_from_models(&v));
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        anyhow::bail!(
            "the key authenticates in {region} but no usable (streaming text) model ids came back — \
             the key's policy may not grant model access in this region"
        );
    }
    Ok(ids)
}

/// ACTIVE system-defined inference profile ids from a ListInferenceProfiles page. Pure/testable.
fn bedrock_ids_from_profiles(v: &serde_json::Value) -> Vec<String> {
    v["inferenceProfileSummaries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["status"].as_str() == Some("ACTIVE"))
        .filter_map(|p| p["inferenceProfileId"].as_str().map(str::to_string))
        .collect()
}

/// On-demand STREAMING TEXT model ids from a ListFoundationModels answer — what an agent can
/// actually run on. Image/embedding models and provisioned-only ids are excluded. Pure/testable.
fn bedrock_ids_from_models(v: &serde_json::Value) -> Vec<String> {
    v["modelSummaries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            let has = |field: &str, want: &str| {
                m[field]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(want)))
            };
            has("outputModalities", "TEXT")
                && has("inferenceTypesSupported", "ON_DEMAND")
                && m["responseStreamingSupported"].as_bool().unwrap_or(false)
        })
        .filter_map(|m| m["modelId"].as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_defs_map_friendly_names_to_real_registry_keys() {
        // The dispatch bug this pins: "bedrock" stored on devices, "aws_bedrock" in the registry.
        assert_eq!(cloud_registry_name("bedrock"), "aws_bedrock");
        assert_eq!(cloud_registry_name("zai"), "zai");
        assert_eq!(cloud_registry_name("google"), "google");
        assert_eq!(cloud_registry_name("deepseek"), "custom_deepseek");
        // identity for local + unknown names (forward compat)
        assert_eq!(cloud_registry_name("lmstudio"), "lmstudio");
        assert_eq!(cloud_registry_name("whatever"), "whatever");
        for d in CLOUD_DEFS {
            assert!(cloud_def(d.name).is_some());
        }
    }

    #[test]
    fn google_roster_parser_keeps_only_generate_content_models() {
        let v = serde_json::json!({"models":[
            {"name":"models/gemini-3.1-pro","supportedGenerationMethods":["generateContent","countTokens"]},
            {"name":"models/embedding-001","supportedGenerationMethods":["embedContent"]},
            {"name":"models/gemini-3.7-flash","supportedGenerationMethods":["generateContent"]}
        ]});
        assert_eq!(
            google_ids_from_models(&v),
            vec!["gemini-3.1-pro".to_string(), "gemini-3.7-flash".to_string()]
        );
    }

    #[test]
    fn bedrock_roster_parsers_keep_only_runnable_ids() {
        // Real ListInferenceProfiles / ListFoundationModels shapes (fields the parsers read).
        let profiles = serde_json::json!({"inferenceProfileSummaries":[
            {"inferenceProfileId":"us.anthropic.claude-haiku-4-5-20251001-v1:0","status":"ACTIVE"},
            {"inferenceProfileId":"us.meta.llama4-maverick-v1:0","status":"INACTIVE"},
            {"status":"ACTIVE"}
        ]});
        assert_eq!(
            bedrock_ids_from_profiles(&profiles),
            vec!["us.anthropic.claude-haiku-4-5-20251001-v1:0".to_string()]
        );
        let models = serde_json::json!({"modelSummaries":[
            {"modelId":"anthropic.claude-3-haiku-20240307-v1:0","outputModalities":["TEXT"],
             "inferenceTypesSupported":["ON_DEMAND"],"responseStreamingSupported":true},
            {"modelId":"stability.sd3-large-v1:0","outputModalities":["IMAGE"],
             "inferenceTypesSupported":["ON_DEMAND"],"responseStreamingSupported":false},
            {"modelId":"anthropic.claude-opus-5-v1:0","outputModalities":["TEXT"],
             "inferenceTypesSupported":["INFERENCE_PROFILE"],"responseStreamingSupported":true},
            {"modelId":"amazon.titan-embed-text-v2:0","outputModalities":["EMBEDDING"],
             "inferenceTypesSupported":["ON_DEMAND"],"responseStreamingSupported":false}
        ]});
        // Only the on-demand streaming TEXT model survives; the profile-only Opus id comes from
        // the profiles listing instead, never from here.
        assert_eq!(
            bedrock_ids_from_models(&models),
            vec!["anthropic.claude-3-haiku-20240307-v1:0".to_string()]
        );
    }
}
