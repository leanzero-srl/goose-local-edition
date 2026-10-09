# Forge 1.0 comment rows: b_comment_exactly_once, u_comment_flow, u_modal_close

Read-only audit, 2026-10-09. Nothing was scored, launched or edited. The only commands run were ffmpeg frame grabs
(niced, one thread) from the published graded-session videos on the Sanity CDN, plus small python/grep reads.

Parity: the installed copies of `score_forge.py`, `forge_probe.mjs`, `forge_site.py`, `forge_oracle.py` and
`forge/kit/lib/bridge-page.cjs` in `/Applications/Goose Swarm.app/Contents/Resources/swarm-bench/` are
byte-identical to the repo copies (cmp).

## 1. What the contract says (FORGE-CONTRACT.md §5, the version the entrants received)

> Clicking a row selects it (`aria-selected="true"`). `[data-testid="post-summary"]` posts one comment on the
> selected change's issue, authored by the viewer, in Atlassian Document Format, naming the issue key, the sprint
> name and the sprint's creep; then a success flag. One click, or a double click, posts exactly one comment. A
> failure shows an error flag and leaves the modal working.
> `[data-testid="close"]` closes the modal.

The contract does not say what a second gesture on the same row should do after a summary was already posted, and
it does not tell the entrant that the double click comes about 1.3 s after a single click on the same row. Nothing
in STARTER.md or BROWSER-TESTING.md mentions flags or how the double click is sent.

DESIGN.md defines the critical more narrowly than the code does:
- §8 row table: "`b_comment_exactly_once` | B | double click → exactly one comment | site comments | ≥ 1 comment POST | C"
- §17.5 E: "dropped the comment-path 429 (contract §5, §5.2 faults, `b_comment_exactly_once` = double click only)"
- criticals table: consequence "duplicate side effect on a customer's Jira", cliff 0 (×0.6).

## 2. What the probe does (forge_probe.mjs)

- 763-767: the probe opens a fresh surface (`reopen()`), then picks `target = rows.find((r) => !forbidden.has(r.cells.issue))`.
  `forbidden` holds only the issues in `commentForbiddenFor` (641). Issues hidden from the viewer are not excluded.
- 770: it selects that row.
- 775-782: on the same page, with the same row still selected, it runs `post` (one `l.click()`) and then
  `doubleClick` (`h.click(); h.click({force:true})` on one element handle). Each run goes through `settle()`
  (698-706: returns once bridge ops and comment count stay still for 250 ms, after at least 1.25 s). So the double
  click is NOT measured from a fresh state. Measured gaps between the post and the double click: Hy4 1.26 s
  (t 2001.72 → 2002.98), Terra 1.30 s (2016.574 → 2017.878).
- 696: `commentsNow` counts site log entries for `POST …/comment` with status 200 or 201.
- 689-693: a flag counts as success when `appearance ?? type` equals `'success'` (the §17.6 fix). This reads flags
  correctly. In every tree checked, a flag the app showed through the bridge was counted.
- Every click ends in `.catch(() => {})` (722, 729, 758, 779, 789, 801, 931). The probe sets no default timeout,
  so Playwright's 30 s actionability timeout applies, and a click that never lands is recorded the same way as an
  app that did nothing.

## 3. What the scorer does (score_forge.py)

- 2233-2238 `_post_scenarios`: both `post` and `doubleClick`, for every sprint.
- 2241-2251 `b_comment_exactly_once`: fails if `commentsAdded != 1 or successFlags != 1` in ANY scenario. That
  covers the single click as well as the double click, a missing success flag as well as a duplicate, and zero
  comments as well as two. The pass text claims "(the 429-once fault included)", but no comment-path fault is armed:
  Haiku's 7 comment records all have `fault: null`, and DESIGN §17.5 E dropped that fault. The probe's video
  caption at forge_probe.mjs:844 ("comment post through the 429 retry") is stale in the same way.
- 2424-2438 `u_comment_flow`: per sprint it checks select, `post.successFlags == 1`, the forbidden post's error
  flag and that sorting still works afterwards.
