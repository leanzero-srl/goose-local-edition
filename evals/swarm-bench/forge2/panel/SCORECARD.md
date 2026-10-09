# Forge 2.0 design panel: scorecard and synthesis decisions

Chief designer's record, 2026-10-09. Inputs read whole: the six designs in this directory, the 30 judge verdicts (six
designs × five lenses: difficulty for GPT-6.1 Sol / Opus 5.5, fairness, fidelity and gradeability, mandate coverage,
spread + economics + feasibility), `forge2/NOW.md` (the mandate, verbatim), `research/BRIEF.md` (its §10
do-not-build-on list is binding), `research/understand/{frontier-behaviour,real-forge-fidelity,machinery,integration}.md`
and Forge 1.0 `DESIGN.md` §8 and §17.8. Nothing was run. The synthesis is `forge2/DESIGN-DRAFT.md`.

---------------------------------------------------------------------------------------------------------------------

## 1. The scores

| design (product) | difficulty | fairness | fidelity | mandate | spread / economics | total |
|---|---:|---:|---:|---:|---:|---:|
| enterprise-product ("Changeproof", change evidence with a hash chain) | 6 | 6 | 5 | 7 | 5 | 29 |
| systems-torture ("Loadline", workload radar, 4 tenants, 10k burst) | **7** | 5 | 6 | **8** | 5 | 31 |
| security-first ("Embargo Desk", vulnerability intake under embargo) | 6 | 6 | 6 | 7.5 | 5 | 30.5 |
| platform-breadth ("Launch Control", 19 module types incl. Confluence, SQL) | 6 | 5 | 5 | 7 | 5 | 28 |
| grader-first ("Escalation Desk", CRM escalations + SLA clocks) | 6.5 | 6 | **6.5** | 7 | **6** | **32** |
| frontier-adversary ("Signal Desk", alert dedup into escalations) | 6 | 6 | 6 | **8** | **6** | **32** |

### 1.1 Predicted GPT-6.1 Sol score: the designs against their judges

| design | the design's own claim | difficulty judge | other judges |
|---|---|---|---|
| enterprise-product | 0.45–0.75, central 0.65 | ≈ 0.58, bimodal: 0.73–0.74 no critical (P ≈ 0.5), 0.46–0.49 one, 0.29 two | spread: ≈ 0.50 expected; P(≥ 1 critical) 0.55–0.75 |
| systems-torture | 0.50–0.78, likely 0.66–0.74 | ≈ 0.55 (modal 0.58; 30% below 0.45) | spread: modal 0.45–0.55; fidelity: harness itself charges 2–4 roster defects |
| security-first | 0.45–0.75, central 0.58 | ≈ 0.47 (0.28–0.72) | spread: ≈ 0.45 trimodal; mandate: 0.50–0.80 |
| platform-breadth | 0.40–0.70, central 0.55 | ≈ 0.55 (0.27–0.76) | spread: median ≈ 0.40 (0.22–0.69) |
| grader-first | 0.45–0.75, central 0.58 | ≈ 0.50, bimodal: 0.82 / 0.49 / 0.30 | spread: 0.28–0.85 (0.50); mandate: 0.50–0.78 (0.62) |
| frontier-adversary | 0.55–0.78, central 0.68 | ≈ 0.52 (0.35–0.68; P(< 0.45) ≈ 0.25) | spread: 0.55–0.70 (0.62); mandate: 0.62–0.85 (0.72) |

Every judge put Sol inside or near the 0.45–0.75 target on every design, so "is Sol challenged" did not separate the
designs. What separated them is WHY Sol loses: on stated engineering, or on composition artefacts, cliffs and
ambiguity. That is the axis the base choice below follows.

### 1.2 What the 30 verdicts agree on (problem classes counted across verdicts)

