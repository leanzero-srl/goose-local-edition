//! Phase 0 of "goose writes its own memories": the deterministic SKILL-vs-MEMORY classifier and the
//! secret/PII pre-write scrubber, as pure, unit-testable functions with NO runtime side effects.
//!
//! This module is intentionally *dead until wired*. Nothing in it is invoked from a production path yet, so
//! landing it is byte-identical to today's behaviour (the non-negotiable invariant). Later phases (chat
//! `auto_remember`, the swarm REFLECT-memory step) call these functions BEHIND the gates in [`memory_gate`],
//! which default OFF via env `GOOSE_MEMORY_*` > config field > `false`.
//!
//! Design: `SOLUTIONS.md` "READY (design): goose writes its own memories + skill-vs-memory differentiation".
//! - §1 THE DISTINCTION (priority-ordered Rule A/B, the three outcomes, the chat-no-stack constraint) →
//!   [`classify_knowledge`].
//! - §2c-0 the SECRET/PII scrubber (deterministic, refuse-on-hit, first gate on both surfaces) →
//!   [`scrub_secret`].
//! - §4b gating (env > config > default false, mirroring `swarm_gate_cfg`) → [`memory_gate`] /
//!   [`resolve_memory_gate`].

/// Ownership marker written on every goose-authored entry (analogue of the importer's `imported:claude-code`
/// and persona's `PERSONA_PROVENANCE`). Only entries carrying this marker may ever be touched by
/// dedup/reconcile/eviction; untagged (user-typed) and imported entries are never rewritten or evicted.
pub const LEARNED_PROVENANCE: &str = "learned:goose";

/// The importer's ownership marker — never rewritten/evicted by the memory-write path.
pub const IMPORTED_PROVENANCE: &str = "imported:claude-code";

/// Env gate for chat auto-write (§4b). Env value wins over the config default.
pub const ENV_MEMORY_AUTOWRITE: &str = "GOOSE_MEMORY_AUTOWRITE";
/// Env gate for swarm memory READ (§4b).
pub const ENV_SWARM_MEMORY_READ: &str = "GOOSE_SWARM_MEMORY_READ";
/// Env gate for swarm memory WRITE (§4b) — separate from read so read ships/A/Bs first.
pub const ENV_SWARM_MEMORY_WRITE: &str = "GOOSE_SWARM_MEMORY_WRITE";

/// The surface a piece of knowledge is being classified on. The chat surface has NO stack context
/// (`detect_stack_key` lives only in the swarm path), so it is structurally barred from emitting a global
/// SKILL (§1 "What the CHAT classifier does WITHOUT stack context").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The `goose` chat/CLI session — no stack awareness.
    Chat,
    /// The local-fleet swarm build — has `detect_stack_key`, may route to a persona/stack skill.
    Swarm,
}

/// The MEMORY sub-bucket (§2d `<type>`). Mirrors the importer's `{feedback, project, reference}` and extends
/// it with `user` (preferences), which the importer does not mint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCategory {
    /// A truth about the *user* (a stated preference / taste / constraint on output). Always-core surface.
    User,
    /// A correction or gotcha ("no, actually…"). The low-blast-radius default when a bucket is unclear.
    Feedback,
    /// A durable fact about *this repo* (layout, file locations).
    Project,
    /// A durable fact about the *environment/config* (port, path, host, alias, version, service).
    Reference,
}

impl MemoryCategory {
    /// The `<type>` token as it is written into the tag line.
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryCategory::User => "user",
            MemoryCategory::Feedback => "feedback",
            MemoryCategory::Project => "project",
            MemoryCategory::Reference => "reference",
        }
    }
}

/// The four possible homes for a piece of knowledge (§1 decision table, with the first-class
/// PROJECT PROCEDURE third outcome that resolves the tiebreaker conflict).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// A declarative fact/preference → a memory entry in the given bucket.
    Memory(MemoryCategory),
    /// A *global* procedure (user/task-independent method) → a persona/stack SKILL. **Swarm surface only.**
    Skill,
    /// A procedure tied to *this repo/env* → a project-scoped local skill (`{cwd}/.goose/skills/…`).
    ProjectProcedure,
    /// Nothing worth persisting.
    Drop,
}

