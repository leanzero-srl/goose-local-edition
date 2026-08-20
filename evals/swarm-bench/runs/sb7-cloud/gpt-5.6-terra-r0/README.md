# Meridian Payments Console

Meridian runs two local services backed by SQLite: `ledgerd` serves the console and owns the payment workflow; `notifierd` durably consumes the ledger outbox. The backend uses Python's standard library and the browser UI has no dependencies.

## Required configuration

Create a tokens file containing three distinct, non-empty bearer tokens. Keep this file outside source control and restrict it to the account that runs Meridian.

```json
{
  "maker": "replace-with-maker-token",
  "checker": "replace-with-checker-token",
  "admin": "replace-with-admin-token"
}
```

`--vendor` must point at the Meridian vendor API. The services bind only to `127.0.0.1`; expose them through an authenticated reverse proxy if remote access is required.

## Start and stop

Run both services together for normal operation. The supervisor starts `notifierd`, then `ledgerd`; when `ledgerd` exits it terminates `notifierd`.

```sh
python -m app \
  --db-dir ./data \
  --ledger-port 8080 \
  --notifier-port 8081 \
  --vendor http://127.0.0.1:8890 \
  --tokens-file ./tokens.json
```

Stop the foreground process with `Ctrl-C`. For a managed deployment, use one process supervisor for this command and send it a graceful interrupt or termination signal rather than killing an individual child.

To run the services separately, start the notifier before the ledger and stop the ledger before the notifier:

```sh
python -m app.notifierd --db-dir ./data --port 8081
python -m app.ledgerd \
  --db-dir ./data \
  --port 8080 \
  --notifier http://127.0.0.1:8081 \
  --vendor http://127.0.0.1:8890 \
  --tokens-file ./tokens.json
```

Use a different port pair and a different database directory for every independent Meridian environment.

## Operational checks and commands

The console is available at `http://127.0.0.1:8080/`. These commands assume the default ports:

```sh
# Ledger state, payment count, last completed sync, and webhook counters
curl -sS http://127.0.0.1:8080/api/health

# Notifier state and durable notification count
curl -sS http://127.0.0.1:8081/health

# Force a foreground reconciliation with the vendor
curl -sS -X POST http://127.0.0.1:8080/api/sync

# Check durable delivery from the ledger outbox to notifierd
curl -sS http://127.0.0.1:8080/api/outbox/status

# Inspect the local payment projection and aggregate reconciliation view
curl -sS 'http://127.0.0.1:8080/api/payments?limit=50&offset=0'
curl -sS http://127.0.0.1:8080/api/summary

# Inspect notifications through ledgerd's notifier proxy
curl -sS 'http://127.0.0.1:8080/api/notifications?limit=50&offset=0'
```

`POST /api/sync` returns `409` while another sync is active and `502` when the vendor cannot be reconciled. A successful response reports `fetched`, `inserted`, `updated`, and the resulting local `total`. Ledgerd also attempts sync at startup and every 10 seconds. It registers the vendor webhook in the background and retries registration every 2 seconds until it succeeds.

The draft and event APIs require bearer tokens. Use the appropriate role token; a maker creates and submits, and a different checker approves or rejects. Administrators can read drafts and events.

```sh
curl -sS -H "Authorization: Bearer $ADMIN_TOKEN" \
  'http://127.0.0.1:8080/api/events?after=0&limit=50'
curl -sS -H "Authorization: Bearer $MAKER_TOKEN" \
  http://127.0.0.1:8080/api/drafts
```

## Recovery runbook

**Vendor unavailable or a failed sync.** Keep ledgerd running. It retains the last committed projection and sync cursor, retries on its 10-second loop, and applies each fetched page transactionally. After the vendor is healthy, force a sync with `POST /api/sync`, then verify `/api/health` has a current `last_sync` and compare `/api/summary` with the expected vendor view. Do not delete the database to recover a vendor outage.

**Notifier unavailable.** Keep ledgerd running and restore notifierd with the same `--db-dir`. Undelivered outbox rows remain in `ledger.db`; ledgerd attempts relay every second and backs failed deliveries off up to 60 seconds. Verify recovery with `/api/outbox/status`: `notifier` should be `up` and `pending` should drain. Notifier processing is idempotent, so replay after restart is safe.

**Ledger restart or host restart.** Start the normal combined command, or start notifierd before ledgerd. The ledger resumes with its stored sync cursor, reuses cached validator pages when appropriate, re-registers its webhook only if no secret is stored, and resumes approved draft delivery using its persisted idempotency key. Validate `/api/health`, force one sync if the vendor is available, and check `/api/outbox/status` before declaring recovery complete.

**A sync that returns `409`.** Another sync is already running. Wait for it to finish and check `/api/health`; do not start another ledger process to work around the lock.

**Database corruption, accidental deletion, or storage loss.** Stop both services first. Restore a matched backup of the complete database directory, including SQLite WAL sidecar files if they were present when the backup was made, then start notifierd followed by ledgerd and run the restart checks above. If no usable backup exists, preserve the failed directory for investigation, move it aside, start with a new empty directory, and reconcile from the vendor; locally created draft approval history and notifier history cannot be reconstructed from the vendor.

## Database ownership, backups, and safety

`--db-dir` is a shared deployment directory, not a per-process temporary directory. Ledgerd exclusively owns `ledger.db`, which contains the payment projection, reversals, events, draft workflow, vendor cursor/cache, webhook secret, and notifier outbox. Notifierd exclusively owns `notifier.db`, which contains consumed event sequence numbers and notifications. Both files use SQLite WAL mode.

Run exactly one ledgerd and one notifierd against a database directory. Do not run two copies of either daemon, point a second environment at the same directory, manually edit either SQLite file, or copy only `*.db` while services are running. SQLite locking does not make two application instances a supported deployment topology.

For a filesystem-level backup, stop both services and copy the entire directory, not just the two main database files:

```sh
# Services stopped
backup="./meridian-backup-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$backup"
cp -a ./data/. "$backup"/
```

Keep the tokens file separately and protect it as a credential; it is not stored in the databases. Test restores against a separate database directory and port pair. Before any destructive recovery, retain an immutable copy of the affected directory.
