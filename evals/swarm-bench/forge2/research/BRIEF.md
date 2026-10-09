# Forge 2.0 — verified research brief for the design panel (2026-10-09)

Owner mandate (verbatim, abridged): "design the toughest possible forge benchmark 2.0 … combines the latest modules
existing with forge … complex backend resolvers that invite both security concepts which matter in forge … stability and
robustness … speed in booting up for the custom UI, usage of UI kit 2 for the administrative panel … a front facing, an
admin panel … a ton of calls and it should know also to dose itself for tier 1 usage … good forge LLM usage."

## How to read this brief

- **Facts here are only claims an independent verifier marked `confirmed`.** Where the verifier corrected the wording,
  the CORRECTED wording is used. Refuted, outdated and unverifiable claims are in §10 with the reason. Do not design on
  them.
- Tags point at the verification tables in the topic notes (`research/<topic>.md`, section "Verification"):
  `tiers#N`, `uikit#N`, `boot#N`, `sec#N`, `llm#N`, `rob#N`. `[B]` = verified in THIS pass (2026-10-09 ~20:40 local):
  200 live changelog entries from `dac-changelogs.services.atlassian.com`, 30 doc pages fetched live, 8 community
  threads via the Discourse JSON API, the public ECO-1403 entry, the `@forge/bridge` 7.1.0 limiter source and the
  `@forge/api` 8.2.0 `safeUrl.js`. Working files: `forge2/verify-brief/`.
- Tally of the six topic verifications: 292 claims; 285 confirmed (many with a correction), 4 refuted, 3 outdated.
  This pass added the module statuses of §1, which the earlier platform research (`forge2/research-2026-10-09.md`) had
  not had verified.
- Grading law inherited from Forge 1.0 (DESIGN §3, §17.3–17.4): every graded behaviour is either STATED in the
  contract or a DOCUMENTED platform rule the emulator enforces exactly. Anything undocumented, contradictory or
  unpublished is stated by the contract or not graded. A documented rule may be a deliberate knowledge trap (1.0's
  `temperature`+`top_p` precedent).

---------------------------------------------------------------------------------------------------------------------

## 0. The fifteen facts the design rests on

