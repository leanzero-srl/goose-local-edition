# Real-Forge fidelity for Forge 2.0

Phase 1 (UNDERSTAND), real-Forge fidelity tooling. Measured 2026-10-09, 19:38–20:05 EEST, on the Mac Studio
(`workhorse`). The Forge CLI is `/usr/local/bin/forge`, which links to `@forge/cli` 14.1.0, installed 2026-10-09 16:34. Local
Node is v24.15.0. Logged in as the wolfaenpak test-site account.

The probe app is the throwaway `lz-range-probe`, `ari:cloud:ecosystem::app/ea75d1b1-757a-4613-a2bc-a4c379494602`,
in the LeanZero SRL developer space. Nothing else on wolfaenpak was touched. The app was installed for 2 minutes and
then uninstalled again.

**CPU discipline.** Every command ran under `nice -n 19`, one at a time. I did not run Chromium, Playwright, the
scorer, pytest, cargo or any npm build. All the local work together cost about 2 minutes of one core. Load
averaged 4–12 while a paid run was building, and was 2.9 when I finished.

**Work directory.** `scratchpad/forge2/understand/fidelity-work/` holds the scripts, the corpus, the probe app and
every raw output. The file paths are listed in the appendix.

---

## TL;DR

1. **`forge lint` cannot run offline.**
   - It needs a login and an app id.
   - Its server half must succeed, or the whole command fails with no client results either. The server half is
     the GraphQL `createAppManifestUploadUrl` mutation, then an upload of the manifest, then the
     `appPreDeploymentCheck` query.
   - `HTTPS_PROXY`/`HTTP_PROXY` are ignored: lint still reached Atlassian. `FORGE_PROXY` and a deny-network
     `sandbox-exec` both end it with `Error: fetch failed`.

2. **The kit's offline lint is already the CLI's client half, byte for byte.**
   - `@forge/lint` 6.3.0, `@forge/manifest` 13.6.0 and `@forge/cli-shared` 9.7.0 hash-identical. So does the
     manifest schema (sha256 `d36082ad1a59…`).
   - The two-range refusal came from the server half, rule `MANIFEST_INVALID_RULE`. No local package contains
     that text.
   - So "make the kit lint match the CLI" is **not** a package bump. Three things are missing:
     1. the server rules;
     2. one live feature-flag value: the list of deprecated runtimes;
     3. CLI-equal file collection.

3. **I ran 11 manifests through the real server check.**
   - **6 server-only refusals that the kit accepts:**
     - two range attributes on one index;
     - an index name shorter than 3 characters;
     - an index name longer than 50 characters;
     - an index name with a character that is not allowed;
     - more than 20 entities;
     - **`nodejs20.x`, which is now a hard ERROR**.
   - **1 client warning that the kit drops:** the `nodejs20.x` deprecation warning.
   - **3 documented constraints that the server does not enforce at lint:**
     - duplicate index names;
     - an `any`-typed range attribute;
     - an `any`-typed partition attribute.

4. **The one end-to-end live measurement is a KVS/Custom Entity semantics probe.**
   - The loop was deploy, install, webtrigger, `curl`, `forge logs`, uninstall. It took about 4 minutes and
     about 10 s of CPU.
   - I then ran the **same probe source inside the 1.0 kit emulator**. **13 of 24 answers were identical; 11
     differed.** In those 11, 4 HTTP statuses were wrong, 7 error codes were wrong, and every error message
     differed.
   - The 1.0 alt golden app caught the FAIL_IF_EXISTS conflict by `err.code === 'CONDITIONAL_CHECK_FAILED'`.
     Real Forge answers `409 KEY_CONFLICT` instead, so only that app's `/already exists/` message fallback
     would have saved it.

5. **Recommendation: scoring stays offline**, so anyone who installs Goose can still score. Real Forge becomes
   the thing that calibrates the offline scorer, in four layers:
   - **(L0)** Offline lint = the CLI's client half, plus the pinned flag value, plus a **measured server-rule
     pack**.
   - **(L1)** The emulator is held to real Forge by **differential conformance**: the same probe source runs live
     and in the emulator, and any difference fails the kit test.
   - **(L2)** Real deploys are only a **golden gate at freeze time** and an **audit lane for our own runs**.
   - **(L3)** Browser calibrations, such as Custom UI boot time and realtime delivery, run in quiet windows only.
   - Nothing on real Forge is a score input.

---

## 1. Does `forge lint` run fully offline? No.

### What the code says (CLI 14.1.0)

**The mode is fixed.**
- `out/command-line/controller/lint-controller.js` calls the lint service with
  `{ fix, approveRules, linterMode: 'both' }`, hard-coded.
- `register-lint-command.js` offers only `--fix`, `--approve <rule...>` and `-e`, plus `.requireAppId()`.
- `command.js` uses `requiresAuthentication ?? true`, so credentials are always needed.

**One failing linter kills the whole run.**
- In `@forge/lint/out/lint/lint.js:166`, `await Promise.all(linters.map((l) => l.bootstrap()))` runs all
  bootstraps together.
- `ServerSideLinter.bootstrap()` zips **only the interpolated `manifest.yml`**, asks GraphQL for an upload URL,
  and uploads it.
