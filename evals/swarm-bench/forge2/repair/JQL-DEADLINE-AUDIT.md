# Forge 2.0 shared freshness deadline audit

The public contract already assigns the same recovery guarantee to the custom field (§15) and native JQL precomputations (§19). A lost event, sprint close or estimation-field switch has no immediate event signal and is compared one hour later. The existing custom-field scorer also carries that no-event recovery set once when its scheduled healing instant was inside the site's native quota wall. The JQL scorer previously ignored this guarantee.

The common Oracle now returns bounded per-issue lower bounds from the existing field rule. Custom fields consume those issue IDs as before. Native JQL ignores only membership first introduced by changes still inside that bounded window. An older change of the same issue continues to require membership. Native search success, stable precomputation identity, cache reuse, duplicates, deletion and current permissions are checked independently. Exhausting the app's own 70% background allocation does not create or extend a recovery window.

## Motivating frozen trace

The retained v4 reference seed `c299c14673c45358` upgraded at `1791623493485`; its h4 checkpoint was `1791637893485`. Issue 14772's change 1103866 happened at `1791637600311`, moving it from future sprint 1355 to active sprint 808. Its event was deliberately dropped. It was absent from the ledger at h4 and present at h5. Treating its newly introduced sprint-808 membership as immediately required at h4 contradicted the published common deadline.

Issue 16944's delivered change 1103848 happened at `1791637596046`, moving it from sprint 808 to 1331. The ledger committed its rows at `1791637743404` and `1791637744084`. Cached precomputations remained older: the last sprint-808 update was `1791637702764`, and the last sprint-1331 update was `1791637690724`. The refresh queued at `1791637751524` retried after the app's own background allocation was exhausted and succeeded only at `1791641106784`. The affected clock hour used exactly 1680 background points and had no native quota refusal. This remains a real reference defect.

Free re-reading of the original observations with the corrected Oracle removes 14772 from the diagnostic h4 differences and keeps 16944 as missing from IN and extra in NOT IN for all viewers. The original aggregate results remain 16/24 refresh cases and 58/72 shared-permission completeness cases, with no disclosure observed. The complete readback is retained at `/private/tmp/forge2-reference-qualification-v4/seed4-jql-deadline-readback.json`; the ordered native calls are at `seed4-jql-ordered-calls.json` alongside it. This correction does not make the failing reference pass.

## Free controls

`test_forge2_freshness_deadlines.py` exercises real changelog/world Oracles: lost-event IN and NOT IN at h4; the same stale membership rejected at h5; older membership of that same issue; exactly one additional native-wall carry; no carry at a reset boundary; delivered events under a wall; own-allocation exhaustion; hidden and deleted issue IDs; duplicate results; broken cache identity and native transport. Existing R7 selftests and R10 mutation controls also pass. No model call, candidate build or full browser score was needed.

The reference repair remains separate. Atlassian's current JQL function guide supplies `precomputationId` to the app's initial evaluation, explicitly permits storing it for later updates and requires shared argument-level precomputations rather than user-specific fragments. The REST API permits one update containing `values: [{id, value}, ...]`; the reference can persist these native IDs and coalesce relevant updates without enumerating every stored precomputation for each event. Quota limits and deadlines remain unchanged.

Primary sources: <https://developer.atlassian.com/cloud/jira/platform/jql-functions/> and <https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-jql-functions--apps-/> (checked 2026-10-11).
