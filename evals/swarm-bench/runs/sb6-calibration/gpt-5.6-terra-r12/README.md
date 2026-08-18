# VendorSync Pro — golden reference implementation (sb-6)

The hand-written correct app that the sb-6 freeze gate scores (red-team F7 / gate **G6**): every
non-calibration-owned check must pass against this implementation before any threshold is
trusted. It follows `spec-build-v3` as amended by the SB6-PACKAGE contradiction ledger and the
binding red-team amendments (notably F1 `#status-filter[data-value]`, F4(ii) rAF rendering while
a pointer drag is captured, F5 nothing-but-bars, F6 wheel never scrolls the page, F8 threaded
server, F11 yaw kept unbounded/unnormalized, F13 challenge increments no counter, F14 client
request timeout, F19 main-thread context on `#viz3d`, F24 exponent-true money).

## Run (nothing to install — Python 3.9+ standard library only)

```bash
python -m vspro --db vspro.db --port 8790
# vendor location comes from the environment (or --base-url/--api-key):
#   MERIDIAN_BASE_URL=http://127.0.0.1:8787  MERIDIAN_API_KEY=sk_test_meridian
```

Then open `http://127.0.0.1:8790/` and press **Sync now**, or:

```bash
curl -X POST http://127.0.0.1:8790/api/sync
```

## Layout

```
vspro/
  meridian.py   vendor client: cursor pagination, per-page ETag/If-None-Match, Retry-After
                (seconds + HTTP-date), 410 restart, If-Match + one refetch/retry on 412,
                idempotent creates (409 duplicate = success), independent batch items,
                idempotent-by-URL webhook registration, 10 s request timeout
  store.py      SQLite (WAL, connection-per-call): upsert without duplication or version
                regression, idempotent+ordered webhook apply_event, Europe/Berlin
                day-bucketing of the INSTANT (2026-03-29 DST correct)
  api.py        ThreadingHTTPServer JSON API + static frontend; one error envelope; webhook
                receiver verifies HMAC against the RAW body; process-lifetime health counters
  __main__.py   binds first, then registers the webhook in the background
  web/          index.html + styles.css + app.js + viz.js (~48 KB of the 150 KB budget)
```

`viz.js` implements the frozen 3D contract with raw WebGL: bar grid `x_i=(i−(D−1)/2)·1.5`,
`z_j=(j−1.5)·1.5`, `h=count·0.25`; orbit camera (35/27/30, target (0,3,0), fovY 50, near 0.1,
far 200) whose MVP is built from the spec's own basis/NDC formulas; flat colors (tops exact,
sides ×0.62 rounded, clear `#0F172A`, dithering off); picking through an offscreen color-id
framebuffer (depth-tested, occlusion-correct) with an analytic ray-AABB fallback; and the full
`window.vsdbg` v3 API.

## Verification that has actually run

- `python3 smoke_test.py` — boots `stub_vendor.py` (a minimal Meridian **v2** stub that doubles
  as executable documentation of the endpoint shapes the client speaks — `/v2/*`, aligned with
  the real `bench/vendor_service_v2.py` mock) plus the app as a subprocess, and passes 35/35
  checks: trap-chain sync, idempotent+conditional second sync,
  Berlin/DST buckets vs an independent zoneinfo computation, per-currency summary, envelope
  validation, the 412 dance (recovered and 409-surfaced), batch partial failure, the scripted
  webhook counter quad (challenge uncounted, forged 401, stale ignored, no version regression),
  reads answering during a stalled sync, and kill+reboot persistence.
- Browser (Playwright, 1280×800 and 375×700): the probe's analytic pipeline re-implemented
  in-page scored tops 28/28, above 10/10, sides 28/28, sky 4/4, corners 4/4, `vsdbg.project`
  max error 0.000 px, picks 7/7; drag −0.35°/px exact (yaw 35→−7), wheel `·exp(0.0012·ΔY)`
  exact, dblclick reset exact; tooltip `<count> <status> · <day>` shows/hides; bar click sets
  `#status-filter[data-value]` and the readout equals the per-status total; 2D toggle cells
  28/28; glKill fallback (no page errors, notice + table, main table alive); error state with
  `/api/*` blocked; optimistic note `saving→saved` with revert on failure; JPY 0 / KWD 3
  decimals rendered; no ISO timestamp leaks; no horizontal scroll at 375 px; zero console
  errors nominal.

The vendor v2 mock is `bench/vendor_service_v2.py`; `vspro/meridian.py`'s `ENDPOINTS` block and
`stub_vendor.py` are aligned with it (`/v2/payments`, `/v2/payments/batch`, `/v2/webhooks`,
If-Match quoted-or-bare). The full G6 freeze gate runs against this tree with
`python3 score_sb6.py --tree golden-vspro --port 8899 --reference` from `bench/`.