- 2441-2446 `u_modal_close`: `close.closeCalled` per sprint.
- 3083-3089 `_run_check`: when a section failed because of the app's own broken manifest, the row returns a plain
  `g(0.0, …)` with no `vacuous_root`. So the "cannot pass on nothing" criticals are charged as cliffs on an app
  that never opened (see solar-mini4 below).

## 4. The emulator's flags: a layer real Forge does not have (forge/kit/lib/bridge-page.cjs:35-66)

`showFlag` puts `<forge-host-flags>` inside the APP'S OWN DOCUMENT with
`position:fixed;left:16px;bottom:16px;z-index:2147483647`, stacks every flag vertically, never auto-dismisses
(`isAutoDismiss` is ignored, and `close` runs only on an explicit `closeFlag`), and has no `pointer-events:none`.
On Jira the flag group is host chrome outside the app's iframe, and success flags auto-dismiss. In the emulator the
flags cover the app's bottom-left corner and take its clicks. Playwright's hit-target check then retries the click
for 30 s, and the probe swallows the TimeoutError. Nothing reads the flag DOM (grep for `forge-host`: only
bridge-page.cjs), so `pointer-events:none` would change no grading input.

## 5. Per-model verdicts (19 of 31 fail the row; only 6 are actually charged its ×0.6)

Of the 19, 12 are vacuous (no comment POST; suppressed, factor 1) and ling-3.0-flash is suppressed by
root:l_bundles_load. The six charged are Sonnet 5.5, Omni Flash, Hy4, GLM-5.3 FlashX, Terra and solar-mini4.

### anthropic/claude-sonnet-5.5: SCORER/EMULATOR DEFECT (proven)
Detail: `doubleClick@931: 0 comment(s), 0 success flag(s)`, `doubleClick@1167: …`. post@931, post@1167 and sprint
963 all pass. Also u_modal_close 0/3 (worst seed 568016f05bc86724), u_comment_flow 8/10, u_llm_explain 0/5 with
no LLM call at all (k_llm_model_current vacuous).
Trace from the published graded video (`file-4d50aab8…webm`) and contact sheet:
- Sonnet's Post summary / Explain creep / Close row sits BELOW the table, at the left (contact sheet, sprint 963).
  Sprints 931 and 1167 have long tables, so that row is the last thing in the document.
- Frame at 284 s: sprint 931's post surface. Frame at 300 s: the page is scrolled to the end and the green
  "Summary posted" flag from the single post sits exactly over the button row (page y ≈ 540–584, x 16–307).
- The 931 surface then stays frozen from about 286 s to about 535 s, roughly 250 s. That matches about eight
  intercepted clicks at Playwright's 30 s timeout (the double click's first click, the forbidden post, 5 explain
  clicks, close). Because the button row is at the end of the document, no scroll alignment can lift it clear of
  a fixed bottom overlay.
- Control on the same app: on sprint 963 (short table, buttons above the flag zone) the double click lands. The frame
  at 590 s shows two "Summary posted" flags, and the row passes. So the app does not disable posting after a post.
  The only thing that differs is the overlay geometry.
- This also explains the other symptoms. explainSteps runs once, on the first sprint's post surface (probe 799),
  which is 931, whose Explain button is covered, so there is no LLM call. Close is covered too, so u_modal_close
  fails. The forbidden post is covered, so there is no error flag and u_comment_flow is 8/10.
Refutation tried: "the button is disabled after posting" is contradicted by 963. "The app's handler drops a quick
second click" is contradicted because Close and Explain fail on the same surface.
Impact: 0.5508 → at least 0.918 (the pre-severity score) from removing the false critical alone. u_modal_close,
u_comment_flow, u_llm_explain and k_llm_model_current are depressed by the same artefact, so the true score needs a
rescore after the fix.

### qwen/qwen3.8-omni-flash: SCORER (test-design) DEFECT; the app's second flag text is imprecise
Detail: `doubleClick@725/1005/1139: 0 comment(s), 1 success flag(s)`. Every single post passes, and u_comment_flow
is 10/10.
- b_comment_adf_as_user is "3/3 comments": the whole run has exactly 3 non-refused comment POSTs, the three single
  posts. So the double click sent no POST at all. This is not a late comment that landed outside settle.
