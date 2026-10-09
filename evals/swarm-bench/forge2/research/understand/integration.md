# Forge 2.0 — launchable from the Goose Swarm app, publishable to leanzero.net, beside a frozen Forge 1.0

Read-only survey, 2026-10-09 19:49 EEST. goose `08ff3c587` (main), LeanZero-website `43c9ce8` (master).
Nothing was run except grep/sed reads, ONE public GET of the catalog, and a small python AST diff of the
site's Forge scorer snapshot against goose HEAD (`tools/snapshot_diff.py` next to this file). No scorer, test,
build, browser or cargo (CPU rule).

## 0. The answer in six lines

1. **The site is the real work.** It holds ONE Forge scorer snapshot (`src/data/forgeScorer.generated.ts`) and
   validates EVERY `forge-*` post against it; the sync script stamps the literal `'forge-1.0'` on whatever goose
   commit it reads. Per-era snapshots + per-era validation must be deployed BEFORE a forge-2.0 state doc exists.
2. **The app is one record per tier plus ~10 single-constant Forge assumptions** (`FORGE_BENCHMARK_TIER`,
   `FORGE_RELEASE`, `FORGE10` in the kit-status script, one tier table, three `'forge-1.0'` literals, the
   rescore whitelist, the clip-less rule). Two interlocking tests refuse most naming mistakes.
3. **The harness needs a FORGE20 tier**, a run flag, a per-VERSION release-manifest payload (today it is
   per-family and hard-wired to FORGE10), and bench file names the Forge payload regex excludes.
4. **The order is forced** by three facts: the app refuses a Forge launch unless the catalog's single forge
   `familyCurrent` equals its bundled era; the site accepts POSTs and clip uploads for ANY registered, non-frozen
   era (current or not); and the catalog proxy that breaks that loop for the first run only knows `sb-7.2`.
5. **1.0 entries do not vanish by themselves at the flip.** Stored docs are never re-validated. They get
   mislabelled or overwritten through (a) re-syncing the single snapshot to a 2.0 commit, (b) the family-wide
   re-score replace match in the POST route, (c) swapping the one Forge brief/prompt/tier table to 2.0's.
6. **Decide 1.0's fate before the flip** (owner's open question in `forge2/NOW.md`: fix + re-score, or freeze
   with a note): once 1.0 is frozen every republish is a 409 and a 2.0 app cannot re-score a 1.0 build.

## 1. Live state (GET https://leanzero.net/api/benchmark-runs, fetchedAt 2026-10-09T16:44:21Z)

| scorerVersion | title | family | familyCurrent | legacy current | frozen | baselines |
|---|---|---|---|---|---|---|
| sb-7.2 | SB7.2 payments | sb | true | true | false | 8 |
| sb-7.1, sb-7.1-rc, sb-7.0-rc, sb-6.0, sb-5.3 | … | sb | false | false | true | 0/1/8/6/3 |
| forge-1.0 | Forge 1.0 — Scope Ledger | forge | **true** | false | **false** | 8 |

