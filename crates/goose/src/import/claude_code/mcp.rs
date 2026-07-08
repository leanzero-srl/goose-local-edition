//! Plan MCP-server imports: Claude Code `mcpServers` → goose `extensions` (CONVERT), with an SSE carve-out.
//!
//! A stdio server (`command`/`args`/`env`) maps to `ExtensionConfig::Stdio`; an http server (`type:"http"`
//! or a bare `url`) maps to `ExtensionConfig::StreamableHttp`. A genuine `type:"sse"` server is SKIP+REPORT
//! because goose no longer supports the SSE transport — auto-mapping it to StreamableHttp would produce a
//! broken extension. Sources: the global `mcpServers` block and (in a later phase) per-project scopes.

use super::manifest::Manifest;
use super::{
    Action, ActionClass, ApplyOutcome, ImportOptions, ImportPlan, ImportReport, ImportType,
};
use crate::agents::extension::{Envs, ExtensionConfig};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

/// Enumerate MCP servers from `~/.claude.json`'s global `mcpServers` block. Read-only.
pub fn plan_mcp(opts: &ImportOptions, plan: &mut ImportPlan) -> Result<()> {
    if !opts.claude_json.is_file() {
        return Ok(());
    }
    let doc: Value = serde_json::from_str(&fs::read_to_string(&opts.claude_json)?)?;
    plan_server_map(doc.get("mcpServers"), "global", plan);
    Ok(())
}

/// Classify each server in a `{name -> server}` map into an Action.
pub(super) fn plan_server_map(map: Option<&Value>, scope: &str, plan: &mut ImportPlan) {
    let Some(servers) = map.and_then(Value::as_object) else {
        return;
    };
    let mut names: Vec<&String> = servers.keys().collect();
    names.sort();

    for name in names {
        let server = &servers[name];
        let transport = server.get("type").and_then(Value::as_str).unwrap_or("");
        let has_url = server.get("url").is_some();

        let (class, note) = if transport == "sse" {
            (
                ActionClass::Skip,
                "SSE transport unsupported by goose — re-add if the endpoint also serves streamable HTTP",
            )
        } else if transport == "http" || has_url {
            (ActionClass::Convert, "http → StreamableHttp")
        } else {
            (ActionClass::Convert, "stdio")
        };

        plan.push(Action {
            import_type: ImportType::Mcp,
            class,
            name: format!("{name} ({scope})"),
            source: None,
            target: "config.yaml extensions".to_string(),
            note: Some(note.to_string()),
        });
    }
}

/// The result of converting one Claude MCP server into a goose extension.
pub struct McpConversion {
    pub config: ExtensionConfig,
    /// Env keys whose values are secret-ish — written to the keyring (name-only into `env_keys`).
    pub secrets: Vec<(String, String)>,
}

/// Env keys never copied into a goose extension (they belong to the host, not the server).
const DISALLOWED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "SHELL",
    "USER",
    "PWD",
    "TMPDIR",
    "NODE_OPTIONS",
];

fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["token", "key", "secret", "password", "auth", "credential"]
        .iter()
        .any(|frag| k.contains(frag))
}

fn is_disallowed(key: &str) -> bool {
    DISALLOWED_ENV.contains(&key) || key.starts_with("DYLD_") || key.starts_with("LD_")
}

