# Meridian Payments Console

An operations console for a finance team: two cooperating services that sync payments from
the Meridian API v3, keep them consistent through webhooks, crashes and partitions, run a
maker/checker approval workflow, and serve a live console with a raw-WebGL 3D field
rendering every payment.

Install nothing — Python 3 standard library only; the frontend ships zero external code.

## Run both services together

```
python -m app --db-dir ./data --ledger-port 8600 --notifier-port 8601 \
    --vendor http://127.0.0.1:9500 --tokens-file ./tokens.json
```

## Run each service alone

```
python -m app.notifierd --db-dir ./data --port 8601
python -m app.ledgerd --db-dir ./data --port 8600 --notifier http://127.0.0.1:8601 \
    --vendor http://127.0.0.1:9500 --tokens-file ./tokens.json
```

`tokens.json` is written by the operator/harness before boot:
`{"maker": "<32 hex>", "checker": "<32 hex>", "admin": "<32 hex>"}`.

On boot ledgerd binds immediately, registers its webhook, and starts the first sync
unprompted, retrying at least every 5 seconds while the vendor is unreachable.

## Sync

The first sync is self-driven. To run one on demand:

```
curl -X POST http://127.0.0.1:8600/api/sync
```

Open `http://127.0.0.1:8600/` for the console.