- forge-1.0 is open and current in its family. DESIGN §17.8 counts 31 published Forge 1.0 entries.
- The site's 1.0 snapshot is `sourceCommit d40aca358`. Diffed against goose HEAD (`snapshot_diff.py d40aca358
  HEAD`): tier order/weights, E weight, criticals, all four admission bands, the graded band, its roots,
  excellence lists and all 66 `@check (name, tier)` rows are IDENTICAL. The 1.0 validator is in sync today.
- Last app bundling forge-1.0: tag `v3.0.108` (goose-rel worktree at `e15f8ae08`, "forge manifest refrozen for
  3.0.108").

## 2. How a Forge run flows today (the seams that carry the era)

Renderer `BenchmarkView.tsx:1531` sends `FORGE_BENCHMARK_TIER` → main `benchmark-run` (`main.ts:4609`)
`benchmarkLaunchTier` → `benchmarkLaunchProblem(catalog, family)` → `readForgeKitStatus` (python
`forge_kit.status()` + `isolated_tiers.FORGE10.reasoning_effort`) → spawn `run_build.py --forge` with
`BENCH_SPEC=<payload>/forge/public/spec-build-forge.md` and `GOOSE_SWARM_RENDER_PROBE=bench/forge_probe.mjs`
→ `isolated_tiers.FORGE10` → `score_forge` module → verdict `scorerVersion: forge-1.0` (rc until calibrated)
→ publish: `forgePublishProblem` + `forgePublishBody` → clip upload `POST /api/benchmark-media`
(`x-benchmark-scorer`) → `POST /api/benchmark-runs` validated against `FORGE_SCORER` → state gate → Sanity
`benchmarkRun` doc → `/agentic-benchmarks/forge`.

## 3. Inventory — goose harness (`evals/swarm-bench`)

| where | single-version assumption | what forge-2.0 needs | if missed |
|---|---|---|---|
| `bench/isolated_tiers.py:60-68` | `FORGE10 = IsolatedTier('BENCH_FORGE10','forge-1.0','score_forge','forge/public/spec-build-forge.md',…, vendor='forge_site', kit=True, reasoning_effort='medium', own_scoring_site=True)`; `TIERS = (SB71, SB72, FORGE10)` | `FORGE20` with its own flag env, version `forge-2.0`, scorer module, spec, starter, scorer_files, public files, vendor module; add to `TIERS` (BY_VERSION and `active()` are generic) | nothing launches 2.0 |
| `bench/run_build.py:1239-1240, 1257-1258` | `--forge` sets `FORGE10.flag` | a distinct flag (e.g. `--forge2`) → `FORGE20.flag`. Everything else in run_build is tier-generic (`_regime()` imports `tier.scorer`/`tier.vendor`, kit via `_regime()[0]._kit()`, `own_scoring_site`, input-manifest kit exclusions) | — |
| `bench/score_forge.py:66-68, 72-73, 97-99, 2930, 3584-3595` | `SPEC`/`CONTRACT`/`PROBE_SCRIPT` paths, `forge-thresholds.json` + `CALIB_SHA256`, `VERSION = 'forge-1.0' if CALIBRATED else 'forge-1.0-rc'`, calibrate asserts `forge-1.0-rc`, `_kit()` → `forge_kit.ensure()` (the 1.0 kit) | a separate scorer module (recommended, §10 D1) emitting `forge-2.0` / `forge-2.0-rc`, `family: 'forge'` (the desktop projection keys on it), its own thresholds pin, `_kit()` ensuring the 2.0 kit | 1.0 stops being reproducible; site snapshot for 1.0 loses its source |
| `bench/forge_site.py:29, 32` | `SITE_JS = forge/site/site.cjs`; `TIER = 'forge-1.0'` written into the trace header | 2.0 vendor module (or tier-parameterised) writing `forge-2.0` | wrong tier in every 2.0 trace |
| `bench/forge_kit.py:37` | `KIT_SRC = …/forge/kit` module global; `status()`/`ensure()` take no tier | per-tier kit source (2.0 must carry the current CLI's lint rules, DESIGN §17.8 E). The cache key (`<lock_sha256[:16]>`) already separates kits | 2.0 runs against 1.0's kit |
| `bench/forge_probe.mjs:814` | media `scorerVersion: 'forge-1.0'` | 2.0 probe; harmless because `score_forge.py:3190` overwrites it with `VERSION`, keep that | — |
| `bench/bench_budget.py:42` | `CALL_BUDGET = 150` shared by SB7.2 and Forge, read by the desktop | if 2.0 ("a ton of calls") needs a different model-call budget it must become a tier field; changing the global moves SB7.2 and its stated budget | SB7.2 comparability breaks |
| `bench/release_manifest.py:47-57, 99-110, 125` | `payload(root, family)` dispatches on the manifest's `family` only; `forge_payload()` hard-reads `isolated_tiers.FORGE10` and `FORGE_TREES = ('forge/public','forge/starter','forge/kit','forge/site')` | dispatch on the manifest's `scorerVersion` via `BY_VERSION`; 2.0's own trees | a `family: "forge"` 2.0 manifest silently pins **1.0's** payload |
| `bench/release_manifest.py:47` `FORGE_BENCH` | `^(score_forge|forge_[a-z_]+|test_score_forge)\.(py|mjs)$|^forge-thresholds\.json$` excludes Forge files from the SB7.x payload | widen it. Tested: `score_forge2.py`, `forge2_probe.mjs`, `forge_probe_v2.mjs`, `forge2_site.py`, `forge2-thresholds.json`, `forge_oracle2.py` ALL leak into the SB7.2 payload pins | caught: `test_score_forge.py:1622-1631 test_release_payload_is_per_family` fails on any payments file containing "forge" |
| `forge/release-manifest.json` | the only Forge manifest (`scorerVersion forge-1.0`, `family forge`) | a 2.0 manifest in 2.0's dir. Once 2.0 is the bundled era, STOP refreezing 1.0's manifest: like `sb7.1/`, it stays as v3.0.108's receipt (refreezing it would pin bytes 1.0 never ran with) | — |
| `forge/INTEGRATION.md` | publish contract written for forge-1.0 only | per-era contract | docs drift |
| tests: `test_score_forge.py:1514, 1551-1631`, `test_bench_rescore.py` | pin forge-1.0/FORGE10 | 2.0 twins | — |
| `bench/bench_rescore.py:94, 150-154` | generic (`BY_VERSION[receipt.scorerVersion]`, `-rc` stripped, kit lock must match receipt) | nothing | — |

## 4. Inventory — desktop app (`ui/desktop`)

| where | single-version assumption | what forge-2.0 needs | if missed |
|---|---|---|---|
| `components/benchmark/baselines.ts:28-52` | `BenchTier` union, `TIERS`, `TIER_SCORER` have only `forge-1.0` | add `forge-2.0` → `forge-2.0`; KEEP `forge-1.0` for history labels and old sessions | type errors (loud) |
| `baselines.ts:119-139` | ONE `FORGE_TIER_ORDER` (L K T R S B U V A E) and ONE `FORGE_TIERS` meaning table | per-era tables if 2.0 changes letters or meanings (recordedTierColumns, ScoringDetail, publish use them) | 1.0 sessions shown with 2.0 tier names, or 2.0 letters hidden |
| `benchTierPayload.ts:34-68` | `BENCH_SPEC_FILE` / `BENCH_RENDER_PROBE` / `BENCH_RUN_FLAG` have only 1.0's Forge row | 2.0 rows, each DISTINCT (spec path, probe file, flag) | caught: `benchTierPayload.test.ts:21-35` (every tier mapped, distinct), `:58-66` flag regex `/^--(?:sb\d+|forge)$/` must be widened for `--forge2` |
| `benchTierPayload.ts:77, 87-90` | `FORGE_BENCHMARK_TIER = 'forge-1.0'`; `BENCH_FAMILY_NAME.forge = eraLabel(…, 'Scope Ledger')` | `'forge-2.0'` and 2.0's product name | the app keeps launching 1.0 |
| `benchTierPayload.ts:106-114` | `CloudBenchmarkTier = 'sb-7'|'sb-7.1'|'sb-7.2'|'forge-1.0'` | add `forge-2.0` (preload.ts:182/606 import the type). `benchmarkLaunchTier` then refuses 1.0 ("Only the latest stable…") | — |
| `benchTierPayload.ts:122-150` `benchmarkLaunchProblem` | generic per family. NOTE: when the app is NEWER than the site's current it still says "Update Goose to run the latest stable benchmark (Forge 1.0). This app bundles Forge 2.0." (the SB analog is pinned in the test) | optional copy fix for the release→flip window | public 2.0-app users are told to update an app that is already newer |
| `scripts/copy-bench-release-manifest.cjs` `FORGE_RELEASE` | `{dir:'forge', scorerVersion:'forge-1.0', spec, probe}` — one Forge release verified at packaging | 2.0's dir/scorer/spec/probe | caught: `benchReleaseManifest.test.ts` pins FORGE_RELEASE to `defaultBenchmarkScorer('forge')` (but also the literal `dir: 'forge'`) |
| `forge.config.ts:13-105` `mirrorSwarmBenchPayload` | copies `forge/{public,starter,kit,site}` only; then `copyManifest(src, dest, FORGE_RELEASE)` | copy 2.0's trees (and 1.0's only if 1.0 is still shipped, §10 D2) | caught at packaging: "Packaged benchmark is missing a release manifest file" |
| `benchPayloadPackaging.test.ts:45, 52-53, 69-73, 96-99` | literal `FORGE_TREES` under `forge/`, `{dir:'forge'}`, 7 filters, `BENCH_SPEC_FILE[forge tier].startsWith('forge/public/')`, 4 forge filters | update with the 2.0 layout | test fails (loud) |
| `benchForgeKit.ts:13-17` `FORGE_KIT_STATUS_SCRIPT` | `forge_kit.status()` (1.0 kit), `bench_budget.CALL_BUDGET`, `isolated_tiers.FORGE10.reasoning_effort` | read the BUNDLED forge tier (its kit, budget, effort) | the view shows 1.0's kit readiness and policy for a 2.0 run; `benchForge.test.ts:216` pins the FORGE10 string |
| `benchForgePublish.ts:48, 53-56, 84-91` | rc refusal text names "the forge-1.0 freeze"; `forgePublishTiers` = exactly `FORGE_TIER_ORDER`; `checksSummary` DROPS rows whose tier is not in `FORGE_TIER_ORDER` | era's name; era's letters | a 2.0 row with a new letter is silently dropped, then the site refuses the post with an admission mismatch (misleading reason) |
| `benchRunResults.ts:155, 171` `clipAbsence` | `CLIPLESS_ERAS = {'sb-7.1','sb-7.2','forge-1.0'}`; `if (scorerVersion === 'forge-1.0')` (mirror of the site's rule) | 2.0 in the set and its own row rule (must mirror the site, §5) | a 2.0 app that rendered no surface cannot publish clip-less; it is refused "requires its graded browser clip" |
| `benchRescore.ts:11-14` `RESCORABLE` | `[…payments, TIER_SCORER['forge-1.0']]` | add 2.0 | 2.0 scoring retry / re-score refused |
| `main.ts:5086` | `sessionTier = forge ? FORGE_BENCHMARK_TIER : isolatedPaymentsTier(…)` — ANY Forge session is re-graded with the BUNDLED tier's probe/node | map the session's own scorer to its tier | latent today (the gate refuses history first); a real bug the day 1.0 is re-scored from a 2.0-era app |
| `main.ts:4609-4626, 4750-4754, 5046-5100, 5236-5240, 5439-5446` | family-generic (familyOfScorer, `-rc` strip, kit checks keyed to "forge") | kit checks must target the bundled forge tier's kit | — |
| `components/benchmark/BenchmarkView.tsx:1941` | Forge description copy is 1.0's ("ten module types plus the Realtime API … seeded Jira site …") | 2.0's | wrong product described |
| `components/benchmark/ForgeKitSetup.tsx:81` | header literal `forge-1.0 runtime` | the bundled era's | cosmetic |
| `benchShots.ts:44-82` | Forge shot pick table keys on 1.0's surfaces (widget, sprint action, edit, noconfig, not-started) | entries for 2.0's surfaces (front-facing Custom UI, UI Kit admin panel, …) | 2.0 publishes with only the contact sheet or no screenshots |
| tests pinning `forge-1.0` | `benchRescore.test.ts` (15), `BenchmarkView.forge.test.tsx` (17), `BenchmarkSection.test.tsx` (9), `benchRunResults.test.ts` (5), `BenchmarkView.rescore.test.tsx` (5), `benchTray.test.ts` (2), `ScoringDetail.test.tsx` (2), plus the four above; `forge-alt.fixture.json` is a real 1.0 verdict | a real `score_forge2` verdict fixture and 2.0 twins; keep the 1.0 cases as history | — |
| generic, no change | `benchmarkLaunchProblem` family filter + `FAMILY_ERA.forge = /^forge-\d+(?:\.\d+)*$/`, `isFamilyCurrent`, `eraDisplayName` ("Forge 2.0"), `BenchmarkSection.deriveEras`, view section sort (current first, then numeric desc), `benchScoreProjection` (keys on `family:'forge'`), `bench_rescore` | — | — |

## 5. Inventory — leanzero.net (`~/Projects/LeanZero-website`)

| where | single-version assumption | what forge-2.0 needs | if missed |
|---|---|---|---|
| `scripts/sync-forge-public.py:26-28, 149, 167-174` | reads `bench/score_forge.py`, `bench/forge-thresholds.json`, `forge/public/*`; writes `'scorerVersion': 'forge-1.0'` LITERALLY; writes ONE `forgeScorer.generated.ts` and ONE `forge-public-prompt.json` | parameterise by era (scorer path, thresholds, public files, version) and write per-era outputs; 1.0's stays pinned at its commit (`d40aca358`, facts = HEAD) | re-running it at a 2.0 commit produces a file that CLAIMS forge-1.0 with 2.0's rules — the silent swap behind §9.1 |
| `src/data/forgeScorer.generated.ts`, `src/data/forge-public-prompt.json` | single snapshot / single prompt | per-era (a map keyed by scorerVersion, or two modules) | — |
| `src/lib/forgeBenchmark.ts:10-13, 22-33, 38-39, 53-56, 94-131, 134-136` | `ForgeTier` union, `FORGE_TIER_LETTERS`, `FORGE_SCORER_VERSION`, `FORGE_TITLE "Forge 1.0 — Scope Ledger"`, hand-written `FORGE_TIER_META` (1.0's meanings), module-level `BANDS`/`CHECK_TIER`/`GRADED`/`BAND_ROOTS`; `validateForgeAdmission`, `isForgeCheck`, `forgeInnerFromTiers` take NO version | every function takes the posted scorerVersion and reads that era's snapshot; per-era tier meta | — |
| `src/app/api/benchmark-runs/route.ts:225, 265-275, 432-435, 455-466` | every `forge-*` post is validated with the ONE snapshot (letters, registry, admission, inner) | per-era; and REFUSE a forge era with no snapshot (422 "no scorer snapshot for forge-X") instead of validating it with 1.0's | today a forge-2.0 post is judged by 1.0's rules (fallback-class silent substitution) |
| `route.ts:618-660` state gate | presence + family + `frozen` (409). NOT gated on current | nothing — this is what lets the first 2.0 run publish before the flip | — |
| `route.ts:716-724` re-score replace match | `string::startsWith(scorerVersion, "forge-")` + same installId + model + (startedAt or buildId) → `createOrReplace` the OLD doc | scope it to the era (`scorerEraKey` equality), or refuse when the existing doc's era is frozen/different | a 2.0 post of a 1.0 build overwrites the 1.0 doc (§9.2) |
| `src/lib/benchmark-recording.ts:28-42` | Forge clip-less rule: 1.0's four `v_*` rows + detail string, no U/V row > 0 | per-era rule (the app mirrors it in `clipAbsence`) | 2.0 clip-less posts refused |
| `src/lib/benchmark-media.ts:5-6`, `api/benchmark-media/route.ts:20, 26-27` | caption ≤ 400 chars, clip ≤ 4 MiB; any registered non-frozen `forge-*` accepted | 2.0's probe caption must stay ≤ 400 (1.0's is 268) and its clip ≤ 4 MiB with more surfaces | upload 400/413 |
| `src/lib/benchmark.ts:97-104` | `printsSb6Formula` true for every forge era; `innerTierCountWord` "nine" for every forge era | per-era if 2.0 changes E weight or inner-tier count | — |
| `components/ScoreStory.tsx:117, 256-277, 350` | `FORGE_TIER_META`; formula printed with literal 0.88/0.12 for every Forge run (`ForgeAdmissionStory` → `ScoreFormulaSb6`) | per-era meta and weights | 2.0 run pages print a wrong formula if weights move |
| `app/(pages)/agentic-benchmarks/run/[id]/page.tsx:121-128, 596` | tier bars from `FORGE_TIER_META`; "How the benchmark is scored" → `/agentic-benchmarks/forge#forge-scoring` | per-era meta; link to THE RUN'S era section | after the brief flips, every 1.0 run page links to 2.0's rules |
| `BoardPage.tsx:17, 158-160` | `<ForgeBrief prompt={forgePrompt} />` — one brief, one prompt (SB already picks its brief by the current state doc, :164-168) | pick by the current forge state; keep 1.0's brief reachable as a frozen section | 1.0's task text and rules leave the site |
| `components/ForgeBrief.tsx:40-45, 52-60, 175-259` | 1.0 narrative ("The task is the Scope Ledger…"), `CAPS` wording, "the Forge 1.0 scorer is frozen", numbers from the single `FORGE_SCORER` | a 2.0 brief; 1.0's frozen | — |
| `components/RunsLeaderboard.tsx:341-393, 397-420, 524-528` | `ForgeNumbersView` columns = `FORGE_TIER_META`; `ForgeEmptyState` text is literally "Forge 1.0 has no published results yet." and "Goose 3.0.89 or later, the first version that runs Forge" whatever `version` it is given | per-era columns; state the era and its first app version | the registered-but-empty 2.0 tab tells the public "Forge 1.0 has no published results yet" |
| `src/lib/seo/benchmarkSeo.ts:129-133, 193-210, 215-235` | `BOARD_SEO.forge.description` "Forge 1.0: …", keyword `forge-1.0`; JSON-LD tier names from `FORGE_TIER_META`; `boardItemListJsonLd` ranks ALL forge eras in one list | current-era copy; per-era names; rank within the current era | stale/mixed SEO |
| `src/lib/forgeSkew.ts` | `FEATURE_CHECKS` are 1.0's feature rows | per-era or retired | `register-forge` refuses |
| `sanity/schemas/benchmarkRun.ts:135-142, 295-300`; `src/lib/sanity/queries.ts:884` | `forgeTiers` fixed fields L…E; checkRow tier list; TS type | add any new 2.0 letters (Studio validation only; API writes are not blocked) | Studio flags 2.0 docs |
| `scripts/register-forge.mjs`, `scripts/lib/forge-registration.mjs` | `FORGE10` constant; `planAdd` creates `current: true` and REFUSES when any forge state is current; deploy probe regex `^Unknown forge-1\.0 tier\(s\): C, D$` | a new `register-forge2` (copy of `sb72-registration` shape): `probe`, `add` (current:false, frozen:false, sourceCommit, releaseManifestSha256), `flip --promote-run <id> [--freeze-forge10]` (one revision-guarded transaction), `unflip`, `verify` | cannot register 2.0 non-current at all |
| deploy probe (inference, not run) | a forge-2.0 probe body with SB tiers gets "Unknown forge-2.0 tier(s): C, D" from BOTH today's 1.0-only route and a 2.0-aware one if 2.0 keeps A and B as letters | a deployed marker instead, the `lz-sb72-release` pattern: e.g. `<meta name="lz-forge-eras" content="forge-1.0:<sha>;forge-2.0:<sha>">` read by the script | registration proceeds against code that cannot validate 2.0 |
| generic, no change | `benchmarkState` schema (family by `^forge-`), GET catalog (family, familyCurrent, legacy current SB-only, order), `scorerEraKey`/`isForgeScorer`/`scorerDisplayName` ("Forge 2.0"), `groupRunsByScorer`, `FamilyToggle` (family-level links) | — | — |

## 6. Ops tooling outside the repos (`~/goose-builds/loop-state`)

- `tools/catalog_proxy.py` (26 lines): `catalog()` sets only the legacy `current = (scorerVersion == 'sb-7.2')`
  and never touches `familyCurrent`. For the 2.0 promote-run it must also set `familyCurrent` true on forge-2.0
  and false on forge-1.0; POSTs (runs and `/api/benchmark-media`) are forwarded verbatim, so the REAL state gate
  still decides. The chain launches the app with `LEANZERO_BENCH_PUBLISH_URL=http://127.0.0.1:8973`
  (`sb72_chain.py:157`; one URL serves both the catalog GET and the POST, `main.ts:3281-3282`).
- `sb72_chain.py`: Forge queue entries are family-level (`family: 'forge'`, 55 of 110 entries, currently
  stopped/blocked), and the history finder keys on the "Forge <n> … CURRENT" header — era-agnostic. Unblocking
  them under a 2.0 app runs 2.0. Under a 2.0 app WITHOUT the patched proxy before the flip, every one fails
  the launch gate.
- `rescore_round.py`: a hard list of forge-1.0 `brun-*` ids (the 1.0 re-score tool) — run it only with the
  1.0 toolchain (v3.0.108), never through a 2.0 app.

## 7. Order of operations

0. **Settle Forge 1.0 first.** If "fix + re-score": do it on the 1.0 toolchain (v3.0.108 or a 1.0 fix release)
   while 1.0 is open. If a fix changes anything the site recomputes (band membership, the check registry, the
   graded-band roots, the weights — e.g. if §17.8 E is implemented as a NEW deployable-band check rather than
   by failing `l_deployable`), re-sync the site's 1.0 snapshot to the fixed commit BEFORE republishing or every
   republish is a 422. Republishes replace the same docs in place (family match + same build). If "freeze with
   a note": there is no note field today (the historic banner is generic) — a `benchmarkState.note` or a 1.0
   brief section carries it.
1. **goose, built and gated but not released:** FORGE20 tier + flag; 2.0 scorer/probe/site/kit modules named so
   the widened `FORGE_BENCH` excludes them; `release_manifest.py` per version; 2.0 manifest; calibrated so the
   scorer emits `forge-2.0` (the app and the site both refuse `-rc`). Desktop changes of §4. Refreeze the SB7.2
   manifest if shared bench files moved; leave `forge/release-manifest.json` as the v3.0.108 receipt.
2. **Site validators first** (§5): per-era snapshots and validation, refusal of an unknown forge era, era-scoped
   replace match, per-era brief/prompt/tier meta/empty state/formula, the deploy marker. Merge, wait for
   Amplify, verify the marker on the live board.
3. **Register forge-2.0 NON-current, open** (`register-forge2 add --live`): readback; catalog check — legacy
   `current` still exactly one SB entry and first; exactly one forge `familyCurrent` (forge-1.0). Installed apps
   (pre-family ≤3.0.88 and family-aware 3.0.89–3.0.108) are unaffected. Exposure: the public board immediately
   offers "Forge 2.0 · open" (empty), and installed apps list "Forge 2.0 (history)" — register as late as
   step 5 needs if that matters.
4. **Release the app** (notarized, published, installed on the line machine). Until the flip, public 2.0-app
   users get the Forge launch refusal worded "Update Goose…"; Gauntlet is unaffected.
5. **First real run:** patch the proxy (§6), launch one paid model from the Benchmark view (gate 3), score,
   publish through the proxy → the 201 and its `brun-*` id. Check its run page and board row render 2.0's tiers.
6. **Flip, one revision-guarded transaction:** forge-2.0 `current: true`; forge-1.0 `current: false` +
   `frozen: true`. Two separate writes leave a window where the catalog has two or zero forge currents and every
   installed app refuses Forge. Readback → revalidate → verify catalog + board + a 1.0 run page (CDN lag
   ~1–5 min: `s-maxage=60` plus CloudFront). Keep `unflip`. Apps ≤ v3.0.108 now say "Update Goose…(Forge 2.0)".
7. Unblock the Forge queue; rerunning every model on 2.0 (`forge2/NOW.md` phase 8) creates NEW docs — new
   startedAt/buildId never match a 1.0 doc.

## 8. How 1.0 and 2.0 show as eras

- **Board** (`/agentic-benchmarks/forge`; the Gauntlet | Forge toggle needs no change): `RunsLeaderboard`
  groups forge runs + forge state docs into eras, newest first. Era dropdown: "Forge 2.0 · latest" (default =
  the state doc marked current), "Forge 1.0 · frozen". Picking 1.0 shows the amber "Historic board · Forge 1.0 ·
  frozen record — not comparable with Forge 2.0" bar. Every era's rows are server-rendered as hidden panels,
  so 1.0 run links stay crawlable. Bars view is era-agnostic; the Numbers view needs per-era tier columns.
  Below the board, the brief/prompt must follow the current era and keep 1.0's reachable.
- **Before the flip** the 2.0 option reads "open" and renders `ForgeEmptyState` with its hardcoded
  "Forge 1.0 has no published results yet." — fix before step 3.
- **App:** the sidebar and the Benchmark view group by family, current era first: Forge 2.0 (Run enabled),
  Forge 1.0 "(frozen)" with its sessions readable; a 1.0 session is "history only" (no run, no re-score) and its
  Publish says "Benchmark frozen — submissions closed".

## 9. What would break or hide 1.0 entries (most severe first)

1. **Re-syncing the single site snapshot to a 2.0 commit** — the script still labels it `forge-1.0`. Every later
   1.0 POST (re-score republish) is judged by 2.0's rules; the board's 1.0 brief, prompt, weights, caps and tier
   names become 2.0's; if 2.0's letters differ, 1.0 tiers drop out of the Numbers table and run pages
   (`FORGE_TIER_META` filters them). Silent. Stored docs themselves are never re-validated.
2. **The family-wide replace match** (`route.ts:716-724`): a forge-2.0 post that shares installId + model +
   startedAt/buildId with a 1.0 doc `createOrReplace`s it, moving it to 2.0. Freezing 1.0 does not protect it
   (the gate checks the POSTED era). Only reachable by re-scoring a 1.0 build under 2.0 (the app refuses; a
   script would not). The match is family-wide on purpose (a corrected scorer label replaces its build), so
   the fix is era-scoped matching or refusing a different/frozen era.
3. **Freezing before step 0:** republishes 409; unpublished local 1.0 results become unpublishable; a 2.0 app
   cannot re-score or retry-score a 1.0 build (history-only gate, `RESCORABLE`, `sessionTier`).
4. **A non-atomic flip:** transient two/zero forge currents → every installed app refuses Forge.
5. **Deleting `bstate-forge-1-0`:** 1.0 POSTs 422 and the catalog entry vanishes (rows still render from run
   docs). `register-forge remove` already refuses when 1.0 runs exist — freeze, never delete.
6. **Swapping the one brief/prompt** without a 1.0 section: 1.0's task text leaves the site and every 1.0 run
   page's "How the benchmark is scored" link points at 2.0's rules.
7. Low: CDN lag after the flip; the JSON-LD ItemList ranks both eras together.

## 10. Decisions (recommendation, confidence)

- **D1 — 2.0 in its own tree and modules** (e.g. `forge2/{public,starter,kit,site}`, `score_forge2.py`, a 2.0
  probe/site/kit), not edits to `forge/` and `score_forge.py` in place. Keeps 1.0 reproducible at HEAD for a
  CLI re-score, keeps the site's 1.0 snapshot source meaningful, and keeps the two kits apart (2.0 needs the
  CLI 14.x lint rule). Confidence HIGH on the mechanics; the cost is duplicated harness code.
- **D2 — keep `forge-1.0` in `TIERS`/`TIER_SCORER`** for history labels; shipping 1.0's trees in the 2.0 app is
  optional (only the bundled era's manifest is verified). If dropped, adapt the packaging test's
  `forge/public/` assertion.
- **D3 — per-era tier tables on both sides** (desktop FORGE_TIERS/ORDER; site FORGE_TIER_META, ForgeTier,
  Sanity forgeTiers). Required if 2.0 changes letters or meanings; harmless otherwise.
- **D4 — era-scoped replace match** on the site. MEDIUM confidence it fits the owner's re-score semantics;
  the alternative is refusing replacement when the existing doc's era is frozen or a different major.
- **D5 — per-tier call budget** if 2.0 needs more than 150 calls.
- **D6 — family-aware catalog proxy** for the first run (precedent), not a new site "preview" flag.
- **D7 (optional)** — "leanzero.net has not opened Forge 2.0 yet" when the app is newer than the site.

## 11. Verified vs not verified

- Verified by reading at the cited lines: every literal and assumption above; the live catalog (one GET); the
  site's 1.0 snapshot equals goose HEAD on every validator fact (AST diff); the `FORGE_BENCH` leak (regex run
  on candidate names); the proxy's behaviour (read).
- Inference, not run: the deploy-probe indistinguishability (§5) depends on 2.0's letter set; whether 2.0
  changes tier letters, E weight or budget is unknown until DESIGN — those rows are conditional.
- Not run (CPU rule): any vitest/pytest/node test, packaging, scorer.

## 12. For the goose-benchmark-iteration skill (not edited — read-only brief)

The catalog proxy is SB-only (no familyCurrent); the site validates every forge-* with one snapshot and
`sync-forge-public.py` hardcodes `forge-1.0`; the POST replace match is family-wide; `release_manifest.py`
dispatches by family and `FORGE_BENCH` leaks digit-named Forge files (guarded by
`test_release_payload_is_per_family`); v3.0.108 is the last forge-1.0 app.
