# Meridian API v3 — Integration Guide

This document is the complete contract for the Meridian API v3 sandbox. Every behaviour your
client must handle is described here; nothing else is documented anywhere. Read it in full
before writing a line of sync code — several behaviours below (fixed-size pages, `Retry-After`,
expired cursors, the collection-generation rule, webhook signatures, transaction groups,
idempotency keys) will defeat a client that did not.

## 1. Basics

- Base URL: passed to your process (`--vendor`); all paths below are relative to it.
- Authenticate every request with `Authorization: Bearer <api key>` (the key you were given).
- All request and response bodies are JSON (`Content-Type: application/json`) unless noted.
- All timestamps are RFC3339. `created_at` values may carry any UTC offset — compare
  INSTANTS, never strings.
- Amounts are integers in minor units. `EUR` and `USD` have exponent 2, `JPY` has 0, `KWD`
  has 3. Amounts NEVER change after creation — only `status`, `note` and `version` do.
- Give every request a timeout (10 seconds is right). A stalled connection is a normal
  sandbox behaviour; retry a timed-out idempotent request.

## 2. The payment resource

```json
{"id": "pay_00042", "amount_minor": 129900, "currency": "EUR",
 "created_at": "2026-04-01T12:34:56+02:00", "settled_at": "2026-04-01T15:02:11Z",
 "status": "settled", "version": 3, "note": "",
 "counterparty": {"name": "Aurora Freight", "country": "DE"}}
```

- `status` ∈ `settled | pending | refunded | failed`.
- `settled_at` is the settlement instant for payments that were settled at collection load;
  it is `null` otherwise (a later status flip does not backdate one).
- `version` starts at 1 and increases by exactly 1 per committed change to the payment.
  Versions are your ordering truth: apply changes in version order, ignore anything at or
  below the version you already applied.

## 3. Listing payments — `GET /v3/payments`

Cursor pagination with a **server-fixed page size of 64**. There is NO `limit` parameter; one
passed is ignored (and noted in our logs).

```
GET /v3/payments                → first page
GET /v3/payments?cursor=<next>  → subsequent pages
```

Response:

```json
{"data": [<payment>, ...], "next_cursor": "<opaque>" | null, "total": <int>}
```

- Treat cursors as opaque. `next_cursor: null` means the walk is complete.
- `total` is the walk bound captured when your walk started. Payments created mid-walk may
  or may not appear on a later page — the walk is NOT snapshot-isolated; the webhook is
  authoritative for anything created or changed while you walk. Mutations committed to rows
  you have or have not yet fetched ARE served in their current state.
- Every 200 list response carries `ETag` and `X-Collection-Generation` headers.

### Conditional requests and the generation rule

Store the `ETag` per page and the latest `X-Collection-Generation`. On a later sync, send
`If-None-Match`. Two outcomes:

- `304 Not Modified` with `X-Collection-Generation` EQUAL to your stored generation: your
  copy is current; proceed.
- `304 Not Modified` with `X-Collection-Generation` DIFFERENT from your stored generation:
  **this 304 is a cache miss.** Drop the stored validator and refetch that page
  unconditionally, exactly once, then continue with the fresh validator. Re-sending the same
  conditional request keeps producing the same 304 — more than 3 identical conditional
  requests in a row is the infinite-loop bug. Serving your stale copy as fresh after a
  generation-mismatched 304 is worse.

### Faults you must survive

- **Dropped connection**: the connection may be severed before any bytes arrive. Retry the
  SAME cursor; the resume costs exactly one extra request. Never restart a walk you have
  already partially committed.
- **`500 internal_error` + `Retry-After`**: wait the advertised delta-seconds, retry ONCE,
  then continue the walk. A fresh unconditional restart of committed work is wrong.
- **`410 cursor_expired`**: the cursor predates a collection rebuild. Restart the walk from
  the first page (no cursor). Cursors held across long gaps can expire; treat 410 as routine.
- **`400 bad_cursor`**: the cursor is malformed — a client bug, not a retry case.

## 4. Reading one payment — `GET /v3/payments/<id>`

Returns the payment, or `404 {"error": "not_found"}`.

## 5. Updating the note — `PATCH /v3/payments/<id>`

The note is the only client-writable field. Optimistic concurrency via `If-Match`:

```
PATCH /v3/payments/pay_00042
If-Match: "3"
{"note": "call the counterparty"}
```

