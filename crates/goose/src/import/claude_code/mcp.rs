//! Plan MCP-server imports: Claude Code `mcpServers` → goose `extensions` (CONVERT), with an SSE carve-out.
//!
//! A stdio server (`command`/`args`/`env`) maps to `ExtensionConfig::Stdio`; an http server (`type:"http"`
//! or a bare `url`) maps to `ExtensionConfig::StreamableHttp`. A genuine `type:"sse"` server is SKIP+REPORT
//! because goose no longer supports the SSE transport — auto-mapping it to StreamableHttp would produce a
//! broken extension. Sources: the global `mcpServers` block and (in a later phase) per-project scopes.

use super::{Action, ActionClass, ImportOptions, ImportPlan, ImportType};
use anyhow::Result;
use serde_json::Value;
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