- Video (`file-5b7c2366…webm`), frame at 50 s: two identical green flags "Summary posted / Added as a comment on
  AUTH-121." So the app answered the second gesture, 1.3 s after posting the same summary for the same change, with
  "already done" instead of a second comment. That is a dedupe/idempotency design, possibly a time window.
- The overlay is not involved: Omni's buttons are at the top (sprint-action screenshot).
- What is wrong: the probe measures the double click after a post of the identical summary on the identical row
  (probe 776), and the scorer charges the result as "duplicate side effect on a customer's Jira" when there is
  exactly ONE comment on the issue. An app that never posts at all (Luna) is vacuous and pays nothing, while an app
  that posts once and declines to duplicate pays ×0.6.
- Contract caveat: a strict reader can say "a double click posts exactly one comment" requires a second comment.
  The contract is silent on repeats, though, and the critical's stated purpose is duplicates. The flag text "Added
  as a comment" for the suppressed gesture is not literally true; that is a u_comment_flow-class finding, not a
  critical.
Impact: 0.5529 → 0.9214 if the double click is measured on a fresh row.

### tencent/hy4-preview: APP DEFECT (missing flag); the CRITICAL is over-applied
Detail: `post@930: 1 comment(s), 0 success flag(s)`, `doubleClick@930: 1 comment(s), 0 success flag(s)` and the
same on every sprint. Comment counts are correct everywhere: the in-flight guard `De=!0,Me.disabled=!0` works.
- Hy4's flag helper (static/sprint-ledger/build/app.js, around offset 102758) is
  `Ce=q=>{…try{(0,Ae.showFlag)({title:ne,description:pe,appearance:ye,type:ye,isAutoDismiss:ye!=="error"})}catch{}}`.
  The callers pass an `id`, but Ce drops it. The bundled @forge/bridge (offset 73901) does
  `if(!e.id)throw new BridgeAPIError('"id" must be defined in flag options')`, and the empty catch swallows that.
  The same thing would happen on real Forge: no flag is ever shown. The observations contain 0 showFlag ops and the
  forbidden post has 0 error flags.
- So the flag defect is real and u_comment_flow 5/10 charges it correctly. But b_comment_exactly_once is at 0 with
  every gesture producing exactly one comment, priced as "duplicate side effect". That contradicts DESIGN §8 ("double
  click → exactly one comment") and §17.5 ("double click only").
Impact: 0.4796 → 0.7993 with the flag removed from the critical.

### z-ai/glm-5.3-flashx: PROBE ARTEFACT on this row (the underlying leak is a real, separately charged APP DEFECT)
Detail: worst seed 133ce75a81764f1a, `post@749: 0 comment(s), 0 success flag(s)` and the double click the same.
The other two seeds are 1.0.
- Sprint 749's first table row is changeId 85610 / SEC-449, with hidden-count shown as 0. b_no_permission_leak lists
  `sprint-action-749-light-800x600: SEC-449` and `data-change-id 85610`. So the app leaked a hidden issue, and the
  probe chose it as the target because 767 excludes only `commentForbiddenFor`.
- The site answers the POST as the viewer with 404 "Issue does not exist or you do not have permission to see it"
  (forge/site/rest/platform.cjs:203-205, `!c.canBrowse(iss)`). The app sent one POST per gesture (2 attempts in
  total) and showed an error flag (errorFlags 1). That is exactly the contract's failure path.
- Whether this row fails depends on whether a leaked row happens to sort first (seed luck), and it charges a second
  ×0.6 for the same defect: multiplier 0.36 instead of 0.6.
Impact: 0.2963 → 0.4939.

### ~openai/gpt-terra-latest: APP DEFECT (real duplicate and no flag)
Detail: post 1 comment / 0 flags, double click 2 comments / 0 flags, on every sprint.
- static/sprint-action/build/app-src.js:6 is
  `async function post(){const x=await invoke('comment',…);draw(x.error?'<div class="flag error">…':'<div class="flag">Summary posted.</div>')}`.
  It has no in-flight guard and no bridge showFlag (the "flag" is an in-page div).
- The site log shows two 201 POSTs 2 ms apart for each double click (19111 @17.878/17.878, 19468 @35.829/35.831,
  19403 @43.390/43.392). This is a genuine duplicate. The scorer is right.

### upstage/solar-mini4: SCORER DEFECT (vacuity bypass), negligible rank impact
Detail: "the app's manifest names resource 'widget' … so the surface cannot be opened". Not one comment was posted,
yet criticalRows charge b_comment_exactly_once (×0.6) and b_no_permission_leak (×0.6) for it. ling-3.0-flash has
the identical manifest defect and escapes only because root:l_bundles_load suppresses them. The cause is
score_forge.py:3087-3089: `candidate_section_fault` → `g(0.0, …)` with no `vacuous_root`.
Impact: multiplier 0.0778 → 0.216; score 0.0016 → about 0.0043.

### The 12 vacuous rows and ling-3.0-flash
Correctly suppressed (factor 1). They are not defects of this row.

### Passing controls
Haiku, Pareto, Qwen3.8 Flash, Grok 4.7, GLM-5.3 Prime and Ember-1 pass (local trees). Their buttons are at the top
or their flags never cover them.

### u_modal_close on xiaomi/mimo-v2.6-flash (2/3 on every seed): EMULATOR DEFECT
Video (`file-a0c208b7…webm`), frame at 118 s: on the first sprint's post surface (958, where the 5 explain steps
also run) eight never-dismissed flags are stacked: Summary posted ×2, Comment not posted, Explanation ready ×2,
Explanation unavailable ×3. They fill the left column x 16–408 over the whole 600 px height. MiMo's Close button
(page x ≈ 285–340, top button row) sits under that column at every scroll position. On the other two sprints only
2 flags appear and Close lands: 2/3.

