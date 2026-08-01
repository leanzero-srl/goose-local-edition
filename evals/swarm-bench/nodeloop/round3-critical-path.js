export const meta = {
  name: 'planning-critical-path-round3',
  description: 'Adversarial audit of the pre-dispatch critical path — where every second of fleet idle time lives',
  phases: [
    { title: 'Find', detail: 'four lenses on the phase before the first dispatch' },
    { title: 'Verify', detail: 'defect and fix attacked separately' },
    { title: 'Synthesize', detail: 'ordered plan' },
  ],
}

const CTX = `
CONTEXT — measured fact from this project's own event logs, not speculation.

Repo: /Users/mihaiperdum/Projects/goose (branch local-edition)
Engine: crates/goose-cli/src/commands/swarm.rs (~22.6k lines), crates/goose-swarm/src/*.rs

THE FLEET: three real nodes, each a distinctly-named LM Studio instance (mihai- / workhorse- /
gabee-), all 27B-class. Proven concurrent: three simultaneous calls put all three into 'generating'.
They are NOT identical — gabee is Q6_K at ctx 193792, the other two Q8_0 at ctx 262144. PARALLEL:1
per node, so a node serves one request at a time.

THE MEASUREMENT THAT DEFINES THIS ROUND. nodeloop/occupancy.py computes busy-node-seconds / (wall x
pool) from task_dispatched/task_completed timestamps:

  run                        pool  whole-run occ  EXECUTE occ  seconds before first dispatch
  swarm-1node-r0               1       0.873         1.000              964
  swarm-3node-r0 (1 device)    1       0.863         1.000              913
  live baseline-n3-r0          3       0.729         0.994            1,312

So the SCHEDULER is not the problem: across the window it owns it keeps essentially every node busy.
ALL of the fleet's idle time is BEFORE the first task is dispatched — research, scouts, planning
drafts, detailing, contract stubs. 22 minutes at three nodes, ~39% of the run's wall clock.

Planning DOES fan out: the 3-node run made 19 planner-side calls spread 6/7/6 across the fleet
(counted from .swarm/activity/<kind>-<id>.json, each of which names the model that ran it), and
best_of_n correctly scaled to 3 skeleton drafts where a 1-node run gets 1. So this is NOT a
"planning ignores the fleet" problem. It is that planning is a SERIAL PREFIX: nothing is dispatched
until all of it finishes.

Also measured: pre-dispatch time went 913-964s at one node to 1,312s at three — 43% LONGER with
more nodes. n=1 across three different plans on a fleet with a measured 46-point replicate spread,
so that is a question, not a result.

ALREADY FIXED IN HEAD — do not re-raise:
 - the detail fan-out failed silently; it now emits detail_fallback {task_id, reason, brief_chars,
   budget_secs} and prints a warning instead of a green check
 - the 75s detail budget is now env-resolvable via detail_budget_secs() (default still 75) and
   echoed in levers_resolved
 - integrate-verify is excluded from the detail fan-out when fan_verify applied
 - read-only verify::/verify-e2e:: tasks are no longer classified as fix rounds

Two prior rounds raised 70 findings and adversarial verification refuted the large majority. Expect
most of yours to be wrong. Ground EVERY claim in file:line from the actual source; do not speculate
about code you have not read.

CONSTRAINTS on any fix: the fleet is three fixed 27B-class workers — no fix may assume a bigger
model, a different model, or changing what LM Studio has loaded. A new behaviour must be
byte-identical when its lever is OFF. Only a deterministic engine event may confer or retract a
verdict, so a change nobody can see fire is a change nobody can keep.
`

