# First-writer source attribution, 2026-10-11

This is a scorer repair and pure archived-evidence replay, not native qualification, calibration,
release or publication evidence. Frozen native results remain unchanged. No native runtime,
browser, model, Cargo, package installation, runtime cache or candidate app was launched or edited.

## Cause and single decision

Frozen source `6351e5b0dbd51ec5fb855ac581c8b0a737a854ba` used wall, hourly-redelivery and
scheduled-lineage timing to admit reconciliation. It did not identify the path that recorded the
row first. Public FORGE2-CONTRACT.md §3 requires that first path; event work may also record other
changes of the issue it reads. An event origin or an app's `kind: reconcile` payload proves neither
path alone.

`bench/score_forge2.py:sources_for` now consults a lazily cached native first-write proof before those
existing timing allowances. The proof finds the earliest mutation of the logical change/Sprint,
including earlier malformed mutations at the same stored key. It requires successful exact Jira
history evidence: issue, changelog id, created instant, author and Sprint movement, read before that
write. Reconciliation needs actual broader issue collection coverage in one returned page or its
connected native cursor walk. Separate bulk/per-issue history reads can complete that collection
evidence. Single-issue event reads may record another change of the platform origin's own issue.
Candidate function names, job labels, cursor key names and asserted sources are not proof.

Queue ancestors join only a unique observed successful publication's job id and indexed delivery
event id, with timestamps and copied platform lineage. An ancestor's page must have been read before
it published the continuation; unrelated reads and reused job ids lend no credit. A cached native
parent page is supported. Source labels never choose the proved path: a conclusive first event or
reconciliation path returns a singleton, so a wrong initial label or later relabel remains wrong,
even under wall/scheduled timing allowances. Metadata updates can retain that original source.
Structured ledger rows and UI cells must use their explicit string `source` attribute; an unrelated
declared string attribute such as `deployedEnvs` cannot impersonate that source. Plain-set legacy
selftest inputs retain their existing interpretation. Known failed malformed-key requests are
ignored, while unknown earlier logical-target mutations remain conservative.

Single entity/set and successful transaction sets are supported. Batch HTTP 200 is insufficient:
only known per-item success can prove a first write, and unknown earlier batches block proof. The
current proxy omits KVS batch response bodies, so native batches have no new positive credit.
Malformed first rows, conflicting first timestamps, untimed target mutations, target deletes or
possible deletes/recreations, TTL first writes and unjoined ancestry cannot prove a first path.
Migrated rows keep v1's source. When the actual path remains unproved, existing oracle/timing
allowances remain intact. Arbitrary app-internal path names cannot all be identified from traces.

## Whole-path audit

Read the attribution context and helpers, every source consumer, pagination walkers and outcome
helpers, economy/cache order and composition bodies. All six current source consumers use the same
decision: t_event_rows, t_out_of_order, t_multi_sprint_parse, r_heal_dropped,
_reconcile_outcome_complete and u_ledger_table/cells_ok. The outcome helper indirectly feeds
_superseded_walks/r_pagination. No separate source allow-list was added to a consumer.

Also read the actual installed @forge/kvs storage-api and transaction-request-builder bodies,
forge2_probe.mjs normCall/lineage/takeCalls, Oracle source assignment, proxy product/KVS records,
KVS entity/transaction/batch bodies and emulator publication, invocation and delivery bodies.
The independent primary review is `/private/tmp/forge2-source-attribution-review/REVIEW.md`
(committed review HEAD `20affe0293cc66169e2cb2bbbaa191db0b98fecf`) and the typed third-site review is
`/private/tmp/forge2-independent-qualification/FINAL-6351e5b0d-THIRD-SITE-PRIMARY-REVIEW-v2.json`.

## Causal trace: would this have changed the motivating outcome?

YES, at native consumer `inv-00970-46320e`, seed `d031c3ee22136508`, target `(1357449,795)`.
The change removed issue `25104` / `WEB-159` at `1791902336588 ms`.

At `1791903722.815`, successful asApp POST `/rest/api/3/search/jql` returned 25 issue ids including
issue25104, exact changelog1357449, its author/created and Sprint field18703 move795→1383.
At `1791903746.155`, the same consumer's entity/set first wrote row1357449:795, source=reconcile,
FAIL_IF_EXISTS, status204. Exactly one successful exact-target write exists in the archived
background trace. Its own event's first successful delivery began later, at `1791903830.315`.
The existing wall, next-hour redelivery and scheduled-work allowances were all false. The new
earliest-mutation → exact history → broader collection branch returns `('reconcile',)`.
Live/heal/rerun snapshots retain that source. The sanitized two-call test fixture retains native
ids/instants while removing author identity/URLs and the large JQL issue list.

