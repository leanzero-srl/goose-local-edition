# Findings ledger — Goose MLX quality loop

Method and statuses: `~/.claude/skills/goose-mlx-quality-loop/SKILL.md`. Ids are assigned by the coordinator;
rows are never deleted. Severity: dead end > misleads > friction > cosmetic. Verdict: CONFIRMED (seen on
screen / measured) or PLAUSIBLE (read in code).

| id | date | track | surface | finding | scenario | verdict | status | note |
|---|---|---|---|---|---|---|---|---|
| Q-1 | 2026-09-25 | ux | Engine tile | headline "Running" over an idle tile; "Idle" only in the corner | 27B idle on the Studio | CONFIRMED (owner) | shipped 3.0.33 (5bd73e93b) | |
| Q-2 | 2026-09-25 | ux | Engine tile | big number = last run only, not a median | a short fast answer shown as the model's rate | CONFIRMED (owner) | fixed 3cb0e0af8 + 06f58b25b · scheduled 3.0.35 | main's run book merged in |
| Q-3 | 2026-09-25 | ux | composer | context always "0 / 128k" — a default, never measured, for a 262,144 model | chat routed to the Studio | CONFIRMED (owner) | fixed af06267e7 · scheduled 3.0.35 | |
| Q-4 | 2026-09-25 | ux | chat top | NODES strip says "unmounted" under a green "Serving from Work's Mac Studio · ready" bar; the strip has no click | chat routed to the Studio | CONFIRMED (screenshot v334/composer-full.png) | open | NodesStrip.tsx:69/92/110 knows no remote route |
| Q-5 | 2026-09-25 | ux | composer | model chip reads "swarm" (a provider id); the served model is never named; changing where it runs is 2+ clicks into a provider modal | any swarm chat | CONFIRMED | open | ModelsBottomBar.tsx:152-162,207 |
| Q-6 | 2026-09-25 | ux | composer | "Coding · Agent" toggle changes nothing — LEANZERO_PERSONA is read nowhere; its tooltip promises an autonomous loop | clicking Agent | CONFIRMED (grep: usePersona.ts:10 only) | open | since 691295198 (2026-07-10) |
| Q-7 | 2026-09-25 | ux | composer | "Set up agent" opens a recipes/loops hub, not agent settings; its "build a recipe with the fleet" talks to LM Studio directly | Agent persona | PLAUSIBLE | open | AgentSetupWizard.tsx:25-34 |
| Q-8 | 2026-09-25 | ux | composer | the green bar stays up while everything works and never names the model | chat routed to the Studio, ready | CONFIRMED | open | ComposerReadiness.tsx:122,162 |
| Q-9 | 2026-09-25 | ux | composer | bug icon: tooltip "Generate diagnostics bundle", dialog "Report a Problem" — two names, one action, permanent in the main row | any session | CONFIRMED | open | ChatInput.tsx:1815-1831, Diagnostics.tsx:12 |
| Q-10 | 2026-09-25 | reliability | restore | restore gave up on an engine that became ready between two reads | reinstall both Macs, relaunch | CONFIRMED (both logs) | shipped 3.0.33 (e6476b9f7) | |
| Q-11 | 2026-09-25 | reliability | Studio engine (remote single) | the text engine's mlx_lm BatchGenerator sets the wired limit to the full recommended working set (generate.py:1545, built at rapid_mlx/scheduler.py:4877) and serves up to 8 concurrent requests with prefix-cache eviction — the reported IOGPU `completeMemory() prepare count underflow` panic recipe on the M3 Ultra 96 GB class (research D1/D3/D5) | 2+ concurrent unique 12–16k prompts, cache eviction churn, ~100 s | PLAUSIBLE (code + reports) | scheduled R2 | measure stock vs wired-limit-disabled arm; a panic reboots the workhorse (headscale, Link worker, MCP servers) |
