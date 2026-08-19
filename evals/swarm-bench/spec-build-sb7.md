# Build `app` — Meridian Payments Console

An operations product for a finance team: two cooperating services that sync payments from the
Meridian API v3, keep them consistent through vendor-pushed webhooks, concurrent edits, crashes
and partitions, run a maker/checker approval workflow that creates real vendor payments, and
give the team a live console — a payments table, a notifications feed, and an interactive 3D
field rendering every one of the 12,288 payments as an instanced column.

The Meridian API v3 documentation is at `{DOCS_URL}`. Read it before you start — every vendor
behaviour you must handle is documented there, and several of them (fixed-size pages,
`Retry-After`, expired cursors, conditional requests and the collection-generation rule,
webhook signatures, transaction groups, idempotency keys) will
defeat a client that did not read. Base URL `{BASE_URL}` — the harness also passes it to your
process as `--vendor`; build against the flag, never a constant. API key `{API_KEY}`; every
vendor request authenticates with it as the docs prescribe.

Work in the current directory. Python 3, standard library only for the backend — no pip
installs (`sqlite3`, `zoneinfo`, `hashlib`, `hmac`, `http`, `threading` are all in the standard
library). The frontend ships ZERO external code — no CDN, no npm, no vendored libraries of any
kind. Everything must work fully offline.

Nothing in a graded run is driven by an operator. Boots, syncs, retries, reconnects, relay
deliveries and post-crash heals are all self-driven; a run that needs a human to click, restart
or nudge anything has already failed.

---

## What to build

### 1. The `app` package — two services, one boot contract

| Command | Effect |
|---|---|
| `python -m app --db-dir P --ledger-port N --notifier-port M --vendor URL --tokens-file T` | boots BOTH services |
| `python -m app.ledgerd --db-dir P --port N --notifier http://127.0.0.1:M --vendor URL --tokens-file T` | boots ledgerd alone |
| `python -m app.notifierd --db-dir P --port M` | boots notifierd alone |

The harness starts and kills the two services independently — the single-command form is a
convenience wrapper, not a lifecycle. Internal module layout inside `app/` is yours; the three
commands, the two database files, the `web/` files and `DECISIONS.md` are the contract.

- Both services bind `127.0.0.1` only, and are listening within **10 seconds** of process
  start — after a fresh boot and after every restart alike.
- `ledgerd` owns `ledger.db`; `notifierd` owns `notifier.db`; one SQLite file per service under
  `--db-dir`. Each service touches ONLY its own file — ledgerd never opens `notifier.db`,
  notifierd never opens `ledger.db`; cross-service truth flows over HTTP only.
- Each service boots cleanly with the vendor down and with the other service down, and neither
  may crash on the other's absence — at boot or at any later moment.
- Restart against an existing `--db-dir` resumes cleanly: idempotent schema init, no data loss,
  no duplicate application of anything already committed.
- `--tokens-file` names a JSON file the harness writes before boot:
  `{"maker": "<32 hex>", "checker": "<32 hex>", "admin": "<32 hex>"}` — the bearer tokens for
  the approval workflow (section 5).
- On boot, once listening, ledgerd starts its first sync unprompted. If the vendor is
  unreachable it keeps serving local data, shows the degraded state in the UI, and retries no
  less often than every **5 seconds** until the sync succeeds — no operator action.

### 2. The collection you are syncing

One payments collection feeds the API, the table, and the 3D field. Its structure is frozen;
its values are seeded per run — build against the structure, never memorized values.