/// PURE conversion of a Claude `mcpServers` entry into a goose [`ExtensionConfig`].
/// Returns `None` for a genuine `type:"sse"` server (goose no longer supports SSE — a broken map is worse
/// than a reported skip). `command`→`cmd`, `url`→`uri`; secret-ish env keys go to `env_keys` (+ keyring),
/// host env keys are dropped, the rest pass through as plaintext `envs`.
pub fn server_to_extension(name: &str, server: &Value) -> Option<McpConversion> {
    let transport = server.get("type").and_then(Value::as_str).unwrap_or("");
    if transport == "sse" {
        return None;
    }
    let has_url = server.get("url").is_some();
    let description = server
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // Split the env map into plaintext envs / secret env_keys, dropping host keys.
    let mut envs_map: HashMap<String, String> = HashMap::new();
    let mut env_keys: Vec<String> = Vec::new();
    let mut secrets: Vec<(String, String)> = Vec::new();
    if let Some(env) = server.get("env").and_then(Value::as_object) {
        let mut keys: Vec<&String> = env.keys().collect();
        keys.sort();
        for key in keys {
            if is_disallowed(key) {
                continue;
            }
            let val = env[key].as_str().unwrap_or("").to_string();
            if is_secret_key(key) {
                env_keys.push(key.clone());
                secrets.push((key.clone(), val));
            } else {
                envs_map.insert(key.clone(), val);
            }
        }
    }

    let config = if transport == "http" || has_url {
        ExtensionConfig::StreamableHttp {
            name: name.to_string(),
            description,
            uri: server
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            envs: Envs::new(envs_map),
            env_keys,
            // Preserve http auth/routing headers. Claude stores them plaintext and goose's StreamableHttp
            // headers are likewise plaintext, so dropping them would import an authenticated server as
            // broken (unauthenticated). Non-string header values are skipped.
            headers: server
                .get("headers")
                .and_then(Value::as_object)
                .map(|h| {
                    h.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default(),
            timeout: Some(300),
            socket: None,
            bundled: None,
            available_tools: Vec::new(),
        }
    } else {
        ExtensionConfig::Stdio {
            name: name.to_string(),
            description,
            cmd: server
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            args: server
                .get("args")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            envs: Envs::new(envs_map),
            env_keys,
            timeout: Some(300),
            cwd: None,
            bundled: None,
            available_tools: Vec::new(),
        }
    };
    Some(McpConversion { config, secrets })
}

/// Apply MCP imports. Re-reads `~/.claude.json`, converts each server, and — only when `live` — upserts it
/// into the goose config via `set_extension` (writing secrets to the keyring). When not live (sandboxed
/// tests / non-live phases) the conversion is still recorded in the report but nothing is written to the
/// global config. SSE servers are recorded as skipped.
pub fn apply_mcp(
    opts: &ImportOptions,
    live: bool,
    manifest: &mut Manifest,
    report: &mut ImportReport,
) -> Result<()> {
    if !opts.claude_json.is_file() {
        return Ok(());
    }
    let doc: Value = serde_json::from_str(&fs::read_to_string(&opts.claude_json)?)?;
    let Some(servers) = doc.get("mcpServers").and_then(Value::as_object) else {
        return Ok(());
    };
    let mut names: Vec<&String> = servers.keys().collect();
    names.sort();

    for name in names {
        let server = &servers[name];
        let Some(conversion) = server_to_extension(name, server) else {
            report.record(
                ImportType::Mcp,
                name,
                ApplyOutcome::Skipped,
                Some("sse transport unsupported".to_string()),
            );
            continue;
        };
        if live {
            use crate::config::extensions::{name_to_key, set_extension, ExtensionEntry};
            use crate::config::Config;
            for (key, val) in &conversion.secrets {
                let _ = Config::global().set_secret(key, &Value::String(val.clone()));
            }
            let target = format!("config.yaml extensions ({})", name_to_key(name));
            set_extension(ExtensionEntry {
                enabled: true,
                config: conversion.config,
            });
            manifest.upsert(ImportType::Mcp, name, &target, "live");
            report.record(ImportType::Mcp, name, ApplyOutcome::Created, None);
        } else {
            // Sandboxed: record the conversion without touching global config.
            report.record(
                ImportType::Mcp,
                name,
                ApplyOutcome::Created,
                Some("converted (not written — sandboxed)".to_string()),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_conversion_renames_and_splits_secrets() {
        let server = serde_json::json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "server"],
            "env": {"API_TOKEN": "sk-1", "REGION": "us", "PATH": "/x"}
        });
        let c = server_to_extension("srv", &server).unwrap();
        match c.config {
            ExtensionConfig::Stdio {
                cmd,
                args,
                env_keys,
                envs,
                ..
            } => {
                assert_eq!(cmd, "npx");
                assert_eq!(args, vec!["-y", "server"]);
                assert_eq!(env_keys, vec!["API_TOKEN"]); // secret → env_keys
                let e = envs.get_env();
                assert_eq!(e.get("REGION"), Some(&"us".to_string())); // plaintext
                assert!(!e.contains_key("PATH")); // host key dropped
                assert!(!e.contains_key("API_TOKEN")); // secret not inlined
            }
            _ => panic!("expected Stdio"),
        }
        assert_eq!(
            c.secrets,
            vec![("API_TOKEN".to_string(), "sk-1".to_string())]
        );
    }

    #[test]
    fn http_maps_to_streamable_and_sse_is_skipped() {
        let http = serde_json::json!({"type":"http","url":"https://ex.com/mcp"});
        match server_to_extension("api", &http).unwrap().config {
            ExtensionConfig::StreamableHttp { uri, .. } => assert_eq!(uri, "https://ex.com/mcp"),
            _ => panic!("expected StreamableHttp"),
        }
        // bare url (no type) → StreamableHttp too
        let bare = serde_json::json!({"url":"https://ex.com/mcp"});
        assert!(matches!(
            server_to_extension("api", &bare).unwrap().config,
            ExtensionConfig::StreamableHttp { .. }
        ));
        // sse → None (skip)
        let sse = serde_json::json!({"type":"sse","url":"https://ex.com/sse"});
        assert!(server_to_extension("legacy", &sse).is_none());
    }

    #[test]
    fn http_preserves_auth_headers() {
        // An authenticated remote server must import WITH its headers, not as a broken unauthenticated one.
        let http = serde_json::json!({
            "type": "http",
            "url": "https://ex.com/mcp",
            "headers": {"Authorization": "Bearer sk-9", "X-Api-Version": "2"}
        });
        match server_to_extension("api", &http).unwrap().config {
            ExtensionConfig::StreamableHttp { headers, .. } => {
                assert_eq!(
                    headers.get("Authorization"),
                    Some(&"Bearer sk-9".to_string())
                );
                assert_eq!(headers.get("X-Api-Version"), Some(&"2".to_string()));
            }
            _ => panic!("expected StreamableHttp"),
        }
    }
}
