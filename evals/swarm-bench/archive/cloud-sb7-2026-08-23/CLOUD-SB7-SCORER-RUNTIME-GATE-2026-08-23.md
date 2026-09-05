# Cloud SB7 scorer runtime gate — 2026-08-23

Verdict: **BLOCKED**. The real sandboxed scorer completed and sealed valid score evidence, but produced no screenshot files. A cloud entrant cannot proceed through the pinned publication contract without a nonempty screenshot plan.

## Scope and isolation

- Harness source: `a91c9187bbd5c2bba3b6e16f355ad30e7df2d876`
- Isolated canonical fixture root: `/private/var/folders/41/670wc_gs2y93f8rs0330cc780000gn/T/cloud-sb7-scorer-gate.9zr3VXFQR1/campaign-canonical`
- Target entrant: `glm-5.3`, populated only from the clean `golden-sb7` source deliverables (`app`, `web`, `README.md`, and `DECISIONS.md`)
- No model/provider call, website/Sanity write, publisher invocation, forensic campaign-root reuse, or credential disclosure occurred.
- Five offline smoke proofs passed before monitor/scorer admission. The real monitor lease was authenticated and remained valid throughout scoring.

## Frozen runtime

- Instrument set SHA-256: `141533b70f655362d6d9d2a9c7d1b3fa9edfcffe8910ce80e90f725a54cd2ce0`
- Scorer runtime identity: `96df569a228b00cfd2084ad3764ecc80d87eaf5705e4f79cd44c139388a9a885`
- Node: `v26.5.0` at `/opt/homebrew/Cellar/node/26.5.0/bin/node`
- Playwright / Playwright Core: `1.57.0` / `1.57.0`
- Chromium / headless-shell revision: `1200`; ffmpeg revision: `1011`
- Pinned website commit observed read-only: `694927b0b610c93f0c34dee01004c6def367e670`

## Real scorer result

- `cloud_sb7.score_one` returned true after 900.162 seconds and committed entrant state `SCORED`.
- Verdict: score `0.2269`, 91 checks, seed `5a05d7631d9276e3`, scorer `sb-7.0-rc`.
- Raw tree before/after: `3686466666ec11ed4445bb97dd58a6a830ac466fc20f63e4330009cb51323027`.
- Verdict SHA-256: `4b4d46e18500c3b467128e06f0f208522ca2b3dac28d05dddd4149010672c36b`.
- Score log SHA-256: `15e13a01e73d2ca85b5f14ebf320f2594f3c658d72eff2dbff37b6dc4574be2a`.
- Listener snapshot SHA-256: `6f7b2186b6422f712d5c97d72d2fa107afc5297dc0ee7072c657833bc9577cbd`.
- Score tree SHA-256: `e705fda4c23068de745dd57e6548e93a7aa037f96d00e31863796f2248267cc5`.
- Post-cleanup score evidence seal SHA-256: `148b6577dc69ff03d5006b4869f9c31006b0c4044a55f2e4813eb5aaf46d895c`.
- `verdict_failure`, `score_evidence_seal_failure`, listener-isolation failure, lineage failure, and instrument mismatch all returned `None`.
- Scorer ownership was cleared after exit: PID, PGID, identity, marker, and process inventory were empty. Ports 9120–9124 were all free. No benchmark-controlled artifact contained a fixture credential value.

## Blocking evidence

- `scores/glm-5.3/attempt-1/tree/sb7-shots` was a regular, non-symbolic directory but contained **zero files**.
- The scorer log recorded 23 `PROBE-UNAVAILABLE` checks, including all 15 T-tier checks, because flow/viz sections were lost to probe hard caps. This is diagnostic context; the current verdict schema still accepts the verdict.
- `score_one` and `score_evidence_seal_failure` accept the empty screenshot directory and commit `SCORED`. The downstream publication contract cannot complete: `publication_stage` expects the scorer screenshot tree and `publisher_plan_from_log` rejects a dry run with no screenshot plan.

The scorer/runtime containment path passes. The scorer-to-publication artifact contract does not: screenshot evidence must be made nonempty or the run must fail closed before `SCORED`.

## Cleanup

`cloud_sb7.stop` returned `0` for both isolated fixture generations. It terminated only the recorded local manager/monitor groups:

- Canonical fixture manager PID/PGID `76407`, monitor PID/PGID `76419`, and launcher PID/PGID `76411` were dead afterward; all three groups were empty.
- The earlier fail-closed alias fixture manager PID/PGID `70815` and launcher PID/PGID `72503` were dead afterward; both groups were empty.
- Both stop receipts had empty `stop_failures`; all recorded runtime ownership fields were cleared and ports 9120–9124 were free.
- No process command referenced the isolated fixture root after stop.

The complete isolated temporary base was then moved recoverably to the user's Trash; no forensic campaign root was touched.
