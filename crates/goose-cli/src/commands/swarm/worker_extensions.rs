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
