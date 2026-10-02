# Build `app` — Meridian Payments Console (SB7 behavioural contract, SB7.2 edition)

Two cooperating services sync 12,288 payments from the Meridian API v3, keep them consistent
through webhooks, concurrent edits, crashes and partitions, run a maker/checker approval
workflow that creates real vendor payments, and serve a console: payments table, notifications
feed, drafts panel and an interactive 3D field. `VISUAL-CONTRACT.md` amends the 3D field.

Vendor docs: `{DOCS_URL}` — they define every vendor behaviour (fixed 64-item pages, cursors,
`Retry-After`, `410 cursor_expired`, `ETag`/`If-None-Match` and the collection-generation rule,
`If-Match` note writes, `Idempotency-Key` creates, webhook registration, challenge and
signatures, transaction groups, reversals). Follow them exactly; this file states what YOUR app
must do. Base URL `{BASE_URL}`, passed to your process as `--vendor` (use the flag, never a
constant). API key `{API_KEY}` on every vendor request. Every vendor request has a timeout of at
most 10 s.

Backend: Python 3 standard library only. Frontend: zero external code (no CDN, npm or vendored
library), works offline. Nothing is operator-driven: boots, syncs, retries, relay deliveries
and post-crash heals are self-driven.

## 1. Processes

| Command | Effect |
|---|---|
| `python -m app --db-dir P --ledger-port N --notifier-port M --vendor URL --tokens-file T` | boots both services |
| `python -m app.ledgerd --db-dir P --port N --notifier http://127.0.0.1:M --vendor URL --tokens-file T` | ledgerd alone |
| `python -m app.notifierd --db-dir P --port M` | notifierd alone |

- The grader starts and SIGKILLs each service independently. Each listens within **10 s** of
  start, every time.
- ledgerd owns `P/ledger.db`, notifierd owns `P/notifier.db`; no other database files; each
  service opens only its own file. Neither crashes when the other (or the vendor) is down.
- Restarting on an existing `--db-dir` resumes cleanly: idempotent schema, no loss, nothing
  applied twice.
- `--tokens-file` is JSON `{"maker": "<32 hex>", "checker": "<32 hex>", "admin": "<32 hex>"}`.
- On every boot, once listening, ledgerd starts a sync unprompted; while the vendor is
  unreachable it serves local data and retries at least every **5 s** until the sync succeeds.

## 2. The collection

N = **12,288** payments at load (192 pages of 64); runtime creates append. One Europe/Berlin
96-day span containing one DST transition (which one is seeded — never hardcode it). A
payment's day is the Berlin calendar date of its `created_at` INSTANT, never the string or the
UTC date. Statuses `settled`, `pending`, `refunded`, `failed`; currencies `EUR`, `USD`, `JPY`,
`KWD` with minor-unit exponents 2/2/0/3; amounts are integers in minor units, never floats.
Values are seeded per run. Created payments are value-dated inside the span, so the 3D layout
basis never moves.

## 3. ledgerd

### Sync

Walk the vendor list (documented parameters only — no `limit`), upsert into `ledger.db`, and
fetch `GET /v3/reversals`. Syncing twice never changes the count. Handle the documented faults:
a dropped connection resumes the same cursor; a `500` + `Retry-After` gets one retry after the
wait, then the walk continues; `410` restarts the cursor; never restart committed work
unconditionally. Later syncs are conditional (`If-None-Match`) and apply the generation rule
exactly as documented: one unconditional refetch on a mismatched `304`, never more than 3
identical conditional requests in a row, never stale data served as fresh. The walk is not
snapshot-isolated and webhooks race it: upserts compare `version` and never regress a row; a
payment created mid-walk ends as exactly one row. The full first walk finishes within
**120 s**.

### API

