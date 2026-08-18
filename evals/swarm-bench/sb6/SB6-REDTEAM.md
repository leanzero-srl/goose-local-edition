# sb-6 RED TEAM — ranked findings

Grounded against the shipped harness: `bench/product_probe.mjs` (90s hard cap, per-scenario browser, viewport 1280×800, no scroll management), `bench/vendor_service.py` (one-shot traps, `mark_phase`/`begin_exercise_phase`, passive HTTP server), `bench/score_build.py` (`gather()` restarts the app for persistence checks), `bench/fixtures.py` (exports scores/constants, not raw measurements).

---

## SEV-1 — invalidates a whole check or gates the instrument shut

**F1. [GAMEABILITY+FAIRNESS] `t_picking` is vacuously passable and under-asserted — the highest-weight interaction check is a textContent grep.**
`pageStatusFilterValue()` reads `data-value || aria-label || textContent` and the scorer tests `got.includes(t.status)`. The spec never defines how a custom dropdown exposes its selected value. Attack: a dropdown whose collapsed markup contains all four option labels (hidden list inside `#status-filter` — a completely natural custom-dropdown DOM) passes **every** pick and the depth pair with a no-op click handler. Simultaneously, a correct app that renders the selection in a sibling element fails. And the spec's "the table refreshes to match" is never asserted at all.
*Amendment:* (a) spec pins `#status-filter` must carry `data-value="<status>"` reflecting the current selection (add one sentence; it's already the frozen-vocabulary style); (b) probe reads **only** `data-value`; (c) counter-assertion: after each pick, re-snapshot the table and assert rendered row statuses are uniformly the picked status AND the `showing X–Y of TOTAL` readout equals the fixture's per-status total. Without (c), picking is still only wired to a label, not to the product.

**F2. [FEASIBILITY] The half-seeded db for the re-based `j_sync_journey` cannot be built — the grader does not know the app's schema.**
The design says "probe boots the app on a half-seeded db," but the app owns its SQLite schema; `gather()` cannot write rows into an arbitrary layout. Since R13 makes this journey an E-gate member, an infeasible re-basing re-creates the exact sb-5 defect (probe-broken check locking the top slice).
*Amendment:* half-seed through the vendor, not the db: add a mock phase control (`mark_phase("halfset")` + a `STATE.visible_rows = N` cap) that serves only the first ~750 fixture rows; gather boots the app, runs sync #1 against the capped collection, flips the cap off, then the probe clicks `#sync-now` — rendered row count and `#summary` totals observably change. Deterministic, schema-agnostic, and it additionally exercises delta-sync for free.

**F3. [FEASIBILITY] Webhook grading collides with the one-shot/phase problem the mock itself already documents, and with `gather()`'s app restart.**
Three unresolved mechanics: (a) the forged event and the out-of-order sequence are one-shot — if deliveries fire at registration time, the agent's own dev-phase app consumes them (the exact seq-3-vs-seq-38 failure `begin_exercise_phase()` exists to prevent); (b) health counters are in-app, in-memory — `gather()` restarts the app for `restart_persistence`, wiping the quad, so `EXPECTED_WEBHOOK_COUNTERS` can only match for one specific ordering of gather steps that the design never fixes; (c) nothing specifies what triggers delivery — the passive mock must now make outbound HTTP calls on a grader-controlled schedule.
*Amendment:* the mock gains an admin trigger (`POST /admin/deliver-script`, localhost-only, undocumented in vendor_docs) that the grader calls **during the exercise phase, after the final app boot and re-registration**; spec adds one sentence: "the health counters count events received by this process since it started." Registration idempotence (same id/secret) makes re-registration after restart safe. Order in `gather()`: restart → re-register → trigger script → read `health2`. Freeze that ordering in code, not convention.

**F4. [FAIRNESS] The drag-frame floor contradicts the spec's own rendering model, and the scorer ladder doubles the spec.**
Spec mandates a **static** scene: "frames are drawn on load, on input, and on data change." The probe sends 20 move events over 2 s. A spec-perfect event-driven renderer draws ≤ 20 frames — below the 24-frame floor **by construction**. Worse, `e_frames_under_drag` computes `fps = frames/wall` and its top rung is `fps ≥ 24` — 48 frames per 2 s, double the spec's stated budget. A reference-perfect app lands on the 0.5 rung.
*Amendment:* pick one model and align all three artifacts: either (i) the probe drives ≥ 60 moves (e.g., `steps: 3` per move, 30 ms sleeps) and the floor becomes "≥ 0.8 frames per delivered move event," or (ii) the spec requires rAF-driven rendering while a pointer is captured (one sentence) and the floor stays fps-based. In either case the ladder unit must be the spec's unit (frames per drag, not fps), with rungs calibration-owned.

**F5. [FAIRNESS] The pixel checks forbid good design without saying so — sky/above/corner samples fail any app that draws a floor grid, axes, or count labels.**
The spec pins bar colors and clear color but never says the canvas may contain **nothing else**. A finance chart with a subtle floor plane, axis ticks, or a count label above tall bars — exactly what the operator's "intentional, designed UI" rules push toward — fails `above`, `sky`, and `corners_bg`, capping `t_scene_binding` and `t_context_real` for a correct, better-than-reference app.
*Amendment:* add the explicit spec sentence: "Draw nothing but the bars: no floor, grid, axes, in-canvas labels, or decorations — the background is bare `#0F172A`. Labeling lives in the tooltip and the 2D table." This is a deliberate taste trade the spec must own out loud; grading an unstated prohibition violates the mock's own fairness bar ("a careful engineer who reads the docs gets every one right").

**F6. [DETERMINISM] No scroll management and no rect re-measurement — every mouse interaction can silently land off-target.**
The page stacks header + summary + viz + table at 1280×800; `#viz3d` can sit below the fold. The probe computes click/hover coords from `getBoundingClientRect()` taken once (`pre`) and never scrolls the canvas into view. Playwright `mouse.*` dispatches at viewport coordinates — coords beyond y=800 hit nothing. Additionally, if the app does not `preventDefault()` wheel over the canvas (the spec never requires it), `mouse.wheel(0,400)` scrolls the page 400 px and every cached `rect.left/top` afterward is wrong: dblclick, drag, and reset all mis-land — an app with a perfect camera scores near 0 on `t_camera` and `t_picking`.
*Amendment:* (a) probe: `#viz3d.scrollIntoView({block:'center'})` before the interaction phase, then re-read the rect; re-read the rect again after the wheel step and before the drag; assert the rect is fully inside the viewport and emit `probeError` otherwise; (b) spec: one sentence — "zooming over the chart must not scroll the page." Both, not either.

**F7. [CALIBRATION] No gate requires the reference implementation to pass — the `view_refreshed` failure class can ship again, aimed straight at the E gate.**
G4 proves every check *fires* on the golden tree; nothing proves every check is *passable*. Concrete live risk: the E gate requires zero console errors "across every scenario incl. viz" — but the `error` scenario blocks API fetches and the `viz-fallback` scenario kills WebGL; Chromium logs failed fetches as console errors, so the gate may be structurally unpassable and the 0.12 slice dead on arrival — the exact sb-5 defect (27% of top-band loss) reproduced at 4× the weight.
*Amendment:* add **G6 (freeze-blocking):* the reference implementation, scored on the calibration machine, must score ≥ 0.95 on every non-calibration-owned check and must pass the E gate; any check it fails is a harness defect and blocks freeze. Independently, redefine E-gate console cleanliness as: uncaught exceptions + app-emitted `console.error` in **nominal** scenarios only (load/sync/empty/viz); network-layer errors in `error`/`viz-fallback` are expected traffic. This is the "gates, not memory" doctrine applied to the instrument itself.

---

## SEV-2 — a check materially mis-measures or is partially gameable

**F8. [FEASIBILITY+FAIRNESS+DETERMINISM] `e_under_load_latency`: the spec never requires concurrent serving, and the "during sync" window is a race.**
A good-faith stdlib reader can ship a single-threaded `HTTPServer` whose `POST /api/sync` handler blocks through the vendor's documented waits — 8 concurrent readers then measure the sync's wall time, not the API. Separately, on a fast build the sync may complete before readers ramp, so "under load" quietly measures an idle server — the P-tier vacuity returning in disguise.
*Amendment:* (a) spec sentence: "the API keeps answering reads while a sync is in flight"; (b) the mock's `--stall` trap deterministically holds the final sync page open for a fixed window (e.g., 6 s) during the measurement, guaranteeing overlap; (c) the measurement records the fraction of reader requests that actually overlapped the in-flight sync and **refuses** (harness error, not app zero) below a floor — an unproven negative must not license the credit.

**F9. [GAMEABILITY] `t_camera`'s reset anchor pays 0.35 to a completely static scene.**
An app that ignores all input renders the default view forever: wheel ≈ 0, drag ≈ 0, cam_ok false — but `reset` (re-sample baseline tops after dblclick) scores 1.0 because the scene never left baseline. 0.35 of the check for zero interaction code.
*Amendment:* gate reset credit on observed motion: `reset` counts only if (drag proj ≥ 0.5 OR wheel proj ≥ 0.5 OR a mid-drag framebuffer sample differs from baseline). Cheapest robust form: sample 5 baseline top positions once mid-drag; if they still all match baseline colors, the scene never moved → reset = 0.

**F10. [GAMEABILITY+DETERMINISM] `e_optimistic_paint` cannot distinguish optimistic from fast, and its 100 ms rung is measured through probe RPC polling.**
The local backend answers a note write in <10 ms; paint-after-response is indistinguishable from paint-before-response. And polling `page.evaluate` loops on a loaded machine (this box runs a live benchmark) add 50–150 ms of harness latency to a ladder whose top rung is 100 ms. Same polling defect applies to the tooltip 150 ms rung.
*Amendment:* (a) the probe intercepts `POST */api/payments/*/note` via `page.route` and **holds the response 800 ms**; assert the cell shows the new value with `data-state="saving"` while the request is provably pending, then release and assert `saved` — this converts the check from a timer to a causal proof; (b) all sub-second latencies (tooltip, paint, camera-visible-within-250ms) are stamped **page-side** (MutationObserver installed pre-interaction records `performance.now()` at the mutation), the probe only collects the stamp. Rungs then measure the app, not the RPC.

**F11. [FAIRNESS] `t_camera` yaw comparison is not modulo-360.**
`expectedYaw = 35 − 42 = −7`; an app that normalizes yaw to [0,360) reports 353 and fails `|got − want| ≤ 1.5` while rendering pixel-identically (the pixel checks would pass). Spec says "yaw unbounded" but normalization is render-equivalent and a reasonable implementation choice.
*Amendment:* compare angular distance: `min(|Δ| mod 360, 360 − |Δ| mod 360) ≤ 1.5`. Same for any vsdbg camera comparison.

**F12. [DETERMINISM] Short-bar top samples sit within rounding distance of a face edge.**
`count=1 → h=0.25`: at distance 30 / pitch 27° a top face projects to a handful of pixels. CSS→backing uses `Math.round` (±1 px) and the sample is the projected top-**center** — for the shortest bars a 1 px shift crosses onto the side face or background. Sides got a 0.75-achievable ladder; tops carry 0.5 weight with **no** allowance.
*Amendment:* deterministic screen-space guard: compute the projected top-face polygon area analytically; exclude tops whose area < 25 px² from grading (report as `skippedSmall`), and sample a 3×3 majority around the center for the rest. Alternatively give tops the same reference-measured achievable-fraction ladder as sides — but exclusion is cleaner because it removes the noise instead of averaging it.

**F13. [FAIRNESS] Webhook `received` counter is ambiguous about the challenge handshake.**
"`received` counts every POST that reached the endpoint (valid or not)" — the `webhook.verify` challenge is a POST to the endpoint. A good-faith reader counts it; another excludes it as pre-registration plumbing. `h_webhook_ledger` demands the quad **exactly**, so the ambiguity is worth a full compound component.
*Amendment:* one spec sentence: "the verification challenge is part of registration, not an event delivery — it does not increment any counter." (Or the opposite; just pick one and freeze it in `EXPECTED_WEBHOOK_COUNTERS`'s derivation comment.)

**F14. [FAIRNESS] The `--stall` trap grades behavior the docs never describe.**
The design converts `client_timeouts` to "sync must return within bound under a vendor stall," but Artifact 2 and (per the current file's philosophy) vendor_docs say nothing about stalls or required client timeouts. That grades unread behavior — the exact fairness bar `vendor_service.py`'s own docstring commits to.
*Amendment:* vendor_docs gains a documented behavior ("the API may occasionally hold a connection open indefinitely; clients must apply a request timeout of at most N seconds and retry") and the spec's Rules echo it. The trap then tests reading, not clairvoyance.

**F15. [GAMEABILITY] `t_vsdbg_truth` never penalizes overclaimed bars, and `request_efficiency_v3`'s optimum branch skips the traps.**
(a) `scene` score = `matched/expected`; an app emitting every cell including zero-count (or duplicates) loses nothing for the extra claims. Fix: `matched / max(expected, claimed)`. (b) `reqs == OPTIMAL and complete ≥ 1.0 → 1.0` without `traps_ok` — a client that retries a 429 instantly (ignoring Retry-After) hits the optimal count and full completeness and takes 1.0 on the check whose docstring says traps are required; also delete the dead first line in the under-optimum branch (`s = … * 0 if … else 1.0` immediately overwritten). Fix: require `traps_ok` in every branch that awards ≥ 0.75.

**F16. [FAIRNESS] `b_summary_currency` caps on a key *name*, not on the sin.**
`any(key in s for key in ("total_minor","total","grand_total")) → cap 0.25`. But `/api/payments` itself uses `total` for a row **count** — an app echoing `"total": <count>` in the summary is harmless and idiomatic within this very spec, yet loses 75% of the check.
*Amendment:* cap only when the offending value is an actual cross-currency money sum: flag iff the field's value equals `sum(total_minor)` across currencies (± any single currency's total, to catch near-misses), or the rendered page shows a combined money figure (`v_` side). Key names are not evidence.

**F17. [DETERMINISM] Missing `BENCH_VIZ_BUCKETS` silently grades against an empty expectation.**
The probe defaults to `'{"days":[],...}'` → `emptyRun=true` → the full-fixture app is graded down the empty-db path, and `t_scene_binding` can even fire its phantom-data zero **against a correct app**. A missing env var must refuse, not improvise — the instrument-refuses doctrine.
*Amendment:* in `viz`/`viz-fallback` scenarios, absent or unparsable `BENCH_VIZ_BUCKETS` ⇒ `emit({probeError: "BENCH_VIZ_BUCKETS missing"})`; scorer maps that to PROBE UNAVAILABLE (harness failure), never to app evidence.

**F18. [DETERMINISM+FEASIBILITY] The viz scenario's worst-case wall time brushes the 90 s hard cap; a partial emit zeroes the whole T tier.**
Worst path: 20 s nav + 10 s idle + 10 s readiness + SwiftShader startup (seconds) + ~15 s of scripted interaction + dozens of `evaluate` round-trips on a contended machine. `HARD_MS=90000` then emits a partial object mid-flow; every T check reads absent keys as failure — a machine-load artifact scored as an app defect (the `view_refreshed` class again). Also: an app that polls (e.g., health every 2 s) defeats `waitIdle` and burns its full timeout every call.
*Amendment:* viz scenarios get `HARD_MS=150000`; the emit is built **incrementally** (each phase merges its section into `result` as it completes) so a cap hit ships everything measured with `timedOut:true`, and the scorer treats absent-because-timeout sections as PROBE UNAVAILABLE per section, not 0. Replace `waitIdle` with the readiness poll alone for viz.

---

## SEV-3 — hardening and calibration hygiene

**F19. [DETERMINISM] Worker/OffscreenCanvas rendering is legal per spec but invisible to the instrumentation.**
`glInstrument` patches main-thread prototypes; an app using `transferControlToOffscreen()` + a Worker renders correctly but shows `drawCalls=0` and unreadable pixels → T tier ≈ 0 for a correct app.
*Amendment:* one spec sentence: "create the WebGL context directly on `#viz3d` in the main thread" (consistent with `vsdbg.project/pick` needing synchronous access anyway).

**F20. [CALIBRATION] The 3-knob grid fit on medians of n=3/4 bakes sampling noise into frozen thresholds.**
Haiku n=3 with plausibly bimodal sb-6 outcomes (3D works / doesn't) makes its median a coin flip between distant modes; the ordering constraints (≥0.06/≥0.10) can pass or fail by luck, steering γ. The G1 IQR ≤ 0.06 gate will also silently absorb machine-contention variance (this box runs a live benchmark during sweeps), attributing harness noise to models.
*Amendment:* (a) leave-one-out stability gate: refit dropping each run in turn; if any knob moves > 0.2 (γ) / one rung (k_P), the sweep is under-powered — add reps before freezing; (b) fit loss uses only the Opus band as a hard target, Sonnet/Haiku bands as soft (they have the least data); (c) latency-ladder checks are excluded from the fit's objective on any sweep where the machine ran concurrent load (record load average per run; refuse to fit latency rungs when max load > threshold).

**F21. [CALIBRATION] `fit_sb6.rescore(v, gc, gh, kp)` requires raw measurements the verdict schema does not archive.**
k_P moves rung *boundaries* — re-application needs the underlying ms/fraction/count inputs, not the 0–1 scores today's verdicts store. Without this the fit silently degenerates to fitting γ only, and k_P freezes at its placeholder.
*Amendment:* every sb-6 check's `g()` call stores its raw inputs in `parts` (ms values, fractions, request counts) and the verdict archives `parts` verbatim; `fit_sb6.py` asserts at load that every k_P-sensitive check has its raw input present, refusing to fit otherwise.

**F22. [GAMEABILITY] `t_fallback`'s notice regex matches any visible "webgl" text anywhere on the page.**
A static footer ("Built with WebGL") collects the 0.15 notice credit in the glKill scenario with zero fallback logic.
*Amendment:* differential assertion — the matched notice element must be visible in `viz-fallback` and **absent/hidden in the normal `viz` run** (probe already visits both; compare the matched text), and must live inside or adjacent to the viz panel container.

**F23. [FAIRNESS] The depth-pair "fixture guarantees" holds only for the reference layout; and `S.tops.slice(0,4)`/`S.tops[0]` assume a populous visible set.**
Occlusion is aspect-independent, but *in-canvas visibility* of the occluded top depends on the app's legal layout choices (full-width canvas, min-height 240 — height otherwise unpinned). A narrow/short canvas can drop the guaranteed pair or leave < 4 tops for the vsdbg spot checks, and the tooltip target `S.tops[0]` may be an edge bar.
*Amendment:* validate the guarantee against the extreme legal layouts (1280×240 and 375-wide) with the reference implementation before freeze; the probe picks the depth pair and tooltip target from the largest-projected-area candidates rather than index 0; when < 4 tops are visible, scale the vsdbg spot set instead of defaulting misses to failure.

**F24. [FAIRNESS] `v_currency_rendered`'s traps must grade the exponent, never the presentation.**
"KWD 129.900" vs "129.900 KWD" vs "د.ك 129.900" are all correct money; `Intl.NumberFormat` output varies by locale data. If `format_ok` string-matches the spec's examples, correct apps fail by locale.
*Amendment:* `format_ok` = parse the cell to (digit groups, decimal places) and assert decimal places == the currency's exponent and the digits equal `amount_minor` — symbol, placement, and separators are free.

---

## Cross-cutting note

Findings F2, F3, F7, F17, F18 are all the same sb-5 lesson (`view_refreshed`: harness failure read as app failure) recurring in new clothes; the single structural fix that covers them is **G6 plus per-section PROBE-UNAVAILABLE semantics** — the reference implementation must pass everything before any threshold is trusted, and the scorer must never convert a missing harness artifact into an app zero. That pair should be implemented first, because it converts every remaining unknown in the "scenario glue has not run against a full v3 app" confidence gap into a gate that refuses instead of a defect that ships.