Pure replay of all five registered row consumers plus the indirect reconciliation outcome gives
t_event_rows **0.9971→1.0** (173/174→174/174), u_ledger_table **0.9977→1.0** (5.99/6→6/6).
The other three registered consumers stay1.0; _reconcile_outcome_complete **false→true**.
Functional rows populate the cache before E rows: reevaluating e_event_economy then gives
**0→0.4875**, from the unchanged **1185 requests / baseline202** (201 relevant changes + one forced
429 repeat), at the unchanged fitted top2.86. No threshold, weight or economy policy was changed.
Exactly those three stored rows change; every other stored row is equal. Derived composition:
**0.9969**, inner**0.9969**, crit_mult**1.0**, unsuppressed criticals**[]**, reliability**1.0**.
The original native score**0.9935** remains the recorded native result.

Additional pure archived controls: common-site-1 seed3c04f61f28a4a511 has no changed rows,
native/derived score0.9870, inner0.9953, crit_mult1.0, unsuppressed[]. Common-site-2 seed3aaa6d6b73d37669
has no changed rows, native/derived score0.9765, inner0.9930, crit_mult1.0, unsuppressed[].
No repeat output was inspected. These are stored-observation replays, not new site runs.

The third site's immutable observations SHA256 remained
`40bc26761756038a7302c958ef27d4adbce31b83dd15baaa6e1e43ffc194df27`; verdict SHA256 remained
`eb6e93d85ffb789bb5fb41bd988f050a746f1bc157c8ffe24b5b7e9d71214ce5`.
All three sites' receipt, verdict, tested-source and observations hashes matched before/after replay.

## Tests and independent review

The actual bundled Python was used with `-B`:
`~/Library/Application Support/Goose/benchmark/runtime/python/bin/python3`.
Focused first-source controls cover read/write failures, exact identity/history mismatches, trigger
or user work, app-only job assertions, direct own-issue work and same-issue other changes,
separate history reads, skipped/unrelated collection pages, parent-page reuse, queue uniqueness/
timing/lineage, transactions, known partial batches, unknown earlier batches/failed transactions,
malformed earlier writes, same-time conflicts, deletion/recreation, initial wrong labels, later
wrong labels under timing allowances, source-preserving metadata and economy cache propagation.
Final focused first-source controls passed23 tests, including positive and wrong-source cases
through all six actual consumers. Existing pagination, compound repair and scorer-control suites
passed90 tests: combined113 tests. --selftest passed all105 registered rows. Edited Python AST
parsing and git diff --check passed.

Independent review `/private/tmp/forge2-two-fix-review/REVIEW.md` and `SOURCE-RECEIPT.json`, committed
as proof commit `f1b1e89d7d5c13689721620658f405a487f01c55` and final review HEAD
`a61b0d340c96be36a25df5a6d5bd6a9f765235bb`, returned PASS on scorer SHA256
`d44c512dfd72272aaf0ad2a6c475410b1d1d0ed3ef212d2d63b4aaf0d313f01c`.
The reviewer independently reproduced and confirmed four intermediate defects, all repaired before
this commit: initial wrong labels falling back to the oracle, failed object keys crashing scoring,
bulk-only multi-issue reads granting an event singleton, and unrelated declared metadata lending
source credit. It independently replayed the actual third site and all six source consumers,
confirming exactly the three row changes and the real1185/202 economy. Source bytes still match that
reviewed hash. The final independent run also passed23 focused tests and105 selftest rows;
SOURCE-RECEIPT.json SHA256 is `a9c63360a9679bac6b58db2b623be309b380c128c16ce48982a31d6fa072a578`.
Final three-site pure replay output is retained at
`/private/tmp/forge2-source-path-final-pure-replay.json`.

## Separate KVS risk, independently reproduced

Only actual createKvs was invoked in memory. A transaction set `first={n:2}` before a later invalid
integer returned400 INCORRECT_PROPERTY_TYPE with first still persisted. A later FAIL_IF_EXISTS
conflict returned409 KEY_CONFLICT with first still persisted. This violates atomicity and belongs
to the orchestrator's separate KVS cut. Failed transaction target mutations remain UNKNOWN in this
scorer even after that cut, because older archived traces carried the defect. The motivating native
first write was successful entity/set204 and is unaffected by this separate issue.

Confidence is high for the measured case and covered native trace shapes. Fresh native three-site
plus repeat qualification and calibration/release remain required outside this isolated scorer cut.
