# Forge benchmark — app ↔ site integration contract (orchestrator-owned; 2026-10-02)

Owner (Mihai, 2026-10-02): "I hope we do have a way to select the benchmark type from the goose app and then on the
website we need to build the forge one as well but as a top right kind of toggle … pretty big and striking a toggle
between the SB and Forge benchmarks."

## Families
Two benchmark FAMILIES live side by side: `sb` (SB5.x … SB7.2 eras) and `forge` (forge-1.0 first). Each family has its
OWN current era. Nothing in one family changes the other's current/frozen state.

## Catalog (GET https://leanzero.net/api/benchmark-runs) — additive, backward compatible
Every `benchmarks[]` entry gains `family: "sb" | "forge"` (absent ⇒ `"sb"`) and `familyCurrent: boolean` (exactly one
true per family). CORRECTED 2026-10-03 (website agent): shipped apps' `benchmarkLaunchProblem` (benchTierPayload.ts)
refuses unless EXACTLY ONE entry has `current: true` and its scorer starts with `sb-` — so the legacy `current` flag stays
true ONLY on the SB current entry (listed first), and a forge entry is never `current: true`. New apps read
`familyCurrent` filtered by `family`, never `current`, for Forge.
Forge eras: scorerVersion `forge-1.0`, title "Forge 1.0 — Scope Ledger".

## Publish — as implemented on the site (branch forge-benchmark-family, fa1382a)
`scorerVersion: "forge-1.0"`; `tiers` exactly L K T R S B U V A E (per-tier means; E = excellence slice); `checksSummary`
required, every row a forge check with its tier letter; `admission` {ceiling, reasons, failedChecksByBand} + `rawScore`
required — the site recomputes the caps from the posted rows with score_forge.py's rules and refuses a mismatch; no
`composition`; excellence condition `share` → `gateConditions[].value`; `forge-1.0-rc` is refused (score_forge emits rc
until thresholds freeze). Site numbers come from scripts/sync-forge-public.py at the frozen goose commit.

## Publish (original intent) (POST /api/benchmark-runs, POST /api/benchmark-media) — same routes, same envelope
A forge verdict publishes with `scorerVersion: "forge-1.0"`; tier letters are score_forge.py's (L K T R S B U V A + E);
admission ladder 0.499 / 0.699 / 0.799 / 0.899 (score_forge.py is the source of truth — read it, don't copy numbers from
here). Media: the screenshot set + any clip score_forge produces. The route validates per FAMILY (sb rules stay
byte-identical).

## App
The Benchmark view gets a benchmark-type selector (SB | Forge) above the era picker; each type launches its own tier
(`run_build.py --sb72` vs `--forge`), runs, scores and publishes through the same flow, and the history groups runs by
family then era. The launch gate checks the CURRENT era OF THE SELECTED FAMILY against that family's bundled default.
