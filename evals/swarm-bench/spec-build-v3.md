# Build `vspro` — VendorSync Pro

An operations product that syncs payments from the Meridian API, keeps them consistent through
vendor-pushed webhooks and concurrent edits, and gives a finance team a live view of the money —
including an interactive 3D chart of payment activity.

The Meridian API v2 documentation is at `{DOCS_URL}`. Read it before you start — every behaviour
you must handle is documented there, and several of them (rate limits, expired cursors, stalled
connections, version conflicts, webhook signatures) will defeat a client that did not read.
Base URL `{BASE_URL}`, API key `{API_KEY}`.

Work in the current directory. Python 3, standard library only for the backend — no pip installs
(`sqlite3`, `zoneinfo`, `hashlib`, `hmac` are all in the standard library). The frontend ships
ZERO external code — no CDN, no npm, no vendored libraries of any kind. Everything must work
fully offline.

---

## What to build

### 1. `vspro/meridian.py` — the vendor client

```python
class MeridianClient:
    def __init__(self, base_url: str, api_key: str) -> None
    def fetch_all_payments(self) -> list[dict]        # every payment, oldest first by instant
    def get_payment(self, payment_id: str) -> dict    # single resource; includes "version"
    def total_count(self) -> int                      # how many payments exist in the collection
    def create_payment(self, value_minor: int, currency: str, counterparty: dict,
                       occurred_at: str, idempotency_key: str) -> str
    def create_batch(self, items: list[dict]) -> list[dict]   # per-item results, input order kept
    def update_payment(self, payment_id: str, fields: dict, version: int) -> dict
    def register_webhook(self, url: str) -> dict      # {"id": ..., "secret": ...}
```

- `create_payment` returns the payment id and is safe to call more than once with the same key.
- `update_payment` sends the documented `If-Match` header built from `version`. When the vendor
  answers `412 Precondition Failed`, the client recovers as the docs prescribe: re-fetch the
  resource, re-apply `fields` on the fresh version, retry ONCE with the new `If-Match`. A second
  412 is surfaced to the caller as a conflict error. It never writes without `If-Match` — the
  vendor answers `428 Precondition Required` if you try, and that response is a bug in your
  client, not a retry case.
- `create_batch` submits up to 20 create operations in one request. The vendor applies them
  independently and reports per-item outcomes; one failed item must NOT discard, retry, or
  re-submit the items that succeeded.
- `register_webhook` is idempotent by URL: registering a URL the vendor already knows returns
  the SAME id and secret. The vendor verifies the URL with a challenge handshake during the
  call — your server must already be listening when you register.
- Pagination, `Retry-After` in both documented forms, `410 cursor_expired` restart, `ETag` /
  `If-None-Match` conditional requests: all exactly as documented, all mandatory.
- The vendor may occasionally hold a connection open without answering — the docs document this.
  Every vendor request carries a request timeout of at most **10 seconds**; a timed-out request
  is retried as the docs prescribe. A client with no timeout hangs forever on a stall and blows
  every budget downstream.

### 2. `vspro/store.py` — local persistence

```python
class Store:
    def __init__(self, path: str) -> None
    def upsert_many(self, payments: list[dict]) -> tuple[int, int]   # (inserted, updated)
    def query(self, limit: int, offset: int, status: str | None = None,
              currency: str | None = None, sort: str = "created_at") -> tuple[list[dict], int]
    def get(self, payment_id: str) -> dict | None
    def apply_event(self, event: dict) -> str    # "applied" | "duplicate" | "stale"
    def buckets(self) -> list[dict]              # day x status counts, Europe/Berlin days
    def count(self) -> int
    def last_sync(self) -> str | None            # RFC3339 UTC, or None if never synced
    def set_last_sync(self, when: str) -> None
```

Persist to SQLite at the given path. A payment already present is updated, never duplicated —
syncing twice must not change the count. `query` returns `(rows, total)` where `total` counts the
rows matching the FILTERS, not the whole table.

`apply_event` is the webhook consumer. It must be idempotent and ordered:

- an event id already processed → `"duplicate"`, state untouched;
- an event whose payment `version` is not greater than the stored version → `"stale"`, state
  untouched — the vendor does not guarantee delivery order and an old event must never overwrite
  a newer row;
- otherwise the payment row is updated and the event id recorded → `"applied"`.

