# Starter

The starter supplies argument parsing, the three service entrypoints, a launcher, HTTP request dispatch with raw body bytes, four static asset routes and an empty page shell, using only the Python standard library and browser JavaScript. Every file is editable or replaceable.

`create_application(args)` in each service returns an object whose every request raises `NotImplementedError`, answered as HTTP 501 `not_implemented`: unfinished work, never an acceptable final response. No database or product state exists. SSE needs a streaming response path the simple JSON transport does not have. Sync, recovery, vendor calls, storage, events, webhooks, outbox, authentication, drafts and the notifier are yours.

The frontend supplies markup, IDs, neutral layout and matrix/shader-compilation helpers (`MeridianGL` in `web/viz.js`); it draws nothing and has no fetching, formatting, interaction, WebGL context, geometry, picking, camera, labels, brush or streaming. Replace the starter notice once the product works.