1. **Tier 1 = Global Pool.** One 65,000-point hourly quota per app, shared by every tenant the app is installed on.
   It is the default, and it resets at the top of each UTC hour with no carry-over. Exhaustion means every request
   gets 429 until reset ("no gradual throttling"). Staff claim spike tolerance with unpublished thresholds, and a
   partner measured a hard wall 5–6 times. Emulate a deterministic hard wall and STATE it. (tiers#1,2,4,7,8)
2. **Points.** A request costs a base 1 point plus a per-object cost: 1 for core and uncategorised objects, 2 for
   identity and access objects. Writes cost the base only. Field data deviates: `search/jql` is charged 1 + results,
   and some identity and permission lists are charged a flat 1. The emulator publishes ITS OWN per-endpoint cost table
   and does not call it production truth. (tiers#5,6,30)
3. **Three Jira limit systems run at once.** (a) The hourly quota. (b) Per-tenant, per-endpoint token buckets: steady
   state GET/POST 100 and PUT/DELETE 50 rps, capacity unpublished, shared by every app and API token on the tenant
   (CHANGE-2753). (c) Per-issue writes: 20 per 2 s and 100 per 30 s. (tiers#9,10,12,13)
4. **Apps cannot see their consumption below ~80%.** `RateLimit` carries `r` only past about 80% usage. Forge storage
   is per installation, so no platform cross-tenant counter exists, and dosing needs self-accounting. (tiers#17,35)
5. **Frontend `@forge/bridge` `requestJira` costs no points.** Staff said so twice (2026-01-31 and 2026-04-01); the
   public docs say nothing. Since FRGE-1923 (fixed 2026-04-09), rate-limit headers DO reach the frontend. Backend
   `@forge/api` calls are charged. (tiers#24,25; #46 outdated)
6. **Forge platform limits are a separate system.**
   - Invocations: 1,200 per user per minute; 7,000 per minute and 300 per second per installation (fixed one-minute
     window).
   - `@forge/bridge` throws after 500 `invoke()` calls in 25 s per frame.
   - Runtime: resolver 25 s; consumer and scheduled 55 s by default, up to 900 s; web trigger, action and
     agentConnector 55 s; one outbound request in a long function 180 s.
   (tiers#31,32,33; rob#5)
7. **No platform authorization inside a surface.** Any user who can load a surface can invoke every resolver attached
   to it. Display conditions run client-side, and Forge has no app roles. Admin authorization therefore lives in
   resolvers, from `context.accountId` plus a live permission check. (sec#4,5,9)
8. **asUser by default; verify before asApp.** Marketplace requirement 1 makes asUser the default and requires a
   permission check before any asApp call. Atlassian's own review rule binds that check to the same resource id the
   asApp call uses. (sec#6,7)
9. **Web triggers are unauthenticated by default.** Platform HMAC exists in the tooling (`@forge/api` 8.1.0+, manifest
   schema 13.6.0) but is undocumented, and its enforcement is unverified. Require an app-implemented scheme, stated in
   full. (sec#15,17)
10. **Async events** are delivered at least once, in no guaranteed order.
    - They are retried within a 24 h retention window, which only platform-level errors extend (to 96 h).
    - They are dropped when app-level errors exhaust the window.
    - A retry is requested by RETURNING an `InvocationError` (`retryAfter` ≤ 900 s).
    - `jobId` is minted per push, so it is not an idempotency key.
    (rob#20–27,30)
11. **Warm processes MAY be reused across tenants.** Unawaited work may run later inside another tenant's invocation,
    or never. (rob#11,12; tiers#36)
12. **Custom UI boot.** Custom UI runs in an iframe, with two platform scripts injected synchronously before the app's.
    `@forge/bridge` binds at module load. Atlassian publishes no numeric boot requirement, and measures iframe
    performance only internally. Every boot budget is therefore a benchmark rule, and it must be count-based, not
    time-based. (boot#2,4,32,35)
13. **UI Kit 2** means `@forge/react` ≥ 10 with `render: native`.
    - The real reconciler sends the whole ForgeDoc tree over `callBridge('reconcile')` on every commit.
    - The product host renderer is Atlassian-internal.
    - An offline host is feasible but home-grown (§5).
    - The closure resolves to about 144 MB, not 1.2 GB.
    (uikit#2,4,13,33)
14. **Forge LLM** has been GA since 2026-07-29/30, and offers `chat`/`stream`/`list` only, with no structured
    outputs.
    - `temperature` and `top_p` together are rejected on every model; either one alone is rejected on
      opus-4-7/4-8/5 and sonnet-5.
    - Per installation: 100 requests per minute across all models, and 500,000 tokens per minute PER MODEL.
    - There is no free allowance; usage is billed to the developer.
    - The 5-minute inference window applies only in consumers with a timeout of at least 300 s.
    (llm#1–3,10,16,17,20–23,29)
15. **No Atlassian prompt-injection guidance exists for the `chat()`/`stream()` path.** That rule must be derived
    (asUser / verify-before-asApp, Marketplace requirement 13 for Rovo actions) and STATED. (llm#39; sec#27)

---------------------------------------------------------------------------------------------------------------------

## 1. Latest modules to combine

### 1.1 Status on 2026-10-09 (`[B]` = verified live this pass)

| capability | status | evidence | notes for 2.0 |
|---|---|---|---|
| Custom UI multi-entry resources (`entry`) | **GA** | CHANGE-3337 (2026-07-08): "now generally available in Jira and Confluence"; the resources page lists `entry` with no Preview label [B]. The Custom UI page (May 21) still links "Multiple entry points (Preview)", which is stale | ≤ 50 entries per resource (boot#12). RISK: the CLI deploy packager injects the bridge only into the root `index.html`; injection into named Custom UI entries (e.g. `admin.html`) was never observed. Prove it on wolfaenpak before demanding it |
| `jira:globalPage` | GA | "You can only register a single `jira:globalPage` module per app … deployment will fail" [B] | one per app |
| `jira:projectPage`, `jira:issuePanel` | GA | module pages, no Preview label [B] | |
| `global:fullPage` | **Preview** | title "Global full page (Preview)" [B]; `routePrefix` mandatory; `render` "[Mandatory for UI Kit only]" (uikit#21) | `jira:fullPage`/`confluence:fullPage` "being deprecated on September 30, 2026" (uikit#21; CHANGE-3380). Deprecated ≠ removed: schema 13.6.0 still accepts them |
| `jira:adminPage` with `render: native` | GA | uikit#18, #20 | subpages are Custom UI only; a `useAsConfig`/`useAsGetStarted` entry cannot have `pages`/`sections`; displayConditions on pages/sections (CHANGE-3458) only hide |
| UI Kit `Router` | **Preview** | title "Router (Preview)" [B]; Jira admin page supported (uikit#20) | |
| `dashboards:widget` / `dashboards:backgroundScript` | GA | CHANGE-3453 (2026-09-22); "The `edit` entry point is currently required" [B] | legacy `jira:dashboardGadget`/`dashboardBackgroundScript` removed May 17, 2027 (CHANGE-3454) |
| `dashboard:filters` | EAP | CHANGE-3485 (sign-up; docs only after enrolment) | never demand |
| async events, `scheduledTrigger`, product `trigger`, `webtrigger` | GA | rob, tiers | |
| web-trigger HMAC (`request.authentication: hmacSharedSecret`) | shipped in tooling, undocumented | sec#17; the module page still warns there is "no built-in authentication" and suggests checking an `Authorization` header [B] | do not require it; do not penalise it |
| KVS + Custom Entity Store (transactions, batch, `keyPolicy`) | GA | rob#43–59 | |
| Forge SQL | **GA** | page "Forge SQL" with no Preview banner; "Foreign keys are not supported"; "Each SQL statement can only contain a single query"; adding SQL = major version; `SQL_POLICY_VIOLATION`; "Dates will be returned as strings" [B]; limits rob#60 | emulation needs a MySQL-compatible engine. RFC-148 local emulators exist only as an RFC (CHANGE-3469); their contents and licence are unverified |
| Realtime | GA | CHANGE-3326 (2026-06-29): publish-only and subscribe-only tokens; 50 ops/s per installation enforced | page note: `subscribe()` is scoped by default to the same module context [B] |
| Forge LLM (`llm` module, `@forge/llm` 1.0.7) | GA | llm#1 | one `llm` module per app; adding it = major version [B page warning] |
| Rovo `action`, `rovo:agent` | GA | action page, no Preview label [B]; `rovo:agent` carries no Preview marker in the Rovo module index (llm note §8), but its page was not re-fetched | |
| `rovo:skill` | **Preview** | CHANGE-3499 (2026-10-02); page "Rovo Skill (Preview)" [B]; llm#47 | |
| `rovo:mcp` | **Preview** | CHANGE-3400 (2026-08-14, Rovo Studio); CHANGE-3495 (2026-10-01): external clients "now available in Preview"; page heading "Rovo MCP (Preview)", but the page text calls third-party clients EAP [B] | ≤ 1 per app; ≤ 50 tools |
| `rovo:agentConnector` | GA | CHANGE-3484 (2026-09-28), A2A 1.0 only | needs an external agent, so it is not offline-friendly |
| App REST APIs (`apiRoute`) | **Preview** | title "Forge app REST APIs (Preview)"; "For each site, app REST APIs are disabled by default" [B] | request-object shape undocumented (earlier research, not re-verified) |
| Rolling releases / `permissions.enforcement: app-managed` | **Preview** | CHANGE-3315; "Atlassian will not roll back"; "You must extract the granted property" [B] | |
| Object Store | **Preview** | page tip "now in Preview"; adding it = major version [B] | needs an emulated pre-signed-URL host |
| `jira:customField` / `customFieldType` | GA | no Preview label; "The order of values returned must be the same as the order of issues received"; "asUser isn't supported"; "constant performance regardless of the number of issues"; validation does not run "when the app updates the value directly" [B] | page examples use `render: native`; staff say custom-field view renderers are UI Kit-only for performance (boot#30) |
| `jira:jqlFunction` | GA | no Preview label; architecture page: "Precomputations are not scoped to users", 7-day expiry, "1,000 right-hand side values", "time limit of 25 seconds" [B] | contract now captured (Forge 1.0 D3/§16 no longer applies) |
| workflow validator / condition / post function | **Preview** | titles "(Preview)"; "configuration with UI Kit is currently not supported" [B] | expression forms need a Jira-expression evaluator |
| `jira:command` (command palette) | **Preview** | [B] | |
| Confluence static macros | **Preview** | CHANGE-3505 (2026-10-06) | a second mock product |
| manual packaging (`bundler: manual@2026`) | **Preview** | CHANGE-3466; TypeScript bundler EAP removed (CHANGE-3470) | lint skips the UI Kit directory and `.html` rules when it is set (uikit#19, #32) |
| Forge Containers | Developer Preview | CHANGE-3357 (2026-08-03); the older CHANGE-3358 says "remains in EAP" | needs a container runtime, so it is out |
| Node runtime | `nodejs22.x` / `nodejs24.x` | "This runtime supports Node.js 22 and Node.js 24"; Node 20 EOL; CHANGE-3209 progressively blocks `nodejs20.x` deploys; the manifest reference still lists `nodejs20.x` [B] | lint trap |
| `invoke` rate-limit metadata | GA | CHANGE-3314 (`{ rateLimitProperties: true }` → `{ body, metadata }`) | |
| frontend custom metrics | counters only, 20 per app | CHANGE-3323; boot#33 | |
| Feature-flags client SDK | (status not checked) | anonymous users unsupported from Dec 1, 2026 (CHANGE-3416); `checkFlag` throws before `initialize()` (boot#29) | |
| asApp permission checks in Jira | rolling out | CHANGE-3445: removes the requirement for permissions to be synced at installation time; tenant-by-tenant | may shorten the install race in production; the life-cycle doc still warns about it [B] |

### 1.2 Combination constraints (all verified)
- **One per app:** `jira:globalPage` [B], `llm` (llm#19), `rovo:mcp` (llm#50).
- **Scheduled triggers:** at most 5 per app, of which at most 1 is `fiveMinute` (tiers#39).
- **Resources:** at most 50 per app. The CLI validator says `MAX_RESOURCE_COUNT = 50`; a stale page still says 10
  (boot#13). Each resource takes at most 50 entries (boot#12).
- **Custom Entity Store:** 20 entities, 7 indexes and 50 attributes per entity; a named index's `range` takes exactly
  one attribute (rob#58). The real CLI refuses more at lint AND at deploy (Forge 1.0 DESIGN §17.8 E, measured on
  wolfaenpak).
- **Major version (needs admin approval)** when an app adds or changes any of these:
  - scopes;
  - egress or CSP entries, including any `unsafe-*` CSP (sec#24, boot#6);
  - the `llm` module [B];
  - Forge SQL [B];
  - `objectStore` [B];
  - its first dynamic web trigger (sec#18; versions page [B]).

  `forge deploy` now stops and asks for `--approve MAJOR_VERSION_RULE` (CHANGE-3376).
- **Upgrade event:** `avi:forge:upgraded:app` "is not sent for minor or patch version upgrades" [B].

### 1.3 Traps a strong model plausibly gets wrong
1. It declares two `jira:globalPage` modules, one front page and one other page, and the deploy fails.
2. It uses `jira:fullPage` (deprecated) or omits `routePrefix` on `global:fullPage`.
3. It uses `jira:dashboardGadget`, or a `dashboards:widget` with no `edit` entry.
4. It plans Custom UI-style sidebar subpages for a UI Kit admin page. Those are Custom UI only, so a UI Kit admin needs
   Tabs or Router for navigation.
5. It writes a JQL function whose result depends on the invoking user. Precomputations are "not scoped to users", so
   one user's result is served to all.
6. A CES index has more than one `range` attribute, which does not deploy.
7. It uses `nodejs20.x`, `@forge/ui` (UI Kit 1, dead since Feb 2025, uikit#2), or `@forge/api` `storage` (1.0 trap).
8. Its custom-field value function returns values out of issue order, uses asUser, or scales per issue.

### 1.4 How the offline grader can test it fairly
- **Static:** the pinned manifest schema (13.6.0) plus a kit lint that equals the CURRENT Forge CLI's rules. Forge 1.0
  defect E: the kit lint lacked the range-index rule the real CLI enforces. Then the deploy-readiness predictions:
  major-version triggers, the one-per-app modules.
- **Runtime:** each module's documented contract, driven by the emulator. Grade the outcome; never grade code shape.

### 1.5 What the contract must state
- The module keys and roles (WHAT). HOW stays discoverable offline (typings, schema, OpenAPI, dev site), per 1.0's D5.
- Whether Runs on Atlassian (RoA) eligibility is required. That decision drives static web triggers, no remotes,
  egress and CSP.
- Every deviation from production.
- That a Preview feature is demanded "as documented on <date>".

### 1.6 Emulator / mock work implied
- Reuse Forge 1.0's kit: real wrapper, proxy, Custom UI host, lint.
- New hosts:
  - the UI Kit host (§5);
  - a tier/points gateway (§6);
  - a multi-tenant warm-process runner (§2, §3);
  - web-trigger ingress with raw-body access (§2);
  - multi-entry resource serving, if demanded.
- Optional: a Jira-expression evaluator, if workflow rules are in.

---------------------------------------------------------------------------------------------------------------------

## 2. Complex backend resolvers + Forge security concepts

### 2.1 Verified facts
- **Resolver context** (sec#1–3; corrected).
  - app-context-security (Nov 20, 2025) calls resolver context safe for authorization. forge-resolver (Nov 8, 2024)
    says "Not all of the values in the context parameter are guaranteed…".
  - Staff RFC-138 (t/101344) admits the field list is undetermined. It was promised for Oct 2026 and is absent today.
  - In the code, `@forge/resolver` 2.0.0 overwrites `accountId`, `installContext`, `license`, `installation` and
    `jobId` from the runtime principal; `extension.*` passes through.
  - So: rely on `context.accountId`, and never on payload identity or roles. Do not treat `extension.*` ids as proof of
    permission.
  - Frontend context (`view.getContext`, `useProductContext`) can be tampered with. (sec#2)
- **Resolver reach and display conditions.**
  - "Every resolver handler attached to a module is invokable by any user who can load that app surface" (Atlassian
    GitHub rule, `atlassian/forge-skills` `missing-resolver-authz.mdc`; Atlassian-authored, not developer docs). (sec#4)
  - Display conditions "are executed on the client-side" and are not a security control
    (https://developer.atlassian.com/platform/forge/manifest-reference/display-conditions/, Sep 30, 2026). (sec#5)
- **asUser and asApp.**
  - asUser is the default: shared responsibility model (Aug 1, 2026) and Marketplace requirement 1, implementation
    detail 1 (https://developer.atlassian.com/platform/marketplace/security-requirements/, revised Feb 19, 2026).
    (sec#6)
  - Before asApp, verify permissions via the permission REST APIs. The "same resource id" binding is from the
    forge-skills `asapp-privilege-escalation.mdc` checklist. (sec#7)
  - "Apps must not rely on client-supplied data alone". (sec#8)
  - Shared responsibility: "Ensure that authorisation controls exist to segregate data access between different user
    roles within the same tenant". The manifest has no roles key, and the only per-user platform gate (user-based
    billing `userAccess`) is EAP and on/off only. App roles are therefore an APP feature: stored in Forge storage,
    administered in the admin panel, enforced in resolvers. (sec#9)
  - The asApp user usually has MORE privileges than the caller (staff t/66667), so returning asApp data without a
    per-user check leaks. But Confluence page restrictions DO bind the app user (staff t/93662, 2025; ECO-822 open), and
    admin app-access rules can block it. Worst-case asApp visibility is an emulator ASSUMPTION. (sec#10, outdated as
    worded)
  - For unlicensed and anonymous users, asApp is more privileged than the caller; anonymous `accountId` is the string
    `"unidentified"`
    (https://developer.atlassian.com/platform/forge/access-to-forge-apps-for-unlicensed-users/, Jul 27, 2026). (sec#11)
- **`authorize()`** (@forge/api 8.2.0 + @forge/auth 1.0.0 source). (sec#12)
  - It wraps POST `/rest/api/3/permissions/check` and the Confluence content permission check, both asUser for the
    runtime principal.
  - It throws outside user-invoked modules.
  - It never sends `globalPermissions`, so there is no ADMINISTER helper.
  - `onJiraIssue([]).canEdit()` resolves TRUE.
  - There is no BROWSE helper; use `onJira([{permissions:['BROWSE_PROJECTS'], issues}])`.
- **Bulk permissions.** One call checks up to 1000 issues and 1000 projects. An asUser JQL search is already BROWSE-
  and issue-security-filtered. (sec#13)
- **`mypermissions` grants are not issue-level grants**: security levels and reporter-only grants differ. (sec#14)
- **Recognising a Jira admin server-side** (documented building blocks): `GET /rest/api/3/mypermissions?permissions=ADMINISTER`
  asUser, or `POST /rest/api/3/permissions/check` with `globalPermissions: ["ADMINISTER"]`
  (https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-permissions/) — security note §2 (doc
  quotes; not a separate verifier claim, but the same page sec#13/#14 verified).
- **Web triggers.**
  - No platform authentication by default; `asUser` does not work in them. (sec#15)
  - Replay protection, bounded retries and circuit breakers are documented developer duties (shared responsibility,
    "DoS protection"). (sec#16)
  - RFC-141 was resolved on 25 Aug 2026 and remains undocumented. The tooling shipped: `@forge/api` 8.1.0+
    `secretKeyConfig`, the CLI `--noKeyExpiry`, and the manifest 13.6.0 field
    `request.authentication: hmacSharedSecret|none`. Enforcement is unverified.
  - The RFC body says `x-webtrigger-signature` with a `sha256=` prefix, plus an optional `x-webtrigger-timestamp`
    signed as `<timestamp>.<body>`, ±10 minutes. Its feature table says `x-hub-signature`, so the RFC contradicts
    itself. (sec#17)
  - Only STATIC web triggers qualify for RoA; triggers are dynamic by default; a static trigger returns `{outputKey}`.
    (sec#18)
- **Egress and RoA.**
  - Undeclared egress is rejected, except under the Preview customer-managed egress (`external.configurable.enabled`).
    (sec#19)
  - RoA excludes non-analytics external domains, Remotes, Connect modules, Providers and dynamic web triggers, and
    needs residency-enabled storage (https://developer.atlassian.com/platform/forge/runs-on-atlassian-apps/). (sec#20)
- **Secrets.**
  - Encrypted environment variables are hidden from the CLI only; functions get them as plaintext, and returning them
    to the frontend exposes them. (sec#21)
  - Runtime-entered secrets go to `kvs.setSecret`, readable only with `getSecret` and not queryable; AGC marks a plain
    `kvs.set` of a key as incorrect. (sec#22)
  - Marketplace requirement 5: no secrets in source, URLs, Referer headers or logs. (sec#23)
- **CSP.** Restrictive by default; `unsafe-*` is an opt-in and may force a major version. Marketplace requirement 9:
  avoid `unsafe-inline`/`unsafe-eval` in `script-src`. (sec#24)
- **AI security.**
  - Marketplace requirement 13: Rovo action inputs are untrusted except context parameters; admin-level agent actions
    need the app's own permission checks. (sec#25)
  - Requirement 14: `actionVerb` must be accurate. (llm#42)
  - Action inputs must never drive authorization; read `accountId` from context. (sec#26)
- **Forge LLM.** Requests are moderated against the AUP, which is not an injection defence. No Forge-LLM injection or
  permission-filter guidance exists. (sec#27)
- **Tenancy.**
  - Module-level state is shared across tenants on warm reuse; key caches by `cloudId`/`installationId`, never by
    issue key (https://developer.atlassian.com/platform/forge/tenant-data-isolation/, Aug 1, 2026).
  - TRAP inside the doc: its own "SAFE" examples cache the asUser `/myself` result per `cloudId`. That is tenant-safe
    but serves one user's data to the next user, so grade per-USER scoping too. (sec#29)
- **Personal data.** Report via the Personal data reporting API: up to 90 accounts per request, 7-day cycle, `closed`
  or `updated`, 429 with Retry-After, test ids Active `5be24ad8b1653240376955d2` and Closed `5be24ba3f91c106033269289`.
  (sec#30)
  - `privacy.reportPersonalData` fires ALL 90-account batches concurrently (`Promise.all`) and rejects on any non-200/204
    while the others keep running. That is a burst and Tier 1 hazard. (sec#31)
- **Abuse prevention.** "Ensure your app does not exceed the Forge platform quotas and limits" (shared
  responsibility). (sec#32)

### 2.2 Traps a strong model plausibly gets wrong
1. It hides admin actions behind `displayConditions`/`isAdmin` and never checks in the resolver. Atlassian's own
   FSRT "damn vulnerable" app does exactly this.
2. It reads `accountId`, `isAdmin` or `role` from the `invoke` payload.
3. It checks permission on project P and then acts asApp on issue I (wrong resource), or substitutes a global role
   check for an object-level one.
4. It takes `authorize().onJiraIssue(ids)` with a possibly empty list, which grants. It uses `onJira` and believes it
   checks ADMINISTER. It calls `authorize()` from a trigger, where it throws.
5. It uses a project-level `mypermissions` grant as an issue-level grant.
6. It returns asApp search results or asApp-built LLM prompts to a narrower viewer.
7. Web trigger: compares the HMAC with `===` instead of a timing-safe compare; signs the parsed and re-serialised JSON
   instead of the raw body; reads headers case-sensitively or as strings rather than arrays; side-effects before
   verifying; keeps a nonce check without atomicity (`get` then `set` instead of `keyPolicy: 'FAIL_IF_EXISTS'`); trusts
   a KVS TTL for the replay window. Expired data is readable for up to 48 h (rob#47), so the app must check
   `expireTime`.
8. It stores an admin-entered API token with `kvs.set`, returns it from `getConfig`, or logs it.
9. It caches per tenant but not per user, or keys caches by issue key; it keeps tenant data in module scope.
10. It lets anonymous users share state through `accountId === 'unidentified'`.
11. It sends personal-data reports for thousands of accounts in one `reportPersonalData` call (a concurrent burst).
12. It writes a JQL function whose output depends on the caller (precomputations are cross-user, §1.1).
13. Realtime: it copies Atlassian's LLM+Realtime tutorial, which is exploitable. Client-supplied `customClaims` and
    `model` reach the consumer unchecked, so a caller can publish into another user's channel and spend tokens.
    Claims and model must be derived server-side. (llm "newer facts" 5)

### 2.3 How the offline grader can test it fairly (stimulus → observable; all depend only on stated text)
| id | test | pass |
|---|---|---|
| S1 | invoke every admin resolver key directly, as a non-admin, bypassing the UI | stated refusal shape; zero KVS/Jira diff |
| S2 | the payload carries another user's `accountId`, or `isAdmin`/`role` | ignored; the action is attributed to `context.accountId` |
| S3 | IDOR: the checked id differs from the sink id; ids the user cannot browse | refusal; no foreign data returned |
| S4 | a security-level issue visible only to asApp (stated emulator assumption) | the canary is absent from the response and from every LLM prompt |
| S5 | EDIT in the project but not on this issue (reporter-only / security level) | refusal |
| S6 | a module hidden by displayConditions; call its resolver | refusal |
| S7 | `grantRole(self,'admin')` as a non-admin; remove the last admin | refusal; invariant kept |
| S8 | web trigger: unsigned, bad signature, tampered body, stale timestamp, case-varied header | stated 4xx; zero side effects |
| S9 | the same signed request twice, sequentially and concurrently | exactly one side effect |
| S10 | static trigger if RoA is required | manifest `response.type: static` + outputs |
| S11–12 | egress proxy log; RoA manifest checks | no undeclared hosts, no `*`, no canary egress; no remotes/providers/Connect modules/dynamic triggers |
| S13 | a secret canary (env var + admin form) | stored via `setSecret`; never in responses, logs, egress or prompts |
| S14 | XSS payloads in issue text, comments, app records and mock-LLM output | no script execution (a canary global stays unset), no unsafe attributes or `javascript:` links, no `unsafe-inline` scripts |
| S15 | tenant A then tenant B in the SAME warm process | no A canary in B |
| S16 | two anonymous sessions | no shared per-user state |
| S17 | the scripted LLM "obeys" an injected tool call | no unauthorized mutation, egress or unlisted tool |
| S18 | Rovo action with a forged `accountId` or an invisible key | identity from context; permission re-checked |
| S19 | low-privilege subscriber on an admin channel | receives nothing |
| S20 | install: the first asApp calls return 403; reinstall: empty storage | retry via `InvocationError`; clean bootstrap |
| S21 | privacy reporting; the mock answers closed/updated/429 | erase or refresh; batches ≤ 90, paced; honours Retry-After |
| S22 | payload id `../../rest/api/3/...` | blocked: a `route` path parameter containing `..` (or an encoded variant), `/`, `\`, `?` or `#` throws "Disallowing path manipulation attempt" (@forge/api 8.2.0 `out/safeUrl.js`, read [B]); `assumeTrustedRoute()` bypasses it |
| S23 | SQL injection (if Forge SQL is used) | parameterised (`?` + `bindParams`) |

Count the permission-API calls the app makes, as proof that a live check happened.

### 2.4 What the contract must state
1. The role model: role names, who grants them, bootstrap admins (e.g. "Jira ADMINISTER holders are implicit app
   admins"), the admin-only resolver keys, and the refusal shape.
2. The emulator's asApp visibility (worst case: everything, including security levels; Confluence restrictions per
   production or stated otherwise).
3. Whether `context.extension` ids are platform-validated in the emulator (pick one).
4. That resolvers are callable directly by any user who can load the module.
5. The full web-trigger scheme: header names (pin `x-webtrigger-signature`, `sha256=`), the signing string, the
   encoding, the window, nonce semantics, status codes and response body. State that the platform `hmacSharedSecret`
   is neither required nor penalised.
6. RoA yes/no.
7. The secrets inventory, and "presence only" read-back.
8. Which fields are user-controlled, and the rendering rule (text vs ADF via `AdfRenderer`).
9. The LLM tool list, which tools mutate, the confirmation rule, the content-inclusion rule, and the budgets.
10. That multi-tenant warm reuse WILL be exercised.
11. The personal data stored, and its erase/refresh semantics.
12. The allowed TTL for cached permission verdicts. This is the security vs Tier 1 trade-off: one bulk
    `permissions/check` covers 1000 issues and 1000 projects.

### 2.5 Emulator / mock work implied
- A per-user permission model: project roles, issue security levels, reporter-only grants, ADMINISTER, and
  `permissions/check` + `mypermissions` (1.0 modelled the first two only).
- A resolver invocation API taking any `functionKey` and principal (admin, project admin, licensed, no-BROWSE,
  anonymous `unidentified`).
- A multi-tenant warm runner: one Node process, interleaved installations, the module cache retained.
- An egress proxy with an allow-list.
- A console/log sink with canary scans.
- A scripted LLM that captures every prompt.
- A mock `POST /app/report-accounts/`.
- KVS with TTL + 48 h lag, `FAIL_IF_EXISTS` → 409 `KEY_CONFLICT`, and the secret store.
- Web-trigger ingress that preserves the raw body and multi-value headers.
- A headless render for XSS canaries.

---------------------------------------------------------------------------------------------------------------------

## 3. Stability and robustness

### 3.1 Verified facts
- **Time.**
  - UI resolvers run 25 s (rob#1).
  - Consumer- and scheduled-only functions run 55 s by default, up to 900 s, via an integer `timeoutSeconds` 1–900.
    If a function serves several modules, the LOWEST timeout wins (rob#2,3).
  - Web trigger, action and agentConnector: 55 s (rob#4).
  - A single outbound fetch in a long function is cut at 180 s (rob#5).
  - `getAppContext().invocationRemainingTimeInMillis()` exists (rob#6). The SDK reads
    `global.__forge_runtime__.lambdaContext.getRemainingTimeInMillis` UNBOUND, and also destructures
    `runtime.appContext`, so an emulator must supply the whole object and a closure (rob#7).
- **Memory and payloads.**
  - Memory is 512 MB by default, configurable 128–1,024 MB; more memory means more CPU (rob#8).
  - Front-end invoke: 500 KB request, 5 MB response; backend payload 5 MB (rob#9).
  - No cold-start figure is published (rob#10).
- **Warm reuse and unawaited work.** Warm reuse across tenants "may" happen (rob#11). Unawaited promises and timers can
  be suspended and resumed in a later, possibly other-tenant, invocation, "or never"; use async events for
  post-response work (rob#12).
- **Invocation limits.** User-led: 1,200 per user per minute; 7,000 per minute AND 300 per second per installation, on
  a fixed window (rob#13; CHANGE-3420). Egress: 100 requests per runtime minute per invocation (excluding
  `requestJira`); 50,000 per minute per app; 3,000,000 network requests per minute per app; 100,000 per minute per app
  per tenant (rob#14).
- **Async push.**
  - Limits: 50 events per push, 200 KB per push (checked client-side), 500 events per minute (rob#15). 100 KB per
    event when the consumer timeout is above 55 s, with the enforcement point undocumented (rob#16).
  - `delayInSeconds` 0–900; the SDK accepts fractions (rob#17).
  - Cyclic limit: 1,000 push requests across all handlers descending from one invocation (rob#18).
  - SDK status mapping: 202 → `PartialSuccessError` whose `failedEvents` carry the original events; 429 →
    `RateLimitError`; 405 → `InvocationLimitReachedError`; 413 → `PayloadTooBigError` (rob#19).
  - `jobId` is minted client-side per `push()` (rob#20).
- **Async delivery.**
  - At least once within 24 h; extended to 96 h ONLY by platform degradation or platform-level errors (rob#21).
  - Exponential backoff with gaps up to about 15 min (rob#22).
  - Throw, timeout and OOM count as app-level errors; the event is dropped when the window expires through them
    (rob#23).
  - Retry = RETURN `InvocationError({retryAfter ≤ 900, retryReason, retryData ≤ 4 KB})` (rob#24). The SDK object is
    `{_retry:true, retryOptions}`, `retryAfter ≤ 0` becomes 1, and an omitted `retryAfter` stays undefined (rob#25).
  - `retryContext` exists only on retries; platform errors do not increment `retryCount`; `retentionWindow` is
    OPTIONAL (rob#26).
  - The "maximum of four retries" text applies only to @forge/events 1.x; v2+ (current 3.0.7) retries within the
    window (rob#27).
- **Concurrency keys.**
  - A key caps processing per installation across queues; without one, processing is unbounded (rob#28).
  - Staff: throttled events back off up to 15 min, there is no cap on waiting events, and they can expire (rob#29).
- **Ordering, jobs, versions.**
  - No ordering guarantee; FIFO (RFC-107) has not shipped (rob#30).
  - Jobs report `getStats()` → `{success, inProgress, failed}` with undefined semantics; `cancel()` stops unstarted
    events and retries, not running ones (rob#31).
  - Events pushed by version N may be processed by N+1 code (rob#32; CHANGE-2526).
- **Product events.**
  - Delivery may take up to 3 min (rob#33).
  - App-requested retries are capped at 4; timeout and OOM are platform-level there; `retryData` must be an object
    (ECO-734) (rob#34).
  - Without `ignoreSelf` the app gets its own `selfGenerated` events, which can loop (rob#35).
  - Only top-level deletes are emitted (rob#36).
  - The installed event may precede the app's permissions, so 401/403 must be retried. Four retries of ≤ 900 s each
    give about 60 min (rob#37).
- **Scheduled triggers.** At most 5, one `fiveMinute` (rob#38). A throw is NOT retried (rob#39). Duplicates happen,
  staff say "expect at least one" (rob#40). Return `{statusCode: 204}` to satisfy both doc pages (rob#41). Runs are
  distributed across installations through the interval (tiers#39).
- **Web-trigger responses.** URLs are public; `statusCode` is required (rob#42).
- **KVS and CES.**
  - `get` is strictly consistent; `query` is eventually consistent (rob#43).
  - Last write wins; writes are atomic per key; `keyPolicy` exists (rob#44). `FAIL_IF_EXISTS` → 409 `KEY_CONFLICT`
    (rob#45). `keyPolicy` is NOT available in batches or transactions, which take TTL only (rob#46).
  - TTL is at most 1 year; expired values stay readable for up to 48 h (rob#47).
  - Transactions: all or nothing, ≤ 25 operations, each key once, 4 MB, counted as one write (rob#48). Conditions
    exist only in CES transactions, failing with `CONDITIONAL_CHECK_FAILED` (rob#49).
  - **SDK trap:** `transact().set(key, value, entity?, options?)` reads TTL ONLY from the 4th argument, while both doc
    pages pass `ttl` in the 3rd, where plain JS drops it silently (rob#50).
  - Batches are best effort, parallel and unordered, ≤ 25 keys / 4 MB, with per-key failures (rob#51).
  - Throughput: 1,000 RPS; 4,000 read + 4,000 write 10 KB units per minute, sizes rounded up (rob#52); 1 MB/s writes
    per key, so one hot counter is a bottleneck (rob#53). Over-limit gives 429 `RATE_LIMIT_EXCEEDED` /
    `TOO_MANY_REQUESTS` (rob#54).
  - Sizes: value 240 KiB, depth 31, key 500 (rob#55).
  - Query returns 10 by default and 100 at most, with a single `beginsWith` (rob#56). Cursors are unstable and must
    not be persisted (rob#57).
  - CES schema caps (rob#58). The docs use the names `equalsTo`, `isGreaterThan` and `SortOrder`; the SDK exports
    `equalTo`, `greaterThan` and `Sort` (rob#59).
- **SQL:** 5/10/20 s per-connection timeouts, 150 DML RPS, 62.5 s of query time per minute (rob#60).
- **Logs:** 100 lines per runtime minute and 200 KB per invocation (rob#61); "avoid" logging secrets and personal data
  (rob#62).
- **Realtime:** 50 ops/s per installation shared by subscribe, publish and `signRealtimeToken`, including UI
  subscriptions; `publish` returns `errors[]` and does not throw (rob#63).
- **LLM:** 100 RPM across models, 500k TPM per model (rob#64). Truncated streams do not throw; detect a missing
  `finish_reason` (rob#65).
- **Documented anti-patterns** (verbatim on the optimise-costs page): polling storage from a resolver (use Realtime)
  and N+1 (rob#73,74). Concurrency used to dodge rate limits is an anti-pattern on the Jira page (rob#71).
- **Production pain, partner observations with verified quotes [B].**
  - t/101559 (2026-06-30): "On a real run, 429s turn a 3-issue batch into a 20-30 second invocation" and "Anything
    above 50,000 issues simply cannot complete within this limit" (the 1,000 cyclic pushes), plus "under heavy load I
    occasionally lose increments" (CAS on CES). A partner's reply: "you should be able to configure a 15min timeout".
  - Staff (t/96705, 2025-12-02): "there are no immediate plans to support schema / breaking changes for CES".

### 3.2 Traps a strong model plausibly gets wrong
1. A long job inside a resolver: killed at 25 s, or blocked waiting on an LLM call.
2. `timeoutSeconds` on a function shared with a 55 s module (lowest wins), or set on a resolver (no effect).
3. `setTimeout`/unawaited `push()` after returning. Atlassian's own Assets tutorial does not await its pushes.
4. Dedupe by `jobId` or `eventId` instead of a business key; read-modify-write counters (lost updates).
5. Throwing instead of returning `InvocationError`; busy-waiting inside a function for a Retry-After.
6. Trusting `retryContext.retentionWindow` on the first delivery (a TypeError) or as always present.
7. One push per item; that hits 500 per minute and the cyclic 1,000.
8. Relying on push order, on query-after-write for uniqueness, or on persisted cursors.
9. `keyPolicy` inside `batchSet`/`transact()`; TTL in the 3rd argument of `transact().set`.
10. A single hot progress key; unbatched small writes; that burns 10 KB units per write.
11. Assuming a scheduled run retries after a throw, or that duplicates cannot occur; overlapping long runs.
12. Missing `ignoreSelf` and causing feedback loops; no cleanup of child records on a project delete; no 403 retry
    on install.
13. Exceeding log caps with payload dumps.
14. Publishing every streamed LLM chunk to Realtime (50 ops/s).

### 3.3 How the offline grader can test it fairly
Principles: virtual time (25 s, 900 s, 15-min gaps, 24 h retention and hourly windows run in seconds of wall time);
seeded, DECLARED fault classes with undisclosed timing; outcome grading against the ground truth; any implementation
that reaches the right outcome passes.
- **Deadlines.** Kill a resolver at 25 virtual seconds and a consumer at `timeoutSeconds`. After a kill: no lost and no
  double-applied items.
- **Chaining.** Variable per-item cost totalling more than 900 s must complete exactly once (remaining-time + watermark
  + continuation event).
- **Delivery.** Seeded duplicates (including redelivery after a kill that followed a side effect), shuffles and one
  poison item. The poison item lands in a visible failed list with its reason, and everything else completes.
- **429 from an upstream.** Count retries issued earlier than `Retry-After` and the virtual seconds slept inside a
  function.
- **Push limits.** Dataset sized so that one-push-per-item chaining hits the cyclic 1,000 while batched pushes fit.
- **Concurrency.** A mock upstream with a declared maximum parallelism; peak concurrency and 429s are recorded.
- **Storage.**
  - Interleaved get→set on one aggregate (lost update).
  - Query lag injected.
  - A seeded subset of batch keys fails.
  - Cursors invalidated across invocations.
  - 10 KB unit accounting.
  - Expired-but-readable TTL values.
- **Schedules.** Duplicate concurrent runs, a skipped tick and an overlap. Checks: no double processing, no gap, stale
  leases expire.
- **Deploy mid-flight.** Old-shape bodies are delivered to new code.
- **Warm reuse and unawaited work.** The emulator freezes pending timers at return and resumes them later, or never.
- **Logs.** Lines and bytes are capped; secrets and emails are scanned for.

### 3.4 What the contract must state (pins; see §9 for the full list)
- The kill semantics: partial writes persist.
- The delivery model: duplicates, no order, retention 24 h, +72 h only for platform errors, drop on app-level expiry.
- The redelivery schedule the emulator uses (1.0 used 1, 2, 4, 8 minutes, then every 15, to 24 h).
- v2+ retry semantics; `retryAfter` ≤ 900.
- The 100 KB per-event enforcement point.
- Job stats and cancel definitions.
- The query lag model.
- Cursor validity ("only within the invocation that produced it").
- Scheduled duplicates, skips and overlaps "will be injected".
- The install 403 window, which must be under about 60 min virtual.
- The memory cap per `memoryMB`.
- That module state may persist or reset at any time.
- Log caps.
- Every injected fault CLASS, never its timing.

### 3.5 Emulator / mock work implied
The real SDKs run unmodified through hooks verified in source (rob §18):
- `global.__forge_runtime__` (`appContext` + a `lambdaContext` closure);
- `global.__forge_fetch__({type:'kvs'…})` for every KVS/CES path;
- `@forge/events` → `/webhook/queue/publish|stats|cancel` via `__requestAtlassianAsApp`, with a retry recognised by
  the RETURNED object shape.

New on top of 1.0:
- the virtual-time kill;
- a worker `resourceLimits` cap equal to `memoryMB`;
- a timer freeze at handler return;
- the warm multi-tenant module cache;
- scheduled overlap and skip injection;
- the query lag;
- cursor invalidation;
- the deploy-mid-flight upgrade;
- jobs, stats and cancel;
- concurrency-key accounting across queues.

---------------------------------------------------------------------------------------------------------------------

## 4. Custom UI boot speed

### 4.1 Verified facts
- Custom UI runs in an iframe, from Atlassian-hosted static resources (boot#1).
- **Served form, observed on 3 vendors' apps, Jul–Sep 2026.** `global-bridge.js` then
  `iframeResizer.contentWindow.min.js` are the first two elements of `<head>`, BOTH synchronous, before any app
  script. The CLI uploads the resizer tag with `async`, so Atlassian rewrites the HTML after upload (boot#2,3).
- `@forge/bridge` 7.1.0 reads `globalThis.__bridge.callBridge` at MODULE LOAD (invoke.js, getContext.js) and throws
  "Unable to establish a connection with the Custom UI bridge." when it is absent (boot#4).
- **CSP.**
  - Scripts, styles and fonts are limited by default to the app origin, the platform CDN and Atlassian object-store
    hosts. Images also allow `data:`, `blob:` and several avatar hosts. Apps CAN add external origins via
    `permissions.external`, losing RoA except for analytics (boot#5; the original claim was refuted).
  - Unsafe sources are declared under `permissions.content`. CSS-in-JS that injects `<style>` elements needs
    `content.styles: 'unsafe-inline'`; React `style` props and CSSOM `insertRule` are not blocked (boot#6).
  - Production `script-src` = `'self'` + `https://forge.cdn.prod.atlassian-dev.net` + 4 object-store hosts, with no
    `unsafe-inline` or `unsafe-eval`. `font-src` = `'self'` + object-store hosts. `style-src` adds the site host and
    the CDN, and `'unsafe-inline'` only when declared (boot#7).
  - `@forge/csp` auto-hashes STATIC inline `<script>` in index.html, unless `unsafe-inline` is declared. This is
    inferred from the code; production auto-hashing is unobserved (boot#8).
- RoA apps may not declare external resource domains except for analytics, so they self-host every boot asset
  (boot#9).
- **Resource limits.** 50 resources; 5,000 files and 100 MB per bundle; 25,000 files and 1 GB cumulative (boot#10).
- **Load-time guidance.** The only docs sentence that names "load times" is about shared chunks across entry points
  and code splitting. A few other pages carry boot-relevant guidance (boot#11).
- **Entries and payloads.** Up to 50 named entries per resource (boot#12; GA per §1.1). Front-end invoke payloads are
  capped at 500 KB request and 5 MB response; resolvers run 25 s (boot#14). User-led invocation limits as in §0.6;
  staff call them "soft limits" (boot#15).
- **Invoke rate limits.** A 429 on invoke arrives as a THROWN error with `metadata.rateLimitProperties.rateLimitReset`
  in epoch seconds. The docs example retries up to 3 times, so "exactly one retry" would be OUR rule (boot#16).
- **The client limiter.** `invoke()` and `initFeatureFlags()` throw `BridgeAPIError` past 500 calls in a fixed 25 s
  window per frame, with no queueing. The window re-anchors at the first call after it expires. `invokeRemote` and
  `invokeService` rebuild their limiter on every call, so it never trips. `requestJira` and `getContext` have no
  client limit (boot#17; re-read in the 7.1.0 source this pass [B]).
- **Bridge call costs.**
  - `view.getContext()` is a host RPC on every call, never cached (boot#18).
  - Awaiting `view.theme.enable()` before mount is DOCUMENTED best practice. It fetches the theme from the host, and
    the bridge then appends 5 token stylesheets from the CDN. That set follows the theme and platform flags, so exclude
    it by URL pattern, not by count (boot#19).
- **Atlassian's own guidance** (optimise-forge-costs, Aug 21, 2026; Atlassian forge-skills, 2026):
  - call `requestJira` from the browser for read-heavy user-context reads, with no resolver round trip (boot#20);
  - invoking a resolver to fetch context is "a surprisingly common anti-pattern" (boot#21);
  - cache resolver results in component state (boot#22);
  - frontend requests run as the context user, and storage is reachable only via a resolver (boot#23);
  - "Multiple independent `invoke()` calls on page load" is a cost signal (boot#25);
  - independent calls should use `Promise.all` or bounded concurrency, about 5–10 concurrent (boot#26);
  - return only the fields the UI consumes (boot#27);
  - missing loading and error states are a readiness gap (boot#28).
- AtlasCamp 2026 (Atlassian engineers): "choose UI Kit over Custom UI where possible, tree-shake and minify bundles"
  (boot#24).
- **Feature flags.** `initialize()` downloads the flag configuration and "may add latency to app startup";
  `checkFlag` THROWS before init (boot#29).
- Staff: UI Kit scales better than many iframes on one page (boot#30). `emitReadyEvent` is Confluence-only, with no
  Jira equivalent (boot#31).
- **No developer-visible boot metric exists.** Invocation metrics exclude cold start and iframe code. Custom metrics
  are counters only. Internal iframe telemetry exists in global-bridge.js but is not exposed (boot#32,33). Marketplace
  has only a qualitative performance rule, and RoA has none (boot#34).
- **Measurement science.**
  - Concurrent runs and machine load skew lab timing; DevTools throttling does not mitigate it; CPU throttling is a
    multiplier relative to the host (boot#35,36).
  - `threadTicks` counts executing time only, but core placement on P/E cores still shifts it (boot#37).
  - V8 precise coverage gives near-load-independent counts if armed before the frame's first script; timer and rAF
    code adds iterations under load (boot#38).
  - CDP initiators give causality, but `stack` needs the Debugger domain on the frame session (boot#39).

### 4.2 Traps a strong model plausibly gets wrong
1. `await invoke()` before `render()`, which leaves the iframe blank (no shell).
2. A sequential waterfall: `getContext` → invoke config → invoke data → invoke prefs.
3. One invoke per widget or row; re-invoking on every render or tab switch.
4. Fetching context through a resolver.
5. Dev React builds, Atlaskit-heavy all-in-one bundles, the admin code shipped in the front entry.
6. Lazy-chunk chains 4–6 deep.
7. External fonts or CDN scripts (CSP violations); runtime `<style>` injection without `content.styles`.
8. `await featureFlags.initialize()` gating the shell; calling `checkFlag` before init (it throws).
9. A retry storm, or a blank page, on a 429 from the bootstrap invoke.
10. Absolute `/assets` paths (1.0 trap).

### 4.3 How the offline grader can test it fairly (count, byte and causality metrics; never wall time as a gate)
- **Protocol: "hold-and-release waves".**
  - Cold browser context; the production CSP (computed by `@forge/csp` as production does).
  - Host-local ops are answered immediately: `getContext`, `theme.enable`, `createHistory`, flags and metrics
    emission.
  - Backend-bound ops are queued: `invoke`, `requestJira`/`requestConfluence`, `invokeRemote`, `initFeatureFlags`,
    Realtime subscribe.
  - When the frame settles, the queue is released as one wave.
  - The resolver side does the same: outbound calls are released in rounds when the event loop idles.
- **Metrics** (thresholds are proposals; calibrate on the golden plus an alternative app, then FREEZE as numbers in the
  contract):
  - B1: shell at wave 0.
  - B2: ≤ 1 backend wave to READY.
  - B3: ≤ 2 backend-bound ops before READY.
  - B4: ≤ 200 KB of app-origin gzip -9 bytes before READY, platform-injected files excluded; prefer served-body gzip
    over `encodedDataLength`.
  - B5: ≤ 6 app-origin requests.
  - B6: initiator depth ≤ 3, with the Debugger domain enabled on the frame.
  - B8: bootstrap resolver ≤ 3 rounds, and calls do not scale with items (seed N vs 4N).
  - B9: bootstrap payload ≤ 2× the golden.
  - B10: JS work as a ratio to the golden in the same session only, low weight; measure the golden-vs-golden spread
    under load first.
  - B11: CSP violations = 0 and non-app, non-platform origins = 0. The allow-list must include the DEFAULT CSP hosts,
    since Jira avatar URLs point there.
  - B12: the shell renders while flags and Realtime are held. It must use its own placeholder, NOT `checkFlag`.
  - B13: 0 repeated bootstrap invokes on in-session navigation.
  - B14: under a 429 on the bootstrap invoke, the shell stays and there is exactly one retry at or after
    `rateLimitReset` (stated rule).
- **Fairness.**
  - Never penalise an awaited `theme.enable()`.
  - Exclude platform scripts and token CSS.
  - Verify READY by seeded CONTENT, not by a marker.
  - Use a scoring seed different from the dev seed.
  - Use the real `@forge/bridge`, so the 500/25 s limiter and module-load capture behave as in production.

### 4.4 What the contract must state
- READY and SHELL selectors per surface, and the seeded content READY must show.
- That boot is graded cold, by counting, not by time.
- The free ops and the backend-bound ops.
- Every numeric budget, and the gzip level.
- The production CSP is enforced, and "no external origins" is a BENCHMARK/RoA rule (not a platform fact; boot#5).
- The bridge limiter.
- That dev and scoring seeds differ in size.
- The B14 429 shape and the exactly-one-retry rule.
- The `rateLimitProperties` field names the emulator returns (docs `rateLimitValue` vs typings `rateLimitLimit`).

### 4.5 Emulator / mock work implied
- Serve each resource as a real iframe document in the SERVED form: bridge first, then the resizer, both synchronous.
  1.0's `addInitScript` install is acceptable because platform cost is excluded.
- Apply the production CSP via `@forge/csp`, the sandbox, and the CSP report collector.
- The wave scheduler.
- CDP: per-frame coverage armed before the first script; Network initiators with Debugger enabled.
- Per-entry serving if multi-entry is used, and bridge injection into each entry document.
- Run this lane serially on the scoring host; never measure while other benchmark work holds the machine (Lighthouse
  receipt).

---------------------------------------------------------------------------------------------------------------------

## 5. UI Kit 2 admin panel (with the offline-rendering feasibility verdict)

### 5.0 Verdict: FEASIBLE, confidence MEDIUM
The real `@forge/react` 12.3.0 reconciler and the real `@forge/bridge` 7.1.0 can run offline against a
benchmark-owned host. The host implements the bridge protocol (`reconcile` → ForgeDoc, `invoke`, `getContext`,
`fetchProduct`, events, flags) and renders the ForgeDoc tree to accessible HTML that a probe can drive.

Grade the ForgeDoc tree, the host-state semantics and the bridge-call log, never pixels.

**Why MEDIUM, not HIGH.**
- The app side is verified in source: reconciler.js, dynamic-table.js, useForm.js, content-wrapper.js.
- The product host (`@atlassian/forge-ui` ^37) is closed (uikit#13), and staff confirm there is no offline renderer
  (uikit#14).
- A few host contracts are inferred:
  - the callback argument shapes for Form `onSubmit`, Tabs `onChange` (2nd argument) and UserPicker `isMulti`
    `onChange`;
  - host behaviour on invalid xcss, unknown types, element-valued props and an empty Root.
- No end-to-end offline render ran in this research. Forge 1.0's spike `prove-uikit.cjs` already captured reconcile
  and drove a click to an invoke.

**Conditions for the verdict to hold.**
1. Pin React 18.3.1: `react-reconciler` 0.29.2 peer-requires ^18.3.1, and `@forge/react` hard-depends on React 18
   (uikit#1).
2. Bundle `.jsx` the way Forge does: Babel classic pragma, so a `.jsx` without `import React` crashes, as it does in
   Forge. esbuild `jsx:'automatic'` hides that; either replicate the classic pragma or state the deviation (uikit#23).
3. Install `globalThis.__bridge` AND `self` before the bundle evaluates (uikit#5,6).
4. Deep-snapshot each `reconcile` payload, because later commits mutate the live container (uikit#24).
5. Deliver callbacks asynchronously with serialised arguments, never inside `act()`. Under both production transports
   (port-rpc, and the post-robot JSON default) functions become proxies, Errors are serialised, and React elements lose
   `$$typeof` (uikit#7).
6. Use an allowlisted component set with exhaustive prop handling. Any other type or prop takes a loud "unmodelled"
   path. forge-sim silently drops Tabs `onChange`/`selected` and the Textfield `onBlur`/`id` from `register()`
   (uikit#26).
7. Host-owned state: uncontrolled inputs; DynamicTable sort and page using the ADS comparator, `Intl.Collator`
   numeric with the locale pinned to en-US (uikit#12); uncontrolled Tabs.
8. Capture the real callback arguments once with a probe UI Kit admin page on **wolfaenpak** (a sanctioned test site),
   and freeze them as the host's golden fixtures. This closes uikit could-not-verify items 2–4.

**Install footprint.** A lock-only resolve of `@forge/react` 12.3.0 is 282 packages, 143.6 MB unpacked, 39,581 files,
with an on-disk bound of about 306 MB. The runtime subset is 6.2 MB. Type-checking TSX needs the full closure
(uikit#33). Forge 1.0's "1,246 MB" (DESIGN D8, §16) is NOT reproducible for 12.3.0, so D8's exclusion reason is void.
Run one real `npm ci` + `du` before freezing the kit size, because the 143.6 MB figure is a registry-metadata sum.

### 5.1 Verified facts
- "UI Kit 2" = `@forge/react` ≥ 10 + `render: native`; UI Kit 1 (`@forge/ui`) stopped working in Feb 2025 (uikit#2).
- **The ForgeDoc.**
  - Nodes are `{type, children, props without children, key: uuid v4}`; text nodes are `{type:'String',
    props:{text}}`; the whole tree goes over `callBridge('reconcile')` after every commit (uikit#4).
  - The core is 80 codegen'd string types, plus the wrapper primitives, `Root` and `String`.
  - Beyond that: `CheckboxGroup`, the deprecated `Em`/`Strike`/`Strong`, a `MacroConfig` root, `CustomFieldEdit`
    (`/jira`) and 15 global types (uikit#3).
- **Callbacks and inputs.**
  - Function props stay in the app, tagged with a `__id__` (uikit#7).
  - Inputs get a `SerialisableEvent` whose `target` carries `{selectionStart, selectionEnd, value, checked, name, id,
    tagName, type}`. RadioGroup has no `onBlur`; Range's `onChange` gets a number (uikit#8).
- **`useForm`.** It runs in mode `onBlur` with re-validation on change, and its blur is `() => trigger(fieldName)`.
  After a field's first change, later changes re-validate without a blur. Textfield types `email`, `url`, `tel`,
  `search` and `date` store the WHOLE event object as the value, so the host must report the real `target.type`
  (uikit#9).
- **DynamicTable.** The tree is pre-flattened: `DynamicTable{tableProps} > [ContentWrapper{name:'head'} > Cell]` and
  `[ContentWrapper{name:'rows'} > Row{rowKey} > Cell]`. The HEAD HAS NO ROW. Function-component cells are called
  outside React (uikit#10, corrected).
- **Sorting** is host-owned, by cell `key`, with no `onSort` the app could see. ADS uses numeric collation, so integer
  digit strings sort correctly. Mis-sorts happen only for decimals ("1.5" < "1.05" < "1.25"), negatives ("-5" <
  "-10"), separators or currency ("$1,200" < "$900") and display dates ("9 Oct 2026" < "10 Sep 2026"). Keys must be
  unique per column (uikit#11, corrected). A new column starts ASC; clicking the same column toggles. In a rankable
  table, clicking the current DESC column unsorts it (uikit#12).
- **Render-prop components** are evaluated in the app and wrapped in ContentWrapper names (`head`/`rows`,
  `editView`/`readView`, `content`/`trigger`, `avatar`/`content`). `AdfRenderer` sends `document` and
  `documentWithoutMedia` instead (uikit#25).
- **`invoke` limits.** The 500 per 25 s client limiter is per loaded bundle; call 501 rejects. For bursts above 300 per
  second the per-install 300/s limit bites first, and the client limiter bites first only between about 20/s and 300/s
  (uikit#16). `rateLimitProperties` field names: docs `rateLimitValue`, typings `rateLimitLimit`; the metadata also
  comes back on 429 errors (uikit#17).
- **Modules.** 56 of the 211 module types in schema 13.6.0 accept `render: native`; `jira:adminPage` has `render` enum
  `['default','native']` (uikit#18).
- **Lint.** A UI Kit resource without a `bundler` must be a FILE, not a directory ("Client Side UI Kit resource
  (${folder}) cannot be a directory"), and its entries must not be `.html` (uikit#19).
- **Admin page.** It adds an Apps item to Jira admin; `useAsConfig` adds a Configure button. Subpages are Custom UI
  only and not allowed on the config or get-started entry. Who may open the page is undocumented, so enforce admin in
  resolvers (uikit#20).
- **Full pages.** `jira:fullPage` deprecated; `global:fullPage` Preview; `global:ui` EAP and banned from production
  Marketplace apps (uikit#21).
- **Constraints.** No DOM, portals, refs or arbitrary HTML. XCSS only on Box and Pressable; nested selectors banned;
  pseudo-classes allowed; MOST attributes token-restricted (uikit#22).
- **Tabs.** Controlled (`selected` + `onChange(index, UIAnalyticsEvent)`) or uncontrolled (`defaultSelected`); `id`
  is required. Every TabPanel is always in the tree, so switching uncontrolled Tabs emits NO reconcile (uikit#27).
- **Form.** The host calls `Form.onSubmit` with NO data. The "only when valid" gate is the app's `handleSubmit`
  (uikit#28).
- **UserPicker.** Under `useForm` the stored value is the `onChange` argument `{avatarUrl,email,id,name,type}`, so the
  app must map `.id`. The documented "array of account IDs" applies to the macro-config form object (uikit#29).
- **Modal.** A `title` renders a header with a close button; `onClose` covers Esc, backdrop and the close button
  (uikit#30).
- **testId** reaches the ForgeDoc for Button, LoadingButton, DynamicTable, Tabs, Tab, TabPanel, Select, Textfield,
  TextArea, Toggle, Checkbox, RadioGroup, Modal, ModalTitle, SectionMessage, Heading, Text, Lozenge, Popup and
  Comment. Form, FormHeader, FormSection, FormFooter, TabList and ModalTransition have none. InlineEdit's `testId` is
  typed but DROPPED by the wrapper. Ids get a random per-load `forge-app-xxxxx-` prefix; keys are random (uikit#31,
  corrected).
- **Manual packaging** (`bundler: manual@2026`, Preview) uploads prebuilt directories unprocessed, and lint skips the
  UI Kit rules (uikit#32).
- Production UI Kit most likely runs in a hidden Forge-CDN iframe page; a worker build exists only behind an
  experiment flag. This is an inference consistent with all the evidence (uikit#15).

### 5.2 Traps a strong model plausibly gets wrong
1. Using react-dom, DOM refs, raw HTML or `@forge/ui`; React 19.
2. A `.jsx` file without `import React` (breaks only under Forge's classic pragma).
3. Expecting `onSubmit(data)` from the host without `handleSubmit`; sending the UserPicker object instead of the
   account id.
4. Textfield `type="email"` with `register()`, which stores the event object.
5. Sorting by display strings: currency, decimals or display dates as cell keys.
6. Sidebar subpages on a UI Kit admin (Custom UI only); `useAsConfig` with `pages`.
7. A `testId` on InlineEdit, or on Form/TabList, for automation hooks the contract names (never delivered).
8. Hooks inside DynamicTable cell components (called outside React).
9. A `setState` storm after `await`: legacy-root unbatched commits, one full-tree reconcile each.
10. Treating `displayConditions` as authorization.
11. Bursts above 500 invokes per 25 s, from polling or per-row invokes, which throw on the client.

### 5.3 How the offline grader can test it fairly (everything here must be STATED)
- **Manifest.** `jira:adminPage` with `render: native`; `resource` is a file, or a stated `bundler: manual@2026`;
  optional `useAsConfig`. Schema and kit lint use the exact messages.
- **Boot and stability.**
  - Logical bridge-call log: a stated maximum of invokes before the first ForgeDoc containing the main table; a
    non-empty first ForgeDoc (loading state).
  - Quiescence: no reconcile within K host ticks after each action; no `onError`; no `BridgeAPIError`.
- **Structure.** Stated `testId`s, only on components that deliver them; stated texts; `Label.labelFor === input.id`
  (compare the relationship, never the random value).
- **Interactions.**
  - Tab switch → expected panel text, read from the host's selected index for uncontrolled Tabs.
  - Sortable header clicked twice → rows in the stated order, using stated cases that actually mis-sort.
  - Page → `onSetPage`.
  - Open the modal → title present.
  - Fill the form (`SerialisableEvent` + blur) → exactly one `invoke('<stated key>', <stated payload>)`.
  - Invalid input → stated `ErrorMessage` text and ZERO invokes.
  - Double-activate Save → one invoke, if stated.
- **Efficiency** (only if stated): reconcile commits per action and ForgeDoc bytes, which are deterministic per app.
- **Security.** Never inferred from the UI; grade it through §2 S1/S6/S7.

### 5.4 What the contract must state
- The component allowlist the host renders, and that anything else is unmodelled.
- The `testId`s and texts graded.
- The sort semantics (by cell key, ADS order).
- The form payload shapes (e.g. `accountId` strings).
- Controlled vs uncontrolled expectations.
- The admin navigation mechanism allowed (Tabs or Router).
- The resolver payloads.
- That no pixels are graded.
- The React version and bundler behaviour (classic-pragma `.jsx`).

### 5.5 Emulator / host work implied
- **Host bridge ops:** `reconcile` (snapshot), `onError`, `getContext`, `invoke` (+ metadata), `fetchProduct`,
  `on`/`emit` (resolving to `{unsubscribe}`), `showFlag`/`closeFlag` drawn OUTSIDE the app area (1.0 defect A),
  `enableTheming`, and `createHistory`/`navigate`/`getUrl` if Router is allowed. Any other op is a loud
  `unsupported_bridge_call`.
- The serialiser (async, proxies, Error shape, dropped symbols).
- Host state keyed by ForgeDoc `key`.
- The ADS sorting comparator, pinned to en-US.
- Accessible HTML: `<table>`/`<th aria-sort>`, tab roles, dialog role, `data-testid`.
- A host-side UserPicker search against the mock user directory.
- The wolfaenpak probe capture as the golden fixture.

---------------------------------------------------------------------------------------------------------------------

## 6. Rate-limit tiers and burst dosing ("a ton of calls … keep it within tier 1")

### 6.0 Three things Atlassian calls a "tier" (tiers note §1; confirmed quotes tiers#43,44)
1. **The REST rate-limit tier.** Tier 1 Global Pool / Tier 2 Per-Tenant Pool. THIS is the owner's "tier 1".
2. **The pricing "free tier".** Atlassian's informal name for the monthly free allowance of consumption pricing:
   billed, not enforced.
3. **LLM model tiers:** Haiku, Sonnet, Opus.

### 6.1 Verified facts
- **Tiers.**
  - Tier 1 is the default: 65,000 points per hour per app across all tenants (tiers#1,2).
  - Tier 2 comes only after Atlassian review: Free 65k; Standard 100k + 10×users; Premium 130k + 20×users; Enterprise
    150k + 30×users; capped at 500k. An app cannot move itself (tiers#3).
  - Quotas reset at the top of each UTC hour with no carry-over (tiers#4).
- **Point costs and the wall.**
  - Costs per §0.2; an 8-user group page costs 17 points (tiers#5,6).
  - On exhaustion every request is denied until reset (tiers#7). Forgiveness is claimed but unpublished; partners
    measured a wall (tiers#8).
- **Bursts and per-issue writes.**
  - Three systems run at once (tiers#9). Burst buckets are per tenant × endpoint (path + method), do not scale with
    users, and have unpublished capacity. The same page also cites "10 requests/second for GET" and a 100-token /
    10-per-second example (tiers#10).
  - Design for the steady-state refill rate, not the buffer (tiers#11). The bucket is shared by ALL apps and API
    tokens on the tenant (tiers#12, CHANGE-2753).
  - Per-issue writes: 20 per 2 s and 100 per 30 s, reason `jira-per-issue-on-write` (tiers#13).
- **Headers.**
  - `RateLimit-Reason` comes only on 429. Jira values: `jira-quota-global-based`, `jira-quota-tenant-based`,
    `jira-burst-based`, `jira-per-issue-on-write`. Confluence: `confluence-quota-global-based`/`-tenant-based`.
    Undocumented values seen in production: `jira-cost-based` and
    `jira-max-concurrent-threads-across-all-instances-per-tenant-per-user` (tiers#14).
  - `Retry-After` comes only on 429 (seconds); some 503s carry it. The April 2026 gateway 429 had NO Retry-After and
    no reason, only `X-Failure-Category`, `X-Ratelimit-Limit: 360000`, `X-Ratelimit-Remaining: 0` and
    `X-RateLimit-Reset` (tiers#15).
  - `X-RateLimit-NearLimit` is true below 20% remaining, but Atlassian is moving quota signalling to
    `RateLimit`/`RateLimit-Policy`, so treat NearLimit as optional legacy (tiers#16).
  - `r` appears only past about 80% usage (tiers#17).
  - Grammar: comma-separated quoted policy names with `;q=;w=;r=;t=`; policies `global-app-quota`, `tenant-app-quota`,
    `jira-burst-based`; count and order are not fixed (tiers#18).
  - `Beta-` = informational; unprefixed = enforced; one response can mix both (tiers#19).
- **Reactions and retries.**
  - React by reason: on quota, pause ALL requests until reset; on burst, slow THAT endpoint; on per-issue, delay
    writes to THAT issue (tiers#20).
  - Retry only idempotent requests with a delay signal, with Retry-After as the minimum, exponential backoff with
    jitter, and capped attempts (example 4). Atlassian's prose and pseudocode disagree on jitter and on retrying
    without Retry-After (tiers#21).
  - At most 5–10 concurrent requests is ADVICE (cost guide), not an enforced cap (tiers#22).
  - Spread background jobs across the hour with jitter (tiers#23).
- **Who pays.**
  - Bridge `requestJira` is exempt (staff-only, twice); backend and remote calls are counted; frontend calls may be
    counted later with notice (tiers#24,25).
  - Invocations cost no points (tiers#26).
  - Staff: Tier 1 lets a concentrated tenant hurt the others (tiers#27, T99654 #2). Partner field report (klaussner,
    2026-03-17, quote verified [B]): one user clicking a 250-page fetch button exhausted the global quota in about
    3 minutes, "rendering the app unusable for up to one hour on all tenants" (T99654 #1).
- **Bulk endpoints.**
  - Bulk issue edit/move: 1000 issues and 200 fields per request, only 5 concurrent across ALL users, async `taskId`
    polled at `GET /rest/api/3/bulk/queue/{taskId}`. Single-issue bulk operations count as one (tiers#28).
  - `bulkfetch` returns 100 issues by default, and 1000 only with explicit fields (≤ 100 fields, single-value only, no
    heavy expands); more than 100 otherwise is a 400. Changelog `bulkfetch`: 1000 issues, 10 field ids (tiers#29).
  - `search/jql` pages with `nextPageToken`, up to 5000 when only id/key is requested; charged 1 + results in the
    field (tiers#30).
- **Platform limits.**
  - Invocation limits as in §0.6 (tiers#31). The bridge limiter lives in the SOURCE (tiers#32). Runtimes 25/55/900 s
    (tiers#33).
  - KVS/CES: 1000 RPS; 4000 + 4000 10 KB units per minute; batching saves units; 25 operations per transaction
    (tiers#34).
  - Storage is per app+installation, so there is no cross-tenant counter (tiers#35). Module-level caches are shared
    across tenants, so caches that save points must be tenant-keyed (tiers#36).
  - Realtime: 50 ops/s (tiers#37). Async: 50 events per push, 500 per minute, 200 KB; `retryAfter` ≤ 900;
    `FUNCTION_UPSTREAM_RATE_LIMITED`; installation-scoped concurrency keys (tiers#38). Scheduled triggers: ≤ 5, one
    `fiveMinute`, spread, duplicates possible, a throw is not retried (tiers#39).
  - LLM: 100 RPM and 500k TPM per model per installation (tiers#40). No free allowance; 10/30/50 credits per 1M tokens
    (Haiku 4.5/Sonnet 4.5/Opus 4.6); $0.10 in / $0.50 out per credit (tiers#41). Usage is reported per response
    (tiers#42).
- **Pricing.** Platform quotas were retired for consumption pricing from 2026-01-01; hard limits and REST limits are
  separate systems (tiers#43). The monthly free allowance per app: functions 200,000 GB-s, then $0.000025/GB-s; KVS
  reads 0.1 GB at $0.055/GB; KVS WRITES 0.1 GB at $1.09/GB (19.8× reads); logs 1 GB at $1.005/GB; SQL 1 compute-hour
  and 100k requests free. Usage in all environments is billed (tiers#44).
- **Visibility.**
  - Frontend: `invoke(..., { rateLimitProperties: true })`. Web triggers return `X-Ratelimit-Limit/Remaining/Reset`
    (tiers#45).
  - **Since 2026-04-09 (FRGE-1923 Fixed), bridge `requestJira`/`requestConfluence` responses forward the REST
    rate-limit headers to frontend code** (tiers#46: the earlier claim is OUTDATED).
- **Identity and testing.** Bridge `requestJira` always runs as the user (no asApp); privileged work goes through
  backend functions, whose product REST calls are what get charged (tiers#47). Atlassian's PM recommends simulated 429s
  and a "test quota" layer, which is exactly an offline emulator. The docs say "Do not perform rate limit testing
  against Atlassian cloud tenants" (tiers#48). So no Tier 1 probing on wolfaenpak.

### 6.2 The design tension the panel must resolve (evidence-backed)
Dosing a pool shared across tenants is hard by construction:
- Forge storage is per installation, and module memory is per warm process, unreliable, and forbidden for tenant data
  (tiers#35,36).
- RoA forbids an external coordinator (egress).
- The only cross-tenant signal is the `global-app-quota` `RateLimit` header, whose `r` appears only past ~80%.

Realistic disciplined designs combine:
1. A per-installation budget (a stated fair share of 65k).
2. Self-accounting of points spent, using the published cost table.
3. A reaction to `r`/`t`/NearLimit and to a quota 429 (pause all, resume at reset).
4. Spreading scheduled work over the hour with jitter.
5. Per-user or per-tenant throttles on expensive user-triggered actions (the button-masher DoS).
6. Pushing user-context reads to bridge `requestJira`, exempt by the stated assumption but still subject to burst.

**The contract must state the fair-share policy the grader uses.** Without it, "dosing" cannot be graded fairly.

### 6.3 Traps a strong model plausibly gets wrong
1. Budgeting per tenant as if Tier 2 applied; assuming forgiveness.
2. Waiting for headers that only arrive past 80%; parsing `RateLimit` assuming one policy, a fixed order, or a
   mandatory `r`; ignoring `Beta-`.
3. Retrying a quota 429 after `Retry-After` seconds on every endpoint in parallel, instead of pausing everything until
   reset; slowing every endpoint on a burst 429.
4. Retrying non-idempotent POSTs blindly; ignoring the gateway 429 with no Retry-After.
5. Unbounded `Promise.all` over hundreds of issues; using concurrency to dodge limits.
6. N+1 single-issue GETs instead of `bulkfetch` with fields; `search/jql` without `fields`; per-row identity reads
   (2 points per object).
7. Polling `/bulk/queue/{taskId}` hot; more than 5 concurrent bulk edits; many writes to one issue (per-issue limit).
8. Running scheduled syncs at :00 for every tenant; a big sync starving interactive users.
9. Caching permission or API results without a tenant (and user) key.
10. Using `privacy.reportPersonalData` for thousands of accounts (concurrent batches).

### 6.4 How the offline grader can test it fairly (tiers note §11, corrected)
| id | observes |
|---|---|
| T1 | points ledger per virtual hour across ≥ 2 tenants; count of `*-quota-global-based` 429s (target 0 nominal); interactive success rate of a small tenant while a large one syncs; scenario completion within N virtual hours |
| T2 | total points for a fixed scenario vs a reference budget; bulk-to-N+1 ratio; `fields` present on search and bulkfetch |
| T3 | after an injected quota 429: no product call to ANY endpoint before reset (+ tolerance); deferred jobs resume; the UI shows a paused state |
| T4 | max in-flight requests; per-endpoint rate; after a burst 429 only that endpoint waits |
| T5 | writes per issue per 2 s / 30 s window; behaviour after `jira-per-issue-on-write` |
| T6 | grade only: delay ≥ Retry-After (or reset when it is absent), attempts ≤ the cap, no blind non-idempotent retry; jitter is allowed, never required in a form |
| T7 | fuzzed header permutations (order, optional `r`, `Beta-` mix, ISO with and without seconds); the app's reported remaining/paused state vs emulator truth |
| T8 | principal of every call (asApp / asUser / bridge-user); points charged per the stated rule; bridge headers delivered (or a stated departure) |
| T9 | platform 429 counts (invocation, client limiter, KVS units, Realtime, async, LLM) |
| T10 | points-per-minute histogram (no spike at :00); per-tenant share; duplicate scheduled run → no double writes; the job resumes after an injected wall |
| T11 | LLM credits for the scenario (§7) |
| T12 | the admin consumption panel's resolver JSON vs emulator ground truth within a stated tolerance: points this hour, 429s by reason, LLM credits, KVS bytes, projected bill |

Fairness: a virtual clock (an hour costs seconds); seeded fault catalogue PUBLISHED with timings hidden; grade
outcomes, not code shape; never reproduce Atlassian bugs (bogus `t`, duplicate Retry-After) unless the contract lists
them as inputs.

### 6.5 What the contract must state
- **Quota:** 65,000 per hour per app shared by all emulated tenants; the virtual UTC-hour reset; no carry-over; a hard
  wall (a stated simplification).
- **Costs:** the per-endpoint cost table, including what counts as an "object" per endpoint.
- **Bursts:** steady-state RPS per method plus the bucket CAPACITY; whether simulated other-app traffic shares the
  bucket.
- **Per-issue writes:** 20/2 s and 100/30 s.
- **Headers:** the exact header set per 429 kind, including the gateway variant without Retry-After and an unknown
  reason string to be treated as quota-class. The grammar: `r` only past 80%, NearLimit optional, both ISO forms.
- **Retries:** which requests are idempotent; the retry cap.
- **Concurrency:** the maximum in-flight requests (a contract rule, since 5–10 is advice).
- **Bridge:** `requestJira` exempt from points (staff 2026-04-01), subject to burst, and receiving the headers
  (FRGE-1923).
- **Platform limits** emulated, with their error shapes.
- **The fair-share policy and the scenario scale:** how many tenants, issues and users.

### 6.6 Emulator / mock work implied
- **A virtual-clock gateway** in front of mock Jira (and Confluence if used) that logs one row per product call:
  `{t_virtual, tenant/cloudId, installationId, principal, method, path, status, points, policy, reason, headers}`.
- **Policies:** per-app global pool; per-tenant × endpoint token buckets with stated capacities; per-issue write
  windows; fault injection (quota, burst, per-issue, 503 + Retry-After, gateway 429).
- **Load and channels:** "other app" background traffic on the shared bucket, if stated; header emission on
  backend AND bridge channels.
- **Platform counters:** the invocation 300/s, 7,000/min and 1,200/user/min limits; the real bridge limiter; KVS
  10 KB-unit accounting.
- **Mocks:** bulk-edit async tasks (5 concurrent) with a polled queue; pagination (1.0 already serves ≥ 2-item lists in
  ≥ 2 pages on the scoring site).
- **Scale:** multiple emulated tenants from one generator. The scenario must be big enough that a naive app breaks
  65k per hour while a disciplined one does not, which needs calibration on the golden plus a naive mutant.

---------------------------------------------------------------------------------------------------------------------

## 7. Forge LLM usage

### 7.1 Verified facts
- **Status and API.**
  - GA, announced 2026-07-29 (changelog CHANGE-3365, 2026-07-30); Preview from 2026-06-01 (llm#1).
  - Only `list()`, `chat()` and `stream()`, from Forge runtime functions (llm#2).
- **Requests.**
  - Fields: `messages`, `temperature`, `max_completion_tokens`, `top_p`, `tools`, `tool_choice` (+ `model`). There is
    NO `response_format`/`json_schema`/`strict`/`stop`/`cache_control`/images. The default `max_completion_tokens` is
    undocumented (llm#3).
  - `tool_choice` takes `'auto'|'none'|'required'|{type:'function',function:{name}}`. That `'required'` or a named
    function FORCES a call is not stated by Atlassian, so state it (llm#4).
- **Responses.**
  - `function.arguments` is typed `object` but not enforced; Atlassian's tutorial parses a string defensively
    (llm#5).
  - `usage` and every usage field are optional (llm#6). Usage is reported per request; no runtime aggregate API is
    documented (llm#7).
  - `finish_reason`: `tool_use`, `end_turn`, `max_tokens`; `refusal` is undocumented on Forge (llm#8).
- **Tool loop.** The follow-up must carry the original messages, the assistant message with `tool_calls`, and a
  `role:'tool'` message with the result. Atlassian's samples answer only `tool_calls[0]`, one round (llm#9). Structured
  outputs are not supported; FRGE-2237 is a Suggestion, "Reviewing" (llm#10).
- **Streams.** A stream can end without a `finish_reason` chunk and without throwing (llm#11,12). A transport read
  error mid-stream DOES throw `StreamResponseError` (llm#12). The doc's check `finish_reason !== undefined` would accept
  a `null` mid-stream, so the emulator omits the key mid-stream or the contract says "non-null" (llm#11).
- **Models.** `list()` returns only `{model, status:'active'|'deprecated'}`: no tier, no context window, no dates
  (llm#14). Statuses change, retirement dates pass, and the 6-month notice "may not always be possible" (llm#15).
- **Validation.**
  - `temperature` + `top_p` together are rejected for ALL models. The `@forge/llm` README and Atlassian's example app
    send both (llm#16).
  - opus-4-7, opus-4-8, opus-5 and sonnet-5 reject both parameters (llm#17).
  - The SDK validates on the client and throws `PromptValidationError`: model present; non-empty messages; roles
    system/user/assistant/tool; `temperature`/`top_p` in [0,1]; positive-integer `max_completion_tokens`. Content is
    never validated (llm#18).
  - The manifest allows one `llm` module, `model: [claude]` (llm#19).
- **Limits.**
  - Per installation: 100 RPM across ALL models (llm#20); 500k TPM per MODEL (llm#21, raised from 50k on 2026-10-02,
    llm#22). Token overuse returns 429, and the SDK surfaces no Retry-After (llm#22, #37).
  - The 5-minute inference window applies only in an async consumer with a timeout of at least 300 s. A single LLM
    call MAY also be cut by the 180 s single-outbound-request cap; whether that cap applies to LLM calls is
    undocumented (llm#23,25).
  - Context windows in/out: Haiku 200K/64K; Sonnet and Opus 1M/128K (llm#24).
  - Resolvers run 25 s (llm#26); web trigger, action and agentConnector 55 s (llm#58).
- **Long jobs and Realtime.** The documented long-job pattern is: resolver enqueues → consumer (up to 900 s) calls the
  LLM → `publishGlobal` to a token-secured channel (llm#27). The tutorial is exploitable (§2.2 trap 13). Channels need
  tokens with unique per-user claims (llm#28).
- **Billing.**
  - No free allowance; billed to the developer (llm#29).
  - 50/30/10 credits per 1M tokens for Opus 4.6/Sonnet 4.5/Haiku 4.5 (llm#30); $0.10 per input credit and $0.50 per
    output credit. The cost guide's "$0.0000001/credit" is a doc error (llm#31).
  - NO published rate for sonnet-4-6, sonnet-5, opus-4-7, opus-4-8 or opus-5, so price by tier and label it an
    assumption (llm#32).
  - Each call bills tokens AND the GB-seconds of the waiting function (llm#33).
  - Apps must build their own per-user, tenant or edition limits; there is no platform per-user quota (llm#34).
- **Errors.**
  - `ForgeLlmAPIError{code?, status, statusText, traceId?}` (llm#35). It is NOT exported from the package entry, so a
    named import is undefined and `instanceof` on it throws. Use `err.name === 'ForgeLlmAPIError'`, a numeric
    `err.status`, or `instanceof ForgeLlmError` (exported) (llm#36).
  - The error keeps only status, statusText, traceId, code and message: no Retry-After, and no `context`, so the docs'
    `err.context?.responseText` is always undefined (llm#37).
  - Observed: 403 `FORGE_LLMS_MODEL_FORBIDDEN` "Forge LLMs model not allowed: anthropic.<model>". A partner fixed it by
    updating the installed version; staff did not confirm the cause (llm#38).
  - Moderation blocks "high-risk messages"; the block's shape is undocumented (llm#39).
- **Security rules that bind AI** (Marketplace): asUser default, verify before asApp (llm#40); requirement 13 plus the
  admin-agent detail (llm#41); `actionVerb` truthful (llm#42); no PII or credentials in logs (llm#43); strict tenant
  isolation including runtime artefacts (llm#44). Agent memory is per user and data is permission-checked before it
  reaches an agent (remote-agents guide) (llm#45).
- Atlassian's example app is stale: a removed model, and `temperature` + `top_p` together. Models trained on samples
  will copy it (llm#46).
- **Rovo.**
  - `rovo:skill` is Preview with no skill-to-skill dependencies, no executable sources, and selection by description
    (llm#47,62).
  - SKILL.md `name`: 1–64 characters of lowercase letters, digits and single hyphens, equal to the directory name;
    `description` 1–1,024; the 500-line body is CLI-warned guidance (llm#48). `allowed-tools` ⊇ `dependencies.tools`
    (llm#49).
  - `rovo:mcp`: ≤ 50 tools, keys under 64 characters, one per app, name ≤ 30 (llm#50). External MCP clients may need
    asApp, which then needs the app's own permission check (llm#51).
  - Action inputs never authorize; context is deterministic (llm#52). Action data is capped at 5 MB (llm#53).
    Non-GET actions need confirmation (a staff RFC statement only) (llm#54).
  - Agent `name` ≤ 30 (llm#55). Rovo AI is paid by the customer's Rovo credits, while Forge LLM tokens are paid by the
    developer (llm#56). `rovo.open` works only for the same app's agents; checking `isEnabled()` is an example pattern
    (llm#57).
  - Action inputs are only string, integer, number or boolean (llm#63).
- **Transport** (emulator hooks).
  - Body: `{...request, stream}`, model in the URL, unknown fields sent unfiltered (llm#59).
  - `__forge_fetch__({type:'llm', model}, 'https://llm/<encoded model>')`; `list()` is GET `https://llm` (llm#60).
  - `stream()` parses newline-delimited JSON: fragments are joined, the tail is flushed, and unparseable segments are
    dropped silently (llm#61).
- **Pipeline limits.** Async 500 events per minute and 100 KB per event for long consumers, so enqueue ids, not
  prompts (llm#64,65). Realtime 50 ops/s, so coalesce streamed chunks (llm#66).

### 7.2 Traps a strong model plausibly gets wrong
1. A hard-coded or stale model id, e.g. `claude-3-7-sonnet-20250219` or `claude-sonnet-4-20250514` from samples, or
   an id `list()` does not return. Calling a `deprecated` model.
2. `temperature` + `top_p` together; either one on opus-4-7/4-8/5 or sonnet-5.
3. `response_format`/`json_schema`/`strict`, or `max_tokens` instead of `max_completion_tokens`; assuming JSON mode.
4. Trusting `arguments` as an always-valid object; no validation of a forced tool call.
5. Answering only `tool_calls[0]`; omitting `tool_call_id`; an unbounded tool loop.
6. Treating a stream that ended without `finish_reason` as complete; resubmitting the bare prompt instead of resuming
   with prior context.
7. `import { ForgeLlmAPIError }` + `instanceof`, which throws a TypeError; reading `err.context.responseText`;
   honouring a Retry-After that never arrives.
8. A long LLM call inside a 25 s resolver; carrying a large prompt in an async event (100 KB); publishing every token
   to Realtime.
9. No `max_completion_tokens`, no caching, re-summarising unchanged items; a cache keyed per tenant serving a
   wider-visibility user's result to a narrower viewer.
10. Prompts built from asApp data the viewer cannot see; model-written numbers or issue keys shown as fact; model
    output rendered as HTML or links.
11. Model output triggering writes without a user confirmation; executing tools outside the allow-list.
12. Logging prompts or outputs (requirement 6); no AI-generated label (AUP).
13. Rovo: authorizing from action inputs; a wrong `actionVerb`; admin actions without an admin check; SKILL.md `name`
    ≠ directory name.

### 7.3 How the offline grader can test it fairly (a scripted fake model; model quality is never graded)
- **G1 Model choice and lifecycle.** Every request uses an id `list()` reports `active` at call time; sampling
  violations get the stated 4xx. The emulator MAY mark one listed model `deprecated` (realistic: Anthropic deprecated
  sonnet-4-5 on 2026-09-30), but only if the contract says `list()` is the truth.
- **G2 Grounding.** Scripted answers carry invented numbers, a hidden key, an unknown key and a correct one. DOM
  numbers and keys must be a subset of the app's own computation and of what the viewer can see.
- **G3 Forced tool.** Malformed arguments (wrong types, missing required fields, extra keys) → the app's error state
  and no side effect. A JSON-string `arguments` must not be graded as malformed.
- **G4 Tool loop.** Two parallel tool calls; the follow-up must carry both tool messages with matching ids (the
  emulator returns 400 otherwise, stated). A write or non-allow-listed tool never reaches the mock Jira write log.
- **G5 Failures.** Stated shapes for: refusal, 403 `FORGE_LLMS_MODEL_FORBIDDEN`, 429, 500, a stream with no
  `finish_reason`, an empty `end_turn`. A defined UI state for each; non-AI features keep working; no retry storm. The
  retry rule is stated, since the SDK exposes no Retry-After.
- **G6 Dosing.** Requests per installation per 60 s window ≤ 100, and tokens per model per window ≤ 500k (stated
  estimator), even when a batch is triggered. The batch must still complete.
- **G7 Economy.** `max_completion_tokens` on every call; input under a stated per-request budget; an identical repeat
  costs 0 new calls; an unchanged rerun costs 0 calls. Cache key = installation + viewer-visibility scope + content
  hash + model. The admin (UI Kit) sets per-user and per-installation budgets; after exhaustion, 0 calls and a stated
  message. Cost shown = emulator usage × stated credit rates.
- **G8 Placement.** Latency above 25 s on long jobs: the resolver path times out and the consumer path (≥ 300 s)
  completes, delivered via Realtime with per-user claims. A second user on the same channel name gets nothing. No LLM
  call during page boot. Keep injected single-call latency ≤ 180 s, or state that the cap does not apply.
- **G9 Permission filtering.** Canaries in hidden issues appear in no person-facing prompt and in no DOM. User A (wide
  visibility) then user B (narrow): B gets no A-only content.
- **G10 Injection.** Issue text carries instructions; the fake model "obeys" (a write tool call, a hidden key, an
  exfiltration URL). Pass = no write without a confirm click, no canary, no egress, the URL not rendered as a link.
- **G11 Rovo.** Forged `accountId` input, an invisible key, a non-admin on an admin action; static manifest checks;
  a tenant with Rovo disabled must not call `rovo.open` (only if stated).
- **G12 Data handling.** A canary scan of logs; an AI label marker; the write log is empty before confirm.

### 7.4 What the contract must state
- That model ids come from `list()` at call time and deprecated ids are not called.
- Whether tier choice per job is graded, and if so the tier policy and how a tier is recognised from an id (`list()`
  has no tier).
- That `'required'` and named-function `tool_choice` force a call in the emulator.
- The tool names and JSON schemas; that invalid arguments are an error; "answer every tool call"; the loop bound.
- The error shapes for refusal, moderation, 403, 429 RPM vs TPM, 5xx, timeout and truncated stream.
- The retry rule, e.g. "after a 429, no LLM request from that installation for N virtual seconds, at most K retries,
  then queue".
- The latency envelope and the 180 s question.
- The token estimator.
- The credit rates for EVERY listed model: tier mapping Haiku 10 / Sonnet 30 / Opus 50 for the unpublished ones,
  labelled an assumption; $0.10 / $0.50 per credit.
- The budget and kill-switch semantics.
- The cache-key semantics.
- The content-inclusion rule (only what the invoking user can see).
- That model output is untrusted, and the confirmation rule for writes.
- The AI label marker.
- The realtime isolation rule (claims derived server-side).

### 7.5 Emulator / mock work implied
- Extend 1.0's scripted responder (§5.2 LLM row) with:
  - NDJSON streaming, including a truncated stream and a `finish_reason`-less end;
  - parallel tool calls;
  - per-installation RPM windows and per-model TPM windows, with 429 above them;
  - the stated error shapes;
  - prompt capture;
  - a token estimator;
  - `list()` statuses;
  - latency injection on the virtual clock (> 25 s, ≤ 180 s per call).
- Realtime token claims and the 50 ops/s limit.
- An admin-panel budget store graded through resolvers.

---------------------------------------------------------------------------------------------------------------------

## 8. Anything else that makes real Forge apps hard (verified)

- **Versions and consent.**
  - Adding scopes, egress, `unsafe-*` CSP, the `llm` module, SQL, Object Store or a first dynamic web trigger forces a
    major version that site admins must approve (§1.2). Installs can sit on old versions; a partner reported 300
    customers stranded on a broken V1 (t/77751 [B]).
  - Rolling releases (Preview) decouple permissions from code. The app must degrade gracefully when a permission is
    not yet granted, and "Atlassian will not roll back" [B].
  - Async events cross versions: N's events reach N+1 code (rob#32).
- **Lifecycle.**
  - The installed event can precede permissions (rob#37). CHANGE-3445 removes install-time permission sync for asApp,
    rolling out.
  - Reinstall = a new installation, relinkable to old data only on request within 21 days (rob §10.7, security note §8
    quotes; not separate verifier claims).
  - Only top-level deletes are emitted (rob#36).
- **Stored-data migration.** No CES schema-change support (staff t/96705 [B]); index changes cannot be backported
  (earlier research, not re-verified). Forge SQL DDL must be idempotent and backward-compatible [B: "idempotent"
  present on the SQL page].
- **Platform-level silent failures** (partner reports, not staff-confirmed; quotes and status verified [B]):
  - an hourly trigger "down to 2 invocations per 6 hours" (t/78982, 2024-04-08);
  - a scheduled trigger that "stopped invoking" for about 36 h, then resumed (t/102177, 2026-08-14);
  - public bug ECO-1403 "[Forge] High latency and out-of-order delivery for avi:confluence:updated:page events",
    closed Fixed 2026-06-09. Its ~70-minute figure and the reporter's dispute come from the robustness note and were
    not re-verified.

  Design for reconciliation, not for trusting the stream.
- **CSP surprises.** The served CSP has `form-action 'self'`, which blocks native `<form>` posts to external URLs even
  with wildcard egress (partner t/100451 [B]; header observed boot §1.3).
- **Custom field and JQL function contracts.** Value-function order, no asUser, constant time [B]; precomputations
  shared across users, 7-day expiry, 1,000 values, 25 s [B].
- **Scale ceilings.** 50,000 issues ≈ 1,000 collect jobs hits the cyclic limit exactly (partner t/101559 [B]).
  Bulk edit allows only 5 concurrent across ALL users (tiers#28). Forge SQL allows 62.5 s of query time per minute per
  installation and 16 MiB of memory per query (rob#60; limits-sql page [B]).
- **Doc and sample errors that a sample-trained model copies.**
  - The `transact().set` TTL position (rob#50).
  - CES helper names (rob#59).
  - `@forge/llm` README sampling parameters and the stale model in the example app (llm#16,46).
  - The exploitable LLM + Realtime tutorial (llm newer fact 5).
  - The tenant-isolation page's per-tenant user cache (sec#29).
  - The Assets tutorial's 1.x consumers and unawaited pushes (robustness note §6).
  - `nodejs20.x` still listed in the manifest reference [B].
- **Remote and identity.** Forge Remote `installationId` "can change without any lifecycle events" (partner t/98403
  [B]). Only relevant if remotes are used, and RoA excludes them.

---------------------------------------------------------------------------------------------------------------------

## 9. Pins and decisions the contract must make (consolidated)

### 9.1 Documentation conflicts (never grade the ambiguous side)
| # | conflict | recommended pin (evidence) |
|---|---|---|
| 1 | Burst GET: table 100 rps vs prose "10 requests/second" and a 100-token / 10-per-second example | state the refill AND the capacity; the table is the default (tiers#10) |
| 2 | Retry: jitter prose ×0.7–1.3 vs pseudocode +delay×U(0.7,1.3); pseudocode retries a 429 without Retry-After | grade only ≥ Retry-After or reset, ≤ cap, no blind non-idempotent retry (tiers#21) |
| 3 | `X-RateLimit-Reset` 429-only (Jira) vs unrestricted (Confluence) | state per product |
| 4 | `r` "optionally included" (Jira) vs "only past ~80%" (Confluence) | only past 80% (tiers#17) |
| 5 | NearLimit in the current table vs CHANGE-3045/3080 dropping `X-RateLimit-*` for quota | optional legacy (tiers#16) |
| 6 | Confluence `RateLimit-Reason` constants in docs vs production (staff: "We will fix the documentation") | use the doc constants; unknown reasons are quota-class |
| 7 | Frontend points: staff 2025-12-22 "including calls originating from the Forge UI" vs 2026-01-31/04-01 exempt; partner 571 points | exempt, as a stated assumption (tiers#24,25) |
| 8 | Async retries: within retention (v2+) vs "a maximum of four retries" (storage guide) | v2+ model; for hour-long quota walls, re-enqueue a fresh event (rob#27; tiers V.2-7) |
| 9 | Timeout/OOM: app-level for async vs platform-level for product events | pin per event type (rob §17 #2) |
| 10 | Scheduled return: "ignored" vs `{statusCode}` 204 / 5xx / 424 | `{statusCode: 204}` satisfies both; a throw = a failed run, not retried (rob#41) |
| 11 | KVS query max 100 vs example "maximum of 20" | 100 (rob#56) |
| 12 | Object depth 31 vs REST "(32)" | 31 (rob#55) |
| 13 | `transact().set` TTL 3rd arg (docs) vs 4th (SDK) | the SDK; grade TTL effect, not placement |
| 14 | `equalsTo`/`isGreaterThan`/`SortOrder` (docs) vs `equalTo`/`greaterThan`/`Sort` (SDK) | the SDK |
| 15 | Queue name: docs "alphanumeric, can start with _" vs SDK `/^[a-zA-Z0-9-_]+$/` | the SDK |
| 16 | Web trigger rate: CHANGE-1596 20,000 per 60 s per path vs limits page 300/s, 7,000/min, 1,200/user | the limits page |
| 17 | Resolver timeout 25 s vs the LLM tutorial's "default function timeout (55s)" | 25 s |
| 18 | Resources per app: 10 (stale page) vs 50 (CLI, limits-resource) | 50 (boot#13) |
| 19 | Multi-entry: GA (CHANGE-3337) vs the Custom UI page link "(Preview)" | GA [B] |
| 20 | LLM credit: $0.10/$0.50 vs $0.0000001/$0.0000005 | the pricing page (llm#31) |
| 21 | LLM lint "will fail" (docs) vs `@forge/lint` 6.3.0 Warning | deploy-readiness failure; the emulator refuses LLM calls without the module (1.0 §7) |
| 22 | `err.context?.responseText` (docs) vs no `context` (SDK) | the SDK |
| 23 | `rovo:mcp` external clients Preview (changelog) vs EAP (module text) | never demand external MCP |
| 24 | Model status `ACTIVE` (docs) vs `'active'` (SDK) | the SDK |
| 25 | Resolver context "guaranteed" vs "not all values" | rely on `accountId` and the runtime-injected fields; state the `extension.*` semantics (sec#1) |
| 26 | RFC-141 `x-webtrigger-signature` (body) vs `x-hub-signature` (table) | `x-webtrigger-signature`, `sha256=` (sec#17) |
| 27 | `rateLimitValue` (docs) vs `rateLimitLimit` (typings) | state what the emulator returns (both is safest) |
| 28 | Interval `fiveMinute` vs cost-guide "fiveMinutes"; `memoryMB` vs cost-guide `memoryMiB` | manifest reference |
| 29 | `nodejs20.x` listed vs unsupported | 22/24 only [B] |
| 30 | invoke 429: doc example up to 3 attempts | our rule (e.g. exactly one retry at reset) stated |
| 31 | Burst bucket sharing: the page "does not say" (rob#68) vs CHANGE-2753 "share the same API bucket" | shared (tiers#12) |

### 9.2 Unpublished knobs the emulator must choose and state
- **Tiers.**
  - Burst bucket capacity per endpoint; Confluence burst numbers.
  - The per-endpoint cost catalogue.
  - Forgiveness (state: none).
  - Whether other-app traffic is injected.
  - The scenario's tenant count and fair-share policy.
- **Robustness.**
  - The redelivery schedule; duplicate, shuffle and drop rates as classes.
  - The `concurrency.limit` maximum and key count.
  - Where the 100 KB per-event check runs.
  - Job stats and cancel semantics.
  - The query lag.
  - A CES conditional set on a missing key.
  - The SDK error surface of `FAIL_IF_EXISTS` (REST is 409 `KEY_CONFLICT`).
  - Scheduled overlap and skip.
  - The trigger-function timeout (25 s likely).
  - The install 403 window.
  - Log-cap overflow behaviour.
- **Security.**
  - asApp visibility.
  - `context.extension` trust.
  - The role model, refusal shape and verdict-cache TTL.
  - The HMAC scheme.
- **LLM.**
  - The error shapes listed in §7.4.
  - Refusal as a `finish_reason`.
  - The default `max_completion_tokens`.
  - Streamed tool-call chunking; `usage` per chunk.
  - Whether `list()` counts toward RPM.
  - The 180 s cap.
  - Rates for unpublished models.
- **UI Kit.**
  - Callback argument shapes, from the wolfaenpak probe.
  - Behaviour for unknown types and props.
- **Boot.**
  - READY and SHELL definitions; budgets; the gzip level; exclusions.

---------------------------------------------------------------------------------------------------------------------

## 10. Do NOT build on these

### 10.1 Refuted (4)
| claim | why it is wrong | use instead |
|---|---|---|
| uikit#10: DynamicTable head is `ContentWrapper{head} > Row > Cell` | dynamic-table.js:21-43: the head has NO Row | §5.1 shape |
| uikit#11: a `'10' < '9'` string-key sort trap | ADS `Intl.Collator(undefined,{numeric:true})` sorts 2, 9, 10, 100 correctly | decimals, negatives, currency or separators, display dates |
| uikit#31: Popup has no `testId` | PopupProps = Omit of ADS (which has `testId`), and the wrapper forwards it; it is InlineEdit that drops it | §5.1 list |
| boot#5: the CSP forces every asset from the app's own directory, with no third-party origins | external origins can be declared (`permissions.external`); the default `img-src` allows `data:`, `blob:`, gravatar, unsplash and Atlassian hosts | "no external origins" as a STATED RoA/benchmark rule |

### 10.2 Outdated (3)
| claim | superseded by |
|---|---|
| tiers#46: only backend resolvers can read rate-limit headers; bridge `requestJira` hides them | FRGE-1923 closed Fixed 2026-04-09: "now live in production" for Jira and Confluence; the reporter confirmed |
| sec#10: asApp can "manage restricted content" (2022) | staff t/93662 (2025): the app user is bound by Confluence page restrictions; ECO-822 is open. The "more privileged than the caller" leak argument still holds; worst-case visibility is an emulator assumption |
| llm#13: 8 Forge models, all ACTIVE | Anthropic's Model status (Forge's named source) shows `claude-sonnet-4-5-20250929` deprecated 2026-09-30, retiring 2026-11-30, and lists haiku/sonnet/opus 5-5, which no Forge page mentions. Forge's live `list()` is unchecked |

### 10.3 Confirmed, but the ORIGINAL wording is wrong (use the corrected one; tag → correction)
- tiers#5/6: "1 point per object" → the table (1 or 2) plus field deviations.
- tiers#22: "5–10 concurrency enforced" → advice.
- tiers#15: "gateway 429 has only Reset" → it also carries `X-Failure-Category`/Limit/Remaining.
- tiers#32 and boot#17: the bridge limit is in the SOURCE, not the README. It is per frame. `invokeRemote` and
  `invokeService` limiters never trip (re-read in source [B]).
- uikit#16: the client limiter is not always the first limit hit.
- uikit#5: `self` is needed outside a page.
- uikit#7: two transports, not one.
- uikit#9: the `useForm` additions.
- uikit#27: uncontrolled Tabs emit no reconcile.
- uikit#28: `onSubmit` gets no data.
- uikit#29: UserPicker under `useForm` stores an object.
- uikit#3: the type list is incomplete.
- uikit#22: XCSS allows pseudo-selectors.
- uikit#20: subpages are Custom UI only.
- boot#2/3: the served form is synchronous and rewritten server-side.
- boot#6: unsafe-inline matters only for `<style>` injection.
- boot#7: the CSP host lists.
- boot#11: "the only load-time guidance".
- boot#12: the named-entry injection risk.
- boot#29: `checkFlag` throws before init.
- boot#31: `emitReadyEvent` is not limited to bodied macros.
- boot#32: internal telemetry exists.
- boot#37/38/39: measurement caveats.
- sec#1: the context trust split.
- sec#7: "same resource id" is an Atlassian GitHub rule, not docs.
- sec#15/17: the HMAC tooling has shipped.
- sec#19: the customer-managed egress exception.
- sec#28 and rob#64: TPM is per model.
- llm#1: the dates.
- llm#4: forcing is unstated.
- llm#7: no aggregate API.
- llm#12: transport errors do throw.
- llm#36: `instanceof ForgeLlmError` works.
- llm#38: the cause is not staff-confirmed.
- llm#57: `isEnabled` is an example, not a must.
- rob#7: the whole runtime object, as a closure.
- rob#11: "may".
- rob#17: cite the error-handling page.
- rob#21: only the platform extends the window.
- rob#26: `retentionWindow` is optional.
- rob#27: v2+.
- rob#37: about 60 min of retries.
- rob#62: "avoid", not "never".
- rob#63: UI subscriptions count.

### 10.4 Unverifiable — design consequence for each (from the six could-not-verify lists)
- **Tiers.**
  - Enforcement coverage (phased since 2026-03-02; Beta headers still seen on 07-23).
  - Real forgiveness.
  - Burst capacities.
  - A per-endpoint cost catalogue, including `permissions/check` (used by `authorize()`).
  - Whether bridge traffic is exempt today in both products.
  - The partner-only quick reference guide.
  - Tier 2 criteria.
  - A developer-console points view.
  - Connect-on-Forge quotas.
  - Invocation window type.
  - The `rateLimitProperties` field name.
  - Credit rates for new models.
  - The GraphQL enforcement date.

  → All are STATED emulator choices. Never probe quotas on a live tenant (docs forbid it).
- **UI Kit.**
  - Where production UI Kit JS runs (inferred).
  - Host callback arguments.
  - Host handling of invalid xcss, unknown types and props, element props, empty Root, and reconcile bursts.
  - A real install/`du` size.
  - The `rateLimitProperties` max name.
  - Who can open `jira:adminPage`.
  - compass render.
  - Whether a payload may redistribute `@forge/*` (assume install from the registry).
  - ADS version parity.
  - No end-to-end offline render was executed.

  → Run a wolfaenpak probe app (callback args, ForgeDoc), a real `npm ci`+`du`, and grade semantics, not pixels.
- **Boot.**
  - Inline-script auto-hash in production.
  - Multi-entry chunk sharing and bridge injection into named entries.
  - Weekly upload quotas.
  - Real invoke latency and cold start.
  - The UI Kit runtime model.
  - The `ufo-perf-observers` flag.
  - The `__ready` handshake.
  - The full changelog history (largely closed this pass by the API).
  - The cache-API redirect.
  - CDN transfer sizes.
  - The status-page component.
  - Same-site vs cross-site iframe in the emulator (a design decision).
  - The B-thresholds are proposals.

  → Calibrate then freeze. Prove multi-entry on wolfaenpak or use one resource per surface.
- **Security.**
  - Admin-page resolver or FCT restriction.
  - Which `extension` fields are validated.
  - asApp vs issue security levels.
  - The RFC-141 ship date and enforcement.
  - The `FAIL_IF_EXISTS` SDK error.
  - Bugcrowd scope; Security Bug Fix Policy timeframes.
  - The doc page for `permissions.hasScope`.
  - No public Forge IDOR CVE.
  - No Forge-LLM injection guidance.
  - A bare `accountId` as personal data.
  - forge-skills contradictions (`route` "encodes" vs throws; SQL placeholders).

  → State the emulator semantics. Code and official docs beat the forge-skills rules.
- **LLM.**
  - Error statuses and codes.
  - Refusal pass-through.
  - The default `max_completion_tokens`.
  - Unknown fields accepted or rejected.
  - Stream chunking and `usage`.
  - 429 Retry-After at the HTTP level (invisible through the SDK anyway).
  - Credit rates for 5 models.
  - Credit price (resolved toward the pricing page).
  - `rovo:skill`/`rovo:mcp` dates.
  - When the sampling rules first appeared.
  - Whether `list()` counts toward RPM; window type.
  - Whether Rovo confirms every non-GET today.
  - Resolver 25 vs 55 s (pin 25).
  - Forge-LLM-in-action billing.
  - Whether a Studio agent can call every action.

  → State them all.
- **Robustness.**
  - Max concurrency and keys.
  - The retry schedule.
  - Ordering.
  - Scheduled overlap and catch-up.
  - Cold start.
  - Trigger timeout.
  - Log-cap overflow.
  - Job stats.
  - CES conditional on a missing key.
  - Query lag.
  - SQL transactions.
  - Whether async invocations count toward 300/s.
  - The pinned doc conflicts.

  → State them all.
- **This pass.**
  - `jira:customField` "Custom rendering currently can't be implemented using Custom UI or Frame component" was NOT
    found on the live page; the page shows `render: native` view examples. Staff confirm custom field view renderers
    are UI Kit-only for performance (boot#30).
  - The `apiRoute` request shape, the dynamic-modules page, the Feature Flags status page and the `reconcileIssues`
    sentence of `search/jql` were not re-verified.

---------------------------------------------------------------------------------------------------------------------

## 11. Forge 1.0 carry-over (DESIGN.md §2, §15, §16, §17.8)

### 11.1 §17.8 accuracy defects → binding rules for the 2.0 grader
| 1.0 defect | 2.0 rule |
|---|---|
| A. Emulator flags drawn inside the app, covering its buttons; the probe swallowed click failures | host chrome never overlays or intercepts app controls (flags outside the app frame or `pointer-events:none`); a click timeout is HARNESS evidence, never "the app made no call" |
| B. A critical "duplicate side effect" fired on zero comments, a missing flag or a 404 | a critical fires only on the defect it names (≥ 2 effects); absence is graded in its own row; the double-click target is a fresh, viewer-visible row |
| C. A consumer `resolver:` form (valid in the pinned schema) was never invoked | the emulator supports EVERY schema-valid form, or the kit lint refuses it with the real CLI's message |
| D. The contract said "a global publish reaches every subscription"; the emulator (correctly) paired `publishGlobal`↔`subscribeGlobal` | contract text = emulator behaviour, verbatim; Realtime `subscribe()` is module-context-scoped by default [B] |
| E. CES range index with > 1 attribute: the scorer gave the wrong reason; the real CLI refuses at lint and deploy (wolfaenpak, 2026-10-09) | the kit lint matches the current CLI (14.1.0); undeployable = the undeployable band |
| F. Economy step cliff at optimum+1 | continuous economy rows |
| G. Vacuous or manifest-fault roots charged twice | price each root once; vacuous rows pay nothing |
| H. `u_widget_live` gave 0.75 for "final numbers wrong"; DESIGN said polling = 0 but the code gave 0.5 | partial-credit rows measure the outcome they name; design text and code agree, enforced by a selftest |

Lesson from the trigger (Haiku 5.5 0.9661 vs Sonnet 5.5 0.5508): harness defects A and B, not knowledge, decided
the ranking. 2.0 needs a pre-freeze cross-model sanity review and mutants for every critical.

### 11.2 §15 risks, status for 2.0
- **R1** fence + provider relay: unchanged, MEDIUM.
- **R2** newest-surface fidelity: now ALSO the UI Kit host (closed renderer, §5), the points gateway (unpublished
  knobs, §6) and web-trigger ingress. Mitigation as in 1.0: quote a source for every emulated reply; a mismatch changes
  the emulator, never the contract.
- **R3** too hard for the call budget: higher in 2.0 because of the scope. Keep bands and partial credit, and decide
  cuts in advance.
- **R4** supply: the pinned wrapper and OpenAPI, plus `@forge/react`'s closure (registry install; licence question
  open).
- **R5** mock coverage: more endpoints (permissions, bulk ops, admin, Confluence if used). Keep the OpenAPI-driven 501
  + `held` verdict.
- **R6** Realtime and LLM emulation: unchanged, plus streaming and parallel tool calls.

### 11.3 §16 "not in v1": revival status on 2026-10-09
| item | 1.0 reason | now |
|---|---|---|
| UI Kit | "1.2 GB closure, no renderer" | closure ≈ 144 MB (≤ 306 MB disk); offline host FEASIBLE (MEDIUM) per §5 |
| `jira:jqlFunction` | handler contract not captured | contract captured and verified (precomputations, 1,000 values, 25 s, not per user) [B] |
| Forge SQL | needs a MySQL engine | GA [B]; still needs an engine; RFC-148 images unverified |
| `apiRoute` | contract unmeasured | Preview, off per site by default, 3LO [B]; request shape still undocumented |
| `global:fullPage` | contract unmeasured | Preview; `routePrefix` mandatory [B, uikit#21] |
| `jira:command` | unmeasured | Preview [B] |
| `objectStore` | unmeasured | Preview; major version; needs an emulated pre-signed host [B] |
| app-managed permissions | unmeasured | Preview; SDK `hasPermission` → `{granted, missing}` [B]; graceful-degradation tests are cheap (run the same build under two permission sets) |
| web trigger `hmacSharedSecret` | docs unconfirmed | tooling shipped, docs absent, enforcement unverified (sec#17) → app-implemented scheme |
| `rovo:mcp` | EAP | Preview (already used in 1.0) |
| `fifoConsumer` | EAP/undocumented | not shipped (RFC-107; rob#30) |
| `global:ui` | EAP | EAP, banned from production Marketplace apps → never |
| `dashboards:filter` | EAP | EAP (CHANGE-3485) → never |
| Confluence | a second mock product | unchanged cost; static macros now Preview (CHANGE-3505) |

### 11.4 §2.4 traps: still valid, or changed
- **Still valid and re-verified:** `temperature`/`top_p` (llm#16,17); stale model ids (llm#46); `publish()` not from
  async (Realtime page [B]); global-channel exposure; asApp for person-facing data; asUser in background work; N+1
  where `changelog/bulkfetch` exists (1000 issues, 10 fields, tiers#29); inline script and CDN font CSP failures
  (boot#7).
- **Changed:** `jira:dashboardGadget` is now formally deprecated, removal May 17, 2027 (CHANGE-3454).
  `dashboards:widget` requires `edit` [B]. Node 20 is blocked progressively (CHANGE-3209). The LLM "deprecated model"
  trap now has a real-world basis (Anthropic deprecated sonnet-4-5); usable only as a stated `list()` status.
- **Not re-verified in this pass:** `/rest/api/3/search` 410, ADF comment bodies, Agile endpoints (1.0 RESEARCH).

---------------------------------------------------------------------------------------------------------------------

## 12. Provenance
- **Topic notes and their independent verifications:** `research/{tiers,uikit,bootspeed,security,llm,robustness}.md`
  (all fetched and verified 2026-10-09). Verifier artefacts: `forge2/verify-{tiers,uikit,bootspeed,sec,llm,robustness}/`.
- **This pass:** `forge2/verify-brief/`.
  - `cl_0.json` and `cl_100.json`: 200 newest Forge changelog entries via
    `https://dac-changelogs.services.atlassian.com/changes?apiGroups=forge-core-platform,forge-jira-cloud-platform,forge-confluence-cloud,forge-jsw-cloud,forge-jsm-cloud,rovo-mcp,adopting-forge-from-connect`.
  - `cl_a.txt` and `cl_b.txt`: the flattened entries.
  - `pages/`: 30 doc pages fetched live.
  - `status.py` and `grepq.py`: banner and quote checks.
  - `cdac/`: threads 101559, 96705, 100451, 77751, 98403, 99654, 102177, 78982.
  - `eco1403.json`: the public tracker's status for ECO-1403.
  - The `@forge/bridge` 7.1.0 `invoke-endpoint.js` and `utils/index.js`, read from the verifier's tarball extract.
  - `@forge/api` 8.2.0 `out/safeUrl.js`, read from `research/pkgs/_forge_api`.
- **Earlier platform research:** `forge2/research-2026-10-09.md`. Only the items re-checked in this pass are used as
  facts; the rest is either confirmed by a topic verifier (cited) or listed as not re-verified.
- **Forge 1.0:** `evals/swarm-bench/forge/DESIGN.md` §2, §15, §16, §17.8 (read 2026-10-09).
