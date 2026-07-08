Verified the load-bearing claims myself (greps above): FanInCard is referenced only by itself+test, `render_fan_in`/`node_chip` have zero external call sites, `isLocal` is read nowhere in production, and 9 of 10 `.local-edition` vars have zero consumers — `--color-block-teal` is consumed only at `main.css:469` (toast close-button focus outline). All three audits reconciled below.

---

# Goose Local Edition — Consolidated Verdict

## 1. HARD-RULE COMPLIANCE

| # | Rule | Verdict | Evidence |
|---|------|---------|----------|
| 1 | No left-accent rail | **PASS** | `FanInCard.tsx:51` full 4-side `border border-border-primary`; zero `border-l*`/`border-left` in components or `.local-edition`. Test-locked `FanInCard.test.tsx:39-40`. |
| 2 | No faded/washed tints | **PASS** | Solid full-hex tokens `main.css:243-257`; active selector state is solid `bg-background-inverse` (`EditionSelector.tsx:34`), not a tint. Zero `color-mix`/`rgba`/`opacity`. |
| 3 | No native browser UI | **PASS** | Edition switch is a custom 2-Button segmented control (`EditionSelector.tsx:46-66`); no `<select>`/`confirm`/`alert`/`prompt`. |
| 4 | Sharp/paper, not rounded | **PASS (scoped)** | The LE surface (FanInCard) is sharp `borderRadius:3` (`FanInCard.tsx:52`). Caveat: `.local-edition` overrides no radius token, and the settings/selector chrome stays `rounded-lg`/`rounded-md` (`AppSettingsSection.tsx:476`) — intentional match to sibling Theme card, not a rule violation, but the LE skin does not enforce sharpness itself. Not a FAIL. |
| 5 | Node hues disjoint from status hues | **PASS** | Ramp `{#17c4c4,#2e8bff,#6a5cff,#b14cff,#ff3ea5,#ff5c7a}` ∩ status `{#2ecc71,#f5a623,#ff3b30}` = ∅. Enforced both stacks: `palette.rs:71-78`, `FanInCard.test.tsx:28-30`. (Perceptual-adjacency concern noted under improvements, not a rule breach.) |
| 6 | Reuse-not-clone (goose identity) | **PASS** | Goose glyphs `●✔✕` + `⬢`; Claude `⏺` appears only in negative assertions (`formation.rs:128`, `FanInCard.test.tsx:48`). Distinct dispatch→fan-in metaphor. |
| 7 | Not half-baked / intentional | **PASS (rule) / see §2** | Uses goose tokens, i18n-wired, persisted, test-covered. The *code that exists* is intentional. But it is wired to almost nothing live — that gap is captured as defects below, not a Rule-7 FAIL since nothing shipped is sloppy, just orphaned. |

**7/7 PASS.** No auditor produced a real FAIL. The design-critic's "half-baked" charge is about wiring/reach, not rule violations — filed as defects.

## 2. CONFIRMED DEFECTS (reproducible, ranked must-fix first)

**D1 — Signature is orphaned; the LE differentiator renders nowhere.** MUST-FIX.
- `FanInCard.tsx` — imported by nothing but its own test (grep confirmed). `formation.rs` `render_fan_in`/`node_chip` — zero call sites outside the module (grep confirmed).
- Fix: render `FanInCard` in the desktop chat when a run fans out to nodes; call `render_fan_in` in the CLI parallel-node path. *(Confidence: the render components are done and tested; the uncertain part is mapping the live per-node swarm event stream to `NodeLane[]` — I could not confirm from these files that a per-node event feed exists. Flagging as the lower-confidence piece that needs verification before/while wiring.)*

**D2 — Desktop edition toggle is a near-total no-op.** MUST-FIX.
- 9 of 10 `.local-edition` vars (`main.css:247-257`: `--color-node-1..6`, `--color-accent-local`, `--color-status-*`) have zero consumers (grep confirmed). `--color-block-teal` (`main.css:243`) is consumed only at `main.css:469` — a Toastify close-button focus outline. `isLocal` from `EditionContext` is read nowhere in production (grep confirmed).
- Fix: route `--color-accent-local` + node/status vars into real chrome (primary focus rings, active nav item, primary buttons) so `.local-edition` visibly repaints; consume `isLocal` for at least one always-on shell signal.