- `If-Match` carries the version you last read, quoted or bare.
- No `If-Match` at all → `428 {"error": "precondition_required"}`. That response is a bug in
  YOUR client: every write carries `If-Match`.
- Version mismatch → `412 {"error": "version_conflict"}`. Someone got there first: re-fetch
  the resource, re-apply your note on the fresh version, retry ONCE with the new `If-Match`.
- Success → `200` with the updated resource (`version` bumped by 1). Persist what you get
  back.
- `note` must be a string of 1–280 characters (`400 invalid_note` otherwise).

## 6. Creating payments — `POST /v3/payments`

```
POST /v3/payments
Idempotency-Key: <your key, required>
{"amount_minor": 4500, "currency": "EUR", "note": "...",
 "counterparty": {"name": "Smoke Co", "country": "DE"}}
```

- `Idempotency-Key` is REQUIRED (`400 idempotency_key_required` without it).
- First use of a key → `201` with the created payment. **Any later request with the same key
  returns `200` with the SAME payment**, regardless of body. A retry after a crash or
  timeout MUST reuse the stored key — a fresh key creates a duplicate payment, and that
  duplicate is real money.
- The response may be held open for several seconds while the payment is already committed;
  a timed-out create is exactly the case the key replay exists for.
- Meridian **value-dates** created payments: `created_at` is assigned by the vendor inside
  the collection's existing day span, in a day with headroom. Do not expect "now".
- Validation errors (`400`): `invalid_amount` (positive integer required),
  `unsupported_currency` (EUR/USD/JPY/KWD only), `invalid_counterparty` (`name` required,
  `country` exactly two uppercase letters), `invalid_note`.
- A successful create is also announced to your webhook as `payment.created`.

## 7. Reversals — `GET /v3/reversals`

```json
{"data": [{"id": "rev_00003", "payment_id": "pay_00042", "amount_minor": 129900,
           "currency": "EUR", "created_at": "..."}, ...], "total": <int>}
```

Every refunded payment has exactly one reversal for its full amount in its own currency.
Refunds commit atomically: the payment's flip to `refunded` and its reversal appear together
(see transaction groups below).

## 8. Webhooks

### Registration — `POST /v3/webhooks`

```
{"url": "http://127.0.0.1:<port>/your/endpoint"}
```

Before accepting, Meridian POSTs an UNSIGNED challenge to your URL:

```json
{"type": "webhook.verify", "challenge": "<hex>"}
```

Answer `200 {"challenge": "<the same hex>"}` within 10 seconds — your endpoint must already
be listening when you register. On success you receive `{"id": "wh_…", "secret": "whsec_…"}`.
Registration is idempotent by URL: re-registering (e.g. after a restart) returns the same id
and secret.

### Deliveries

Each event is POSTed to your URL with header `Meridian-Signature: t=<unix>,v1=<hex>` where

```
v1 = HMAC_SHA256(secret, "<t>." + <raw request body bytes>)
```

Verify against the RAW bytes before parsing. A delivery that fails verification is forged
traffic: reject it (401), count it, change nothing. Event shape:

```json
{"id": "evt_0001", "type": "payment.created" | "payment.updated" | "reversal.created",
 "created_at": "...", "txn": null | {"id": "txn_9", "part": 1, "of": 2},
 "data": <payment or reversal resource>}
```

Rules your consumer must hold:

- **At-least-once**: the same event id may be delivered more than once, byte-identical.
  Deduplicate on the event id; a duplicate changes nothing and is counted, not applied.
- **Out of order**: versions may arrive out of order (v+2 before v+1). Apply by version:
  anything at or below your applied version is stale — ignore it, count it.
- **Transaction groups**: events carrying `txn` with matching `id` form one atomic group of
  `of` parts (the refund pair: `payment.updated` to `refunded` + `reversal.created`). Stage
  parts until the group is complete, then apply them in ONE local transaction. No read of
  your store may ever observe half a group.
- Answer deliveries quickly (within 3 seconds) with a 2xx once durably accepted. Deliveries
  time out at 10 seconds.

## 9. Operational notes

- Meridian may briefly refuse connections (maintenance windows, restarts). Keep serving your
  local data, retry on your own schedule, and recover without operator action.
- Do not hardcode collection facts (window dates, counts, tokens): every value is
  environment-specific. The page size (64) and the full-collection page count at load
  (12,288 / 64 = 192) are fixed.
