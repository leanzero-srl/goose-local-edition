# Native saved-node transport qualification

Measured 2026-10-26. This is isolated simulator qualification, not production Link deployment or physical-device qualification.

## Reproduction and diagnosis

The testcontrol fixture requires its fixture enrollment key on every registration. It therefore cannot prove resume using only saved identity. Added `tests/mesh-fixture/headscale_test.go`: pinned official Headscale 0.29.3 darwin-arm64 SHA256 `fb55b6d3d1ef3b850fe02837299ac1853f383a06f561d7f180d36b9ce406904b`, private fresh database/socket/noise key, loopback endpoints, local DERP only, one-use distinct owner/controller enrollment keys, no production credentials.

The original native test failed after the recreated node returned Running with the same public node key and no new enrollment key (`182200-b3d0`). Actual owner peer visibility and discovery ping did not establish usable TCP (`183417-91c1`, `183906-b79c`). The isolated direct Go probe measured the first GET timeout at 5.001s and the second GET success at 4.996ms on the same node (`184719-5d59`). Native diagnostic `184942-35de` confirmed first connection POSIX22 and second success. The embedded SOCKS implementation has a five-second dial timeout; Apple's adapter reports its failed connect as EINVAL. This establishes a recoverable first connection failure, not a complete upstream root-cause claim.

## Applied repair and limits

Wait for the authenticated owner's actual online mesh address after node startup. Retry only proxied EINVAL during connection establishment, before constructing/sending HTTP bytes; reuse the absolute request deadline, require cancellation checks and retain the same route, owner SAN, SPKI pin, certificate dates and server-auth usage. TLS failures do not qualify. Once any application request is sent, the transport never retries it. Durable command reconciliation still owns missing acknowledgements.

The iPad gate also caught an immediate WebSocket authorization-close race (`185610-9c64`: code4403 became POSIX57). Stop the startup state handler once ready and prefer a received protocol close frame over its simultaneous terminal socket error. The final test exercises thirty immediate authenticated close frames across4401/4403/4413.

## Final evidence

- iPad `laptop-20261026-185814-40c2`:51 tests passed, no skips/failures. Real Headscale, reconstructed saved identity without auth key, first request POST, HTTPS/WSS, wrong-owner and broken/no-proxy negatives, dropped acknowledgement and thirty close frames.
- iPhone `laptop-20261026-185933-5b28`:same51 tests passed, no skips/failures.
- Independent source and evidence review: `/tmp/goose-controller-security-review-current.md`; no confirmed iOS transport regression.
- Actual owner journals independently counted `/command:1`, `/command-lost-ack:1`, `/counts:1`, `/events:2`, `/:1` in each final run. Journals: `/tmp/goose-remote-derived/mesh-run-ABF46DF4-5D52-4EC4-9EC0-7D28DACA42F1/requests.jsonl` (iPad) and `mesh-run-FE5414EB-3E4F-4C20-B8FC-3A21A4D0E655/requests.jsonl` (iPhone).
- Xcode27.1, simulatorSDK27.1, deployment target18.1. Framework provenance remains recorded in the earlier transport proof. No ATS weakening, production service change, device enrollment or App Store upload occurred.

## Trace

Would have changed the motivating failure: YES. On saved-node iteration1, the first SOCKS connect failure reached `start()` as proxied EINVAL before HTTP send. The new `connected()` removes that failed connection and opens another under the same deadline; the independent next connection succeeded. A lost ACK after the POST reaches receive/parsing after `connected()` returned, so it cannot enter the connection-retry loop. Actual owner counts prove one delivery in both final device-family runs.
