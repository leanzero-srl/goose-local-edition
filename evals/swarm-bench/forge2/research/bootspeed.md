# Forge 2.0 research — Custom UI boot speed

Topic owner mandate (2026-10-09): "speed in booting up for the custom UI".
Researched 2026-10-09 (all fetches dated 2026-10-09 unless stated). Primary sources only: developer.atlassian.com,
Atlassian staff posts on community.developer.atlassian.com, Atlassian-owned npm packages / GitHub repo, Atlassian's
production CDN (live HTTP headers fetched today), and Playwright traces of REAL production Custom UI loads on the
wolfaenpak test site recorded by forge-live-harness (2026-08-27 and 2026-09-12). Measurement-method sources:
Chrome DevTools Protocol JSON (ChromeDevTools/devtools-protocol master), Chromium source, Lighthouse docs, web.dev,
Playwright and Puppeteer API docs.

Raw material (all kept for re-reading):
- page text: `raw-bootspeed/*.txt` (+ `.html`), community threads `raw-bootspeed/community/t*.json`
- CDN files + headers: `raw-bootspeed/cdn/` (global-bridge.js, iframeResizer.contentWindow.min.js, *.headers)
- production trace extracts: `raw-bootspeed/trace-sentinel/`, `raw-bootspeed/trace-j6/`
- packages: `pkgs/bootspeed-{bridge,csp,tunnel,clishared,manifest,react}/`
- Atlassian forge-skills: `raw-bootspeed/forge-skills/`
- measurement refs: `raw-bootspeed/measure/`

Quote hygiene: quotes are verbatim page text; the only normalisation is whitespace that the HTML-to-text step put
around hyperlinks (e.g. "iframe , providing" → "iframe, providing").

---------------------------------------------------------------------------------------------------------------

## 0. The short answer (for the designer)

1. Atlassian publishes NO numeric boot-speed requirement for Custom UI: no web-vitals, no load-time SLO, no
   Marketplace load budget. The only Marketplace rule is qualitative ("shouldn't significantly impact the host
   Atlassian app performance"). Developer-console metrics cover backend invocations (Lambda-measured, cold start
   excluded, Custom UI iframe code excluded) and counter-only custom metrics. ⇒ Every boot threshold in Forge 2.0
   must be STATED in the contract; none can be "known from the platform".
2. What Atlassian DOES say, concretely: tree-shake and minify bundles, prefer UI Kit where possible (AtlasCamp 2026,
   Atlassian engineers), use code splitting/shared chunks across entry points (resource limits page), get context from
   the bridge instead of a resolver, call product APIs from the browser for read-heavy user-context reads, cache
   resolver results in component state, do not fire multiple independent `invoke()` calls on page load, use
   `Promise.all` for independent calls, use Realtime instead of polling, show loading/error states (Atlassian's own
   forge-skills repo + optimise-forge-costs page). Feature-flag initialisation "may add latency to app startup".
3. How Custom UI actually boots in production (observed today + Sept/Aug traces): the iframe document is the app's
   `index.html` from `https://<hash>.cdn.prod.atlassian-dev.net/<appId>/<envId>/<deployment>/<resourceKey>/_ctx_<gzip-b64>/`
   with TWO platform scripts prepended, SYNCHRONOUS, at the top of `<head>`:
   `global-bridge.js` (101,996 B raw; 28,810 B gzip -9) and `iframeResizer.contentWindow.min.js` (14,988 B raw).
   `@forge/bridge` captures `globalThis.__bridge` at module load, so the order is load-bearing. App assets are
   served `cache-control: max-age=1728000, …, immutable`; the platform scripts `max-age=86400`. `view.theme.enable()`
   adds 5 stylesheet loads from `forge.cdn.prod.atlassian-dev.net`. Every bridge call (`getContext`, `invoke`,
   `requestJira`, …) is a cross-window RPC to the host page; `invoke` then reaches the Forge function.
4. Wall-clock boot timing is the wrong instrument for an offline, shared-machine benchmark (Lighthouse's own docs:
   client resource contention has HIGH impact and DevTools throttling has NO mitigation; CPU throttling is relative to
   the host). Count-based, causality-based and byte-based metrics are load-independent and still discriminate a
   well-engineered app from a naive one (§3).

---------------------------------------------------------------------------------------------------------------

## 1. How Custom UI is served, and what the platform says about loading fast

### 1.1 Hosting model: iframe, Atlassian-hosted static resources

- "Custom UI runs within an iframe, providing an isolated environment for the app's interface to be displayed."
  — https://developer.atlassian.com/platform/forge/custom-ui/ (Last updated May 21, 2026)
- "A resource is a collection of static assets, which is hosted on and distributed by Atlassian cloud infrastructure."
  — same page
- "path is the relative path from the top-level directory of your Forge app to the directory of the static assets for
  the resource. It should contain the index.html entry point for the Custom UI app" — same page
- "Since the static assets of a Custom UI app are distributed via a URL with a particular path that identifies your
  app, you should use relative paths when accessing these assets from your Custom UI app." — same page
- "For Custom UI apps, the Forge platform hosts your static resources, enabling your app to display within an iframe."
  — https://developer.atlassian.com/platform/forge/manifest-reference/resources/ (Last updated Sep 24, 2026)
- iframe: "All Custom UI apps are run within an iframe. This provides a secure and isolated hosting environment for
  custom-built user interfaces." … "The following permissions are applied to the iframe by default and cannot be
  modified by the developer of the Forge application." Sandbox attributes listed: allow-downloads, allow-forms,
  allow-modals, allow-pointer-lock, allow-same-origin, allow-scripts. Feature policies: camera, clipboard-write,
  display-capture, fullscreen, microphone.
  — https://developer.atlassian.com/platform/forge/custom-ui/iframe/ (Last updated Nov 11, 2024)
  NOTE: the CSP `sandbox` directive actually served (observed, §1.3) also carries `allow-popups
  allow-popups-to-escape-sandbox`; the iframe page is older. Not boot-relevant.