const FINDING_SCHEMA = {
  type: 'object', additionalProperties: false, required: ['findings'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object', additionalProperties: false,
        required: ['title', 'file', 'line', 'evidence', 'harm', 'proposed_change'],
        properties: {
          title: { type: 'string' },
          file: { type: 'string' },
          line: { type: 'integer' },
          evidence: { type: 'string' },
          harm: { type: 'string' },
          proposed_change: { type: 'string' },
        },
      },
    },
  },
}
const VERDICT_SCHEMA = {
  type: 'object', additionalProperties: false, required: ['refuted', 'reason', 'confidence'],
  properties: {
    refuted: { type: 'boolean' },
    reason: { type: 'string' },
    confidence: { type: 'string', enum: ['HIGH', 'LOW'] },
  },
}

const LENSES = [
  {
    key: 'overlap',
    prompt: `${CTX}

YOUR LENS: does the serial prefix have to be serial? This is the highest-value question in the round.
Read run_swarm's phase sequence (swarm.rs ~19739 research, ~20349 parallel_plan, ~21044 contracts,
~21193 pillars, ~21254 execute) and the scheduler entry (crates/goose-swarm/src/scheduler.rs
run_with_decisions ~1877).
Answer with evidence:
- Which pre-dispatch work is a true prerequisite of the FIRST dispatch, and which is only a
  prerequisite of the task that consumes it? Contracts are generated for ALL modules before ANY
  module is dispatched — is that necessary, or could a module be dispatched as soon as ITS OWN stub
  and the stubs of its dependencies exist?
- Trace exactly what the first dispatched task actually reads from the planning output. If a root
  module with no dependencies needs only its own spec and its own stub, what forces it to wait for
  the whole fleet-wide contract phase?
- Is there an existing mechanism that already overlaps a planning-ish activity with EXECUTE (look at
  dynamic replan, scheduler.rs ~2032) whose shape could be reused?
- Argue the other side honestly: what breaks if a module starts before every stub exists? The frozen
  interface is injected at dispatch, so name precisely which invariant depends on ALL stubs existing.
Report every distinct defect you can prove.`,
  },
  {
    key: 'bestofn',
    prompt: `${CTX}

YOUR LENS: does adding nodes make PLANNING slower, and if so exactly where?
Read best_of_n sizing (swarm.rs ~19956), the distinct-model cap (~11551), the draft round
(~11586-11711), plan_agreement / best_subset_agreement / structural_convergence (~9209, ~9285,
~9381), the retarget ladder (retarget_action ~8765, the plan loop ~20481), and the draft timeout
(~8815).
Answer with evidence:
- best_of_n is sized to the fleet. With 3 nodes that is 3 drafts. Do the drafts run concurrently, or
  does anything serialize them? Check the semaphore/queue in fanout_over_fleet and whether the
  planner model is also a worker.
- With 1 draft there is NO agreement signal (n==1). With 3 there is. Can a LOW agreement score
  trigger extra retarget rounds that a 1-node run structurally cannot have? Trace the condition.
  If so, more nodes buy more planning rounds — quantify the worst case from the constants.
- Does the clarity probe or the ask handshake sit on the critical path, and can they overlap the drafts?
Report every distinct defect you can prove.`,
  },
  {
    key: 'contracts',
    prompt: `${CTX}

YOUR LENS: the CONTRACTS phase, which sits entirely on the pre-dispatch critical path.
Read generate_contracts (swarm.rs ~11214), contract_stub_spec (~13054), the contract budget and
retry (~11266), drop_unparseable_stubs (~14979), the contracts event (~21164), and the injection
site frozen_interfaces_block (~17240).
Answer with evidence:
- How long does this phase take relative to the pre-dispatch total? Derive it from the real runs:
  evals/swarm-bench/runs/build/*/run.jsonl timestamps, and the .swarm/activity/contract-*.json files.
- The contract call gets worker_timeout_secs.max(120) — measured 900s — while a detail call gets 75s.
  Both are one model call producing a few hundred characters. Is that asymmetry justified by
  anything in the code, or is one of the two numbers simply wrong?
- The contracts event in swarm-3node-r0 shows a module whose stub failed to parse and was DROPPED.
  Trace what a downstream worker receives when its dependency's stub was dropped, and whether
  anything tells it the interface is missing.
Report every distinct defect you can prove.`,
  },
  {
    key: 'research',
    prompt: `${CTX}

YOUR LENS: research and scouts — the first thing on the critical path.
Read run_scouts (swarm.rs ~11011), SCOUT_LENSES (~8648), select_lenses (~8680), the scout budget
(scout_budget_secs, scout_max_lookups), fanout_over_fleet_straggler and the straggler-stop logic
(~14565, ~14636), research_tools (~19741) and grounded_research_only (~20554).
Answer with evidence:
- research_tools defaults FALSE, so scouts have no lookup tools and every "finding" is model
  knowledge. The spec under test (evals/swarm-bench/spec-build.md) says the vendor API docs are at a
  URL and must be read. Trace what actually reaches a worker about those docs, and whether any scout
  could have fetched them.
- SCOUT_LENSES has 4 entries and one is amendment-only, so at most 3 run. With 3 nodes that is one
  per node — but does the straggler-stop logic abort a lens? Check whether grace is measured from
  the straggler's own start or from the arming instant, and what that does on a serialized pool.
- What does an aborted lens cost? Name which lens and what it was carrying.
Report every distinct defect you can prove.`,
  },
]

