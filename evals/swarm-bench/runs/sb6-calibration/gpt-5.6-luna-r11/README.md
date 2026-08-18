# VendorSync Pro

Standard-library-only offline finance sync server. Install nothing.

```sh
python3 -m vspro --db ./vspro.sqlite --port 8080
```

Open `http://127.0.0.1:8080/`. Use **Sync now** to retrieve Meridian payments. The vendor is expected at `http://127.0.0.1:9003` with the test API key configured in `vspro/__main__.py`.