- When that rejects, `lint()` throws `ApplicationCompileError`. The client linters' results are thrown away, and
  no partial report is printed.

**What the server half does.**
- `batchExecuteImpl` sends `appPreDeploymentCheck(input: {appId, environmentKey, manifestUrl, bypassRules,
  buildTag, majorVersion})`.
- It maps every outcome `{rule, category, value, reason}` to a finding at `manifest.yml 0:0`, with
  `reference = rule`.
- The source code is never uploaded.

**Network inputs that even the client half uses:**
- The feature-flag config `xls-forge-cli-deprecated-runtimes`.
  - It comes from `POST https://api.atlassian.com/flags/api/v2//configurations` (`cli-shared/out/service/statsig-service.js`).
  - If the fetch fails it silently becomes `[]`. That failure is logged only with `--verbose`.
- The PermissionLinter downloads 6 OpenAPI files from developer.atlassian.com and caches them for 12 h
  (`permission-linter.js:166-173`), unless `USE_LOCAL_SWAGGER` is set.

**Environment hooks the CLI reads:**
- `FORGE_PROXY`, in `command-line/proxy.js`; the standard `*_PROXY` variables are ignored.
- `FORGE_GRAPHQL_GATEWAY`, in `cli-shared/out/graphql/gateway.js:6`.
- `FORGE_EMAIL` / `FORGE_API_TOKEN`, in `auth/personal/credential-store.js`.

### Measured

All four runs used the receipt app `range-probe/app`, whose index has two range attributes.

**The deny-network profile** is `fidelity-work/deny-net.sb`, the same shape as `bench_isolation.py`'s fence:

```
(version 1) (allow default) (deny network-outbound)
(allow network-outbound (remote ip "localhost:*")) (allow network-outbound (remote unix-socket))
```

**Control for the profile:** inside it, `curl https://api.atlassian.com/` gave `(7) Failed to connect … 000`.
Outside it, the same call gave `301`.

| # | command (in the app dir) | wall / user CPU | result |
|---|---|---|---|
| A | `nice -n 19 forge lint -e development` | 7.46 s / 5.61 s | exit 1 — `manifest.yml 0:0 error Storage entity named index must include exactly one range attribute.  MANIFEST_INVALID_RULE` |
| B1 | `HTTPS_PROXY=http://127.0.0.1:9 HTTP_PROXY=… https_proxy=… http_proxy=… forge lint -e development` | 6.27 s / 5.94 s | **identical to A**: the proxy variables are ignored and the server answered |
| B2 | `FORGE_PROXY=http://127.0.0.1:9 forge lint -e development` | 4.10 s / 5.48 s | exit 1 — `Error: fetch failed`, **no findings at all** |
| C | `sandbox-exec -f deny-net.sb forge lint -e development` (`--verbose` for the second capture) | 4.02 s / 5.35 s | exit 1 — `Error: fetch failed` / `Error: connect EPERM 34.160.81.0:443`. The verbose run shows `Failed to fetch dynamic configurations: TypeError: fetch failed`, then `▶️ GraphQL https://api.atlassian.com/graphql  mutation forge_cli_createAppManifestUploadUrl … "environmentKey": "default"`, and dies there |

Raw outputs: `lint-sandbox-plain.txt`, `lint-sandbox-verbose.txt`. Note that `-e development` resolves to
`environmentKey: "default"` on the wire.

**The only offline lint is the library call the kit already makes.** That is
`lint(files, manifest, env, logger, statsigStub, {linter:{mode:'client-side'}})` in `kit/bin/lint.cjs`.

---

## 2. What implements CLI lint, compared with the 1.0 kit

### 2.1 Packages: versions, hashes, sizes

Hashes are sha256 over every file except `*.map`, truncated to 16 hex characters.

| package | CLI 14.1.0 (`/usr/local/lib/node_modules/@forge/cli/node_modules/…`) | kit `lint-modules/node_modules/…` | size (CLI copy) |
|---|---|---|---|
| `@forge/lint` | 6.3.0 `53d2a0b6de0e4d33` | 6.3.0 `53d2a0b6de0e4d33` | 1.3 MB |
| `@forge/manifest` | 13.6.0 `1262370a1704e1bb` | 13.6.0 `1262370a1704e1bb` | 5.7 MB |
| `@forge/cli-shared` | 9.7.0 `ac1e983520f7b6b4` | 9.7.0 `ac1e983520f7b6b4` | 6.0 MB |
| `@forge/egress` / `util` / `i18n` | 3.0.0 / 3.0.0 / 1.0.0 | identical hashes | — |
| manifest schema | `@forge/manifest/out/schema/manifest-schema.json` | `schema/manifest-schema.json`, 2,295,735 B | sha256 both `d36082ad1a59fa92e2e8…` |

**Tree sizes.**
- The whole CLI `node_modules` is 336 MB. It also bundles `@forge/bundler` 7.2.3 (webpack 5.109.2 + babel),
  `runtime` 7.0.0, `tunnel` 7.2.0 and `csp`.
