# Forge 1.0 scorer audit: `u_llm_explain` / `k_llm_model_current` and `u_widget_live`

Read-only. The installed bench files (`score_forge.py`, `forge_probe.mjs`, `forge_site.py`, `forge_oracle.py`, `forge_kit.py`) are
byte-identical to the repo (cmp). The kit lib each app ran against (`forge-kit/825be630e817fabf/kit-ad1a414ede060b2e/lib`) is
identical to `forge/kit/lib` (diff -rq). Oracle numbers below were recomputed with `forge_oracle.Oracle` over
`fixtures.cjs --seed <run seed> --scoring` packs (`llm-live/live.py`, `llm-live/pack-*.json`). Each recomputation matches the
recorded "right k/2" in the verdict. Video frames were cut from the runs' own raw recordings (`llm-live/*.png`, `qwen-post-*.png`).

## 1. The LLM rows

### How the probe and scorer decide
- Probe `forge_probe.mjs:923-958` `explainSteps`: `await button.click().catch(() => {})` (931), then it counts the site LLM log
  entries with `op` chat|stream since the click (933). A click that fails is swallowed, so it looks like "the app made no call".
- Scorer `score_forge.py:2645-2648`: `llmCalls != 1` gives `one_llm_call_answered_<kind>` = False, and `continue`.
- `k_llm_model_current` precondition `bool(_llm_calls(c))` (2510). With zero calls it is `vacuous` and scores 0.
- Interception fidelity is good. The kit runs the real `@forge/llm` 1.0.7 and catches it at the transport (`shim.cjs:19`
  `/llm/<model>`, `proxy.cjs:156-169` passes it to `site/llm.cjs`, which logs every list/chat/stream). Any import style or API
  shape the real package accepts reaches the log. One exception: without an `llm` module the proxy refuses with 403 BEFORE the
  site log (`proxy.cjs:161-167`), so a refused call is not counted.

### Qwen3.8 Flash (d37e8678): SCORER DEFECT (kit + probe), high confidence
- The app is correct as written. `src/resolver.js:109-158` calls `list()`, takes the first active model, and calls `chat()`
  with tool `report_scope` and a forced `tool_choice: {type:'function', function:{name:'report_scope'}}`. The frontend
  (`static/sprint-action/src/index.jsx:109-119,190`) invokes `explain` on click. The build has the same code (checked by grep
  of `build/index.js`).
- Recorded evidence: `ui.invokeResponses` holds NO `explain` invoke at all. On the post surface s12 there is
  `sprintData` 1 and `postSummary` 2. The forbidden post also produced no invoke (`forbidden: commentsAdded 0, errorFlags 0`).
  All 5 explain steps show `llmCalls 0, explanation ""`. `llm.entries` is empty, not even a `list`.
- Timing: the post surface lasted 195.2 s (media-manifest segment). Haiku's lasted 14.4 s. That is six default 30 s
  Playwright actionability timeouts: the forbidden post plus 5 explain clicks.
- Cause, seen directly in frames at 4 s, 8 s and 120 s of that surface (`qwen-post-*.png`): two kit host flags, "Scope Ledger /
  Comment posted on CRM-385.", sit over "Post summary" and "Explain creep". The forbidden row CRM-439 is highlighted, so the
  select click landed and the button clicks did not.
  `bridge-page.cjs:39-44` appends `<forge-host-flags>` INSIDE the app's own document:
  `position:fixed; left:16px; bottom:16px; z-index:2147483647`. The flags never dismiss: there is no close control and
  `isAutoDismiss` is ignored. Close still worked (`closeCalled: true`) because its centre (x≈318) is right of the flag edge
  (x≈308).
- Refuting "busy stuck": `postSummary` always runs `setBusy(false)` after its `try`. Both postSummary invokes resolved, and
  their success flags were counted. So the button was enabled. It was covered.
- Real Forge renders flags in the Jira host page, outside the Custom UI iframe, and they carry a dismiss control. The contract
  says nothing about flags covering the surface. Haiku only escaped because its buttons are full width (frame
  `llm-live/haiku-post-8s.png`: five flags stacked, the button centres at x≈400).
- Collateral: `u_comment_flow` 6/8 (forbidden error flag, modal works) has the same cause.
- Score effect: inner would rise from 0.9537 to about 0.98. The final 0.7961 is pinned by the band-3 `v_dark_mode` cap.

### Claude Sonnet 5.5: UNCLEAR, most likely the same SCORER DEFECT (tree not on this machine)
- Same signature on the board. The first post succeeded with a flag. Then: `b_comment_exactly_once 0 ['doubleClick@931: 0
  comment(s), 0 success flag(s)', ...]` (a CRITICAL row, multiplier 0.6), `u_modal_close 0/3`, explain 0 calls.