## 6. Fixes

1. forge/kit/lib/bridge-page.cjs:44: add `pointer-events:none` to `:host` of `forge-host-flags`. Flags are graded
   from the bridge log, never from the DOM, and they still appear in the video. Optionally honour `isAutoDismiss`.
   Then re-probe Sonnet and MiMo. This file also ships in the entrant dev kit; the change touches no contract text.
2. forge_probe.mjs: (a) send the double click on a DIFFERENT row whose issue was not commented on in this run,
   preferably on a freshly reopened surface. (b) At 767, choose targets only among issues the viewer can browse
   (`pack.issues[].hiddenFrom` does not include the viewer) and may comment on. (c) Stop swallowing click
   TimeoutErrors silently (gate 1): record `{clickFailed: <playwright message>}` and let the scorer mark a click the
   harness could not deliver as unavailable, not as app evidence. The "intercepts pointer events" message names
   the culprit.
3. score_forge.py:2246-2247: grade what DESIGN §8 and §17.5 define. Fail on `commentsAdded > 1` (a duplicate), plus
   `commentsAdded == 0` only when a non-refused POST attempt failed (this keeps the m_string_comment mutant's
   expected loss). Move the success-flag check, including the double click's flag, into u_comment_flow
   (non-critical). Update the forge_controls mutant expectations in the same commit.
4. score_forge.py:3087-3089: give manifest-fault rows of the "cannot pass on nothing" criticals
   (t_no_double_count, b_no_permission_leak, b_comment_exactly_once) `parts={'vacuous_root': 'manifest: …'}`.
5. score_forge.py:2250-2251 and forge_probe.mjs:844: drop the "429-once fault" claims (no comment-path fault is
   armed).

## 7. Confidence

- Sonnet (emulator overlay): HIGH. Video frames, a same-app control (963), and the frozen span matching the 30 s
  timeouts.
- MiMo close: HIGH. The frame shows the flag column over the Close position.
- Hy4 app flag defect: HIGH (the bridge source in its own bundle). That the critical is over-applied: HIGH against
  DESIGN's own definition.
- GLM-5.3 FlashX row artefact: HIGH (SEC-449 is in its own leak list; the 404 comes from canBrowse).
- Terra real: HIGH.
- Omni Flash: MEDIUM. The mechanism (no POST, success flag) is proven. Whether a dedupe that declines a second
  identical summary violates "a double click posts exactly one comment" is a contract-reading call; charging it as
  a duplicate is wrong either way.
- solar-mini4: HIGH, immaterial to rank.

Not verified: the Sonnet and Omni app sources (their trees are not on this machine), so the claims for those two
rest on the published videos, screenshots and row details plus the probe and emulator code, not on their handlers.