| # | consensus finding | designs it hit | resolved in the draft by |
|---|---|---|---|
| 1 | A count-based graded band 4 compresses the frontier: its floor is reached at n = 5 (breadth), 9 (enterprise, security), 16 (systems), 17 (adversary), so the top is ranked by an integer and 1.0's 0.9 cluster reappears at 0.64–0.80 | 5 of 6 (every spread judge) | no graded robustness band at all; two "unusable" bands only; every row continuous (DESIGN-DRAFT §F.12) |
| 2 | Cliff criticals make one paid pilot a lottery, and stacking ×0.6 factors squeezes mid models toward 0 | all 6 | five criticals, the extensive ones scaled by extent, bounded compounding with a 0.30 floor (§F.11) |
| 3 | Scoring time underestimated 2–10× (per-call 0.6–1.5 ms was measured without scheduler gating; invocation spawn cost was never measured) | all 6 | re-derived 15–30 min/tree, a measured WP1 go/no-go, mutant mode, gate compute budgeted (§J.7) |
| 4 | Browser and UI Kit lanes run on real timers, so wave counts, 429 retries and "no polling" rows move with load | 5 of 6 | a virtual frame clock aligned to the backend clock; UI Kit host fake timers; event-defined settle (§D.1) |
| 5 | Zero-tolerance dosing rows ("calls after the 429 = 0") under the stated concurrency charge races the golden itself cannot avoid | 5 of 6 | 1.0 §17.6's concurrent-refusal rule extended to every "after a signal" row; continuous scoring (§F.5) |
| 6 | Criticals or rows that fire on correct designs: "any KVS diff", "an idle hour writes nothing" against mandated self-accounting, canaries that ignore an admin/auditor carve-out, global display names as tenant canaries | enterprise, systems, grader-first, adversary, security | criticals keyed to ENUMERATED domain state; bookkeeping exempt by name; per-viewer canary sets; installation-unique canaries only (§F.0, §F.11) |
| 7 | Graded numbers missing from the contract (the 1.0 anchor test checks a section, not the number in it) | all 6 | the anchor test asserts every numeric threshold verbatim in its sentence or `RATE-MODEL.json`, equal to the emulator constant (§H) |
| 8 | The dev site cannot exercise what the scorer grades: scale, the permission-rich world, boot counters, the UI Kit host | all 6 | permission-rich dev installation, `--scale upper`, `forge-dev boot report`, `forge-dev uikit`, each class ≥ 3 times (§D.6) |
| 9 | Phase 4 needs a pass bar and must run BEFORE the build | all 6 (grader-first even recast it as a fairness check) | phase 4 with Sol and Opus 5.5 as planners, a blind grader and a numeric bar, gating WP7 (§G.5) |
| 10 | The deterministic scheduler is the one MEDIUM-LOW mechanism everything rests on, and no design had a workable fallback | all 6 | day-3–5 measured go/no-go with numbers; two named fallbacks with the rows each loses (§J.1, §J.5) |
| 11 | Vacuous "absent" rows charged as failures penalise the more robust design (an app never killed fails the kill row) | adversary, grader-first, breadth | app-independent stimuli keyed to seeded entities; a two-verdict precondition column (§F.0 rule 4) |
| 12 | The pool arithmetic must make compliance provably safe AND overspend bite: 62k > 60k (systems), a non-dosing app never walls (breadth), the golden over its own cap (adversary) | 3 of 6 | a stated inequality with a 5,500-point margin, a hot hour and a burst hour calibrated so naive designs wall, and the golden's ledger as a freeze artefact (§C.4, §D.3) |

---------------------------------------------------------------------------------------------------------------------

## 2. Each design: what the judges found

