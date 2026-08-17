# Benchmark Product Contract v2 (2026-08-17)

The single source of truth for the goose-desktop → leanzero.net benchmark posting flow.
Both repos build against THIS file. Scorer comparability rail: sb-5.2.

## Identity (goose side)
`~/.config/goose/benchmark/identity.json`, created on first benchmark run:
```json
{ "installId": "<uuid4>", "handle": "<adjective>-<animal>-<4hex>" }
```
The handle is the public pseudonym (shown on bars, cards, leaderboard). installId is sent but
NEVER displayed — it groups a poster's runs server-side. Handles are generated from fixed
word lists + 4 hex chars so they are readable, unique enough, and stable per install.

## Screenshots (probe side — engine binary untouched)
product_probe.mjs captures PNG screenshots when env `BENCH_SHOTS_DIR` is set:
one per scenario (`loaded.png`, `synced.png`, `error.png`, `empty.png`, `mobile.png` at
375px) into `$BENCH_SHOTS_DIR/<epoch>/`. The render gate runs the probe during the engine's
repair/verify rounds, so successive epochs show the page AS THE SWARM REPAIRS IT; the final
epoch is the shipped quality. run_build.py sets BENCH_SHOTS_DIR=<workdir>/bench-shots.
The publisher picks: first epoch's `loaded.png` (before) + final epoch's full set (after).

## POST https://leanzero.net/api/benchmark-runs  (v2 payload)
Strict allowlist; unknown keys REJECT. All of v1 plus:
```json
{
  "label": "Your fleet · 3 nodes",          // v1, required
  "score": 0.8645, "tiers": {"A":0.9,"B":0.8,"C":0.9,"D":0.85},  // v1, required
  "nodes": 3, "hard": 0.5, "excellent": 0.4, "wallSecs": 6261,    // v1 optional
  "scorerVersion": "sb-5.2", "buildSha": "abc123", "notes": "",   // v1 optional
  "title": "My M4 fleet first run",          // NEW optional, user-typed, <=80 chars
  "poster": { "installId": "<uuid4>", "handle": "crimson-heron-7f3a" },  // NEW required
  "screenshots": [                            // NEW optional, <=5 entries
    { "name": "loaded-before", "caption": "First render, round 0", "b64": "<png base64>" }
  ],
  "runMeta": { "startedAt": "iso", "finishedAt": "iso",           // NEW required
               "engineEvents": 1234, "repairRounds": 2 }
}
```
Server rules (best-effort gating, decided 2026-08-17):
- screenshots: PNG magic-sniffed, <=1.5MB decoded each, <=5; uploaded as Sanity assets.
- consistency: 0<=score<=1, tiers all present in [0,1], engineEvents>=100 for swarm labels,
  wallSecs>=600 for nodes>=1 swarm entrants, finishedAt>startedAt. Fail => 422.
- rate limit per IP + per installId (5/day). Result: draft benchmarkRun (202, human-promoted).
- The browser submission form is REMOVED; the site states posting happens ONLY from the
  goose Local Edition desktop app.

## Sanity schema deltas (benchmarkRun)
+ posterHandle (string, indexed), posterInstallId (string, hidden in Studio list),
+ title (string), screenshots (array of image assets w/ caption),
+ runMeta {startedAt, finishedAt, engineEvents, repairRounds}.
Baseline docs: the Anthropic ladder ships as three benchmarkRun docs flagged `baseline:true`
(Opus 0.975 / Sonnet 0.969 / Haiku 0.786 — the FROZEN sb-5.2 v2 numbers from SCHEDULE.md;
never the desktop's stale sb-3 baselines.ts numbers, which get the same refresh).

## Comparison view (site)
/agentic-benchmarks gets a ComparisonBars section ABOVE the table: hand-rolled SVG bars,
solid saturated colors (NO left rails, NO faded washes, NO native controls — Mihai's UI law):
baseline bars (distinct accent + "Anthropic baseline" tag) + every promoted run's bar,
sorted by score; click ANY bar -> that run's card page /agentic-benchmarks/run/[id]:
score + tiers + screenshots gallery + poster handle + title/notes + scorer/build metadata.
Only same-scorer rows share the chart (sb-5.2); older rows stay in a collapsed legacy table.
