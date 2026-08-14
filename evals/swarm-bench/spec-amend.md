# Task: add the STATUS SUMMARY feature to the EXISTING vendorsync app

The working directory already contains a complete, working application called `vendorsync` —
a Python tool that syncs payments from the Meridian vendor API into a local SQLite store and
serves them over its own HTTP API with an offline HTML page. **Do not rebuild it. Do not rewrite
modules that work. Read the existing code first and extend it.**

Your job is ONE new feature, three deliverables:

1. `GET /api/summary/by-status` — a new endpoint on the existing API returning JSON of the form
   `{"by_status": {"<status>": {"count": <int>, "total_minor": <int>}, ...}}`, computed from the
   local store (never by calling the vendor). Every payment in the store must be counted in
   exactly one bucket; the counts must sum to the collection total the API already reports.
2. `GET /api/export.csv` — a CSV export of ALL payments in the local store. First line is a
   header including at least `id`, `amount_minor`, `currency`, `created_at`, `status`; then one
   row per payment (all of them, not one page).
3. The served HTML page gains a visible "By status" section rendering the by-status summary
   (server-rendered or fetched from the new endpoint — either is fine, but the section must be
   present in the page and identifiable, e.g. an element with id `by-status`).

Hard constraint: **every existing behavior must keep working exactly as before** — all current
endpoints, the sync flow, pagination, the summary endpoint, persistence across restarts, and the
existing page. A regression in the existing app makes the work unacceptable regardless of the
new feature. Add tests for the new endpoints beside the existing tests.