`buckets` returns one cell per (day, status) pair covering every calendar day the data spans, in
the **Europe/Berlin** timezone. The day a payment belongs to is the Berlin calendar date of its
`created_at` INSTANT — not of its raw string, and not the UTC date. The fixture spans the
2026-03-29 DST transition on purpose; UTC-day bucketing produces measurably wrong counts.

### 3. `vspro/api.py` — the HTTP backend

`serve(port: int, store: Store, client: MeridianClient)` starts a JSON API on `127.0.0.1:port`:

| Method | Path | Response |
|---|---|---|
| `GET` | `/api/health` | see shape below |
| `GET` | `/api/payments?limit=<int>&offset=<int>&status=<s>&currency=<c>&sort=<k>` | `{"data": [...], "total": <int>, "limit": <int>, "offset": <int>}` |
| `GET` | `/api/payments/<id>` | the payment, or 404 envelope |
| `GET` | `/api/summary` | see shape below |
| `GET` | `/api/buckets` | see shape below |
| `POST` | `/api/sync` | `{"fetched": <int>, "inserted": <int>, "updated": <int>, "total": <int>}` |
| `POST` | `/api/payments/<id>/note` | `{"id": <str>, "note": <str>, "version": <int>}` |
| `POST` | `/api/payments/batch` | `{"results": [...], "succeeded": <int>, "failed": <int>}` |
| `POST` | `/api/webhooks/meridian` | vendor-facing; see section 4 |

**Health.**

```json
{"status": "ok", "payments": <int>, "last_sync": <str or null>,
 "webhook": {"registered": <bool>, "received": <int>, "applied": <int>,
             "ignored": <int>, "rejected": <int>}}
```

The four webhook counters are live evidence: `received` counts every event-delivery POST that
reached the endpoint (valid or not); `applied` / `ignored` / `rejected` follow the definitions
in section 4. The counters count events received by THIS process since it started — keep them in
memory; they do not survive a restart and are not supposed to.

**Payments.** `limit` defaults to 50 and is capped at 200. `offset` defaults to 0. `data` items
carry exactly the keys `id`, `amount_minor`, `currency`, `created_at`, `settled_at`, `status`,
`version`, `note`, `counterparty_name`, `country` — the vendor's nested `counterparty` object is
flattened into the last two. `status` filters to one of `settled`, `pending`, `refunded`,
`failed`; `currency` filters to one of `EUR`, `USD`, `JPY`, `KWD`; the two combine. `sort` is one
of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`; default `created_at` (ascending
by INSTANT). `total` always reflects the active filters. An unknown `status`, `currency` or
`sort` value is a validation error, not an empty result.

**Summary.**

```json
{"count": <int>, "last_sync": <str or null>, "oldest": <str or null>, "newest": <str or null>,
 "by_currency": [{"currency": "EUR", "count": <int>, "total_minor": <int>}, ...]}
```

`by_currency` is sorted by currency code ascending and contains one entry per currency present.
There is NO cross-currency total anywhere in the response — summing minor units across currencies
is meaningless and forbidden. `oldest` / `newest` are `created_at` of the earliest and latest
payments as RFC3339 **UTC**.

**Buckets.**

```json
{"timezone": "Europe/Berlin",
 "days": ["2026-03-23", "..."],
 "statuses": ["settled", "pending", "refunded", "failed"],
 "cells": [{"day": "2026-03-23", "status": "settled", "count": <int>}, ...]}
```

`days` is every calendar day from the first to the last, ascending, no gaps. `cells` contains
one entry for EVERY (day, status) pair — `days x statuses`, count 0 included — ordered day-major,
statuses in the frozen order above.

**Sync.** `POST /api/sync` runs a sync against the vendor and answers with the counts. The API
keeps answering reads while a sync is in flight: `GET /api/payments`, `GET /api/summary` and
`GET /api/buckets` must respond normally — and inside their latency budgets — during a running
sync. A single-threaded server that parks every reader behind the sync's vendor waits fails this
by construction; the standard library has what you need.

**Note.** `POST /api/payments/<id>/note` with body `{"note": <str>}` (1–280 chars) writes the
note through to the vendor with `update_payment` — full optimistic-concurrency dance included —
then persists the returned resource locally and responds with the new `version`. If the conflict
cannot be resolved (a second 412), respond `409` with the error envelope, code `"conflict"`, and
leave the local row unchanged.

**Batch.** `POST /api/payments/batch` with body:

```json
{"items": [{"amount": {"value_minor": <int>, "currency": <str>},
            "counterparty": {"name": <str>, "country": <str>},
            "occurred_at": <rfc3339>, "idempotency_key": <str>}, ...]}
