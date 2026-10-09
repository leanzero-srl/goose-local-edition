# Forge 2.0 research: UI Kit (@forge/react) for the admin panel, and offline render/drive fidelity

Fetched/measured 2026-10-09 (UTC 16:41-17:30) on the workhorse. Primary sources only: developer.atlassian.com pages,
npm tarballs (packed into `pkgs/`, read as text, never executed), Atlassian's production CDN bridge script, and
Atlassian-staff posts on community.developer.atlassian.com. Every quote is verbatim. "SOURCE" = file:line inside an npm
tarball. Where something is inferred from code rather than stated by Atlassian, it says INFERRED.

Artifacts (sha256):
- `@forge/react@12.3.0` https://registry.npmjs.org/@forge/react/-/react-12.3.0.tgz `09fe8274…8c1fe4`
- `@forge/bridge@7.1.0` `f02f467e…e03e960`
- `@atlaskit/forge-react-types@2.10.5` `353b0754…a3a71`
- `@forge/bundler@7.2.3` `dca0288a…f89d`, `@forge/tunnel@7.2.0` `2d736246…dab9`, `@forge/manifest@13.6.0` `74629fba…a7c`
- `@atlaskit/dynamic-table@19.3.8` `39d90fad…5118`, `react-reconciler@0.29.2`, `@babel/plugin-transform-react-jsx@7.29.7`
- `forge-sim@0.1.19` (community, MIT) `967e23a4…e94e61`
- `https://forge.cdn.prod.atlassian-dev.net/global-bridge.js` (101,996 bytes) `e6bef60d9ea0ba258f14cbf86b4aa4be318c4900553f2e57105fe2ef4fbd1594`
- `@forge/cli-shared@9.7.0` read from a sibling agent's extract (`pkgs/bootspeed-clishared/x`), read-only.

---------------------------------------------------------------------------------------------------------------------

## 1. Version, catalogue, hooks, modules, constraints

### 1.1 Current version
- npm `dist-tags` (2026-10-09): `latest: 12.3.0`, `next: 12.3.1-next.2`, `experimental: 12.3.1-next.1-experimental-e03cc85`.
  12.3.0 published `2026-09-28T02:45:57.978Z`. 12.0.0 (major: "Adds support for TypeScript 5") `2026-06-22`.
- `@forge/bridge` latest `7.1.0`; `@atlaskit/forge-react-types` latest `2.10.5` (2026-10-09T01:37Z).
- CHANGELOG 12.3.0: "Added MenuSpacer to GSN", "Export the `SidebarFooter` global component", "Expose
  `SearchableFlyoutMenuItems` from the `@forge/react/global` entry point." 12.0.0: "Add routing components and hooks for
  use in full-page apps. Added the following: `Router` `Route` `useNavigate` `useLocation` `useParams`".
- Runtime deps (package.json): `react ^18.2.0`, `react-reconciler ^0.29.0`, `react-hook-form 7.65.0`, `uuid ^11.1.1`,
  `lodash`, `@forge/bridge ^7.1.0`, `@forge/i18n 1.0.0`, `@forge/egress ^3.0.0`, `@atlaskit/adf-utils`,
  `@atlaskit/adf-schema`, `@atlaskit/forge-react-types ^2.10.0`, `react-test-renderer`. React 18 only (reconciler 0.29).
