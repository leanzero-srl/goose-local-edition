//! Parser wiring from the checkpoint's wire format, independent of its repository name.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn configured_template(value: &Value) -> Option<&str> {
    if let Some(template) = value.as_str() {
        return Some(template);
    }
    // Transformers chooses tool_use when tools are supplied, then default.
    for name in ["tool_use", "default"] {
        if let Some(template) = value.get(name).and_then(Value::as_str) {
            return Some(template);
        }
        if let Some(template) = value.as_array().and_then(|templates| {
            templates.iter().find_map(|entry| {
                (entry.get("name").and_then(Value::as_str) == Some(name))
                    .then(|| entry.get("template").and_then(Value::as_str))
                    .flatten()
            })
        }) {
            return Some(template);
        }
    }
    None
}

fn embedded_template(path: &Path) -> Result<Option<String>> {
    let Some(text) = read_optional(path)? else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid template metadata {}", path.display()))?;
    Ok(value
        .get("chat_template")
        .and_then(configured_template)
        .map(str::to_owned))
}

/// The chat template the engine renders a TOOL-bearing request with (every goose agent turn
/// carries tools): a standalone `additional_chat_templates/tool_use.jinja`, then
/// `chat_template.jinja`, then the one embedded in `tokenizer_config.json` / `chat_template.json`
/// (a named-template list or dict prefers `tool_use`, as Transformers does when tools are
/// supplied). `None` when the directory carries no template at all.
pub(crate) fn tool_use_chat_template(dir: &Path) -> Result<Option<String>> {
    if let Some(template) = read_optional(&dir.join("additional_chat_templates/tool_use.jinja"))? {
        return Ok(Some(template));
    }
    if let Some(template) = read_optional(&dir.join("chat_template.jinja"))? {
        return Ok(Some(template));
    }
    if let Some(template) = embedded_template(&dir.join("tokenizer_config.json"))? {
        return Ok(Some(template));
    }
    embedded_template(&dir.join("chat_template.json"))
}