| Method | Path | Response |
|---|---|---|
| `GET` | `/`, `/web/*` | the four frontend files with correct content types |
| `GET` | `/api/payments?limit=&offset=&status=&sort=` | `{"data": [...], "total": <int>, "limit": <int>, "offset": <int>}` |
| `GET` | `/api/payments/<id>` | the payment row, or 404 envelope |
| `GET` | `/api/summary` | below |
| `GET` | `/api/buckets` | below |
| `GET` | `/api/viz/records` | §8 |
| `GET` | `/api/stream` | SSE `text/event-stream`, §8 |
| `POST` | `/api/sync` | runs a sync; `last_sync` advances on success |
| `POST` | `/api/payments/<id>/note` | body `{"note": <str>}` → `{"id", "note", "version"}` |
| `POST` | `/api/webhooks/meridian` | the vendor's delivery endpoint, §4 |
| `GET` | `/api/events?after=<seq>&limit=<int>` | `{"events": [...]}`, bearer token required (any role) |
| `GET` | `/api/outbox/status` | `{"pending": <int>, "notifier": "up" \| "down", ...}` |
| `GET` | `/api/notifications?limit=&offset=` | proxied to notifierd; notifier down → `502`, code `"notifier_unreachable"` |
| `POST/GET` | `/api/drafts...` | §5 |