phase('Find')
const found = await parallel(LENSES.map((l) => () =>
  agent(l.prompt, { label: `find:${l.key}`, phase: 'Find', schema: FINDING_SCHEMA })
    .then((r) => ({ lens: l.key, findings: (r && r.findings) || [] }))))

const raw = found.filter(Boolean).flatMap((r) => r.findings.map((f) => ({ ...f, lens: r.lens })))
const bySite = new Map()
for (const f of raw) {
  const key = `${String(f.file).split('/').pop()}:${f.line}`
  const prev = bySite.get(key)
  if (!prev) bySite.set(key, { ...f, also: [] })
  else {
    const best = String(f.evidence || '').length > String(prev.evidence || '').length ? f : prev
    bySite.set(key, { ...best, also: [...prev.also, f.lens] })
  }
}
const deduped = [...bySite.values()]
log(`${raw.length} findings raised across ${LENSES.length} lenses -> ${deduped.length} distinct sites`)

phase('Verify')
const judged = await parallel(deduped.map((f) => () =>
  parallel([
    () => agent(`${CTX}

REFUTE this claimed defect. You are not here to agree.

  TITLE: ${f.title}
  SITE:  ${f.file}:${f.line}
  EVIDENCE CLAIMED: ${f.evidence}
  HARM CLAIMED: ${f.harm}

Open the file at that line and READ IT, plus enough of the surrounding function to judge. Refute if
the code does not say what is claimed, the path is unreachable in practice, an existing guard already
handles it, or the reasoning does not follow. Confirm ONLY if you can locate the defect in the actual
source. A claim you cannot verify in the code is refuted.`,
      { label: `refute:${f.lens}:${String(f.title).slice(0, 22)}`, phase: 'Verify', schema: VERDICT_SCHEMA }),
    () => agent(`${CTX}

Attack this proposed FIX on consequences, not on whether the defect is real. Assume the defect IS
real and ask only whether this change is the right response.

  DEFECT: ${f.title}  (${f.file}:${f.line})
  PROPOSED CHANGE: ${f.proposed_change}

Refute the fix if it would make a run slower or less reliable for no gain, breaks an invariant the
surrounding code documents in a COMMENT, assumes a bigger model or a different fleet, changes
behaviour when its lever is OFF, or if a cheaper change gets most of the value. Read the surrounding
code AND its comments first — this codebase records why things are the way they are in comments, and
overriding one of those has burned this project before. If you refute, say what the RIGHT fix is.`,
      { label: `fixcheck:${f.lens}:${String(f.title).slice(0, 20)}`, phase: 'Verify', schema: VERDICT_SCHEMA }),
  ]).then((vs) => {
    // votes[0] judges the DEFECT, votes[1] judges the FIX. They are not redundant voters, and
    // treating either refutation as a kill discards the "real defect, wrong fix" class — which on
    // the two earlier rounds was 26 of 66 findings, the most useful class of all.
    const votes = vs || []
    // THREE states, not two. A vote is null when its agent DIED — a session limit killed 40 of 71
    // agents on the first pass of this round — and Boolean(null) being false meant a dead agent
    // scored exactly like a HIGH-confidence refutation. An agent that never ran is not a
    // refutation; it is an unanswered question, and it must be re-run rather than silently
    // resolved against the finding.
    const state = (v) => (v == null ? 'unknown'
                          : (v.refuted && v.confidence === 'HIGH') ? 'refuted' : 'stands')
    const d = state(votes[0])
    const fx = state(votes[1])
    return { ...f, votes, defect_state: d, fix_state: fx,
             defect_stands: d === 'stands', fix_stands: fx === 'stands',
             unverified: d === 'unknown' || fx === 'unknown',
             survives: d === 'stands' && fx === 'stands' }
  })
))