- The kit's `lint-modules` tree is 172 MB. It is built with `npm ci --ignore-scripts` from the committed
  lockfile, and pins `@forge/cli-shared` 9.7.0, `@forge/lint` 6.3.0, `@forge/manifest` 13.6.0 and `yaml` 2.9.1.

**KIT.json** gives:
- `kit_lock_sha256` `825be630…`
- `kit_code_sha256` `ad1a414e…`
- the wrapper sha `4f8170e0…`, which is the runtime-pin wrapper of 2026-09-29.
- `app_modules`: `@forge/api` 8.2.0, `bridge` 7.1.0, `dashboards-bridge` 2.0.0, `events` 3.0.7, `hooks` 2.0.0,
  `kvs` 2.0.7, `llm` 1.0.7, `realtime` 1.0.1, `resolver` 2.0.0, `esbuild` 0.28.2, `react`/`react-dom` 18.3.1.

**Client linters (16), identical in both because the packages are identical.** These come from
`getClientSideLintersOnly`:
1. Permission
2. PermissionsManifest
3. AgentProductContextPermissionsManifest
4. DeprecatedCspPermissionsManifest
5. FullManifest (the schema plus the `@forge/manifest` validators, e.g. StorageValidator)
6. Handler
7. DynamicPropertiesPermissions
8. InvokeEndpoint
9. StorageModules
10. FrameComponent
11. LlmModule
12. DeprecatedApiModule
13. DeprecatedEgressPermissionsManifest
14. AppManagedPermissionsSdk
15. TeamworkGraph
16. FunctionTimeout

### 2.2 Why the kit missed the range rule: the server half

`grep -r "exactly one range attribute"` over the whole CLI install gives **0 hits**.

The client `StorageValidator` (`@forge/manifest/out/validators/storage-validator.js`) checks only these:
- at most 50 attributes;
- attribute names of at most 30 characters;
- at most 7 indexes;
- the reserved index name `by-key`;
- range and partition attributes must be declared.

The schema allows `range: {minItems: 1}` with no maximum.

The server's rule set is typed in `cli-shared/out/graphql/gql/graphql.d.ts:2893-2894`:

```ts
export type AppPreDeploymentCheckRuleCategory = 'APPROVAL' | 'ERROR';
export type AppPreDeploymentCheckRules = 'MAJOR_VERSION_RULE' | 'MANIFEST_INVALID_RULE' | 'SYSTEM_USER_CHANGE_RULE';
```

- `MANIFEST_INVALID_RULE` carries **free-text** server-side manifest validation. The range rule is one of those
  texts, so the only way to enumerate them is to probe.
- `MAJOR_VERSION_RULE` and `SYSTEM_USER_CHANGE_RULE` are approvals, which `forge deploy --approve <rule>`
  acknowledges.

### 2.3 Measured: the corpus through the real server check

Script: `scripts/lint_corpus.py`. Results: `lint-corpus-results.jsonl`. Manifests: `corpus/vNN-*/`.

**How each variant is built.**
- Each is a minimal app: a webtrigger, one function, and one entity `scope-change`.
- The entity has attributes `sprintId`, `at` and `changeId` (strings) and `blob` (`any`).
- It has one index `by-sprint`, with partition `[sprintId]` and range `[at]`.
- The manifest is written as JSON-in-YAML, so the YAML library played no part.

**Per-variant cost:** the CLI took 5.7–9.0 s and the kit lint 3.2–3.7 s.

| variant | real `forge lint` (client + server) | kit `bin/lint.cjs --json` |
|---|---|---|
| v00 control (valid) | exit 0 "No issues found." | exit 0, 0/0 |
| v01 `range: [at, changeId]` | **ERROR** `Storage entity named index must include exactly one range attribute.  MANIFEST_INVALID_RULE` | exit 0, 0/0 |
| v02 index name `bs` (2 chars) | **ERROR** `Storage entity index name is too short.` | exit 0 |
| v03 index name `by sprint` | **ERROR** `Storage entity index name contains non-allowed characters.` | exit 0 |
| v04 two indexes both named `by-sprint` | pass (the docs say index names must be unique) | exit 0 |
| v05 `range: [blob]` (type `any`) | pass (the docs say "all data types except any") | exit 0 |
| v06 `partition: [blob]` (type `any`) | pass | exit 0 |
| v07 21 entities | **ERROR** `Your app exceeds the maximum number of custom entities allowed: 20` | exit 0 |
| v08 `runtime: nodejs20.x` | **client WARNING** `22:15 The nodejs20.x runtime is deprecated and will be removed in the future…  deprecated-property`, plus **server ERROR** `The nodejs20.x runtime is deprecated. Migrate your app to the latest Node.js runtime: https://go.atlassian.com/runtime  MANIFEST_INVALID_RULE` | exit 0, 0 warnings |
| v09 string-form index `['at']` | pass | exit 0 |
| v10 index name of 51 characters | **ERROR** `Storage entity index name is too long.` | exit 0 |

**What the rules diff contains:**
- **6 server rules the kit lacks**: v01, v02, v03, v07, v08, v10.
- **1 client warning the kit lacks**: v08.
- **3 documented constraints that are not refused at lint** (v04–v06). Scoring must not charge these as
  undeployable on the docs alone. Whether the deploy step refuses them is untested; that is follow-up 4.
