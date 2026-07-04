# Browser-verify smoke oracle — feasibility + vetted design (crunch wwc4rqqay, 2026-07-04)
Motivation: polish-web (sparkboard) shipped BROKEN (DATA exported not imported -> console ReferenceError -> blank chart, visually confirmed) because a static web app has no smoke oracle. fix#3 correctly flagged verified=false (honest-unverified). A browser-verify oracle would SURFACE the specific error.

## FEASIBLE (crunch: feasible=true, worth_building=true, false_fail_risk=low WITH guards, confidence=high; adversarial: implement-with-change, sound only with the guards below).
- Invocation (option d, PROVEN on this machine): the standalone `chrome-headless-shell` binary directly (NO npm/playwright driver — the driver is NOT resolvable under hermit node). `chrome-headless-shell --headless --disable-gpu --no-sandbox --enable-logging=stderr --v=1 --virtual-time-budget=3000 --dump-dom <url>`. stderr emits `CONSOLE ... "Uncaught ReferenceError: X is not defined"` (the polish-web class); --dump-dom gives a render/emptiness check. Exit code is NOT a signal (chrome exits 0 with a page throw). Binary at ~/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-mac-arm64/chrome-headless-shell (glob newest + PATH probes chromium/google-chrome/chrome).
- Detection: minimal special-case at top of run_smoke_gate (swarm.rs ~4105) BEFORE the match: if root/index.html exists AND no package.json/Cargo.toml/pyproject.toml/go.mod AND GOOSE_SWARM_BROWSER_VERIFY on -> return smoke_web(root). Leaves detect_language + planner prompts byte-identical.
- Reuse: smoke_output (fail-open None on spawn-err/timeout), SmokeResult/skipped, honest-unverified reporting. Model smoke_web on smoke_typescript.

## MANDATORY GUARDS (adversarial — the file:// version was UNSOUND without these):
1. Serve over an ephemeral localhost http server (so bundled-local fetches SUCCEED) OR file:// with a STRICT positive filter.
2. POSITIVE filter — flag ONLY code-defect signatures: ReferenceError / SyntaxError / "is not defined". EXCLUDE all else: Failed to fetch, Failed to load resource, net::ERR_, "Cannot read properties of undefined/null", favicon 404, warnings, CDN/CORS on external origins, chrome shutdown chatter.
3. ADVISORY only — do NOT drive the COMPLETE corrective re-dispatch (a phantom fix on a fine-but-needs-server app is the worst outcome). Surface as an event + factor into verified honestly.
4. FAIL-OPEN mandatory — no binary / spawn error / timeout -> skipped() -> verified=false (today's honest-unverified). Env-gated GOOSE_SWARM_BROWSER_VERIFY default-OFF (byte-identical default path).

## RESIDUAL RISK (honest): a fine app needing runtime server context (server-injected bare globals) can throw a ReferenceError indistinguishable from a real bug. false_fail LOW not ZERO -> advisory-only mitigates (a false advisory is noise, not a broken build/phantom fix).

## STATUS: build-ready + vetted, but PRESENTED to the user as a decision — a corner-case (CLI is the main path), lower-confidence new pillar with residual nuance; fix#3 already handles honest-unverified with zero false-green harm today. Build the advisory+guarded version on the user's green-light.
