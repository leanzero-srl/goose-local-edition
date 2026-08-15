# Build `vendorsync`

A small operations tool that syncs payments from the Meridian API and shows them to a finance team.

The Meridian API documentation is at `{DOCS_URL}`. Read it before you start. Base URL `{BASE_URL}`,
API key `{API_KEY}`.

Work in the current directory. Python 3, standard library only for the backend — no pip installs.

---

## What to build

### 1. `vendorsync/meridian.py` — the vendor client

```python
class MeridianClient:
    def __init__(self, base_url: str, api_key: str) -> None
    def fetch_all_payments(self) -> list[dict]   # every payment, oldest first by when it occurred
    def total_count(self) -> int                 # how many payments exist in the collection
    def create_payment(self, amount_minor: int, currency: str, idempotency_key: str) -> str
```

`create_payment` returns the payment id and is safe to call more than once with the same key.

### 2. `vendorsync/store.py` — local persistence

```python
class Store:
    def __init__(self, path: str) -> None
    def upsert_many(self, payments: list[dict]) -> int   # returns how many were newly inserted
    def all_payments(self) -> list[dict]                 # oldest first
    def count(self) -> int
    def last_sync(self) -> str | None                    # RFC3339 UTC, or None if never synced
    def set_last_sync(self, when: str) -> None
```

Persist to SQLite at the given path. A payment already present must be updated, not duplicated —
syncing twice must not change the count.

### 3. `vendorsync/api.py` — the HTTP backend

`serve(port: int, store: Store, client: MeridianClient)` starts a JSON API on `127.0.0.1:port`:

| Method | Path | Response |
|---|---|---|
| `GET` | `/api/health` | `{"status": "ok", "payments": <int>, "last_sync": <str or null>}` |
| `GET` | `/api/payments?limit=<int>&offset=<int>` | `{"data": [...], "total": <int>, "limit": <int>, "offset": <int>}` |
| `GET` | `/api/summary` | `{"count": <int>, "total_minor": <int>, "currency": "EUR", "oldest": <str or null>, "newest": <str or null>}` |
| `POST` | `/api/sync` | `{"fetched": <int>, "inserted": <int>, "total": <int>}` |

`limit` defaults to 25 and is capped at 100. `offset` defaults to 0. `data` items carry exactly the
keys `id`, `amount_minor`, `currency`, `created_at`, `status`. `oldest` and `newest` are the
`created_at` of the earliest and latest payments, as RFC3339 **UTC**.

An unknown path returns 404 with `{"error": "not_found"}`. A bad `limit` or `offset` — non-numeric or
negative — returns 400 with `{"error": "bad_request"}`. Every response is JSON.

### 4. `vendorsync/web/index.html` — the frontend

A single page, served by the backend at `GET /`. Plain HTML/CSS/JS, no build step, no CDN — it must
work offline. This page is what the finance team uses every day. Build it as a product, not as a
debug view over the API.

It reads the backend API and shows:

- a summary line with the payment count and the total value, formatted as currency (`€1,234.56`)
- a table of payments with columns Date, Amount, Status
- a **Sync now** button that calls `POST /api/sync` and refreshes the view
- the last-sync time, or `Never synced` when there is none

**Dates.** Every timestamp a user sees is rendered human-readable in the user's locale — e.g.
`1 Mar 2026, 14:00`. A raw ISO-8601 string with an offset (`2026-03-01T14:00:00+00:00`) must never
appear in the rendered page. This covers the Date column and the last-sync time alike.

**Status.** Payment status must be visually distinct at a glance: color-coded badges or an
equivalent device. `settled`, `refunded` and `pending` must differ in computed color, not only in
their text.

**Pagination.** The table is paginated through the documented `limit`/`offset` parameters of
`GET /api/payments`, with visible controls: **Prev** and **Next** buttons and a
`showing X–Y of TOTAL` readout. Never render all rows in one scroll when more than 50 rows exist.

**Filter.** A status filter control lets the user narrow the table to one status. Wire it to
client-side filtering or to the API — either — but it must actually change the rows shown.

**Responsive.** At a viewport 375 px wide the page lays out cleanly with no horizontal scroll.

**States.** The page must handle, visibly and distinctly: **loading**, **empty** (no payments yet), and
**error** (the backend is unreachable or returns an error). Each of those three states must show
text a user can act on — not a blank screen, not a spinner that never resolves.

**Design.** The page has an intentional visual design: a real palette with strong solid accent
colors, a clear typographic hierarchy, and a branded header bar carrying the app name. Never use
faded pastel washes — pick saturated, solid colors over tints. Never decorate cards or rows with a
left accent line or rail. Never use browser-native controls where custom styling is expected — no
default `<select>` for the filter, no `alert()`/`confirm()` dialogs.

### 5. `vendorsync/__main__.py` — the entry point

`python -m vendorsync --db PATH --port N` starts the backend serving both the API and the page.
It must not crash when the database file does not yet exist.

### 6. Performance budgets

Measured against the vendor's 247-row fixture, on the machine the build runs on:

- The page is interactive — first data rows rendered — within **2 seconds** of load.
- `GET /api/payments` at `limit=25` answers in under **150 ms** at p95.
- `POST /api/sync` completes the full 247-row fixture within **60 seconds**.

---

## Rules

- Amounts are integers in minor units everywhere, end to end. Never floats.
- Sorting and comparing times happens on the instant, not on the string.
- The tool is run repeatedly against the same database; a second sync must be cheap and must not
  duplicate rows.
- Include a README.md with the exact commands to install nothing, run the server, and sync.