- Each variant was run once. Server validation of an identical manifest is presumably deterministic, but that
  has not been checked twice.

### 2.4 Other kit-vs-CLI differences, besides the server half

**(a) The feature-flag value.** I fetched the live configuration with the CLI's own request shape: the namespace
`forge_cli`, the CLI's embedded PROD client key (`statsig-service.js:84`), and our account and app id. It
returned:

```
xls-forge-cli-deprecated-runtimes => {"value": ["sandbox", "nodejs18.x", "nodejs20.x"]}
```

The kit stubs `getDeprecatedRuntimes: async () => []`. The CLI's offline fallback is also a silent `[]`.

**(b) The OpenAPI files** that the PermissionLinter maps calls to scopes with. I compared the kit's pinned copies
(2026-10-02) with the live downloads using `scripts/openapi_drift.py` (output in `openapi-drift.txt`):
- `conf`, `confv2` and `jsw` are byte-identical.
- `jira` and `bb` differ in bytes but have the **same operations and the same scopes**.
- `jsm` lost `POST /rest/servicedeskapi/request/skip-login-check`.
- This is negligible today, but the input moves, so it needs a drift check.

**(c) How the files to lint are chosen.**
- **CLI** (`LintService.run`): every file under `./src`, plus the directories of UI Kit (`render: native`)
  resources, minus `.gitignore`d files, `.git` and `node_modules`. `lint()` then parses any extension matching
  `/jsx?$/`, which includes **`.mjs` and `.cjs`**, and `ts`/`tsx` per the tsconfig include/exclude.
- **Kit** (`walk('src')`): only `src`, matching `\.(jsx?|tsx?)$`. It **skips `.mjs` and `.cjs`** and
  dot-directories. It ignores `.gitignore` and UI Kit resource directories outside `src`.
- The CLI reads the manifest through `ConfigFile.readConfig()`, which interpolates variables. The kit uses
  `YAML.parse`.
- The CLI passes the resolved environment. The kit passes `'development'`.

**(d) Version drift.** Atlassian can ship a new CLI at any time. Today CLI 14.1.0 equals the kit's pins.

### 2.5 Could the kit pin the CLI's lint offline?

**The client half is already pinned.** Bumping packages fixes nothing. What closes the gap offline:

1. **A server-rule pack.**
   - Each `MANIFEST_INVALID_RULE` text measured through the real check becomes a predicate over the parsed
     manifest.
   - Each predicate keeps the corpus manifest that triggers it and a receipt: the date, the CLI version and the
     message.
   - Its finding is `{sev: 'error', reference: 'MANIFEST_INVALID_RULE', provenance: 'server-measured 2026-10-09'}`.
2. **The flag value pinned as a dated receipt**, `["sandbox","nodejs18.x","nodejs20.x"]`, instead of `[]`.
3. **CLI-equal file collection**, mirroring `LintService.run`.
4. **A drift job, run online by us.** It re-runs the corpus through the real `forge lint`, re-fetches the flag
   value and the OpenAPI files, and compares the latest `@forge/cli` dependencies with the kit lock.

**Option K2 (not verified):** vendor the real CLI and run it offline. Three environment settings would do it:
- `FORGE_GRAPHQL_GATEWAY=http://127.0.0.1:<stub>`, with the stub answering `createAppManifestUploadUrl` and
  `appPreDeploymentCheck` from the rule pack;
- dummy `FORGE_EMAIL` and `FORGE_API_TOKEN`;
- `FORGE_PROXY` pointing at a stub that serves the flags.

K2 gives exact parity in which files get linted and in the output format. But it costs 336 MB and about 6 s of
CPU per lint, against 3.5 s for the kit, and it still needs the rule pack.

---

## 3. Platform behaviours measurable live on wolfaenpak

### 3.1 The one measurement proven end to end: KVS / Custom Entity semantics

**The probe app** is `fidelity-work/kvs-probe-app/`. It has the same app id as `lz-range-probe`.
- Its `manifest.yml` has a webtrigger `probe-wt` (`urlFormat: v2`, `response.type: dynamic`) and a function
  `probe-fn`, handler `index.kvsProbe`.
- It has an entity `scope-change` (`sprintId`/`at`/`changeId` string, `n` integer) with index `by-sprint`
  (partition `[sprintId]`, range `[at]`), and the scope `storage:app`.
- `node_modules` is an APFS clone of the kit's `app-modules`, so `@forge/kvs` 2.0.7 is exactly what entrants get.
- `src/index.js` runs 24 cases in one invocation and returns `{status, code, message, ms}` for each. It also
  `console.log`s one line per case.

**The loop: commands, outputs and times.**

1. **Deploy.** `nice -n 19 forge deploy -e development --non-interactive` (16:52:34Z) printed:
   - `Running forge lint... No issues found.` / `ℹ Packaging app files` / `ℹ Uploading app` /
     `ℹ Validating manifest` / `✔ Deployed`.
   - `The version of your app [2.0.0] … is not eligible for the Runs on Atlassian program` (because of the
     dynamic webtrigger).
   - Time: **36.1 s wall, 6.1 s user CPU**.