```

Validate shape locally first (1–20 items; `value_minor` a positive integer; `currency` in the
supported set; `country` exactly two uppercase letters; `name` 1–80 chars; `occurred_at`
RFC3339; `idempotency_key` non-empty). Shape-valid batches are forwarded via `create_batch`;
the vendor may still fail individual items on business rules (the docs name the per-payment
amount limit). Respond 200 with per-item results in input order:

```json
{"results": [{"index": 0, "status": "created", "id": "pay_..."},
             {"index": 1, "status": "error",
              "error": {"code": "amount_over_limit", "message": "..."}}],
 "succeeded": <int>, "failed": <int>}
```

Partial failure is a NORMAL outcome: succeeded items stay succeeded, failed items report their
own error, and nothing is retried with a fresh key.

**Error envelope.** Every error this API returns uses ONE structured envelope:

```json
{"error": {"code": "<snake_case>", "message": "<human sentence>",
           "field_errors": [{"path": "items[2].amount.value_minor", "code": "not_an_integer"}]}}
```

`field_errors` appears only on validation failures (HTTP 400) and uses dot paths with `[index]`
for arrays. Frozen `code` vocabulary for field errors: `required`, `not_an_integer`,
`not_positive`, `unsupported`, `too_long`, `bad_format`. Envelope codes: `bad_request`,
`not_found`, `conflict`, `bad_signature`, `vendor_unavailable`. An unknown path is 404 with the
envelope, code `"not_found"`. A bad `limit`/`offset` — non-numeric or negative — is 400 with a
`field_errors` entry naming the parameter. Every response is JSON except the static frontend
assets.

### 4. Webhooks — the vendor calls YOU

On startup, AFTER the server is bound and listening, the app registers
`http://127.0.0.1:<port>/api/webhooks/meridian` with the vendor via `register_webhook`.
Registration triggers the documented challenge handshake: the vendor POSTs
`{"type": "webhook.verify", "challenge": "<hex>"}` to the URL and the endpoint must answer
`200` with `{"challenge": "<the same hex>"}` — this request is unsigned, because the secret does
not exist until registration completes. The verification challenge is part of registration, not
an event delivery — it does not increment any counter.

Every subsequent delivery is a signed event:

```json
{"id": "evt_00c4", "type": "payment.updated", "created_at": "<rfc3339 UTC>",
 "data": { <the full payment object, including "version"> }}
```

with header `Meridian-Signature: t=<unix seconds>,v1=<hex>` where
`v1 = HMAC_SHA256(secret, "<t>.<raw request body>")` — the raw bytes, not a re-serialization.

The endpoint must, deterministically:

- verify the signature FIRST; missing or wrong → `401` with the envelope, code
  `"bad_signature"`, state untouched, `rejected` +1;
- pass valid events to `Store.apply_event`: `"applied"` → `applied` +1, `"duplicate"` or
  `"stale"` → `ignored` +1; respond `200 {"received": true}` in all three cases;
- count every delivery arrival in `received`, valid or not;
- answer within 3 seconds and never trigger a sync or any vendor call from inside the handler.

The vendor WILL deliver duplicates, WILL deliver events out of order, and WILL (once) deliver a
forged signature. The four health counters are the ledger of how the app handled all of it.

### 5. `vspro/web/` — the frontend

A single page, served by the backend at `GET /`. Plain HTML/CSS/JS, no build step, no CDN, no
external code of any kind — it must work offline. This page is what the finance team uses every
day. Build it as a product, not as a debug view over the API.

Ship it as FOUR files, each owned and written separately: `web/index.html` (structure only),
`web/styles.css` (all styling), `web/app.js` (page behavior: table, filters, sync, notes), and
`web/viz.js` (the 3D engine, nothing else). The backend serves all four with correct content
types; the page references them with relative paths. Combined size of the four files: at most
**150 KB** uncompressed — hand-written code fits in a tenth of that; the budget exists so that a
vendored library cannot.

