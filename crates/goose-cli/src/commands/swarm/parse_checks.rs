//! Per-file syntax and compile checks the deliverable gates and THE SPLIT's dossier share — moved
//! verbatim from swarm.rs under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases), paying for the merger's dispatch/completion wiring (2c S4).

use std::path::Path;

/// Language-aware per-file syntax check, dispatched on extension. `.py` -> the Python ast.parse check
/// verbatim (byte-identical); other languages have no cheap parse-only per-file check (tsc/rustc/etc. are
/// project-level), so they skip cleanly (None) and rely on the language's own build/test step.
pub(super) async fn syntax_error(path: &Path) -> Option<String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => py_syntax_error(path).await,
        _ => None,
    }
}

/// Syntax-check a Python file without polluting `__pycache__` (ast.parse, not py_compile). Returns the
/// last error line on a SyntaxError, `None` if it parses.
pub(super) async fn py_syntax_error(path: &Path) -> Option<String> {
    let out = tokio::process::Command::new("python3")
        .arg("-c")
        .arg("import ast,sys; ast.parse(open(sys.argv[1]).read())")
        .arg(path)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        None
    } else {
        Some(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("syntax error")
                .trim()
                .to_string(),
        )
    }
}

/// Deterministic Rust compile gate for the DONE check: `cargo check --all-targets` on the crate; returns
/// (owned_file, error) if an OWNED .rs file fails to compile, else None. `--all-targets` is required so
/// owned `tests/*.rs` are compiled too. Scoped to OWNED files so a not-yet-written sibling module that
/// breaks the crate never rejects THIS worker. Timeout / missing toolchain -> None (degrade gracefully).
pub(super) async fn rust_compile_error(cwd: &Path, owned: &[String]) -> Option<(String, String)> {
    if !owned.iter().any(|f| f.ends_with(".rs")) || !cwd.join("Cargo.toml").is_file() {
        return None;
    }
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        tokio::process::Command::new("cargo")
            .args([
                "check",
                "--all-targets",
                "--quiet",
                "--message-format=short",
            ])
            .current_dir(cwd)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if out.status.success() {
        return None;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if let Some((loc, msg)) = line.split_once(": error") {
            let path_tok = loc.split(':').next().unwrap_or("");
            if owned
                .iter()
                .any(|o| o.ends_with(".rs") && path_tok.ends_with(o.as_str()))
            {
                return Some((path_tok.to_string(), format!("error{}", msg.trim())));
            }
        }
    }
    None
}