2. **Index status.** `forge storage entities indexes list -e development` showed `by-sprint` **ACTIVE** (sprintId
   / at) and `by-key (default)` **ACTIVE**.
3. **Install.** `forge install --site wolfaenpak.atlassian.net --product jira -e development --non-interactive
   --confirm-scopes` printed `✔ Install in Jira complete!`. Time: **51.2 s wall, 1.2 s CPU**.
   - Without `--confirm-scopes`, non-interactive mode would stop at the scope prompt.
   - The CLI prints a "development app to a production site" warning; wolfaenpak is a sanctioned test site.
4. **Webtrigger URL.** `forge webtrigger create -f probe-wt --site wolfaenpak.atlassian.net --product jira -e
   development` returned `https://<installation-uuid>.webtrigger.atlassian.app/public/<id>`.
5. **Invoke.** `curl -X POST "$URL/kvs?trial=1" -d '{"probe":"kvs"}'`:
   - first call: **HTTP 200 in 4.55 s** (cold), function `totalMs` 2834;
   - repeat: **1.87 s**, `totalMs` 1531, **identical outcomes in all 24 cases**.
6. **Logs.** `forge logs -e development -n 60 --since 15m` printed all 24 `KVSPROBE …` lines under invocation
   `45bd23c5-af2d-4a1e-8691-bafe9fababd3`. They were available at most 16 s after the call; the command took 3.4 s.
7. **Uninstall.** `forge uninstall --site wolfaenpak.atlassian.net --product jira -e development` printed
   `✔ Uninstalled` in 64.4 s. `forge install list` then said "The app is not installed anywhere". The dead webtrigger now
   answers **HTTP 424 Failed Dependency** rather than 404.

**Loop total: about 4 minutes wall and about 10 s of CPU.** One extra probe run on an installed app costs 2–5 s.