- `exports` map exposes only `"."`, `"./jira"`, `"./global"`, `"./router"`.
- Docs (https://developer.atlassian.com/platform/forge/ui-kit/components/): "You must be on `@forge/react` major version 10
  or higher to use the latest version of UI Kit components."
- Upgrade page (https://developer.atlassian.com/platform/forge/ui-kit/upgrade-to-ui-kit-latest/, updated Aug 21 2026):
  "Add `render: native` to each module." / "Use the `resource` key to point to the frontend, instead of `function`." /
  "Use the `resolver` key with `function` properties to point to any resolvers." Breaking renames: ModalDialog→Modal,
  Table→DynamicTable, TextField→Textfield, ButtonSet→ButtonGroup, StatusLozenge/DateLozenge→Lozenge.
  ("UI Kit 2" in the owner's wording = this: `@forge/react` ≥10 + `render: native`.)

### 1.2 Component catalogue (SOURCE `out/components/ui-kit-components.js`, `out/components/index.js`)
Every UI Kit component is a STRING constant, e.g. `exports.Button = 'Button';` — the host renders it. 80 codegen'd names:
Badge BarChart Box Button ButtonGroup Calendar Checkbox Code CodeBlock DatePicker DonutChart EmptyState ErrorMessage
FileCard FilePicker Form FormFooter FormHeader FormSection Heading HelperMessage HorizontalBarChart
HorizontalStackBarChart Icon Image Inline Label LineChart LinkButton List ListItem LoadingButton Lozenge Modal ModalBody
ModalFooter ModalHeader ModalTitle ModalTransition PieChart ProgressBar ProgressTracker Radio RadioGroup Range Select
SectionMessage SectionMessageAction Spinner Stack StackBarChart Tab TabList TabPanel Tabs Tag TagGroup TextArea Textfield
TimePicker Toggle Tooltip Text ValidMessage RequiredAsterisk Pressable CommentEditor ChromelessEditor Tile User
AtlassianTile AtlassianIcon Link UserPicker Frame UserGroup Bleed Breadcrumbs BreadcrumbsItem Pagination.
Plus JS wrapper components: DynamicTable, InlineEdit, Popup, Comment, AdfRenderer; `CheckboxGroup` string; deprecated
UI Kit 1 strings Em/Strike/Strong ("@deprecated - UIKit 1 specific component"). `@forge/react/jira`: CustomFieldEdit.
`@forge/react/global` (global:ui EAP): Global Main Sidebar LinkMenuItem ExpandableMenuItem FlyOutMenuItem
SearchableFlyoutMenuItems CreateButton CreateMenuItem HelpLink PersonalSettings PersonalSettingsItem ReorderableMenuItems
MenuSection MenuSpacer SidebarFooter. `@forge/react/router`: Router Route useNavigate useLocation useParams.
Docs catalogue (fetched) groups them and marks Preview: Atlassian icon/tile, Comment editor, Tile, Bleed, Pressable,
Breadcrumbs, Pagination, Router, Popup, Calendar, Inline edit, Time picker, Chromeless editor, List; EAP: File picker,
File card. Charts: Bar, Donut, Horizontal bar, Stack bar, Horizontal stack bar, Line, Pie.

### 1.3 Hooks (SOURCE `out/index.js`)
Exports: `useProductContext, useConfig, useTheme, usePermissions, useContentProperty, useSpaceProperty,
useIssueProperty, useTranslation, I18nProvider, useForm, useObjectStore, xcss, replaceUnsupportedDocumentNodes`.
- `xcss` is identity at runtime: `const xcss = (style) => style;` (index.js:5) — validation happens host-side.
- `useProductContext` = `view.getContext()` in a `useEffect` (useProductContext.js:6-16) → `undefined` on first render.
- `useConfig` re-reads on event `'FORGE_CORE_MACRO_CONFIG_CHANGED'`; `useTheme` on `'FORGE_CORE_THEME_CHANGED'`.
- `usePermissions` needs `context.permissions`; else `setError(new Error('This feature is not available yet'))`
  (usePermissions.js:79-84), then `checkPermissions` from @forge/bridge.
- `useForm` (useForm.js) wraps react-hook-form 7.65.0 with `mode: 'onBlur', // defaulting to validating onBlur to follow
  ADS behaviour` and `reValidateMode: 'onChange'` (lines 18-22). Register options allowed: required, disabled,
  maxLength, minLength, max, min, pattern, validate ("Only permitting the below as they've been tested and confirmed
  working"). onChange: `if (event?.target?.type === 'checkbox') … setValue(fieldName, event.target.checked …)`;
  `['number','text','textarea','radio','password'].includes(event?.target?.type)` → `event.target.value`; else the
  argument itself is the value (Select option object, DatePicker string…) (lines 64-77). Field ids
  `form-${useId()}-${fieldName}`.
- Hooks docs (https://developer.atlassian.com/platform/forge/ui-kit/hooks/hooks-reference/, updated Sep 28 2025) also
  list useCurrentFilter (EAP), useFilters (EAP), useWidgetConfig, useWidgetContext — these live in `@forge/hooks`
  (2.0.0): "`import { useWidgetContext } from "@forge/hooks/dashboards";`"
  (https://developer.atlassian.com/platform/forge/ui-kit/hooks/use-widget-context/, updated Sep 30 2026).
- React hook limits (hooks-reference): "Hooks that require direct DOM access, such as `useRef` for DOM elements" …
  "`useLayoutEffect`, `useImperativeHandle` and `useInsertionEffect`, will have limited functionality" … "since UI Kit
  does not have access to the underlying DOM nodes."
- useForm docs (https://developer.atlassian.com/platform/forge/ui-kit/hooks/use-form/): "For performance reasons,
  `useForm` capabilities are limited to input state handling, validation, and submission." Not supported: "dynamically
  setting values, clearing values, and watching form state."

### 1.4 Which modules accept UI Kit (`render: native`)
SOURCE `@forge/manifest@13.6.0 out/schema/manifest-schema.json`, walked by `scripts/native-modules.mjs`: 211 module
types, **56 accept `render: native`**: confluence:attachmentAction backgroundScript contentAction contentBylineItem
contextMenu customContent fullPage globalPage globalSettings homepageFeed pageBanner spacePage spaceSettings;
customerServiceManagement:crmImport queuePage requestDetail; dashboards:backgroundScript filter widget; global:fullPage
global:ui; **jira:adminPage** backlogAction boardAction customField customFieldType dashboardBackgroundScript
dashboardGadget **fullPage** globalBackgroundScript **globalPage** issueAction issueActivity issueContext issueGlance
issueNavigatorAction **issuePanel** issueViewBackgroundScript personalSettingsPage projectPage **projectSettingsPage**
sprintAction uiModifications; jiraServiceManagement:assetsImportType organizationPanel portalFooter portalHeader
portalProfilePanel portalRequestCreatePropertyPanel portalRequestDetail portalRequestDetailPanel portalRequestViewAction
portalSubheader portalUserMenuAction queuePage; macro.
- jira:adminPage schema: `"render":{"default":"default","enum":["default","native"],"type":"string"}`.
- Docs: "UI Kit is not supported in Forge workflow condition, Forge workflow validator, and Forge workflow post function
  modules." (https://developer.atlassian.com/platform/forge/ui-kit/)
- Open: compass:* modules carry no `render` in this schema although the compass docs mention `render: 'native'`.
- jira:adminPage docs (https://developer.atlassian.com/platform/forge/manifest-reference/modules/jira-admin-page/,
  updated Sep 22 2026): "The `jira:adminPage` module adds an item in the Apps section of the left navigation of Jira
  admin settings." `render`: "Indicates the module uses UI Kit." useAsConfig: "When it's set to `true`, it creates a
  Configure button that leads to this page from the app's entry in **Manage Apps**." "You can only specify `pages` or
  `sections` but not both." Extension context: `type`, `location` ("The full URL of the host page where this module is
  displayed."). The page does NOT say who can open it.
- jira:fullPage docs (updated Aug 3 2026): "Product-specific full page modules are being deprecated on September 30,
  2026." → global:fullPage (Preview, updated Sep 4 2026): "The `global:fullPage` module delivers an immersive full-page
  experience separate from Atlassian core app UIs."; `render` "[Mandatory for UI Kit only]"; `routePrefix` required.
- global:ui (updated Jul 1 2026): EAP, "Must be set to `native`, which enables UI Kit rendering. Custom UI is not
  supported for `global:ui`."; "deploy any Marketplace App using the Forge global:ui module or Global component in a
  Production environment" is forbidden → not benchmark material.
- Router (https://developer.atlassian.com/platform/forge/ui-kit/components/router/, updated Oct 7 2026): Preview;
  "The `Router` component provides client-side routing for Forge UI Kit full page apps."; Jira support: admin page, full
  page, global page, project page, project settings page. Router needs bridge `createHistory`; the source notes
  "The history object returned by the bridge does not conform to the v5 types. Instead it uses v4 types"
  (router/components/Router.js:34-35).

### 1.5 Resource-path rules (lint-time, objective)
SOURCE `@forge/manifest out/validators/resources-validator.js:192-199` + `out/text/errors.js:63`:
`if (module.render === 'native' && !resource.bundler) { if (…isDirectory()) … wrongResourceType }` with message
"Client Side UI Kit resource (${folder}) cannot be a directory". errors.js:240: "UI Kit resource entry '${entryPath}'
referenced by ${key} module must not point to an .html file" (only when `!resource.bundler`). Manual packaging
(https://developer.atlassian.com/platform/forge/manifest-reference/packaging, Preview, updated Sep 24 2026):
`bundler: manual@2026`, `path` = "a directory containing the assets", entries `home: home.html`; "Forge CLI does not
process the code to upload."

### 1.6 UI Kit constraints
- https://developer.atlassian.com/platform/forge/ui-kit/: "You won't have access to any of the underlying DOM, so features
  that depend on that will not work." / "You also still cannot use arbitrary HTML, and are restricted to using the
  components exported from `@forge/react`." / "With everything happening directly in the browser, your app doesn't need
  to call a separate server-side function for every state change."
- XCSS (https://developer.atlassian.com/platform/forge/ui-kit/components/xcss/): "XCSS support is currently available on
  the Box and Pressable components." / "XCSS restricts nested selectors completely from usage." / "The majority of style
  attributes will be restricted to Atlassian Design Token based values". Invalid-value behaviour: NOT documented.
- Overview (https://developer.atlassian.com/platform/forge/ui-kit/overview/, updated Jul 7 2026): "UI Kit relies only on
  the `@forge/react` components and does not directly rely on React DOM." / "UI Kit allows only images."
- Frame (https://developer.atlassian.com/platform/forge/ui-kit/components/frame/, updated Jul 1 2026): renders a Custom
  UI resource inside UI Kit; "Only modules that currently support Custom UI will support Frame." / "Only a single Frame
  component can be rendered per module." Communication via the Events API.
- AtlasCamp 2026 session by two Atlassian staff (Principal Engineer UI Extensibility; HoE Ecosystem Platform
  Extensibility), 23 Jun 2026: key takeaway "choose UI Kit over Custom UI where possible"
  (https://events.atlassian.com/atlascamp26-bengaluru/session/4053173/scaling-forge-apps-practical-strategies-for-performance-and-cost-optimization).

---------------------------------------------------------------------------------------------------------------------

## 2. Rendering architecture from SOURCE

### 2.1 The reconciler (`@forge/react@12.3.0 out/reconciler.js`, 254 lines)
- Host config: `supportsMutation: true`, `isPrimaryRenderer: false` (lines 120-125).
- Commit → bridge, whole tree every time (126-128):
  `resetAfterCommit(containerInfo) { (0, exports.callBridge)('reconcile', { forgeDoc: containerInfo }); }`
- Transport global (56-60): `self?.__bridge?.callBridge(cmd, data);`
- Node shape (62-74): `const element = { type, children: [], props: newProps, key: (0, uuid_1.v4)() };`
  Text (133-135): `createElement({ type: 'String', props: { text } })`.
- Props normalisation (36-46): "the `children` prop is dropped (children are tracked separately on the ForgeDoc tree)".
- Functions (27-35): `if (typeof propValue === 'function' && !propValue.__id__) { propValue.__id__ = (0, uuid_1.v4)(); }`
- Ids (8-26): "Checks the props of the component to see if it has an id prop. If so prefixes the value with the
  idPropsPrefix" for `['id', 'labelFor', 'inputId']`, prefix `forge-app-${uuid.slice(0,5)}` (random per load).
- Root (229-244): `createElement({ type: 'Root', props: {}, forgeReactMajorVersion: 11 })` (literally 11 in 12.3.0);
  `reconciler.createContainer(rootElement, 0, …)` → tag 0 = `LegacyRoot` (react-reconciler constants:
  `exports.ConcurrentRoot=1; … exports.LegacyRoot=0;`). Macro config root: `'MacroConfig'`.
- Render errors: `handleReconcilerError` → `self?.__bridge?.callBridge('onError', { error })` (47-55).
- Type (out/types/forge.d.ts): `interface ForgeDoc { children: ForgeDoc[]; key?: string; props?: ForgeProps; type:
  string; reconciliationCount?: number; forgeReactMajorVersion?: number; hasChanged?: boolean; }`.
- INFERRED consequence of LegacyRoot: react-reconciler 0.29.2 development.js:17677-17686 flushes synchronously "unless
  we're already working or inside a batch" when `lane === SyncLane && executionContext === NoContext && (fiber.mode &
  ConcurrentMode) === NoMode`. Handlers called by the host are not wrapped in a batch → every `setState` in a handler
  or after an `await` is its own render+commit → its own full-tree `reconcile` message. Deterministic given the app
  code, so countable by a grader.

### 2.2 Wrapper components that pre-transform for the host
- DynamicTable (components/dynamic-table.js:7-12): "These are internal components that the table head and rows are
  converted into. This is to ensure that they are correctly reconciled into ForgeDoc before being sent over the bridge.
  On the product frontend, the ForgeDoc is transformed back into the props the ADS DynamicTable expects." Output tree:
  `DynamicTable{…tableProps}` > `ContentWrapper{name:"head"}` > `Cell{cellKey,…}`, `ContentWrapper{name:"rows"}` >
  `Row{rowKey}` > `Cell{cellKey}`. Function-component cell content is CALLED as a plain function:
  `content.type(content.props)` (line 26, 38) — hooks inside a cell component therefore run outside React's render.
- ContentWrapper (utils/content-wrapper.js:4-7): "Functions in Forge apps are invoked from within the runtime and it's
  definition cannot be passed across the bridge. For ADS components that accept a call back function that returns a
  ReactNode as a prop, these must be evaluated within the runtime to return a ForgeDoc before being passed to the
  Renderer."
- InlineEdit reimplements validate/onConfirm/onCancel/onEdit in the runtime ("functions cannot be passed outside of the
  runtime"); Popup/Comment/AdfRenderer wrap their render-props in ContentWrapper.

### 2.3 Bridge (@forge/bridge@7.1.0)
- `getCallBridge` (bridge.js) throws `BridgeAPIError` "Unable to establish a connection with the Custom UI bridge." when
  `globalThis.__bridge?.callBridge` is missing; every API module calls it AT MODULE LOAD
  (`const callBridge = (0, bridge_1.getCallBridge)();`, invoke.js:8) → the bridge must exist before the app bundle runs.
- invoke (invoke.js:16-31): `validatePayload` throws 'Passing functions as part of the payload is not supported!';
  `callBridge('invoke', { functionKey, payload, metadata })`;
  `const limitedInvoke = (0, utils_1.withRateLimiter)(_invoke, 500, 1000 * 25, 'Resolver calls are rate limited at
  500req/25s');` — utils/index.js: fixed window; `if (numOps >= maxOps) { throw new errors_1.BridgeAPIError(…) }`.
- Rate-limit metadata: docs (https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/invoke/,
  updated Jun 19 2026) name fields `rateLimitValue`, `rateLimitRemaining`, `rateLimitReset` ("The time (in seconds since
  epoch) when the rate limit window resets"); typings (out/types.d.ts:8-12) name them `rateLimitRemaining`,
  `rateLimitReset`, `rateLimitLimit` — DISAGREEMENT on the maximum's field name.
- All bridge commands used by @forge/bridge + @forge/react (grep of `callBridge('…'`): reconcile, onError, invoke,
  getContext, fetchProduct (requestJira/Confluence/Bitbucket; adds `X-Atlassian-Token: no-check`), fetchRemote,
  fetchUserRecommendations, on/emit/onPublic/emitPublic, showFlag/closeFlag, openModal, open, close, submit, refresh,
  reload, onClose, createHistory, navigate, getUrl, getFrameId, getFrameDispatch, changeWindowTitle, enableTheming,
  emitReadyEvent, publishRealtimeChannel/subscribeRealtimeChannel, requestTeamworkGraph, openRovo/isRovoEnabled,
  initFeatureFlags, emitFrontendCustomMetric, trackObjectStoreAction, __permission__egress*/remote*.
- i18n loads `fetch('./__LOCALES__/i18n-info.json')` and `./__LOCALES__/<locale>.json` relative to the page
  (i18n/index.js:6-22; @forge/i18n constants `I18N_BUNDLE_FOLDER_NAME = '__LOCALES__'`).

### 2.4 Where the app's JS runs (INFERRED; the docs do not say)
- @forge/bundler config/nativeui.js:64-103 `getNativeUiBuildConfig` (named `iframeConfig` in nativeui.js:10) builds
  `target: 'web'` with `HtmlWebpackPlugin` → `index.html` (`${name}.html` for multi-entry). nativeui.js:11-12:
  `if (process.env.EXP_FORGE_UI_WORKER_RUNTIME === 'true') { return [iframeConfig, getNativeUiWorkerBuildConfig(…)] }`
  — the worker build (`target: 'webworker'`, ESM, `__worker__/`) is experimental.
- @forge/tunnel native-ui-tunnel-server.js:43-45 applies `getCustomUIHtmlTransformMiddleware(permissions, remotes)`,
  which (resource-tunnel-server.js:55-66) injects the iframe-resizer script and the global bridge script and sets a
  `Content-Security-Policy` header — identical to Custom UI.
- @forge/cli-shared bridge-script-service.js:6-14: `<script src="https://${GLOBAL_FORGE_INSTALL_ID}.cdn.${this.env}
  .atlassian-dev.net/${BRIDGE_CORE_CDN_PATH}">` with `'forge'` and `'global-bridge.js'`.
- @forge/react testUtils.js:4-7: "In an actual UI Kit 2 application the bridge is added to the global namespace by
  scripts served from the Forge CDN."
- global-bridge.js: installs `__bridge` only if absent (`void 0===o.__bridge&&(o.__bridge=i)`); adapters:
  `if(this.featureFlags.includes("forge-ui-bridge-core-reset")){…port-rpc…}else if(this.shouldLoadAdapter("postrobot"))
  {var r=(0,n(449).u)(window.postRobot,window.parent);…}`; parent-origin allow-list starts
  `[/.*\.atlassian\.net$/,…]`; page host parsed with `window.location.hostname.split(".forge-cdn.")`; port-rpc also
  supports `DedicatedWorkerGlobalScope`.
- Conclusion (INFERRED): production UI Kit runs the app bundle in a separate Forge-CDN page (an iframe whose DOM is
  never shown; the ADS components are drawn by the product page), talking to the product over postMessage (post-robot
  or port-rpc); a web-worker variant exists behind an experiment flag.

### 2.5 Callback round-trip (global-bridge.js serializer, module 906; constants module 438)
- `if("function"==typeof e)return i(e,t,n);` → `{__type__:"port_rpc_function",__id__:s,__name__:e.name||"anonymous"}`;
  `PROXY_ID_FIELD="__id__"` (re-uses @forge/react's `__id__`); `FUNCTION_CALL_PREFIX="__port_rpc_fn__"`;
  Errors → `{__type__:"port_rpc_error",message,stack,code,name}`; RegExp → `port_rpc_regex`; `Date/ArrayBuffer` pass;
  objects recursed; cycles → `undefined` (WeakSet); symbols → `undefined` (so React elements lose `$$typeof`).
- Host → app event payload contract (forge-react-types `types.codegen.ts`, "@codegenDependency
  ../../../../forge-ui/src/components/UIKit/types.ts"): `export type SerialisableEvent = { bubbles; cancelable;
  defaultPrevented; eventPhase; isTrusted; target: { selectionStart?; selectionEnd?; value?; checked?; name?; id?;
  tagName?; type? }; timeStamp; type }` and `EventHandlerProps = { onChange?(event: InputEvent); onBlur?; onFocus? }`.
- DatePicker `onChange?: (value: string) => void`; Select `onChange: (newValue: Option | Option[]) => void` (docs);
  Tabs `onChange` "receives the selected index and `UIAnalyticsEvent`" (docs); UserPicker onChange `UserPickerValue
  {avatarUrl,email,id,name,type}` (types); form submission value: "If `isMulti` is `true`, the submitted value is an
  array of account ID strings; otherwise it is a single string."

### 2.6 The host renderer (Atlassian-internal)
- forge-react-types README: "The types are code generated from the `@atlassian/forge-ui` package and are guaranteed to
  be in sync with the source component implementation in the `@atlassian/forge-ui` package." devDependency
  `"@atlassian/forge-ui": "^37.27.0"`. Public npm `@atlassian/forge-ui` = only 9.0.1/9.0.2 from 2019 ("Used for creating
  platform-independent Forge UI extensions") → the current host is NOT published.
- Codegen headers name the host files, e.g. ButtonProps: "@codegenDependency
  ../../../../forge-ui/src/components/UIKit/button/index.tsx", and types are Picks of ADS props:
  `import type { ButtonProps as PlatformButtonProps } from '@atlaskit/button/default/button';`
  `export type ButtonProps = Pick<PlatformButtonProps, 'children'|'autoFocus'|'isDisabled'|'isSelected'|'onBlur'|
  'onClick'|'onFocus'|'testId'|'shouldFitContainer'|'appearance'|'type'> & {iconBefore?; iconAfter?; spacing?}`.
- DynamicTableProps Pick has NO `onSort` (head, rows, sortKey, sortOrder, defaultSortKey, defaultSortOrder, rowsPerPage,
  page, onSetPage, isRankable, onRankStart, onRankEnd, isLoading, emptyView, label, caption, testId, …) → sorting is
  host-owned; the app cannot observe a user's sort.
- Atlassian Staff (QuocLieu, title "Atlassian Staff", 2023-12-06,
  https://community.developer.atlassian.com/t/ui-kit2-unit-testing-jsx-components/75091): "unit testing libraries such
  as RTL or enzyme will not work in UI kit 2 as the component implementations itself only exists within products (such
  as Jira or Confluence). Because of this, UI Kit 2 components can’t be rendered locally for testing purposes. The
  ForgeReconciler works by converting your JSX code to a JSON object and sends it to the product to be rendered."
- Host code leaks in a user stack trace (2025-03-05, https://community.developer.atlassian.com/t/89914):
  "at bn (RendererNextComponent.tsx:204:50) which in turn links to const isFirstChildAForm = forgeDoc.children[0].type
  === 'Form';" — the host inspects ForgeDoc structure and crashed on an empty tree (content-action modal).

### 2.7 ADS behaviour the host inherits (verified in @atlaskit/dynamic-table@19.3.8)
- Docs (https://developer.atlassian.com/platform/forge/ui-kit/components/dynamic-table/, updated Jul 23 2025): "Sorting a
  dynamic table is done based on the `key` set on each cell." / "The content of a cell does not affect its sorted
  order." / "Dynamic table manages sorting, pagination, loading, and drag and drop state management by default."
- hoc/with-sorted-page-rows.js:47-49 `new Intl.Collator(undefined, { numeric: true, sensitivity: 'accent' })`;
  56 "Algorithm will sort numerics or strings, but not both"; numbers grouped before strings; undefined keys → `return
  modifier`. stateless.js:119 `var sortOrderFormatted = key !== sortKey ? 'ASC' : toggleSortOrder(sortOrder);`.
  styled/head-cell.js:47-58 `"aria-sort"` = 'ascending'/'descending'. RankEnd = `{ sourceIndex, sourceKey,
  destination?: { index, afterKey?, beforeKey? } }`.

---------------------------------------------------------------------------------------------------------------------

## 3. Install size (closure)

Method: `npm install --package-lock-only --ignore-scripts` (npm 11.12.1, scratch cache) then Σ registry
`dist.unpackedSize` over every lock entry (`scripts/closure-size.mjs`). No tarballs installed, no `du` run.
| package | lock entries | Σ unpackedSize | files | @atlaskit/* |
|---|---|---|---|---|
| @forge/react@12.3.0 | 282 | **143.6 MB** (273 sized; 9 tiny legacy pkgs unsized) | 39,581 | 76.9 MB / 93 pkgs |
| @forge/react@11.17.0 | 282 | 131.6 MB | 37,621 | 65.0 MB / 88 |
| @forge/react@11.2.0 | 1,591 | **3,738.1 MB** | 447,710 | 2,628.4 MB / 831 |
Top of 12.3.0: @atlaskit/tokens@20.4.1 20.0 MB, @atlaskit/tokens@1.61.0 13.0 MB, date-fns 6.7 MB (via
@atlaskit/calendar/datetime-picker), @forge/manifest 4.7 MB (via @forge/bridge → @sentry/node, cheerio, undici, glob,
ajv), react-dom 4.5 MB (auto-installed peer). 11.2.0's size comes from forge-react-types 0.41.25 depending on
`@atlaskit/renderer` + `@atlaskit/editor-json-transformer` (15 nested `typescript` copies). Lock-only resolution
uses TODAY's latest matching transitive versions, so the 11.x numbers are not what those versions pulled at publish.
- On-disk bound for 12.3.0: 143.6 MB + 39,581 × 4 KiB ≈ 305.7 MB → the Forge 1.0 DESIGN figure "`@forge/react`
  1,246 MB (excluded, D8)" (`du -sk`, 2026-10-02, evals/swarm-bench/forge/DESIGN.md:633) is NOT reproducible for
  12.3.0 today; cause unknown (the spike app's package.json does not list @forge/react; forge-react-types 2.10.4, the
  version current on 10-02, had the same 35 light deps).
- Runtime import graph (non-test `require`s): @forge/react → react, react/jsx-runtime, react-reconciler(+constants),
  uuid, lodash/get, lodash/isEqual, react-hook-form, tslib, @forge/bridge, @atlaskit/adf-utils/traverse (always loaded
  via components/index → adf-renderer), `history` only in router/utils/test-utils.js (NOT a dependency). @forge/bridge
  → tslib, @forge/i18n, @forge/egress only (its @forge/manifest, @atlaskit/tokens, @atlaskit/adf-schema, iframe-resizer
  deps are never required at runtime). `tslib` is required but not declared by @forge/react.
- Runtime subset (15 packages: @forge/react, @forge/bridge, @forge/i18n, @forge/egress, react, react-reconciler,
  scheduler, loose-envify, js-tokens, react-hook-form, uuid, lodash, tslib, @atlaskit/adf-utils, @babel/runtime) =
  **6.2 MB** unpacked. Type-checking TSX needs the ADS types (codegen files `import type … from '@atlaskit/…'`), i.e.
  the full ~144 MB closure.
- Licences: @forge/react / @forge/bridge LICENSE.txt: "Permission is hereby granted to use this software in accordance
  with the terms and conditions outlined in the Atlassian Developer Terms"; ADS packages Apache-2.0.
- Conclusion: a kit can ship @forge/react 12.3.0 (npm ci from a committed lockfile, ~144 MB / ≤306 MB on disk) or only
  the 6.2 MB runtime subset if it bundles and does not type-check.

---------------------------------------------------------------------------------------------------------------------

## 4. Existing testing / rendering utilities
- Official: none public. Staff (QuocLieu) 2023-12-06: "snapshot testing will work, but we do understand that it’s not a
  complete replacement for standard unit tests. The team intends on providing unit test support down the line after
  GA"; 2025-04-17: "we have this on our internal backlog, but unfortunately don’t have firm dates for this yet."
- Inside @forge/react's tarball (not exported; `exports` lists only ".", "./jira", "./global", "./router"):
  `out/__test__/reconcilerTestRenderer.js` (`create` = `act(async () => ForgeReconciler.render(element))`, "NOTE: This
  only works with React 18"), `out/__test__/testUtils.js` (`setupBridge` sets `global['self'] = { __bridge: { callBridge:
  (cmd, data) => { bridgeCalls.push({ cmd, data }) } } }`, `getLastBridgeCallForgeDoc`, `findElementInForgeDoc`),
  `__mocks__/@forge/bridge.js` (`module.exports = { events: {} };`). Their own tests click by calling
  `get(forgeDoc, 'children[1].props.onClick')()`.
- No relevant npm package found for "forge ui kit test/renderer/mock" (registry search 2026-10-09);
  `@forge-ui-official/core` is unrelated (React 19 + Tailwind, gmail maintainer, atomgit repo).
- Community: `forge-sim` 0.1.19 (MIT, created 2026-07-16, by a non-Atlassian: "I don't work for Atlassian"): README
  "Renders UIKit 2 modules to a ForgeDoc tree you can query and interact with; no browser." / "Headless UIKit 2: your
  JSX runs through the real @forge/react reconciler." / "a _mostly_ truthful forge implementation" /
  `permissions.external` "parsed but not enforced". Its browser renderer (renderer/src/component-map.tsx, 1,769 lines)
  maps ForgeDoc → real ADS (`@atlaskit/dynamic-table ^18.3.0`, older than forge-react-types' `^19.3.0`). Observed
  silent drops in its map: `Tabs: (props, children) => <Tabs id={props.id ?? 'tabs'}>{children}</Tabs>` (drops
  onChange/selected/defaultSelected); Textfield passes only name/placeholder/defaultValue/value/isDisabled/onChange
  (drops onBlur/id/isInvalid that `register()` returns).
- In-repo: `evals/swarm-bench/forge/spike/harness/prove-uikit.cjs` (Forge 1.0) installs `globalThis.__bridge` before
  the esbuild'ed bundle, captures every ForgeDoc, paints a tiny generic renderer, clicks a Button, observes the invoke.
  It bundles with `jsx: 'automatic'` (see risk F10).

---------------------------------------------------------------------------------------------------------------------

## 5. What an offline host must implement (and the fidelity risks)

### 5.1 Host contract (from the sources above)
1. Install `globalThis.__bridge = { callBridge(cmd, payload) }` BEFORE evaluating the bundle (bridge APIs bind at
   module load). Implement at least: `reconcile`, `getContext`, `invoke` (+ `metadata.rateLimitProperties`),
   `fetchProduct` (requestJira → mock Jira), `on`/`emit` (resolve to `{ unsubscribe() }`), `showFlag`/`closeFlag`,
   `onError`, `emitReadyEvent`, `enableTheming`, `createHistory`/`navigate`/`getUrl` if Router is allowed; serve
   `./__LOCALES__/` if i18n is allowed. Anything else → loud `unsupported_bridge_call` event, never a silent default.
2. Snapshot each `reconcile` payload at receipt (the reconciler passes the LIVE container that later commits mutate).
3. Deliver callbacks asynchronously (next macrotask), with arguments passed through the same serialisation rules as
   global-bridge.js (functions → proxies, symbols dropped, cycles → undefined, Errors → port_rpc_error), never inside
   `act()`/batchedUpdates (would hide the legacy-root unbatched commits the product produces).
4. Host-owned state keyed by ForgeDoc `key` (uuid, stable while mounted, new on remount): uncontrolled input values
   (`defaultValue`/`defaultChecked`), DynamicTable sort+page (ADS comparator, pinned collator locale), Tabs selection
   when `selected` is absent, Modal open is app-owned (ModalTransition + conditional render).
5. Events: inputs get `SerialisableEvent` with `target.type` = 'text' | 'number' | 'password' | 'textarea' | 'checkbox'
   (Checkbox, Toggle) | 'radio' (RadioGroup) and `value`/`checked`; Select gets the option object (array if isMulti);
   DatePicker the date string; Tabs `onChange(index)`; DynamicTable `onSetPage(page)`, `onRankEnd(RankEnd)`; Modal
   `onClose()` on Esc/overlay/close button; Form submit when a `Button type="submit"` inside the Form is activated;
   `onBlur` after each field edit (useForm validates on blur).
6. Accessible HTML for screenshots/inspection: label↔input via the (identically prefixed) `labelFor`/`id`;
   `<table>`+`<caption>`+`<th aria-sort>`; tablist/tab/tabpanel roles; dialog role with title; `data-testid` from
   `testId`; unknown types/props rendered with a visible "unmodelled" marker and an event, never dropped.
7. Minimal admin-panel component set: Root, String, Stack, Inline, Box(xcss), Heading, Text, Link, Lozenge, Badge, Tag,
   Spinner, EmptyState, SectionMessage(+Action), Tooltip, Button, LoadingButton, ButtonGroup, Tabs/TabList/Tab/TabPanel,
   DynamicTable(+ContentWrapper/Row/Cell), Form/FormHeader/FormSection/FormFooter, Label, RequiredAsterisk,
   HelperMessage, ErrorMessage, ValidMessage, Textfield, TextArea, Select, Toggle, Checkbox, RadioGroup, DatePicker,
   UserPicker (host-side user search against the mock directory), User, Modal/ModalTransition/ModalHeader/ModalTitle/
   ModalBody/ModalFooter, ProgressBar; optional: one chart (BarChart/LineChart), Pagination, Router/Route.

### 5.2 Fidelity risks (ranked)
- F1 Host renderer is closed (`@atlassian/forge-ui` ^37 not public): behaviour must be inferred from types + docs.
- F2 Event payload shapes only partly specified (Form onSubmit's argument, Tabs' analytics 2nd arg, UserPicker multi
  onChange) — grade only what an app cannot be wrong about, or measure once on a real site.
- F3 Async + serialisation: in-process calls are synchronous and pass live objects; product calls are async postMessage.
  Double-submit/race behaviour differs unless the host defers.
- F4 Live-tree mutation → must snapshot at reconcile.
- F5 LegacyRoot unbatched commits: real behaviour (inherited because the real reconciler runs) — but `act()` wrapping
  would mask it.
- F6 ADS version drift (product vs kit), collator locale (`Intl.Collator(undefined, …)`).
- F7 Silent prop drops in a home-grown host (forge-sim evidence) — require exhaustive prop handling for the allowlist.
- F8 Element-valued props (e.g. `emptyView={<Text/>}`) arrive as raw element objects, `$$typeof` lost in serialisation.
- F9 Host structural expectations (RendererNextComponent `forgeDoc.children[0].type === 'Form'`) unknown in general.
- F10 Bundling: Forge compiles `.jsx` with Babel 7 `@babel/plugin-transform-react-jsx` `{ pragma: 'React.createElement' }`
  (config/nativeui.js:41-55); the plugin's non-development default is `runtime: RUNTIME_DEFAULT = development ?
  "automatic" : "classic"` (create-plugin.js:43) → a `.jsx` file without `import React` throws at runtime in Forge but
  works under esbuild `jsx: 'automatic'`. `.tsx` goes through `ts-loader` with the app's tsconfig (the bundler's
  fallback `static/tsconfig.json` is an empty file).
- F11 Pixels: without real ADS + tokens CSS, screenshots are not Atlassian-faithful; grade the tree/semantics, not pixels.
- F12 xcss/token validation behaviour undocumented; ids/keys random per load (never grade them, grade relationships).

### 5.3 How the benchmark can test UI Kit objectively (all must be STATED in the contract)
- Manifest: module `jira:adminPage` (`render: native`, `resource` = a file, or `bundler: manual@2026` dir), optionally
  `useAsConfig: true` → checked by schema + kit lint with the exact messages above.
- Boot: bridge-call log with logical sequence numbers: number of `invoke`/`fetchProduct` before the first ForgeDoc that
  contains the main DynamicTable (stated budget, e.g. 1 batched invoke); a non-empty first ForgeDoc (loading state).
- Quiescence/stability: after boot and after each scripted action the app must stop emitting `reconcile` within K host
  ticks with no input (render-loop detector); no `onError`; no `BridgeAPIError` from the 500/25 s limiter.
- Structure: stated `testId`s (supported by Button, DynamicTable, Tabs, Tab, TabPanel, Select, Textfield, TextArea, Toggle,
  Checkbox, Modal, SectionMessage, Heading, Text, Lozenge… NOT by Form/FormFooter/FormHeader/FormSection/TabList/
  ModalTransition/Popup/InlineEdit) + stated visible texts; `Label.labelFor === input.id`.
- Interaction scripts: switch tab → expected panel text; click sortable header twice → rows in stated order (stating
  that UI Kit sorts by cell `key`, so numeric/date columns need numeric/ISO keys); page → `onSetPage`; open modal →
  Modal present with stated title; fill form (SerialisableEvent + blur) → submit → exactly one `invoke('<stated key>',
  <stated payload>)`; invalid input → stated ErrorMessage text and ZERO invokes; double-activate Save before the first
  resolves → still exactly one invoke (if stated as a robustness requirement).
- Efficiency (only if stated): reconcile commits per action and ForgeDoc bytes per commit are deterministic for a given
  app and measurable.
- Security: never inferred from UI — the grader calls the admin resolvers directly with a non-admin context and expects
  refusal (UI hiding is not authorisation; `displayConditions`/`isAdmin` only hide).

---------------------------------------------------------------------------------------------------------------------

## 6. Open questions / not verified
1. Where production UI Kit JS executes is INFERRED (iframe page by default; worker behind EXP_FORGE_UI_WORKER_RUNTIME);
   Atlassian docs do not state it.
2. Exact arguments the product host passes to Form `onSubmit`, Button `onClick`, Tabs `onChange` (2nd arg), UserPicker
   `onChange` with `isMulti` — undocumented; measurable on wolfaenpak with a probe app that logs its callback args.
3. Host behaviour for invalid xcss, unknown component types, element-valued props, empty Root.
4. Whether the product host batches bursts of `reconcile` messages (only the app side was read).
5. The 1,246 MB Forge 1.0 measurement could not be reproduced; no real install/`du` was run here (registry metadata
   sum + bound only), to keep CPU/network light during the paid run.
6. `rateLimitProperties` maximum field: docs `rateLimitValue` vs typings `rateLimitLimit`.
7. compass:* modules: docs mention `render: 'native'` but manifest schema 13.6.0 has no `render` for them.
8. Legal: whether a benchmark kit may redistribute @forge/* (Atlassian Developer Terms) vs install from the registry.
Recommended closure for 2/3/4: deploy a probe UI Kit admin page to wolfaenpak (test site, writes permitted), log every
callback argument and the ForgeDoc it emits, and use that capture as the offline host's golden fixture.

---------------------------------------------------------------------------------------------------------------------

## Verification

Independent fact-check, 2026-10-09 (~20:10-20:40 local), of the 33 design-critical claims handed over from this file.
Method: every npm tarball was downloaded again into fresh directories under `forge2/verify-uikit/`. The sha256 prefixes
match the ones listed at the top: react 09fe8274, bridge f02f467e, forge-react-types 353b0754, dynamic-table 39d90fad,
bundler dca0288a, manifest 74629fba, forge-sim 967e23a4. global-bridge.js is byte-identical (e6bef60d…1594). Every doc
page was fetched again as raw HTML and its text grepped. Community threads were read through the Discourse JSON API. The
manifest schema was walked with a new script, not the one above. The @forge/react closure was resolved again lock-only
and the registry sizes summed. I also read the newest pre-releases (@forge/react 12.3.1-next.2, @forge/bridge
7.1.1-next.7, @forge/bundler 7.3.0-next.15) and the Forge changelog entries visible for 26 Sep to 9 Oct 2026. No code
from the tarballs was executed.

**Tally: 30 confirmed (several need the stated correction), 3 refuted (#10, #11, #31), 0 unverifiable, 0 outdated.**

1. CONFIRMED. Registry dist-tags: latest 12.3.0 (2026-09-28T02:45:57Z), next 12.3.1-next.2 (2026-10-09T06:35Z).
   package.json lists react ^18.2.0, react-reconciler ^0.29.0 and react-hook-form 7.65.0 as `dependencies`, not peers.
   react-reconciler 0.29.2 peer-requires react ^18.3.1, so pin react 18.3.1. A lock-only resolve gives exactly one react
   (18.3.1). In 12.3.1-next.2, reconciler.js, useForm.js, dynamic-table.js and ui-kit-components.js are byte-identical
   to 12.3.0.
2. CONFIRMED. The sentence appears verbatim on the components page (last updated Jan 29 2024) and on the UI Kit overview
   (Sep 24 2026). The upgrade guide (Aug 21 2026) says "Add `render: native` to each module." UI Kit 1 no longer works at
   all. Staff posts: t/82956 (2024-08-28) "will stop working on 2025-02-27T13:00:00Z", and t/89214 (2025-02-19) says the
   deprecation took effect on 28 Feb 2025.
3. CONFIRMED, list incomplete. ui-kit-components.js has exactly 80 `exports.X = 'X'` lines; line 35 is
   `exports.Button = 'Button';`. The wrappers emit DynamicTable, Row, Cell, ContentWrapper, InlineEdit, Popup, Comment and
   AdfRenderer, and the reconciler emits Root and String. No react-dom is required outside tests. The list is not the whole
   type space, though. The main entry also exports `CheckboxGroup` and the deprecated `Em`, `Strike` and `Strong`.
   `addConfig` roots its tree at `'MacroConfig'`. `/jira` adds `CustomFieldEdit` and `/global` adds 15 more types. Any
   type outside the allowlist must take the loud "unmodelled" path.
4. CONFIRMED. reconciler.js:126-128 sends `callBridge('reconcile', { forgeDoc: containerInfo })`. containerInfo is the
   Root element from line 231, which carries `forgeReactMajorVersion: 11`. Nodes are built at lines 62-74 as {type,
   children:[], props, key: uuid v4}, with `children` removed from props (36-46). Text nodes are {type:'String',
   props:{text}} (133-135). A macro's `addConfig` sends a second tree, rooted at MacroConfig, through the same command.
5. CONFIRMED, page context only. The testUtils.js:4 comment is verbatim. global-bridge.js module 787 installs the bridge
   only when it is absent (`void 0===o.__bridge&&(o.__bridge=i)`), and `o` is globalThis (module 179). The catch: the
   reconciler reads `self?.__bridge` (reconciler.js:49 and 59), while @forge/bridge reads `globalThis.__bridge`
   (bridge.js:9). In a browser page `self === globalThis`, so one global is enough. Plain Node 24.15 has no `self`, and
   `self?.` throws a ReferenceError there. A host that is not a page must also set `globalThis.self = globalThis`.
6. CONFIRMED. bridge.js:8-16 throws a BridgeAPIError that starts "Unable to establish a connection with the Custom UI
   bridge. If you are trying to run your app locally, Forge apps only work in the context of Atlassian products…". 29
   modules bind `const callBridge = (0, bridge_1.getCallBridge)();` at top level, and fetch/index.js:7-8 does the same.
   The @forge/react hooks require @forge/bridge, so importing @forge/react alone throws when the bundle is evaluated
   without a bridge.
7. CONFIRMED, one of two transports. The quoted serializer (module 906) behaves as stated: functions become
   `port_rpc_function` and reuse `__id__` (`PROXY_ID_FIELD="__id__"`, module 438), Errors become `port_rpc_error`, symbols
   become undefined, and cycles become undefined. reconciler.js:27-35 tags each top-level function prop with a uuid
   `__id__`. That serializer is only one of two transports. module 144 uses port-rpc only when the iframe URL's
   `platformFeatureFlags` contains `forge-ui-bridge-core-reset`. Otherwise it uses post-robot (`window.postRobot`,
   `window.parent`), which serializes with JSON.stringify and a replacer:
   - functions become `{__type__:"cross_domain_function", __val__:{id: fn.__id__||uid, name}}`;
   - Errors become `{__type__:"error", __val__:{message,stack,code,data}}`;
   - Dates become `{__type__:"date", __val__: ISO string}`;
   - symbol-valued properties are omitted;
   - a cycle THROWS.
   Which transport a product page uses cannot be seen offline. The design consequence holds under both: deliver handlers
   asynchronously, pass serialized arguments, never pass live objects, and expect React elements to lose `$$typeof`.
8. CONFIRMED. types.codegen.d.ts:10-28 in forge-react-types 2.10.5 has exactly the stated fields. Its
   `@codegenDependency ../../../../forge-ui/src/components/UIKit/types.ts` resolves to
   packages/forge/forge-ui/src/components/UIKit/types.ts. Who uses it:
   - Textfield, TextArea, Checkbox and Toggle: onChange, onBlur and onFocus.
   - Radio and RadioGroup: onChange only, with no onBlur.
   - Range: onBlur and onFocus. Its onChange receives a number, not an event.
9. CONFIRMED, with additions. useForm.js:18-22 sets mode 'onBlur' and reValidateMode 'onChange'. Lines 64-77 read
   `checked` for checkbox, `value` for number/text/textarea/radio/password, and otherwise store the argument itself.
   Additions:
   - (a) The onBlur from register() is thrown away and replaced by `onBlur: () => trigger(fieldName)` (78-82).
   - (b) onChange calls setValue with `shouldTouch: true` and `shouldValidate = submitCount>0 || touched`. After a field's
     first change, every later change re-validates even without a blur. A host that fills each field with one onChange
     and no blur leaves it unvalidated until blur or submit. The host must also call the handler from the LATEST
     snapshot, because older closures see stale formState.
   - (c) Trap: any other input type, such as Textfield `type="email"`, `"url"`, `"tel"`, `"search"` or `"date"`, falls to
     the last branch and stores the whole SerialisableEvent object as the field value. The host must report the real
     `target.type`, or it hides this trap.
   - (d) RadioGroup's type has no onBlur, so it is unknown whether the product ever calls the onBlur that register()
     spreads in.
10. REFUTED: the head shape is wrong. dynamic-table.js:21-43 gives the head as DynamicTable > ContentWrapper{name:"head"}
    > Cell{cellKey,…}, with NO Row. Only the rows go ContentWrapper{name:"rows"} > Row{rowKey,…} > Cell{cellKey,…}. Lines
    7-12 confirm that the product turns the ForgeDoc back into ADS props. Also, a function-component cell is called as a
    plain function in the app (`content.type(content.props)`, lines 26 and 38), so hooks inside it run outside React.
11. REFUTED: the numeric trap is wrong as stated. These parts hold. The docs (Jul 23 2025) say "Sorting a dynamic table
    is done based on the `key` set on each cell." and "The content of a cell does not affect its sorted order."
    DynamicTableProps is a Pick that has sortKey, sortOrder, defaultSortKey and defaultSortOrder but no onSort, so the
    app never sees a user's sort. The wrong part: ADS compares string keys with
    `Intl.Collator(undefined,{numeric:true})`, so integer digit strings sort numerically. Measured with en-US:
    - "10","9","100","2" sorts to 2, 9, 10, 100;
    - the unpadded "2026-9-30" sorts before "2026-10-09".
    So the proposed "'10' < '9'" test would NOT fail. String keys mis-sort only in these cases:
    - decimals: "1.5" < "1.05" < "1.25";
    - negatives: "-5" < "-10";
    - thousands separators and currency: "$1,200" < "$900";
    - display dates: "9 Oct 2026" < "10 Sep 2026".
    Mixed number and string keys put the numbers first. The docs also require keys to be unique per column ("used for both
    reconciliation of lists and column sorting"), which raw numeric keys break when two values repeat.
12. CONFIRMED, two edge cases. dist/cjs/hoc/with-sorted-page-rows.js:47-50 builds the collator and line 56 holds the
    comment. Lines 60-78 return the modifier for undefined keys and put numbers before strings. stateless.js:119 holds
    the toggle. Edge cases:
    - (a) In a rankable table, clicking the current column while it is DESC sends `{key:null, sortOrder:null}` and the
      table goes unsorted (stateless.js:110-117).
    - (b) `toggleSortOrder(undefined)` returns undefined, and anything that is not 'ASC' sorts descending.
    Pin the host's locale to en-US.
13. CONFIRMED. The README sentence is verbatim, and devDependencies has `"@atlassian/forge-ui": "^37.27.0"`. The public
    package has only 9.0.1 and 9.0.2, both published 2019-09-10. Some types are not Picks. Popup, InlineEdit, Comment,
    Badge, Box, Heading, Text and Pressable are `Omit<ADS props>`, so they silently include every ADS prop that was not
    omitted.
14. CONFIRMED. In t/75091, post 5 is by QuocLieu ("Atlassian Staff") at 2023-12-06T06:02Z and the quote is verbatim. Post
    10, on 2025-04-17, says "we have this on our internal backlog, but unfortunately don’t have firm dates for this yet".
    No newer official renderer turned up on npm or developer.atlassian.com. @forge/react exports only ".", "./jira",
    "./global" and "./router".
15. CONFIRMED, as an inference. out/nativeui.js:10-14 holds `iframeConfig` and the EXP_FORGE_UI_WORKER_RUNTIME branch
    verbatim. config/nativeui.js:64-103 builds target 'web' with HtmlWebpackPlugin. Both are unchanged in 7.3.0-next.15,
    whose CHANGELOG says "Setup internal worker based native UI app compilation", so the worker build is still internal.
    The default post-robot transport needs `window.postRobot` and `window.parent` and reads `window.location`. The docs
    still do not say where UI Kit code runs.
16. CONFIRMED, framing corrected. invoke.js:25 and utils/index.js:5-21 implement a fixed 25-second window that starts
    at module load. Call 501 throws a BridgeAPIError (a rejected promise) before callBridge runs. 7.1.1-next.7 has the
    same limiter. invokeRemote and invokeService get their own 500 per 25 s limiter (invoke-endpoint.js:8-9 and 44).
    Framing correction: the Forge invocation limits (limits-invocation, updated Sep 1 2026, fixed one-minute window) are:
    - per user: 1,200 per minute, which equals 500 per 25 s;
    - per install: 7,000 per minute AND 300 per second.
    A burst of more than 300 calls in under a second hits the per-install 300/s limit, a 429 through invoke, before the
    client limiter. The client limiter bites first only between about 20/s and 300/s. None of this is the REST
    Tier 1/Tier 2 points quota.
17. CONFIRMED. The invoke docs (Jun 19 2026) name `rateLimitValue`, `rateLimitRemaining` and `rateLimitReset` (seconds
    since epoch). types.d.ts:8-12 names `rateLimitRemaining`, `rateLimitReset` and `rateLimitLimit`, with no units.
    7.1.1-next.7 still uses `rateLimitLimit`. Newer evidence: the example on limits-invocation (Sep 1 2026) reads
    `error.metadata?.rateLimitProperties?.rateLimitReset` from a 429 and waits `reset*1000 - Date.now()`. So the
    metadata also comes back on 429 errors.
18. CONFIRMED. My own walk of `definitions.ModuleSchema.properties` found 211 types, and 56 of them accept native, the
    same list as above. jira:adminPage's render is `{"default":"default","enum":["default","native"],"type":"string"}`.
    The quote is a compacted form; the file itself is pretty-printed.
19. CONFIRMED. resources-validator.js:193-201 holds the rule, and its message is errors.js:63 verbatim. The `.html` entry
    rules are at :69 and :115 (errors.js:240 and 244). Both apply only when `!resource.bundler`. The rule is skipped
    when the path does not exist (:190) and when the module points at `resource: key/entry`.
20. CONFIRMED, important omission. The adminPage docs (Sep 22 2026) quotes are verbatim, and the page does not say who
    may open it. Omitted: the same page says "Subpages only work with Custom UI. Subpages are not supported by the
    Configure and Get started pages." An entry with useAsConfig or useAsGetStarted "can’t include either `pages` or
    `sections`", and each kind of entry may appear only once. A UI Kit admin panel therefore needs its own navigation,
    with Tabs or with Router (the Router docs, Oct 7 2026, list "Jira admin page"). Newer changelog entries: on 29 Sep
    2026 pages and sections gained displayConditions, and on 9 Oct 2026 `and`/`or`/`not` began accepting arrays.
21. CONFIRMED. The jira:fullPage docs (Aug 3 2026) say "Product-specific full page modules are being deprecated on
    September 30, 2026" and point to global:fullPage. global:fullPage is Preview (Sep 4 2026), with routePrefix
    [Mandatory] and render "[Mandatory for UI Kit only]". global:ui is EAP (Jul 1 2026) and carries the
    no-Production-Marketplace clause. Deprecated does not mean removed: schema 13.6.0 still accepts native for both
    full-page modules.
22. CONFIRMED, XCSS rules softer than stated. The UI Kit page (Sep 24 2026) quotes are verbatim. The XCSS page (Nov 24
    2025) limits XCSS to Box and Pressable, and only BoxProps and PressableProps have an xcss prop. Nested selectors are
    banned. However, "Pseudo class / element selectors are supported currently", and only "The majority of style
    attributes" must be design tokens.
23. CONFIRMED. config/nativeui.js:36 sends .tsx to ts-loader. Lines 37-55 send .jsx (outside node_modules) to Babel with
    `{pragma:'React.createElement'}`. The bundler's dependency ^7.23.4 resolves to 7.29.7. In that version,
    create-plugin.js:43 has `runtime: RUNTIME_DEFAULT = development ? "automatic" : "classic"`, and index.js passes
    `development: false`. A per-file `@jsxRuntime automatic` comment throws "pragma and pragmaFrag cannot be set when
    runtime is automatic.", because the pragma option is set. The bundler has no ProvidePlugin. 7.3.0-next.15 is
    identical (^7.29.7). The spike harness uses esbuild `jsx: 'automatic'` (prove-uikit.cjs:50).
24. CONFIRMED. prepareUpdate mutates `instance.props` in the render phase (reconciler.js:149-153). The children arrays are
    spliced in place (76-91 and 183-190), and commitTextUpdate mutates `props.text` (193-195). The same Root object is
    sent on every commit.
25. CONFIRMED. The content-wrapper.js:5 quote is verbatim. The ContentWrapper names are exactly the seven listed:
    DynamicTable head/rows, InlineEdit editView/readView, Popup content/trigger and Comment avatar/content. AdfRenderer does
    NOT use ContentWrapper. It rewrites the document in the app and sends `document` and `documentWithoutMedia` props.
    Comment's other element-valued props pass through raw via `...rest`.
26. CONFIRMED. renderer/src/component-map.tsx:1152 in forge-sim is verbatim. Its Textfield mapping (790-801) passes only
    name, placeholder, defaultValue, value, isDisabled and onChange.
27. CONFIRMED, but the proposed test is flawed. The Tabs docs (Jan 29 2024) quotes are verbatim, including that id is
    required and that onChange receives the index and a UIAnalyticsEvent. Every TabPanel is always in the ForgeDoc. For
    uncontrolled Tabs with no onChange, switching tabs produces NO new reconcile, so "assert the panel text in the next
    snapshot" cannot work there. Assert the host's selected index or the visible panel instead.
28. CONFIRMED. FormProps.codegen.d.ts:14 and the Form docs (Jun 27 2024) carry the sentence. The examples use
    `handleSubmit(fn)` and `<Button appearance="primary" type="submit">`. However, FormProps.onSubmit is
    `() => Promise<void|boolean> | void`. The host passes no data. The "only when valid" gate is react-hook-form's
    handleSubmit inside the app. Whether the host withholds a raw onSubmit while fields are invalid is unknown.
29. CONFIRMED as documented, behaviour unverified. base/index.d.ts:76-102 and the UserPicker docs (Jan 29 2024) state the
    isMulti rule (array of strings, otherwise one string). They state it for the "returned form object", that is the
    `name` prop, marked "Available in macro config: Yes". With useForm, register() stores whatever onChange receives
    (useForm.js:73-75). The typed onChange argument is the object {avatarUrl,email,id,name,type}, so handleSubmit data
    holds that object unless the app maps `.id`. The contract must state the resolver payload shape. Probe the real
    product to see what onChange receives when isMulti is set.
30. CONFIRMED. ModalProps.codegen.d.ts:27 adds "Fallback to app name if not supplied for fullscreen modals." The Modal
    docs (Jun 19 2026) say:
    - a non-fullscreen modal with `title` gets a default header with the title and a close button;
    - fullscreen modals always render a header;
    - onClose is `(e: KeyboardOrMouseEvent) => void`;
    - the example is `<ModalTransition>{isOpen && <Modal onClose={closeModal}>…`.
31. REFUTED: Popup does take testId. testId is a real prop on every component in the "accept" list. It is absent from
    Form, FormHeader, FormSection, FormFooter, TabList and ModalTransition. But:
    - Popup: PopupProps is `Omit<ADS Popup props, 'popupComponent'|'zIndex'|'offset'|'boundary'>`. ADS @atlaskit/popup
      9.0.2 has `testId?: string` (types.d.ts:229), and the @forge/react Popup wrapper forwards testId (popup.js:12-13).
      Popup supports testId.
    - InlineEdit: the type allows testId through Omit, but the wrapper never forwards it (inline-edit.js:27 and 85), so
      it is silently dropped.
    - Comment: testId is typed and forwarded through `...rest`.
    The id prefix `forge-app-${uuid.slice(0,5)}-` (reconciler.js:10-26) and the random keys are confirmed.
32. CONFIRMED. The packaging page (Sep 24 2026) says verbatim "When using manual packaging, Forge CLI does not process
    the code to upload.", and adds that it "does not catch errors related to wrong types, missing variables, or
    dependencies". The Forge changelog for 28 Sep 2026 has "Manual packaging for Forge functions and UI Kit modules now
    in Preview", and the same day removed the TypeScript bundler. When `bundler` is set, the lint skips both the
    directory rule and the .html entry rule (see #19).
33. CONFIRMED. My own lock-only resolve (npm 11.12.1), with sizes summed from the registry `dist` fields:
    - 282 entries, 143.6 MB and 39,581 files (9 old packages report no size);
    - @atlaskit: 93 packages, 76.9 MB, including @atlaskit/tokens 20.4.1 (20.0 MB) and 1.61.0 (13.0 MB);
    - on-disk bound 305.7 MB;
    - @forge/manifest 13.6.0 and @sentry/node 7.106.0 are in the tree, and undici 7.30.0 arrives through cheerio;
    - exactly one react, 18.3.1.
    For 11.2.0 the cause is confirmed: it pulls forge-react-types ^0.41.14, which resolves to 0.41.25 and depends on
    @atlaskit/renderer ^118. The 3.7 GB total was not measured again. DESIGN.md:27 and :633 cite 1,246 MB.

Newer or extra sources found while checking:
- Forge changelog, 28 Sep 2026: "Manual packaging for Forge functions and UI Kit modules now in Preview", and the
  TypeScript bundler was removed.
- Forge changelog, 29 Sep 2026: displayConditions on adminPage pages and sections.
- Forge changelog, 9 Oct 2026: display conditions accept arrays under `and`/`or`/`not`.
- Forge changelog, 7 Oct 2026: the refreshed Lozenge, Tag and Badge styling is now the default in UI Kit, which changes
  pixels only.
- limits-invocation (Sep 1 2026): 1,200 calls per user per minute; 7,000 per minute and 300 per second per install; the
  429 example uses rateLimitProperties.
- Router (Oct 7 2026): Jira admin page is a supported module.
- jira:adminPage (Sep 22 2026): "Subpages only work with Custom UI."
- Modal docs (Jun 19 2026): how `title` behaves in fullscreen and non-fullscreen modals.
- Community t/82956 and t/89214: UI Kit 1 stopped working in Feb 2025.
- @forge/react 12.3.1-next.2, @forge/bridge 7.1.1-next.7 and @forge/bundler 7.3.0-next.15: no change to any mechanism
  above.
- @atlaskit/popup 9.0.2 and @atlaskit/inline-edit 16.4.6 types: both have testId.
- global-bridge.js: the transport is chosen by the iframe URL's `platformFeatureFlags` (see #7).
