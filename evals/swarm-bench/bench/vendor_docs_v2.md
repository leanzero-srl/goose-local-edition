# Meridian Payments API — v2

The Meridian API is a JSON/HTTPS service for reading, creating and updating payments, and for
pushing payment events to you over webhooks. Every request must carry
`Authorization: Bearer <api_key>`. All responses are `application/json` unless stated otherwise.
The base URL is given to you at integration time; do not hard-code it.

## Endpoints

| Method | Path | Purpose | Notes |
|---|---|---|---|
| `GET` | `/v2/docs` | this document | text/markdown |
| `GET` | `/v2/payments` | list payments, **cursor-paginated** | query: `cursor` (opaque string, omit for the first page), `limit` (max 100, default 25) |
| `GET` | `/v2/payments/<id>` | one payment, including its current `version` | |
| `PATCH` | `/v2/payments/<id>` | update mutable fields (`note`, `status`) | requires an `If-Match` request header — see *Optimistic concurrency* |
| `POST` | `/v2/payments` | create a payment | requires an `Idempotency-Key` request header |
| `POST` | `/v2/payments/batch` | create up to 20 payments in one call | per-item `idempotency_key` in the body; items are applied independently |
| `POST` | `/v2/webhooks` | register a webhook endpoint | `{"url": "..."}` → `{"id": "...", "secret": "..."}`; idempotent by URL |

### The payment object

```json
{
  "id": "pay_0042",
  "amount_minor": 129900,
  "currency": "EUR",
  "created_at": "2026-03-24T09:15:00+02:00",
  "settled_at": "2026-03-24T10:03:00Z",
  "status": "settled",
  "version": 1,
  "note": "",
  "counterparty": {"name": "Baltic Traders", "country": "US"}
}
```

`amount_minor` is an **integer in the currency's minor unit** — `129900` is €1,299.00 in EUR
(2 decimals), ¥129,900 in JPY (0 decimals), and KWD 129.900 in KWD (3 decimals). Meridian never
sends fractional amounts and never sends amounts as strings or floats. Supported currencies and
their minor-unit exponents: `EUR` 2, `USD` 2, `JPY` 0, `KWD` 3.

`status` is one of `settled`, `pending`, `refunded`, `failed`. `settled_at` is `null` unless the
payment settled or was refunded.

`created_at` is RFC 3339. **Meridian reports each payment in the local offset of the originating
region**, so a single page can legitimately contain `+02:00`, `-05:00` and `Z` values.

`version` is a positive integer that increases by exactly one on every change to the payment,
whoever makes it. It is the concurrency token for `PATCH` and the ordering token for webhook
events — see below.

### `GET /v2/payments` — example exchange

```http
GET /v2/payments?limit=2 HTTP/1.1
Authorization: Bearer sk_test_...
```

```http
HTTP/1.1 200 OK
Content-Type: application/json
ETag: "d41d8cd98f00b204"

{
  "data": [ { "id": "pay_0000", "...": "..." }, { "id": "pay_0001", "...": "..." } ],
  "next_cursor": "eyJvIjoyfQ",
  "total": 1553
}
```

Pass `next_cursor` back as the `cursor` query parameter to retrieve the following page.

### `POST /v2/payments` — example exchange

```http
POST /v2/payments HTTP/1.1
Authorization: Bearer sk_test_...
Idempotency-Key: 9f2c1b7e-4d3a-4c6e-9a1f-2b8d5e0c7a41
Content-Type: application/json

{ "amount": {"value_minor": 4500, "currency": "EUR"},
  "counterparty": {"name": "Smoke Co", "country": "DE"},
  "occurred_at": "2026-04-06T10:00:00Z" }
```

```http
HTTP/1.1 201 Created

{ "id": "pay_new_0000", "amount_minor": 4500, "currency": "EUR",
  "created_at": "2026-04-06T10:00:00Z", "settled_at": null, "status": "pending",
  "version": 1, "note": "", "counterparty": {"name": "Smoke Co", "country": "DE"} }
```

Note the asymmetry, which is deliberate: **creates take a nested `amount` object**
(`{"value_minor": <int>, "currency": "<code>"}`), while payment objects you read carry the
flattened `amount_minor` + `currency` pair.

### `POST /v2/payments/batch`

