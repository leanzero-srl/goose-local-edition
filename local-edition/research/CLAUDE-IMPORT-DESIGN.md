All anchors verified against the tree. Writing the authoritative doc now. Below is the full markdown.

---

# Goose Local Edition — FINAL Authoritative Design & Build Plan

Repo root: `/Users/mihaiperdum/Projects/goose` (not a git-repo per the env note, but a git worktree exists — revert tag `pre-claude-code-import` confirmed present, HEAD is 1 commit past it). All paths absolute. Every line-number anchor below was re-verified in this session; where a review corrected the original design, the corrected anchor is used and marked ✓.

This doc supersedes the draft design and folds in every valid finding from the feasibility, clone-risk, completeness, and build-realism reviews. Points that were wrong in the draft are called out as **[was: …]** so the delta is auditable.

---

## 0. Executive summary & load-bearing decisions

1. **Two products, one binary/app.** An `edition` flag (`standard` | `local`) selects "goose" vs "goose local edition." It is UX/skin only and never gates model capability. **[Revised per clone-risk D11]:** but the *default* resolution is **derived, not asked** — if a local/swarm provider (LM Studio / lmstudio / swarm nodes) is configured, LE is auto-applied (opt-out available). This makes the "Local" name *true* instead of a badge divorced from locality, and kills the wizard-wall first-run screen (clone-risk D10/D12).

2. **Deliverable 1 is a *config/setup* importer, not a transcript importer.** `goose session import` already ingests Claude Code `.jsonl` *conversations* (`crates/goose-cli/src/commands/session.rs`, `crates/goose/src/session/import_formats/claude_code.rs`). The new command lives in a **separate namespace** `goose::import::claude_code` and imports the **setup surface** (skills, CLAUDE.md, memory, MCP, settings, subagents, commands, output-styles, plugins). Do not reuse or collide with the transcript path. `crates/goose/src/lib.rs` has no `import` module today — `pub mod import;` is clean.

3. **Most "conversion" for skills is a normalize-and-relocate**, because goose already discovers `~/.claude/skills` in place via `all_skill_dirs` (`crates/goose/src/skills/mod.rs:296` ✓). Import's value is making config permanent in goose's canonical home `~/.agents/skills` (`skills/mod.rs:38` ✓, precedes `~/.claude/skills` in dedup so the imported copy wins) and routing the types goose does *not* auto-read (memory, hints, MCP).