const all = judged.filter(Boolean)
const survivors = all.filter((f) => f.survives)
const needFix = all.filter((f) => f.defect_state === 'stands' && f.fix_state === 'refuted')
const unverified = all.filter((f) => f.unverified)
const trulyRefuted = all.filter((f) => f.defect_state === 'refuted')
log(`${all.length} findings: ${survivors.length} intact, ${needFix.length} real defects with a refuted fix, ` +
    `${trulyRefuted.length} refuted, ${unverified.length} UNVERIFIED (agent never ran — not a refutation)`)

if (!survivors.length && !needFix.length) {
  return { survivors_count: 0, needs_new_fix_count: 0, refuted_count: all.length,
           survivors: [], needs_new_fix: [], plan: null,
           note: 'every finding was refuted as not-a-defect — report that plainly, do not soften it' }
}

phase('Synthesize')
const plan = await agent(`${CTX}

SURVIVED INTACT (defect confirmed, fix judged sound):
${JSON.stringify(survivors.map((s) => ({ title: s.title, site: `${s.file}:${s.line}`,
  evidence: s.evidence, harm: s.harm, fix: s.proposed_change })), null, 1)}

REAL DEFECTS WHOSE FIX WAS REFUTED — these need a DIFFERENT fix. The fix verifier was asked to say
what the right fix is when it refuted; use that. Do not restate the rejected change:
${JSON.stringify(needFix.map((f) => ({ title: f.title, site: `${f.file}:${f.line}`,
  evidence: f.evidence, harm: f.harm, rejected_fix: f.proposed_change,
  why_rejected: (f.votes[1] || {}).reason,
  why_defect_stands: (f.votes[0] || {}).reason })), null, 1)}

Produce an ordered implementation plan for the goose engine. Order so the change that makes the
others MEASURABLE comes first. For each step: exact file and function, what the code should do, the
event or config field it needs, whether it is byte-identical when off, and the ONE deterministic
signal that proves it worked on a real run. The prize this round is wall-clock before the first
dispatch — 22 minutes at three nodes — so say for each step how much of that it plausibly reclaims
and on what evidence. State honestly which step you are least confident about and why.`,
  { label: 'synthesize', phase: 'Synthesize' })

return { survivors_count: survivors.length, needs_new_fix_count: needFix.length,
         refuted_count: trulyRefuted.length,
         unverified_count: unverified.length,
         unverified: unverified.map((f) => ({ title: f.title, file: f.file, line: f.line,
           defect_state: f.defect_state, fix_state: f.fix_state })),
         survivors,
         needs_new_fix: needFix.map((f) => ({ title: f.title, file: f.file, line: f.line,
           evidence: f.evidence, harm: f.harm, rejected_fix: f.proposed_change,
           why_fix_rejected: (f.votes[1] || {}).reason,
           why_defect_stands: (f.votes[0] || {}).reason })),
         plan }