- **N = 12,288 payments** exactly, at fixture load. Payments created during the run (the
  vendor's mid-walk create, approved drafts) append to it.
- The collection spans **96 consecutive Europe/Berlin calendar days** containing exactly one
  Berlin DST transition — one of `2026-03-29`, `2026-10-25`, `2027-03-28`, `2025-10-26`, at
  least 7 days from both ends of the window. Your code must not hardcode which. The day a
  payment belongs to is the Berlin calendar date of its `created_at` INSTANT — not of its raw
  string, and not the UTC date; UTC-day bucketing produces measurably wrong counts and
  measurably wrong 3D positions.
- Per-day count never exceeds **180** (mean ≈ 128). Every day has at least one payment.
- Statuses: `settled`, `pending`, `refunded`, `failed` — each at least 8% of N. Currencies:
  `EUR`, `USD`, `JPY`, `KWD`, minor-unit exponents **2 / 2 / 0 / 3**. Amounts are integers in
  minor units, spanning at least 3 decades in major units. All timestamps RFC3339.
- Vendor list pagination is **server-fixed at 64 per page** — there is no limit parameter;
  12,288 / 64 = **192 pages** for the full walk.
- The vendor **value-dates** every payment created during a run (mid-walk creates and approved
  drafts get a `created_at` INSIDE the existing 96-day span, in a day with
  headroom) — a documented guarantee: the 3D layout basis of section 8 never moves after load.

### 3. `ledgerd` — vendor sync, event ledger, API, UI host

#### Sync discipline

A sync walks the vendor's paginated payments list (192 pages of 64 at fixture count), upserting
into `ledger.db`, and also fetches `GET /v3/reversals` (a small collection) so reversal truth
survives a webhook outage. A payment already present is updated, never duplicated — syncing
twice must not change the count. The walk resumes per the docs after a dropped connection,
honours `Retry-After` on a 500 (a single documented retry, then continue — never a fresh
unconditional restart of committed work), and restarts the cursor on `410 cursor_expired` as
documented.

Later syncs are cheap: use the documented validators (`ETag` / `If-None-Match`) and store the
`X-Collection-Generation` header alongside your validator. **The generation rule:** a `304`
whose `X-Collection-Generation` disagrees with your stored generation is a cache miss — drop
the stored validator and refetch unconditionally, exactly once. Sending more than 3 identical
conditional requests in a row is the infinite-loop bug, and serving stale data as fresh after a
mismatched 304 is worse.

The v3 walk is documented as NOT snapshot-isolated: a payment created mid-walk may or may not
appear on a later page; the webhook is authoritative; you must end with exactly one row for it.
The vendor WILL commit mutations against payments you have already synced and payments you have
not yet reached, WHILE your walk is running, and deliver the webhooks for them before, between
and after your page fetches. A sync page that lands after a webhook already applied a newer
version must not regress the row — upserts compare `version`, never blind-write.

Every vendor request carries a timeout of at most **10 seconds**; a timed-out request is
retried as the docs prescribe. A client with no timeout hangs forever on a stall and blows
every budget downstream.

#### Endpoints

| Method | Path | Response |
|---|---|---|
| `GET` | `/` + `web/*` | the frontend files, correct content types |
| `GET` | `/api/health` | shape below |
| `GET` | `/api/payments?limit=<int>&offset=<int>&status=<s>&currency=<c>&sort=<k>` | `{"data": [...], "total": <int>, "limit": <int>, "offset": <int>}` |
| `GET` | `/api/payments/<id>` | the payment, or 404 envelope |
| `GET` | `/api/summary` | shape below |
| `GET` | `/api/buckets` | shape below |
| `POST` | `/api/sync` | `{"fetched": <int>, "inserted": <int>, "updated": <int>, "total": <int>}` |
| `POST` | `/api/payments/<id>/note` | `{"id": <str>, "note": <str>, "version": <int>}` |
| `POST` | `/api/webhooks/meridian` | vendor-facing; section 4 |
| `GET` | `/api/events?after=<seq>&limit=<int>` | `{"events": [...], "latest_seq": <int>}` |
| `GET` | `/api/outbox/status` | `{"pending": <int>, "delivered": <int>, "last_delivered_seq": <int>, "notifier": "up"\|"down"}` |
| `GET` | `/api/notifications?limit=&offset=` | proxied to notifierd; notifier unreachable → `502`, envelope code `"notifier_unreachable"` |
| `GET` | `/api/viz/records` | section 8 |
| `GET` | `/api/stream` | SSE, section 8 |
| `POST/GET` | `/api/drafts...` | section 5 |

**Health.**

```json
{"status": "ok", "payments": <int>, "last_sync": <str or null>,
 "webhook": {"registered": <bool>, "received": <int>, "applied": <int>,
             "ignored": <int>, "rejected": <int>}}
```

The four webhook counters are live evidence: `received` counts every event-delivery POST that
reached the endpoint (valid or not); `applied` / `ignored` / `rejected` follow section 4. They
count events received by THIS process since it started — keep them in memory; they do not
survive a restart and are not supposed to.

**Payments.** `limit` defaults to 50 and is capped at 200. `offset` defaults to 0. `data` items
carry exactly the keys `id`, `amount_minor`, `currency`, `created_at`, `settled_at`, `status`,
`version`, `note`, `counterparty_name`, `country` — the vendor's nested `counterparty` object
is flattened into the last two. `status` filters to one of `settled`, `pending`, `refunded`,
`failed`; `currency` filters to one of `EUR`, `USD`, `JPY`, `KWD`; the two combine. `sort` is
one of `created_at`, `-created_at`, `amount_minor`, `-amount_minor`; default `created_at`
(ascending by INSTANT). `total` always reflects the active filters. An unknown `status`,
`currency` or `sort` value is a validation error, not an empty result.

**Summary.**

```json
{"count": <int>, "last_sync": <str or null>, "oldest": <str or null>, "newest": <str or null>,
 "by_currency": [{"currency": "EUR", "count": <int>, "total_minor": <int>}, ...],
 "reversals": [{"currency": "EUR", "count": <int>, "total_minor": <int>}, ...]}
```

`by_currency` is sorted by currency code ascending, one entry per currency present.
`reversals` is sorted by currency code ascending and contains ONLY currencies that have
reversals. There is NO cross-currency total anywhere in the response — summing minor units
across currencies is meaningless and forbidden. `oldest` / `newest` are `created_at` of the
earliest and latest payments as RFC3339 **UTC**.

**Buckets.**

```json
{"timezone": "Europe/Berlin",
 "days": ["2026-03-23", "..."],
 "statuses": ["settled", "pending", "refunded", "failed"],
 "cells": [{"day": "2026-03-23", "status": "settled", "count": <int>}, ...]}
```

`days` is every calendar day from the first to the last, ascending, no gaps — 96 entries at
fixture load. `cells` contains one entry for EVERY (day, status) pair — `days x statuses`,
count 0 included, 384 cells at fixture load — ordered day-major, statuses in the frozen order
above. Bucketing is instant-based Berlin days; the DST transition is where UTC-day code goes
visibly wrong.

**Sync.** `POST /api/sync` runs a sync and answers with the counts. Vendor unreachable → `502`
with the envelope, code `"vendor_unavailable"`, and the app keeps serving local data. The API
keeps answering reads while a sync is in flight: `GET /api/payments`, `GET /api/summary`,
`GET /api/buckets` and `GET /api/viz/records` must respond normally — inside their latency
budgets — during a running sync. A single-threaded server that parks every reader behind the
sync's vendor waits fails this by construction; the standard library has what you need.

**Note.** `POST /api/payments/<id>/note` with body `{"note": <str>}` (1–280 chars) writes the
note through to the vendor with the documented `If-Match` optimistic-concurrency dance: on
`412 Precondition Failed`, re-fetch the resource, re-apply the note on the fresh version, retry
ONCE with the new `If-Match`. A second 412 → respond `409` with the envelope, code
`"conflict"`, local row unchanged. Never write without `If-Match` — the vendor answers
`428 Precondition Required`, and that response is a bug in your client. On success, persist the
returned resource locally and respond with the new `version`.

#### The event ledger

Every state change ledgerd applies appends EXACTLY ONE event to an append-only log in
`ledger.db`. `seq` is strictly increasing and contiguous from 1 — a gap is evidence of a lost
write and is graded as one.

```json
{"seq": 217, "type": "payment.updated", "payment_id": "pay_x", "version": 3,
 "source": "webhook", "txn": null, "at": "<rfc3339 UTC>"}
```

- `type` ∈ `payment.created | payment.updated | reversal.created | draft.created |
  draft.submitted | draft.approved | draft.rejected | payment.sent`.
- `source` ∈ `sync | webhook | local | approval` — the initial walk appends one
  `payment.created` per new row (source `sync`); an upsert that changes a row appends
  `payment.updated`; a page that changes nothing appends nothing; a local note write appends
  `payment.updated` (source `local`); workflow actions use source `approval`.
- `txn` carries the vendor transaction-group id when the change is part of one, else `null`.
- Duplicate and stale webhook outcomes appear ONLY in the webhook counters — never as events.

`GET /api/events?after=<seq>&limit=<int>` returns events with `seq > after`, ascending, plus
`latest_seq`. It requires a bearer token (any of the three roles).

#### The outbox

Exactly five event types cross to the notifier: `draft.submitted`, `draft.approved`,
`draft.rejected`, `reversal.created`, `payment.sent`. Each is written to an outbox table **in
the same SQLite transaction as the state change it records** — commit-then-POST and
POST-then-commit are both the dual-write bug, and the graded run arranges the exact kill window
that exposes it.

A background relay delivers outbox rows to notifierd in batches of at most **50**, ascending
`seq`, via `POST /notify/events`, retrying with backoff capped at **2 seconds**,
at-least-once; a row is marked delivered only after a 200. The relay never runs inside a user
request handler, and a user write NEVER blocks on notifier availability — while the notifier is
down, writes commit, `pending` grows, and the relay catches up after the heal, in `seq` order.
After a ledgerd restart the relay resumes from the durable outbox rows.

#### Error envelope

Every error this API returns uses ONE structured envelope:

```json
{"error": {"code": "<snake_case>", "message": "<human sentence>",
           "field_errors": [{"path": "items[2].amount.value_minor", "code": "not_an_integer"}]}}
```

`field_errors` appears only on validation failures (HTTP 400) and uses dot paths with
`[index]` for arrays. Frozen `code` vocabulary for field errors: `required`, `not_an_integer`,
`not_positive`, `unsupported`, `too_long`, `bad_format`. Envelope codes: `bad_request`,
`not_found`, `conflict`, `unauthorized`, `forbidden`, `approval_forbidden`, `bad_signature`,
`vendor_unavailable`, `notifier_unreachable`. An unknown path is 404 with the envelope, code
`"not_found"`. A bad `limit`/`offset` — non-numeric or negative — is 400 with a `field_errors`
entry naming the parameter. Every response is JSON except the static frontend assets and the
SSE stream.

### 4. Webhooks — the vendor calls YOU

On startup, AFTER ledgerd is bound and listening, the app registers
`http://127.0.0.1:<ledger-port>/api/webhooks/meridian` with the vendor as the docs prescribe
(registration is idempotent by URL and retried until the vendor is reachable). Registration
triggers the documented challenge handshake: the vendor POSTs
`{"type": "webhook.verify", "challenge": "<hex>"}` to the URL and the endpoint must answer
`200` with `{"challenge": "<the same hex>"}` — this request is unsigned, because the secret
does not exist until registration completes. The verification challenge is part of
registration, not an event delivery — it does not increment any counter.

Every subsequent delivery is a signed event:

```json
{"id": "evt_00c4", "type": "payment.updated", "created_at": "<rfc3339 UTC>",
 "txn": null,
 "data": { <the full payment or reversal object, including "version"> }}
```

with header `Meridian-Signature: t=<unix seconds>,v1=<hex>` where
`v1 = HMAC_SHA256(secret, "<t>.<raw request body>")` — the raw bytes, not a re-serialization.

The endpoint must, deterministically:

- verify the signature FIRST; missing or wrong → `401` with the envelope, code
  `"bad_signature"`, state untouched, `rejected` +1;
- apply valid events idempotently and in version order: an event id already processed →
  `ignored` +1, state untouched; an event whose `version` is not greater than the stored row's
  → `ignored` +1, state untouched (the vendor does not guarantee delivery order — v+2 WILL
  arrive before v+1, and the late v+1 must not overwrite); otherwise apply, append the ledger
  event, `applied` +1. Respond `200 {"received": true}` in all three cases;
- count every delivery arrival in `received`, valid or not;
- answer within **3 seconds** and never trigger a sync or any vendor call from inside the
  handler.

**Transaction groups.** A refund commits two changes atomically on the vendor: the payment
flips `status → refunded` (version bump) and a reversal appears — same `payment_id`, same
`amount_minor`, same `currency`. Both webhooks carry `"txn": {"id": "txn_9", "part": 1,
"of": 2}` (parts may arrive in either order, race other traffic, and be duplicated like any
delivery). The v3 docs are explicit: consumers MUST apply a transaction group atomically —
stage parts until the group is complete, then apply them in ONE local transaction, appending
the group's ledger events together. No API read may ever observe a half-applied group: a
summary showing the refunded payment without its reversal total (or the reverse) is the graded
failure. Reversals never change the payment vocabulary — payments keep the frozen four
statuses; reversals surface in the summary `reversals` block and in the notifier.

The vendor WILL deliver duplicates, WILL deliver events out of order, WILL (once) deliver a
forged signature, and WILL deliver webhooks concurrently with your sync walk. The four health
counters are the ledger of how the app handled all of it.

### 5. The approval workflow — maker, checker, admin

Three bearer tokens from the tokens file. Authentication is `Authorization: Bearer <token>` on
every drafts endpoint and on `/api/events`. Missing or unknown token → `401`, envelope code
`"unauthorized"`. Known token, wrong role → `403`, code `"forbidden"`. `admin` reads
everything — drafts, events — and writes nothing.

**Draft state machine (frozen):** `draft → submitted → approved | rejected`, and
`approved → sent` once the vendor accepts the payment. Whether a rejected draft is terminal or
resubmittable is YOUR published decision — corner D2 in `DECISIONS.md` (section 9).

| Method | Path | Role | Effect + ledger event |
|---|---|---|---|
| `POST` | `/api/drafts` | maker or checker | create from `{"amount_minor": <int>, "currency": <str>, "counterparty": {"name": <str>, "country": <str>}, "note": <str>}` → `draft.created` |
| `POST` | `/api/drafts/<id>/submit` | maker or checker | state `submitted` → `draft.submitted` (outbox) |
| `POST` | `/api/drafts/<id>/approve` | checker | state `approved` → `draft.approved` (outbox), then SEND (below) |
| `POST` | `/api/drafts/<id>/reject` | checker | state `rejected` → `draft.rejected` (outbox) |
| `GET` | `/api/drafts?state=` | any role | `{"data": [...], "total": <int>}`, filtered by state; unknown state = validation error |

**Four-eyes.** The approver must not be the submitter: an approve or reject attempted with the
SAME bearer token that submitted the draft → `403`, code `"approval_forbidden"`, state
untouched.

Draft validation: `amount_minor` a positive integer; `currency` one of
the four; `name` 1–80 chars; `country` exactly two uppercase letters; `note` 0–280 chars. A
draft object carries at least `id`, `state`, `amount_minor`, `currency`, `counterparty`,
`note`, `created_at`.

**The SEND.** On approve, after committing `approved` + `draft.approved`, the app creates the
real payment: `POST /v3/payments` with the documented `Idempotency-Key` header. Store the key
WITH the draft before the first attempt. On 2xx, append `payment.sent` (outbox-crossing; no
notification). If the send is interrupted — crash, timeout, vendor stall — the retry MUST
reuse the stored key: the vendor returns the same payment for a reused key, and a fresh key
per retry is the seeded duplicate-payment bug. Exactly one vendor payment per approved draft,
ever. The created payment is value-dated in-span and flows back through webhook/sync like any
other payment; your final totals include it.

**Durability.** `submitted` and `approved` are durable the moment their 200 is written: a
SIGKILL immediately after either — including mid-send — must, after restart, find the state
intact, the outbox event preserved, and the send completed or safely retried with the same
key. An approved draft that reverts, a submitted draft that vanishes, or a doubled vendor
payment are the graded failures.

### 6. `notifierd` — the idempotent consumer

| Method | Path | Response |
|---|---|---|
| `POST` | `/notify/events` | `{"events": [...]}` → `{"accepted": [<seq>...], "duplicate": [<seq>...]}` |
| `GET` | `/health` | `{"status": "ok", "received": <int>, "applied": <int>, "duplicate": <int>, "notifications": <int>}` |
| `GET` | `/notify/processed?after=<seq>` | `{"processed": [{"seq": <int>, "type": <str>}...], "latest_seq": <int>}` — durable |
| `GET` | `/notify/notifications?limit=&offset=` | `{"data": [{"id": <str>, "event_seq": <int>, "kind": <str>, "message": <str>, "at": <str>}...], "total": <int>}` newest first |

- **Idempotent by `seq`.** The dedupe key is the ledger event `seq`. A seq already in the
  durable processed set → reported in `duplicate`, state untouched. A batch mixing new and
  already-seen events applies the new ones and reports each seq in the right list — a
  duplicate never aborts the batch.
- The processed set and the notification rows are DURABLE in `notifier.db` — they survive
  SIGKILL and restart, and exactly-once is graded on them. The `received` / `applied` /
  `duplicate` counters are in-memory per-process, like ledgerd's webhook counters.
- **Selective materialization.** Exactly four event types produce exactly one notification row
  each: `draft.submitted`, `draft.approved`, `draft.rejected`, `reversal.created`.
  `payment.sent` is processed and recorded in the processed set but produces NO notification.
  Notify-everything and notify-nothing are both wrong. `kind` is the event type; `message` is
  a human sentence.

### 7. `web/` — the frontend

A single page, served by ledgerd at `GET /`. Plain HTML/CSS/JS, no build step, no CDN, no
external code of any kind — it must work offline. This page is what the finance team uses
every day. Build it as a product, not as a debug view over the API.

Ship it as FOUR files, each owned and written separately: `web/index.html` (structure only),
`web/styles.css` (all styling), `web/app.js` (page behavior: table, filters, sync, notes,
workflow, notifications), and `web/viz.js` (the 3D engine, nothing else). The backend serves
all four with correct content types; the page references them with relative paths. Combined
size of the four files: at most **150 KB** uncompressed — hand-written code fits in a tenth of
that; the budget exists so that a vendored library cannot.

The page shows, top to bottom: a branded header bar (`#app-header`) carrying the product name;
the summary; the 3D field panel; the payments table; the notifications feed and the drafts
panel (side by side on wide viewports, stacked on narrow ones).

**Summary** (`#summary`). One element per currency present, class `cur-total`, attribute
`data-currency`, showing the payment count and the total formatted in that currency. For each
currency with reversals, an element class `rev-total`, attribute `data-currency`, showing the
reversal count and total. Never a combined cross-currency figure. The last-sync time
(`#last-sync`) reads human, or `Never synced` when there is none. A **Sync now** button
(`#sync-now`) calls `POST /api/sync`, shows a visible in-flight state
(`data-state="syncing"`, control disabled), and refreshes every view on completion. A failed
sync (vendor down) surfaces as a non-blocking notice in `#notice` (`role="status"`) and the
button returns to idle — local data keeps rendering.

**Table.** Columns Date, Amount, Status, Counterparty, Note. Server-driven through the
documented `limit`/`offset`/`status`/`currency`/`sort` parameters — the table never fetches
the whole collection to paginate in memory, and never renders all rows in one scroll when more
than 50 exist. (`/api/viz/records` is the 3D field's sanctioned full fetch; the table never
uses it.)

- **Rows** carry `data-id` (the payment id) and `data-brushed` (`"true"` while the record is
  in the brush set, absent or `"false"` otherwise). Clicking a row toggles that record in the
  brush set (section 8) — except clicks on the Note cell, which open the editor and never
  touch the brush.
- **Pagination:** **Prev**/**Next** buttons (`#prev`, `#next`) and a `showing X–Y of TOTAL`
  readout, where TOTAL is the filtered total.
- **Sorting:** the Date and Amount column headers are clickable and toggle
  ascending/descending, reflected in `aria-sort` on the header cell and driven through the
  API's `sort` parameter.
- **Filters:** a status filter (`#status-filter`) and a currency filter (`#currency-filter`),
  each a custom dropdown (never a native `<select>`), each actually changing the rows AND the
  TOTAL readout. Each filter element carries a `data-value` attribute that ALWAYS reflects the
  current selection: the exact lowercase status, or exact uppercase currency code, and the
  empty string `""` when the filter is off. The grader reads `data-value` and nothing else to
  learn what is selected.
- **Status badges:** `settled` `#059669`, `pending` `#D97706`, `refunded` `#7C3AED`, `failed`
  `#B91C1C` — the same four hex values the 3D field uses. Distinct in computed color, not only
  in text.
- **Notes, optimistically:** each row's Note cell is editable through a custom inline editor
  (never `prompt()`). On confirm the new value paints IMMEDIATELY — before the network
  responds — with the row in `data-state="saving"`; success moves it to `data-state="saved"`;
  a `409` reverts the cell to the previous value and shows a non-blocking notice in `#notice`
  (`role="status"`), never an `alert()`.

**Notifications feed** (`#notifications`). Reads ONLY the ledgerd proxy
(`GET /api/notifications`), polling at an interval of at most **5 seconds**. Rows newest
first, each carrying `data-event-seq` and `data-kind`, showing kind, message and a
human-readable time. The container carries `data-state="live"` normally; when the proxy
answers 502 it flips to `data-state="degraded"` with a visible degraded treatment, and it
recovers to `"live"` — without a reload — within **5 seconds** of the notifier healing.

**Drafts panel.** A token input (`#role-token`) — the bearer the page sends on every drafts
call; a create form (`#draft-form`) with amount, currency, counterparty name/country, note; a
draft list (`#draft-list`) whose rows carry `data-draft-id` and `data-state`, click to select
(`data-selected="true"` on the selected row); action buttons `#submit-btn`, `#approve-btn`,
`#reject-btn` acting on the selected draft, each enabled only when the action is legal for the
draft's state. Auth errors (401/403/`approval_forbidden`) surface in `#notice`, non-blocking.
The full journey works through the UI alone: maker token → create → submit; checker token →
approve (or reject); the notifications feed shows the submitted/approved rows; the vendor
round-trip lands the new payment in the table.

**States.** The page handles, visibly and distinctly: **loading**, **empty** (no payments yet —
with a call to sync; whether the table renders empty-with-progress or blocks before the first
sync completes is corner D3), and **error** (backend unreachable or erroring — with text a
user can act on). The viz panel additionally owns its own states: `#viz-empty` when there are
no records, `#viz-error` when `/api/viz/records` fails. Never a blank panel, never a spinner
that never resolves. If `getContext('webgl')` and `webgl2` both return null, the page must not
throw: the viz panel shows a visible notice that 3D is unavailable and every other part of the
page keeps working. The console stays clean through every journey — zero errors, zero
unhandled rejections.

**Dates.** Every timestamp a user sees is rendered human-readable in the user's locale — e.g.
`1 Mar 2026, 14:00`. A raw ISO-8601 string with an offset must never appear in the rendered
page. This covers the Date column, the labels' source data, the notifications feed, and the
last-sync time alike.

**Money.** Amounts render in each row's OWN currency with that currency's minor-unit exponent:
`EUR` and `USD` have 2 decimals, `JPY` has 0, `KWD` has 3. `129900 EUR → €1,299.00`;
`129900 JPY → ¥129,900`; `129900 KWD → KWD 129.900`. Symbol choice, symbol placement and
thousands separators are yours — the digits and the decimal-place count are not: the decimals
must equal the currency's exponent and the digits must equal the stored minor units. A yen
amount with two decimals, or a dinar truncated to two, is wrong money, and money is the
product.

**Responsive.** At a viewport 375 px wide the page lays out cleanly with no horizontal scroll;
the canvas shrinks to full width (min height **240 px**) and stays interactive.

**Design.** The page has an intentional visual design: a real palette with strong solid accent
colors, a clear typographic hierarchy, and a branded header bar carrying the product name.
Never use faded pastel washes — pick saturated, solid colors over tints. Never decorate cards
or rows with a left accent line or rail. Never use browser-native controls where custom
styling is expected — no default `<select>`, no `alert()`/`confirm()`/`prompt()` dialogs.

### 8. The 3D field — 12,288 instances, five mechanisms

An interactive 3D field rendering EVERY payment as one instanced column on a day × in-day-rank
grid, with GPU picking, an inertial camera, collision-culled labels, a brush linked to the
table, and live streaming diffs. Raw WebGL — no three.js, no library, no exceptions; the asset
budget enforces it. Every contract below is FROZEN — the grader recomputes this math
independently and compares it to your API, your pixels, your picking, and your GL call stream.

#### Data → scene

`GET /api/viz/records` returns the full collection, columnar, one fetch:

```json
{"count": N,
 "id": [...], "amount_minor": [...], "currency": [...], "status": [...],
 "created_at": [...], "day": [...], "version": [...]}
```

All arrays length N, initial order `(created_at instant ASC, id ASC)`. `day` is the
**server-computed Europe/Berlin calendar date** (`YYYY-MM-DD`) — the backend owns DST; a
frontend recomputing days in UTC produces probeably wrong x positions.

**Instance identity.** `n` = **stable arrival index**: initial records take their serve-order
position (0-based); each streamed create appends at `n = current count`. `n` NEVER re-sorts —
not on filter, not on brush, not on any mutation. Pick encoding, digest indexing, label
binding and every `vs7dbg` answer key off this `n` and the record `id`.

**Layout basis** — locked when the page first renders a non-empty `/api/viz/records` response,
exposed via `vs7dbg.layout()` as `{"d0": "<YYYY-MM-DD>", "D0": 96, "R0": <int>}`: `d0` = first
day present, `D0` = 96 (the span), `R0` = max in-day count at load. The basis does NOT change
when streamed records arrive — the vendor's in-span value-dating and per-day headroom
guarantee it.

**Per-instance transform** — `d` = Berlin day − `d0` in calendar days; `r` = in-day rank at
load for initial records (the same `(created_at ASC, id ASC)` sort restricted to the day); a
streamed create takes `r = current in-day count` at apply time:

```
Δ = 1.2                                  (cell pitch, world units)
x = (d − (D0 − 1)/2) · Δ
z = (r − (R0 − 1)/2) · Δ
footprint 0.9 × 0.9 centered at (x, z), base y = 0
a_major = amount_minor / 10^exp(currency)      exp: EUR 2, USD 2, JPY 0, KWD 3
h = clamp(0.9 + 0.55 · log10(a_major), 0.2, 4.2)
```

Height goes through the currency exponent on purpose: a client that forgets JPY = 0 or
KWD = 3 renders measurably wrong heights, and the grader measures rendered column tops in
device pixels (**±3 px** at a close-up pose, JPY and KWD instances included). Every record
renders every frame — view-frustum culling is legal; LOD and decimation are not.

**Colors** (flat, unlit, exact — the grader reads pixels with a ±8-per-channel tolerance):

| role | rule |
|---|---|
| top face | `settled #059669 (5,150,105)` · `pending #D97706 (217,119,6)` · `refunded #7C3AED (124,58,237)` · `failed #B91C1C (185,28,28)` |
| side faces | `round(0.55 · top)` per channel |
| brushed-dim (brush non-empty, instance NOT in it) | base `c' = round(0.30 · c)` applied BEFORE the side factor: side-of-dim = `round(0.55 · round(0.30 · c))` |
| background | `#101828 (16,24,40)`; every non-face pixel is exactly this — no floor, grid, axes, or in-canvas text |

**Scene digest** — `vs7dbg.sceneDigest()` returns, rounded to 4 decimals:

```
{count, Sh: Σh, Sh2: Σh², Sx: Σx, Sz: Σz, Sxh: Σx·h, Szh: Σz·h, brushedCount}
```

sums over ALL current records in float64. Graded against an independent recomputation with
tolerance `|Δ| ≤ max(0.5, 1e-4·|expected|)` — a single wrong cell exceeds it. The digest is
cross-checked against rendered pixels; a digest the canvas does not show scores as broken, not
as clever.

#### Rendering — bounded draw calls, demand rendering

- `<canvas id="viz3d">`, context `webgl` or `webgl2` created
  `{antialias: false, alpha: false}`, on the MAIN thread — no OffscreenCanvas, no Worker:
  `vs7dbg` needs synchronous scene access. Backing store sized `clientWidth × devicePixelRatio`
  (likewise height); the rendered image must agree with the projection math at any DPR.
- Depth testing ON; draw order is yours — the graded pick set kills last-drawn-wins in both
  index orders, so only the depth buffer's answer survives.
- **Draw budget: at most 8 draw calls to the default framebuffer per rendered frame at full
  count.** The harness wraps and counts every GL entry point — `drawArrays`, `drawElements`,
  their instanced forms, the `ANGLE_instanced_arrays` extension objects — and classifies each
  call by the framebuffer bound at call time. Over a scripted drag of M pointer moves (the
  graded drag uses **M = 40**; window = first move → pointerup, sampled at pointerup + one
  rAF), default-framebuffer draws must satisfy BOTH `≤ 8 · max(frames drawn, 1)` and
  `≤ 8 · (M + 8)`. At N = 12,288 the budget forces instanced draws or a merged buffer — both
  legitimate; the budget is the interface, not the technique.
- **Demand rendering.** At rest — no input, no active coast, no pending stream batch — **0
  default-framebuffer draws over any 500 ms window**. Draw on load, on input, during coast,
  and on data change; never on an idle rAF loop.
- Per-frame uniform uploads are free. A "buffer realloc" is any `bufferData` call with
  `byteLength > 4096`; where reallocs and upload bytes are forbidden or bounded is the
  streaming section below.

#### The pick buffer

Picking is GPU truth, never a CPU raycast. Maintain an offscreen framebuffer (RGBA8 color +
depth attachment), sized exactly to the drawing buffer in device pixels, no MSAA. Every
instance renders into it with an identity color:

```
idNum = n + 1                (n = stable arrival index; 0 = background)
r = idNum & 255,  g = (idNum >> 8) & 255,  b = (idNum >> 16) & 255,  a = 255
clear color (0,0,0,0) or (0,0,0,255) — decode ignores a
decode: idNum = r + 256·g + 65536·b;  0 → background, else record id[idNum − 1]
```

Depth testing ON in the pick pass — the nearest rendered surface wins, exactly as the depth
buffer says: a partially occluded instance loses to the instance in front of it, whatever
their indices.

`vs7dbg.pick(sx, sy)` answers from the pick buffer at device pixel
`(round(sx·DPR), Hdev − 1 − round(sy·DPR))` against the live camera;
`vs7dbg.pickPixel(sx, sy)` returns the raw `[r, g, b, a]` bytes from the pick FBO at the same
mapping. The grader checks all three agree: decode(pickPixel) == pick's answer == the
analytically correct front instance.

**Real-pass accounting** (the wrapper watches): after each scene invalidation (camera change
or applied stream batch), the FIRST `pick`/`pickPixel` call must be accompanied by at least 1
offscreen draw AND at least 1 offscreen `readPixels` since the invalidation; subsequent picks
may serve from a CPU-side cache of that readback — legal, good engineering. At most **4
offscreen draw calls** per pick-buffer refresh. A `pick()` call causes **0 default-framebuffer
draws** — ID colors never flash on the visible canvas.

**Click semantics:** a pointerup within **5 px** and **300 ms** of its pointerdown on the
canvas is a click. Click on an instance toggles it in the brush set; click on background
clears the brush.

#### Camera — orbit + inertia

Angles in degrees. The projection contract, which `vs7dbg` and your rendering must both obey:

```
θ = yaw·π/180   φ = pitch·π/180   T = (0, 1, 0)
eye = T + distance · (cos φ · sin θ,  sin φ,  cos φ · cos θ)
f = normalize(T − eye)   r = normalize(f × (0,1,0))   u = r × f
q = p − eye;  xc = q·r;  yc = q·u;  zc = q·f;   zc ≤ 0.5 → does not project
fovY = 50°,  k = 1/tan(fovY/2),  aspect = Wcss/Hcss,  near 0.5 / far 1000
ndcx = (k/aspect)·xc/zc    ndcy = k·yc/zc
sx = (ndcx+1)/2·Wcss       sy = (1−ndcy)/2·Hcss        (CSS px, canvas top-left)
```

Defaults: `yaw = 30`, `pitch = 40`, `distance = 260`. Clamps: pitch `[5, 85]`, distance
`[15, 340]`; yaw unbounded — the grader compares yaw modulo 360.

- **Drag:** per pointermove with CSS-pixel deltas: `yaw ← yaw − 0.30·Δx`,
  `pitch ← clamp(pitch + 0.30·Δy, 5, 85)`.
- **Wheel:** `distance ← clamp(distance · exp(0.0012·deltaY), 15, 340)`. Zooming over the
  field must NOT scroll the page — the canvas consumes its wheel events.
- **Double-click:** reset to the defaults AND zero all angular velocity.

**Inertia.** Angular velocity `(vyaw, vpitch)` in deg/s. At pointerup it equals the rate
implied by the LAST TWO move events — `v = 0.30·Δpx/Δt`, drag sign preserved. After release
the camera coasts under exponential decay with **τ = 0.4 s**:

```
v(t) = v0 · e^(−t/τ)
yaw(t) = yaw0 + v0·τ·(1 − e^(−t/τ))
stop when |vyaw| < 2 and |vpitch| < 2   (deg/s; demand rendering resumes — no further draws)
```

Pitch clamps apply continuously during the coast; hitting a clamp zeroes `vpitch`.
`pointerdown` or double-click cancels the coast; wheel does NOT. Implement the closed form (or
integrate against real elapsed time) — a per-frame constant decay tuned for 60 Hz drifts
measurably at the harness's frame cadence and fails the graded identities:

- **Remaining-coast identity:** at any coasting instant,
  `yaw_rest − yaw(t) = v(t)·τ`, graded from two mid-coast samples of `vs7dbg.camera()` and the
  rest pose, tolerance `|yaw_rest − (yaw_t + v_t·τ)| ≤ max(1.0°, 0.15·|v_t·τ|)`.
- **Reality:** after a fast flick (≥ 600 px/s ⇒ `v0` ≈ 180°/s), yaw keeps moving in the drag
  direction at least **3°** past release — confirmed from `camera()` AND from mid-coast pixel
  projection.
- **Slow release:** a drag whose last-move rate is below **6 px/s** starts no visible coast:
  `|yaw_rest − yaw_release| ≤ 0.5°`.
- **Settle budget:** from release, the coast settles within
  `τ·ln(max(v0, 2)/2) + 0.7 s`, capped at **2.5 s**, with `v0` = `|camera().vyaw|` at release.

Harness guarantees you can rely on: scripted drags have pinned move counts and spacing; the
two release-velocity moves are dispatched at least 30 ms apart; drags that must not coast end
below 6 px/s.

#### Screen-space labels — deterministic collision culling

- **Candidates:** the **12** records with highest `a_major` (ties broken by `id` ASC). The
  fixture guarantees distinct amounts and at least 6 distinct days among them.
- **Anchor:** `A = project(x, h, z)` of the instance's top-center under the live camera.
- **Eligibility:** `A` non-null, inside the canvas, AND `pick(A.sx, A.sy)` returns that
  instance — labels are occlusion-culled through YOUR pick buffer. A label floating over an
  instance the pick buffer says is hidden is the graded failure.
- **Geometry:** each label is a DOM element in `#viz-labels` (absolutely positioned over the
  canvas, never drawn into it), class `viz-label`, attribute `data-id`, border-box exactly
  **110 × 18 CSS px** (single line, ellipsized), top-left at `(A.sx + 10, A.sy − 9)` ± 2 px.
  Text contains the record's amount formatted in its OWN currency — the money rules apply here
  too.
- **Culling:** consider candidates in priority order (`a_major` DESC, `id` ASC); show a label
  iff it is eligible AND its rect intersects (by ≥ 1 px) no already-shown rect; otherwise cull
  it — hidden or absent, never nudged, never given an alternate position. Rendered labels
  never overlap, at any pose. Labels update every rendered frame and re-cull after
  `vs7dbg.setCamera`.

#### The linked brush — table ⇄ instances

ONE brush set of record ids; `vs7dbg.brush()` returns it as an array sorted ascending by id.
`#brush-count` always shows its size.

- **Table → 3D:** clicking a table row toggles that record. While a record is in the set its
  row carries `data-brushed="true"`. When the set is non-empty, non-members render at the 0.30
  dim and members keep their exact status hex — graded by pixel probes on member and
  non-member tops.
- **3D → table:** clicking an instance toggles it AND, when the record matches the active
  filters, navigates the table to the page containing it under the current sort, with the row
  `data-brushed="true"` and scrolled into view. A record filtered out just toggles — set,
  count and dim still update.
- **Background click** clears the set and lifts the dim — pixels back to full status hex.
- **Cost:** the dim is a per-instance flag plus a uniform, never a geometry rebuild: any
  single brush toggle or clear uploads at most `stride + 4096` buffer bytes and performs no
  realloc.
- Whether the brush survives a streamed mutation of a brushed record is YOUR published
  decision — corner D1 (section 9); the run mutates a brushed record and checks you do what
  you documented.

#### Streaming diffs — SSE with byte accounting

The page consumes `GET /api/stream` (SSE, `text/event-stream`, concurrent subscribers
supported). Every payment-state change committed while a subscriber is connected reaches it in
exactly one message; the initial dataset comes from `/api/viz/records`, never from replay.
Each message is one atomic batch — `batch` numbers increase monotonically per connection:

```json
{"batch": <int>, "records": [{"id": ..., "amount_minor": ..., "currency": ...,
                              "status": ..., "created_at": ..., "day": ..., "version": ...}, ...]}
```

- **Diff rule:** applying a batch touches exactly the minimal changed-instance set `S`: a
  status flip touches 1 instance; a create touches 1 (appends at `n = count`,
  `r = current in-day count`; NOTHING re-ranks, NOTHING re-sorts).
- **Upload accounting:** during a batch-apply window, uploaded buffer bytes (`bufferData` +
  `bufferSubData`, all targets) total at most `|S| · stride + 4096`, and no realloc
  (`bufferData` > 4096 bytes) occurs. A full-array re-upload per message is the graded
  failure; the wrapper counts every byte.
- After apply, the digest reflects the change, the changed instance's pixels show the new
  color/height at a close-up pose, and a batch that mutates a brushed record triggers your
  documented D1 behavior.

#### `vs7dbg` — REQUIRED and graded

The page exposes `window.vs7dbg`, all methods synchronous:

```js
window.vs7dbg = {
  layout(),        // {d0, D0, R0} — the locked layout basis
  sceneDigest(),   // {count, Sh, Sh2, Sx, Sz, Sxh, Szh, brushedCount} — 4 decimals
  camera(),        // {yaw, pitch, distance, vyaw, vpitch} — live values, degrees and deg/s
  setCamera(yaw, pitch, distance),  // applies clamps, cancels any coast, renders, re-culls labels
  pick(sx, sy),    // {id, index} or null — index = n, from the pick buffer, occlusion-correct
  pickPixel(sx, sy), // [r, g, b, a] raw bytes from the pick FBO at the same pixel mapping
  brush(),         // array of record ids, ascending
  frames(),        // total frames rendered since load, monotonically increasing
};
```

`vs7dbg` must tell the truth: `camera()` agrees with the pixels the projection puts on the
canvas, `sceneDigest()` agrees with the data AND with measured column heights, `frames()`
agrees with the wrapper's counted draws, `pick()` agrees with `pickPixel()` agrees with what a
user's click hits. An instrumentation layer that reports a scene the canvas does not show
scores as broken, not as clever.

### 9. `DECISIONS.md` — three corners you must decide

The spec deliberately leaves three corners unstated. Decide each, ship the decision, and
document it in `DECISIONS.md` with EXACTLY these headings:

- `## D1` — does the brush survive a streamed mutation of a brushed record (stay brushed vs
  drop out)?
- `## D2` — is a rejected draft terminal, or resubmittable?
- `## D3` — before the first sync completes, does the table render empty-with-progress, or
  block behind a loading state?

Two or three sentences each: the choice, and why. Either answer passes; an undocumented corner
or a document that contradicts observed behavior does not — the run exercises all three.

---

## What WILL happen during a graded run

The schedule is seeded per run — positions and targets change, the boundaries do not. All of
it is normal operation; none of it excuses a crash, a hang, a duplicate, or a lost row.

- The vendor is DOWN for the first 3–8 seconds of a boot. You bind anyway, serve local data,
  show the degraded state, and complete the first sync unprompted once it returns.
- During the first walk: one connection dropped mid-page-stream, and one `500` with
  `Retry-After` — resume and retry per the docs; never restart committed work unconditionally.
- Webhooks race the walk: mutations against already-served AND not-yet-served pages, delivered
  both before and after the page bodies they race. Plus: one out-of-order pair (v+2 delivered
  before v+1), one duplicate delivery, one forged signature, one mid-walk create, and one
  refund transaction group (two parts).
- One later conditional sync gets a `304` whose `X-Collection-Generation` mismatches — the
  generation rule applies, exactly once.
- `ledgerd` is SIGKILLed mid-sync and restarted. It resumes, converges, and duplicates
  nothing.
- `notifierd` is SIGKILLed. While it is down, ledgerd commits further events (8 in the graded
  schedule, several outbox-crossing) — user writes never block, `/api/outbox/status` reports
  `"down"` with growing `pending`, the UI feed shows `data-state="degraded"`. After restart
  the relay catches up in seq order, the processed set dedupes to exactly-once, and the feed
  returns to `"live"` within 5 seconds, no reload.
- `ledgerd` is SIGKILLed between an outbox commit and its delivery — the restart delivers from
  the durable outbox; nothing is lost, nothing doubles.
- `ledgerd` is SIGKILLed immediately after a draft submit's 200, and again immediately after
  an approve's 200 with the vendor send still in flight. Restart finds the states intact and
  finishes the send with the SAME idempotency key: exactly one vendor payment, exactly one
  `payment.sent`.

Every kill is followed by a restart with the same flags. Convergence — not heroics at the
moment of the kill — is what is graded.

## Consistency rules — graded continuously over the live run

The grader polls your API throughout the run and replays your event log and the notifier's
processed set against the vendor's commit ledger. These hold at EVERY observable instant, not
just at the end:

- **No invented states.** Every `(payment_id, version)` you apply or serve exists in the
  vendor's committed history.
- **Per-key order.** Applied versions per payment strictly increase in event-log order;
  duplicate and stale webhook outcomes appear in the counters, never as events.
- **Monotonic reads.** The version served for a payment never decreases from one read to the
  next. A sync page landing after a webhook applied v+1 must not regress the row.
- **Convergence.** At quiescence, every payment's version and status equals the vendor's
  final committed state, and your row count equals the vendor's — the mid-walk create present
  exactly once.
- **Group atomicity.** No read ever observes half a transaction group. In EVERY summary
  snapshot, per currency: `sum(reversals.total_minor)` equals the sum of `amount_minor` over
  `refunded` rows — both halves visible, or neither.
- **Amount immutability.** No served row ever shows an `amount_minor` different from the
  vendor-committed amount — v3 never mutates amounts, only status/note/version.
- **Terminal conservation.** At quiescence, per-currency counts and totals — reversals
  included — equal vendor ground truth: fixture plus scripted mutations plus every payment
  your app created.
- **No cross-currency sum.** Not in the API, not in the UI, not in a label.
- **No lost acknowledgement.** Any write your API answered 2xx for (a note, a draft action, a
  send) is present in the final state.
- **Exactly-once effects.** No ledger event applied twice downstream, no doubled vendor
  payment, no doubled notification row.

## Performance budgets

Measured on the machine the build runs on, against the full collection — **N = 12,288**, 96
Berlin days, 192 vendor pages.

- Each service is listening within **10 seconds** of process start.
- First data rows render within **2 seconds** of page load (local data present).
- The 3D field shows its first non-background frame within **3 seconds** of page load at full
  count.
- `GET /api/payments` at `limit=50` answers in under **150 ms** at p95 — including while a
  sync is running with 8 concurrent readers, and while the SSE stream is live.
- `GET /api/buckets` answers in under **200 ms** at p95; `GET /api/summary` under **150 ms**
  at p95; `GET /api/viz/records` under **400 ms** at p95.
- `POST /api/sync` completes the full 192-page walk within **120 seconds**, documented waits
  included.
- A streamed batch is applied — store, digest, pixels — within **250 ms** of receipt.
- A camera change is visible on the canvas within **250 ms** of the input that caused it, and
  during a scripted drag the scene draws at least **0.8 frames per pointer move** delivered.
- Draw accounting, always: at most **8** default-framebuffer draws per rendered frame; over a
  graded 40-move drag window at most `8 · (M + 8)` = **384**; at rest **0** default-FBO draws
  in any 500 ms window; a pick refresh costs at most **4** offscreen draws and no default-FBO
  draws.
- A coast settles within `τ·ln(max(v0, 2)/2) + 0.7 s`, capped at **2.5 s**.
- An optimistic note edit paints within **100 ms** of confirm — before the network responds.
- The webhook endpoint answers within **3 seconds**. The notifications feed polls at most
  every **5 seconds** and recovers from degraded within **5 seconds** of heal.
- `index.html` + `styles.css` + `app.js` + `viz.js` total at most **150 KB** uncompressed.

## Rules

- Amounts are integers in minor units everywhere, end to end. Never floats. Rendering
  respects each currency's minor-unit exponent (EUR 2, USD 2, JPY 0, KWD 3).
- Never sum amounts across currencies — not in the API, not in the UI, not in a label.
- Sorting, comparing and bucketing times happens on the INSTANT, not on the string. Buckets
  and 3D day positions use the Europe/Berlin calendar day of the instant; the frontend
  consumes the server's `day` field and never recomputes it.
- Every vendor write carries `If-Match`. A 412 means someone got there first: re-fetch,
  re-apply, retry once. Never blind-write. Never retry a create — a draft send — with a
  fresh idempotency key.
- Every vendor request has a timeout of at most **10 seconds**; a stalled connection is a
  documented vendor behaviour, and a timed-out request is retried per the docs.
- Webhook deliveries are untrusted input until the signature verifies against the RAW request
  body. Duplicates and stale events are normal traffic, silently ignored, and counted.
  Transaction groups apply atomically or not at all.
- The event log is append-only, contiguous from seq 1. Outbox rows commit in the same
  transaction as the state they announce. The relay is at-least-once; the notifier's durable
  processed set makes it exactly-once. A user write never waits on the notifier.
- The API answers reads while a sync is in flight — a user browsing the table must never wait
  for the vendor.
- The services run repeatedly against the same `--db-dir`; every restart resumes cleanly, and
  a re-sync is cheap (conditional requests, generation-checked) and duplicates nothing.
- The frontend ships zero external code and works fully offline. The backend is standard
  library only. The 3D field is raw WebGL on the main thread, instanced under its draw budget,
  picked from its own pick buffer, coasted by the printed law.
- Include a README.md with the exact commands to install nothing, run both services (together
  and separately), and sync.