Body: `{"items": [ <create item>, ... ]}` with **1 to 20 items**. Each item has the same shape as
a single create, plus its own `"idempotency_key"` **inside the item** (there is no batch-level
idempotency header):

```json
{"items": [
  {"amount": {"value_minor": 1000, "currency": "USD"},
   "counterparty": {"name": "Alpha GmbH", "country": "DE"},
   "occurred_at": "2026-04-06T11:00:00Z", "idempotency_key": "batch-a-1"},
  {"amount": {"value_minor": 9000000, "currency": "USD"},
   "counterparty": {"name": "Over Limit LLC", "country": "US"},
   "occurred_at": "2026-04-06T11:05:00Z", "idempotency_key": "batch-a-2"}
]}
```

The response is `200` with per-item results **in input order**, whatever happened to the
individual items:

```json
{"results": [
  {"index": 0, "status": "created", "id": "pay_new_0001"},
  {"index": 1, "status": "error",
   "error": {"code": "amount_over_limit", "message": "per-payment limit is 5000000 minor units"}}
], "succeeded": 1, "failed": 1}
```

Items are applied **independently**. One failed item does not roll back, discard, or retry its
neighbours — the succeeded items exist, the failed items report their own error, and the correct
client behaviour is to surface each item's outcome as-is. Never resubmit a failed item under a
fresh idempotency key.

### `PATCH /v2/payments/<id>`

Updates the mutable fields of a payment — `note` (a string) and `status`. The request **must**
carry an `If-Match` header naming the version you are updating from; see *Optimistic
concurrency* below. On success the response is the full updated payment object, with `version`
incremented by one.

### `POST /v2/webhooks`

Registers an HTTPS/HTTP endpoint to receive payment events. Body: `{"url": "<your endpoint>"}`.
Response: `{"id": "wh_...", "secret": "whsec_..."}`.

Registration is **idempotent by URL**: registering a URL Meridian already knows returns the SAME
id and the SAME secret, every time. During every registration call Meridian verifies the URL with
a challenge handshake — see *Webhooks* below — so **your server must already be listening when
you register**.

---

## Notes & gotchas

These are the things integrators most often get wrong. Read them before you write your client.

**`total` counts the whole collection, not the page.** It is the number of payments matching the
query across *every* page, and it does not change as you paginate. It is not the length of `data`,
and it is not a page count. Use it for progress reporting only — never to decide when to stop
paginating.

**Pagination ends when `next_cursor` is `null`.** The key is always present on a successful list
response. On the final page its value is `null`. Stop when you see `null`; do not stop merely
because `data` came back shorter than `limit`, because Meridian may return a short page at any
position.

**Rate limiting returns `429` with a `Retry-After` header.** Per RFC 7231 that header comes in
**either** of two forms, and Meridian sends both depending on which limiter tripped:

- a number of **seconds** to wait — `Retry-After: 2`
- an **HTTP-date** after which you may retry — `Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`

Handle both. Wait until the stated moment, then retry the same request. A `429` is never a
permanent failure and must not be surfaced to the caller as an error.

**Cursors expire, and an expired cursor returns `410 Gone`.** A cursor is valid for a short
window and is invalidated if the collection changes while you are paging. On `410` the response
body is `{"error": "cursor_expired"}`; the correct recovery is to **restart pagination from the
first page** (no cursor) and rebuild the collection. Do not treat it as the end of the data, and
do not retry the same expired cursor — it will never succeed.

**Conditional requests are supported and expected.** List responses carry an `ETag`. Send it back
as `If-None-Match` on a subsequent identical request; if nothing has changed Meridian replies
`304 Not Modified` **with no body**, which costs you no quota. Clients that re-download an
unchanged collection on every sync are the single most common cause of rate-limit exhaustion on
this API. An ETag stops matching whenever the underlying data has changed — including changes
made by webhook-visible events — so a `304` is always safe to trust.

**Meridian may occasionally hold a connection open without answering.** This is a documented
behaviour of the service under internal contention, not an outage: the connection is eventually
released, but a client that waits for it blows every latency budget downstream. **Apply a
request timeout of at most 10 seconds to every call and retry a timed-out request once** — the
retry will answer promptly. A client with no request timeout will hang for the full hold and has
not implemented this API correctly.

