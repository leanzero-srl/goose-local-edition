# VendorSync Pro

No packages need to be installed. VendorSync Pro uses Python 3 and its standard library only.

```sh
python -m vspro --db ./vspro.sqlite --port 8080
# On systems where Python is named python3:
python3 -m vspro --db ./vspro.sqlite --port 8080
```

Open `http://127.0.0.1:8080/`. The server begins accepting requests before it registers its Meridian webhook, so it remains usable if Meridian is briefly unavailable at startup. Use **Sync now**, or run:

```sh
curl -X POST http://127.0.0.1:8080/api/sync
```

## Meridian connection overrides

The default Meridian test endpoint is `http://127.0.0.1:9008` with API key `sk_test_meridian`. Override either setting with environment variables or explicit command-line flags; flags take precedence over environment variables.

```sh
MERIDIAN_BASE_URL=http://127.0.0.1:9008 MERIDIAN_API_KEY=sk_test_meridian \
  python -m vspro --db ./vspro.sqlite --port 8080
python -m vspro --db ./vspro.sqlite --port 8080 \
  --base-url http://127.0.0.1:9008 --api-key sk_test_meridian
```
