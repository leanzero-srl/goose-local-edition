# SB7.2 — Meridian Payments Landscape, 3D weighted into the score

Status: implemented and reference-validated on 2026-10-02 from the owner's approved design ("the scorer
needs to put sufficient emphasis on the 3D aspects", "very judicious on the 3D elements it demands", "a
really tough benchmark", "avoid benchmaxxed topics", "tests it in a usable, functional way", "economically
viable"). No model or paid run was made; the golden reference, six one-defect controls and re-scores of
archived SB7.1 entrant trees are the proof. The archived entrants were built against SB7.1's prompt, so their
SB7.2 numbers below are calibration only and must never be published as SB7.2 results.

## Why SB7.1 needed changing (receipts, measured before this tier)

- Composition: `final = min(earned, ceilings)`, `earned = (0.88·inner + 0.12·gate_fraction·e_mean) × critical
  multiplier` (score_sb7.py `compose_from_rows`). SB7.1's inner weights are SB7's (A .04 B .09 C .09 D .06 J .12
  V .06 P .08 T .14 X .16 R .16) and its S/Q/M visual rows are admission-only, weight 0 (score_sb71.py `admit`).
  3D was ≈22% of raw points; difficulty came only from the ceilings.
- Gemini 3.8 Flash earned raw 0.9683 but finished 0.799 on three small defects, one a 3.45:1 Replay-button
  contrast. A DeepSeek run's presentation band was tripped by a 4.49:1 button. Text contrast was capping a 3D
  benchmark.
