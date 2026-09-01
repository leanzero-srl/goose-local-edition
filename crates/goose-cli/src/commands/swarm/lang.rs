//! Target-language detection for a swarm run — moved verbatim from swarm.rs under the
//! incremental-split law (development_gates::swarm_rs_line_count_only_decreases), paying for
//! the shard-completion verification seam (SPLIT v2 mechanism 2, `shard_verify.rs`), the fix
//! wave's `setup_failed` field and the split's `free_hosts` derivation. `TargetLang` itself
//! stays in swarm.rs (every sibling names it as `super::TargetLang`).

use super::TargetLang;

/// True if `s` mentions a `.<ext>` file at a word boundary (the char after the ext is not alphanumeric), so
/// ".js" matches "cli.js" but NOT "schema.json", and ".ts" matches "a.ts" but not "a.tsx". `s` is ASCII-lower.
fn mentions_ext(s: &str, ext: &str) -> bool {
    let needle = format!(".{ext}");
    let bytes = s.as_bytes();
    s.match_indices(&needle).any(|(i, _)| {
        let after = i + needle.len();
        after >= bytes.len() || !bytes[after].is_ascii_alphanumeric()
    })
}

/// Detect the target language. Existing files (an amendment) are the strongest signal; otherwise an EXPLICIT
/// language name in the spec wins, then weaker word-boundary file-extension cues; default Python otherwise.
pub(super) fn detect_language(spec: &str, existing_files: &[String]) -> TargetLang {
    if !existing_files.is_empty() {
        let ext_of = |p: &str| {
            p.rsplit('.')
                .next()
                .filter(|e| *e != p)
                .unwrap_or("")
                .to_lowercase()
        };
        let n = |e: &str| existing_files.iter().filter(|p| ext_of(p) == e).count();
        let (py, ts, rs, go) = (n("py"), n("ts") + n("tsx") + n("js"), n("rs"), n("go"));
        let top = [py, ts, rs, go].into_iter().max().unwrap_or(0);
        if top > 0 {
            if ts == top {
                return TargetLang::TypeScript;
            }
            if rs == top {
                return TargetLang::Rust;
            }
            if go == top {
                return TargetLang::Go;
            }
            return TargetLang::Python;
        }
    }
    let s = spec.to_lowercase();
    // EXPLICIT language declarations win over incidental file-extension mentions: a Python app whose spec
    // says "validate SCHEMA.json" must NOT be read as TypeScript just because ".json" contains ".js" (the
    // exact APP8 failure — a LANG=Python JSON validator was built in TypeScript).
    if s.contains("python") || s.contains("pytest") {
        return TargetLang::Python;
    }
    if s.contains("typescript")
        || s.contains("javascript")
        || s.contains("node.js")
        || s.contains("nodejs")
    {
        return TargetLang::TypeScript;
    }
    if s.contains("rust") || s.contains("cargo") {
        return TargetLang::Rust;
    }
    if s.contains("golang") {
        return TargetLang::Go;
    }
    // Weaker file-extension / tool cues — matched at a word BOUNDARY so ".js" does not match ".json" and
    // ".ts" does not match ".tsx".
    if mentions_ext(&s, "ts")
        || mentions_ext(&s, "tsx")
        || mentions_ext(&s, "js")
        || s.contains("vitest")
        || s.contains(" jest")
        || s.contains("npm ")
    {
        return TargetLang::TypeScript;
    }
    if mentions_ext(&s, "rs") {
        return TargetLang::Rust;
    }
    if mentions_ext(&s, "go") || s.contains(" go ") {
        return TargetLang::Go;
    }
    if mentions_ext(&s, "py") {
        return TargetLang::Python;
    }
    // A named-but-unprofiled language: still honor it (generic non-Python guidance), never force Python.
    if s.contains("ruby")
        || s.contains("java")
        || s.contains("c#")
        || s.contains("c++")
        || s.contains("php")
        || s.contains("swift")
        || s.contains("kotlin")
        || s.contains("scala")
        || s.contains("elixir")
        || s.contains("haskell")
    {
        return TargetLang::Other;
    }
    TargetLang::Python
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_language_defaults_python_and_honors_cues() {
        // No cue -> Python (the validated baseline default).
        assert_eq!(
            detect_language("a CLI markdown to HTML renderer", &[]),
            TargetLang::Python
        );
        // Explicit spec cues win.
        assert_eq!(
            detect_language("build a TypeScript CLI todo app", &[]),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language("a Rust CLI using cargo", &[]),
            TargetLang::Rust
        );
        assert_eq!(
            detect_language("a golang command line tool", &[]),
            TargetLang::Go
        );
        // A named-but-unprofiled language is honored (generic), never forced to Python.
        assert_eq!(detect_language("a Ruby CLI gem", &[]), TargetLang::Other);
        // APP8 regression: an explicit LANG=Python wins over ".json" (which contains ".js") — previously
        // mis-detected as TypeScript and the JSON validator was built in the wrong language.
        assert_eq!(
            detect_language(
                "LANG=Python — a CLI JSON-schema validator: validate SCHEMA.json DATA.json",
                &[]
            ),
            TargetLang::Python
        );
        // ".json" with no explicit language is NOT TypeScript (word-boundary ext match) -> default Python.
        assert_eq!(
            detect_language("a CLI that reads config.json and prints a report", &[]),
            TargetLang::Python
        );
        // a real .js file mention IS TypeScript; node.js name IS TypeScript.
        assert_eq!(
            detect_language("a CLI whose entry is bin/cli.js", &[]),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language("a node.js CLI that validates data.json", &[]),
            TargetLang::TypeScript
        );
        // Amendment: the existing files' extensions are the strongest signal, overriding a bare spec.
        assert_eq!(
            detect_language("add a --json flag", &["index.ts".into(), "util.ts".into()]),
            TargetLang::TypeScript
        );
        assert_eq!(
            detect_language(
                "add a --json flag",
                &["cli.py".into(), "detector.py".into()]
            ),
            TargetLang::Python
        );
    }

    #[test]
    fn clarify_answer_flips_detected_language_forcing_replan() {
        // The ASK-answer fix folds the user's clarifications into the spec BEFORE language detection, so a
        // runtime choice in the answer is honored. This is the exact miss it fixes: a vague spec defaults to
        // Python, the user picks Rust, and previously the answer went only into research findings (which
        // detect_language never reads) so the run silently stayed Python. Appending the Q&A block (which
        // embeds "A: Rust ...") must flip the detected language — that flip is what forces the re-plan.
        let spec =
            "Build a small command-line developer utility that saves time in day-to-day work. \
                    Pick something genuinely useful and make it good.";
        assert_eq!(detect_language(spec, &[]), TargetLang::Python);
        // The real Q&A block shape from ask_clarifying_questions (question + verbatim answer).
        let qa = "\n\n[User clarifications incorporated into the spec]\n\n\
                  USER CLARIFICATIONS (authoritative — they resolve ambiguity in the spec above; honor them):\n\
                  Q: What runtime should it be?\nA: Rust (faster, single binary)\n";
        let amended = format!("{spec}{qa}");
        assert_eq!(detect_language(&amended, &[]), TargetLang::Rust);
        // The fix's decision: a differing language BEFORE vs AFTER folding the answer is what triggers the
        // forced re-plan (a reused Python-shaped plan cannot honor a switch to Rust).
        assert_ne!(detect_language(&amended, &[]), detect_language(spec, &[]));
    }
}
