# Claude Code → Goose Local Edition — Import Tool (plan)

## TL;DR — the big surprise

**Most of the backend already exists.** Goose ships a `goose import claude-code` command that already
imports three of the four categories, and it already *discovers* `~/.claude/skills` read‑only. So the heavy
lift you're describing is almost entirely the **desktop UI** — a conditional, category‑based import wizard —
plus a thin wiring layer, **not** new import engines. That lowers the risk a lot.

What exists today (Rust, `crates/goose/src/import/claude_code/`):
- **`hints.rs`** — `~/.claude/CLAUDE.md` → `~/.config/goose/.goosehints` (idempotent, provenance‑fenced block,
  conflict `merge|overwrite|skip`).
- **`memory.rs`** — Claude auto‑memory notes → `~/.config/goose/memory/<category>.txt`.
- **`mcp.rs`** — MCP servers from **`~/.claude.json`** (`mcpServers`) → goose `extensions:` in `config.yaml`
  (stdio→Stdio, http/url→StreamableHttp, secrets split into keyring `env_keys`, sse skipped).
- CLI: `goose import claude-code [--hints|--memory|--mcp|--all] [--apply] [--yes] [--conflict merge|overwrite|skip]`
  (dry‑run preview by default). Wired at `cli.rs:747` / dispatched `cli.rs:2352`.

What does **not** exist yet:
- **Skills import** is *not* part of `goose import claude-code`. Skills go through the separate **sources**
  API (`sourcesImport_unstable` / `sourcesCreate_unstable`) — greenfield in the desktop.
- **Any desktop UI** for importing. `goose import claude-code` is CLI‑only; the sources/extensions add APIs
  exist over ACP but nothing calls them from a wizard.

## What's importable from YOUR `~/.claude` (the "conditional" logic, grounded in real data)

| Category | Present now | Source | Goose target | Backend status |
|---|---|---|---|---|
| **Skills** | **11** (aerlingus, atlassian‑*, goose‑knob‑turning, jira‑api‑skill…) | `~/.claude/skills/<name>/SKILL.md` | `~/.agents/skills/<name>/` | sources API exists; **wizard greenfield** |
| **Memory / instructions** | CLAUDE.md (~8.4 KB) + auto‑memory notes | `~/.claude/CLAUDE.md`, memory notes | `.goosehints` + `memory/` | **importer exists** (`hints.rs`/`memory.rs`) |
| **MCP servers** | in `~/.claude.json` (`mcpServers`) | `~/.claude.json` | `config.yaml` `extensions:` | **importer exists** (`mcp.rs`) |
| **Slash commands** | **0** | `~/.claude/commands/` | goose recipes | hide (none present) |
| **Subagents** | **0** | `~/.claude/agents/` | goose recipes/subrecipes | hide (none present) |
| **Plans** | 69 (`~/.claude/plans/*.md`) | design docs, not recipes | ??? ("loops"?) | **open decision** — see below |

The wizard renders **only categories that scan non‑empty** — exactly the conditional behavior you asked for.
On your machine that's Skills + Memory + MCP; commands/agents stay hidden.

## Key facts that shape the design (from grounding)

**Skills are format‑identical.** A goose skill is a `SKILL.md` directory with YAML frontmatter
(`name`, `description`, nested `metadata:`) — the same shape as Claude Code. Goose *already reads*
`~/.claude/skills`, so the 11 skills are technically usable as‑is; "import" **relocates** them into a
writable canonical dir so they're first‑class and editable. Two write routes:
- **API**: `sourcesImport_unstable({data:{version:1,type:'skill',name,description,content}, target})` —
  `content` = the SKILL.md **body only** (goose regenerates frontmatter). Auto‑suffixes on name collision.
- **Dir copy** (Electron main, Node `fs`): copy `~/.claude/skills/<name>/` → `~/.agents/skills/<name>/`.
  **Required for skills with supporting files** — the API drops everything except SKILL.md, and
  `jira-api-skill` alone has ~80 supporting files (docs/, scripts/, templates/). So: **body‑only skills → API;
  skills with supporting files → dir copy.** Name rule: `^[a-z0-9-]+$`, ≤64 chars (validate/slugify on import).