- Every entrant's overview, and the golden's (runs/sb71-validation/20260920/sb71-reference-1a06212bc-…-evidence/
  sb7-shots/*-sb71-field.png), is an unreadable colour carpet: 12,288 towers at SB7's default camera (yaw 30,
  pitch 40, distance 260) on a 1170 × 460 canvas, about 1.7 px per world unit — caps 1.6 × 1.3 px. DeepSeek run
  6 rendered the field nearly black (s_visible_surface 0).
- The stream-apply excellence rung was 100 ms; the reference measures 131–135 ms on this host. Unpassable.
- E rungs (100 ms stream, 40 frames over 40 moves, 16 gate conditions) were not in the public contract.
- VISUAL-CONTRACT.md's "Evidence and grading" section was 6.6 KB of grader method (clocks, ±16 ms windows,
  phase windows), resident in every model turn and an invitation to rebuild the probe.

## What stays identical

Product, SB7 behaviour (SB7.2 hands out its own trimmed `SB7-CONTRACT.md`, see "Public contract trim"), starter (`sb7.1/starter`, reused),
seeded fixtures, vendor v3, gather, every SB7 check and every SB7.1 visual check, the 0.12 excellence term and
its 16 gate conditions, the critical registry and multiplier floor 0.6, the severity selftest, isolation,
completion receipts, fresh scoring vendor, provider-error refusal, reaping and cost recording. The token cost
profile is SB7.1's: the public contract is smaller (below).

## Weights (inner, sum 1.00)

| tier | S | Q | M | T | P | B | C | X | R | A | D | J | V |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| SB7.1 | 0 | 0 | 0 | .14 | .08 | .09 | .09 | .16 | .16 | .04 | .06 | .12 | .06 |
| SB7.2 | .10 | .08 | .06 | .16 | .06 | .08 | .08 | .10 | .10 | .02 | .04 | .08 | .04 |

3D = S + Q + M + T + P = **0.46** of inner (asserted in score_sb72.py). Rows:
S `s_visible_surface`, `s_tower_geometry`, `s_currency_collar`; Q `q_payment_context`, `q_inspector_framing`,
`q_overview_legibility` (3D legibility and inspection, no text contrast); M `m_committed_event_replay`;
V gains `v_presentation_text`. SB7.1's `q_legible_presentation` is split: its 8 inspector framing poses
become `q_inspector_framing` (3D), its four text groups (12 px, 4.5:1 effective contrast, unclipped, uncovered,
inked) become `v_presentation_text`, weighted points that never cap.

Single-defect costs through the real composition (test_score_sb72.py `compose`): a 3.4:1 button
(`v_presentation_text` .75) → 0.9985, no ceiling; wrong inspector values (`q_payment_context` .75) → 0.9941;
illegible overview (`q_overview_legibility` 0) → earned 0.9765, ceiling 0.799; no animation → earned 0.9472,
ceiling 0.899; one lost acknowledged write → 0.5956 (critical, unchanged).

## Ceilings (3D-pure; passing awards nothing)

| condition missing | max |
|---|---:|
| `s_visible_surface` (visible, data-backed 3D) | 0.599 |
| + `s_tower_geometry`, `s_currency_collar`, `t_layout_basis`, `t_scene_binding`, `t_height_pixels`, `t_vs7dbg_truth` | 0.699 |
| + the 10 T interaction checks (draw budget, pick buffer, real pick pass, click, camera, coast ×2, labels, brush, incremental stream), `q_overview_legibility`, `q_inspector_framing` | 0.799 |
| + `m_committed_event_replay` AND every X and R check (backend excellence) | 0.899 |

`q_inspector_framing` is the 3D half of SB7.1's "readable inspection" band leg; `q_payment_context` (DOM
values of the inspected payment) and all text readability earn points only.

## The framed default camera and overview legibility

SB7's default camera cannot frame the field: the field is 115 × 216 world units (96 days × up to 180 ranks,
1.2 pitch) and at yaw 30 / pitch 40 / distance 260 its long axis runs into the screen. Projected bounding-box
shares (scratch computation, R0 = 180): 1170 × 460 → 0.35 × 0.75 of the canvas at 1.75 px/unit; 1170 × 640 →
0.49 × 0.75. SB7.2 publishes **yaw 70, pitch 50, distance 190** (target, projection, clamps and laws unchanged):
1170 × 640 → 0.77 × 0.88 bounding box at ~2.9–3.5 px/unit. The probe's wheel step becomes 800 so one graded
step still reaches the 340 clamp from 190 (SB7.1's 400 from 260 did; the law is unchanged).

`q_overview_legibility` (Q, 0.799 band) is read from the same composited screenshot as `s_visible_surface`, at
the default camera, label boxes excluded. Every number is in sb7.2/VISUAL-CONTRACT.md and the parity test
`test_every_legibility_number_and_the_camera_are_published` fails the build if the probe and contract drift.

| leg | rule | golden-sb72 (5 runs, identical) | margin |
|---|---|---|---|
| framing | pixels within ±8 of a published tower colour span ≥ 60% of canvas width and height (0.5–99.5 percentile); none within 4 px of the edge | 0.681 × 0.717, 0 edge pixels | 13% / 20% |
| lighting | median WCAG contrast vs #101828 over ≤ 200 seeded pixels predicted to be tower surface ≥ 2.0:1 | 2.743:1 (200 pixels) | 37% |
| status | ≥ 64 of ≤ 96 seeded unmutated payments show a cap top covering a whole 2 × 2 px block; ≥ 90% read as their status (nearest legend colour within RGB distance 40, all four pixels) | 96 sampled, 96 correct | 32 caps / 10 points |

The legend distance 40 is below half the closest pair of status colours (pending–failed, 99). Score: mean of
the three legs, each 1 when met, else proportional. It demands no new feature: the published camera plus a
canvas the field can fill. The reference's canvas went from 460 to 640 CSS px; its colours are the published
ones (the contract fixes overview shading, so "lighting" is checked, not re-specified).

## Excellence rungs, now public

Published in the contract's "Excellence rungs" section (437 bytes). One recalibration: the stream-apply top rung
100 → **180 ms** (sb72-thresholds.json carries the receipt: reference 156.1 / 141.2 / 144.9 / 141.9 ms at load
31 / 8 / 7 / 10, so 15% above the worst and 27% above the median; still below the 200 ms 0.75 rung and the
public 250 ms P budget). No other threshold moved.

## Public contract size

| | spec | visual contract | SB7-CONTRACT | total |
|---|---:|---:|---:|---:|
| SB7.1 | 3,598 | 16,287 | 54,146 | 74,031 |
| SB7.2 | 3,322 | 12,229 | 54,146 | 69,697 |

−4,334 bytes (−5.9%) per resident copy. "Evidence and grading" 6,610 → 925 bytes: what is checked, not how.
Superseded by the trim below (SB7.2 now 35,746 bytes of its own public text).

## Public contract trim (2026-10-02, owner-approved)

Why: a GPT-6 Luna SB7.2 run spent 3 h, 309 calls and $9 desk-auditing the contract. Its reasoning at minute
169: "the absolute value of x should be less than or equal to .45, and both x and z combined should not exceed
.80"; at minute 183: "pedestal has 96, core has 36, ribs total 144, cap is 96, and rails are 144, which sums
to 516" — 189 one-line edits, 16 browser tests, and nothing told it when it was done. The owner approved a
trim on one condition: "the only hope is that the benchmark doesn't water down completely".

Rule applied: nothing a check measures is removed or weakened; a kept requirement stays exactly as strict
(restated shorter); text no check measures is cut. Method: every check in score_sb7.py (91), score_sb71.py /
score_sb72.py (8 visual rows), the gather battery that feeds them and the browser probe
(product_probe_sb71.mjs under the sb-7.2 profile, sb72-thresholds.json) was read and mapped to the sentence a
candidate needs to pass it (map below); every unmapped sentence was cut (list below). No scorer, probe,
threshold, vendor, fixture or starter byte changed; the golden reference proves scoring does not depend on
the removed text.

Mechanics: SB7.1 keeps handing out `spec-build-sb7.md` (frozen). SB7.2 renders its own files into the
workdir — `bench/isolated_tiers.py` `IsolatedTier.public`:
`SB7-CONTRACT.md` ← `sb7.2/SB7-CONTRACT.md`, `VISUAL-CONTRACT.md` ← `sb7.2/VISUAL-CONTRACT.md`,
`STARTER.md` ← `sb7.2/STARTER.md` (overrides the shared starter's copy in the workdir only). The prompt
`spec-build-sb72.md` gains a Definition of done (11 scored behaviours, each pointing at its section) and the
call budget: "You have a budget of 150 model calls. The harness stops the session at the budget and scores
whatever exists; plan to implement, test, and finish well inside it — polishing beyond the scored behaviours
earns nothing." (enforcement: run_build `goose --max-turns`, another change; keep the number 150 in sync).

### Bytes the SB7.2 entrant receives

| file | before (main 8c20adc96) | after | change |
|---|---:|---:|---:|
| benchmark-prompt.md (spec-build-sb72.md) | 3,322 | 3,267 | −1.7% |
| SB7-CONTRACT.md | 54,146 | 24,488 | −54.8% |
| VISUAL-CONTRACT.md | 12,229 | 6,963 | −43.1% |
| STARTER.md | 2,410 | 1,028 | −57.3% |
| **SB7.2-owned subtotal** | **72,107** | **35,746** | **−50.4%** |
| BROWSER-TESTING.md (run_build, shared with SB7.1) | 537 | 537 | 0 |
| prompt browser line (run_build, shared) | 153 | 153 | 0 |
| vendor docs at /v3/docs (vendor_docs_v3.md, shared with SB7.1's vendor) | 9,150 | 9,150 | 0 |
| **everything the entrant receives** | **81,947** | **45,586** | **−44.4%** |

Raw repository bytes (`SB7-CONTRACT.md` renders `{DOCS_URL}`/`{BASE_URL}`/`{API_KEY}`, ±tens of bytes). The
vendor docs are untouched: they are SB7.1's vendor's bytes, and the trimmed contract now points at them
instead of restating them (generation rule, faults, If-Match, Idempotency-Key, signatures, groups).

### Kept: check → the sentence a candidate needs (S = sb7.2/SB7-CONTRACT.md, V = VISUAL-CONTRACT.md)

| check(s) | measured | kept in |
|---|---|---|
| a_package_layout | app/__main__.py, app.ledgerd, app.notifierd, DECISIONS.md, four web files | S §1 table, §7 file list, §9 |
| server_runs, r_b4 bound_in_10s | GET / 200 + notifier /health within 10 s | S §1 "listens within 10 s" |
| a_combined_entrypoint | `python -m app …` boots both | S §1 table |
| serves_page, d_content_types | / and three assets, css/javascript types; JSON + text/event-stream | S §3 API table, "Every response is JSON except…" |
| a_asset_budget | ≤150 KB, no external refs | S §7 "at most 150 KB", preamble "zero external code" |
| a_db_ownership | ledger.db, notifier.db, no extra *.db | S §1 ownership bullet |
| sync_completeness, b_total_field, x_l4 count | 12,288 (+creates) rows and total | S §2, §3 Sync, §3 Payments |
| b_row_shape | exactly ten row keys | S §3 Payments |
| b_chronological_order, b_viz_records order | default sort by instant; (instant, id) | S §3 Payments, §8 Data |
| b_summary_shape, x_m3, x_m4, b_money_rendered cross-sum | by_currency/reversals keys, sorted, no cross-currency total | S §3 Summary |
| b_buckets_dst | timezone, every (day,status) cell incl. 0, Berlin instant days | S §2, §3 Buckets |
| b_viz_records | 7 columns, equal lengths, server Berlin day | S §8 Data |
| b_events_log, x_l2, x_ooo dup, r_no_dupe_effect events | contiguous seq from 1, type/source vocab, seq/type/at keys, no duplicate/stale events | S §3 Event ledger |
| b_events_log, r_no_dupe_effect (critical), x_* via `_fetch_events` | the grader pages `/api/events` with `after=max(seq)`: an endpoint that ignores `after` or is not ascending repeats or skips events (contiguity, duplicate-effect count) | S §3 API table "events with `seq > after`, ascending by `seq`" (restored after independent review) |
| b_error_envelope, b_json_shapes, d_validation | 400 + field_errors (path, code) for bad limit/offset/sort/status and invalid draft; 404 envelope; 401/403/404/200 auth matrix | S §3 Payments + Errors, §5 roles + validation |
| c_paged_walk | 192 pages, no undocumented params, no duplicate pages | S §3 Sync "documented parameters only — no limit" + docs |
| c_b1_drop_resume, c_b2_retry_after | resume same cursor; one retry after Retry-After, continue | S §3 Sync |
| c_b5_generation_304, r_cache_truth | one unconditional refetch, ≤3 identical conditionals, no stale-as-fresh | S §3 Sync |
| c_conditional_resync | later syncs carry validators | S §3 Sync "Later syncs are conditional"; §1 "every boot … starts a sync" |
| c_webhook_discipline, x_ooo forged | deliveries 2xx-acked; forged → 401 | S §4 |
| c_send_idempotency, r_no_dupe_effect vendor leg | Idempotency-Key on every send, reused on retry | S §5 SEND |
| d_client_timeouts | served while vendor down, or `timeout=` in source | S preamble "timeout of at most 10 s", §1 last bullet |
| d_peer_absence | proxy 502 + notifier_unreachable; each service survives the other's death | S §3 API table, §1 "Neither crashes" |
| d_decisions_doc | ## D1/D2/D3, ≥15 chars, stance matches observed | S §9 |
| d_decisions_doc D2 observation | gather reads `GET /api/drafts?state=rejected` and resubmits the first non-F3 row; an unfiltered list resubmits the wrong draft | S §5 table "filtered by state" (restored after independent review) |
| j_loads_data, j_first_use | rows render; DOM states the total ("of N"); first rows ≤2 s; console | S §7 Table ("showing X–Y of TOTAL", "within 2 s") |
| j_console_clean, gate console | zero console errors | S §7 States |
| j_sync_journey | #sync-now / disabled or data-state=syncing / POST /api/sync / table rows = latest page with money, status word, note / .cur-total count + total / last_sync advanced | S §7 Summary + Table, §3 API `/api/sync`, Summary `last_sync` |
| j_workflow_journey, r_workflow_durability | #role-token, #draft-form fields, #draft-list data-draft-id/data-state, submit/approve buttons, feed shows approval, sent payment found via #prev/#next (+ Date header sort) on a ≤50-row page with correct cells | S §5, §7 Drafts panel + Table |
| j_workflow_reject | reject reaches `rejected`, feed shows it | S §5, §7 Drafts panel |
| j_workflow_journey (critical), j_workflow_reject | pageNotificationsState reads only the first 6 `#notifications` entries and tests /approv/, /reject/ on them: an oldest-first feed hides the new rows (journey 6/7 → ×0.943, reject ≤ 2/3) | S §6 `/notify/notifications` "newest first", §7 feed "entries newest first" (restored after independent review) |
| j_notifications_feed, r_b7 ui_* | data-state degraded → live within 5 s, no reload | S §7 Notifications feed |
| j_error_state | visible, actionable error when /api is blocked | S §7 States |
| j_empty_state | no phantom rows; empty state with progress or blocked (D3) | S §7 States, §9 D3 |
| v_dates_readable | Date column human, no raw ISO | S §7 Dates |
| v_money_presentation, b_money_rendered | currency token; decimals = exponent; JPY/KWD | S §7 Money, §2 exponents |
| v_status_badges | four hex values, distinct computed colours | S §7 Table |
| v_responsive_375 | no horizontal scroll at 375 px, rows rendered | S §7 "375 px" |
| v_styling | stylesheet, ≥3 distinct backgrounds, non-default font, #app-header | S §7 opening paragraph |
| p_drag_frames, e_frames_under_drag | frames over the 40-move drag | S §8 Rendering "0.8 frames per move"; V Excellence rungs |
| p_idle_flatness | 0 default draws per 500 ms at rest | S §8 Demand rendering |
| p_stream_apply, e_stream_apply_latency | SSE receipt → visible ≤250 ms (E: 180 ms rung) | S §8 Streaming; V Excellence rungs + Evidence |
| p_under_stream, e_under_load_latency, p_api_latency | /api/payments?limit=50 and /api/summary p95 ≤150 ms, incl. under a webhook burst | S §3 "Reads during a sync" |
| p_sync_wall | first walk ≤120 s | S §3 Sync |
| e_optimistic_paint | note painted while held, saving → saved | S §7 Notes; V Excellence rungs |
| e_mastery, excellence gate | T+X+R mean, 16 conditions | V Excellence rungs (verbatim, test-pinned) |
| t_context_real | webgl on #viz3d, antialias/alpha false, backing store × DPR, draws, coverage | S §8 Rendering |
| t_layout_basis | d0/D0/R0, unmoved after creates | S §8 Data |
| t_scene_binding | digest moments within tolerance | S §8 Scene digest |
| t_height_pixels | tops ±3 px, JPY+KWD | S §8 transform + "±3 px" |
| t_draw_budget | ≤8·frames and ≤8·(M+8) default draws, drew at all, ended slow | S §8 Draw budget |
| t_pick_buffer | pick == pickPixel == analytic, four occlusion constructions | S §8 Pick buffer |
| t_pick_real_pass | ≥1 offscreen draw + readPixels after invalidation, ≤4 offscreen draws, 0 default draws | S §8 Pick buffer |
| t_click_semantics | instance click toggles on/off; background clears | S §8 click rule |
| t_camera_math | defaults, drag law, wheel law + no page scroll, pitch/distance clamps, dblclick reset + zero velocity, projection | S §8 Camera; V default camera |
| t_coast_identity, t_coast_reality | remaining-coast identity, slow release, settle budget, ≥3° flick in drag direction, pixels move | S §8 Inertia |
| t_labels_culling | exact shown set, 110×18, offset ±2, data-id, no overlap, pick-occlusion | S §8 Labels |
| t_brush_link | row toggle, dim/member pixels, #brush-count, instance toggle, row navigated + in viewport + data-brushed, background clear + full hex | S §8 Linked brush, §7 Table rows |
| t_stream_diff | bytes ≤ \|S\|·stride+4096, no realloc, digest delta, changed pixel | S §8 Streaming |
| t_vs7dbg_truth | camera vs pixels, digest, frames vs wrapper, pick triplet | S §8 vs7dbg |
| x_l1, x_l3, x_m1, x_m2/x_l5, x_l4, x_m3, x_conservation_residual, x_no_lost_write | invented states, monotonic reads, amount immutability, group atomicity, convergence, terminal conservation, residual, acked writes | S §10 second paragraph; §4 groups; §3 Sync "never regress" |
| x_ooo_dup_forged | v+2 kept, forged untouched, duplicate applied once | S §4 |
| r_b3_sigkill_resync | restart mid-sync, converge, no dupes | S §1 restart bullet, §10 |
| r_b4_vendor_down_boot | bound ≤10 s, served while down, no crash, recovered unattended | S §1 last bullet |
| r_b6_outbox_atomic, r_notifier_exactly_once | pending before kill, resumed, exactly once, none lost; `duplicate` counter | S §3 Outbox, §6 |
| r_b7_partition | writes fast, status down + pending, UI degraded, catch-up in order, live ≤5 s | S §3 Outbox, §7 feed |
| r_notification_multiset | four notifying types, payment.sent none | S §6 |
| r_no_row_loss | no committed row lost across kills | S §1 restart bullet, §10 |
| s_visible_surface | visible ≥300×240 canvas, unobscured, seeded tower pixels in published colours at the default camera | V Overview + shading + default camera; S §8 colours/background |
| s_tower_geometry, s_currency_collar | per currency at yaw 35/125: every part and gap colour, picks incl. background through gaps | V Inspection geometry + materials + "both passes"; V Evidence |
| q_payment_context | #inspect-id/currency/amount/status/version equal the backend | V Controls and cameras |
| q_inspector_framing | canvas ≥600×460; 40–90% height, unclipped; four callouts named with width, at part height, not covering tower or each other | V Controls and cameras |
| q_overview_legibility | framing 60%/4 px, contrast 2.0:1 over ≤200 px, ≥64 of ≤96 caps 2×2, ≥90% at RGB 40 | V A legible overview (verbatim, test-pinned) |
| m_committed_event_replay | live note edit animates the frame on the published trajectory, camera fixed; replay without writes; newer restarts, stale/duplicate change nothing; Full field → default camera, idle, callouts hidden | V Animate |
| v_presentation_text | legend, details, callouts, controls: ≥12 px, ≥4.5:1, unclipped, uncovered, inked | V Controls and cameras last sentence (legend kept as a visible element for this) |

### Cut — no check measures it (from SB7-CONTRACT.md = spec-build-sb7.md)

- `GET /api/health` and its four webhook counters (received/applied/ignored/rejected), and every "+1" bookkeeping
  sentence in §4: nothing in gather reads /api/health (perf_probe measures it only in its own CLI, never called by
  gather).
- `bind 127.0.0.1 only`; "shows the degraded state in the UI" while the vendor is down at boot (UI probes run after
  sync #1).
- Fixture statistics: per-day ≤180 / mean 128, every day ≥1 payment, each status ≥8%, amounts span 3 decades, the
  four candidate DST dates and the ≥7-day margin — they describe the seeded data, nothing checks the app against them.
- `/api/payments` `currency` filter (no check sends one; `status=bogus` and `sort=bogus` are the only filter probes —
  both kept).
- Summary `count`, `oldest`, `newest` (RFC3339 UTC); buckets `days` and `statuses` arrays and day-major ordering
  (b_buckets_dst keys cells by (day, status) and counts them).
- `POST /api/sync` response body and its `502 vendor_unavailable`: the scorer never POSTs /api/sync; the UI journey
  needs only the POST and an advancing `last_sync` (kept).
- Note conflict path (second 412 → `409 conflict`, row unchanged; UI revert + `#notice`): no check makes a note
  conflict. The If-Match write-through itself is kept (the optimistic and animation note edits need it).
- Event `txn` field and `latest_seq` (`_fetch_events` stops on an empty page without it; the `seq > after` paging
  rule itself is KEPT); which `source` each kind of change uses (only the vocabulary is checked).
- Outbox relay batch size ≤50: the relay talks to the candidate's own notifier; no check can observe it.
  `/notify/events` request/response shape (`accepted`/`duplicate`): internal to the candidate, never called by the
  grader. Notifier `/health` `received`/`applied`/`notifications`, `/notify/processed` `latest_seq`, notifications
  "human sentence". ("Newest first" was cut here at first and RESTORED: the browser feed is read through its
  first 6 entries, see the Kept map.)
- Error-envelope frozen vocabularies (the six field-error codes, the envelope code list beyond the codes kept): the
  check requires string `code`/`message` and string `path`/`code` per field error; "naming the parameter" likewise.
- Four-eyes (`approval_forbidden`, submitter cannot approve/reject): no check uses the submitter's token to approve.
- Draft validation bounds (name 1–80, country `^[A-Z]{2}$`, note 0–280, currency one of four) and
  `?state=<unknown>` → 400: the only validation probe is a non-integer amount with missing fields (kept).
- Frontend: page section order and side-by-side layout; each web file's content purity ("structure only", "nothing
  else"); the `#summary` container id (the probe reads `.cur-total[data-currency]` directly); `rev-total`; `#last-sync` human text and "Never synced" (the sync journey computes lastSyncText then
  overwrites the verdict with `syncRequested && refreshedTruth`); the failed-sync `#notice`; the status/currency
  custom-dropdown filters with `data-value` (the probe never operates a filter); the Amount header sort and
  `aria-sort` (only the Date header click is driven, and only its `sort` query is read); notification
  `data-kind`, the human time and the "visible degraded treatment"; drafts `data-selected`, "buttons enabled only
  when legal", auth errors in `#notice`; the loading state, `#viz-empty`, `#viz-error` and the no-WebGL fallback
  (never induced); canvas full width / 240 px minimum at 375 px; the design bans (no pastel, no left rail, no
  native `<select>`/`alert`/`confirm`) — v_styling reads stylesheet, backgrounds, font and `#app-header` (kept).
  `prompt()` stays banned for notes: the probe needs a real input in the Note cell.
- Brush toggle upload cost (≤ stride+4096 bytes, no realloc): t_brush_link has no byte leg (the stream's byte budget
  is kept). The "matches the active filters / filtered out just toggles" clause went with the filters.
- Performance budgets nothing measures: first non-background 3D frame ≤3 s; camera change visible ≤250 ms;
  `/api/buckets` ≤200 ms and `/api/viz/records` ≤400 ms p95 (p_api_latency measures payments and summary only).
  "8 concurrent readers while a sync runs" restated as the measured shape (concurrent readers during webhook bursts).
- "Harness guarantees you can rely on" (pinned move counts, ≥30 ms between release moves): grader method.
  Pick clear-colour alternatives and the decode formula (implied by the encoding); label "single line,
  ellipsized" (only the box is measured); "fixture guarantees distinct amounts and ≥6 days" among label
  candidates (fixture statistic).
- README.md "with the exact commands" (no check reads README).
- Rationale and duplicates: every "because"/"the graded failure is"/"scores as broken, not as clever" sentence; the
  vendor-docs restatements (delivery JSON, signature formula, challenge body, fault mechanics, value-dating); the
  "What WILL happen" schedule's grader detail (3–8 s, 8 events, kill choreography) — compressed to the list of
  faults in §10; the Performance budgets and Rules sections (every measured number stays at its requirement);
  the standard-library module list; the side-face `round(0.55·top)` (superseded in SB7.2 by VISUAL-CONTRACT's
  directional factors, which S §8 points to).

### Cut — VISUAL-CONTRACT.md

- The legend's CONTENT (axes, height, status colour, collar encoding, four parts, four widths, the 0.06 ribs):
  only `#tower-legend`'s existence and text readability are measured (v_presentation_text) — kept as "a visible
  legend explains the encoding".
- "labeled" input; no native select or browser dialog; "selecting a payment in the field can populate the ID input"
  (permissive).
- Inspection usable at 375 px and the 320 px mobile minimum (the inspector is only probed at 1280 × 800).
  KEPT after review: "hide the full-field labels while inspecting" — no row reads labels during inspection, but an
  inspected payment that is a label candidate keeps an eligible `.viz-label` over its cap callout, which
  v_presentation_text (uncovered) and q_inspector_framing (callout overlap) would charge.
- Replay disabled until an update exists (only "enabled after an update" is read); `#replay-status` text; "reloaded
  pages need not retain replay history".
- The EUR frame may touch the ribs (implied by the numbers); prose on dimensional contrast, showcases, "one finished
  product", "not a claim that money moved"; "No amount, height, position or status may be invented" (the stream
  pixel check and the animation's fixed-part check keep measuring it; S §8 "pixels show the change", V "never move").
- Evidence and grading: the method and the duplicated readability/animation clauses; kept: what is compared, the two
  inspection yaws, gaps in their published colour, the stream-latency clock, the real note edit (≤1,100 bytes, test).

### Cut — prompt and STARTER.md

- Prompt: the 3D descriptive paragraph (all of it is in V and S §8), "Use the documented runtime vendor flag" (S),
  "HTTP 501 … unfinished work" (STARTER.md), "Private scorers, golden applications, previous entrants and operator
  logs are outside the workspace" and "Test your own application" (replaced by the Definition of done).
- STARTER.md: "Credit and comparison" (a reporting policy for whoever publishes a result — no entrant behaviour),
  and restatements of the contract (grader kills services independently; "build every state").

### Proof that scoring does not depend on the removed text

`golden-sb72` (disposable copy) through `score_sb72.py --reference` after the trim and the merge of main's
362fe0f35, seed 5a05d7631d9276e3, port 8899, bundled runtime (Python 3.12, node 24, chrome-headless-shell 153),
MacBook, load 4.57 → 6.11 (the bench unit suite ran ~30 s concurrently): **1.000**, earned 1.0000, inner 1.0000,
excellence 1.0 (16/16 gate conditions), critical multiplier 1.0, no unsuppressed criticals, ceiling 1.0, every
band open, freeze gate and severity selftest passed (exit 0). Every one of the 99 rows is 1.0 (b_buckets_dst
prints 0.9999999999999999, float rounding of 0.7+0.2+0.1). Stream apply 140.9 ms (rung 180); legibility
0.6812 × 0.7172, 2.743:1, 96/96 — identical to the pre-trim reference. `contract_sha256` now names
sb7.2/SB7-CONTRACT.md, sb7.2/VISUAL-CONTRACT.md and sb7.2/STARTER.md.

Repeat on the FINAL bytes (34b466ea4, after the label-hiding restore), quiet otherwise, load 5.68 → 8.97:
**1.000**, identical breakdown (inner 1.0, excellence 1.0, ×1.0, no unsuppressed criticals, ceiling 1.0, reference
gate passed, exit 0); stream apply 134.6 ms; legibility 0.6812 × 0.7172, 2.743:1, 96/96; `contract_sha256` equals
the sb7.2/release-manifest.json pins (VISUAL-CONTRACT d9f37411…, SB7-CONTRACT a434aeaf…, STARTER 58796eb6…).

### Measured but never stated — flagged, not changed

`j_workflow_journey`'s payment witness compares the vendor-created payment's `amount_minor` with the value the probe
typed into the draft form's amount field, so that field takes MINOR units; and the form is filled through native
`input`/`textarea`/`select` elements found by name/id/placeholder/aria-label. Neither contract (before or after) says
so. Adding it would make SB7.2 easier than SB7.1 on the same scorer, so it is left for the owner.


## Probe and harness changes (SB7.1 behaviour unchanged)

`product_probe_sb72.mjs` sets `globalThis.__BENCH_PROBE_TIER = 'sb-7.2'` and imports product_probe_sb71.mjs —
one probe implementation, a profile that no environment can leak into SB7.1 grading. Under the profile only:
the camera defaults and wheel step above; `q_overview_legibility`; and three precision fixes the framed pose
exposed on the reference (each traced on the evidence, see commit 2e38bdfdc): the brush colour witness reads
one homogeneous 3×3 face patch (the decisive ray met a 0.84h collar ledge, the read pixel was the shaft side);
coast "pixels moved" spots are those predicted to change colour at every reachable yaw (a framed field put a spot
on a same-coloured tower, 4/5); the brush-clear restore pixel uses the same witness. The golden finishes its
table reveal after `scrollIntoView('nearest')` (a row sat 0.36 px below the fold) and drops stale table
responses (an auto-navigation to the sent payment and the probe's page click raced; offset 400 landed after
3500, `j_workflow_journey` 0.857 once in five runs — an inherited SB7.1 golden race, seen under load in SB7.1's
own release notes).

score_sb72.py wraps score_sb71: `tier_runtime()` swaps PROBE_NAME, VERSION, the weight table, ROOT_BLOCKS
(visual rows downstream of `sync_completeness`) and the stream rung for the duration of gather/evaluate; it
recomposes SB7.1's rows plus the eight visual rows through `compose_from_rows`, then applies the 3D-pure bands.
`score_sb71.run_cli` is the shared hermetic CLI. `isolated_tiers.py` is the single table run_build.py and
bench_rescore.py read: `--sb72` gets SB7.1's isolation, starter, limits, reaping, fresh scoring vendor,
provider-error refusal, cost record and completion receipt; receipts carry their tier, and the retry replays
that tier's scorer and contract. `severity_selftest` runs SB7's selftest under SB7.2 weights plus SB7.2
orderings (text costs V points and never caps; legibility outweighs text; every visual row earns).

## Proof

All runs: this Mac Studio, bundled runtime (Python 3.12, node 24, chrome-headless-shell 153 from Goose
Swarm.app), serial, hermetic CLI (`score_sb72.py`/`score_sb71.py --tree … --seed … --port …`), 1-minute load
average at start in brackets. Reports: `runs/sb72-validation/20261002/` (local, gitignored); the reference
report is committed under `sb7.2/release-validation/`, all summarised in `bench/sb72-release-validation.json`.

### Golden reference (seed 5a05d7631d9276e3, port 8899)

`golden-sb72` with final bytes, `--reference`: **1.000** — earned 1.0000, inner 1.0000, excellence 1.0, critical
multiplier 1.0, no unsuppressed criticals, every band open, freeze gate and severity selftest passed, every
non-calibration check ≥ 0.95 and in fact every check 1.0 [9.60]. Stream apply 137.4 ms (rung 180).
Legibility 0.681 × 0.717, 2.743:1, 96/96. Earlier runs on the way, kept as the record: 0.799 (t_brush_link
colour witness, [31]), 0.799 (brush row reveal, [8]), 0.799 (brush + coast spot, [7]), 1.000 [10],
0.941 (j_workflow_journey race, [22]) — each failure traced and fixed in the commits above.

### One-defect controls (`bench/sb72_controls.py`, same seed and port)

| control | final | earned | ceiling | what it lost (vs the 1.0 reference) |
|---|---:|---:|---:|---|
| unlit field (overview × 0.08) [7.9] | 0.599 | 0.8887 | 0.599 | s_visible_surface 0; q_overview_legibility 0.05 (lit and framing and status legs); every colour-witness row: t_height_pixels 0, p_stream_apply 0, e_stream 0, t_vs7dbg_truth .7, t_brush_link .67, t_stream_diff .8, t_camera_math .86 |
| mis-framed canvas (SB7.1's 460 px under SB7.2's camera) [7.0] | 0.799 | 0.9915 | 0.799 | q_overview_legibility 0.637 only (framing 0.49 wide; 6 readable caps of 64) |
| status colours swapped (pending↔failed) [4.2] | 0.599 | 0.8329 | 0.599 | s_visible_surface 0, legibility .76 (status leg), s_tower_geometry .80, q_inspector_framing 0, m 0, colour-witness T/P rows |
| cuboid towers (overview shaft and inspector core fill the footprint) [4.3] | 0.699 | 0.8872 | 0.699 | s_tower_geometry .75, s_currency_collar 0, q_inspector_framing 0, m 0 (the collar is buried in the solid) |
| frozen animation [5.3] | 0.899 | 0.9472 | 0.899 | m_committed_event_replay 0 only |
| 3.37:1 Replay button [4.7] | 0.9985 | 0.9985 | 1.0 | v_presentation_text 0.75 only — V points, no band |

Judgement: the mis-framed, frozen and low-contrast controls lose exactly their own row. The cuboid loses the
structure band and the motion it hides. The unlit and swapped controls fall to 0.599: SB7.2, like SB7.1, treats
the overview's published colours as data, so a black field or a field showing the wrong status fails
visible-surface admission and every colour-witness check — this is inherited SB7.1 behaviour, not a new
penalty, and the owner may judge it harsh for the swap.

### SB7.1 controls

- `golden-sb71` under the SB7.1 scorer after these changes [7.6]: **0.994**, earned 0.9940, inner 1.0, reference
  gate passed — identical to its validated release receipt (0.994; its 132 ms stream apply still misses
  SB7.1's 100 ms rung). SB7.1 behaviour is unchanged.
- `golden-sb71` under SB7.2 [9.2]: 0.799 (earned 0.9691) — an SB7.1-correct app that keeps SB7's camera and a
  460 px canvas fails t_camera_math (defaults, reset), t_coast_reality, t_brush_link, the legibility framing and
  the inspection-exit camera leg.

### Calibration only — archived SB7.1 entrant trees (built against SB7.1's prompt; never publish)

Each with its own fixture seed at port 8850, both scorers on today's bytes (SB7.1 column reproduces the archived
hermetic numbers where they exist: c003209f 0.4900 → 0.4932, 43896353 0.799 → 0.799, 01dae737 0.699 → 0.699,
Gemini raw 0.9683 → 0.9683).

| tree | SB7.1 final (earned, inner, crit) | SB7.2 final (earned, inner, crit) | SB7.2 bands failed |
|---|---|---|---|
| Gemini 3.8 Flash 20940258 [8.2/5.5] | 0.799 (0.9683, 0.9764, ×1.0) | 0.699 (0.9466, 0.9453, ×1.0) | t_height_pixels, t_vs7dbg_truth; camera, coast, brush, legibility |
| DeepSeek pinned 43896353 [5.8/5.6] | 0.799 (0.8715, 0.9055, ×1.0) | 0.799 (0.8348, 0.8639, ×1.0) | 10-T leg + legibility, inspector framing |
| DeepSeek pinned 01dae737 [5.1/26.1] | 0.699 (0.7721, 0.9023, ×0.8674) | 0.699 (0.7738, 0.8979, ×0.8674) | tower geometry, scene truth |
| DeepSeek c003209f [6.1/7.0] | 0.4932 (0.4932, 0.8789, ×0.6) | 0.4520 (0.4520, 0.8009, ×0.6) | tower geometry, height, scene truth |
| DeepSeek pinned c605215d (run 6, black field) [—/4.6] | refused today: `fire_d1_mutation:failed`, no decisive D1 pixel witness on its dark field (archived hermetic 0.5051 on the same SB7.1 probe bytes — the D1 arm is not deterministic on this tree) | 0.4593 (0.4593, 0.782, ×0.6) | s_visible_surface (black field) |

Unsuppressed criticals: c003209f and c605215d `j_workflow_journey`; 01dae737 `b_money_rendered`,
`j_workflow_journey`; none for the others. Every SB7.1 tree loses SB7.2's camera-default checks because it was
never asked for the framed camera: the comparison measures the weighting, not these entrants' SB7.2 ability.
Gemini's 3.45:1 button now costs only V points (v_presentation_text 0.75); it falls to 0.699 because its
default camera is SB7's and every colour check after a double-click reset is taken at the wrong pose.


## Confidence and limits

- High: the weighting, bands, split, selftest, wiring and receipts (unit-tested; the SB7.1 reference
  reproduces 0.994 exactly; Gemini's SB7.1 raw reproduces 0.9683 exactly).
- Medium: legibility thresholds rest on one reference app and one seed (five identical runs). The framing
  share depends on canvas aspect only; the status leg is a good discriminator (golden 96/96, mis-framed 6
  readable caps); the lighting leg is redundant with s_visible_surface for apps that render the published
  colours and exists for apps that do not.
- Lower: the three probe precision fixes are tier-gated to SB7.2 and proven on the reference, the six controls
  and five archived trees only; an entrant whose geometry differs could expose another sub-pixel assumption in
  the inherited SB7 checks at the framed pose. The camera change is the single heaviest consequence for an
  entrant that misses it (Gemini's archived tree: 0.799 → 0.699), stated in bold in the contract.
- Inherited, not fixed: the D1 stream-witness refusal on a near-black field (c605215d) is SB7.1 behaviour;
  `j_workflow_journey` races in golden-sb71 remain (SB7.1's frozen reference was not edited).
- Not done (scope cut by the owner's coordinator, 2026-10-02): the second reference repeat with final bytes,
  the opt-in browser test file run (test_sb72_visual.py, SB72_BROWSER_TESTS=1), the SB7.1 re-run of c605215d.
  Desktop packaging verifies sb7.2/release-manifest.json (main 3c65824af); the starter text still says SB7.1.

