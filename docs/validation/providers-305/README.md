# Configured provider selection — Goose 3.0.5

Source/tag: 27f8e742f / v3.0.5.

The Add node menu filters the shared cloud adapter list by configured registry IDs, including during loading or a failed read. Setup remains reachable from the dialog. Benchmark provider selection excludes the local-engine entries that belong to Swarm. Cloud Providers renders configured entries first and setup entries below a divider.

The inventory configuration default no longer treats empty/optional/default-only metadata as completed setup. It requires actual configuration or an existing configured provider entry while enforcing required values. Custom providers requiring authentication now mark their API key required. OAuth-specific resolvers are preserved. Configuration is not a live authentication/health check; e.g. an explicitly saved AWS profile or command still needs valid external credentials when used.

Validation: 26 focused desktop tests passed; full desktop suite 2053 passed, 1 skipped; TypeScript, ESLint and locale validation passed. Focused Rust configuration test passed (missing/default-only, explicit setup, missing/blank/present key), cargo fmt and full workspace clippy --all-targets -- -D warnings passed.

Installed from the generated DMG into /Applications/Goose.app. Bundle version 3.0.5. Gatekeeper accepted the Developer ID notarized app. Installed engine and app.asar SHA-256 matched the release bundle:

- engine: 82312e1cda67cc7442e56413c37e92ab263fc7779854e9b44080d357b8794f94
- app.asar: 7e68927a87e9040c18de7da6e1e0853b444679251fdd91b37c1dfd221b21fbdc

Actual installed-app UI: prior profile showed 6 of 63 configured from default metadata; updated profile shows 0 of 63, with Configured and Available to set up sections separated by a horizontal divider. Benchmark single-model view shows no configured providers and disables Model ID/Run. Add node offers only LeanZero MLX. Its Configure in Cloud Providers button navigates to setup; Bedrock configuration modal opens and was cancelled without saving. No credentials or models were changed and no model/benchmark run was started. Desktop screenshot of the grouped cards was inspected.

App and DMG notarization accepted. DMG submission: 058ad7a2-e379-4193-9564-ca76bd405b9c. Prior app retained at /tmp/Goose-3.0.4-before-305.app. Unrelated MCP setup changes in the primary checkout were left untouched.