pub(crate) fn append_checkpoint_parser_flags(dir: &Path, argv: &mut Vec<String>) -> Result<()> {
    let Some(config) = read_optional(&dir.join("config.json"))? else {
        return Ok(());
    };
    let config: Value = serde_json::from_str(&config).context("invalid model config.json")?;
    // These architectures share the parameterized XML contract. Other families retain
    // Rapid-MLX's own specialized parser selection instead of being guessed from a name.
    if !matches!(
        config.get("model_type").and_then(Value::as_str),
        Some("qwen3_5" | "qwen3_5_moe")
    ) {
        return Ok(());
    }
    let Some(template) = tool_use_chat_template(dir)? else {
        return Ok(());
    };
    let xml_contract = [
        "tool_calls",
        "arguments",
        "<tool_call>",
        "</tool_call>",
        "<function=",
        "</function>",
        "<parameter=",
        "</parameter>",
    ]
    .iter()
    .all(|marker| template.contains(marker));
    if !xml_contract {
        return Ok(());
    }
    // Not `qwen3_xml`: in Rapid-MLX that name registers the JSON-body parser
    // (`<tool_call>{"name":…}`), which this template never emits. Residue after a parameter
    // closes (Q-85) is refused at decode time by the engine's skeleton guard (lz.7), not here.
    argv.extend([
        "--enable-auto-tool-choice".into(),
        "--tool-call-parser".into(),
        "qwen3_coder_xml".into(),
    ]);
    // The generation prompt can supply the opening tag itself; deepseek_r1 also
    // recognizes reasoning that starts implicitly and ends with </think>.
    if ["enable_thinking", "<think>", "</think>"]
        .iter()
        .all(|marker| template.contains(marker))
    {
        argv.extend(["--reasoning-parser".into(), "deepseek_r1".into()]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{build_serve_command, EngineSettings};

    const XML: &str = "{% for call in message.tool_calls %}{{ '<tool_call><function=' + call.name + '><parameter=' + call.arguments + '></parameter></function></tool_call>' }}{% endfor %}{% if enable_thinking %}{{ '<think>' }}{% else %}{{ '<think></think>' }}{% endif %}";

    #[test]
    fn renamed_checkpoint_wires_xml_tools_and_implicit_reasoning_into_app_argv() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("publisher/renamed-checkpoint");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), r#"{"model_type":"qwen3_5"}"#).unwrap();
        std::fs::write(dir.join("chat_template.jinja"), XML).unwrap();
        let settings = EngineSettings {
            models_dir: root.path().to_string_lossy().into(),
            ..Default::default()
        };
        let argv = build_serve_command(&settings, "publisher/renamed-checkpoint").unwrap();
        assert!(argv.windows(3).any(|part| part
            == [
                "--enable-auto-tool-choice",
                "--tool-call-parser",
                "qwen3_coder_xml"
            ]));
        assert!(argv
            .windows(2)
            .any(|part| part == ["--reasoning-parser", "deepseek_r1"]));
    }

    #[test]
    fn tokenizer_templates_require_architecture_and_full_xml_contract() {
        let dir = tempfile::tempdir().unwrap();
        for template in [
            serde_json::json!(XML),
            serde_json::json!({"tool_use": XML}),
            serde_json::json!([{"name":"tool_use","template": XML}]),
        ] {
            std::fs::write(
                dir.path().join("config.json"),
                r#"{"model_type":"qwen3_5_moe"}"#,
            )
            .unwrap();
            std::fs::write(
                dir.path().join("tokenizer_config.json"),
                serde_json::json!({"chat_template":template}).to_string(),
            )
            .unwrap();
            let mut argv = vec![];
            append_checkpoint_parser_flags(dir.path(), &mut argv).unwrap();
            assert!(argv.contains(&"qwen3_coder_xml".to_string()));
        }
        for (architecture, template) in [
            ("other", XML),
            (
                "qwen3_5",
                "{{ '<tool_call>' + tools|tojson + '</tool_call>' }}",
            ),
        ] {
            std::fs::write(
                dir.path().join("config.json"),
                serde_json::json!({"model_type":architecture}).to_string(),
            )
            .unwrap();
            std::fs::write(dir.path().join("chat_template.jinja"), template).unwrap();
            let mut argv = vec![];
            append_checkpoint_parser_flags(dir.path(), &mut argv).unwrap();
            assert!(argv.is_empty());
        }
    }

    #[test]
    fn named_tool_template_takes_precedence_over_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"model_type":"qwen3_5"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("chat_template.jinja"), XML).unwrap();
        std::fs::create_dir(dir.path().join("additional_chat_templates")).unwrap();
        let named = dir.path().join("additional_chat_templates/tool_use.jinja");
        std::fs::write(&named, "{{ tools | tojson }}").unwrap();
        let mut argv = vec![];
        append_checkpoint_parser_flags(dir.path(), &mut argv).unwrap();
        assert!(argv.is_empty());
        std::fs::write(&named, XML).unwrap();
        std::fs::write(dir.path().join("chat_template.jinja"), "{{ messages }}").unwrap();
        append_checkpoint_parser_flags(dir.path(), &mut argv).unwrap();
        assert!(argv.contains(&"qwen3_coder_xml".to_string()));
    }

    #[test]
    fn json_sidecar_is_used_only_without_an_active_tokenizer_template() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"model_type":"qwen3_5"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("chat_template.json"),
            serde_json::json!({"chat_template": XML}).to_string(),
        )
        .unwrap();
        let mut argv = vec![];
        append_checkpoint_parser_flags(dir.path(), &mut argv).unwrap();
        assert!(argv.contains(&"qwen3_coder_xml".to_string()));
        std::fs::write(
            dir.path().join("tokenizer_config.json"),
            r#"{"chat_template":"{{ messages }}"}"#,
        )
        .unwrap();
        argv.clear();
        append_checkpoint_parser_flags(dir.path(), &mut argv).unwrap();
        assert!(argv.is_empty());
    }
}