- The published video frame (`media/s/f300.png`) shows a kit "Summary posted" flag over the bottom-left, where the action bar
  sits.
- If confirmed, Sonnet's 0.5508 is mostly manufactured. Without the multiplier and the 3 band-4 rows, pre-severity is
  about 0.92 or more.

### Hy4 preview (84b93386): APP DEFECT, certain
- `src/index.js:707` `await activeModel()` and `:714` `askModel(model, messages)`. Neither is defined anywhere (grep finds only
  these call sites). `import llm from '@forge/llm'` (line 6) is never used.
- All 5 explain invokes returned `{'error': 'activeModel is not defined'}` (`invokeResponses`, s12).
- Separately, its flags are lost because `showFlag` is called without `id` (`ui/sprint-ledger.js:179`). Bridge 7.1.0
  `flag.js` throws `'"id" must be defined in flag options'`, so this is also an app defect.

### GPT-6 Luna: APP DEFECT, likely (no tree)
- The board shows `k_manifest_semantics` M13 failing: the source imports `@forge/llm` and the manifest declares no `llm`
  module. Contract §2 requires `llm` (`model: [claude]`).
- The proxy refuses with 403 before the site log, so nothing can reach the script. Whether the button reached the call is
  unverifiable.

### Pricing of one missing call
- Band 4 prices it once. `ROOT_BLOCKS['k_llm_model_current'] = ('u_llm_explain',)` (`score_forge.py:3010`) and `band_defects`
  (3244-3252) make it a shadow. Qwen's verdict shows `priced_once: ['u_llm_explain']`. This is what DESIGN §8.5:560-565
  intends ("an unknown model id fails `k_llm_model_current` and every explain step — one defect").
- Points are charged twice: K-tier 0.1/10 = 0.010 and U-tier 0.16/11 = 0.0145 of inner, plus the K row's share in the
  excellence fraction (`excellence()` includes every K row). This is consistent with DESIGN ("vacuous_root rows price 0"), so
  it is by design.
- Luna gets a third charge. M13's `graded_by=_charged(c,'k_llm_model_current')` (1305-1307) ignores vacuous rows
  (`_row_score` returns None, line 1164), so M13 is priced again in `k_manifest_semantics`. This is a minor triple count of
  one manifest omission.

## 2. `u_widget_live` = exactly 0.75

### The check
- `score_forge.py:2563-2580`: the mean of 4 equal conditions:
  - `subscribed` (any `subscribeRealtimeChannel` op)
  - `no_polling_while_idle`
  - `no_reload`
  - `shows_new_numbers` = `moved non-empty AND moved ⊆ right AND len(right) == len(want)`, where `want` is ALL active
    sprints of the board (`sprints_of_board`).
- Probe `forge_probe.mjs:868-896`: it idles 8 s, delivers both live-UI changes and drains, polls up to 15 s for any metric
  change, then reads again 500 ms later.
- Every app that subscribes and does not poll gets 3/4 automatically. Any failure of the single outcome condition, whatever
  its cause, lands on exactly 0.75. So the clustering is the arithmetic, not a broken measurement. In all 8 local cases the
  failing condition is `shows_new_numbers`.
- Fixture guarantee (`fixtures.cjs:504-519`): both live changes move the first scrum board's sprints, so `moved` is never
  empty for a compliant app.

