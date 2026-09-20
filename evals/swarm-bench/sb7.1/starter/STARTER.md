# SB7.1 starter ownership

This starter supplies process argument parsing, independent service entrypoints, a convenience launcher, HTTP request dispatch with raw body bytes, four static asset routes, and an empty semantic page shell. It uses Python's standard library and browser-native JavaScript without dependencies or a build step. All files are editable.

`create_application(args)` in each service currently returns an object whose every application request raises `NotImplementedError`; the transport responds HTTP 501 `not_implemented`. That error is an explicit starter deficiency, not an allowed final SB7 response. No database or mock product state is created. Empty page regions and disabled controls do not claim that the product loaded, synced, or succeeded.

Implement or replace the request dispatch and runtime as needed. In particular, SSE requires a streaming response path that this simple JSON transport does not implement. Background sync, recovery, vendor calls, database ownership and transactions, event history, webhooks, outbox, authentication, drafts, and notifier behavior are all yours. The supplied launcher handles normal termination of its direct child PIDs; the grader also starts and kills each service independently.

The frontend supplies markup, IDs, neutral layout, and generic matrix/shader-compilation utilities (`MeridianGL` in `web/viz.js`). It supplies no fetching, money/date formatting, interaction, custom dropdown, WebGL context, payment geometry, shader programs, picking, camera controller, labels, brush, or streaming algorithm. The helpers do not draw anything; the complete scene and its behavior are candidate-owned. Replace the starter notice once actual product behavior is implemented. Build the required product design and every state in `SB7-CONTRACT.md`.

## Credit and comparison

The starter can satisfy portions of the unchanged SB7 structure, boot, static asset and page-shell checks before any model implementation. This is disclosed supplied coverage, not model-authored achievement. A reported SB7.1 result must retain the full scorer breakdown and identify this starter revision. It must not be presented as a directly comparable from-empty SB7.0 build, nor claim that these starter contributions were produced by the model. The hard behavioral requirements and scorer remain intact; lower token cost or wall time has not yet been measured.
