# Forge benchmark — app ↔ site integration contract (orchestrator-owned; 2026-10-02)

Owner (Mihai, 2026-10-02): "I hope we do have a way to select the benchmark type from the goose app and then on the
website we need to build the forge one as well but as a top right kind of toggle … pretty big and striking a toggle
between the SB and Forge benchmarks."

## Families
Two benchmark FAMILIES live side by side: `sb` (SB5.x … SB7.2 eras) and `forge` (forge-1.0 first). Each family has its
OWN current era. Nothing in one family changes the other's current/frozen state.

## Catalog (GET https://leanzero.net/api/benchmark-runs) — additive, backward compatible
Every `benchmarks[]` entry gains `family: "sb" | "forge"` (absent ⇒ `"sb"`, so shipped apps ≤ 3.0.88 keep working).
Exactly one `current: true` per family. Shipped SB apps compare the current SB entry to their bundled default — the site
must keep a rule that an old app (which ignores `family`) still finds ONE current entry with scorerVersion sb-7.2:
old apps read `benchmarks.find(b => b.current)` — keep the SB current entry FIRST in the array (verify in main.ts).
Forge eras: scorerVersion `forge-1.0`, title "Forge 1.0 — Scope Ledger".

## Publish (POST /api/benchmark-runs, POST /api/benchmark-media) — same routes, same envelope
A forge verdict publishes with `scorerVersion: "forge-1.0"`; tier letters are score_forge.py's (L K T R S B U V A + E);
admission ladder 0.499 / 0.699 / 0.799 / 0.899 (score_forge.py is the source of truth — read it, don't copy numbers from
here). Media: the screenshot set + any clip score_forge produces. The route validates per FAMILY (sb rules stay
byte-identical).

## App
The Benchmark view gets a benchmark-type selector (SB | Forge) above the era picker; each type launches its own tier
(`run_build.py --sb72` vs `--forge`), runs, scores and publishes through the same flow, and the history groups runs by
family then era. The launch gate checks the CURRENT era OF THE SELECTED FAMILY against that family's bundled default.