- **Payments:** `limit` default 50, up to 200 honoured; `offset` default 0. Rows carry exactly
  `id`, `amount_minor`, `currency`, `created_at`, `settled_at`, `status`, `version`, `note`,
  `counterparty_name`, `country` (the vendor's `counterparty` flattened). `status` filters;
  `sort` ∈ `created_at` (default, ascending by INSTANT), `-created_at`, `amount_minor`,
  `-amount_minor`. `total` reflects the filters. A bad `limit`/`offset` (non-numeric or
  negative) or an unknown `status`/`sort` → 400 with `field_errors`.
- **Summary:** `{"last_sync": <str|null>, "by_currency": [{"currency", "count", "total_minor"}],
  "reversals": [{"currency", "count", "total_minor"}]}` — both lists sorted by currency code,
  `reversals` only for currencies that have reversals. Never a cross-currency total anywhere.
- **Buckets:** `{"timezone": "Europe/Berlin", "cells": [{"day": "YYYY-MM-DD", "status": <s>,
  "count": <int>}, ...]}` — one cell per (day, status) over every day from first to last, zero
  counts included (96 × 4 = 384 at load), bucketed by Berlin day of the instant.
- **Note:** 1–280 chars, written through to the vendor with `If-Match` and the documented
  412 re-fetch/retry-once; on success persist the returned resource and answer the new version.
- **Reads during a sync:** every read endpoint keeps answering while a sync runs. `GET
  /api/payments?limit=50` and `GET /api/summary` answer in under **150 ms** p95, including with
  concurrent readers during webhook bursts.
- **Errors:** one envelope `{"error": {"code": "<snake_case>", "message": "<sentence>",
  "field_errors": [{"path": "<dot.path[i]>", "code": "<code>"}]}}`; `field_errors` only on 400
  validation failures. Unknown path → 404, code `"not_found"`. Every response is JSON except
  static assets and the SSE stream.

### Event ledger

Every state change ledgerd applies appends exactly one event to an append-only log in
`ledger.db`: `{"seq", "type", "payment_id", "version", "source", "at"}`, `seq` contiguous from 1.

- `type` ∈ `payment.created | payment.updated | reversal.created | draft.created |
  draft.submitted | draft.approved | draft.rejected | payment.sent`; `source` ∈ `sync | webhook |
  local | approval`.
- A new row → `payment.created`; a change → `payment.updated`; no change → no event. Applied
  versions per payment strictly increase; duplicate and stale webhooks never produce events.

### Outbox

`draft.submitted`, `draft.approved`, `draft.rejected`, `reversal.created` and `payment.sent`
cross to notifierd via `POST /notify/events`. Each outbox row commits in the same SQLite
transaction as the state change it records. A background relay (never a request handler)
delivers at-least-once in `seq` order, marks a row delivered only after a 200, backs off at
most **2 s**, and resumes from the durable rows after a restart. User writes never block on the
notifier: while it is down, writes commit fast, `/api/outbox/status` reports `"down"` with
`pending` > 0, and the relay catches up after it returns.

## 4. Webhooks

After ledgerd is listening, register `http://127.0.0.1:<ledger-port>/api/webhooks/meridian`
as documented (idempotent, retried until the vendor is reachable) and answer the challenge.
For every delivery, deterministically: verify the signature over the raw body first — missing
or wrong → `401`, state untouched; an already-processed event id or a version not above the
stored row → `200`, state untouched; otherwise apply, append the event, `200`. Answer within
**3 s**; never call the vendor from the handler. Apply transaction groups atomically as
documented: stage parts, apply the complete group in one local transaction; no read may ever
observe half a group. Payments keep the four statuses; reversals appear in the summary
`reversals` block and the notifier.

## 5. Approval workflow

Bearer tokens (`Authorization: Bearer <token>`) on every drafts endpoint and on `/api/events`.
Missing or unknown token → `401` (`"unauthorized"`); known token, wrong role → `403`
(`"forbidden"`). `admin` reads drafts and events and writes nothing.

| Method | Path | Role | Effect + ledger event |
|---|---|---|---|
| `POST` | `/api/drafts` | maker, checker | `{"amount_minor", "currency", "counterparty": {"name", "country"}, "note"}` → `draft.created` |
| `POST` | `/api/drafts/<id>/submit` | maker, checker | `submitted` → `draft.submitted` |
| `POST` | `/api/drafts/<id>/approve` | checker | `approved` → `draft.approved`, then SEND |
| `POST` | `/api/drafts/<id>/reject` | checker | `rejected` → `draft.rejected` |
| `GET` | `/api/drafts?state=` | any | `{"data": [...], "total": <int>}` |

- States `draft → submitted → approved | rejected`, `approved → sent`. A draft object carries
  at least `id`, `state`, `amount_minor`, `currency`, `counterparty`, `note`, `created_at`. An
  unknown draft id → 404. Invalid draft input (e.g. a non-integer `amount_minor`, a missing
  field) → 400 with `field_errors`.
- **SEND:** after committing `approved`, create the vendor payment (`POST /v3/payments`) with
  an `Idempotency-Key` stored with the draft before the first attempt; on 2xx append
  `payment.sent`. Any retry (crash, timeout, stall) reuses the stored key: exactly one vendor
  payment per approved draft. The payment returns through webhook/sync like any other.
- **Durability:** `submitted` and `approved` are durable once their 200 is sent. After a
  SIGKILL right after either — including mid-send — the restart finds the state intact, the
  outbox event preserved, and the send completed with the same key.

## 6. notifierd

| Method | Path | Response |
|---|---|---|
| `POST` | `/notify/events` | batch of ledger events from the relay |
| `GET` | `/health` | `{"status": "ok", "duplicate": <int>, ...}` |
| `GET` | `/notify/processed?after=<seq>` | `{"processed": [{"seq": <int>, "type": <str>}...]}` |
| `GET` | `/notify/notifications?limit=&offset=` | `{"data": [{"id", "event_seq", "kind", "message", "at"}...], "total": <int>}` |

Idempotent by ledger `seq`: a seq already in the processed set changes nothing (counted in
`duplicate`), and a mixed batch still applies its new events. The processed set and the
notifications are durable in `notifier.db` across SIGKILL; exactly-once is graded on them.
Exactly `draft.submitted`, `draft.approved`, `draft.rejected` and `reversal.created` produce
one notification each (`kind` = event type); `payment.sent` is processed but notifies nothing.

## 7. Frontend — `web/`

Four files served by ledgerd and referenced with relative paths: `web/index.html`,
`web/styles.css`, `web/app.js`, `web/viz.js` — together at most **150 KB**. Style it as a
product: your own stylesheet, a non-default font, distinct solid background colours on the
header, table and controls, and a branded header bar `#app-header` with the product name.

- **Summary:** per currency an element `.cur-total[data-currency]` showing that currency's
  payment count and total in its own currency. A sync button `#sync-now` calls
  `POST /api/sync`, shows an in-flight state (disabled, `data-state="syncing"`) and, on
  completion, re-enables and refreshes the table and summary.
- **Table:** columns in this order: Date, Amount, Status (the status word), Counterparty,
  Note. Server-paginated through `/api/payments` at most 50 rows per page (never
  `/api/viz/records`), with `#prev`/`#next` and a `showing X–Y of TOTAL` readout. Clicking the
  Date header toggles ascending/descending via `sort`. The first rows render within **2 s** of
  page load. Rows carry `data-id` and `data-brushed`
  (`"true"` while brushed); clicking a row toggles it in the brush (§8), except the Note cell.
  Status badges use the 3D hex values: `settled #059669`, `pending #D97706`, `refunded #7C3AED`,
  `failed #B91C1C`, distinct in computed colour.
- **Notes, optimistically:** clicking a Note cell opens an inline text input (never
  `prompt()`); on confirm (Enter or a Save button) the new text paints in the cell
  immediately, before the network answers, with the row at `data-state="saving"`, then
  `data-state="saved"` after the 200.
- **Notifications feed `#notifications`:** reads only `/api/notifications`, polling at least
  every **5 s**; each entry carries `data-event-seq` and shows its kind and message. `data-state="live"`; `"degraded"`
  while the proxy answers 502; back to `"live"` without a reload within **5 s** of the notifier
  returning.
- **Drafts panel:** a token input `#role-token` (the bearer for drafts calls); `#draft-form`
  with amount, currency, counterparty name and country, and note; `#draft-list` rows with
  `data-draft-id` and `data-state`, click to select; `#submit-btn`, `#approve-btn`,
  `#reject-btn` act on the selected draft. The full journey works through the UI: maker →
  create → submit; checker → approve or reject; the feed shows the submitted/approved/rejected
  notifications; the sent payment appears in the table.
- **States:** an empty database shows an empty state with a call to sync (corner D3); a
  backend that is unreachable or erroring shows an error a user can act on (e.g. retry); never
  a blank page. Zero console errors and unhandled rejections in every journey.
- **Dates** render human-readable in the user's locale — never raw ISO-8601 strings.
- **Money** renders in each amount's own currency with exactly its exponent's decimals and the
  stored digits (`129900 EUR → €1,299.00`, `JPY → ¥129,900`, `KWD → KWD 129.900`), with a
  currency symbol or code.
- **375 px** viewport: no horizontal scroll, rows still rendered.

## 8. The 3D field

Raw WebGL (no library) on `<canvas id="viz3d">`, main thread, every payment one instance on a
day × in-day-rank grid, with GPU picking, an inertial camera, culled labels, a brush linked to
the table and live SSE diffs. The grader recomputes all of this and compares it with your API,
pixels, picks and GL call stream.

### Data → scene

`GET /api/viz/records` → `{"count": N, "id": [...], "amount_minor": [...], "currency": [...],
"status": [...], "created_at": [...], "day": [...], "version": [...]}`, all length N, order
`(created_at instant ASC, id ASC)`; `day` is the server-computed Berlin date, which the
frontend uses and never recomputes.

- `n` = stable arrival index: initial records in serve order (0-based), each streamed create
  appends at `n = count`. `n` never re-sorts. Picks, digest, labels and `vs7dbg` key on `n`/`id`.
- Layout basis, locked at the first non-empty render: `vs7dbg.layout()` →
  `{"d0": <first day>, "D0": 96, "R0": <max in-day count at load>}`; unchanged by creates.
- `d` = day − `d0` in calendar days; `r` = in-day rank at load by `(created_at, id)`; a
  streamed create takes `r` = current in-day count.

```
Δ = 1.2;  x = (d − (D0 − 1)/2)·Δ;  z = (r − (R0 − 1)/2)·Δ;  base y = 0
a_major = amount_minor / 10^exp(currency)        exp: EUR 2, USD 2, JPY 0, KWD 3
h = clamp(0.9 + 0.55·log10(a_major), 0.2, 4.2)
```

Rendered tops are measured in device pixels (±3 px, JPY and KWD included). Every payment
renders every frame (frustum culling only; no LOD or decimation outside VISUAL-CONTRACT's
inspection mode).

**Colours** (exact, ±8 per channel): status `settled (5,150,105)`, `pending (217,119,6)`,
`refunded (124,58,237)`, `failed (185,28,28)`; face shading per VISUAL-CONTRACT; brushed-dim
(brush non-empty, instance not in it) `round(0.30·c)` applied before the face factor;
background `#101828 (16,24,40)` — every non-tower pixel, no floor, grid, axes or in-canvas text.

**Scene digest** `vs7dbg.sceneDigest()` → `{count, Sh: Σh, Sh2: Σh², Sx: Σx, Sz: Σz,
Sxh: Σx·h, Szh: Σz·h, brushedCount}` over all current records, float64, rounded to 4 decimals;
graded within `max(0.5, 1e-4·|expected|)` and against the rendered pixels.

### Rendering

- Context `webgl` or `webgl2`, `{antialias: false, alpha: false}`; backing store
  `clientWidth × devicePixelRatio` by `clientHeight × devicePixelRatio`. Depth testing on.
- **Draw budget:** at most **8** default-framebuffer draw calls per rendered frame (every
  draw entry point and instancing extension is counted). Over a 40-move drag (pointerup + one
  rAF), default-framebuffer draws ≤ `8·max(frames, 1)` and ≤ `8·(40 + 8)`, and the scene draws
  at least **0.8 frames per move**.
- **Demand rendering:** draw on load, input, coast and data change only; at rest **0**
  default-framebuffer draws in any 500 ms window.
- A "realloc" is any `bufferData` over 4096 bytes.

### Pick buffer

GPU truth, never a CPU raycast: an offscreen RGBA8 + depth framebuffer sized to the drawing
buffer, no MSAA, depth-tested, every instance drawn in identity colour
`idNum = n + 1`, `r = idNum & 255`, `g = (idNum >> 8) & 255`, `b = (idNum >> 16) & 255`;
0 = background. `vs7dbg.pick(sx, sy)` → `{id, index}` or null and `vs7dbg.pickPixel(sx, sy)` →
raw `[r, g, b, a]`, both at device pixel `(round(sx·DPR), Hdev − 1 − round(sy·DPR))`, and both
agree with the analytically nearest surface (occlusion in either index order). After each
invalidation (camera change or applied batch) the first pick does ≥ 1 offscreen draw and ≥ 1
offscreen `readPixels`, at most **4** offscreen draws per refresh; later picks may use the
cached readback; a pick never draws to the default framebuffer.

A pointerup within **5 px** and **300 ms** of its pointerdown is a click: on an instance it
toggles that record in the brush; on background it clears the brush.

### Camera

```
θ = yaw·π/180   φ = pitch·π/180   T = (0, 1, 0)
eye = T + distance·(cos φ·sin θ, sin φ, cos φ·cos θ)
f = normalize(T − eye)   r = normalize(f × (0,1,0))   u = r × f
q = p − eye;  xc = q·r;  yc = q·u;  zc = q·f;   zc ≤ 0.5 → not projected
fovY = 50°,  k = 1/tan(fovY/2),  aspect = Wcss/Hcss,  near 0.5, far 1000
sx = ((k/aspect)·xc/zc + 1)/2·Wcss     sy = (1 − k·yc/zc)/2·Hcss     (CSS px from canvas top-left)
```

Default camera: VISUAL-CONTRACT. Clamps: pitch `[5, 85]`, distance `[15, 340]`; yaw unbounded
(compared modulo 360).

- **Drag:** per pointermove, `yaw ← yaw − 0.30·Δx`, `pitch ← clamp(pitch + 0.30·Δy)`.
- **Wheel:** `distance ← clamp(distance·exp(0.0012·deltaY))`; the canvas consumes the event,
  the page never scrolls.
- **Double-click:** reset to the defaults and zero all angular velocity.
- **Inertia:** at pointerup, `(vyaw, vpitch)` in deg/s = `0.30·Δpx/Δt` from the last two
  moves. Then `v(t) = v0·e^(−t/τ)`, `yaw(t) = yaw0 + v0·τ·(1 − e^(−t/τ))`, τ = **0.4 s**, using
  real elapsed time (a per-frame constant decay fails); stop when `|vyaw| < 2` and `|vpitch| < 2`.
  Pitch clamps apply during the coast and zero `vpitch`. pointerdown and double-click cancel the
  coast; wheel does not. Graded: `yaw_rest − yaw(t) = v(t)·τ` at mid-coast samples within
  `max(1.0°, 0.15·|v·τ|)`; a fast flick keeps moving ≥ **3°** in the drag direction, visibly;
  a release under **6 px/s** moves ≤ 0.5°; the coast settles within
  `τ·ln(max(v0, 2)/2) + 0.7 s`, capped at **2.5 s**.

### Labels

Candidates: the **12** records with the highest `a_major` (ties by `id` ASC). Anchor
`A = project(x, h, z)` (top centre, live camera). Eligible iff `A` is inside the canvas and
`pick(A)` returns that instance. Each shown label is a DOM element `.viz-label[data-id]` in
`#viz-labels` over the canvas, border-box exactly **110 × 18** CSS px, top-left at
`(A.sx + 10, A.sy − 9)` ± 2 px, text containing the amount in its own currency. Consider
candidates by `a_major` DESC, `id` ASC; show one iff eligible and its rect intersects (≥ 1 px)
no already-shown label, otherwise hide it (never move it). Labels update every rendered frame
and after `vs7dbg.setCamera`.

### Linked brush

One set of record ids; `vs7dbg.brush()` returns it sorted by id; `#brush-count` shows its size.
A table row click toggles the record (row `data-brushed="true"` while in the set). While the set
is non-empty, non-members render dimmed and members keep their exact colours. An instance click
toggles the record and navigates the table to the page containing it under the current sort,
with the row `data-brushed="true"` and scrolled fully into view. A background click clears the
set and the dim. Whether a brushed record stays brushed after a streamed mutation is corner D1.

### Streaming diffs

`GET /api/stream` (SSE, concurrent subscribers) delivers every committed payment change
(including note writes) in exactly one message, each message one atomic batch:
`{"batch": <int, increasing>, "records": [{"id", "amount_minor", "currency", "status",
"created_at", "day", "version"}, ...]}`. The initial dataset comes from `/api/viz/records`.

- Applying a batch touches only the changed instances: a status change touches 1; a create
  appends at `n = count`, `r` = current in-day count; nothing re-ranks or re-sorts.
- During a batch apply, uploaded bytes (`bufferData` + `bufferSubData`) ≤ `|S|·stride + 4096`
  and no realloc.
- Within **250 ms** of receipt the store, digest and pixels show the change.

### `window.vs7dbg` — required, synchronous, truthful

```js
layout()                  // {d0, D0, R0}
sceneDigest()             // {count, Sh, Sh2, Sx, Sz, Sxh, Szh, brushedCount}
camera()                  // {yaw, pitch, distance, vyaw, vpitch} — live values
setCamera(yaw, pitch, distance) // clamps, cancels coast, renders, re-culls labels
pick(sx, sy)              // {id, index} | null
pickPixel(sx, sy)         // [r, g, b, a]
brush()                   // ids, ascending
frames()                  // frames rendered since load, agrees with the counted draws
```

Each answer must agree with what the canvas shows.

## 9. `DECISIONS.md`

Decide each corner, implement it, and document it under exactly these headings, two or three
sentences each (the choice and why); the run exercises all three and an undocumented or
contradicted corner fails:

- `## D1` — does a brushed record stay brushed after a streamed mutation, or drop out?
- `## D2` — is a rejected draft terminal or resubmittable?
- `## D3` — before the first sync completes, does the table render empty-with-progress or block?

## 10. The graded run

Faults are seeded; all of them are normal operation. The vendor is down for the first seconds
of a boot. The first walk gets one dropped connection and one `500` + `Retry-After`, with
webhooks racing it: mutations to served and unserved pages, an out-of-order pair, a duplicate,
a forged signature, a mid-walk create and a two-part refund group. A later sync gets a `304`
with a mismatched generation. ledgerd is SIGKILLed mid-sync; notifierd is SIGKILLed while
ledgerd commits more events; ledgerd is SIGKILLed between an outbox commit and its delivery,
right after a submit's 200, and right after an approve's 200 with the send in flight. Every kill
is followed by a restart with the same flags.

The grader polls your API throughout and replays your event log and the notifier's processed
set against the vendor's history. At every instant: every served or applied
`(payment_id, version)` exists in the vendor's history; served versions never decrease; no
read observes half a refund group (per currency, reversal totals equal the refunded rows'
amounts); amounts never change. At quiescence: every version and status equals the vendor's,
counts and per-currency totals (reversals included) equal the vendor's ground truth (fixture,
scripted mutations and your creates), every write answered 2xx is present, and no event,
vendor payment or notification is applied twice.