- Developer status page component "Forge CDN (Custom UI)" exists (seen via a third-party status mirror only —
  not verified on Atlassian's own status page).

Offline-test note: the emulator must serve the Custom UI as a real iframe document with the production CSP and the
sandbox (the 1.0 kit already computes the CSP with Atlassian's own `@forge/csp`, `forge/kit/lib/csp.cjs`).

### 1.2 What the platform injects into index.html (bridge startup cost)

Observed in production (Playwright traces of real loads on wolfaenpak; trace files on the AI-workhorse SSD):
- Sentinel/doc app, CDN object last-modified Fri, 24 Jul 2026, served Aug 2026 (trace
  `/Volumes/AI-workhorse/media/forge-live-harness/evidence/20260827-192154/sentinel-steward-console-admin-page-renders-content-in-its-f/trace.zip`),
  the served index.html (394 bytes) is verbatim:
  `<!doctype html><html><head><script src="https://forge.cdn.prod.atlassian-dev.net/global-bridge.js"></script><script src="https://forge.cdn.prod.atlassian-dev.net/iframeResizer.contentWindow.min.js"></script><meta charset="utf-8"><title>Webpack App</title><meta name="viewport" content="width=device-width,initial-scale=1"></head><body><script defer="defer" src="main.js"></script></body></html>`
- CogniRunner admin, CDN object last-modified Sat, 12 Sep 2026 (trace
  `…/evidence/20260912-193559/j6-admin-rules-disable-first-rule-flips-registry-ui-re-enabl/trace.zip`): same two
  scripts, same order, both without `async`, prepended before the app's own `<meta charset>`.
- So the injection happens at deploy/upload time (the stored S3 object already contains it: content-length 394 with
  the scripts inside) and BOTH platform scripts are parser-blocking.

The code that does it (Atlassian's CLI shared lib, same CDN path):
- `@forge/cli-shared@9.7.0` `out/service/bridge-script-service.js`:
  `const BRIDGE_CORE_CDN_PATH = 'global-bridge.js';` … `return `<script src="https://${GLOBAL_FORGE_INSTALL_ID}.cdn.${this.env}.atlassian-dev.net/${BRIDGE_CORE_CDN_PATH}"></script>`;` … `head.prepend(this.createBridgeCoreScriptTag());`
  — https://unpkg.com/@forge/cli-shared@9.7.0/out/service/bridge-script-service.js
- `out/service/iframe-resizer-script-service.js`: `<script async src="https://${GLOBAL_FORGE_INSTALL_ID}.cdn.${this.env}.atlassian-dev.net/${IFRAME_RESIZER_CDN_PATH}"></script>`
  (the tunnel version is `async`; production as observed is NOT async — discrepancy, production wins).
- `@forge/tunnel@7.2.0` `out/servers/resource-tunnel-server.js` applies resizer then bridge injection, then computes
  the CSP header from the rewritten HTML.

Why the order matters — `@forge/bridge@7.1.0` (published 2026-10-09T00:10Z):
- `out/bridge.js`: `if (!isBridgeAvailable(globalThis.__bridge)) { throw new errors_1.BridgeAPIError(` "Unable to
  establish a connection with the Custom UI bridge." … `return globalThis.__bridge.callBridge;`
- `out/invoke/invoke.js` top level: `const callBridge = (0, bridge_1.getCallBridge)();` — i.e. captured at MODULE LOAD.
  — https://unpkg.com/@forge/bridge@7.1.0/out/invoke/invoke.js
- `global-bridge.js` sets it: `void 0===o.__bridge&&(o.__bridge=i)`, exposes `{callBridge: …, __SEMVER:"1.15.3"}`,
  uses a MessageChannel/port-RPC handshake (`HANDSHAKE_MSG_TYPE="__port_rpc_handshake__"`, `HANDSHAKE_TIMEOUT=5e3`)
  and at init sends a `"__ready"` request to the host (`e.adapter.request(r.xp,{})`, `xp="__ready"`) (minified code
  reading, moderate confidence).

Size and caching of the platform scripts (fetched live 2026-10-09 14:43 GMT):
- `https://forge.cdn.prod.atlassian-dev.net/global-bridge.js`: `content-length: 101996`, `cache-control: max-age=86400,
  s-maxage=86400, stale-while-revalidate=3600`, `last-modified: Mon, 28 Sep 2026 00:37:34 GMT`, `x-cache: Hit from
  cloudfront`, served gzip when accepted. Locally `gzip -9` = 28,810 bytes.
- `https://forge.cdn.prod.atlassian-dev.net/iframeResizer.contentWindow.min.js`: 14,988 bytes raw (5,614 gzip -9),
  same cache-control.
- `@forge/bridge@7.1.0` own JS (68 files, CommonJS, `out/index.js` re-exports every submodule eagerly, no
  `sideEffects`/`module` field ⇒ little tree-shaking): 80,937 B raw / 16,673 B gzip -9 before minification, excluding
  its deps (`@forge/i18n`, `@forge/egress`, `tslib`).
- React 18.3.1 for scale (npm tarballs, measured): `react-dom.production.min.js` 131,835 B / 42,912 B gzip -9;
  `react.production.min.js` 10,751 / 4,287; DEVELOPMENT builds: `react-dom.development.js` 1,080,227 / 232,575;
  `react.development.js` 109,931 / 28,254.

Offline-test note: the platform scripts are a fixed, unavoidable cost — the grader must EXCLUDE them (and the
theme token CSS) from the app's budgets but the emulator must reproduce the ORDER (bridge before app code).

### 1.3 Content Security Policy — effect on fonts, CDNs, inline scripts and styles

Docs:
- "all Custom UI apps are served with a content security policy (CSP)." … "All scripts and assets used in your Custom UI
  app must come from the same resource directory as your Custom UI app. This means that you cannot use scripts or
  images from external sources, such as Google Analytics or Sentry, in your static assets." … "If there is code in
  your Custom UI app that violates the CSP, the app will not behave as expected, and an error will be shown in the
  browser console." — https://developer.atlassian.com/platform/forge/custom-ui/
- "By default, Atlassian blocks any policies that are considered unsafe for your Custom UI app. To include
  capabilities, such as inline CSS, you will need to declare these policies in the manifest.yml file of your app."
  and "Using the capabilities discussed on this page may make your app ineligible for Runs on Atlassian."
  — https://developer.atlassian.com/platform/forge/add-content-security-and-egress-controls/ (Last updated Apr 8, 2026)
- `permissions.content.scripts` sources: "unsafe-inline", "unsafe-hashes", "unsafe-eval", "blob:",
  "<sha-algorithm>-<base64-value> Allows a specific script to be executed, provided it matches the hash declared here.
  The only valid hash algorithms are: sha256, sha384, and sha512." `permissions.content.styles`: "unsafe-inline".
  External: `fetch.client` (connect-src), `fonts` (font-src), `styles` (style-src), `frames`, `images`, `media`,
  `scripts` (script-src). — https://developer.atlassian.com/platform/forge/manifest-reference/permissions/
  (Last updated Sep 16, 2026)
- Runs on Atlassian: "Eligible apps do not list any of the following in the manifest: External resource domains,
  except when these domains are used for the purpose of analytics." — https://developer.atlassian.com/platform/forge/runs-on-atlassian-apps/
  (Last updated Jul 14, 2026) ⇒ a RoA-eligible app self-hosts every font/script/style in its resource bundle.

Observed production CSP header (index.html of the Sentinel resource, `date: Fri, 14 Aug 2026`), relevant directives
(FOS hosts elided as `<FOS>` = the tdp-os/object-store download URLs):
- `default-src 'self' <FOS>`
- `script-src 'self' https://forge.cdn.prod.atlassian-dev.net <FOS>` (no unsafe-inline / unsafe-eval)
- `style-src 'self' wolfaenpak.atlassian.net https://forge.cdn.prod.atlassian-dev.net <FOS>` (no unsafe-inline unless
  declared — the CogniRunner frame, which declares it, shows `'unsafe-inline'` here)
- `font-src 'self' <FOS>` (no external fonts)
- `connect-src 'self' wolfaenpak.atlassian.net https://api.atlassian.com/metal/ingest … https://forge-outbound-proxy.services.atlassian.com …`
- `form-action 'self'`; `sandbox allow-popups allow-popups-to-escape-sandbox allow-downloads allow-forms allow-modals allow-pointer-lock allow-same-origin allow-scripts`;
  `report-uri https://web-security-reports.services.atlassian.com/csp-report/forge-cdn`
- the object carries `x-amz-meta-csp: {"style-src":[],"script-src":[],"img-src":["https://apps.stagil.com/sncc/icon"],"media-src":[],"connect-src":[],"font-src":[],"frame-src":[]}`
  — exactly the key order produced by `@forge/csp`'s `CSPProcessingService.getCspDetails` (style-src, script-src,
  then img/media/connect/font/frame), i.e. the per-app CSP part is computed at deploy time with that library.

The library (`@forge/csp@6.3.1`, "Contains the CSP configuration for Custom UI resources in Forge"):
- `this.STYLE_SRC_ALLOWLIST = [`'unsafe-inline'`];` `this.QUOTED_SCRIPT_SRC_ALLOWLIST = ['unsafe-inline', 'unsafe-eval', 'unsafe-hashes'];`
- `const generatedScriptHashes = validUserScriptSrc.includes('unsafe-inline') ? [] : this.getInlineScriptHashes($);`
  with `getInlineScriptHashes($) { return $('script:not([src])').map(... `sha256-${this.hashScript(html)}` ...` —
  STATIC inline `<script>` blocks present in index.html are auto-hashed into script-src; scripts injected at
  runtime are not. — https://unpkg.com/@forge/csp@6.3.1/out/csp/csp-processing-service.js
- `CSPInjectionService.getInjectableCSP` builds the directive list above ('prod' env, report-uri `forge-cdn`).
  — https://unpkg.com/@forge/csp@6.3.1/out/csp/csp-injection-service.js

Boot-speed consequences (derived):
- Fonts: no Google Fonts/CDN without `external.fonts` (+ the stylesheet host in `external.styles`) — which costs RoA
  eligibility. A fast, eligible app uses the system font stack / Atlassian tokens, or ships woff2 inside the bundle
  (then font bytes are part of its critical path if preloaded).
- CDNs: no shared JS CDN (unpkg/jsdelivr) ⇒ every byte the app executes comes from its own resource; the bundle size
  is entirely the app's responsibility.
- Inline: runtime CSS-in-JS (`<style>` injection: emotion, styled-components, style-loader) needs
  `content.styles: unsafe-inline`; inline `<script>` in the shipped index.html is tolerated (auto-hash);
  `eval`/`new Function` (some template engines, some dev builds) need `unsafe-eval`.

Offline-test note: serve the production CSP (as 1.0 does) and collect CSP reports through the emulator's reporter:
"CSP violations during boot = 0" is binary and load-independent. Contract must state: production CSP is enforced;
external domains are forbidden (RoA). Do NOT require knowledge of the undocumented auto-hash — the contract can say
"the emulator computes the CSP with Atlassian's @forge/csp exactly as production does".

### 1.4 CDN caching and URL structure (observed)

- App assets (index.html, JS, CSS) from `*.cdn.prod.atlassian-dev.net`: `cache-control: max-age=1728000,
  s-maxage=1728000, stale-while-revalidate=86400, immutable`, `server: AmazonS3`, `via: … (CloudFront)`, JS/CSS
  `content-encoding: gzip`; some first loads `x-cache: Miss from cloudfront`, later `Hit from cloudfront`.
- URL shape: `https://<hash>.cdn.prod.atlassian-dev.net/<appId>/<envId>/<uuid>/<resourceKey>/_ctx_<gzip+base64url>/<asset>`.
  The `_ctx_` segment decodes (gzip, base64url) to installation-level data, e.g.
  `{"egress":[{"addresses":["https://apps.stagil.com/sncc/icon"],"type":"IMAGES"},{"addresses":["https://apps.stagil.com/sncc"],"type":"FETCH_BACKEND_SIDE"}],"hostname":"wolfaenpak.atlassian.net","installationConfig":[]}`
  — not per user or per issue. ⇒ asset URLs are stable per (deployment, site/installation config), so a repeat visit
  on the same site is served from the browser cache; the expensive load is the FIRST load after each deploy.
- Iframe URL query observed: `?platformFeatureFlags=forge-ui-iframe-analytics%2Cforge-ui-iframe-ufo-perf-observers%2C…`
  — suggests Atlassian observes iframe performance internally (UFO); nothing about it is documented or exposed to
  developers (unverified).
- Theme: `view.theme.enable()` made the bridge append five stylesheets from the platform CDN, observed:
  `atlaskit-tokens_shape.css`, `_spacing.css`, `_motion.css`, `_typography.css`, `_light.css`
  (`cache-control: max-age=1728000, … immutable`). Code in global-bridge.js: `var o=document.createElement("link");o.rel="stylesheet",o.href=n,o.dataset.theme=t,document.head.appendChild(o)`.

Offline-test note: measure the COLD (fresh browser context) boot only; caching headers are the platform's, not the
app's. Do not grade warm-cache behaviour beyond "no runtime cache-busting of own assets" (optional).

### 1.5 Documented limits (resources, files, payloads, invocations)

- Resource limits — https://developer.atlassian.com/platform/forge/limits-resource/ (Last updated Jul 7, 2026):
  "Your app can have up to 50 resources, with each resource bundle containing up to 5,000 files and 100 MB."
  Cumulative: "Total files 25000 … Total bundle size 1 GB". Entry points: "define up to 50 named entry points within a
  single resource". And the only explicit load-time sentence in the docs:
  "Smaller deploy size and faster load times — shared dependencies across entry points can be extracted into common
  chunks, reducing total deploy size and allowing shared code to be cached once and reused across all entry points."
  … "For Custom UI, you can achieve the same benefit by configuring code splitting in your own build pipeline (see
  webpack code splitting or Vite multi-page app)."
- `entry` property (resources reference, Sep 24, 2026): "An optional map of named entry points within this resource.
  … an .html file for Custom UI (for example, global.html). Nested paths … are not supported. A maximum of 50 entries
  are allowed per resource. When entry is defined, modules reference a specific entry using the slash syntax:
  resource: <resource-key>/<entry-key>." and "This feature is supported for Jira and Confluence modules."
  The Custom UI page calls it "Resources — Multiple entry points (Preview)".
- CONFLICT: https://developer.atlassian.com/platform/forge/limits-app-developer/ (Last updated Dec 1, 2025) still says
  "Resources per app 10 Maximum number of unique resources that can be declared in a single app manifest."
  The CLI decides: `@forge/manifest@13.6.0` `out/validators/resources-validator.js` → `exports.MAX_RESOURCE_COUNT = 50;`
  with error text `document exceeds ${limit} resources`. ⇒ the kit's lint must use 50.
- Weekly resource quotas (Custom UI page, May 21, 2026): "Resource quotas are consumed per deployment to your
  production environment; deployments to development and staging environments are unmetered. These quotas are
  refreshed weekly." Table: "File capacity (weekly) 150 MB 75 MB 75 MB / Files uploaded (weekly) 500 files 250 files
  250 files" (paid / free / distributed). The "Calculate resources usage" anchor on the platform limits page no longer
  exists and that page (Jul 13, 2026) does not list weekly resource quotas — possibly stale; unverified.
- Front-end invocation payloads — https://developer.atlassian.com/platform/forge/limits-invocation/ (Sep 1, 2026):
  "Front-end invocation request payload size 500KB" and "Front-end invocation response payload size 5MB"; resolver
  runtime "25" seconds; user-led rate limits "Per user 1,200 per minute" and "Per install 7,000 per minute and 300 per
  second (whichever is hit first)"; "For UI invocations, use rate limit metadata from invoke and retry after
  rateLimitReset".
- Client-side limiter inside the bridge (`@forge/bridge@7.1.0` invoke.js):
  `const limitedInvoke = (0, utils_1.withRateLimiter)(_invoke, 500, 1000 * 25, 'Resolver calls are rate limited at 500req/25s');`
  — `withRateLimiter` THROWS `BridgeAPIError` when exceeded (it does not queue). Same 500/25 s limiter on
  `invokeRemote`/container invokes and `initFeatureFlags`; none on `requestJira`/`getContext`.

Offline-test note: lint-time checks (counts, sizes, entries) are deterministic. The 300/s per-install and
1,200/min per-user limits make "invokes per boot" a real scaling quantity (a page that fires 6 invokes per open
supports ~50 opens/s per install before 429s).

### 1.6 Per-call cost of the bridge at boot

- `view.getContext()` is an RPC every call — no caching in the client: `const context = await callBridge('getContext');`
  (https://unpkg.com/@forge/bridge@7.1.0/out/view/getContext.js). Docs: "Not all of the values in the context data
  are guaranteed to be secure, unalterable, and valid to be used for authorization."
  — https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/view/
- `view.theme.enable()` = `enable: () => callBridge('enableTheming')`; docs: "This will fetch the current active theme
  from the host environment (e.g. Jira) and apply it in your app." and the documented React pattern:
  "// Make sure to call enable() before the app is mounted for tokens to be available before the first render".
  ⇒ awaiting theme.enable before mount is DOCUMENTED best practice and must not be penalised.
- `requestJira` = `callBridge('fetchProduct', …)` (host performs the REST call as the user).
- `invoke` = `callBridge('invoke', { functionKey, payload, metadata })` → host → Forge function. A developer measured
  "~1 seconds of 'nothing-ness'" between iframe resource load and the `forge_ui_invokeExtesion` request (Dec 2025,
  https://community.developer.atlassian.com/t/the-forge-invoke-pause-any-hint-where-the-1-second-goes/97589 — no staff
  reply; anecdotal).
- Historical latency (staff, 2023): "the invocation overhead is between 100ms - 200ms (from the Platform service
  received the invocation request to Platform service sending request to your local backend service (tunnel) ), which
  matches our SLO." — JingYuan (Atlassian Staff), https://community.developer.atlassian.com/t/forge-invoke-is-very-slow/68746
  and the Forge runtime PM: "I suspect what you are are seeing is geographic latency because you’re in Europe but
  Forge apps are deployed in US West." (same thread, 2023-05-01; multi-region was "planned"). 2021: staff asked
  "I’m wondering how much of this is Lambda’s “cold start” problem." (https://community.developer.atlassian.com/t/latency-of-forge-custom-ui-bridge-resolver/46936).
  ⇒ one backend round trip costs hundreds of ms in the field — the number of SEQUENTIAL round trips before first
  data render is the dominant, app-controlled boot factor.

### 1.7 Atlassian guidance for a fast Custom UI (all the guidance that exists)

- AtlasCamp 2026 session by Atlassian engineers (Anish Kumar, "Principal Engineer - Ecosystem | UI Extensibility,
  Atlassian"; Prasad Pathapati, "HoE, Ecosystem Platform Extensibility, Atlassian"), June 23, 2026: "Best practices to
  keep your Forge app fast — choose UI Kit over Custom UI where possible, tree-shake and minify bundles, batch storage
  calls, keep event handlers idempotent and quick, defer heavy work to async consumers and use narrow manifest filters
  so only relevant events reach your code."
  — https://events.atlassian.com/atlascamp26-bengaluru/session/4053173/scaling-forge-apps-practical-strategies-for-performance-and-cost-optimization
- Optimise Forge platform costs (Aug 21, 2026) — https://developer.atlassian.com/platform/forge/optimise-forge-costs/ :
  "Forge's UI Kit and Custom UI frontends run entirely in the browser and are not subject to function invocation costs
  or limits."; "requestJira() / requestConfluence() — both UI Kit and Custom UI can call Atlassian REST APIs directly
  from the browser using the Forge bridge, without invoking a backend function at all."; "This eliminates the
  round-trip through a Forge function for read-heavy operations like fetching issue details or page content.";
  "Caching fetched data — storing the result of a resolver call in component state and reusing it across interactions,
  rather than re-invoking the resolver on every render."; "A surprisingly common anti-pattern is invoking a Forge
  resolver to look up contextual metadata that is already available in the frontend context"; "Note that API requests
  from the frontend are always authenticated as the context user. Secure operations, such as accessing Forge storage,
  can only be accessed via a resolver."; Realtime instead of polling: "This can cause unnecessary compute usage and a
  lengthy wait for the user."
- Atlassian-authored agent skills (github.com/atlassian/forge-skills, Atlassian GitHub org, latest commit
  2026-10-01; NOT product docs but Atlassian's own review rubric):
  forge-app-review "Lightweight Cost Signals": "Resolver invoked only to return static data or product context." and
  "Multiple independent `invoke()` calls on page load."; "Lightweight Debuggability Signals": "Missing loading/error
  states around async UI paths." — https://github.com/atlassian/forge-skills/blob/main/skills/forge-app-review/SKILL.md
  forge-cost-optimizer: "`invoke()` calls inside render bodies, unbounded effects, repeated event handlers, or multiple
  calls on page load that can be cached or batched."; "Sequential independent calls. Use `Promise.all` or bounded
  concurrency."; "Unbounded concurrency. Batch large workloads to avoid rate limits; use about 5–10 concurrent requests
  unless docs or tests justify otherwise."; "Large resolver responses. Return only fields consumed by the UI."; and the
  guard "Never reduce costs by weakening authorization, exposing secrets to the frontend, skipping required validation,
  dropping necessary error handling, or making data stale beyond the user's business requirements."
  — https://github.com/atlassian/forge-skills/blob/main/skills/forge-cost-optimizer/SKILL.md
- Feature flags (client SDK in @forge/bridge): "Downloads flag configuration and prepares the SDK for evaluation. Must
  be called before using checkFlag." / "Call initialize() once at app startup, not on every render." / "Synchronous
  after initialization." — https://developer.atlassian.com/platform/forge/feature-flags/feature-flags-client-sdk/
  (Sep 2, 2026); limitations page: "Flag evaluation latency - Feature flag initialization may add latency to app
  startup" — https://developer.atlassian.com/platform/forge/feature-flags/limitations/ (Mar 25, 2026).
- Resource page: shared chunks / code splitting (quoted in 1.5).
- UI Kit bundle (staff relay, 2024-12-13): "this is only the local installation size, this is not reflected on the
  actual runtime bundle size which is very minimal" — https://community.developer.atlassian.com/t/please-reduce-the-ui-kit-package-size/87125

Nothing in the docs gives: a byte budget, a request budget, a time-to-interactive target, preload guidance, or a rule
on `invoke` before first paint. Those are benchmark inventions and must be stated.

### 1.8 Native rendering vs iframe (Atlassian's own performance position)

- Staff, 2026-05-11, on why custom field view renderers are UI Kit-only: "The short answer is for performance and
  scalability reasons. On some screens, there could be lots of custom field renderers on the page (eg. issue list
  view). Loading a separate iframe in each cell of a multiple columns of a 50 row table would not deliver a good
  experience. UI Kit performance scales much better in this scenario."
  — https://community.developer.atlassian.com/t/why-are-some-modules-implementation-restricted-to-using-ui-kit/100594
- Changelog 6 Oct 2026, Confluence static macros (Preview): "Static macros allow you to render Forge macro content
  natively on Confluence pages without iframe overhead. By returning ADF or Confluence Storage Format nodes directly,
  your app improves page load performance and provides a more seamless user experience." and "Native rendering :
  Content is rendered as part of the page HTML, eliminating iframe latency." — https://developer.atlassian.com/platform/forge/changelog/
- `emitReadyEvent` (Confluence bodied macros only): "This enables Confluence or other consumers, such as our PDF export
  service, to reliably detect when macros are fully loaded, rather than relying on DOM scanning or timing heuristics."
  — https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/view/ ⇒ Atlassian itself prefers an
  explicit ready signal over timing heuristics — precedent for a stated "ready" contract in the benchmark.

---------------------------------------------------------------------------------------------------------------

## 2. Platform-provided performance metrics and program requirements

- Invocation metrics (Jul 13, 2026) — https://developer.atlassian.com/platform/forge/monitor-invocation-metrics/ :
  "This invocation time is measured from inside the AWS lambda, and doesn't include cold start, but it includes the
  time it took for the lambda initialization phase to complete." and "Invocation response time doesn’t include code
  executing in a Custom UI iframe, but includes functions invoked by @forge/bridge." Percentiles P50/P90/P95.
- API metrics (Oct 30, 2024): API response time P50/P95/P99 for the app's HTTP requests —
  https://developer.atlassian.com/platform/forge/monitor-api-metrics/
- Custom metrics (Jun 25, 2026): "You can register up to 20 custom metrics per app. Only counter-type metrics are
  currently supported." and frontend emission via `frontendCustomMetrics` from @forge/bridge; reference (Jun 27, 2026):
  "counter is currently the only supported metric type. Other metric types (such as gauge or histogram) aren't
  available." — https://developer.atlassian.com/platform/forge/monitor-custom-metrics/ ,
  https://developer.atlassian.com/platform/forge/custom-ui-bridge/frontendCustomMetrics/
  ⇒ no timing/web-vitals metric for Custom UI exists; a developer can only bucket-count.
- Marketplace approval (May 15, 2026): "Doesn't degrade host Atlassian app performance : Your app shouldn't
  significantly impact the host Atlassian app performance." — https://developer.atlassian.com/platform/marketplace/app-approval-guidelines/
  (qualitative only).
- Runs on Atlassian: privacy/egress/residency requirements only; no performance requirement —
  https://developer.atlassian.com/platform/forge/runs-on-atlassian/ (Aug 24, 2026).
- Atlassian Enterprise Certified (Oct 1, 2026): reliability = incident management/Statuspage; "We are no longer
  accepting new submissions for the Cloud Fortified Apps Program. Cloud Fortified will be phased out by the end of
  CY2026." — https://developer.atlassian.com/platform/marketplace/atlassian-enterprise-certified-program/
- Runtime knobs that affect resolver speed (manifest reference): "Increasing the function memory also increases its
  CPU allocation. The default value is 512 MB. The value can be between 128 MB and 1,024 MB." and "Lambda functions
  powered by arm64 architecture are designed to deliver up to 19 percent better performance at 20 percent lower cost."
  — https://developer.atlassian.com/platform/forge/manifest-reference/

---------------------------------------------------------------------------------------------------------------

## 3. What an OFFLINE benchmark can measure objectively and robustly to machine load

### 3.1 Why not time (primary sources)

- Lighthouse: "Other applications running on the same machine while Lighthouse is running can cause contention for
  CPU, memory, and network resources." … "Running multiple instances of Lighthouse at once also typically distorts
  results due to this problem. DevTools throttling is susceptible to this issue." and "DO NOT collect multiple
  Lighthouse reports at the same time on the same machine. Concurrent runs can skew performance results due to
  resource contention." — https://github.com/GoogleChrome/lighthouse/blob/main/docs/variability.md
- "Unlike network throttling where objective criteria like RTT and throughput allow targeting of a specific
  environment, CPU throttling is expressed relative to the performance of the host device."
  — https://github.com/GoogleChrome/lighthouse/blob/main/docs/throttling.md  (⇒ CDP CPU throttling multiplies load
  noise; it does not normalise it.) CDP: `Emulation.setCPUThrottlingRate` "Throttling rate as a slowdown factor (1 is
  no throttle, 2 is 2x slowdown, etc)."
- TBT/long tasks are wall-time lab metrics: "a task that runs on the main thread for more than 50 milliseconds"; "TBT is
  a metric that should be measured in the lab" — https://web.dev/articles/tbt

Load-independent instruments that exist (primary):
- Thread CPU clock: Chromium `ThreadTicks` "Represents a thread-specific clock that runs only while the thread is
  scheduled. This has the effect of counting time spent actually executing code, but not time spent blocked (e.g. on
  I/O), or ready and waiting to be run." — https://chromium.googlesource.com/chromium/src/+/main/base/time/time.h ;
  CDP `Performance.enable` param `timeDomain` enum `["timeTicks", "threadTicks"]` "Time domain to use for collecting
  and reporting duration metrics." (devtools-protocol browser_protocol.json). Metrics available (Puppeteer
  `Metrics`): ScriptDuration "Combined duration of JavaScript execution, in seconds.", TaskDuration "Combined duration
  of all tasks performed by the browser, in seconds.", LayoutCount "Total number of full or partial page layouts.",
  RecalcStyleCount "Total number of page style recalculations.", Nodes, JSHeapUsedSize
  — https://github.com/puppeteer/puppeteer/blob/main/docs/api/puppeteer.metrics.md (also https://pptr.dev/api/puppeteer.page.metrics)
- V8 precise coverage: `Profiler.startPreciseCoverage` `callCount` "Collect accurate call counts beyond simple 'covered'
  or 'not covered'.", `detailed` "Collect block-based coverage."; `CoverageRange.count` "Collected execution count of the
  source range." Note "Enabling prevents running optimized code and resets execution counters." (js_protocol.json) —
  deterministic work counts, independent of CPU speed.
- Network causality: `Network.requestWillBeSent.initiator` with `type` enum `parser|script|preload|…` and `url`/`stack`;
  `Network.loadingFinished.encodedDataLength` "Total number of bytes received for this request." — resource waterfall
  depth and bytes without timing.
- Virtual time: `Emulation.setVirtualTimePolicy` "Turns on virtual time for all frames (replacing real-time with a
  synthetic time source)"; policies "advance … pause … pauseIfNetworkFetchesPending" (experimental) — makes app timers
  deterministic if needed.