**enterprise-product (29).** Best customer framing in the panel: every requirement is a persona's demand (auditor,
CISO, DPO, the vendor's SRE), and Phase B replays a release train under serial, reads-first-adversarial and seeded
schedules, worst counts. Judges' decisive findings: `c_chain_integrity` (its largest C row) reads only the app's own
export, so storing facts unordered and computing seq/prev/hash at export time passes every chain row with no
concurrency control (fidelity); `e_idle_reconcile` demands 0 writes while the stated budget rules force a ledger write,
so the golden cannot score 1.0; `s_admin_bypass` fires on any KVS diff, including the permission cache and refusal
audit its own rules require (fairness, fidelity: 1.0 defect B again); the graded evidence semantics (reason codes,
policy retroactivity, deletion identity, release membership) were left as "§C.1 verbatim"; the 40-row band 4 floors
at n = 9 and pins every frontier model at ≈ 0.74; scoring time is off ≈ 10× (≈ 13,500 export invocations per world).
Disposition: not the base (fidelity 5; the product's core semantics are the most ambiguity-prone of the six).
Grafted: the reads-first adversarial schedule, the persona table, the per-person export throttle (as a per-person
budget), "world changes stay driveable through the stated admin API", personal-data reporting.

**systems-torture (31).** Best difficulty lens (7) and joint-best mandate coverage (8): trivial arithmetic so that all
difficulty sits in operations; a 10,000-update burst against the 500-events/min push limit; a quota wall longer than
the 900 s `retryAfter` clamp; disjoint probe populations so each root is priced once by construction. Judges' decisive
findings: several requirements no app can meet together (an idle hour with 0 writes against ±2 % self-accounting; "exact
2 minutes after quiet" against stated 3-minute event lateness; get-workload's array input against Rovo's scalar-only
inputs at the ROOT of every read-contract row; a pool budget that sums to 62k > its own 60k attribution bound), so the
golden fails `p_idle_rerun` and `k_entities_indexed`; `p_final_exact` is a catch-all ×0.8 critical; 11 compounding
criticals collapse mid models to ≈ 0.04–0.15. Disposition: not the base (fairness 5, several jointly unsatisfiable
rules). Grafted: disjoint populations, the throughput physics (burst above the push limit; a serial reader misses the
bar), the "exact or `migrating`, never a wrong number" read rule, the shared dev ledgers, the dashboard widget.

**security-first (30.5).** Best statement of a threat model ("who can attack, what must never happen"), and the best
coupling of security with Tier 1: per-request permission checks for an open Desk cost 9,180 points/h against a stated
per-person cap, so the app must cache, and caching per tenant, beyond the TTL or without the installation leaks.
Consequence-class critical dedup and existence-only leaks at ×0.8. Judges' decisive findings: `f_desk_loads` (band-2
gate AND critical) needs an under-specified oracle first page; the assistant must publish answers over Realtime while
the Desk "never displays data received over Realtime"; "counts" and "JQL fragments" are leak channels with no
observable; the ambiguous bulk create cannot happen to an app that budgets its remaining time; `a_triage` needs ≈ 5 h
of LLM work against a 90-minute bar at the stated limits; a stub refusing everything collects much of S because the
precondition column was dropped. Disposition: not the base. Grafted: the per-person interactive budget collision, the
threat-model sentence, consequence-class dedup, existence-vs-content leak severity, derived personal-data erasure,
positive controls on every S row.

**platform-breadth (28).** Most modules (19 types), with a real argument that breadth bites when one fact must hold on
surfaces with conflicting contracts (shared JQL precomputations, a user-less macro cache, a synchronous gate).
Judges' decisive findings: the Confluence macro must show "ready" computed from Jira-installation storage it cannot
read, with no sanctioned channel (fairness); Forge SQL needs an unmeasured 100–250 MB engine; the riskiest machinery has
no cut path while four Preview modules are probe-gated; Tier 1 overspend is toothless (an undosed bulk app peaks at
≈ 56.6k of 65k and loses ≈ 0.0007 of inner); band 4 caps at 0.799 from n = 5. Disposition: disqualified as a base on
fidelity (5) and fairness (5): the breadth that defines the angle is exactly what cannot be graded faithfully.
Grafted: `dashboards:widget`, a stored read-only `jira:customField` written through `/app/field/value` as the honest way
to keep a JQL function fresh above 1,000 matches, the inequality that makes compliant apps provably unable to wall, "stay
useful during the wall through bridge reads", per-installation grading start for backfills.

