# Cloud SB7 instrument-qualification restart

Date: 2026-08-23

## Decision

The publication paragraph later in this incident record is historical. The
operator subsequently reaffirmed that the website must remain one stable
`sb-7.0` board, never an `sb-7.0-rc` era. Current code therefore seals the raw
RC/calibration identity locally while requiring the pinned publisher's exact
authorized mapping to public `scorerVersion=sb-7.0`; remote and rendered
receipts verify both the stable public document and the separate raw identity
hash. The operator also authorized launch once all technical gates pass.

The roots `cloud-sb7-20260823-ac376` and
`cloud-sb7-20260823-0a905-s1` are instrument-qualification history, not SB7
benchmark outcomes. No full build episode began in either root. The first root
failed before any provider attempt. The second root made only bounded contract
smoke calls and exposed defects in the smoke coordinator/verifier.

The campaign of record may therefore start only through the dedicated
`qualification-restart` transition. This does not erase either root, reset
spend, reclassify a failed model output, or authorize an extra benchmark
outcome. It creates one fresh all-entrant qualification contract before the
first full episode.

## Measured source state

The source root is
`/Users/mihaiperdum/goose-builds/cloud-sb7-20260823-0a905-s1`.

- All five full entrants have zero episode attempts, zero admissions, zero
  terminal requests, zero lifecycle events, empty raw trees, no score, and no
  publication artifacts.
- Its budget ledger is structurally valid, has no outstanding reservation, and
  records eight settled requests costing an upper-bound $0.034278575.
- Those eight request IDs equal the union of the sealed terminal request IDs in
  the four Gemini/DeepSeek smoke attempts. GLM admitted no request and cost
  zero. There is no unexplained settlement.
- The four non-GLM streams contain the exact structured shell stdout and exit
  status required by the intended contract. The frozen verifier rejected them
  because it expected a legacy tool name/display string and because its process
  inspector included its own `ps` process.
- GLM failed before admission against the wrong Z.ai endpoint. The corrected
  candidate manifest changes only its endpoint from `/api/paas/v4` to
  `/api/coding/paas/v4` and adds `ZAI_API_BASE_URL`.

The detailed stream hashes and root-cause evidence are in
`CLOUD-SB7-INCIDENT-2026-08-23-STREAM-VERIFIER.md`.

## Enforced transition

`qualification-restart` refuses execution unless all of the following are
proven from frozen artifacts:

1. The source campaign, manager, monitor, entrant processes, smoke processes,
   and vendor ports are stopped and clean.
2. Every full entrant is pristine as measured above. A non-empty tree, a score
   directory, one lifecycle event, one attempt, or one admission forbids the
   transition.
3. Every dollar in the cumulative ledger is explained by sealed smoke terminal
   usage, no reservation is outstanding, and enough frozen budget remains for
   the first full episode.
4. The binary, prompt, scorer, thresholds, fixtures, task, model roster,
   sampling policy, pricing, caps, complete publisher runtime identity, and
   requested models are unchanged. The only endpoint transition is the exact
   GLM-5.3 Z.ai `/api/paas/v4` to `/api/coding/paas/v4` correction with the
   `ZAI_API_BASE_URL` binding; all other endpoint changes are refused before a
   credential is read or an authenticated request is made. The only publisher
   transition is LeanZero commit
   `817b5367bd8a176c45aff1bdc1c0fb2bea32ea4a` (instrument
   `b6ab4f36cd217d491ff1e928059bc74ef67a6361b6be9f7c88df06c705862384`)
   to `694927b0b610c93f0c34dee01004c6def367e670` (instrument
   `5bb8138f206aea054076c6100b0f6aa94d82f31e154ddd46babc214a8ddc4de7`).
   Its tracked delta is exactly the frozen seed script and publisher library;
   the manifest, package files, dependencies, Node runtime, environment-file
   identity, Sanity target, entrant mapping, expected check count, live URL,
   revalidation URL, and process/verification timing remain byte-for-byte or
   value-for-value identical. A local snapshot proves this before authenticated
   provider preflight.
5. Defect evidence names all entrants and binds the source campaign, unchanged
   binary, current fix commit, root-cause artifact, and regression artifact.

The transition writes an immutable receipt and complete source seal before a
target can run. The source is then permanently non-runnable. The target carries
the old ledger byte-for-byte, the original fixture seeds, copied defect/seal
evidence, and a qualification-history hash inside its smoke contract. Every
target smoke state is new, has zero launches, and must pass the strict current
verifier. A crash at any commit boundary resumes idempotently into the same
target; a fork or second qualification restart is refused. One authenticated
roster snapshot is reused by target initialization only after its binary,
roster, ports, credential mode, and publisher snapshot are locally revalidated.

The qualification transition still pins the historical publisher bytes exactly,
but that pin grants no authority to rewrite benchmark identity. The hermetic
`sb-7.0-rc` scorer version, complete uncalibrated disclosure, and provisional
status must now survive publication exactly. Before any live process, the
publisher dry run must emit one sealed machine-readable receipt carrying those
three exact values. The pinned publisher maps the RC version onto stable
`sb-7.0` and omits calibration/provisional fields, so it is deliberately refused
at that boundary. Remote Sanity and rendered verification require the same
exact values once a future public schema and pinned publisher can represent
them; until then scoring evidence may remain sealed locally but no cloud result
can be published.

The target remains generation zero for benchmark outcomes. If a later full
episode proves a genuine infrastructure defect, the existing single-hop
supersession remains available and carries the qualification history forward.
Quality, score, timeout, or model behavior never authorize that hop.

## Regression coverage

The harness tests cover cumulative spend/seed preservation, source immutability,
fresh smoke reset, any full-activity refusal, hidden output and outstanding
reservation refusal, semantic-manifest refusal, fork and second-restart refusal,
all six explicit crash boundaries plus a failure during evidence copying,
exact endpoint-transition admission and cross-provider rejection, publisher
runtime drift, internal source-mutator rejection, source/target artifact
tampering, ledger rollback, and a subsequent one-hop full infrastructure
supersession that retains qualification history.

The warning-as-error harness suite passes all 111 tests. A read-only check
against the real frozen source campaign and current clean LeanZero checkout
accepts exactly the publisher transition and the complete qualification
candidate without reading provider credentials, making provider calls, or
mutating either campaign root.