/// Classify a piece of knowledge into MEMORY / SKILL / PROJECT PROCEDURE / DROP using the priority-ordered
/// Rule A (procedure test dominates) → Rule B (procedural scope), plus the chat-no-stack constraint (§1).
///
/// This is the deterministic first-pass heuristic (the design's "deterministic-first", mirroring
/// `detect_stack_key`/`relevant_pitfalls`): the model may only ever *phrase and classify* a fact an
/// observable event already grounded; this engine function pins the bucket and refuses to let the chat
/// surface emit a global skill it cannot scope.
///
/// Hard rule enforced here: **the chat classifier may only ever produce MEMORY, PROJECT PROCEDURE, or DROP —
/// never a global SKILL.** Procedural-but-scope-unknown on chat → `Memory(Feedback)` (the low-blast default),
/// because wrongly promoting a stack convention to a *global* skill is the high-blast-radius error we refuse.
pub fn classify_knowledge(text: &str, surface: Surface) -> Classification {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Classification::Drop;
    }
    let lower = trimmed.to_lowercase();

    // Rule A — procedure test (DOMINATES). Does this encode a positively-stated ordered method / command to
    // execute a task? A prohibition/constraint or a plain predicate is NOT a procedure → MEMORY.
    if is_positive_procedure(&lower) {
        // Rule B — procedural scope (only reached if procedural).
        if is_repo_scoped(&lower) {
            // Tied to one repo/env → a first-class PROJECT PROCEDURE (a local skill). Allowed on both
            // surfaces (it does not require stack awareness — it is explicitly repo-local).
            Classification::ProjectProcedure
        } else {
            match surface {
                // The swarm surface HAS detect_stack_key → it may route a stack convention to a global skill.
                Surface::Swarm => Classification::Skill,
                // The chat surface cannot scope a convention to a stack → refuse the global skill, store the
                // low-blast-radius default instead.
                Surface::Chat => Classification::Memory(MemoryCategory::Feedback),
            }
        }
    } else {
        Classification::Memory(memory_bucket(&lower))
    }
}

/// True when `lower` (already lowercased) reads as a positively-stated procedure — an ordered method or a
/// command to RUN — rather than a prohibition/constraint or a plain predicate.
fn is_positive_procedure(lower: &str) -> bool {
    // A leading prohibition/correction makes the MAIN clause a constraint, not a procedure ("don't restart
    // the fleet", "NEVER use a left accent line"). "not pnpm build" mid-sentence is a *secondary* contrast
    // and must NOT flip a positive build instruction, so only a LEADING prohibition disqualifies.
    if starts_with_any(
        lower,
        &[
            "no ", "no,", "not ", "don't", "do not", "never", "avoid", "stop ", "n't ",
        ],
    ) {
        return false;
    }

    // Command verbs that denote an action to execute (deliberately excludes the generic "use", which appears
    // in taste constraints like "don't use a left rail").
    let has_command_verb = contains_any(
        lower,
        &[
            "run ",
            "build",
            "install",
            "execute",
            "compile",
            "recompile",
            "rebuild",
            "restart",
            "deploy",
            "migrate",
            "seed",
            "clone",
            "checkout",
            "rebase",
            "launch",
            "generate",
        ],
    );
    // Concrete tool/shell tokens.
    let has_tool_token = contains_any(
        lower,
        &[
            "cargo ", "npm ", "pnpm", "npx ", "yarn ", "git ", " just ", "make ", "docker",
            "kubectl", "brew ", "gradle", "mvn ", "./", "hermit",
        ],
    );
    // Ordered/sequence markers.
    let has_sequence = contains_any(
        lower,
        &[
            "first ",
            "then ",
            "step 1",
            "before commit",
            "before running",
            "after running",
        ],
    );
    // Explicit how-to framing.
    let has_howto = contains_any(lower, &["how to ", "how do i", "how do you", "how to do"]);
    // A code-construct requirement (a stack convention like "@MainActor is required on a SwiftUI store"):
    // an `@`-annotation combined with a positive requirement modal.
    let has_code_requirement = lower.contains('@')
        && !lower.contains('.') // an `@` in an email/host has a dot; an annotation does not
        && contains_any(lower, &["required", "must", "needs", "annotate", "annotation"]);

    has_command_verb || has_tool_token || has_sequence || has_howto || has_code_requirement
}