### Per model (oracle after the live slot, against what the widget showed)
| model | what happened | verdict |
|---|---|---|
| Qwen Flash | No change in 15 s. The consumer `publishGlobal`s (`src/core.js:334`; 23 consumer events, all `isGlobal: true`, `delivered: []`). The widget uses `realtime.subscribe(...)` (`static/widget/src/index.jsx:66`; all subscriptions `isGlobal: false`). `realtime.cjs:122-123` matches only `sub.isGlobal === e.isGlobal`, per the docs ("publishGlobal events can only be received by ... subscribeGlobal"). The contract the entrant saw (§4 Live) says "the bridge's realtime subscribe in the widget … a global publish reaches every subscription on its channel". | CONTRACT DEFECT (scorer side). The emulator is doc-faithful, but a literal contract reading fails. Medium-high confidence. |
| Grok 4.7 | No change. The consumer is declared `resolver: {function: consume-ledger, method: consume}` (manifest:41-46). That is valid: it is the FIRST `oneOf` alternative in the kit's own pinned `manifest-schema.json` (required `queue, resolver, key`), and lint showed 0 errors. `emulator.cjs:283` invokes `consumer.function` (undefined), giving 3564/3606 live invocations failing `no function 'undefined' in the manifest` (`emulator.cjs:219`). No consumer ever ran: `t_event_rows 0/22` and a band-2 cap of 0.699, which explains the 0.6912 final. | SCORER DEFECT (kit), high confidence. It masks a probable app bug: `consumeLedger` reads `request.body \|\| request`, while `@forge/resolver` hands `{payload, context}` (`out/index.js:31-36`), so `issueId` would be undefined on real Forge too. Medium confidence on that. |
| Terra | No change. `exports.consumer` loops `event.events \|\| []` (`src/index.js:167-169`). The async event carries `body`, so it ran OK 82× and ingested nothing. 0 consumer publishes, `t_event_rows 0/23`. The widget was already wrong before the live slot (committed 41.5 against the oracle's 71). | APP DEFECT |
| Hy4 | It updated (2 delivered events) but to wrong numbers. 930 shows 70/40.5 against the oracle's 65/45.5 (the joiner was counted as committed). 1102 shows committed 3 and creep 1750% against 0/55.5/—. | APP DEFECT |
| Ember-1 | The live update was CORRECT (779 went to 78.5/78/19/99.4%, equal to the oracle). It fails only because the widget never renders the board's second active sprint 1070, which `u_widget_numbers` (0.125) already charges. | APP DEFECT, but double-charged (see note) |
| GLM-5.3 Prime | The update fired, and 1246 reached the right values. But `[data-metric]` text is "Committed71" (the label sits inside the metric element; the contract says the text is the §1 number), and 1286 is rounded to integers (51/33 against 50.5/33.5). | APP DEFECT |
| MiMo Flash | No tree. Board: `u_widget_numbers 0/4 views exact`, `t_reestimate_followed 0`. | UNCLEAR, leaning app |
| Luna | No tree. Board: `r_backfill_complete 0/62`, `u_widget_numbers 0/4`. | APP DEFECT, likely |

### Notes and latent defects
- **Double charge.** `len(right) == len(want)` folds static widget-number defects into the live row. `ROOT_BLOCKS` would absorb
  it, but `u_widget_numbers` is outside band 4 ("a root outside the band never absorbs a band row"). So Ember and GLM pay the
  same numbers defect again as a band-4 defect. Fix: grade only `moved` sprints, comparing against before and after.
- **DESIGN and code disagree on polling.** DESIGN §8 (DESIGN.md:489) says "(polling scores 0)". The code averages the
  conditions, so `m_rt_poll_instead` (unsubscribed, polling, updated) scores 0.5. `test_score_forge.py:1144` asserts only
  `< 1`. Also, "never updated live" earns 0.75. Fix: `score = 0 if idleInvokes else shows_new_numbers`, with `subscribed` and
  `no_reload` as gates.
- **Latent probe race.** Not observed in the 8 trees: no recorded "after" state was an intermediate one.
  `forge_probe.mjs:886` breaks on the FIRST metric change, then waits 500 ms. If a widget refetches once per publish, and the
  second refetch, which runs in a fresh Node process, takes more than 500 ms, a correct app would be recorded in an
  intermediate state.
- **Contract §8 conflicts with the emulator.** "`@forge/realtime` works in every backend function, resolvers and async functions
  alike" invites `publish()` from the consumer, which the emulator rejects (`realtime.cjs:144-148`). It is the same class as
  the Qwen wording defect.

## Fixes
1. `forge/kit/lib/bridge-page.cjs:44`: add `pointer-events:none` to the flags `:host` rule. The probe counts flags from the
   bridge log (`flagCounts`, `forge_probe.mjs:688-692`), not the DOM, so grading is unchanged and shots still show the flags.
   Also honour `isAutoDismiss`.
2. `forge_probe.mjs:931,779,…`: record the click error instead of `.catch(() => {})`. If the click is intercepted by
   `forge-host-flags`, mark the step `harness_missing`, not an app failure.
3. `forge/kit/lib/emulator.cjs:128,283`: resolve `consumer.function ?? consumer.resolver?.function`. For the resolver form,
   invoke with `{call:{functionKey: consumer.resolver.method, payload: ev.body, jobId}, context}`, which is the
   `@forge/resolver` shape.
4. Contract §4 Live: "a `publishGlobal` reaches every `subscribeGlobal` on its channel; `publish` reaches `subscribe` in the
   same context". Also fix §8's realtime sentence.
5. `score_forge.py:2574-2578`: polling gives 0 per DESIGN, and `shows_new_numbers` is judged on the moved sprints only.
6. Re-score Qwen Flash, Sonnet 5.5 and Grok 4.7 after 1–3, and audit every board entry whose `u_comment_flow`,
   `u_modal_close`, `b_comment_exactly_once` or explain rows fail after a success flag.