**D3 — No persistent LOCAL marker in the desktop shell.** MUST-FIX (pairs with D2).
- The badge is deferred/dev-gated; combined with D2 a user in LE gets no standing signal they switched editions.
- Fix: un-defer a small always-on LOCAL marker in the shell header/sidebar.

**D4 — Formation ramp triplicated with no source of truth.** SHOULD-FIX.
- Same 6 hues hardcoded at `palette.rs:39-46`, `FanInCard.tsx:16`, `main.css:247-252`. They match today; nothing prevents drift. `FanInCard` uses hardcoded hex, not the `--color-node-*` vars it is meant to embody.
- Fix: single TS/CSS source of truth; have `FanInCard` read `var(--color-node-*)`. (Rust side can keep its own const with a cross-check test.)

**D5 — `.local-edition` does not enforce the sharp-corner intent.** SHOULD-FIX.
- The scope overrides colors only, no radius token; sharpness rides on the one orphaned 3px card. Ties to Rule-4 caveat.
- Fix: override the radius token(s) under `.local-edition` so the whole skin squares when active.

**D6 — Fake `▾ fan-in` disclosure affordance + static CLI banner.** MINOR.
- `FanInCard.tsx:81` `▾ fan-in · N lane(s)` implies collapsibility that doesn't exist. `render_local_banner` (`output.rs:586-593`) is one static line that never reflects real device count/names.
- Fix: drop the `▾` glyph (or make it a real toggle); have the banner reflect actual nodes ("swarm ready · 3 nodes: m4-max, m3-ultra…").

Dropped as speculation: perceptual-adjacency of ramp nodes 4-6 to status-red is a *taste/robustness* observation (hex-disjointness is genuinely proven), not a reproducible defect — see §4.

## 3. HONEST VERIFICATION STATEMENT

**Verified (executed):** `vitest` ran green — 2 files, 4 tests. These are real `@testing-library/react` jsdom renders that mount `FanInCard` and `EditionContext`, read actual DOM output (chip text `⬢A/⬢B/⬢C`, inline `style.color`, `borderRadius === '3px'`, presence of `border ` and absence of `border-l*`, `✔●✕` present / `⏺` absent, `.local-edition` class add/remove on toggle, `setSetting('edition',…)` called). So "does it render the intended tree" is answered by executed renders, not just static reading.

**Verified (static audit + grep by me):** JSX/CSS structure, all 7 hard rules, hue-disjointness math, and the orphaned-wiring/dead-var claims in §2 (grep output above).

**NOT verified (stated plainly):** No live pixel screenshot of the running desktop app. Electron is not installed (`node_modules/electron` absent), `goosed` is not built (`target/{debug,release}/goosed` absent), `@playwright/test` binary absent, and this is a headless subagent with no `DISPLAY`/WindowServer session. A real screenshot needs, in order: `cargo build` → `pnpm install` with Electron download approved → `npx playwright install` → `ENABLE_PLAYWRIGHT=true pnpm run start-gui` on a machine with a live GUI session (e.g. the workhorse via a logged-in desktop, not headless SSH). None were satisfiable here. No screenshot was fabricated. Consequently, D1/D2's *visible* impact (or lack of it) is inferred from code, not seen running.

## 4. IMPROVEMENTS — DO NOW vs DEFER

**Do now (these are what make LE read as real vs. a no-op toggle):**
1. D1 — wire `FanInCard` + `render_fan_in` into live runs.
2. D2 — route the azure accent + node/status vars into real chrome so toggling repaints.
3. D3 — un-defer the always-on LOCAL shell marker.
4. D5 — enforce sharp corners under `.local-edition`.

**Defer (quality, not blocking the "is it real" problem):**
5. D4 — single source of truth for the ramp + have `FanInCard` consume the CSS vars (naturally falls out of doing D2 well).
6. D6 — real/removed `▾` affordance and a live-node CLI banner.
7. Re-space ramp for *perceptual* distance from status-red and strengthen the test from `hex ≠ hex` to a min-perceptual-distance assertion. Deferred because current hex-disjointness genuinely satisfies Rule 5; this is hardening, not a fix.

Bottom line: the design system honors all 7 hard rules everywhere it touches pixels — but today it touches almost none. D1-D3 convert LE from a well-tested library behind an invisible switch into an actual edition; everything else is polish.