**grader-first (32).** Strongest measurement architecture: a lockstep virtual-time emulator where verdict = f(tree,
seed), proven by a freeze gate that scores every tree idle and under a saturating CPU hog and demands byte-identical
verdicts; every emulated behaviour classed M / D / W, with any G (guess) refused at freeze; no excellence slice (every
efficiency target is a stated budget); no graded band; five criticals with C4 continuous and C5 attributed by
counterfactual ("the app's overspend caused the wall"). Judges' decisive findings: `admin.export` (the auditor read path
behind all R rows) contradicts the "browse to see" rule; the 5-minute admin TTL behind critical C3 exists only in the
reference; 1.x data has no `openedAt`; role-restricted comments and the Realtime threat model are knowledge triggers for
C1, not stated; R rows graded over ALL items dilute a missing mechanism to ≈ 0.001 per row (no-critical Sol inner ≈ 0.82);
crypto entropy is unseeded and the browser lane is off the virtual clock; its R1 fallback ("snapshot replay") cannot work;
"pretty" is unscored, `dashboards:widget` absent, no tool loop, phase 4 recast as a fairness check. Disposition:
**BASE** (reasons in §3).

**frontier-adversary (32).** Best requirement-writing method: guarantees and platform facts, never the test plan; every
hard row a coupling of two or more stated facts; faults keyed to DOMAIN ENTITIES (the same fingerprints lose their
answers whatever order an app works in; the app's OWN most-used model is the one deprecated); a stated admin API that
decouples backend grading from UI Kit rendering; a parallel-tool-call Ask loop. Judges' decisive findings: a dynamic
web trigger plus Relay egress (not Runs-on-Atlassian eligible); the golden as specified fails seven rows (10 s live
updates against a ≥ 60 s flush; 2-minute JQL freshness against a 3-minute field lag; a 6-minute SLA bound against a
10-minute skipped tick …) and spends 11–20k points in the storm hour against its own 12k cap; band 4 of 77 exact rows
saturates at n = 17; vacuous rows fail the robust design; zero-diff criticals fire on the golden's own accounting;
issue-property JQL needs an undeclared `jira:entityProperty`. Disposition: not the base (see §3 for the tie). Its
method is grafted wholesale.

---------------------------------------------------------------------------------------------------------------------

## 3. The base: grader-first ("Escalation Desk")

**Chosen because** (judges' evidence, not the total):

1. It is the only design whose judged problems are all TEXT or DEFINITION fixes on top of a sound measurement core. The
   fidelity judge: "The architecture is right for gradeability, but the spec is not yet gradeable"; the fairness judge:
   "The architecture is fair; the check registry is not yet". Every other design's decisive problem sits in its
   composition (band-4 compression: enterprise, systems, security, breadth, adversary) or its product semantics
   (enterprise's chain, breadth's macro data path, systems' contradictions).
2. It is the only design that already removed the two composition defects every spread judge found: no graded band, no
   unstated excellence optimum. Five criticals, C4 continuous.
3. Its load-robustness is a refusing gate (G1: byte-identical idle vs saturated), not an intention. That answers the
   owner's "measurement holds under machine load" constraint by construction, and it is what makes calibration
   parallelisable.
4. Its M / D / W / G freeze rule is the strongest form of "live wolfaenpak conformance for every emulated behaviour"
   in the panel (fidelity 6.5, the highest).
5. Its product is Runs-on-Atlassian eligible (static web trigger, no egress), which matters for a security mandate and
   is LeanZero's own rule zero.

**Why not frontier-adversary at the same total.** Its strengths are the requirement-writing METHOD (graftable intact),
while its defects are structural: the composition (a 77-row exact band 4) must be replaced, the golden's own design
fails seven rows and its own Tier 1 cap, and the product needs egress and a dynamic trigger. Rebuilding its composition,
golden and posture would leave little of the base. The products are close cousins (external events → exactly one Jira
issue, SLA clocks), so its difficulty devices transplant without friction.

**What the base keeps:** the product, the lockstep core and G1, the fidelity classes, the auditor export as a stated
compliance feature, five criticals, two "unusable" bands, continuous rows, the CRM sender as a stated world (W), the
exactly-once protocols (comment marker, label/property with lag-aware lookup or `reconcileIssues`), the 1.x data
migration, the role-restricted-comment prompt-scope trap (now STATED).

---------------------------------------------------------------------------------------------------------------------

## 4. What is grafted, from where, and where it lands

| idea | from | why | lands in DESIGN-DRAFT |
|---|---|---|---|
| guarantees + platform facts, never the plan; hard rows are couplings | frontier-adversary §0 | the 1.0 frontier won by paraphrasing an announced fault list | §E, §G.2 |
| faults keyed to domain entities (cases, notes, deliveries), the app's OWN most-used model deprecated | frontier-adversary §0 rule 3, §D.3 | every implementation meets the same adversity whatever its call order | §D.4 |
| disjoint probe populations; deep rows graded over their population | systems-torture §D.5 + grader-first difficulty-judge fix 1 | prices each root once by construction AND makes a missing mechanism cost its row, not 0.001 | §D.4, §F |
| reads-first adversarial schedule for race rows; canonical schedule for throughput rows | enterprise §D.4 + its fairness judge's fix | exposes read-modify-write for every code shape without distorting SLOs | §D.1 |
| stated admin API; the harness configures and changes the world through it | frontier-adversary §C.3, enterprise "driveable" | decouples backend grading from the UI Kit host (security-first's cascade problem) | §C.3, §E §6 |
| burst above the 500 events/min push limit; inbox + doorbell; a `busy` 503 output | systems-torture §C.3, breadth §C.2 | throughput must come from batching, not parallelism | §C.2, §D.3 |
| per-person interactive budget that forces scoped verdict caching | security-first §C.5 | security against Tier 1 in one stated arithmetic | §C.4, §F.5 |
| parallel-tool-call loop with tool results scoped to the requester | frontier-adversary §C.5, security-first §C.6 | BRIEF G4 was untested in the base | §C.5, §F.6 |
| `dashboards:widget` + `edit` | systems-torture, breadth; grader-first mandate judge | newest GA Custom UI module; a third boot surface | §B, §C.6 |
| stored read-only `jira:customField` via `/app/field/value` (number), probe-gated | breadth §B, adversary §B | the honest way to keep a JQL function fresh above 1,000 matches; bulk-write economy | §C.10 |
| `rovo:skill` and `rovo:mcp` wiring (static, low weight) | adversary, breadth | "latest modules combined" without volume | §B, §F.1 |
| personal-data reporting with derived-data erasure | security-first §C.1, enterprise | a Marketplace security duty that is also a burst hazard (sec#31) | §C.11, §F.2 |
| pool inequality that makes compliance provably safe; overspend walls in the burst hour | breadth §C.4 + spread-judge fixes | C5 attributable and biting | §C.4, §D.3 |
| "exact or `migrating`, never a wrong number" during migration | systems-torture §C.10 | a fair rule the golden can meet | §C.9 |
| consequence-class dedup; existence-only leak at ×0.8 | security-first §F.2 | one event fires one critical; severity by harm | §F.11 |
| positive controls on S rows (refusal correctness × authorised success) | security-first spread judge | a stub that refuses everything earns nothing | §F.2 |
| "useful during the wall": stored data plus bridge reads | breadth §C.4 | interactive surfaces under a Tier 1 outage | §C.6, §F.7 |
| alternative-designs table, ≥ 2 per hard guarantee | adversary §C.10 | proves outcome-only grading | §C.12 |

---------------------------------------------------------------------------------------------------------------------

## 5. The judges' fixes: adopted, modified, rejected

### 5.1 Adopted (the base's 4-lens problem lists, each mapped)

| problem raised on grader-first | fix in the draft |
|---|---|
| `admin.export` and the digest contradict "browse to see" | §E §2 carve-out: desk-admins see CRM-sourced fields of every escalation; Jira content only where they can browse; the harness auditor browses everything (stated); C1 scans admin channels for secrets, other-tenant canaries and Jira content of unbrowsable issues only |
| C3 rests on an unstated 5-minute admin TTL | ONE stated reuse bound for every access decision: 5 virtual minutes; probes at revocation + 5 min + ε |
| 1.x data has no `openedAt` | added, with a stated 1.x clock rule (default policy from the upgrade instant) |
| role-restricted comments and the Realtime threat model are unstated knowledge triggers | both stated in §E §2 and §E §7 |
| inner diluted by all-items fractions | deep R rows graded over their population (+ a 20 % all-items share) |
| bimodal single pilot | extent-scaled C1/C2/C4/C5, bounded compounding, a no-critical inner reported beside every final, three scoring seeds, and an owner option for a second run |
| crypto entropy unseeded; browser lane on real time; CPU watchdog yields unpublishable verdicts | all entropy seeded; virtual frame clock; CPU-bound loops become the invocation's own timeout (app evidence) while harness stalls hold |
| vacuity punishes avoidance | stimuli independent of the app (seeded platform terminations, seeded push failures); two-verdict precondition column |
| `t_retry_discipline` repeat = same invocation | repeat = same installation + method + path + body hash, across invocations |
| `t_pool_wall_pause` binary and racy | continuous; a call counts only if issued one stated KVS round trip after the installation's first refusal |
| `t_smoothness` share-of-own-spend fails quiet hours | absolute cap: ≤ 2,500 background points in any 5 virtual minutes |
| `r_freshness` exempts only pool refusals | exempts every interval a §4 rule forbids the needed calls, + 5 min |
| "changes nothing" collides with bookkeeping | C3 only on enumerated domain state; caches, accounting and refusal logs exempt by name |
| FAIL_IF_EXISTS on expired-but-readable keys unmeasured | P03 extended to writes; the replay rule is stated in time ("applied once for 24 h"), never via the app's TTL |
| `l_bundles_load` "M" with no probe (esbuild vs webpack/Babel) | bundling as `@forge/bundler` does (classic pragma); P25 parity corpus through real deploy |
| platform limits not stated | §E §10 table + `t_platform_limits` |
| R1 fallback ("snapshot replay") cannot work | measured go/no-go; fallback A (fresh processes, A at ⅓ scale) and fallback B (per-key serial + scripted pairwise interleavings), rows lost named |
| scoring time 2–3× low; gate compute unbudgeted | re-derived per lane; mutant mode; machine-hours per gate pass stated |
| "pretty" unscored | V tier 0.05: tokens, dark surface, layout fit, designed states, console, axe-core (pinned) |
| `dashboards:widget` absent; `rovo:skill`/`mcp` cut | both in |
| no tool loop | the draft is a bounded parallel tool loop |
| phase 4 recast as a fairness check | phase 4 is a PRE-BUILD difficulty gate with a numeric bar; fairness misses are fixed in text in the same pass |
| resolver robustness ungraded | `r_resolver_contract` and `s_input_handling` |
| dosing only on Jira REST | `t_platform_limits` (push, KVS units/RPS/per-key, Realtime, bridge limiter, invocations) |
| READY contradicts the bridge read wave | READY needs only escalation fields; Jira fields through the bridge may follow READY |
| defect C only in lint | the emulator invokes every schema-valid consumer form (resolver arm included) |
| personal data missing | `s_personal_data` |
| dev site lacks the permission world and the UI hosts | §D.6 |
| contract size under-counted | honest 32 KB prose + 15 KB lookup JSON |
| output tokens and packaging understated | Sol $5–9; user-visible first-run kit download 0.45–0.7 GB |
| §G.5 gap (0.80–0.85 undefined, no lower bound) | full acceptance table: challenged / cruise / too hard / harness-suspect |

### 5.2 Modified

| judge fix | what I did instead, and why |
|---|---|
| "adopt K.5's 0.36 floor" (several) | bounded compounding: the worst critical at full factor, each further one at half its penalty, floor 0.30. Two cliff criticals give 0.48 instead of 0.36: mid models keep visible partial credit and each extra critical still costs |
| "make band 4 continuous" (every spread judge) | removed the graded band entirely (grader-first's choice): a band step on top of a row's own loss charges one root twice |
| "add `jira:customField` with a value function" (security mandate judge) | a STORED read-only field written by the app; the value function's invocation and caching are unmeasured (machinery §4.7: HIGH risk) |
| "score A at the contract's upper bounds" (grader-first mandate judge) | the A burst and size sit near the bounds; the bounds stay an envelope with seeded variation |
| "Opus AND Sol pilots" | Sol is the owner's acceptance test; Opus runs in phase 8. A second Sol run is an owner option only when the first verdict is ambiguous |
| "per-tier budget of 400 if Opus uses > 2× Sol's calls" | 300 recommended now; pull the 1.0 Sol/Opus call counts from the provider's generation logs before freeze and revisit |
| "blind alt app from a different model family as THE fairness gate" | two alts: alt A (our agent, public text only, different mechanisms) is the ≥ 0.95 gate; alt B (blind, another family, cheap) is diagnostic: every loss is classified app defect or contract gap |

### 5.3 Rejected

| judge fix | reason |
|---|---|
| "building a surface with one security slip must score ≥ omitting it − 0.05" (systems and security spread judges) | contradicts the owner's 1.0 rule, kept as a selftest: a leak scores below a missing surface. Kept for FUNCTIONAL defects only: no critical fires on a functional partial |
| Forge SQL (breadth K2) | needs a MySQL-compatible engine in the payload with unmeasured fidelity, and an atomic upsert dissolves the concurrency difficulty (adversary's own point) |
| Confluence static macro (breadth K3) | the cross-product data path is undocumented; +≈ 45 % mock work; frontier-behaviour §5.3 "mostly volume" |
| Relay egress for an external exactly-once effect (adversary) | breaks Runs-on-Atlassian; the exactly-once class is already graded on Jira comments and issues |
| a verbatim Atlassian reference pack (enterprise, ≈ 70 KB) | licence question and desk-audit risk; one-line facts with URLs in the contract instead |
| an unscored LLM or human "beauty vote" | not deterministic and grades model taste; screenshots and video are published for people to judge |
| `worker_threads` isolates to save memory (grader-first R6, systems-torture) | one process shares `BroadcastChannel` and the filesystem: a cross-tenant side channel production lacks. Separate fenced processes stay, sized by the WP1 RAM measurement |
| two scoring worlds instead of three seeds | worst-of-three is what keeps a lucky seed from carrying a race; mutant mode carries the gate cost |
| a pre-decided cut list that drops mandate modules if the build runs long | owner: "maximal now". Cuts exist only where a FIDELITY probe fails (the custom field), never for effort |

---------------------------------------------------------------------------------------------------------------------

## 6. Base against synthesis, in numbers

| | grader-first as written | DESIGN-DRAFT |
|---|---|---|
| manifest module types | 12 | 15 (+ widget, custom field, skill, mcp) |
| weighted rows / criticals / bands | 76 / 5 / 2 | 93 / 5 (extent-scaled, bounded) / 2 |
| deep-row grading | all items | seeded populations (+ 20 % all-items share) |
| Sol predicted | 0.45–0.75 (judges: ≈ 0.50, bimodal 0.82 / 0.49 / 0.30) | 0.40–0.75, central ≈ 0.56; no-critical inner 0.66–0.78 (§G.3) |
| mid / weak | 0.17–0.48 / 0.03–0.25 (judge: internally inconsistent) | 0.12–0.35 / 0.00–0.15, gated by archetype controls |
| public input | 27 KB prose + 6 KB JSON | ≈ 32 KB prose + ≈ 15 KB lookup JSON |
| reference app | 5.9–7.9k LOC | ≈ 7.5–9.5k LOC |
| call budget | 250 | 300 (owner decision) |
| Sol cost per run | $2.3–6.6 | $5–9 (central ≈ $6.5) |
| scoring time per tree | 6–10 min | 15–30 min (pathological ≤ ≈ 60) |
| calendar to the paid Sol pilot | unstated | Pilot-0 (backend-only Sol) ≈ day 10–14; full pilot ≈ week 4–5 |