**MCP source is `~/.claude.json`, not `settings.json`** (a grounding correction). `mcp.rs` already maps the
fields and splits secrets into the keyring. `settings.json` and `mcp-needs-auth-cache.json` do *not* hold
server defs.

**Desktop scan primitives already exist** (renderer): `window.electron.readFile('~/.claude/CLAUDE.md')`
returns `{file, found, error}`; `window.electron.listFiles('~/.claude/skills')` enumerates dir names;
`~` is expanded in the main process — pass `~/.claude/...` literally. (There is **no** `Checkbox` component —
use `Switch variant="mono"`, per `RecipeExtensionSelector`.)

## Proposed architecture

A single self‑contained modal wizard, `components/import/ImportFromClaudeCodeModal.tsx`, launched from a
**conditional Settings tab `import`** (mirror the `{showSwarm && <TabsTrigger>}` pattern in `SettingsView.tsx`).
Three steps:

1. **Scan / detect (conditional).** On open, in the renderer:
   - `listFiles('~/.claude/skills')` → for each dir, `readFile('.../SKILL.md')`, parse frontmatter for
     name/description, and `listFiles` the dir to flag "has supporting files".
   - `readFile('~/.claude/CLAUDE.md')` → present size + a preview; `readFile('~/.claude.json')` → parse
     `mcpServers` into a list (name, transport, whether it needs secrets).
   - Render only non‑empty categories.

2. **Select (per‑item checklists).** Reuse `RecipeExtensionSelector`'s `Set<string>` + `<Switch>` row pattern,
   grouped under headings: **Skills** (each with a "docs/scripts included" badge), **Memory** (CLAUDE.md → hints;
   auto‑memory notes), **MCP servers** (each with a "sets a secret" warning). Global vs project **scope** toggle
   at the top (default global). "Select all / none" per group.

3. **Apply + report.** Per selected item, call the right route (below), collect per‑item success/skip/error,
   show a result summary, toast, and reload the relevant views (SkillsView / extensions / hints).

### How each category applies (the wiring — this is the real new code)

- **Skills** → new `acp/sources.ts` helpers: `importSkillSource(...)` (`sourcesImport_unstable`) for body‑only;
  for skills with supporting files, a **new Electron IPC `copy-skill-dir(src, destScope)`** (Node `fs.cp`
  recursive) into `~/.agents/skills/<name>/`, then reload. (Wire into the existing hidden "Add Skill" button too.)
- **Memory (CLAUDE.md + notes)** → **decision A/B below.** Simplest: a new IPC that shells
  `goose import claude-code --memory --apply --yes --conflict <chosen>` (reuses the idempotent,
  provenance‑fenced importer wholesale). Finer‑grained/native: expose `hints.rs`/`memory.rs` over ACP.
- **MCP servers** → read `~/.claude.json` in the renderer, map each selected server to an `ExtensionConfig`
  (stdio→cmd/args/envs, http/url→StreamableHttp; route token/secret env vars to keyring `env_keys`), and add via
  the existing `acp/extensions.ts` `addConfigExtension(config, enabled)` (`configExtensionsAdd_unstable`).
  Skip `sse` (goose dropped it) with a visible note. (Alternative: shell `goose import claude-code --mcp --apply`.)

## Build phases

**Source 1 — Claude Code:**
1. **Skills‑only v1** (highest value, all APIs ready): scan `~/.claude/skills`, checklist, import via API + the
   new dir‑copy IPC for supporting‑file skills. Ships the wizard shell + the conditional Settings tab.
2. **MCP servers**: parse `~/.claude.json`, per‑server checklist with secret warnings, add via
   `configExtensionsAdd_unstable`; sse skipped.
3. **Memory (CLAUDE.md + notes)**: new IPC shelling `goose import claude-code --memory --apply --yes`.

