# goose Local Edition — first stable release (DRAFT)

> Status: DRAFT. Nothing here is pushed, tagged, or published. Prepared 2026-08-17 for the
> coordinator to execute once credentials work. All git evidence below was gathered read-only.

## Proposed tag: `local-edition-v1.0.0`

Collision check, performed against all 190 tags present in this clone (which include upstream's):
upstream goose tags follow `vX.Y.Z` (e.g. `v1.0.0` … `v1.9.x`), `X.Y.Z-nightly.YYYYMMDD[.sha]`
(e.g. `1.11.0-nightly.20251023.8d9c19f2c2`), plus the singletons `stable`, `canary`, `ls`,
`pre-claude-code-import`. No existing tag contains `local`, `edition`, or an `le-` prefix.
The `local-edition-` prefix therefore cannot collide with any existing upstream tag and, because
upstream versions bare `vX.Y.Z`, cannot collide with future ones either. Subsequent releases
follow the same scheme (`local-edition-v1.0.1`, `local-edition-v1.1.0`, …).

The tag targets the release commit on branch `local-edition` (the release commit on `local-edition` at tag time).

## What this release contains

The first stable build of goose Local Edition — goose adapted to run a swarm of local models
(one model, several LM Studio machines) to build working software. On top of upstream goose
(fork point `a0aed81f3`, 2026-07-03; ~2,257 commits since):

- **The swarm engine** (`crates/goose-swarm`, `goose swarm` CLI): research scouts, parallel plan
  drafting with agreement/convergence, frozen per-module signature contracts, the dispatch DAG
  with speed-weighted scheduling and fat-task splitting, the judge and pre-review supervision,
  and the deterministic check → block → repair chain (spec-contract probes, vendor-truth POST
  checks, dom-id and css-coherence scans, the headless render gate, scheduled fix waves,
  ship-best-verified, early-close).
- **The desktop app with the Benchmark page**: Run → Swarm build → Scoring → Publish against the
  frozen sb-5.2 scorer (60 checks, tiers A/B/C/D + J/V/P + hard blocks); full check-by-check
  scoring detail; repair-epoch screenshots; opt-in pseudonymous publishing to
  leanzero.net/agentic-benchmarks with engine-truth model identifiers and per-node detail.
  Desktop version at draft time: 1.41.101.
- **The measurement harness** (`evals/swarm-bench/`): the sb-5.2 scorer with determinism and
  isolation controls, the run supervisor, the product contract shared with the website, and the
  FINDINGS ledger (F1–F873).

Honest headline numbers (sb-5.3, the rendered-means-seen scorer; the sb-5.2 era stays
published as a frozen historic board behind the site's scorer selector): the fleet's
published 3-node entry **0.93** (tiers 1.00/1.00/1.00/0.94, 190 min) against cloud
baselines re-scored on the same ruler — Opus 5 0.9344, Sonnet 5 0.4971, Haiku 4.5 0.4615.
The fleet lands 0.004 below Opus and roughly doubles Sonnet. Also in this release: the
sb-6 hard tier ("VendorSync Pro" — raw-WebGL 3D graded by analytic pixel recomputation,
HMAC webhooks, optimistic concurrency) fully implemented with its golden reference
passing the freeze gate at 100%; Opus calibrates at 0.7445 on it, threshold freeze
pending the remaining Bedrock baselines.

## Artifact plan

| artifact | source | status |
|---|---|---|
| `Goose-local-edition-v1.0.0-darwin-arm64.zip` | `ditto -c -k --sequesterRsrc --keepParent ui/desktop/out/Goose-darwin-arm64/Goose.app <zip>` | **verified**: `Goose.app` exists (518 MB unpacked, packaged 2026-08-17 20:39); a test zip of exactly this bundle measures **198 MB** |
| `Goose.dmg` (optional secondary) | `ui/desktop/out/make/Goose.dmg` | exists (198 MB) but was made 2026-08-17 **11:16** — ~9 h older than the current `out/Goose-darwin-arm64` package. If a DMG ships, re-run the make step against the final build first; do not ship the stale one. |
| `RELEASE-NOTES` | this file (top section) | draft |

Notes: the app bundle is unsigned/un-notarized as packaged here — first-run requires the usual
right-click-Open (or a signing pass before release, coordinator's call). CLI binaries are not
part of this first release; users build from source per the README.

---

## Merge assessment (appended per task)

### Question: can aaif's main fast-forward to local-edition?

**No — and the two "main"s must not be conflated.** This clone has three remotes:

| remote | URL | head relation to `local-edition` |
|---|---|---|
| `origin` | `leanzero-srl/goose-local-edition` | `origin/main` = `a0aed81f36` = **the merge-base itself** |
| `aaif` | `aaif-goose/goose` | diverged: 396 commits `local-edition` lacks (fetched read-only 2026-08-17, head `92c0fe902`) |
| `upstream` | `block/goose` | historical; project moved to aaif |

- **`aaif-goose/goose` main: fast-forward impossible.** `git merge-base local-edition <aaif main>`
  = `a0aed81f36` (2026-07-03). Current aaif main (`92c0fe902`, 2026-08-17) carries **396 commits
  local-edition lacks** while local-edition carries 2,257 aaif main lacks — true divergence, no
  ancestor relationship in either direction. (The stale cached `aaif/main` ref in this clone,
  last fetched 2026-07-15, showed 94; the fresh read-only fetch shows the real gap is 396.)
- **`leanzero-srl/goose-local-edition` main: fast-forward is clean.** `origin/main` sits exactly
  at the fork point `a0aed81f36`, which is an ancestor of `local-edition`
  (`git rev-list --count local-edition..origin/main` = 0). Pushing `local-edition` onto the fork's
  `main` is a pure fast-forward.

### Files where (aaif) main has commits local-edition lacks

The 396 aaif-main-only commits touch **1,082 files** (upstream releases 1.42.0/1.43.0+, provider
additions, dependency bumps, CI, docs). Of those, **94 files were also modified on
local-edition** — the conflict surface for any future upstream ingest. The load-bearing ones:

- Workspace: `Cargo.toml`, `Cargo.lock`, `Justfile`, `.gitignore`, `README.md`
- Core agent: `crates/goose/src/agents/agent.rs`, `prompt_manager.rs` (+ its snapshot),
  `reply_parts.rs`, `retry.rs`, `extension.rs`, `extension_manager.rs`,
  `platform_extensions/{mod,ext_manager,summon,developer/shell}.rs`
- ACP: `crates/goose/src/acp/server.rs`, `acp/server/custom_dispatch.rs`, `acp-schema.json`,
  `acp-meta.json`
- Providers: `crates/goose/src/providers/{init,mod,inventory/mod,chatgpt_codex}.rs`,
  `crates/goose-providers/src/{openai,api_client}.rs`,
  `crates/goose-provider-types/src/formats/openai.rs`
- Config/session: `crates/goose/src/config/{base,extensions,permission}.rs`,
  `crates/goose/src/session/session_manager.rs`, `crates/goose/src/model_config.rs`,
  `crates/goose/src/prompts/system.md`
- CLI: `crates/goose-cli/src/{cli,lib}.rs`, `crates/goose-cli/src/session/*`
- Desktop: `ui/desktop/src/{main,preload}.ts`, `App.tsx`, `components/BaseChat.tsx`,
  `ChatInput.tsx`, `Layout/NavigationPanel.tsx`, `settings/SettingsView.tsx`,
  `ui/desktop/package.json`, `ui/pnpm-lock.yaml`, `ui/sdk/src/generated/*`
- Tests: `crates/goose/tests/{agent,compaction}.rs`, `acp_common_tests`, `acp_fixtures`

Full lists are reproducible with:
`git fetch https://github.com/aaif-goose/goose.git main` then
`git log --name-only --pretty=format: local-edition..FETCH_HEAD | sort -u`.

### Verdict and recommendation

Release from the fork repo (`origin` = leanzero-srl/goose-local-edition), where main
fast-forwards cleanly. Do **not** merge aaif main as part of this release: it would inject ~6
weeks of upstream churn (396 commits, 94-file conflict surface) into the exact binary the
benchmark numbers were measured on, invalidating the frozen-build claim on release day.
Upstream ingest stays what it already is — a deliberate post-release boundary activity (the
goose-knob-turning flow), done as `git merge` (never rebase: 2,257 published commits), with
conflicts expected concentrated in the 94 files above.

### Exact command sequence (for the coordinator, once credentials work)

```bash
cd /Users/mihaiperdum/Projects/goose

# 0. Preconditions
git status                          # clean tree, on local-edition, at the release commit
git fetch origin                    # refresh; then re-verify the fast-forward still holds:
git merge-base --is-ancestor origin/main local-edition && echo FF-OK   # must print FF-OK

# 1. Push the branch and fast-forward the fork's main
git push origin local-edition
git push origin local-edition:main  # pure fast-forward (verified above; add --ff-only semantics
                                    # by NOT using force — a non-FF will be rejected, which is correct)

# 2. Tag and push the tag
git tag -a local-edition-v1.0.0 -m "goose Local Edition v1.0.0 — first stable release: local-model swarm engine + sb-5.2 benchmark product" local-edition
git push origin local-edition-v1.0.0

# 3. Package the artifact from the CURRENT build output
cd ui/desktop/out/Goose-darwin-arm64
ditto -c -k --sequesterRsrc --keepParent Goose.app \
  /tmp/Goose-local-edition-v1.0.0-darwin-arm64.zip   # ~198 MB, measured

# 4. Create the GitHub release on the FORK repo (never on aaif-goose/goose)
gh release create local-edition-v1.0.0 \
  --repo leanzero-srl/goose-local-edition \
  --title "goose Local Edition v1.0.0" \
  --notes-file RELEASE-NOTES-draft.md \
  /tmp/Goose-local-edition-v1.0.0-darwin-arm64.zip

# 5. (Separate, post-release) upstream ingest — NOT part of the release:
# git fetch https://github.com/aaif-goose/goose.git main
# git checkout -b upstream-ingest local-edition && git merge FETCH_HEAD   # expect ~94 conflict files
```