/// True when the knowledge is explicitly tied to *this* repo/project/codebase (Rule B "YES" branch).
fn is_repo_scoped(lower: &str) -> bool {
    contains_any(
        lower,
        &[
            "this repo",
            "this project",
            "in this repo",
            "this codebase",
            "our repo",
            ".goose/",
        ],
    )
}

/// Second-level tiebreak inside MEMORY (§1): which fact bucket. Order matters — user taste is checked before
/// the correction/gotcha default so "NEVER use a left accent line" lands as `user`, not `feedback`.
fn memory_bucket(lower: &str) -> MemoryCategory {
    // 1. USER — a stated preference / taste / output constraint.
    if contains_any(
        lower,
        &[
            "prefer",
            "i like",
            "i want",
            "i hate",
            "i love",
            "we use",
            "we always",
            "rounded",
            "corners",
            "accent line",
            "left rail",
            "colour",
            "color",
            "font",
            " ui ",
            "styling",
            "square everything",
        ],
    ) {
        return MemoryCategory::User;
    }
    // 2. REFERENCE — an environment / config fact.
    if contains_any(
        lower,
        &[
            "port",
            "listens on",
            "host",
            "alias",
            "ssh",
            "http://",
            "https://",
            "url",
            "endpoint",
            "version",
            "@",
            "env var",
            "service",
            "hostname",
        ],
    ) {
        return MemoryCategory::Reference;
    }
    // 3. PROJECT — a repo-layout fact.
    if contains_any(
        lower,
        &[
            "live in",
            "lives in",
            "located in",
            "located at",
            "directory",
            "folder",
            "is at ",
            "are in ",
            "tests are",
        ],
    ) {
        return MemoryCategory::Project;
    }
    // 4. FEEDBACK — a correction/gotcha, and the low-blast-radius default when nothing else matches.
    MemoryCategory::Feedback
}

// ---------------------------------------------------------------------------------------------------------
// §2c-0 — the SECRET / PII scrubber: a hard, deterministic, non-model pre-write gate.
// Refuse-on-hit (refuse the ENTIRE candidate, never redact). Runs first, before any write, on both surfaces.
// ---------------------------------------------------------------------------------------------------------