**Source 2 — Goose config (the "loops" ask):**
4. **Recipes**: reuse `ImportRecipeForm` + a scan of a chosen goose `recipes/` dir → multi‑select → `saveRecipe`.
5. **Loops**: read schedules (cross‑config read or export/import JSON) → `acpCreateSchedule`.

**Polish:** conflict handling UI, idempotency indicators ("already imported / newer available"), dry‑run preview.

Recommended order to actually ship: **Phase 1 (skills)** first — it's the highest‑value, highest‑confidence,
all‑APIs‑ready slice and it stands up the whole wizard shell + conditional Settings tab that phases 2–5 slot into.

## Decisions (LOCKED 2026-07-10)

1. **"Loops" = import goose's OWN loops/recipes** (a SECOND source, goose→goose — NOT from Claude Code). So the
   tool is a **hub with two sources** (see "Source 2" below). Claude Code plans are NOT imported.
2. **Memory route = shell the CLI for v1.** A new IPC runs `goose import claude-code --memory --apply --yes
   --conflict <chosen>`, reusing the idempotent, provenance‑fenced importer. ACP‑expose later only if
   per‑section selection is wanted.
3. **Supporting files → dir copy** whenever a skill has extras (else `jira-api-skill` loses ~80 files);
   body‑only skills go through the API.
4. **Scope default = global**, with a project toggle.

## Source 2 — Goose config (recipes + loops), the "loops of its own" ask

A second tab/section in the same wizard: **Import goose config** — bring recipes and loops from ANOTHER goose
setup (another project's `~/.config/goose` or an exported bundle) into this one.
- **Recipes**: already importable — reuse `ImportRecipeForm` (goose://recipe deeplink or a recipe YAML file)
  and `saveRecipe`/`recipesSave_unstable`. The wizard can also scan a chosen goose config's `recipes/` dir and
  multi‑select.
- **Loops (schedules with `loop_config`)**: read schedules from a source (another config's schedule store, or a
  JSON export) and re‑create via `acpCreateSchedule` — the exact call `LoopView`/the Agent wizard already use.
  **Implementation detail to resolve at build time:** how goose persists schedules and whether they can be read
  from a *different* config dir (vs. an explicit export/import file). If there's no clean cross‑config read, add
  a lightweight "export loops → JSON / import loops ← JSON" pair. Flagged **MEDIUM** confidence.
- Conditional here too: show Recipes / Loops only if the chosen source has them.

## Confidence flags (honest)

- **HIGH**: skills import (format‑identical, APIs exist, verified on `jira-api-skill`), MCP add
  (`mcp.rs` + `configExtensionsAdd_unstable` both exist), the conditional wizard UI (all scan primitives + the
  checklist pattern exist).
- **MEDIUM**: the dir‑copy IPC (new Electron main code — straightforward Node `fs.cp`, but new); the
  renderer‑side MCP field mapping if we don't shell the importer (duplicates `mcp.rs` logic — better to shell).
- **LOWER / needs your input**: anything under "loops" (decision 1) — I don't want to build a lossy
  plan→recipe transform on a guess; and per‑section memory (only if you reject shelling).

## Files to touch (map)

- **New**: `ui/desktop/src/components/import/ImportFromClaudeCodeModal.tsx`; helpers in
  `ui/desktop/src/acp/sources.ts` (`importSkillSource`) and `ui/desktop/src/acp/extensions.ts` (reuse
  `addConfigExtension`); new IPC `copy-skill-dir` (+ maybe `run-cc-import`) in `main.ts` + `preload.ts`.
- **Edit**: `ui/desktop/src/components/settings/SettingsView.tsx` (conditional `import` tab), reuse
  `RecipeExtensionSelector` checklist, `SkillsView.tsx` hidden "Add Skill" button → wire to the wizard.
- **Reuse (no change)**: `crates/goose/src/import/claude_code/*` (hints/memory/mcp), sources + extensions ACP.
