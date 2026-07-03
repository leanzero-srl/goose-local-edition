# Claude Code adversarial review + dynamic flows — researched, then mapped to goose-swarm (2026-07-03)

## A. Dynamic workflows (fan-out -> reduce -> synthesize)
Publicly documented mechanics (alexop.dev; Anthropic "dynamic workflows GA"):
- Control flow is EXPLICIT CODE (a JS script), each step a FRESH subagent with a clean context window — "the script holds the plan, not the model's context."
- Primitives: parallel() = BARRIER (await all, failed agent -> null); pipeline() = STREAMING (no barrier, item A in stage 3 while B in stage 1); agent(prompt, {schema}) = one subagent with STRUCTURED OUTPUT (validated at the tool layer, model retries on mismatch — replaces fragile text parsing).
- Dominant shape: FAN-OUT (N independent agents in parallel) -> REDUCE (dedup/filter/rank in plain code, no agent) -> SYNTHESIZE (one agent from curated findings).
- Up to 1000 subagents/run, ~16 concurrent.

## B. Adversarial review (panel + verify)
Publicly documented (agent-review-panel; Deep Review; ASDLC critic pattern):
- 4-6 reviewers with DISTINCT personas + DISTINCT reasoning strategies (Correctness Hawk, Security Auditor, Devil's Advocate, Feasibility, Risk, Clarity), reviewing INDEPENDENTLY in parallel (no cross-talk) to avoid an echo chamber.
- Then: private confidence -> adversarial DEBATE / cross-examine (1-3 rounds) -> BLIND final scoring (anti-anchoring).
- A VERIFICATION LAYER before the judge: each claim checked against SOURCE (read the actual code / grep-cat), severity verified by reading the real code, hallucinated findings DEMOTED, a post-judge re-verify catches judge-introduced hallucinations.
- A SUPREME JUDGE synthesizes + arbitrates + resolves disagreements.
- Anti-groupthink: blind scoring, sycophancy detection, correlated-bias warning.
- Core principle (the "three-vote"): a finding SURVIVES only if independent skeptics fail to REFUTE it (majority). One AI writes, ANOTHER tears it apart.

## C. What goose-swarm already has vs the gap
HAVE: fan-out across models (fanout_over_fleet: scouts/plan/contracts/complete-fix); distinct-persona parallel review (REVIEW_FANOUT: correctness/wiring/interface/edge-cases — the "panel", read-only, just shipped a28a827f5); a judge (JUDGE verdict); reduce (group_findings_by_file dedup).
GAP (the heart of the adversarial pattern): findings are NOT independently VERIFIED before they drive a fix. The golden-check regression proved the cost — an unverified finding drove a fix that broke a correct app. Claude Code's answer is exactly the missing piece: a finding must survive an independent REFUTATION (by a DIFFERENT model, checked against the real code) before it is accepted, and the fix must be re-verified against ground truth after.

## D. Implementation: bring the adversarial-verify + synthesize layer in-product (REVIEW-fanout Phase 2)
Map the pattern onto the swarm's post-execute REVIEW as a fan-out -> reduce -> VERIFY -> synthesize -> re-verify pipeline:
1. FAN-OUT (done): the 4 dimension reviewers = the distinct-persona panel, independent + parallel, read-only.
2. REDUCE (done): group_findings_by_file dedups + buckets by file.
3. VERIFY (NEW = the adversarial core): each candidate finding is handed to a DIFFERENT fleet model (round-robin, never the raiser) prompted to REFUTE it AGAINST THE ACTUAL CODE -> CONFIRM|REFUTE + HIGH/LOW confidence (structured). Only a finding with an independent HIGH CONFIRM SURVIVES (the three-vote / severity-verification, in-product). This is READ-ONLY too (no write-race) and fans across models (idle nodes used).
4. SYNTHESIZE (NEW): only SURVIVING findings drive the existing fix path (group -> shadow-isolated fix).
5. RE-VERIFY (NEW, closes the silent-regression hole the skeptics flagged): after the REVIEW fix, run the RUNNING oracle (run_smoke_gate: pytest + entry --help) against ground truth; if the fix went RED, it regressed -> do not ship it green.
Flags: GOOSE_SWARM_REVIEW_FANOUT (Phase 1, advisory, shipped) + GOOSE_SWARM_REVIEW_VERIFY (Phase 2: verify -> fix -> re-verify). Default OFF. Anti-groupthink = the refuter is always a DIFFERENT model + fail-closed (uncorroborated -> stays advisory, never fixes).
