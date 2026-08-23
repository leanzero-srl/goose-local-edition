# Cloud SB7 scorer screenshot resolution — 2026-08-23

Verdict: **PASS** for the local hermetic scorer/runtime gate. The browser sandbox now exercises the product, current-run screenshots are sealed before `SCORED`, and the full screenshot bytes remain bound through publication staging and the terminal published audit. No model/provider or website call was made.

## Root cause

The prior `0.2269` scorer result did not exercise the golden app's browser surface. The outer macOS Seatbelt profile denied every Mach lookup while Chromium requires its children to look up the browser-owned service `org.chromium.Chromium.MachPortRendezvousServer.<browser PID>`. Unified logs contain 66,416 denials across the eight browser launches in that scoring window. Each `playwright.chromium.launch()` remained unresolved until the frozen probe's own scenario timer emitted partial `timedOut: true` JSON.

The 23 `PROBE-UNAVAILABLE` rows were a consequence of that harness defect: 20 depended on the timed-out viz evidence, two on flow, and one on the notification feed. Several other checks converted the same empty partial evidence into zeros. Because browser context creation, navigation, and `saveShot` all occur after the unresolved launch, `sb7-shots` remained empty. The scorer accepted parseable partial JSON and its Python caller did not retain stderr on that path, obscuring the common failure. The prior `0.2269` verdict is therefore invalid evidence and must not be published or reused.

## Harness correction

- The scorer profile still denies all Mach lookups by default. Only profiles carrying the pinned Chromium runtime permit the exact Chromium rendezvous namespace with a positive 1–5 digit macOS PID. Zero, leading-zero, six-digit, suffix-extended, other Chromium, branded Chrome, and Apple service names remain denied. No `mach-register` permission was added.
- `score_evidence_seal` now requires stable, regular, non-symbolic PNG evidence and at least one screenshot consumable by the pinned publisher. It records name, byte count, SHA-256, dimensions, publisher classification, and the complete screenshot-tree digest. Empty, malformed, linked, changing, and loaded-only-oversize evidence fails before `SCORED`.
- Publication staging compares the screenshot inventory to the score seal before and after copying. Its runs digest now uses `artifact_tree_sha256`; the previous raw-tree helper intentionally excluded every `sb7-shots` directory.
- The terminal published audit recomputes the complete staged tree, source and staged screenshot inventories, and the manifest's exact file map.

The `loaded` requirement is not invented by the harness. The pinned publisher at commit `694927b0b610c93f0c34dee01004c6def367e670` requires at least one valid `*-loaded.png` no larger than `Math.floor(1.4 * 1024 * 1024)` bytes. The harness neither renames nor substitutes boot/error evidence. A genuinely navigation-broken entrant now fails closed; supporting a boot-only publication would require a separately reviewed publisher-contract change.

## Verification

Focused score, screenshot, staging, published-audit, and sandbox tests: 9/9 passed with warnings treated as errors. The full offline suite passed 208/208 in 56.206 seconds with warnings treated as errors.

The real-process security tests used the runtime-enabled profile and proved that provider-secret reads, raw-tree writes, external network, an explicitly denied localhost listener, outside-process inspection, and outside-process signalling remain blocked. Direct `bootstrap_look_up` checks proved only the bounded Chromium PID namespace crosses the Mach policy. A pinned Playwright 1.57.0 / Chromium revision 1200 canary launched, rendered, captured a 640×480 PNG that macOS `sips` decoded, and left no descendant process.

The provider-free `cloud_sb7.score_one` golden fixture then completed through the real scorer, local vendor, Playwright, Chromium, cleanup, and post-cleanup seal:

- Duration: `224.2948` seconds
- State: `SCORED`; score `0.9961`; 91 checks
- `probe_unavailable=[]`, `harness_missing=[]`, `sched_unreached=[]`
- 12 current-run PNGs, all regular and publisher-accepted; three `loaded` captures
- Screenshot tree SHA-256: `3488d5d29c691f78c5794756df8b445cc1cf75d5596f6d5a56d33e6cb7fa66f1`
- Verdict SHA-256: `a623b9c854366ee493b15c6e14ca0ec8f4f196392cc5de22617068315bf62a5b`
- Score evidence seal SHA-256: `907bd28064460631188926067c91a44a9c16bc030830a89ecf8a321ab569138d`
- Raw tree before/after: `3686466666ec11ed4445bb97dd58a6a830ac466fc20f63e4330009cb51323027`
- `score_evidence_seal_failure=None`; scorer PID, PGID, and inventory cleared
- Ports 9120–9124 free; exact monitor and launcher groups empty

The isolated fixture root was removed after those values were captured. A final process scan found no command referencing it. The scorer/task, website, Sanity, provider credentials, provider APIs, and forensic campaign roots were not modified or invoked.