/// Scan a candidate memory string for secrets / credentials / PII. Returns `Some(detector_name)` if the write
/// must be REFUSED (the entire candidate is rejected — never redacted, per §2c-0), else `None` (may store).
///
/// Deterministic detectors only (regex-free, std-only): token prefixes, credential headers, connection
/// strings, private-key material & key-file paths, a sensitive-env-var heuristic, email/phone PII, and a
/// Shannon-entropy check on long base64/hex runs. Unit-tested on the design's own workhorse example: the bare
/// host/alias `workhorse@192.168.8.220` PASSES (an IP is not an email), the key path `~/.ssh/id_ed25519_*`
/// is REFUSED.
pub fn scrub_secret(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();

    // Private-key material & key-file paths (the flagship refusal — check first).
    if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
        return Some("private_key_material");
    }
    for raw in text.split_whitespace() {
        let tok = raw.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == '`' || c == ',' || c == '(' || c == ')'
        });
        if tok.contains("/.ssh/id_") {
            return Some("ssh_key_path");
        }
        let tl = tok.to_lowercase();
        if tl.ends_with(".pem")
            || tl.ends_with(".key")
            || tl.ends_with(".p12")
            || tl.ends_with(".keychain")
        {
            return Some("key_file_path");
        }
    }

    // Token / key prefixes.
    for raw in text.split_whitespace() {
        let tok = raw.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        });
        if tok.len() > 8 && tok.starts_with("sk-") {
            return Some("openai_key");
        }
        if tok.starts_with("ghp_")
            || tok.starts_with("gho_")
            || tok.starts_with("ghu_")
            || tok.starts_with("ghs_")
            || tok.starts_with("ghr_")
        {
            return Some("github_token");
        }
        if tok.starts_with("xoxb-")
            || tok.starts_with("xoxa-")
            || tok.starts_with("xoxp-")
            || tok.starts_with("xoxr-")
            || tok.starts_with("xoxs-")
        {
            return Some("slack_token");
        }
        if tok.len() == 20
            && tok.starts_with("AKIA")
            && tok[4..]
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Some("aws_access_key");
        }
        if tok.len() > 20 && tok.starts_with("AIza") {
            return Some("google_api_key");
        }
        if tok.starts_with("eyJ") && tok.matches('.').count() == 2 {
            return Some("jwt");
        }
    }

    // Credential headers / inline creds.
    for needle in [
        "authorization:",
        "bearer ",
        "basic ",
        "x-api-key",
        "password=",
        "passwd=",
    ] {
        if lower.contains(needle) {
            return Some("credential_header");
        }
    }

    // Connection strings scheme://user:pass@host.
    for scheme in [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
    ] {
        if let Some(idx) = lower.find(scheme) {
            let rest = &lower[idx + scheme.len()..];
            let authority = rest.split('/').next().unwrap_or("");
            if authority.contains('@') && authority.contains(':') {
                return Some("connection_string");
            }
        }
    }

    // Sensitive-env-var heuristic: KEY=VALUE where KEY is credential-shaped and VALUE is non-trivial.
    for raw in text.split_whitespace() {
        if let Some(eq) = raw.find('=') {
            let key = raw[..eq].to_uppercase();
            let val = raw[eq + 1..].trim_matches(|c| c == '"' || c == '\'' || c == '`');
            let sensitive = [
                "TOKEN",
                "SECRET",
                "_KEY",
                "API_KEY",
                "PASSWORD",
                "PASSWD",
                "PRIVATE",
                "CREDENTIAL",
            ]
            .iter()
            .any(|m| key.contains(m));
            if sensitive && val.len() >= 6 {
                return Some("sensitive_env_var");
            }
        }
    }

    // Email PII — require an alphabetic TLD so `user@192.168.8.220` (a host, not an email) PASSES.
    for raw in
        text.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '<' || c == '>')
    {
        let tok = raw.trim_matches(|c| {
            c == '"' || c == '\'' || c == '`' || c == '(' || c == ')' || c == '.'
        });
        if is_email(tok) {
            return Some("email_pii");
        }
    }

    // Phone PII — conservative: a leading '+' with >= 10 digits (E.164), so IPs/versions do not trip it.
    if looks_like_e164(&lower) {
        return Some("phone_pii");
    }

    // High-entropy base64/hex run (>= 20 chars, Shannon entropy > 4.0 bits/char).
    for raw in text.split_whitespace() {
        let tok = raw.trim_matches(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '_' || c == '-')
        });
        if tok.len() >= 20 && is_b64_or_hex(tok) && shannon_entropy(tok) > 4.0 {
            return Some("high_entropy_secret");
        }
    }

    None
}

fn is_email(tok: &str) -> bool {
    let at = match tok.find('@') {
        Some(i) if i > 0 => i,
        _ => return false,
    };
    let (local, domain) = (&tok[..at], &tok[at + 1..]);
    if local.is_empty() || domain.is_empty() || !local.chars().any(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    // Domain must have a dot and an alphabetic TLD of >= 2 chars (so a bare IP is not an email).
    let last = match domain.rsplit('.').next() {
        Some(l) if domain.contains('.') => l,
        _ => return false,
    };
    last.len() >= 2 && last.chars().all(|c| c.is_ascii_alphabetic())
}

fn looks_like_e164(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'+' {
            // Count consecutive digits (allowing separators) right after the '+'.
            let mut digits = 0usize;
            for &c in &bytes[i + 1..] {
                if c.is_ascii_digit() {
                    digits += 1;
                } else if c == b' ' || c == b'-' || c == b'(' || c == b')' {
                    continue;
                } else {
                    break;
                }
            }
            if digits >= 10 {
                return true;
            }
        }
    }
    false
}

fn is_b64_or_hex(tok: &str) -> bool {
    tok.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '_' || c == '-'
    })
}

fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
    }
    let len = s.chars().count() as f64;
    counts
        .values()
        .map(|&n| {
            let p = n as f64 / len;
            -p * p.log2()
        })
        .sum()
}

// ---------------------------------------------------------------------------------------------------------
// §4b — gating (env > config field > default false), mirroring `swarm_gate_cfg`.
// ---------------------------------------------------------------------------------------------------------

/// Pure precedence logic (no env I/O, so it is unit-testable without env races): an explicit env value wins
/// (truthy on `1/on/true/yes`), else the config default. Identical semantics to `swarm_gate_cfg`.
pub fn resolve_memory_gate(explicit: Option<String>, cfg_default: bool) -> bool {
    match explicit {
        Some(v) => matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        ),
        None => cfg_default,
    }
}