The page shows, top to bottom: a branded header bar (`#app-header`) carrying the app name; the
summary; the 3D visualization panel; the payments table.

**Summary** (`#summary`). One element per currency present, class `cur-total`, attribute
`data-currency`, showing the payment count and the total formatted in that currency. Never a
combined cross-currency figure. The last-sync time (`#last-sync`) reads human, or `Never synced`
when there is none. A **Sync now** button (`#sync-now`) calls `POST /api/sync`, shows a visible
in-flight state (`data-state="syncing"`, control disabled), and refreshes every view on
completion.

**Table.** Columns Date, Amount, Status, Counterparty, Note. Server-driven through the
documented `limit`/`offset`/`status`/`currency`/`sort` parameters — the page never fetches the
whole collection to paginate in memory, and never renders all rows in one scroll when more than
50 exist.

- **Pagination:** **Prev**/**Next** buttons (`#prev`, `#next`) and a `showing X–Y of TOTAL`
  readout, where TOTAL is the filtered total.
- **Sorting:** the Date and Amount column headers are clickable and toggle ascending/descending,
  reflected in `aria-sort` on the header cell and driven through the API's `sort` parameter.
- **Filters:** a status filter (`#status-filter`) and a currency filter (`#currency-filter`),
  each a custom dropdown (never a native `<select>`), each actually changing the rows AND the
  TOTAL readout. Each filter element carries a `data-value` attribute that ALWAYS reflects the
  current selection: the exact lowercase status (`settled`, `pending`, `refunded`, `failed`) or
  exact uppercase currency code (`EUR`, `USD`, `JPY`, `KWD`), and the empty string `""` when the
  filter is off. The grader reads `data-value` and nothing else to learn what is selected.
- **Status badges:** `settled` `#16A34A`, `pending` `#F59E0B`, `refunded` `#8B5CF6`, `failed`
  `#DC2626` — the same four hex values the 3D chart uses. Distinct in computed color, not only
  in text.
- **Notes, optimistically:** each row's Note cell is editable through a custom inline editor
  (never `prompt()`). On confirm the new value paints IMMEDIATELY — before the network responds
  — with the row in `data-state="saving"`; success moves it to `data-state="saved"`; a `409`
  reverts the cell to the previous value and shows a non-blocking notice in `#notice`
  (`role="status"`), never an `alert()`.

**The 3D visualization.** An interactive 3D bar chart of payment activity: one bar per
(day, status) bucket from `GET /api/buckets`, rendered with **raw WebGL** — a `<canvas
id="viz3d">` with a `webgl` or `webgl2` context, created with `{antialias: false, alpha: false}`.
No three.js, no library, no exceptions; the asset budget enforces it. Create the WebGL context
directly on `#viz3d` in the MAIN thread — no OffscreenCanvas, no Worker: `vsdbg.project()` and
`vsdbg.pick()` below need synchronous access to the live scene. The harness browser provides
WebGL; section *2D fallback* below defines what happens when a browser does not. Every contract
below is FROZEN — the grader recomputes this math independently and compares it to your API,
your pixels, and your picking.

*Scene contract.* Right-handed world, +Y up, units are world units.

- Day index `i` (0 = oldest day) → bar center `x_i = (i − (D−1)/2) · 1.5` where `D` is the day
  count. Status index `j` (frozen order `settled`=0, `pending`=1, `refunded`=2, `failed`=3) →
  bar center `z_j = (j − 1.5) · 1.5`.
- Each bar is an axis-aligned box: footprint 1.0 x 1.0 centered at `(x_i, z_j)`, base at `y = 0`,
  height `h = count · 0.25`. A zero-count cell draws NO geometry and is never pickable.
- Flat, unlit colors. Top face: EXACTLY the status hex above. Side faces: the same color with
  each channel multiplied by 0.62 and rounded. Background clear color: `#0F172A`. Depth testing
  on — near bars occlude far bars.
- Draw nothing but the bars: no floor, no grid, no axes, no in-canvas labels or decorations —
  every pixel that is not a bar face is bare `#0F172A`. Labeling lives in the tooltip and the
  2D table.
- The scene is STATIC between inputs: no idle animation. Frames are drawn on load, on input,
  and on data change.

*Camera contract.* An orbit camera, angles in degrees:

```
θ = yaw · π/180        φ = pitch · π/180        T = (0, 3, 0)
eye = T + distance · ( cos φ · sin θ,  sin φ,  cos φ · cos θ )
f = normalize(T − eye)    r = normalize(f x (0,1,0))    u = r x f
```

For a world point `p`: `q = p − eye`, `xc = q·r`, `yc = q·u`, `zc = q·f`. Points with
`zc ≤ 0.1` do not project. With `fovY = 50°`, `k = 1 / tan(fovY/2)`,
`aspect = Wcss / Hcss` of the canvas:

```
ndcx = (k / aspect) · xc / zc        ndcy = k · yc / zc
sx = (ndcx + 1) / 2 · Wcss           sy = (1 − ndcy) / 2 · Hcss
```

`sx, sy` are CSS pixels relative to the canvas's top-left. GL near/far planes: 0.1 / 200. The
canvas backing store is sized `clientWidth x devicePixelRatio` (likewise height); the rendered
image must agree with this projection at any DPR. Defaults: `yaw = 35`, `pitch = 27`,
`distance = 30`. Clamps: pitch `[5, 85]`, distance `[10, 90]`; yaw unbounded — an
implementation that normalizes yaw modulo 360 is equivalent and accepted; the grader compares
angles modulo 360.

*Interaction contract.*

- Pointer drag on the canvas, per move event with CSS-pixel deltas `Δx, Δy`:
  `yaw ← yaw − 0.35·Δx`, `pitch ← clamp(pitch + 0.35·Δy, 5, 85)`.
- Wheel: `distance ← clamp(distance · exp(0.0012 · deltaY), 10, 90)`. Zooming over the chart
  must NOT scroll the page — the canvas consumes its wheel events.
- Double-click: reset to the defaults.
- Hover: within 150 ms of the pointer resting on a bar, a tooltip `#viz-tooltip` appears near
  the cursor with the text `<count> <status> · <day>` — the day human-readable, e.g.
  `12 settled · 29 Mar 2026`. Off a bar, the tooltip hides.
- Click on a bar: sets `#status-filter` to that bar's status — its `data-value` updates — and
  the table refreshes to match: every rendered row carries that status and the
  `showing X–Y of TOTAL` readout shows the total under the now-active filters.

*Picking* is geometric truth: the bar whose rendered surface is nearest the camera at that CSS
pixel — a partially occluded bar loses to the bar in front of it, exactly as the depth buffer
says.

*2D fallback.* A toggle button `#viz-toggle` (with `aria-pressed`) swaps the canvas for a real
`<table id="viz-fallback">` of the same buckets — one row per day, columns Day, Settled,
Pending, Refunded, Failed, Total, each count cell carrying `data-day` and `data-status` — and
back. The two views always agree because both read `/api/buckets`. If `getContext('webgl')`
(and `webgl2`) returns null — a machine without WebGL — the page must not throw: it shows the
same 2D table automatically, with a visible notice INSIDE the viz panel that 3D is unavailable,
and every other part of the page keeps working. That notice belongs to the fallback state — it
never appears while the 3D view is live.

*Instrumentation contract* — REQUIRED and graded. The page exposes `window.vsdbg`:

```js
window.vsdbg = {
  version: 3,                                    // the literal number 3
  scene(),      // {days: [...], statuses: [...], bars: [{key, i, j, count, x, z, h}, ...]}
                //   key = "<YYYY-MM-DD>|<status>"; zero-count cells omitted
  camera(),     // {yaw, pitch, distance} — live values, degrees
  setCamera({yaw, pitch, distance}),             // applies clamps, renders
  project(x, y, z),  // [sx, sy] CSS px per the camera contract, or null if zc <= 0.1
  pick(sx, sy),      // bar key or null, occlusion-correct
  frames(),          // total frames drawn since load, monotonically increasing
};
```

`vsdbg` must tell the truth: `scene()` agrees with `/api/buckets`, `project()` agrees with the
pixels actually on the canvas, `pick()` agrees with what a user's hover hits. The grader
cross-checks all three against screenshots — an instrumentation layer that reports a scene the
canvas does not show scores as broken, not as clever.

**Dates.** Every timestamp a user sees is rendered human-readable in the user's locale — e.g.
`1 Mar 2026, 14:00`. A raw ISO-8601 string with an offset must never appear in the rendered
page. This covers the Date column, the tooltip, and the last-sync time alike.

**Money.** Amounts render in each row's OWN currency with that currency's minor-unit exponent:
`EUR` and `USD` have 2 decimals, `JPY` has 0, `KWD` has 3. `129900 EUR → €1,299.00`;
`129900 JPY → ¥129,900`; `129900 KWD → KWD 129.900`. Symbol choice, symbol placement and
thousands separators are yours — the digits and the decimal-place count are not: the decimals
must equal the currency's exponent and the digits must equal the stored minor units. A yen
amount with two decimals, or a dinar truncated to two, is wrong money, and money is the product.

**States.** The page handles, visibly and distinctly: **loading**, **empty** (no payments yet —
with a call to sync), and **error** (backend unreachable or erroring — with text a user can act
on). The viz panel additionally owns its own states: `#viz-empty` when every bucket is zero,
`#viz-error` when `/api/buckets` fails. Never a blank panel, never a spinner that never
resolves.

**Responsive.** At a viewport 375 px wide the page lays out cleanly with no horizontal scroll;
the canvas shrinks to full width (min height 240 px) and stays interactive.

**Design.** The page has an intentional visual design: a real palette with strong solid accent
colors, a clear typographic hierarchy, and a branded header bar carrying the app name. Never use
faded pastel washes — pick saturated, solid colors over tints. Never decorate cards or rows with
a left accent line or rail. Never use browser-native controls where custom styling is expected —
no default `<select>`, no `alert()`/`confirm()`/`prompt()` dialogs.

### 6. `vspro/__main__.py` — the entry point

`python -m vspro --db PATH --port N` starts the backend serving the API and the page, then —
after the server is listening — registers the webhook with the vendor. It must not crash when
the database file does not yet exist, and must start (serving whatever is already local) even if
the vendor is briefly unreachable at boot.

### 7. Performance budgets

Measured against the vendor's 1,553-row fixture (14 Berlin days spanning the 2026-03-29 DST
switch, 4 statuses, 4 currencies), on the machine the build runs on:

- First data rows rendered within **2 seconds** of page load.
- The 3D canvas shows its first non-background frame within **3 seconds** of page load.
- `GET /api/payments` at `limit=50` answers in under **150 ms** at p95 — including while a sync
  is running with 8 concurrent readers.
- `GET /api/buckets` answers in under **200 ms** at p95.
- `GET /api/summary` answers in under **150 ms** at p95.
- `POST /api/sync` completes the full fixture within **90 seconds**, documented waits included.
- During a scripted drag, the scene keeps up with the pointer: `vsdbg.frames()` advances by at
  least **0.8 frames per pointer move event delivered** — the scene is event-driven, so each
  move should draw; dropping more than one in five is lag — and a camera change is visible on
  the canvas within **250 ms** of the input that caused it.
- The hover tooltip appears within **150 ms**.
- An optimistic note edit paints the new value within **100 ms** of confirm — before the network
  responds.
- `index.html` + `styles.css` + `app.js` + `viz.js` total at most **150 KB** uncompressed.

---

## Rules

- Amounts are integers in minor units everywhere, end to end. Never floats. Rendering respects
  each currency's minor-unit exponent (EUR 2, USD 2, JPY 0, KWD 3).
- Never sum amounts across currencies — not in the API, not in the UI, not in a tooltip.
- Sorting, comparing and bucketing times happens on the INSTANT, not on the string. Buckets use
  the Europe/Berlin calendar day of the instant.
- Every write to a vendor resource carries `If-Match`. A 412 means someone got there first:
  re-fetch, re-apply, retry once. Never blind-write, never retry a create with a fresh
  idempotency key.
- Every vendor request has a timeout of at most **10 seconds**; a stalled connection is a
  documented vendor behaviour, and a timed-out request is retried per the docs. A sync must
  still land inside its 90-second budget when the vendor stalls once.
- Webhook deliveries are untrusted input until the signature verifies against the RAW request
  body. Duplicates and stale events are normal traffic, silently ignored, and counted.
- The API answers reads while a sync is in flight — a user browsing the table must never wait
  for the vendor.
- The tool runs repeatedly against the same database; a second sync must be cheap (conditional
  requests) and must not duplicate rows or regress webhook-applied versions.
- The frontend ships zero external code and works fully offline. The backend is standard library
  only.
- Include a README.md with the exact commands to install nothing, run the server, and sync.