- Frame targets: Playwright `browserContext.newCDPSession(page)` — "it can be a `Page` or `Frame` type"; "CDP sessions
  are only supported on Chromium-based browsers." — https://playwright.dev/docs/api/class-browsercontext
  (if the emulator's iframe is same-site with the host page it shares the main thread ⇒ filter coverage by app
  script URL rather than reading page-level Performance metrics).

### 3.2 Proposed boot metrics (all count/byte/causality based) — with thresholds

Definitions the contract must state: SURFACE = the front-facing Custom UI module(s); APP ORIGIN = the app's own
resource files (index.html, JS, CSS, fonts, images it ships); PLATFORM-INJECTED = global-bridge, iframe resizer, theme
token CSS (excluded from budgets); BACKEND-BOUND bridge ops = `invoke`, `requestJira`/`requestConfluence`/
`requestBitbucket`, `requestRemote`/`invokeRemote`, `initFeatureFlags`, Realtime subscribe; HOST-LOCAL ops =
`getContext`, `theme.enable`, `createHistory`, `changeWindowTitle`, flags/metrics emission (answered immediately, free);
READY = the stated selector(s) present AND showing the seeded data (verified against the scoring seed, so a fake marker
fails); SHELL = the stated layout selector(s) (header, nav, empty skeleton).

