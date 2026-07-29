# Meridian Payments API — v1

The Meridian API is a JSON/HTTPS service for reading and creating payments. Every request must carry
`Authorization: Bearer <api_key>`. All responses are `application/json` unless stated otherwise. The
base URL is given to you at integration time; do not hard-code it.

## Endpoints

| Method | Path | Purpose | Notes |
|---|---|---|---|
| `GET` | `/v1/docs` | this document | text/markdown |
| `GET` | `/v1/payments` | list payments, **cursor-paginated** | query: `cursor` (opaque string, omit for the first page), `limit` (max 100, default 25) |
| `POST` | `/v1/payments` | create a payment | requires an `Idempotency-Key` request header |

### The payment object

```json
{
  "id": "pay_7Qk2",
  "amount_minor": 129900,
  "currency": "EUR",
  "created_at": "2026-03-01T09:15:00+02:00",
  "status": "settled"
}
```

`amount_minor` is an **integer in the currency's minor unit** — `129900` is €1,299.00. Meridian never
sends fractional amounts and never sends amounts as strings or floats.

`created_at` is RFC 3339. **Meridian reports each payment in the local offset of the originating
region**, so a single page can legitimately contain `+02:00`, `-05:00` and `Z` values.

### `GET /v1/payments` — example exchange

```http
GET /v1/payments?limit=2 HTTP/1.1
Authorization: Bearer sk_live_...
```

```http
HTTP/1.1 200 OK
Content-Type: application/json
ETag: "d41d8cd98f00b204"

{
  "data": [ { "id": "pay_7Qk2", "...": "..." }, { "id": "pay_8Rm5", "...": "..." } ],
  "next_cursor": "eyJvIjoyfQ",
  "total": 47
}
```

Pass `next_cursor` back as the `cursor` query parameter to retrieve the following page.

### `POST /v1/payments` — example exchange

```http
POST /v1/payments HTTP/1.1
Authorization: Bearer sk_live_...
Idempotency-Key: 9f2c1b7e-4d3a-4c6e-9a1f-2b8d5e0c7a41
Content-Type: application/json

{ "amount_minor": 4500, "currency": "EUR" }
```

```http
HTTP/1.1 201 Created

{ "id": "pay_9Tn1", "amount_minor": 4500, "currency": "EUR",
  "created_at": "2026-03-04T11:02:00Z", "status": "pending" }
```

---

## Notes & gotchas

These are the things integrators most often get wrong. Read them before you write your client.

**`total` counts the whole collection, not the page.** It is the number of payments matching the
query across *every* page, and it does not change as you paginate. It is not the length of `data`,
and it is not a page count. Use it for progress reporting only — never to decide when to stop
paginating.

**Pagination ends when `next_cursor` is `null`.** The key is always present on a successful list
response. On the final page its value is `null`. Stop when you see `null`; do not stop merely because
`data` came back shorter than `limit`, because Meridian may return a short page at any position.

**Rate limiting returns `429` with a `Retry-After` header.** Per RFC 7231 that header comes in
**either** of two forms, and Meridian sends both depending on which limiter tripped:

- a number of **seconds** to wait — `Retry-After: 2`
- an **HTTP-date** after which you may retry — `Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`

Handle both. Wait until the stated moment, then retry the same request. A `429` is never a permanent
failure and must not be surfaced to the caller as an error.

**Cursors expire, and an expired cursor returns `410 Gone`.** A cursor is valid for a short window and
is invalidated if the collection changes while you are paging. On `410` the response body is
`{"error": "cursor_expired"}`; the correct recovery is to **restart pagination from the first page**
(no cursor) and rebuild the collection. Do not treat it as the end of the data, and do not retry the
same expired cursor — it will never succeed.

**Conditional requests are supported and expected.** List responses carry an `ETag`. Send it back as
`If-None-Match` on a subsequent identical request; if nothing has changed Meridian replies
`304 Not Modified` **with no body**, which costs you no quota. Clients that re-download an unchanged
collection on every sync are the single most common cause of rate-limit exhaustion on this API.

**A `409` on `POST /v1/payments` means the payment already exists.** When you retry a create with an
`Idempotency-Key` Meridian has already seen, it replies `409 Conflict` with the body
`{"error": "duplicate", "payment_id": "pay_..."}`. This is the **success** path for a retry — the
payment was applied exactly once, and `payment_id` identifies it. Treat it as success and use that
id. Do not resubmit with a fresh key: that is how integrators create double charges.

**Sort by instant, not by string.** Because offsets vary per region, lexicographic ordering of
`created_at` does not produce chronological order. Convert to a single instant (UTC) before sorting or
comparing.