**Differential check.** I ran the identical probe source inside the 1.0 kit emulator with
`scripts/emu_kvs_probe.cjs` (Atlassian's pinned wrapper, `kit/lib/kvs.cjs` behind the proxy, sandbox fence). It
took 0.3 s. The comparison is in `kvs-live-vs-emulator.txt`.

| case | REAL Forge (wolfaenpak) | 1.0 emulator | same? |
|---|---|---|---|
| setup set / entity set, both FAIL_IF_EXISTS controls, both passing transactions, all four get/readback checks, 25-op transaction, 25-key batchSet, get/delete/tx-delete of missing keys (13 cases) | ok, same values | ok, same values | **yes** |
| 01 `set(k, v, {keyPolicy:'FAIL_IF_EXISTS'})` on an existing key | **409 `KEY_CONFLICT`** "Provided key already exists and cannot be overwritten" | 409 `CONDITIONAL_CHECK_FAILED` "Key '…' already exists." | code no |
| 03 transaction whose check condition is false | **400** `CONDITIONAL_CHECK_FAILED` "Request failed due to conditional check specified or optimistic locking"; atomic (the set did not land) | **409** `CONDITIONAL_CHECK_FAILED`; atomic | status no |
| 05 transaction check on a missing entity key | **400** `CONDITIONAL_CHECK_FAILED` (same text); atomic | 409 | status no |
| 06 transaction with 26 operations | **422 `UNPROCESSABLE_ENTITY`** "Request cannot be processed due to one or more semantic errors" | 400 `TOO_MANY_OPERATIONS` | status + code no |
| 07 the same key twice in one transaction | 400 **`KEY_DUPLICATION_ERROR`** "Duplicate key found in request" | 400 `DUPLICATE_KEY` | code no |
| 08 `batchSet` of 26 items | 400 **`TOO_MANY_BATCH_ENTITIES`** "Number of entities to set was 26, but you can only set a maximum of 25 entities at a time." | 400 `MAX_BATCH_SIZE` | code no |
| 09 `batchGet` of one existing + one missing key | ok; `failedKeys: [{key, error:{code:'KEY_NOT_FOUND', message:'Provided key does not exist'}}]` | ok; same code, message "Key '…' not found." | message only |
| 10 `entity('not-declared').set` | **404 `SCHEMA_NOT_FOUND`** "The schema provided does not exist" | 400 `INVALID_ENTITY_TYPE` | status + code no |
| 11 `ttl {value: 0}` | 400 `INVALID_TTL` "TTL value must be a positive integer, received: 0" | 400 `INVALID_TTL` (other text) | message only |
| 12 integer attribute = 2^31 | 400 **`INCORRECT_PROPERTY_TYPE`** 'Data type for property "n" is defined as "integer"' | 400 `INVALID_ENTITY_VALUE` | code no |
| 12b integer attribute = "5" | 400 `INCORRECT_PROPERTY_TYPE` (same text) | 400 `INVALID_ENTITY_VALUE` | code no |

**Score: 13 of 24 identical.** Of the 11 that differ, 4 have the wrong HTTP status (03, 05, 06, 10), 7 have the
wrong error code (01, 06, 07, 08, 10, 12, 12b), and every error message differs.

**What the emulator gets right:**
- transactions are atomic;
- `get` of a missing key returns `undefined`;
- `delete` of a missing key is a no-op;
- the 25-operation and 25-key limits exist;
- `INVALID_TTL` and the `KEY_NOT_FOUND` code in batch results.

**Scope of what was measured:**
- Case 01 was measured on a plain key. FAIL_IF_EXISTS on an entity `set` was not probed, though it is likely the
  same.
- The request object in the emulator run was my hand-made stub. The 1.0 emulator has no webtrigger path at all.

**Why it matters for scoring.**
- An app written against real Forge, testing `e.code === 'KEY_CONFLICT'`, is treated as broken by the emulator.
- An app testing `'CONDITIONAL_CHECK_FAILED'` passes the emulator and fails on Forge.
- The 1.0 alt golden app does exactly the latter (`bench/golden-forge-alt/src/store.js:39`). Its
  `/already exists/i` message fallback is the only reason it would survive real Forge, and
  `ALT-NOTES.md` §1.5 recorded the guess.
- 2.0 will lean on optimistic concurrency and robustness, so these codes become graded behaviour.

**Also observed:**
- The live webtrigger request carries `body, call, context, contextToken, headers, method, path,
  queryParameters, userPath`. The docs do not name `call`, `context` or `contextToken`.
- The live runtime is Node **v24.21.0**. The kit runs Atlassian's wrapper on the local **v24.15.0**.
- The first KVS call took 845 ms cold and 402 ms on the repeat. Later calls took 25–280 ms.

### 3.2 What else can be measured, and how

All of these use the same throwaway app: one webtrigger router keyed on `userPath`, one consumer whose behaviour
the event body selects, and a Custom UI page for the browser legs. "Wall" means time to run once the probe is
built. CPU is negligible except for deploys, at about 6 s each.

| behaviour | measurable? | probe sketch | wall | browser? | emulator assumption it settles |
|---|---|---|---|---|---|
| KVS/CE error codes, atomicity, limits | **done** | §3.1 | 2–5 s per run | no | the UNCONFIRMED list in the `kvs.cjs` header |
| KVS query semantics (index range conditions, sort, cursor, `limit` default 10 / max 100 → what at 101), TTL expiry granularity | yes | `/kvs-query`: seed N rows, query; TTL 60 s, read at +30/+65/+120 s | 5 min | no | query paging in `kvs.cjs` |
| KVS optimistic locking under concurrency | yes | push 50 events; the consumer transacts with a check on one shared key; count `CONDITIONAL_CHECK_FAILED` (the real message names "optimistic locking") | 10 min | no | the emulator drains serially, so it can never produce this |
| KVS rate limits (1000 RPS, 4000 units of 10 KB per minute; transactions fail with `TOO_MANY_REQUESTS`) | partly | consumer write burst; record status and code | 10 min | no | not modelled |
| Queue retry schedule after a throw or timeout | yes | consumer throws on attempts 0–4 and logs `retryContext` plus arrival time; read intervals from `forge logs` | about 35–40 min (waits of 1+2+4+8+15 min) | no | `REDELIVERY_MINUTES [1,2,4,8]` then 15 (`emulator.cjs:36`) |
| `retryContext.retryReason` values | yes | same probe | — | no | **suspect**: the emulator sets `'FUNCTION_ERROR'`/`'FUNCTION_TIMEOUT'` (`emulator.cjs:302`), but `@forge/events` 3.0.7's `InvocationErrorCode` has `FUNCTION_TIME_OUT` and no `FUNCTION_ERROR` |
| `InvocationError` `retryAfter` clamp, 4 KB `retryData` | yes | return `new InvocationError({retryAfter: 2000})`, time the redelivery; send 5 KB of retryData | 15–20 min | no | `MAX_RETRY_AFTER_S = 900` |
| Delivery concurrency, `concurrency {key, limit}`, duplicates, ordering | yes | push 50 events with and without `concurrency`; consumer logs start and end and sleeps 10 s; compute overlap; count deliveries per eventId | 10 min | no | serial drain (`drainQueues`) |
| Push limits (50 per request, 500 per minute, 200 KB, `delayInSeconds`) | yes | push 51 events, then 201 KB, then a 60 s delay; record error codes and delay accuracy | 5 min | no | 413 for long-running payloads is a guess |
| Invocation timeouts (webtrigger 55 s; consumer 55 s default and `timeoutSeconds`; scheduled) | yes | functions that log every second until killed; record what the webtrigger caller sees; a consumer with `timeoutSeconds: 120` | 15 min | no | `TIMEOUTS` |
| Resolver 25 s timeout, front-end payload limits (500 KB / 5 MB) | yes | a Custom UI `invoke` on a real Jira page | 30 min | **yes** | — |
| Realtime from an async consumer: `publish()`, `publishGlobal`, `signRealtimeToken` | partly now | the consumer calls each and logs `PublishResult`. With no subscriber, eventId/eventTimestamp are documented to be null, so delivery itself cannot be seen | 10 min | delivery leg yes | `site/realtime.cjs` harness choices |
| Realtime delivery, `publish`↔`subscribe` pairing, payload parsing | yes | a Custom UI page subscribes (`subscribe` and `subscribeGlobal`); the consumer publishes | 1 h | **yes** | DESIGN §17.8 D |
| `@forge/llm` validation (temperature + top_p together; sampling on opus-class models) | yes | `/llm` with the invalid combinations; record `ForgeLlmAPIError {status, code, message}`. Needs an `llm` module; token cost is cents | 15 min | no | `llm.cjs`: "status code and code strings … not documented (harness choice: 400)" |
| `@forge/llm` `list()` statuses, how stream splits tool arguments, refusal `finish_reason`, usage | yes | one streamed tool call; record the raw chunks | 15 min | no | `llm.cjs` R6 |
| `@forge/llm` rate limit (100 requests per minute per installation) | yes, costs tokens | 101 tiny prompts in one minute | 5 min | no | — |
| Webtrigger shapes, static `outputs`, 429 headers, after-uninstall status | partly done (request shape, 424) | `outputKey` mapping; a parallel `curl` burst above 300 per second for `429` + `X-Ratelimit-Reset` | 10 min | no | the 1.0 emulator has no webtrigger path |
| App-managed permissions (`permissions.enforcement: app-managed`, Preview) | partly | lint/deploy acceptance and Permissions SDK answers can run headless. Revoking an optional scope needs the admin UI (Playwright with the harness admin profile) | 1–2 h | revoke leg yes | only `AppManagedPermissionsSdkLinter` today |
| Jira REST rate limits ("tier 1" dosing, mandate item 9) | partly | a consumer burst of `requestJira` GETs; record `429` plus `Retry-After` / `X-RateLimit-*` / `RateLimit-Reason`. Per-second burst limits are reachable; hourly quotas probably not, short of abuse | 15 min | no | `site/limits.cjs` (doc values 20 writes / 2 s, 100 / 30 s) |
| Custom UI boot time on real Jira (mandate item 6) | yes | forge-live-harness profile; time navigation → iframe → bridge ready → first `invoke` → first paint | 1–2 h | **yes** | needed to calibrate 2.0's local boot metric |
| Runs on Atlassian eligibility | yes | `forge eligibility -e development` on a deployed version (deploy already prints "not eligible" for a dynamic webtrigger) | 1 min per version | no | not modelled |
| Deploy-time manifest validation beyond lint (v04–v06) | yes | deploy each variant | about 40 s each | no | — |

**Totals:**
- Building the whole probe set: about 1.5–2 days of agent work.
- One full live calibration pass: about 1.5–2 h wall, mostly waiting on retries, with CPU under 5 min.
- The browser legs need a quiet machine and must never overlap a scoring window.

---

## 4. Recommended fidelity architecture for 2.0

**The constraint that decides it.**
- The published score is computed by the Goose desktop app on whoever runs the benchmark. That machine has a
  fenced network and no Atlassian credentials. The owner's rule is that anyone who installs Goose gets the same
  benchmark.
- Real Forge as a score **input** would therefore need one of two things:
  - every user has a Forge login and a registered app; or
  - a LeanZero-hosted oracle runs untrusted manifests or code on our Atlassian account.
- Both make Atlassian's availability and rate limits part of every score. The oracle also turns our account into
  an execution service for arbitrary entrant code.
- **So real Forge decides what the offline scorer believes, and the offline scorer decides the score.**

| layer | graded by | when | cost |
|---|---|---|---|
| **L0 Deployability (lint + manifest)** | **Offline at score time:** the CLI's client half (pinned, byte-identical), plus the pinned flag value (deprecated runtimes), plus the **measured server-rule pack**, plus CLI-equal file collection. Every finding states where it came from: `client`, `server-measured <date>`, or `docs-only (not charged)`. | every score; calibrated before each freeze, plus a weekly drift job | score time: about 3.5 s per tree (today's kit lint). Calibration: about 6.5 s per corpus manifest through the real `forge lint`, so about 5 min for about 40 manifests covering every 2.0 module and storage feature. Drift job about 1 min. |
| **L1 Platform semantics** (KVS/CE, queue, timeouts, webtrigger, LLM validation, realtime publish, rate limits) | **The calibrated emulator.** The probe sources form a **conformance suite** that runs live (answers committed as fixtures with date, CLI, runtime and invocation ids) and in the emulator (a kit test). **Any difference fails the build.** A score row may depend only on behaviour that has a live receipt, or a doc quote no live run contradicts. A harness choice (undocumented and unmeasured) may not decide a row, unless the row accepts every plausible reading. | every score uses the emulator; the live suite runs before each freeze | live pass about 1.5–2 h wall, under 5 min CPU; the emulator replay of the suite takes seconds |
| **L2 Real deploy** | **Never a score input.** Used two ways. **(a) Real-Forge golden gate at freeze:** the golden and alt apps deploy, install and pass a scripted smoke test on wolfaenpak. Every *platform-semantics* mutant (a race between parallel consumers, a wrong KVS code) must also misbehave on real Forge; otherwise the mutant measures the emulator, not Forge. **(b) Audit lane for our own baseline runs before publishing:** run `forge deploy -e development --non-interactive` on the entrant's final tree into a dedicated audit app (app.id rewritten, deploy only, no install), and compare accept/refuse and its messages with the scorer's M7 deploy verdict. A mismatch blocks publishing until the rule pack is fixed, then the build is re-scored in the app. | (a) once per freeze; (b) per baseline run, outside scoring windows | deploy about 36 s wall / 6 s CPU measured on a tiny backend app. A 2.0 app with Custom UI builds will be larger: estimated 1–3 min and 20–60 s of webpack CPU, **not measured**. Install about 51 s, uninstall about 64 s. |
| **L3 Browser calibrations** (Custom UI boot time, realtime delivery, resolver timeouts, app-managed-permission revocation) | forge-live-harness on wolfaenpak in a quiet window. The outputs are calibration constants with receipts. For example, the local boot-speed metric must rank the golden app against a deliberately slow mutant the same way real Jira does. | before freeze | about 2–4 h to build once, 15–30 min per pass, Chromium CPU (**never during scoring**) |

