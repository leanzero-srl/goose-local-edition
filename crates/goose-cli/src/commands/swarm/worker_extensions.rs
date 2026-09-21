//! Legacy worker MCP builders. Saved MCP configurations are resolved by Agent Work first.

use goose::agents::ExtensionConfig;
use std::collections::HashMap;

pub(super) fn build_worker_extension(name: &str) -> Option<ExtensionConfig> {
    build_worker_extension_from(name, super::research_secret)
}

fn build_worker_extension_from(
    name: &str,
    secret: impl Fn(&str) -> Option<String>,
) -> Option<ExtensionConfig> {
    let required = |key| {
        secret(key).or_else(|| {
            eprintln!("worker_extension_unavailable: {name} requires {key} in the environment or swarm.research_keys");
            None
        })
    };
    match name {
        "context7" => Some(ExtensionConfig::Stdio {
            name: "context7".to_string(),
            description: "Upstash Context7 library docs".to_string(),
            cmd: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@upstash/context7-mcp".to_string(),
                "--api-key".to_string(),
                required("CONTEXT7_API_KEY")?,
            ],
            envs: Default::default(),
            env_keys: vec![],
            timeout: Some(120),
            cwd: None,
            bundled: None,
            available_tools: vec![],
        }),
        "web-search" => {
            let bearer = required("WEBSEARCH_BEARER")?;
            let uri = required("WEBSEARCH_URI")?;
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {bearer}"));
            if let Some(k) = secret("SERPER_KEY") {
                headers.insert("X-Serper-Key".to_string(), k);
            }
            if let Some(k) = secret("GITHUB_TOKEN") {
                headers.insert("X-GitHub-Token".to_string(), k);
            }
            Some(ExtensionConfig::StreamableHttp {
                name: "web-search".to_string(),
                description: "Web search + GitHub".to_string(),
                uri,
                envs: Default::default(),
                env_keys: vec![],
                headers,
                timeout: Some(120),
                socket: None,
                bundled: None,
                available_tools: vec![],
            })
        }
        "doc-processor" => {
            let bearer = required("DOCPROC_BEARER")?;
            let uri = required("DOCPROC_URI")?;
            let mut headers = HashMap::new();
            headers.insert("Authorization".to_string(), format!("Bearer {bearer}"));
            Some(ExtensionConfig::StreamableHttp {
                name: "doc-processor".to_string(),
                description: "Document processor".to_string(),
                uri,
                envs: Default::default(),
                env_keys: vec![],
                headers,
                timeout: Some(120),
                socket: None,
                bundled: None,
                available_tools: vec![],
            })
        }
        other => {
            eprintln!("(unknown worker extension: {other})");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_workers_require_an_explicit_endpoint_and_keep_its_credentials() {
        for (name, bearer, endpoint) in [
            ("web-search", "WEBSEARCH_BEARER", "WEBSEARCH_URI"),
            ("doc-processor", "DOCPROC_BEARER", "DOCPROC_URI"),
        ] {
            assert!(build_worker_extension_from(name, |key| (key == bearer)
                .then(|| "fixture".into()))
            .is_none());
            let config = build_worker_extension_from(name, |key| {
                if key == bearer {
                    Some("fixture".into())
                } else if key == endpoint {
                    Some("https://mcp.example.com/custom".into())
                } else {
                    None
                }
            })
            .unwrap();
            let ExtensionConfig::StreamableHttp { uri, headers, .. } = config else {
                panic!("HTTP worker expected")
            };
            assert_eq!(uri, "https://mcp.example.com/custom");
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer fixture")
            );
        }
    }
}

/// THE BENCHMARK INVARIANT at the worker door (frame 1.14 §2.6). A measured run is knowledge-blind:
/// its workers never receive the `memory` or `skills` extension, so recall has nothing to inject and
/// nothing to capture. Today the swarm builds that list from research MCPs only, so this is a stated
/// refusal over a state that already holds — and the day a feature adds `memory` to a worker's menu,
/// the bench path drops it and names it instead of silently learning. Off benchmark: byte-identical.
pub(super) fn knowledge_blind_refusals(
    benchmark: bool,
    extensions: &[ExtensionConfig],
) -> (Vec<ExtensionConfig>, Vec<String>) {
    if !benchmark {
        return (extensions.to_vec(), Vec::new());
    }
    let mut kept = Vec::new();
    let mut refused = Vec::new();
    for ext in extensions {
        let name = ext.name();
        if KNOWLEDGE_EXTENSIONS.contains(&name.as_str()) {
            refused.push(name);
        } else {
            kept.push(ext.clone());
        }
    }
    (kept, refused)
}

/// The extensions that READ or WRITE durable knowledge: the memory store and the skill catalogue.
const KNOWLEDGE_EXTENSIONS: &[&str] = &["memory", "skills"];

#[cfg(test)]
mod knowledge_blind_tests {
    use super::*;

    fn builtin(name: &str) -> ExtensionConfig {
        ExtensionConfig::Builtin {
            name: name.to_string(),
            display_name: None,
            description: String::new(),
            timeout: None,
            bundled: Some(true),
            available_tools: vec![],
        }
    }

    /// A bench worker's session is assembled with `benchmark() == true` and the list contains
    /// neither `memory` nor `skills`, whatever the caller handed in; every other extension rides.
    #[test]
    fn a_benchmark_worker_never_gets_memory_or_skills() {
        let handed = vec![builtin("memory"), builtin("web-search"), builtin("skills")];
        let (kept, refused) = knowledge_blind_refusals(true, &handed);
        let kept: Vec<String> = kept.iter().map(|e| e.name()).collect();
        assert_eq!(kept, vec!["web-search".to_string()]);
        assert_eq!(refused, vec!["memory".to_string(), "skills".to_string()]);
    }

    /// Off benchmark the list is byte-identical: the invariant changes nothing for a real run.
    #[test]
    fn an_attended_worker_keeps_every_extension() {
        let handed = vec![builtin("memory"), builtin("web-search"), builtin("skills")];
        let (kept, refused) = knowledge_blind_refusals(false, &handed);
        assert_eq!(kept.len(), 3);
        assert!(refused.is_empty());
    }
}
