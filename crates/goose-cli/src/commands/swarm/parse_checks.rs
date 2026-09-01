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

/// What the Rust compile gate could establish about the OWNED `.rs` files (S14-3: a tri-state,
/// because `Option` collapsed "cargo never ran" into "compiles" and the merge check labelled every
/// `.rs` module checked with the toolchain absent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RustCheck {
    /// cargo reached a verdict: the first error located in an owned file, or None — they compile
    /// (also the vacuous verdict when no `.rs` file is owned).
    Ran(Option<(String, String)>),
    /// cargo produced NO verdict about the owned files — no manifest, no toolchain, the wall, or
    /// the build failing outside them (another file/crate broke first, so these may never have
    /// been compiled). Said with the reason; each caller decides what "unproven" means to it.
    DidNotRun(String),
}

/// Deterministic Rust compile gate: `cargo check --all-targets` on the crate. `--all-targets` is
/// required so owned `tests/*.rs` are compiled too. Scoped to OWNED files so a not-yet-written
/// sibling module that breaks the crate never rejects THIS worker (the DONE gate acts only on
/// `Ran(Some(..))`); the merge check turns `DidNotRun` into an UNCHECKED file. The 120 s wall bounds
/// a compiler run, never a model.
pub(super) async fn rust_compile_error(cwd: &Path, owned: &[String]) -> RustCheck {
    if !owned.iter().any(|f| f.ends_with(".rs")) {
        return RustCheck::Ran(None);
    }
    if !cwd.join("Cargo.toml").is_file() {
        return RustCheck::DidNotRun(format!("no Cargo.toml in {}", cwd.display()));
    }
    let run = tokio::time::timeout(
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
    .await;
    let out = match run {
        Err(_) => return RustCheck::DidNotRun("cargo check exceeded its 120 s wall".to_string()),
        Ok(Err(e)) => return RustCheck::DidNotRun(format!("cargo could not start: {e}")),
        Ok(Ok(out)) => out,
    };
    if out.status.success() {
        return RustCheck::Ran(None);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if let Some((loc, msg)) = line.split_once(": error") {
            let path_tok = loc.split(':').next().unwrap_or("");
            if owned
                .iter()
                .any(|o| o.ends_with(".rs") && path_tok.ends_with(o.as_str()))
            {
                return RustCheck::Ran(Some((
                    path_tok.to_string(),
                    format!("error{}", msg.trim()),
                )));
            }
        }
    }
    let first = stderr
        .lines()
        .find(|l| l.contains("error"))
        .unwrap_or("(cargo printed no error line)")
        .trim();
    RustCheck::DidNotRun(format!(
        "cargo check failed outside the owned files: {first}"
    ))
}