4. **Idempotency + provenance are first-class invariants** (completeness §C, the most important process finding). The importer must be safe to run in a loop (the user's autonomous-loop mode guarantees it will be). Every write carries a provenance marker + source content-hash; re-import **reconciles** (unchanged→no-op, changed→replace, gone→report) instead of appending/suffixing. See §1.4.

5. **The swarm/formation view is the signature and is designed first** (clone-risk D16). The draft hand-designed every borrowed Claude-Code element and left goose's own differentiator as a slogan. That inversion is corrected: §2.2 designs the parallel-node fan-in unit (CLI + desktop) concretely, and the rest of the UI is derived from it.

### Confidence ledger (ranked by correctness risk, per the standing rule — NOT by effort)

| Piece | Confidence | Why |
|---|---|---|
| Skills import | **HIGH** | Near-identity format; canonical dir + `default_enabled:true` platform ext (`platform_extensions/mod.rs:198`) means it genuinely injects. |
| CLAUDE.md → hints | **HIGH** | Assembled by core PromptManager, not gated by any extension toggle; `~/.config/goose/.goosehints` is a default global context file (`load_hints.rs:235,289` ✓). |
| MCP import (stdio + http) | **HIGH** | `set_extension` upsert, `command→cmd`, `url→uri`, 31-key `DISALLOWED_KEYS` drop (`extension.rs:81` ✓), secret→keyring all verified. |
| Memory import | **HIGH after fixes** | Was a false-green (feasibility D1+D2). Fixed by one-entry-per-note write + enabling the `memory` builtin + provenance rewrite. |
| MCP enable/allowlist import | **HIGH** | Mechanical `disabled→enabled=false`, `allowedTools→available_tools` (`extension.rs:199,452`). Was silently dropped (completeness #1). |
| Output-styles → recipe | **MEDIUM-HIGH** | Same target as subagents; was mis-skipped on a false premise (completeness #8). |
| Desktop `.local-edition` CSS + edition state | **HIGH** | Direct extension of the shipping `.dark` block + ThemeContext mirror. |
| CLI palette/formation view | **MEDIUM-HIGH** | Needs a render-to-`String` refactor first (build-realism D6) — that refactor is the risk, not the colors. |
| **subagent → recipe** | **LOWER — flagged** | Semantic downgrade: recipes are user-*invoked*, subagents are model-*delegable* (feasibility D5). **Report-only by default; mapping behind an opt-in flag.** |
| **settings / permissions** | **LOWER — flagged** | Permissions DSL, hooks, statusLine have no goose equivalent; a wrong map silently changes agent authorization. **Report-and-skip; only `defaultMode→GOOSE_MODE` and `allowedTools→available_tools` port.** |
| **sse-transport MCP** | **LOWER — flagged** | `ExtensionConfig::Sse` "no longer supported" (`extension.rs:164` ✓). Auto-mapping to StreamableHttp produces a broken extension (feasibility D3). **SKIP+REPORT unless `--verify` handshake passes.** |
| **Signature accent color** | **LOWER — user's call** | `#13bbaf` is Block-corporate teal (focus-ring only), not "goose's own" (clone-risk D15). §2.3 gives a derived default + flags it for confirmation. |
| **Sharp/paper radii** | **taste, user's call** | Divergence from goose's rounded brand (clone-risk D14). Behind an explicit flag, not a gate. |
| **Verb sprawl consolidation** | **LOWER — flagged, non-breaking only** | 7 near-synonymous start-verbs (clone-risk D5). LE adds aliases + deprecation notes; never removes verbs. |

---

# DELIVERABLE 1 — `goose import claude-code`

## 1.1 Command surface (clap)

```
goose import claude-code [OPTIONS]

  --from <PATH>          Claude root (default: ~/.claude)
  --claude-json <PATH>   ~/.claude.json blob for MCP (default: ~/.claude.json)
  --project <PATH>       Import a specific project's scope from ~/.claude.json projects{}
  --all-projects         Walk EVERY project entry in ~/.claude.json (default: cwd project only)

  --dry-run              Plan only; write nothing. Prints the full action table.
  --all                  Import every supported type (default if no type flag given)
  --skills               Skills
  --memory               CLAUDE.md (→hints) + auto-memory (→global memory, enables memory ext)
  --mcp                  MCP servers (global + selected projects + committed .mcp.json)
  --commands             ~/.claude/commands/*.md + <repo>/.claude/commands → skills
  --output-styles        ~/.claude/output-styles/*.md → recipes  [CONVERT]
  --settings             Portable settings (env passthrough, model, defaultMode)  [LOSSY]
  --subagents            ~/.claude/agents/*.md → recipes  [LOSSY, opt-in, report-only without this flag]
  --plugins              Unpack enabled plugins' skills/commands/.mcp.json  [CONVERT]

  --conflict <POLICY>    merge | overwrite | skip   (default: merge)
  --prune                On re-import, remove previously-imported artifacts no longer in source
  --yes                  Skip the interactive confirm (autonomous/CI use)
  --verify               After apply, boot each imported stdio/http MCP for an initialize handshake
```

- No type flag ⇒ `--all`. Any explicit type flag ⇒ only those.
- Dry-run is **auto-implied when stdout is not a TTY** unless `--yes` is given (autonomous safety).
- Interactive summary + confirm is a **custom cliclack component, never a native prompt** (hard UI rule).
- `--subagents` is the only type whose *mapping* is off by default: without it, subagents are enumerated in the skip report; with it, they are written as recipes (LOSSY). This satisfies "when confidence is lower, don't guess-map" (feasibility D5, build-realism phase split).

## 1.2 Master SOURCE → TARGET mapping (honest classes)

Classes: **DIRECT** (format-preserving copy), **CONVERT** (deterministic reshape, fidelity-complete), **LOSSY** (some semantics don't survive — enumerated), **SKIP+REPORT** (no honest target — listed so nothing is silently lost).

| # | CC type | Source (verified) | goose target | Class | Honest reason / caveat |
|---|---|---|---|---|---|
| 1 | **Skills** | `~/.claude/skills/<n>/SKILL.md`+assets; `<repo>/.claude/skills` | `~/.agents/skills/<n>/` (`skills/mod.rs:38` ✓) | **DIRECT** | Copy dir verbatim; rewrite frontmatter only (§1.3.1). Imported copy wins dedup over `~/.claude/skills`. |
| 2 | **Global CLAUDE.md** | `~/.claude/CLAUDE.md` | `~/.config/goose/.goosehints` (`load_hints.rs:235` ✓) | **DIRECT** | Always-injected under `### Global Hints` (`:289` ✓). |
| 3 | **Project + nested CLAUDE.md** | `<repo>/CLAUDE.md`, `<repo>/.claude/CLAUDE.md`, **`**/CLAUDE.md`, `CLAUDE.local.md`** | matching `<dir>/.goosehints` | **DIRECT** | goose's loader walks cwd→git-root (`load_hints.rs` nested tests `:471,506,538` ✓), so nested files map 1:1. **[was: only top-level repo file — completeness #5]** |
| 4 | **Auto-memory notes** | `~/.claude/projects/<slug>/memory/*.md` | `~/.config/goose/memory/<category>.txt` (GLOBAL) | **CONVERT** | ONE entry per note, importer owns the file (§1.3.3). Global because only global auto-injects (`memory/mod.rs:132`). `MEMORY.md` index dropped; `[[links]]` flattened. |
| 5 | **Global MCP** | `~/.claude.json` `mcpServers{}` | `config.yaml` `extensions:` via `set_extension` | **CONVERT** | §1.3.2. `--verify` boots a handshake. |
| 6 | **Project MCP (private)** | `~/.claude.json` `projects[path].mcpServers{}` | same | **CONVERT** | Only selected project(s) via `--project`/`--all-projects`. **[was: cwd-only, 34 projects invisible — completeness D1]** |
| 7 | **Project MCP (committed)** | `<repo>/.mcp.json` `{mcpServers}` | same | **CONVERT** | Parsed if present under `--from`/`--project` repo. |
| 8 | **MCP enable/disable state** | `~/.claude.json` `projects[p].{enabled,disabled}McpjsonServers[]` | `ExtensionEntry.enabled` | **CONVERT** | `disabled→enabled:false`. Mechanical. **[was: silently dropped, all imported `enabled:true` — completeness #1]** |
| 9 | **MCP per-server tool allowlist** | `projects[p].allowedTools[]` (server-scoped) | that extension's `available_tools` (`extension.rs:199,452` ✓) | **CONVERT** | Honored by `supports_tool`. **[was: dropped, all imported as `available_tools: vec![]` = all-tools]** |
| 10 | **Slash commands** | `~/.claude/commands/<n>.md`; `<repo>/.claude/commands` | `~/.agents/skills/<n>/SKILL.md` | **CONVERT / LOSSY** | Pure `$ARGUMENTS`/`$n` = clean CONVERT; any `` !`cmd` `` bash, `@file`, or frontmatter `model` ⇒ LOSSY + reported (§1.3.4). **[was: "elegant 1:1" — feasibility D4]** |
| 11 | **Output styles** | `~/.claude/output-styles/<n>.md` | `~/.config/goose/recipes/<n>.yaml` `instructions` (`recipe/mod.rs:55`) | **CONVERT** | Frontmatter `name/description` + body → recipe `instructions` (same target as subagents). **[was: SKIP with the *false* "stdin/exit-code" reason — completeness #8]** |
| 12 | **Subagents** | `~/.claude/agents/<n>.md`; `<repo>/.claude/agents` | `~/.config/goose/recipes/<n>.yaml` (`get_recipe_library_dir(true)`=`config_dir/recipes` ✓) | **LOSSY, opt-in** | Body→`instructions`; `tools`→best-effort `extensions`/allowlist; `model` dropped unless a goose provider serves it. **Semantic downgrade: recipes are user-invoked, subagents are model-delegable** (feasibility D5). Report-only unless `--subagents`. |
| 13 | **Portable settings** | `~/.claude/settings.json`; **`settings.local.json`; `<repo>/.claude/settings*.json`** | `config.yaml` scalars/`providers` | **LOSSY** | Only `env` keys goose understands + `model` iff a goose provider serves it + `permissions.defaultMode`→`GOOSE_MODE` (§1.3.6). Everything else reported. **[was: global settings.json only — completeness #4/#10]** |
| 14 | **Permissions DSL** | `settings.json` `permissions{allow,ask,deny}` (`Bash(npm run test:*)` etc.) | *(none)* | **SKIP+REPORT** | No goose DSL. The *coarse* `defaultMode` and tool `allowedTools` DO port (rows 9/13); the fine-grained rules do not. Reported so the user re-expresses via `GOOSE_MODE`/`available_tools`. |
| 15 | **Hooks / statusLine / keybindings / output config** | `settings.json` `hooks`,`statusLine`; `keybindings.json` | *(none)* | **SKIP+REPORT** | CC stdin/exit-code contract is not portable. Each found item listed. |
| 16 | **mcpContextUris[]** | `~/.claude.json` | *(none)* | **SKIP+REPORT** | No goose auto-context-resource equivalent. **[was: absent from table AND report — completeness #2]** |
| 17 | **ignorePatterns[]** | `~/.claude.json` | *(none)* | **SKIP+REPORT** | goose has **no `.gooseignore`** (verified — only `.gitignore`, `load_hints.rs`). Writing globs into `.gitignore` would be destructive → report, don't map. **[was: silent — completeness #3]** |
| 18 | **Enterprise managed policy** | `/Library/Application Support/ClaudeCode/managed-settings.json` | *(none)* | **SKIP+REPORT** | Org policy, out of scope for a personal importer — one report line. **[was: silent — completeness #6]** |
| 19 | **Plugins** | `settings.json` `enabledPlugins`; `plugins/marketplaces/<mp>/plugins/<n>/{skills,commands,.mcp.json,agents}` | reuse rows 1/10/5/12 per contained type | **CONVERT** (with `--plugins`) | An enabled plugin is a bundle of already-handled types (the user's live `context7@…`, `warp@…` MCP servers live here). Unpack = near-free reuse. Without `--plugins`, each enabled plugin's contents are *enumerated in the report*. **[was: deferred wholesale to v2 — completeness #9]** |

## 1.3 Conversion rules (exact)

### 1.3.1 Skills (DIRECT + frontmatter normalize)
For each source skill dir:
1. Recursively copy to `~/.agents/skills/<name>/` (all `docs/`, `scripts/`, `references/`, `api/` carried verbatim; goose loads them via `load_skill("name/rel/path")`).
2. Rewrite only `SKILL.md` frontmatter (emit via `serde_yaml`, matching goose's own `build_skill_md`):
   - Keep `name`, `description` (goose's routing signal).
   - Relocate top-level `argument-hint`/`arguments` into nested `metadata:` (goose reads these only from `metadata`, `skills/mod.rs:144-167`).
   - Drop `allowed-tools` (no goose skill tool-gating) → stash under `metadata.x_claude_allowed_tools` for provenance.
   - Enforce `name` `/`-free, kebab-case, ≤64 (CRUD gate `skills/mod.rs:74`); slugify + keep dir==`name`.
   - **Sanitize** any literal `---` inside a frontmatter value (goose's naive `content.split("---")` parser, `sources.rs`).
   - Stamp provenance: `metadata.x_imported_from: claude-code`, `metadata.x_import_hash: <sha256>` (§1.4).

### 1.3.2 MCP (CONVERT via `set_extension`)
Per server `{name → server}`:
```rust
// stdio
ExtensionConfig::Stdio {
    name, description: server.description.unwrap_or_default(),
    cmd: server.command,                 // RENAME command→cmd
    args: server.args,
    envs: Envs::new(non_secret_pairs),   // 31 DISALLOWED_KEYS (PATH/NODE_OPTIONS/DYLD_*…) dropped → REPORT each
    env_keys: secret_names,              // token-ish keys → env_keys; value → Config::set_secret into keyring
    timeout: Some(300), cwd: None, bundled: None,
    available_tools: allowlist_for(name),   // from projects[p].allowedTools (row 9), else vec![]
}
// http (type:"http" / url present, NOT type:"sse") → StreamableHttp ONLY
ExtensionConfig::StreamableHttp { name, uri: server.url /*RENAME url→uri*/, headers, env_keys, available_tools, .. }
```
- **SSE carve-out (feasibility D3):** a server whose transport is genuinely `type:"sse"` is **SKIP+REPORT** ("SSE unsupported by goose; re-add if the endpoint also serves streamable HTTP"). If the user forces it via `--verify`, import as LOSSY and require a successful `initialize` handshake before marking success. Never silently map sse→StreamableHttp.
- **Secret heuristic:** env key matches `(?i)(token|key|secret|password|auth)` ⇒ value → keyring via `Config::set_secret`, name-only → `env_keys`; never inline into plaintext `envs`. **False-negatives (a secret under a non-matching key) get written as plaintext `envs` — every such key is surfaced in the report** (feasibility, don't rely on buried `tracing::warn!`).
- `enabled` from row 8. Persist via `set_extension(ExtensionEntry{ enabled, config })` — atomic upsert-by-key, preserves siblings.

### 1.3.3 Auto-memory (CONVERT → global memory) — **rewritten (fixes feasibility D1+D2)**
The reader (`memory/mod.rs` `retrieve`) splits a category file on `\n\n`, treats a leading `#`-line as space-separated **tags**, and files everything else under the joined-tag key with a HashMap **insert** (collisions overwrite). The draft's multi-paragraph "one blank line after description" format shattered each note into N chunks, consumed body headings as tags, and could overwrite content. Corrected rule — **one entry per note, importer owns the file**:

Per `memory/*.md` note (skip `MEMORY.md`):
- `category` = frontmatter `name` (kebab) → file `~/.config/goose/memory/<name>.txt`. One note ⇒ one category file ⇒ the importer can rewrite it wholesale each run (idempotent, §1.4).
- Write **exactly one entry**:
  - Line 1: `# <metadata.type-or-"feedback"> imported:claude-code` (the single tag line; the provenance token doubles as the reconcile key).
  - Then the note as ONE block: `description` + `" "` + body, with `\n\n+` collapsed to single `\n`, and **any body line starting with `#`/`##` de-headed** (strip the leading `#`s or indent) so the reader never eats it as tags. `[[wikilinks]]` flattened to plain text.
  - No blank line inside the entry.
- **Enable the memory builtin (feasibility D2):** `memory` is a builtin ext, `default_enabled` is *not* set for it (`DEFAULT_EXTENSION="developer"`, `config/extensions.rs:10` ✓); unlike skills it does **not** inject unless enabled. So `--memory` apply must also `set_extension(ExtensionEntry{ enabled:true, config: ExtensionConfig::Builtin{ name:"memory", .. }})`. Otherwise Phase-2's green report (which instantiates `MemoryServer::new` and reads files unconditionally) diverges from a real session (which spawns no `MemoryServer` unless enabled).

### 1.3.4 Slash commands (CONVERT/LOSSY → skill) — **[was "elegant 1:1"]**
`commands/<n>.md` ⇒ `~/.agents/skills/<n>/SKILL.md`:
- `name`=filename; `description`=frontmatter or first body line; body copied.
- goose's arg engine (`skills/arguments.rs`) handles `$ARGUMENTS`, `$ARGUMENTS[n]`, `$n`, `$name` only. It does **not** run `` !`cmd` `` bash, does **not** embed `@file`, has no `model`. Scan the body: if it contains any of those, classify the command **LOSSY** and enumerate it in the report (the unsupported construct becomes inert literal text). Pure-placeholder commands are the clean CONVERT case.
- Nested `commands/foo/bar.md` (CC `/foo:bar`) flatten to skill `foo-bar`.
- `argument-hint`→`metadata.argument-hint`; `allowed-tools`→`metadata.x_claude_allowed_tools`.

### 1.3.5 CLAUDE.md (DIRECT → hints)
- Global → `~/.config/goose/.goosehints`; nested/project → matching-dir `.goosehints` (row 3).
- Preserve `@relative/path` imports; **reject absolute imports** and any import escaping the git-root boundary (goose enforces this, `hints/import_files.rs`), rewriting-or-reporting them.
- Written inside a stable provenance-fenced block (§1.4) so re-import replaces rather than stacks.

### 1.3.6 Settings (LOSSY → report-first)
- `env{}`: pass through only goose-meaningful keys (`GOOSE_*`, provider base-URLs) as `config.yaml` scalars via `set_param`; everything else reported.
- `model`: set active model only if `get_all_providers` already serves that id; else report "model `X` has no matching goose provider — not set."
- `permissions.defaultMode` (`acceptEdits`/etc.) → best-effort `GOOSE_MODE` analogue (reported as approximate). Tool `allowedTools`→`available_tools` (row 9). Fine-grained `permissions` rules, `hooks`, `statusLine`, `keybindings`: SKIP+REPORT.
- Read global **and** `settings.local.json` **and** `<repo>/.claude/settings*.json` for the selected project.

## 1.4 Provenance & idempotency (first-class invariant — completeness §C)
The importer MUST be safe to run repeatedly. Every write carries an origin key + source content-hash, and re-import **reconciles**:

| Type | Provenance carrier | Reconcile behavior |
|---|---|---|
| Skills | `metadata.x_imported_from` + `x_import_hash` in SKILL.md | hash unchanged → **Unchanged**; changed → **Updated** (rewrite dir); source gone + `--prune` → remove. **[was: `foo`→`foo-imported`→`foo-imported-imported` unbounded suffix]** |
| Memory | 1 file per note, importer-owned | rewrite file in full each run → inherently idempotent. **[was: append-only → duplicates every run]** |
| Hints | fenced block `<!-- goose:import claude-code hash=… -->` … `<!-- /goose:import -->` in `.goosehints` | replace the matching fenced block; append only if absent. **[was: append below a marker with no replace → N stacked copies]** |
| Extensions | keyed by `name_to_key` | `set_extension` upserts → already idempotent. |
| Recipes (output-styles/subagents) | `x_imported_from`/`x_import_hash` in recipe yaml | Unchanged/Updated/prune, same as skills. |
| Settings | recorded in an import manifest (below) | `--conflict skip` respected (see §1.5) so re-import never re-clobbers a user-edited scalar. |

An **import manifest** `~/.config/goose/.import/claude-code.json` records every artifact the importer wrote (path, origin, hash, timestamp). It powers `--prune` and the revert path (§Revert) — the importer can enumerate exactly what it created without touching anything else.

## 1.5 Conflict handling (now covers ALL targets — completeness §C)
Resolved per-action from `--conflict`:
- **merge (default):** skills→new dir wins if absent, else reconcile-by-hash (not blind suffix); hints→replace-or-append the fenced block; memory→rewrite owned file; extensions→upsert; recipes→reconcile-by-hash; **settings→merge only keys not already user-set** (never clobber an existing `GOOSE_MODEL`).
- **overwrite:** replace the target artifact wholesale (incl. settings scalars).
- **skip:** target exists → no-op, mark `Skipped(exists)`. **Settings now honor `skip`** — the draft's unconditional `set_param` ignored `--conflict` and would clobber `config.yaml` scalars regardless. **[completeness §C fix]**

Every action records `{source, target, op, class, conflict, hash}` so the dry-run table and the post-apply report are identical in shape.

## 1.6 Rust module & CLI wiring plan (anchors re-verified ✓)

**New library module** `crates/goose/src/import/` — declare `pub mod import;` in `crates/goose/src/lib.rs` (no existing `import` module ✓):
```
crates/goose/src/import/mod.rs                    // pub use claude_code::*
crates/goose/src/import/claude_code/mod.rs        // ImportOptions, ImportPlan, plan(), apply(), validate()
crates/goose/src/import/claude_code/model.rs      // Action{Skill,Mcp,Memory,Hint,Recipe,Setting,Skipped}, ActionClass{Direct,Convert,Lossy,Skip}, ConflictOp, provenance/hash types
crates/goose/src/import/claude_code/manifest.rs   // read/write ~/.config/goose/.import/claude-code.json; reconcile + prune
crates/goose/src/import/claude_code/skills.rs
crates/goose/src/import/claude_code/mcp.rs        // parse ~/.claude.json (global + projects[]); enable/allowlist; secret split; sse carve-out
crates/goose/src/import/claude_code/memory.rs     // one-entry-per-note; de-head; flatten links; enable memory builtin
crates/goose/src/import/claude_code/hints.rs      // CLAUDE.md (global+nested) → fenced .goosehints; @import validation
crates/goose/src/import/claude_code/commands.rs   // slash→skill; !/@/model LOSSY scan
crates/goose/src/import/claude_code/output_styles.rs // → recipe instructions
crates/goose/src/import/claude_code/subagents.rs  // → recipe [opt-in]
crates/goose/src/import/claude_code/settings.rs   // env/model/defaultMode + skip report
crates/goose/src/import/claude_code/plugins.rs    // unpack enabled plugins → reuse above
```
Core API:
```rust
pub struct ImportOptions { from: PathBuf, claude_json: PathBuf, project_scope: ProjectScope,
                           types: TypeSet, conflict: ConflictPolicy, prune: bool, dry_run: bool, verify: bool }
pub fn plan(opts: &ImportOptions) -> Result<ImportPlan>;          // READ-ONLY scan
pub fn apply(plan: &ImportPlan, opts: &ImportOptions) -> Result<ImportReport>;   // writes + set_extension + set_secret + manifest
pub fn validate(report: &ImportReport, cwd: &Path) -> Result<ValidationReport>;  // re-reads goose to confirm visibility
```
Library placement (not CLI) so parsing/conversion is unit-testable with `tempdir` fixtures + overridden config/HOME dirs, mirroring `sources.rs`.

**CLI wiring** (`crates/goose-cli/src/`, anchors re-verified this session ✓):
- New `commands/import.rs` — `handle_import_subcommand(cmd)`; renders the plan table; custom cliclack confirm; calls `apply` + `validate`; prints report.
- `commands/mod.rs` (18 lines, ends `pub mod update;` ✓) — add `pub mod import;`.
- `cli.rs`:
  - New `#[derive(Subcommand)] enum ImportCommand { ClaudeCode(ImportClaudeCodeArgs) }` beside `enum SkillsCommand` (`cli.rs:729` ✓).
  - New `Command::Import { #[command(subcommand)] command: ImportCommand }` in `enum Command` (`cli.rs:802` ✓); insert after the `Skills` variant (`cli.rs:983-985` ✓).
  - Dispatch arm in `match cli.command` (`cli.rs:2229` ✓, Skills arm at `:2328`): `Some(Command::Import { command }) => commands::import::handle_import_subcommand(command).await`.
  - Telemetry arm in `get_command_name` (`cli.rs:1343` ✓, Skills arm at `:1361`): `Some(Command::Import { .. }) => "import"`. **A Phase-6 test asserts every `Command` variant has a telemetry name** (guards this mirror).

## 1.7 Validation — "does goose actually SEE + USE it?" (corrected API names — feasibility D6)
`validate()` re-enters goose's **real** read paths, against the fixture/temp config (never live config in autonomous phases):
- **Skills:** `goose::skills::discover_skills(Some(cwd))` returns `Vec<SourceEntry>` (`skills/mod.rs:460` ✓); assert each imported `name` present with `source_type: SourceType::Skill` (`:506` ✓) — **[was: `.kind`]**. Cross-check `goose skills list` shows them with token counts.
- **MCP:** `get_all_extensions()` contains each key; assert `get_extension_by_name(k).enabled` matches the imported enable-state (row 8) and `available_tools` matches the allowlist (row 9). With `--verify`, spawn each stdio/http server and require a successful MCP `initialize` within a timeout.
- **Memory:** call the memory server's `retrieve_all(true, None)` (two args, method on the server — `memory/mod.rs:174` ✓) — **[was: 1-arg free fn]** — assert each category present; **also assert `get_extension_by_name("memory").enabled` is true** (feasibility D2) and that `MemoryServer::new()` bakes the categories into `instructions` (`memory/mod.rs:132` ✓).
- **Hints:** call `goose::hints::load_hint_files(cwd, &get_context_filenames(), &gitignore)` (`load_hints.rs:225,13` ✓) — **[was: `load_hints()` — no such fn]** — assert CLAUDE.md content appears under `### Global Hints` (`:289` ✓).

A green validation report is the acceptance gate for every Deliverable-1 phase.

## 1.8 Interactive summary (custom, non-clone — clone-risk D4)
cliclack intro (formation-hue chip), grouped table, then a confirm reworded and reordered out of Claude's `Yes / Yes, don't ask / No` into goose voice:
```
 import · claude-code
  skills        11  →  ~/.agents/skills             DIRECT
  memory        33  →  ~/.config/goose/memory       CONVERT   (1 entry/note · memory ext enabled)
  hints          1  →  ~/.config/goose/.goosehints  DIRECT
  mcp servers    4  →  config.yaml extensions       CONVERT   (1 secret → keyring · 1 sse skipped)
  output-styles  0  —
  commands       0  —
  subagents      2  →  report-only (pass --subagents to map → recipes, LOSSY)
  skipped        —      permissions · hooks · statusLine · mcpContextUris · ignorePatterns  (not portable)
  Proceed?   Import all   Import & remember   Cancel
```
Numbered/keyboardable, sharp, solid formation-hue on the default action. Reordered + reworded so it is not Claude's exact string (the matrix promised this; the draft violated it).

---

# DELIVERABLE 2 — Goose Local Edition (CLI + Desktop)

**Design-order correction (clone-risk D1/D2/D16):** the draft adopted all 12 Claude-Code principles 1:1 and left goose's own differentiator (N parallel nodes) as prose. Here the **swarm/formation view is designed first (§2.2)** and the CC-derived grammar is deformed to fit it, not the reverse.

## 2.1 Edition selection & persistence — derived-first, no wizard wall

**Single source of truth:** `config.yaml` key `edition: standard|local` (`Config::set_param`/`get_param`).

**Resolution precedence (CLI):** `--local` flag > `GOOSE_LOCAL_EDITION` env > `config.yaml edition` > **derived default**. The derived default (clone-risk D10/D11/D12): if a local/swarm provider is configured (`active_provider` ∈ {`lmstudio`, swarm} or swarm nodes present), resolve to `local`; else `standard`. **No interactive first-run prompt** — the calm home is never interrupted for a skin. A one-line, dismissible inline note ("Local Edition on — local model detected · switch in Settings") is the only surfacing.
- Add global `--local` bool on `struct Cli` (`cli.rs:72` ✓).
- New `crates/goose-cli/src/edition.rs` — `resolve_edition()` implementing the precedence + derivation, plus a `set_edition()` persister. Unit-tested.

**Desktop:**
- New `ui/desktop/src/contexts/EditionContext.tsx` — `edition: 'standard'|'local'`, persisted via `window.electron.setSetting('edition', …)` (same mechanism as `ThemeContext.tsx`) **and** cached in `localStorage` for a synchronous pre-paint read (avoids the one-frame flash, mirroring `getResolvedTheme()` in `theme-tokens.ts`). Toggles `.local-edition` on `document.documentElement`.
- Wire into the provider stack in `ui/desktop/src/App.tsx` (wrap `ThemeProvider` so edition+theme both drive the document class).
- **Selection is fused into the existing provider/local-model choice, not a new screen** (clone-risk D10): in `components/onboarding/OnboardingGuard.tsx`, picking "Use a local model" auto-applies LE (opt-out checkbox), rather than inserting a separate `EditionSelector` card *before* `ProviderSelector`. The draft's pre-gate violated the very "no wizard wall" principle it revered.
- Re-selectable later: an "Edition" segmented control in `components/settings/app/AppSettingsSection.tsx` Appearance card, copying the `ThemeSelector` segmented-control pattern (`components/GooseSidebar/ThemeSelector.tsx`).
- Pre-paint hook: `renderer.tsx` reads the cached edition and stamps `.local-edition` alongside `applyThemeTokens()`.

## 2.2 THE SIGNATURE — the swarm/formation view (designed first; CLI + desktop)

This is the thing Claude Code cannot show, and it is the most-designed artifact in the doc (clone-risk D16). **Parallel node work renders as a structurally-different fan-in unit, not N sequential CC-cards each wearing a chip** (clone-risk D2).

**Concept.** Goose's reality is N models in *formation* (tensor-parallel over Thunderbolt/JACCL, or an LM-Studio swarm). The signature is a **dispatch → braid → fan-in** unit: one dispatch header, parallel node lanes that run concurrently, and a single rolled-up fan-in result. Node identity is a **solid inline chip** (a filled hex/letter token), **never a left rail** (hard UI rule — a per-row left color bar is explicitly forbidden).

**CLI fan-in unit** (rendered by new code in `crates/goose-cli/src/session/output.rs`, colors from `theme/palette.rs`):
```
  swarm · dispatch                              3 nodes · 2/3 done
   ⬢A  m4-max     edit auth.rs           +18 −4        ✔ 1.2s
   ⬢B  m3-ultra   grep callsites → 14 hits            ● running
   ⬢C  studio-2   cargo test → 47 passed              ✔ 0.8s
   ▾ fan-in · 18 tool-calls across 3 nodes · roll up
```
- `⬢A/⬢B/⬢C` = solid formation-hue filled glyph + node letter (identity chip, **inline leading token, not a rail**). Hue = the node's slot in the formation ramp (§2.3), disjoint from status colors.
- Status dot uses goose's own glyphs `●` running / `✔` done / `✕` error (not CC's `⏺`), colored from the **semantic triad** (OK/WARN/ERR), which is *orthogonal* to the identity ramp so a red status never reads as "node #5's identity."
- Collapses to `summary + N` with an expand hint (progressive disclosure), but the *unit* is the braid, not a stack of single-node cards.

**Desktop formation strip + fan-in card:**
- A compact horizontal **formation strip** of sharp full-border node cards, each headed by its solid formation-hue chip + device name + live status dot. Resident but *thin* — the transcript stays the hero.
- When a turn dispatches parallel work, an inline **fan-in card** in the transcript shows the braided lanes converging to one rolled-up result (the desktop twin of the CLI unit). This card — not the single-node card — is the hero component.
- A **node inspector** opens as an on-demand overlay (not a resident panel) for per-node logs/tokens/tool-calls.
- Files: new `components/swarm/FormationStrip.tsx`, `components/swarm/FanInCard.tsx`, `components/swarm/NodeInspector.tsx`; mounted by `components/Layout/AppLayout.tsx` and rendered into the turn by `components/BaseChat.tsx`.

## 2.3 Color system — formation ramp + semantic triad (fixes clone-risk D7/D8/D9/D15)

Two **orthogonal** color axes, so identity and state never collide:

**Axis 1 — node identity: a 6-hue "formation ramp", matched high saturation, DISJOINT from the status triad** (fixes D7 collision + D8 half-muted):
```
node1 cyan-teal   #17c4c4
node2 azure       #2e8bff
node3 indigo      #6a5cff
node4 violet      #b14cff
node5 magenta     #ff3ea5
node6 rose        #ff5c7a
```
A cool cyan→rose arc, all vivid (S≈70–100%), reinforcing the "formation" metaphor by hue-position. **None of these is green/yellow/red**, so a node chip is never confusable with a status. **[was: `NODES=[teal,orange,blue,green,yellow,red]` — node #5 chip was red(=error), and `#91cb80`/`#fbcd44` were goose's deliberately desaturated status hues → the exact "faded" the hard rule bans.]**

**Axis 2 — semantic status triad (unchanged role, solid):**
```
OK   #2ecc71   WARN #f5a623   ERR #ff3b30   DIM #878787
```

**Signature brand accent (LOWER-confidence, user's call — clone-risk D15).** `#13bbaf` is Block-corporate teal used only as goose's focus ring, and `#ff4f00` is Block orange used nowhere — calling teal "goose's own hue" was inaccurate. Rather than derive the signature from *avoidance* ("not-Claude-sand"), derive it from goose's world: the **formation ramp itself is the signature** (the mesh lighting up is what CC can't render). For the single brand-accent slot (LOCAL lockup, active/live emphasis), the recommended default is the ramp's anchor **azure `#2e8bff`** (migratory-dusk-sky → flight), **not** Block teal. **Flagged for the user's explicit confirmation** — a concrete default so build proceeds, but this is a brand decision, not a correctness one. (Mono-as-*voice* is itself an AI-local-tool cliché — clone-risk D14 — so mono is used as an *accent* for badges/code only, not as the product's typographic identity.)

**Diff colors — concrete spec, not a slogan (fixes D9):**
```
add-fg #2ecc71  add-bg #113b26   del-fg #ff3b30  del-bg #3b1113   hunk #6a5cff
```
Solid, legible, full-saturation foregrounds on dark low-key backgrounds (no `color-mix` washes), plus a **which-node-proposed** formation chip on each diff header. **[was: "solid legible" adjective with no hex.]**

## 2.4 CLI redesign — reuse the grammar, express as goose (file-level)

The CLI reads "wanton" for three reasons: color picked per-callsite (~97 ad-hoc `style()` in `output.rs`), `Theme` default `Ansi` (borrows terminal palette → no identity), and **verb sprawl** (7 near-synonymous start-verbs — `commands/mod.rs` shows `session`/`swarm`/`swarm_serve`/`term`/`tui` plus top-level `run`/`serve`). The draft fixed only color.

- **Shared palette (kills color wantonness).** New `crates/goose-cli/src/theme/palette.rs` exporting the two-axis tokens above as truecolor constants. **Refactor BOTH `output.rs` AND `swarm.rs` onto these tokens** (clone-risk D6 — `swarm.rs` is the swarm surface, hand-styling `on_cyan().black().bold()`; leaving it out re-creates two color systems). **Build-cost note (build-realism D2):** `swarm.rs` is **10,463 lines / 498 KB** and recompiles on any goose-cli edit — budget it; use `cargo check` in the inner loop.
- **Identity default.** When `edition==local`, default `Theme` away from `Ansi` to a fixed goose-dark (`output.rs` defaults `:26-27,76`).
- **Banner (reinterprets CC's calm-empty-prompt into goose's world — the one thing the draft got right, kept):**
  ```
   local  goose   ·   swarm 3 nodes ready · LM Studio · qwen3-class local
   >
  ```
- **Tool-call & fan-in rendering** per §2.2 (formation unit, goose glyphs, node chips).
- **Status line — reworded/reordered out of CC's exact fields (fixes D3).** CC's is `model · ctx% · cwd/branch · cost`. goose's leads with the differentiator: `nodes 2/3 · ctx 42% · qwen3-local · ~/proj (main)`. One line, live bits in the anchor accent.
- **Slash-command hygiene.** Retire the deprecated `/summarize` (`input.rs`), keep current mode always inline. Higher blast radius (flat `enum Command` + 3 dispatch/telemetry mirrors) → its own phase + gate (Phase 6).
- **Verb sprawl (clone-risk D5) — non-breaking only.** LE does **not remove** any verb (would break scripts/muscle-memory). It adds a unified calm `goose` default and emits a one-line deprecation/"did you mean" note aliasing the synonyms. **Flagged LOWER-confidence, optional phase, aliases-not-removal.** Removing verbs is explicitly out of scope for LE.

## 2.5 Desktop LE theme + shell (file-level, hard rules baked in)

**Theme tokens** (`ui/desktop/src/theme/theme-tokens.ts` + `ui/desktop/src/styles/main.css`):
- Add the formation-ramp + triad + anchor-accent aliases to the `@theme inline` block so Tailwind emits `bg-node-1…6`, `bg-accent-local`, etc.
- **Palette override, not token-map rewrite (low-risk route):** add `.local-edition { … }` and `.local-edition.dark { … }` blocks in `main.css` that override the `--color-*` custom properties, modeled exactly on the existing `.dark` block (`main.css` ~`:181-233`). Composes with light/dark for free; doesn't touch token-type machinery.
- **Hard UI rules baked in and audited:** accents are solid saturated fills (**no `color-mix` washes, no 8–12% tints**); emphasis via full borders, solid status dots, node chips, bold colored numerals — **never a left rail**; all dialogs/dropdowns use goose's own primitives (audit for any native `<select>`/`alert`/`confirm`).

**Sharp/paper (flagged — user's call, clone-risk D14).** goose ships rounded (8px, 20% icon squircle); the goose-brand argument is radii keep it "unmistakably goose." Recommendation: for LE only, override `--radius-*` to sharp (2–4px) + flatten shadows, keeping mascot + neutral ramp so it still reads as goose. **Behind an explicit flag, not a gate** — confirm before Phase 8 touches radii.

**Shell demotion (transcript is the hero; swarm = one strip + overlays, not resident panels):**
- `components/GooseSidebar/EnvironmentBadge.tsx` — generalize the dev-dot (`bg-orange-400`) into the **one** LOCAL badge (solid anchor-accent chip; note: this is a *recolor* orange→accent, called out visually per D13). **Restraint (clone-risk D13):** the signature lives in exactly one shell chip + the wordmark lockup + the app icon — **the redundant resident transcript-adjacent chip is dropped**. "LOCAL" is not sprinkled 4–5 places.
- `components/Layout/AppLayout.tsx` + `components/Layout/NavigationPanel.tsx` — reduce resident chrome; node inspector / MCP sidecars open as on-demand overlays.
- `components/Hub.tsx` — the calm home, rebranded with the node-readiness line.
- `components/BaseChat.tsx` / `components/ChatInput.tsx` — render the fan-in card (§2.2) and each tool step as a **sharp full-border paper card** (status dot + node chip, no left rail); swap wordmark for the LOCAL lockup (both already import `icons/Goose.tsx`).
- Approvals: custom inline numbered component in the transcript, reusing goose's dialog primitive — **never `window.confirm`** — reworded out of CC's exact strings.

**Assets:**
- LOCAL lockup = goose glyph + solid anchor-accent chip, white mono bold (mono as *accent* only).
- App icon bg: `ui/desktop/src/images/icon.svg` `fill="#ffffff"` → `fill="#2e8bff"` (anchor accent; keep black goose, keep `rx="400"` unless sharp confirmed); regenerate `.png/.icns/.ico`.
- **Bundle a self-hosted mono** as local `.woff2` (JetBrains Mono / IBM Plex Mono) — `--font-mono` is bare `'monospace'` today. **Also replace the remote `cash-f.squarecdn.com` Cash Sans `@font-face` with a bundled asset** (offline/local ethos; also required — Artifact/CSP-style, no external hosts, and it's on-theme for a *local* edition).
- Brand strings: `ui/desktop/index.html` `<title>` and `package.json` `productName` conditionally "Goose Local Edition."

## 2.6 Identity-drift accounting (clone-risk D14 — the question the draft never asked)
After all divergences (sharp corners, bundled mono-as-accent, azure accent, goose-own glyph set, restyled cards, demoted chrome), **is it still recognizably goose?** Explicit answer and the anchors that keep it goose: the **mascot is unchanged**, the **neutral ramp is unchanged**, the **layout/IA is goose's existing shell** (only demoted), and the **formation view is additive** (goose gains a signature, doesn't lose one). Divergences that are *taste* (sharp/paper, azure-vs-teal) stay behind the user's explicit yes precisely so drift is opt-in and reversible, not a silent slide into "a new product."

## 2.7 Reuse-not-clone matrix (reworded so it actually diverges — fixes D3/D4)

| CC principle | Adopted as goose (concrete divergence) | NOT copied |
|---|---|---|
| Calm empty prompt | Node-readiness banner (`swarm 3 nodes ready · LM Studio · qwen3-local`) | Claude's paper/sand editorial world |
| One linear transcript | Transcript is hero; chrome → overlays | — |
| Titled tool units | **Fan-in braid** for parallel nodes (structurally different unit), goose glyphs `●✔✕`, formation chip | CC's single-card-per-call, `⏺`, exact strings |
| Neutral + one accent | Neutral goose ramp + **formation ramp** as signature | Block teal/orange; faint diff tints |
| Terse status line | `nodes 2/3 · ctx% · model · cwd(branch)` — **swarm-led order** | CC's `model·ctx·cwd·cost` field order |
| Inline mode | Palette + slash cmds, mode always visible | CC keybindings verbatim |
| Inline numbered approvals | Custom component, reworded/reordered options | CC's `Yes / Yes don't ask / No` + native confirm |
| Plan/diff-before-apply | Reviewable swarm plan; **solid hex diff** + which-node-proposed | Claude's washed diff greens |
| Progressive disclosure | Fan-in roll-up of parallel node output | — |
| Glyph + spinner | goose's own flight/wind voice + glyph set | Claude's `⏺` + playful verbs |
| Onboarding by doing | Derived LE + gentle inline node/LM-Studio detection | Wizard walls (incl. the draft's own edition pre-gate) |
| One signature element | **Formation/fan-in view** (the thing CC can't show), used with restraint | Reskinning Claude's minimalism until goose disappears |

---

# BUILD PLAN (phased, risk-ordered, autonomously-runnable)

**Gate conventions (build-realism D1/D3/D4):**
- Every gate is prefixed `source bin/activate-hermit &&` — the baseline is **RED without hermit** (`cargo build -p goose-cli` dies in `llama-cpp-sys-2` because `cmake` is hermit-only; `.hermit/bin/.cmake-4.2.3.pkg` provides it). The agent's Bash resets cwd + env per call, so **keep hermit active for the whole loop and never switch env mid-run** (the switch itself invalidates fingerprints → forces a ~5.5-min rebuild).
- **Rust gate** = `source bin/activate-hermit && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test -p <crate>` (AGENTS.md mandates fmt+clippy; "build passes" ≠ "commit gate passes"). Inner fix-loop uses `cargo check`/`clippy` (no codegen); full `build`+`test` only at phase exit. Per-edit warm-cache recompile of `goose`+`goose-cli`+llama link ≈ **5m33s measured** — consider offloading to the workhorse (M3 Ultra) in parallel per the user's setup.
- **Desktop gate** = `pnpm run typecheck && pnpm run lint:check && pnpm test:run` (the desktop is **pnpm, and has NO `build` script** — `npm run build`/`npm --prefix … build` in the draft were unrunnable). `pnpm make` (electron-forge full package) is far too heavy per iteration.
- **Isolation (build-realism D5/D7/D8):** all apply/validate runs against a **tempdir fixture with `HOME`/config-dir overridden** — never the user's live `~/.claude`, `~/.config/goose`, keyring, or desktop `userData`, until the single final live phase.

**Phase −1 — Env + revert harness (NEW).** Activate hermit; assert `cmake`/`cargo`/`pnpm` on PATH; `git switch -c le-autobuild pre-claude-code-import`; back up `~/.config/goose/` and desktop `userData/settings.json`.
- Gate: green `cargo build -p goose-cli` under hermit (also primes the warm cache once).
- Validation: baseline provably green before any change.
- Confidence: **HIGH.**

**Phase 0 — Edition plumbing (CLI + config).** `--local` (`cli.rs:72`), `GOOSE_LOCAL_EDITION`, `config.yaml edition`, `crates/goose-cli/src/edition.rs` resolver **with derivation** (§2.1) — no interactive first-run prompt.
- Gate: Rust gate + precedence/derivation unit test (flag>env>config>derived; local-provider→local).
- Validation: `GOOSE_LOCAL_EDITION=1 goose --help` runs; resolver returns `local` given an lmstudio config.
- Confidence: **HIGH.**

**Phase 1 — Import core, READ-ONLY plan** (skills/memory/hints/mcp incl. enable+allowlist rows 8/9). Module tree §1.6 + CLI wiring (`ImportCommand`, `Command::Import`, dispatch `:2229`, telemetry `:1343`).
- Gate: Rust gate + `cargo test -p goose import::claude_code` over **tempdir fixtures** (a fake `~/.claude` + fake `~/.claude.json` with projects/enable/allowlist/sse).
- Validation: `--dry-run --from <fixture>` prints the action table with correct classes (incl. sse→SKIP, subagents→report-only) and asserts **zero writes**.
- Confidence: **HIGH.**

**Phase 2 — Import apply + validate (DIRECT/CONVERT), sandboxed.** `apply()` for skills, hints (fenced), memory (one-entry + **enable memory builtin**), mcp (+secret→keyring, sse carve-out, enable/allowlist), provenance manifest + reconcile, conflict policies; then `validate()`.
- Gate/Validation are one thing: run apply into a **temp config dir**, then green `discover_skills`/`get_all_extensions`(+`enabled`/`available_tools`)/`retrieve_all(true,None)`(+ `memory` ext enabled)/`load_hint_files`. **Re-run apply and assert idempotency** (no duplicate memory, no stacked hint block, no `-imported-imported`). This is the Deliverable-1 benchmark. No live-config mutation.
- Confidence: **HIGH** (was a false-green before the memory + enablement fixes).

**Phase 3 — command→skill (DIRECT/LOSSY-scan) + output-styles→recipe.** slash→skill with the `!`/`@`/`model` LOSSY scan; output-styles→recipe `instructions`; plugin-unpack reuses these.
- Gate: Rust gate + fixture tests asserting `$ARGUMENTS/$1/$2` substitution parity via `skills::arguments`, and that a command containing `` !`cmd` `` is classified LOSSY + reported.
- Validation: imported command `load_skill`s with substituted args in temp config; imported output-style parses as a recipe.
- Confidence: **HIGH** for pure-placeholder commands + output-styles; **MEDIUM** on the LOSSY-scan completeness.

**Phase 4 — subagents + settings/permissions (LOW-CONFIDENCE, ISOLATED, report-only default).** Default = SKIP+REPORT for subagents, permissions DSL, hooks, statusLine; only `env`/`model`/`defaultMode→GOOSE_MODE`/`allowedTools→available_tools` port; subagent→recipe mapping only behind `--subagents`.
- Gate: fixture test asserting the skip report enumerates permissions/hooks/statusLine/mcpContextUris/ignorePatterns/managed-policy, and that **nothing is written for these types without the opt-in flag**; settings honor `--conflict skip` (no clobber of an existing `GOOSE_MODEL`).
- Validation: with `--subagents`, an agent emits a recipe that `goose recipe` parses (syntactic only).
- Confidence: **LOWER — flagged for extra adversarial review.** Semantics genuinely don't survive; report-and-skip means a wrong map can't silently change agent authorization.

**Phase 5 — CLI Local Edition look. Render-to-`String` refactor FIRST (build-realism D6).**
- Sub-step A (prerequisite, non-visual): refactor target `output.rs` render fns (`render_message`/`render_error`/etc., currently `println!` to stdout returning `()` — 103 print sites) to return `String`/write to a buffer; **add `insta` to goose-cli dev-deps** (it is NOT a goose-cli dep today, only a `goose` dev-dep). Snapshot current output to lock no-visual-change.
- Sub-step B: `theme/palette.rs` (two-axis tokens); refactor `output.rs` **and `swarm.rs`** onto tokens; LE banner + LOCAL chip; **fan-in/formation unit (§2.2)**; one-line swarm-led status; fixed goose-dark default when local.
- Gate: Rust gate + `insta` snapshots of the fan-in unit + status line; assert the anchor-accent ANSI codes and node-ramp hues appear, and that no node hue equals an OK/WARN/ERR code.
- Confidence: **MEDIUM-HIGH** (the refactor is the risk, not the colors; `swarm.rs` size is a build-cost tax).

**Phase 6 — Slash-command hygiene (own phase, own gate — build-realism).** Retire `/summarize`, mode-inline; add non-breaking verb aliases + deprecation notes (no verb removal).
- Gate: Rust gate + a test that **every `Command` variant maps to a telemetry name** (guards the 3-mirror sync, `get_command_name` `:1343`) + dispatch tests.
- Confidence: **MEDIUM** — higher blast radius (flat `enum Command` + 3 mirrors); a broken mirror breaks the whole CLI, hence isolated.

**Phase 7 — Desktop edition state + selection.** `EditionContext.tsx`, fused selection in `OnboardingGuard` (no pre-gate screen), Settings segmented control, `renderer.tsx` pre-paint stamp, `App.tsx` wiring.
- Gate: desktop gate + **vitest/RTL** tests (EditionContext toggles `.local-edition` on `documentElement`, persists via mocked `setSetting`, pre-paint cached read). **Budget writing this harness — there are 0 existing vitest tests touching theme/context/onboarding.**
- Validation: **one** Playwright pass pointing goose's config home at a **temp dir with no provider** so onboarding actually renders (the user's real config has `active_provider: lmstudio`, so `OnboardingGuard` would otherwise never show — build-realism D5): local selection stamps `.local-edition`, survives reload.
- Confidence: **HIGH** on the vitest path; **MEDIUM** on Playwright (headed Electron + CDP flakiness) — hence it's one reserved pass, not the primary gate.

**Phase 8 — Desktop LE theme + shell + live full-stack A/B.** `.local-edition`/`.local-edition.dark` overrides, formation-ramp + triad + accent tokens, LOCAL badge recolor in `EnvironmentBadge`, sharp fan-in/tool cards in `BaseChat`/`ChatInput`, `FormationStrip`/`FanInCard`/`NodeInspector`, shell demotion, bundled mono + bundled Cash Sans, accent `icon.svg`. Sharp-radius token only if the user confirmed §2.5.
- Gate: desktop gate + Playwright screenshots LE vs standard (light+dark) + **CSS audit asserting NO `border-left` accent, NO native `<select>`/`window.confirm`, NO `color-mix` washes**, and that node chips use the formation ramp (disjoint from status).
- Validation (live, last, after Phase −1 backups): scripted flow — run the real import, then `goose --local` session that `load_skill`s an imported skill and lists an imported MCP tool (proves import + edition compose); desktop screenshot with the LOCAL badge + an imported skill invoked in a fan-in card. Per "validate features by enabling," LE is default-on for LE builds, proven valuable and non-breaking.
- Confidence: **HIGH** on tokens/CSS/audit; **MEDIUM** on the live A/B (Electron/Playwright).

---

# REVERT PATH (covers git AND out-of-repo state — build-realism D8/D9)

Git alone does **not** undo imported skills (`~/.agents/skills`), memory (`~/.config/goose/memory`), extensions + **keyring secrets** (`config.yaml`/keychain), or desktop `edition` (`userData/settings.json` + `localStorage`). Full revert:

1. **Code:** `git switch local-edition && git branch -D le-autobuild`. The whole build ran on the throwaway `le-autobuild` branch (Phase −1); a failed run is discarded by deleting the branch — **never force-push** (the draft's "commit+push per phase" would have published commits, forcing a `reset --hard && push --force` on shared history). `local-edition` is fast-forwarded only on full success.
2. **Config:** restore the Phase −1 backups of `~/.config/goose/` and desktop `userData/settings.json`.
3. **Imported artifacts:** the import manifest (`~/.config/goose/.import/claude-code.json`, §1.4) enumerates exactly what the importer created → remove those `~/.agents/skills`/memory/recipe files, delete the keyring secrets it set, and remove the extension entries it added. **Never** glob/wildcard-`rm` in `$HOME` — only the exact manifest-listed paths (safe-deletion rule).
4. **Desktop:** clear the `edition` setting + `localStorage` key.

Because all autonomous apply/validate runs against a sandboxed temp config (Phases 1–7), only Phase 8's final live A/B ever touches real config — so the blast radius of a revert is small by construction.

---

# CONFIDENCE CALL-OUT PER PHASE (restated for the user — ranked by correctness risk, not effort)

- **HIGH:** Phase −1, 0, 1, 2, 7 (vitest path), 8 (tokens/CSS/audit). These are direct extensions of shipping goose mechanisms verified in-tree this session (canonical skill dir + `default_enabled:true`, `set_extension` upsert, hints assembled by core, `.dark`-block CSS pattern, ThemeContext mirror). The memory path is HIGH **only after** the one-entry rewrite + `memory`-builtin enablement — without both it is a false-green.
- **MEDIUM-HIGH:** Phase 5 sub-step B (colors are easy; the render-to-`String` refactor of `output.rs`/`swarm.rs` is the real work + build-cost tax).
- **MEDIUM:** Phase 3 LOSSY-scan completeness; Phase 6 (CLI-wide dispatch blast radius); Phase 7/8 Playwright/Electron passes (headed-app flakiness → reserved, not primary, gates).
- **LOWER — the genuine risk pockets, designed to report-and-skip so a wrong map can't land silently:** Phase 4 **subagent→recipe** (semantic downgrade: user-invoked recipe vs model-delegable subagent) and **settings/permissions** (no goose DSL; only coarse `defaultMode`/`allowedTools` port). Flagged for extra adversarial review. The **sse-transport** MCP mapping is LOWER for the same reason (broken artifact if auto-mapped) — carved out to SKIP+REPORT unless `--verify` passes.
- **User's call, not correctness (concrete defaults given so build proceeds, but confirm before Phase 8):** the **signature accent** (azure `#2e8bff` derived from goose's world, NOT Block's focus-ring teal) and **sharp/paper radii** (behind a flag, not a gate). Both are behind explicit opt-in so identity-drift is reversible, never a silent slide into a different product.