Measurement protocol ("hold-and-release waves", load-independent): cold browser context; production CSP; host-local
ops answered at once; backend-bound ops are QUEUED; when the frame is settled (no in-flight resource fetch for the
frame, two consecutive `requestIdleCallback` turns with no new bridge op) the emulator releases ALL queued responses
as one wave; repeat until READY or no progress. Machine load only stretches wall time; it cannot change which calls the
app issues before which responses. (Same idea backend-side: the emulator's mock fetch holds outbound calls of one
invocation and releases them in rounds when the Node event loop is idle ⇒ sequential depth of the resolver.)

| id | metric (what the grader observes) | well-engineered | naive | proposed threshold (STATE it) |
|---|---|---|---|---|
| B1 | SHELL present before any backend-bound response (wave 0) | renders layout immediately, data slots loading | `await invoke()` before `ReactDOM.render` → blank iframe | binary: shell visible at wave 0 |
| B2 | backend waves until READY | 1 (bootstrap issued at module top-level or first effect) | getContext→invoke(config)→invoke(data)→invoke(prefs) = 3+ | ≤ 1 wave |
| B3 | backend-bound ops issued before READY | 1 bootstrap `invoke` (+ ≤1 `requestJira`) | one invoke per widget/row (6–30+) | ≤ 2 (front surface) |
| B4 | app-origin bytes fetched before READY (sum of gzip -9 sizes of the served files, platform-injected excluded) | ~60–150 KB (React prod ≈ 47 KB gz + bridge + app) | dev React (+214 KB gz), Atlaskit-heavy/all-in-one bundle (400 KB–3 MB) | ≤ 200 KB gz, or ≤ 1.5× the golden's measured value, frozen as a number at release |
| B5 | app-origin requests before READY | index.html + entry JS + CSS (+1 chunk) = 3–4 | dozens of chunks, per-locale files, icon sprites | ≤ 6 |
| B6 | resource initiator-chain depth (parser→script→script…) before READY | 2 (html → entry/css) | lazy-chunk chains 4–6 deep | ≤ 3 |
| B7 | admin-only code not shipped to the front | front entry excludes admin views/charts/editor | shared mega-bundle | covered by B4 (+ optional: no admin-only module path in front's loaded scripts) |
| B8 | bootstrap resolver: outbound calls and sequential rounds (emulator call log + round release) | bulk/search with `fields`, `Promise.all` independent reads: ≤ 3 rounds | N+1 per item, sequential awaits | rounds ≤ 3; call count does not grow with item count (run seed N and 4N: calls(4N) ≤ calls(N) + extra pages) |
| B9 | bootstrap response payload bytes (JSON) | only fields the UI renders | whole Jira issue objects | ≤ 2× golden (or a stated KB number); hard platform cap 5 MB |
| B10 | JS work before READY: V8 precise-coverage call counts over app scripts (and/or ScriptDuration in threadTicks, compared to the golden measured in the SAME session) | baseline | re-render storms, dev builds, sync heavy parsing | ≤ 3× golden; secondary weight (threadTicks still varies with CPU frequency/caches) |
| B11 | CSP violation reports during boot; requests to non-app, non-platform origins | 0 / 0 | external fonts, CDN scripts, eval | binary: 0 and 0 |
| B12 | flags/realtime do not gate the shell: SHELL present while `initFeatureFlags`/subscribe responses are held | uses `checkFlag(name, default)` after init, shell first | `await featureFlags.initialize()` before render | binary |
| B13 | in-session re-use: scripted navigation between front views after READY | 0 repeated bootstrap invokes | re-invokes on every render/tab | 0 repeats (Atlassian: cache resolver results in component state) |
| B14 | boot under a 429 on the bootstrap invoke (`rateLimitProperties` metadata, retry after `rateLimitReset`, virtual time) | shell stays, one retry at/after reset, READY | blank/error page or immediate retry storm | exactly 1 retry, none before reset (shared with the tier/robustness topic) |

Calibration: per gate 10 (ratios/measurements), derive B4/B9/B10 from the golden (and an alternative app) and then
FREEZE the absolute numbers into the contract text — the model must be able to read the number.

What each metric needs STATED (never grade unstated requirements):
- READY and SHELL selectors per surface and the exact seeded content READY must show.
- That boot is graded on a cold load, by counting (waves, ops, bytes, requests, chain depth) — not by time; platform
  scripts, token CSS, `getContext` and `theme.enable` are free; which ops are backend-bound.
- Every numeric budget (B3, B4, B5, B6, B8 rounds, B9) and the gzip level used for B4.
- That the production CSP is enforced and no external origins are allowed (RoA).
- That bridge `invoke` is client-side limited to 500 per 25 s and throws (the real bridge does this — state it anyway).
- For B8: the seeds differ in size between dev and scoring sites, and that call counts must not scale per item.
- For B14: the 429 shape (`rateLimitProperties` metadata) — cross-reference with the tier/robustness section.

Fairness traps to avoid (lessons):
- Do not penalise the documented `await view.theme.enable()` before mount (it is host-local and free in B2).
- Do not count platform-injected scripts/CSS against the app.
- Do not use wall-clock or CPU-throttled time as a gate (1.0's distortion); B10 only as a ratio to the golden in the
  same session, low weight.
- Verify READY content, not just the marker (anti-gaming); count requests the app could hide by inlining (index.html
  bytes count in B4).
- Use a scoring seed different from the dev seed so data cannot be baked into the bundle.
- If the emulator uses the real `@forge/bridge` (1.0 does: the kit injects `globalThis.__bridge` before the bundle),
  the 500/25 s limiter and module-load capture behave exactly as production.

---------------------------------------------------------------------------------------------------------------

## 4. Open questions / could not verify

1. Whether production auto-hashes STATIC inline `<script>` in index.html (inferred from `@forge/csp` code + the
   `x-amz-meta-csp` key order; not observed with an app that has an inline script).
2. Whether `entry`-based multi-entry Custom UI resources share chunk URLs across entries (docs claim shared caching;
   the URL shape with `entry` was not observed).
3. Whether the weekly resource upload quotas (150 MB / 500 files paid) still apply after consumption pricing (Custom UI
   page says yes; the current platform-limits page omits them; its "calculate resources usage" anchor is gone).
4. Current real invoke round-trip latency (only 2021/2023 staff/community numbers and a Dec-2025 developer
   observation of a ~1 s pre-invoke pause with no staff reply). Regions: multi-region deployment status not checked.
5. UI Kit 2 runtime model (where @forge/react code executes) — not documented; boot metrics for the UI Kit admin can
   only be count-based (B2/B3/B8), not byte-based.
6. Meaning of the iframe feature flag `forge-ui-iframe-ufo-perf-observers` — undocumented.
7. The global-bridge `__ready` handshake semantics (read from minified code; moderate confidence) and whether the host
   shows a spinner until it.
8. The full Forge changelog history (the page paginates client-side; only the latest 15 entries were machine-read;
   older entries were searched via web search only). No Custom UI load-time improvement entry was found.
9. `https://developer.atlassian.com/platform/forge/storage-reference/storage-cache-api/` and
   `/monitor-cache-metrics/` both 301 to `/platform/forge/changelog/#CHANGE-2761`; what that entry says was not found.
10. Exact CDN transfer sizes (CloudFront serves gzip without content-length; gzip -9 numbers are local estimates).
11. The developer status page component "Forge CDN (Custom UI)" — seen only through a third-party mirror.
12. Cold-start magnitude for Forge functions — docs only say metrics exclude it.

---------------------------------------------------------------------------------------------------------------

## Verification (independent fact-check, 2026-10-09)

Method. A separate checker re-fetched every source itself and never used this file's `raw-*` copies. That covered
the developer.atlassian.com pages (curl, text extraction, whitespace-normalised exact match), the @forge/* files on
unpkg at the cited versions and also at the newest `next` prereleases (bridge 7.1.1-next.7, csp 6.3.2-next.0,
manifest 13.7.0-next.7, cli-shared 9.8.0-next.13; every cited file is byte-identical), npm registry metadata, the
tarballs of @forge/cli 14.1.0, @forge/tunnel 7.2.0 and @forge/manifest 13.6.0, both production trace.zip files
re-extracted from the SSD, the live global-bridge.js, GitHub raw/API (forge-skills, Lighthouse), Chromium gitiles,
the ChromeDevTools/devtools-protocol master JSON (rolled 2026-10-08, which is the source the CDP docs site renders),
the AtlasCamp page and the community thread JSON. Scratch: `../verify-bootspeed/`.

Result: all 39 quotes appear verbatim at their URLs. 38 claims are confirmed and 1 is refuted as stated (#5).
Fifteen of the confirmed claims carry a correction or caveat that changes how the design should use them.
Those are marked ⚠ below.

| # | verdict | what the checker saw / correction |
|---|---|---|
| 1 | confirmed | Quote present (Custom UI page, May 21 2026); "Custom UI apps are hosted by Atlassian" is also on the page. |
| 2 | confirmed ⚠ | Both traces show the two platform scripts first in `<head>`, neither async; body = content-length = 394 B. Three corrections. (a) The 394-B document is resource `main` of a DIFFERENT app on the same page (app 31da19ab…, egress apps.stagil.com), not Sentinel. Sentinel's own `steward-console`/`doc-ribbon` (deployed Aug 27) serve 417 B with the identical injection, and the same holds for a third vendor's app, so the evidence is broader than stated. (b) The CLI's own deploy packager (`@forge/cli@14.1.0 out/service/resource-packaging-service.js` `processFile`) writes the resizer WITH `async` (every sampled cli-shared from 8.13.0, Dec 2025, to 9.7.0). The served HTML lacks it, so production re-processes index.html after upload. "Stored in S3 at deploy time" stays an inference: the ETags are not MD5s of the bodies, so the stored bytes cannot be proven. (c) The 1.0 kit installs `__bridge` with Playwright `addInitScript` (bridge-host.cjs:330), not with a parser-blocking tag. That is fine because platform scripts are excluded from budgets. |
| 3 | confirmed ⚠ | `head.prepend(this.createBridgeCoreScriptTag());` present. Correction: the `async` resizer tag is not only the tunnel's. The same `IframeResizerScriptService` is used by the deploy packager. Model the SERVED form: bridge first, resizer second, both synchronous. |
| 4 | confirmed | `getCallBridge` throws "Unable to establish a connection with the Custom UI bridge."; invoke.js calls it at top level; `out/index.js` re-exports every submodule eagerly. global-bridge.js sets `void 0===o.__bridge&&(o.__bridge=i)` synchronously at evaluation. |
| 5 | **refuted as stated** | The quote is real but sits under "Default limitations — By default, the CSP … restricts some behavior". The same page continues: "we're providing a way to add permissions to share data with external resources, as well as to use custom Content Security Policies", and `permissions.external.{scripts,styles,fonts,images,fetch.client,…}` exist. CogniRunner's production CSP shows declared connect-src openai/anthropic/openrouter. Even the DEFAULT served img-src allows `data: blob:` and third-party hosts (secure.gravatar.com, images.unsplash.com, Atlassian avatar/media hosts), and style-src allows the site host. ⇒ "no external origins" must be a STATED benchmark/RoA rule, and B11's "platform origins" allow-list must include the default CSP hosts (Jira avatar URLs point at them). |
| 6 | confirmed ⚠ | Quote present (Apr 8 2026). The manifest permissions reference (Sep 16 2026) lists the content.scripts sources unsafe-inline/unsafe-hashes/unsafe-eval/blob:/sha256-384-512 hashes and content.styles unsafe-inline only. Adding any unsafe-* CSP may force a major version upgrade. Caveat on the CSS-in-JS inference: it holds for `<style>`-element injection (emotion, styled-components, style-loader). MDN style-src says JS writes to `element.style` are not blocked and no browser blocks CSSOM `insertRule`, so React `style={{…}}` props need no declaration. |
| 7 | confirmed ⚠ | Quote present in the served header. Correction: script-src and font-src are `'self'` + the platform CDN (script) + four Atlassian object-store hosts (tdp-os…/fos-eap\|fos/app\|fos/cdn/download/, object-store.atlassian.com/os/ecosystem/installation/), not "'self' only". style-src also lists the site host and forge.cdn. The declared parts merge (Sentinel: style-src 'unsafe-inline'; CogniRunner: connect-src openai…). x-amz-meta-csp present in both traces. |
| 8 | confirmed (inference) | Code line present. x-amz-meta-csp key order = `getCspDetails` order (style, script, img, media, connect, font, frame). Still not observed with an inline script: the 51 KB CogniRunner index.html has no `<script>` without src, so the auto-hash remains unproven in production. |
| 9 | confirmed | Quote present (Jul 14 2026). "External resource domains" links to permissions#external-permissions. Addition: analytics egress "must not include in-scope End-User Data". |
| 10 | confirmed | Quote and table (bundle count 50; total files 25000; total bundle size 1 GB) present (Jul 7 2026). |
| 11 | confirmed ⚠ | Quote present. "The only" is too strong. It is the only docs sentence found that names "load times", but other pages carry boot-relevant performance guidance: an extra ADF-renderer iframe "will affect performance" (bridge view), bulk calls instead of many (optimise-costs), client-SDK flag evaluation "does not require a resolver round-trip" and initialize "once at app startup" (feature flags SDK). |
| 12 | confirmed ⚠ | Quotes present (Sep 24 2026). The custom-ui page links "Resources - Multiple entry points (Preview)". manifest 13.6.0 schema: `entry` maxProperties 50, values must be bare filenames. Validator checks missing map/key/file, `..`, and HTML-vs-UI-Kit type. RISK: the CLI deploy packager injects the bridge only into the root `index.html` (`processFile`: `filePath === 'index.html'`). Whether production injects into named Custom UI entries (e.g. `admin.html`) was NOT observed. Prove it on wolfaenpak before a design depends on Custom UI multi-entry. |
| 13 | confirmed | Stale page (Dec 1 2025) still says 10. `MAX_RESOURCE_COUNT = 50` with "document exceeds ${limit} resources". The schema's resources array has no maxItems. Unchanged in 13.7.0-next.7. |
| 14 | confirmed | 500KB / 5MB rows present ("for example, invoke and invokeRemote via @forge/bridge"); runtime 25 s (Sep 1 2026). |
| 15 | confirmed | Rows present, "applied on a fixed one-minute window". RFC-130 (closed May 22 2026) produced these values. Staff called the limits "soft limits, not hard limits" that Atlassian can raise via a break-glass procedure. No later changelog change found (entries Sep 26–Oct 9 read). |
| 16 | confirmed | Quote and the code example are present. The 429 arrives as a THROWN error: `error.statusCode ?? error.status` = 429, `error.metadata?.rateLimitProperties?.rateLimitReset` in epoch SECONDS. The bridge invoke reference (Jun 19 2026) documents `{body, metadata:{rateLimitProperties:{rateLimitValue, rateLimitRemaining, rateLimitReset}}}`. The doc example retries up to 3 attempts. "Exactly 1" is a benchmark rule and must be stated. |
| 17 | confirmed ⚠ | invoke: `withRateLimiter(_invoke, 500, 1000 * 25, …)` at module level; utils throws `BridgeAPIError` at numOps ≥ maxOps. It is a fixed window anchored at module load, per JS realm (per frame). Feature flags: module-level limiter, effective. requestJira (`fetch.js`) and getContext have none. WRONG part: `invokeRemote`/`invokeService` call `_invokeEndpointFn(...)` INSIDE each call, which creates a fresh limiter per call, so the 500/25 s check never trips for remote/container invokes. |
| 18 | confirmed | `await callBridge('getContext')` with no cache; the docs warning is present. global-bridge.js has no getContext special case. Every `callBridge` first awaits its `onBridgeReady` handshake, then `adapter.request(...)` to the host. |
| 19 | confirmed ⚠ | Doc comment and "This will fetch the current active theme from the host environment" present (bridge view, Sep 29 2026). Both traces load exactly the 5 token CSS files. Live global-bridge.js builds `atlaskit-tokens_<t>.css` links. Caveat: the set follows theme state (light/dark) and platform flags (`platform-dst-shape/motion-theme-default` sit in the iframe URL), so exclude the token CSS by URL pattern, not by count. |
| 20 | confirmed | Quote present (Aug 21 2026). Addition from the same page: frontends "are not subject to function invocation costs or limits". REST calls still fall under Atlassian app (points-based) limits per RFC-130. |
| 21 | confirmed | Quote present. |
| 22 | confirmed | Quote present. |
| 23 | confirmed | Quote present. Nuance: bridge 7.1.0 object-store `upload`/`download` still need a resolver `functionKey` to mint presigned URLs, after which bytes move browser↔object store directly. |
| 24 | confirmed | Exact sentence on the session page (Jun 23 2026; speakers and titles as stated). It is a session abstract, not docs. The React figures reproduce exactly but are the UMD builds. The CJS builds a bundler ships: react-dom prod 42,303 B / dev 229,990 B (gzip -9). |
| 25 | confirmed | Under "Lightweight Cost Signals" (a cost framing). The file last changed 2026-09-08; repo HEAD 2026-10-01; owner = the atlassian org. |
| 26 | confirmed ⚠ | Present in "Step 4: Optimize API and Data Fetching" of a COST skill (added 2026-06-09). "About 5–10 concurrent requests" belongs to the next bullet ("Unbounded concurrency"), not to the Promise.all bullet. |
| 27 | confirmed | Present, same section. |
| 28 | confirmed | Present under "Lightweight Debuggability Signals — Only flag readiness gaps". |
| 29 | confirmed ⚠ | Both quotes present (limitations Mar 25 2026; client SDK Sep 2 2026). The limitations page also says flag calls "are subject to standard Forge API rate limits". how_to_test FIX: `checkFlag` THROWS "FeatureFlags not initialized. Call initialize() first." before init (bridge 7.1.0 featureFlags.js). While the init response is held, the shell must use the app's own placeholder or default WITHOUT calling checkFlag. `defaultValue` only covers unknown flags after init. |
| 30 | confirmed | HeyJoe, Atlassian Staff and moderator, 2026-05-11T12:08Z; quote verbatim. |
| 31 | confirmed ⚠ | Quote present. Correction: the docs say it "notifies Confluence that a Forge macro has completed loading", not bodied macros only. It is Confluence-specific; the bridge emits `EXTENSION_READY` plus `callBridge('emitReadyEvent')` (static macros). There is no Jira equivalent. |
| 32 | confirmed ⚠ | Both quotes present (Jul 13 2026). The developer console's app metrics (Aug 24 2026) are invocation, API and container (EAP) only. NEW: live global-bridge.js (Sep 28 2026) contains flag-gated INTERNAL iframe telemetry: a navigation-timing and resource-timing PerformanceObserver (`callBridge("iframe-analytics")`), and `ufo-event`s including `ufo-forge-dom-mutations` (a whole-document MutationObserver with a 300 ms idle timer). Both production traces' iframe URLs carried `forge-ui-iframe-analytics` and `forge-ui-iframe-ufo-perf-observers`. So Atlassian measures Custom UI boot internally, but exposes nothing to developers. |
| 33 | confirmed | Both quotes present (Jun 25 / Jun 27 2026), plus "Additional metric types may be added in future releases". |
| 34 | confirmed | Quote present (May 15 2026). AEC (Oct 1 2026) reliability = incident process, Statuspage and support SLAs, with no performance number. The Marketplace changelog confirms CFA submissions closed Sept 1. |
| 35 | confirmed | Sentence present (as "**DO NOT** collect…") plus the table's "Client resource contention: High … DevTools Throttling: NO MITIGATION". The doc is unchanged since 2023-04-18 and still current. |
| 36 | confirmed | Both quotes present (throttling.md; Emulation.setCPUThrottlingRate.rate in protocol JSON). |
| 37 | confirmed ⚠ | time.h comment verbatim; `Performance.enable` `timeDomain` enum [timeTicks, threadTicks] (setTimeDomain deprecated). Caveat: on a heterogeneous P/E-core host (the Apple-silicon workhorse), core placement under load can move thread CPU time materially. Measure the golden-vs-golden spread under load before freezing "≤ 3×". |
| 38 | confirmed ⚠ | `callCount` text verbatim. startPreciseCoverage: "Coverage data for JavaScript executed before enabling … may be incomplete" — so it must be armed before the frame's first script (OOPIF: on the frame target). Caveat: counts are NOT fully load-independent. Timer- and rAF-driven code and React's scheduler (yields on a wall-clock frame budget) add iterations as wall time stretches, so use a ratio, never equality. |
| 39 | confirmed ⚠ | `Network.Initiator` "Information about the request initiator." and loadingFinished.encodedDataLength verbatim. Caveat for B6: `stack` is "set for Script only. Requires the Debugger domain to be enabled" (module imports carry `url`), so the grader must enable Debugger on the frame's session to attribute script-initiated requests. Prefer gzip -9 of the served body for B4. encodedDataLength is wire bytes, which (checker's understanding, not re-verified) include response headers. |

Newer or contradicting material found:
- Forge changelog 7 Oct 2026, FRGE-2205: regional remote URLs in `permissions.external.fetch.client` had been blocked by
  the CSP; this is fixed, and `remoteInstallationRegion` was added to getAppContext. That is a CSP connect-src change,
  not boot-relevant to an RoA app.
- Forge changelog 6 Oct 2026: Confluence static macros moved to Preview, "eliminating iframe latency". 28 Sep 2026:
  manual packaging (`bundler: manual@2026`) moved to Preview for functions and UI Kit resources.
- global-bridge.js internal perf telemetry (#32 above).
- Ineffective remote/container client limiter (#17), `checkFlag` throwing before init (#29), and deploy-packager
  injection only into root index.html with the resizer async (#2/#3/#12).

Provenance errors in this file (above): `@forge/bridge@7.1.0` was published 2026-09-28T02:44:54Z, not 2026-10-09
(that timestamp is 7.1.1-next.7). The §1.2 "Sentinel/doc app … 394 bytes" document belongs to app 31da19ab
(resource `main`). The emitReadyEvent "(bodied macros only)" note in §1.8 is wrong (#31). §1.7's "about 5–10
concurrent requests" belongs to the "Unbounded concurrency" bullet.