**A `409` on `POST /v2/payments` means the payment already exists.** When you retry a create with
an `Idempotency-Key` Meridian has already seen, it replies `409 Conflict` with the body
`{"error": "duplicate", "payment_id": "pay_..."}`. This is the **success** path for a retry — the
payment was applied exactly once, and `payment_id` identifies it. Treat it as success and use
that id. Do not resubmit with a fresh key: that is how integrators create double charges. The
same rule applies per item inside a batch: a replayed item comes back `"status": "created"` with
`"duplicate": true` and the original id.

**The per-payment amount limit is 5,000,000 minor units.** A create whose `value_minor` exceeds
it fails with the error code `amount_over_limit` — as a `400` on a single create, and as that
item's per-item error inside a batch. This is a business rule, not a validation quirk: the rest
of the batch still applies.

**Sort by instant, not by string.** Because offsets vary per region, lexicographic ordering of
`created_at` does not produce chronological order. Convert to a single instant (UTC) before
sorting or comparing.

## Optimistic concurrency

Payments are edited by many parties — other API clients, Meridian's own settlement engine — so
every `PATCH` is guarded by a version check:

- Send `If-Match: "<version>"` with the version of the payment you last read. Quoted
  (`If-Match: "3"`) and bare (`If-Match: 3`) forms are both accepted.
- If the version matches, the update applies and the response carries `version + 1`.
- If someone else changed the payment first, Meridian answers **`412 Precondition Failed`** and
  changes nothing. The correct recovery: **re-fetch the payment (`GET /v2/payments/<id>`),
  re-apply your change on the fresh object, and retry ONCE with the new version.** If that retry
  also comes back `412`, the resource is genuinely contended — surface a conflict to your caller
  rather than fighting for it in a loop.
- A `PATCH` without `If-Match` answers **`428 Precondition Required`**. That is a bug in the
  calling client, not a retry case: never write blind.

## Webhooks

After registration, Meridian POSTs signed events to your endpoint.

**The verification challenge.** During every `POST /v2/webhooks` call, Meridian POSTs
`{"type": "webhook.verify", "challenge": "<hex>"}` to the URL being registered. Your endpoint
must answer `200` with `{"challenge": "<the same hex>"}`. This request is **unsigned**, because
the secret does not exist for you until registration returns. The challenge is part of
registration, not an event delivery.

**Event deliveries.** Every event is a POST with body

```json
{"id": "evt_0001", "type": "payment.updated", "created_at": "2026-04-06T12:00:00Z",
 "data": { <the full payment object, including "version"> }}
```

and header `Meridian-Signature: t=<unix seconds>,v1=<hex>` where

```
v1 = HMAC_SHA256(secret, "<t>" + "." + <raw request body bytes>)
```

Verify against the **raw request body bytes** — not a re-serialization of the parsed JSON;
key order and whitespace matter to the MAC. A missing or wrong signature means the request is
not from Meridian.

**Delivery semantics — plan for all three:**

- **Duplicates.** Meridian delivers at-least-once. The same event (same `id`, byte-identical
  body and signature) may arrive again. Process every event id at most once.
- **Out-of-order.** Delivery order is NOT guaranteed. An event carrying an older `version` of a
  payment may arrive after you already hold a newer one. Use `data.version` to decide: an event
  whose version is not greater than what you have is stale, and must never overwrite newer
  state.
- **Forgeries.** Anyone who finds your endpoint URL can POST to it. Only the signature makes a
  delivery trustworthy.

Answer deliveries within **3 seconds**. Do your own bookkeeping first and return `200`; never
call back into the Meridian API from inside the webhook handler — that is how integrators
deadlock their own sync.

## Errors

Vendor-side errors are flat JSON: `{"error": "<code>"}` with the HTTP status carrying the
semantics — e.g. `{"error": "rate_limited"}` (429), `{"error": "cursor_expired"}` (410),
`{"error": "version_conflict"}` (412), `{"error": "precondition_required"}` (428),
`{"error": "duplicate", "payment_id": "..."}` (409), `{"error": "amount_over_limit"}` (400),
`{"error": "not_found"}` (404). The structured-envelope style some APIs use is not Meridian's;
what YOUR service returns to YOUR callers is your own contract.