**Not recommended for 2.0:**
- **A real deploy as a score input for everyone.** The reasons are above, plus two more: about 2–4 min per tree,
  and it would break scoring inside the fenced network.
- **A hosted manifest-only oracle.** The server check needs only the manifest, takes about 2–3 s, and the source
  never leaves the machine. It is the safest way to put the real platform in the score loop. But it still adds a
  network dependency, and it does not cover runtime semantics. **Better:** keep it as the L0 calibration and
  drift tool, and pre-measure every rule the 2.0 contract can trigger.

---

## 5. Follow-ups, ranked by correctness risk (not by effort)

1. **Align `kit/lib/kvs.cjs` with the 11 measured differences (§3.1), and make this probe the first conformance
   test.** Confidence is high: two identical live runs and a byte-identical source run in both places. This
   changes behaviour for 1.0 entrants that check codes, so it belongs in the 2.0 kit (1.0's benchmark is frozen).
2. **Kit lint for 2.0.** Add the server-rule pack (v01, v02, v03, v07, v08, v10 so far), the pinned runtime
   deprecations (`nodejs20.x` is now a **hard deploy refusal**) and the CLI-equal walk. Extend the corpus to
   every 2.0 module, then re-run it at freeze. Confidence is high for the six measured rules. Enumerating the
   rest by probing is open-ended, so coverage is only as good as the corpus.
3. **Queue probe** (retry schedule, `retryReason` values, concurrency, duplicates). Both the serial drain and the
   `FUNCTION_ERROR`/`FUNCTION_TIMEOUT` reasons are likely fidelity holes, and 2.0's robustness checks will sit on
   them. Confidence is medium until measured; it costs about 40 min of mostly idle wall time.
4. **Deploy-test v04, v05 and v06.** This settles whether "docs say so" or "server accepts" is right before any
   2.0 rule cites the docs. About 2 min.
5. **The `@forge/llm` validation and stream probe**, about 30 min plus cents of tokens. Before building it,
   confirm that Forge LLM is enabled for the developer space and site.
6. **The browser legs** (boot time, realtime delivery, resolver timeout, app-managed permissions), in a quiet
   window only.

**Safety record for this session:**
- Only `lz-range-probe` was deployed (version 2.0.0, development environment).
- It was installed on wolfaenpak Jira for about 2 minutes and then uninstalled. `forge install list` is empty.
- The webtrigger URL is dead (424) and its id is redacted in the saved outputs.
- No other app, site setting or Jira data was touched.
- To re-install for the next probes:
  `cd fidelity-work/kvs-probe-app && forge install --site wolfaenpak.atlassian.net --product jira -e development --non-interactive --confirm-scopes`.

---

## Appendix: artifacts

All paths are under
`/private/tmp/claude-501/-Users-workhorse-Projects-goose/0570f02f-1f0b-4ad8-b216-ced5871b4923/scratchpad/forge2/understand/fidelity-work/`.

| path | what |
|---|---|
| `deny-net.sb` | the deny-network `sandbox-exec` profile |
| `lint-sandbox-plain.txt`, `lint-sandbox-verbose.txt` | the offline `forge lint` failures |
| `statsig-configurations.json` | the CLI's live dynamic configs (deprecated runtimes) |
| `scripts/lint_corpus.py`, `corpus/v00…v10/`, `lint-corpus-results.jsonl` | the server-rule corpus: real CLI vs kit |
| `scripts/openapi_drift.py`, `openapi-live/`, `openapi-drift.txt` | pinned vs live OpenAPI files |
| `kvs-probe-app/` (`manifest.yml`, `src/index.js`, cloned `node_modules`) | the live probe app source |
| `kvs-probe-response-1.json`, `kvs-probe-response-2.json` | the live answers (2 runs, identical outcomes) |
| `forge-logs-1.txt` | the `forge logs` output for invocation `45bd23c5-…` |
| `scripts/emu_kvs_probe.cjs`, `kvs-probe-emulator.json`, `kvs-live-vs-emulator.txt` | the same probe in the 1.0 emulator, and the diff |
| `webtrigger-create.txt`, `webtrigger-after-uninstall.txt` | the webtrigger URL (redacted) and the 424 after uninstall |

The receipt app `scratchpad/range-probe/app/` was left untouched: lint was run there, and nothing was edited.