/// Read a memory gate: env var `name` wins over the persisted config default. Defaults OFF whenever the env
/// var is unset and the config default is `false` — the safety property that keeps every memory-write path
/// dormant until a user opts in.
pub fn memory_gate(name: &str, cfg_default: bool) -> bool {
    resolve_memory_gate(std::env::var(name).ok(), cfg_default)
}

// ---------------------------------------------------------------------------------------------------------

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn starts_with_any(hay: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| hay.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- §1 classifier: the CLEAN cases (1, 3, 5, 6) ----

    #[test]
    fn clean_cases_classify_as_designed() {
        // 1. taste-constraint → MEMORY(user)
        assert_eq!(
            classify_knowledge(
                "prefers sharp corners / square everything, no rounded UI",
                Surface::Chat
            ),
            Classification::Memory(MemoryCategory::User)
        );
        // 3. env fact → MEMORY(reference)
        assert_eq!(
            classify_knowledge(
                "the workhorse SSH alias is workhorse@192.168.8.220",
                Surface::Chat
            ),
            Classification::Memory(MemoryCategory::Reference)
        );
        // 5. location fact → MEMORY(project)
        assert_eq!(
            classify_knowledge("tests live in crates/goose/tests/", Surface::Chat),
            Classification::Memory(MemoryCategory::Project)
        );
        // 6. stack convention (code requirement) → SKILL on the swarm surface
        assert_eq!(
            classify_knowledge("@MainActor is required on a SwiftUI store", Surface::Swarm),
            Classification::Skill
        );
    }

    // ---- §1 classifier: the CONFLICTING cases 7–10 (the load-bearing Phase-0 test set) ----

    #[test]
    fn case7_project_specific_procedure_is_project_procedure_not_memory() {
        // "this repo builds with `just build-fork` under Node 24, not `pnpm build`" — procedural + repo-scoped.
        // The mid-sentence "not pnpm build" must NOT flip it to declarative.
        let c = classify_knowledge(
            "this repo builds with `just build-fork` under Node 24, not `pnpm build`",
            Surface::Chat,
        );
        assert_eq!(c, Classification::ProjectProcedure);
        // Same verdict on the swarm surface (repo-scope wins before the surface split).
        assert_eq!(
            classify_knowledge(
                "this repo builds with `just build-fork` under Node 24, not `pnpm build`",
                Surface::Swarm,
            ),
            Classification::ProjectProcedure
        );
    }

    #[test]
    fn case8_global_procedure_swarm_skill_chat_feedback() {
        // "always run `cargo fmt` before committing" — procedural, user-independent.
        // Swarm HAS stack context → global SKILL.
        assert_eq!(
            classify_knowledge("always run `cargo fmt` before committing", Surface::Swarm),
            Classification::Skill
        );
        // Chat has NO stack context → may NEVER emit a global skill → low-blast MEMORY(feedback).
        assert_eq!(
            classify_knowledge("always run `cargo fmt` before committing", Surface::Chat),
            Classification::Memory(MemoryCategory::Feedback)
        );
    }

    #[test]
    fn case9_output_constraint_is_user_memory() {
        // "NEVER use a left accent line / no rounded UI" — a constraint on output, not steps to execute.
        assert_eq!(
            classify_knowledge(
                "NEVER use a left accent line / no rounded UI",
                Surface::Chat
            ),
            Classification::Memory(MemoryCategory::User)
        );
        assert_eq!(
            classify_knowledge(
                "NEVER use a left accent line / no rounded UI",
                Surface::Swarm
            ),
            Classification::Memory(MemoryCategory::User)
        );
    }

    #[test]
    fn case10_correction_is_feedback_memory() {
        // "no, don't restart the fleet" — a correction/prohibition → MEMORY(feedback), never a procedure,
        // even though "restart" is a command verb (the leading prohibition disqualifies it).
        assert_eq!(
            classify_knowledge("no, don't restart the fleet", Surface::Chat),
            Classification::Memory(MemoryCategory::Feedback)
        );
    }

    #[test]
    fn chat_surface_never_emits_a_global_skill() {
        // The structural invariant: no input on the Chat surface may ever produce Classification::Skill.
        for input in [
            "always run `cargo fmt` before committing",
            "how to build a Forge app end-to-end",
            "@MainActor is required on a SwiftUI store",
            "run the migration then seed the database",
        ] {
            assert_ne!(
                classify_knowledge(input, Surface::Chat),
                Classification::Skill,
                "chat surface must never emit a global SKILL for {input:?}"
            );
        }
    }

    #[test]
    fn empty_input_drops() {
        assert_eq!(
            classify_knowledge("   ", Surface::Chat),
            Classification::Drop
        );
    }

    // ---- §2c-0 scrubber: the flagship workhorse example ----

    #[test]
    fn scrubber_passes_host_alias_refuses_key_path() {
        // Host/alias may store (an IP is not an email, the token is not high-entropy).
        assert_eq!(
            scrub_secret("the workhorse SSH alias is workhorse@192.168.8.220"),
            None
        );
        // The private-key PATH is refused.
        assert_eq!(
            scrub_secret("~/.ssh/id_ed25519_workhorse"),
            Some("ssh_key_path")
        );
        // A candidate carrying the key path is refused as a whole.
        assert_eq!(
            scrub_secret("connect to the workhorse via ~/.ssh/id_ed25519_workhorse"),
            Some("ssh_key_path")
        );
    }

    #[test]
    fn scrubber_refuses_tokens_and_creds() {
        assert!(scrub_secret("the api key is sk-abcdefghijklmnop").is_some());
        assert!(scrub_secret("token ghp_0123456789abcdef0123456789abcdef0123").is_some());
        assert!(scrub_secret("use xoxb-111-222-abcdef as the bot token").is_some());
        assert!(scrub_secret("AKIAIOSFODNN7EXAMPLE is the access key").is_some());
        assert!(scrub_secret("google key AIzaSyD-abcdefghijklmnopqrstuv").is_some());
        assert!(scrub_secret("header Authorization: Bearer xyz").is_some());
        assert!(scrub_secret("the db url is postgres://user:secretpw@db.host:5432/app").is_some());
        assert!(scrub_secret("set OPENAI_API_KEY=sup3rs3cr3tvalue in the env").is_some());
        assert!(scrub_secret("email me at alice@example.com").is_some());
        assert!(scrub_secret("call +14155552671 for support").is_some());
        assert!(scrub_secret("-----BEGIN OPENSSH PRIVATE KEY-----").is_some());
    }

    #[test]
    fn scrubber_passes_ordinary_memories() {
        for ok in [
            "prefers sharp corners / square everything, no rounded UI",
            "tests live in crates/goose/tests/",
            "this repo builds with just build-fork under Node 24",
            "the fleet listens on port 1234",
            "always run cargo fmt before committing",
            "no, don't restart the fleet when lms ps shows GENERATING",
        ] {
            assert_eq!(scrub_secret(ok), None, "should pass the scrubber: {ok:?}");
        }
    }

    #[test]
    fn scrubber_ip_host_is_not_a_phone_or_email() {
        // A bare IP must trip neither the phone (no leading '+') nor the email (numeric TLD) detector.
        assert_eq!(scrub_secret("the service is at 192.168.8.220"), None);
    }

    // ---- §4b gate precedence ----

    #[test]
    fn gate_defaults_off_and_env_wins() {
        // Env unset → config default governs.
        assert!(!resolve_memory_gate(None, false));
        assert!(resolve_memory_gate(None, true));
        // Explicit env value wins over the config default, both directions.
        assert!(resolve_memory_gate(Some("1".into()), false));
        assert!(resolve_memory_gate(Some("on".into()), false));
        assert!(resolve_memory_gate(Some("TRUE".into()), false));
        assert!(!resolve_memory_gate(Some("0".into()), true));
        assert!(!resolve_memory_gate(Some("off".into()), true));
    }

    #[test]
    fn provenance_marker_matches_the_importer_shape() {
        // The ownership marker is a distinct token from the importer's, so dedup/eviction can tell them apart.
        assert_eq!(LEARNED_PROVENANCE, "learned:goose");
        assert_eq!(IMPORTED_PROVENANCE, "imported:claude-code");
        assert_ne!(LEARNED_PROVENANCE, IMPORTED_PROVENANCE);
    }
}
