# Forge 2.0 research: Forge "tiers", REST rate limits, platform limits and consumption pricing

Fetched on 2026-10-09 (all URLs below were fetched that day with curl or the Discourse/changelog JSON APIs; raw copies sit next to this file in `raw/`, `community/`, `changelog/`, `pkgs/tiers-agent/`).
Rule followed: primary sources only (developer.atlassian.com docs, the dac changelog service that backs every developer.atlassian.com changelog, Atlassian-staff posts on community.developer.atlassian.com, the Atlassian blog, the Atlassian developer status page, npm package code). Partner (non-staff) posts are quoted only as *field observations* and are labelled PARTNER.

---------------------------------------------------------------------------------------------------

## 0. The short version for the benchmark designer

1. **What Atlassian calls a "tier"** (three different things share the word):
   - **Rate-limit tier** (the one that matters for "dose itself for Tier 1"): Jira/Confluence REST points quotas. **Tier 1 = Global Pool = 65,000 points per hour per app, shared by ALL tenants the app is installed on.** Tier 2 = Per-Tenant Pool, granted only after Atlassian review.
   - **Forge pricing "free tier"**: an informal name for the monthly *free usage allowance* of consumption pricing (in effect since 2026-01-01). It is billed, not enforced as a hard stop.
   - **LLM "model tier"**: Haiku / Sonnet / Opus in Forge LLMs.
2. Three independent Jira limits run at once: hourly points quota, per-second per-endpoint burst (token bucket, per tenant, shared by every app and API token on the tenant), and per-issue write limits (20 writes / 2 s, 100 writes / 30 s).
3. **Quota exhaustion is a hard wall until the top of the next UTC hour** ("There is no gradual throttling"). Staff claimed occasional-spike forgiveness; a partner measured none. A benchmark should emulate a deterministic hard wall and say so.
4. The app **cannot see its own consumption below ~80% usage**: `RateLimit` carries `r` (remaining) only once usage passes ~80% (`X-RateLimit-NearLimit: true` when <20% remains). Forge storage is per installation, so there is no platform-native cross-tenant counter. Dosing therefore needs **local per-installation self-accounting**, plus reacting to `NearLimit`/`r`/`t`.
5. **Frontend `@forge/bridge.requestJira` calls are exempt from points** (latest staff word, 2026-04-01). Backend (`@forge/api` asApp/asUser), Forge Remote and `invokeRemote` flows are counted. The public docs say none of this; it is staff-only. **A benchmark must STATE its rule.**
6. Forge platform limits (invocations, KVS, SQL, Realtime, async events, LLM) are a **separate system**, enforced per installation. Forge consumption pricing is a third system (billing).

---------------------------------------------------------------------------------------------------

## 1. Terminology: which thing Atlassian calls a "tier"

| Sense | Atlassian wording (verbatim) | URL |
|---|---|---|
| REST rate-limit tier | "Tier 1 – Global Pool (default)" / "Tier 2 – Per-Tenant Pool" | https://developer.atlassian.com/cloud/jira/platform/rate-limiting/ |
| REST rate-limit tier (changelog) | "We’re also introducing two types of app-level quotas (tiers) that apply consistently across the Atlassian platform for all apps." | https://developer.atlassian.com/changelog/#CHANGE-2958 |
| Forge pricing | "Free Usage Allowance: Each Forge app includes a generous monthly usage allowance at no cost." and "For practical techniques to keep your app within the free tier or minimise overage charges, see Optimise Forge platform costs." and "Apps exceeding the free tier will be eligible for enhanced support and SLAs, including 99.90% uptime for Compute, KVS, and SQL." | https://developer.atlassian.com/platform/forge/forge-platform-pricing/ |
| Forge platform limits page | "In addition to platform quotas and limits, Forge apps may also be affected by Atlassian app-specific rate limits, such as when making REST API calls to Jira or Confluence." | https://developer.atlassian.com/platform/forge/platform-quotas-and-limits/ |
| LLM model tier | "Forge LLMs supports Claude models across three tiers: Haiku, Sonnet, and Opus." | https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-models/ |

Conclusion: "Tier 1/Tier 2" in the owner's mandate ("dose itself for tier 1 usage") = the **REST points quota tiers**. Forge "quotas" (platform) were *retired* in favour of consumption pricing: "Quotas were originally introduced to prevent abuse of the Forge platform by placing strict upper limits on usage. With the introduction of the new consumption-based pricing model, quotas are no longer necessary." (https://developer.atlassian.com/platform/forge/platform-quotas-and-limits/, last updated Jul 13, 2026)

---------------------------------------------------------------------------------------------------

## 2. Jira/Confluence REST points-based quotas (Tier 1 / Tier 2)

Sources:
- Jira: https://developer.atlassian.com/cloud/jira/platform/rate-limiting/ (page says "Last updated Oct 9, 2026")
- Confluence: https://developer.atlassian.com/cloud/confluence/rate-limiting/ ("Last updated Oct 9, 2026")

### 2.1 Scope and the three systems
- "Enforcement of the new points-based API rate limits and tiered quota rate limits for Jira and Confluence Cloud apps will begin on March 2, 2026" (banner on both pages; still worded in future tense on 2026-10-09).
- "The new rate limits will apply to all Forge, Connect, and OAuth 2.0 (3LO) apps. If your app integrates with Jira or Confluence Cloud, we recommend reviewing the updated documentation on rate limits and best practices for optimizing API usage. API token-based traffic is not affected by this change, and will continue to be governed by existing burst rate limits."
- Jira: "Jira Cloud enforces three independent rate limiting systems that work simultaneously to protect platform stability. Your app or integration must handle all three:" — "Points-based quota (per-hour)", "Burst API rate limits (per-second)", "Per-issue write limits".
- "When any limit is exceeded, Jira returns an HTTP 429 Too Many Requests response. Your app should handle this gracefully by respecting the Retry-After header and implementing appropriate backoff strategies."

### 2.2 How points are computed
- "Points are calculated based on the type of API request and the objects affected. Each request starts with a base cost of 1 point, and additional points are added for each object involved. Write requests are charged only the base cost, with no additional points."
- "This straightforward model applies to both REST and GraphQL APIs."
- Jira cost table (verbatim rows, table cells joined with |):
  - "Core domain objects (GET, GraphQL query) | 1 point | Standard read operations on primary content | Issues, Projects, Dashboards, Attachments"
  - "Identity & access (GET, GraphQL query) | 2 points | Reads involving authentication or permissions | Users, Groups, Project Roles, Permissions"
  - "Write / modify / delete (POST, PUT, PATCH, DELETE, GraphQL mutation) | 1 point | Operations that create, update, or remove data | Create or edit issues"
  - "Others | 1 point | Read operations on uncategorized objects | Endpoints or fields not listed above (default cost applies)"
- Confluence table is the same with examples "Pages, Spaces, Attachments" (core), "Users, Groups, Permissions" (identity), "Create or edit pages" (write).
- "Note: We plan to expand our catalog in the future to provide more detail on object costs. Most requests are dominated by object costs."
- Worked examples (Jira): "GET /rest/api/3/issue/ABC-123" → "1 (base) + 1 Issue = 2 points"; "GET /rest/api/3/group/member?groupname=my-group" → "1 (base) + 8 users = 17 points (1 + 8 × 2)"; "POST /rest/api/3/issue → 1 point"; "If Alex's script creates 50 issues in a batch, that's 50 points consumed from the quota (50 issues × 1 point each)."
- Worked examples (Confluence): "GET /wiki/rest/api/content/123456" → "1 (base) + 1 (Page object) = 2 points"; "GET /wiki/rest/api/space/DOCS" → "1 (base) + 1 (Space object) = 2 points"; "GET /wiki/rest/api/user?accountId=557058:12345678-abcd-1234" → "1 (base) + 2 (User object) = 3 points"; "POST /wiki/rest/api/content" → "1 point"; "PUT /wiki/rest/api/content/123456" → "1 point".
- FAQ: "Each object type has a published point value (e.g., Issues = 1, Users = 2). Unlisted objects default to 1 point. The catalog will expand over time."

### 2.3 Quotas, windows, tiers
- "All quotas are measured in points per hour and reset at the top of each UTC hour."
- "Your app's hourly quota depends on two factors: - Rate limit tier: Global Pool (default) or Per-Tenant Pool. - Customer edition: In the Per-Tenant Pool, Free, Standard, Premium, or Enterprise and the number of users."
- **Tier 1**: "Your app shares a single 65,000 point hourly quota across all tenants. This is the default tier for all apps. Most apps operate comfortably within the Global Pool."
- **Tier 2**: "Your app receives a separate hourly quota for each tenant, with limits varying by their edition. Only apps with exceptionally high or concentrated usage patterns may be assigned to the Per-Tenant Pool after review."
- Tier 2 table: "Tier 2 – Per-Tenant Pool | 65,000 points/hour | 100,000 + 10 × users points/hour | 130,000 + 20 × users points/hour | 150,000 + 30 × users points/hour" (columns Free, Standard, Premium, Enterprise).
- "Standard: 100,000 base + 10 points per user per hour", "Premium: 130,000 base + 20 points per user per hour", "Enterprise: 150,000 base + 30 points per user per hour".
- "Per-tenant rate limits are capped at 500,000 points per hour for Standard, Premium, and Enterprise editions."
- Examples: "Standard tenant with 2,000 users: 100,000 + (10 × 2,000) = 120,000 → 120,000 points/hour"; "Enterprise tenant with 15,000 users: 150,000 + (30 × 15,000) = 600,000 → 500,000 points/hour (capped)".
- Confluence: "Each app or token has a set quota of points per hour, which resets at the top of each hour (UTC). If your usage exceeds the quota, further requests are denied with an HTTP 429 response until the next window begins. There is no partial throttling once the quota is reached, all requests are blocked until reset."
- Confluence: "There is no carry-over between hours; unused quota does not accumulate."
- FAQ (both): "You will receive a 429 Too Many Requests response with a Retry-After header. All requests are denied until the next hourly reset. There is no gradual throttling."
- FAQ: "Can limits be increased if needed? | No, not on demand. Quota increases require a review by Atlassian for Tier 2 Per-Tenant Pool eligibility."
- FAQ: "What is the scope of the rollout? | Jira and Confluence APIs. REST enforcement comes first, followed by GraphQL at a future date which will be announced."
- FAQ: "Use the Partner Portal for documentation and app quota increase requests, or please contact your Atlassian representative."

Staff clarification that Tier 1 is per app (not per developer, not per site):
- MaheshPopudesi (Atlassian Staff), 2025-12-22, https://community.developer.atlassian.com/t/2026-point-based-rate-limits/97828/84 : "In Tier 1 (Global Pool), each app receives its own 65K points/hour quota, shared across all of that app’s tenants." / "In Tier 2 (Per‑Tenant Pool), each app receives a separate quota per tenant, which varies based on that tenant’s product edition and user count."
- MaheshPopudesi, 2026-02-11, https://community.developer.atlassian.com/t/2026-point-based-rate-limits/97828/148 : "Confirming - 65,000 pts/hr is per app, shared across all tenants for that app in the Global Pool. We will tighten the FAQ wording to be precise."

Clustering of app copies (Atlassian blog, Alan Braun, Head of Product Ecosystem, published 2025-12-12, modified 2026-04-20), https://www.atlassian.com/blog/development/evolving-api-rate-limits :
- "To maintain performance, reliability, and security, we will cluster apps that are functionally identical or created as copies, such as separate apps deployed for each customer rather than a single distributable app. These clustered apps share one rate limit quota, treating them as a single app."
- "All apps begin in Tier 1 by default. Tier 2 access can be requested for apps with elevated workloads, pending a review of usage patterns and eligibility criteria. If your app requires real-time data access or supports AI-based features and does not meet the Tier 2 criteria, consider using Atlassian Rovo MCP as an alternative to direct API queries."

### 2.4 How an app gets into / moves between tiers
- Default: Tier 1 (docs above). Only by Atlassian review: "Per-Tenant Pool: Only available after Atlassian review and meant for apps with sustained, high, or concentrated usage." (FAQ)
- Changelog CHANGE-2958 (2025-12-12): "Apps start in Tier 1 - Global Pool, with a shared hourly quota across all tenants. Apps with consistently high or concentrated usage may qualify for Tier-2 Per‑Tenant Pool, which provides dedicated hourly quotas per tenant." and "Planned updates to the developer console to show which tier your app belongs to ahead of the enforcement" (https://developer.atlassian.com/changelog/#CHANGE-2958)
- Staff (2025-12-17, #31): "Based on our traffic analysis for the past year, ~95% of the apps never cross boundaries of the Global Pool; of those that did, we proactively moved qualifying apps to Tier 2." and "The current escalation path for requesting additional capacity is via a support ticket in the Partner Portal. Increase Marketplace App Limits." and "During an active review, the app will receive a grace period so that the app’s customers are not affected." (https://community.developer.atlassian.com/t/2026-point-based-rate-limits/97828/31)
- Staff (#57): "free apps can also be in Tier 2. We’ve already reviewed traffic across the ecosystem and proactively moved hundreds of free apps into Tier 2"
- Staff (#79): "We are working to ensure that any apps that migrated to Forge have the same Tier assignment as their Connect apps."
- Blog: "Check your app’s tier. Once available, you’ll be able to see whether your app is in Tier 1 (Global Pool) or Tier 2 (Per‑Tenant Pool) in the Developer Console."
- Exemptions (staff only, not in public docs): DC→Cloud migration (2026-01-31, #133): "App traffic originating from tenants that are actively migrating from Data Center (DC) to Cloud is exempt from points-based app quota rate limits for the duration of the migration window, regardless of the app’s rate-limit tier." Regulated clouds (#61): "I wanted to indicate that IC and FedRamp are not part of this rollout."

### 2.5 Enforcement timeline and current status
- CHANGE-2958 (2025-12-12): "A phased enforcement of the new rate limits will begin on February 2nd, 2026."
- CHANGE-3003 (2026-01-12): "Enforcement of point-based rate limits has been postponed from February 2, 2026 to March 2, 2026." (https://developer.atlassian.com/changelog/#CHANGE-3003)
- CHANGE-3045 (2026-02-02, beta headers): "When these two headers are returned without the Beta- prefix (RateLimit, RateLimit-Policy), points-based quota limits are actively enforced, and requests may be rate limited." (https://developer.atlassian.com/changelog/#CHANGE-3045)
- CHANGE-3080 (2026-03-02): "Effective March 2, 2026, we are starting the phased enforcement of points-based quota rate limits for Jira and Confluence Cloud REST APIs. The rollout will begin with a small percentage of apps and gradually expand over several weeks, allowing us to closely monitor progress and minimize any disruption." and "To learn whether points-based quota enforcement has started for your app, inspect your API response headers. Quota-related headers with a Beta- prefix (e.g., Beta-RateLimit-Policy: "global-app-quota") indicate enforcement has not yet begun for your app." and "We plan to discontinue sending quota rate limit values via the X-RateLimit-* headers in the future. A timeline will be published separately." (https://developer.atlassian.com/changelog/#CHANGE-3080)
- Blog: "Starting March 2, 2026: Phased enforcement begins for the new rate limits across Jira and Confluence REST APIs, followed by GraphQL endpoints in subsequent phases."
- No changelog entry announcing completion of the phased rollout was found (changelog service searched for "rate limit", "points-based", "quota", "tier", "Tier 2", "429", "Retry-After", "headers" on 2026-10-09).
- PARTNER, 2026-07-23 (#177): "My app is seemingly still not enrolled as I’m still seeing the Beta prefix in the headers" with "Beta-Ratelimit-Policy: “tenant-app-quota”;q=480000;w=3600".

### 2.6 Spike "forgiveness": staff statement vs field measurement
- Docs: "Enforcement is designed to account for normal variability in traffic, and brief or infrequent spikes may not immediately result in rate limiting. Clients should nevertheless treat published quotas as fixed limits and implement appropriate backoff and retry logic."
- Staff (#31): "The hourly quota is intended as an accounting boundary, not as a guarantee that an app will be unavailable for a full hour if a spike occurs."
- Staff (#84): "We’ve designed the system to tolerate occasional overages above the points/hour allocations without immediately impacting your tenants. Only sustained overage over time will trigger hard rate limiting."
- Staff (#96): "If your app reaches or exceeds the per-hour rate limit quotas, we will still let the app continue to operate without impacting your customer workflows. However, sustained overage over time will be rate-limited."
- Staff (#103, Alan Braun): "We’re not going to share exact internal thresholds or “guarantees” around how other apps will behave under specific scenarios."
- PARTNER scott.dudley (#163, 2026-03-18): "As soon as my global-pool test app meets the 65k threshold, access to all APIs is slammed shut and it does not reset until the next hour. I have repeated this 5-6 times and the behaviour is consistent."
- PARTNER klaussner, "Rate limit abuse (new attack vector)", https://community.developer.atlassian.com/t/rate-limit-abuse-new-attack-vector/99654 : "I’ve created a simple Confluence app (Tier 1) that fetches 250 pages when the user clicks a button (one API request). It takes only about 3 minutes for a single user to exceed the global quota by clicking the button repeatedly, rendering the app unusable for up to one hour on all tenants." Staff reply (#2, 2026-03-19): "The Tier 1 Global Pool is a shared quota across tenants and is intended to keep the model simpler for the majority of apps that operate well within those limits. Apps with higher per-request point costs can be more sensitive to concentrated usage from a single tenant."
- => The cross-tenant denial-of-service is a REAL, staff-acknowledged property of Tier 1. A benchmark can legitimately test that the app stops one tenant (or one user) from burning the shared pool.

### 2.7 What traffic counts (frontend vs backend) — staff only, contradictory over time
- 2025-12-22 (#84): "Any request that invokes Jira or Confluence Product APIs will be counted toward the rate limits, including calls originating from the Forge UI."
- 2026-01-31 (#133): "For the March 2 enforcement, only app‑initiated backend traffic counts toward points‑based rate limits. Direct, user‑initiated UI calls from Forge UI to Jira or Confluence using @forge/bridge.requestJira (with no resolver or backend) is treated as standard UI traffic and is not included in points. By contrast, any Jira or Confluence API calls made by a Forge backend or remote service are included in points‑based rate limit calculations. We may include this category of traffic in points‑based limits in the future, with clear advance notice, but it will not be counted as part of the March 2 enforcement."
- 2026-01-31 (#135), counted flows: "App‑initiated backend calls - for example, UI → resolver → backend (@forge/api)"; "Forge Remote flows invoked from the UI - for example, UI → @forge/bridge.invokeRemote() → Forge backend/remote service → Jira/Confluence API"; "Forge Remote flows invoked from backend code".
- PARTNER BurakAKTEPE (#154, 2026-02-27): 50 UI-only `requestJira('/rest/api/3/search/jql', POST)` calls moved tenant-app-quota by "Delta used: 571 points".
- 2026-04-01 (#168): "We confirmed that Forge front-end traffic is exempted."
- Function invocations are NOT points (HeyJoe #127, 2026-01-29): "function invocations are governed by a fixed rate limit as documented on https://developer.atlassian.com/platform/forge/limits-invocation/. Function invocations will not be subject to the points-based rate limiting for REST APIs being discussed in this thread." and (#129) "Forge function invocations and REST API rate limits are governed by separate systems."
- The public rate-limiting pages contain no statement about frontend exemption, migrations, or Connect-on-Forge (grep on 2026-10-09).
- Forge cost guide nudges toward frontend calls: "Forge's UI Kit and Custom UI frontends run entirely in the browser and are not subject to function invocation costs or limits." and "Note that API requests from the frontend are always authenticated as the context user. Secure operations, such as accessing Forge storage, can only be accessed via a resolver." (https://developer.atlassian.com/platform/forge/optimise-forge-costs/, last updated Aug 21, 2026)
- Bridge requestJira runs as the user: "There is no equivalent of asApp() on the @forge/bridge package: calls from front-end code always run with the permissions of the current user." (https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/requestJira/, last updated May 27, 2026)

---------------------------------------------------------------------------------------------------

## 3. Burst limits, per-issue write limits, other endpoint-specific limits

### 3.1 Jira burst (token bucket) — https://developer.atlassian.com/cloud/jira/platform/rate-limiting/
- "Burst API rate limiting in Jira Cloud controls how many requests a single tenant can send per second to a given REST API endpoint. This is a short term “spike” safeguard that is separate from the hourly, points based rate limit."
- "It is enforced per tenant and per API/resource path (for example, /rest/api/3/issue and /rest/api/3/search each have their own burst behavior for the same tenant)."
- "The burst limit is independent of the number of users in the tenant; adding more users does not increase this per second allowance."
- "Hitting the burst threshold for one endpoint affects only that endpoint for that tenant; other endpoints and other tenants are unaffected."
- "Steady-state refill rate: The sustained number of requests per second your app should be designed to handle (e.g., 10 requests/second)" / "Burst buffer: The total bucket size that allows for temporary traffic spikes above the steady-state rate (e.g., 100 tokens)"
- "Design for steady-state limits: Your app should be designed around the steady-state refill rate, not the burst buffer. The burst buffer exists to absorb occasional spikes, but relying on it for normal operations will lead to rate limit errors."
- "Each API request consumes one token from that endpoint's bucket."
- Defaults: "By default, each REST API endpoint has its own bucket categorised by the HTTP method of the request. The values below show the default steady state requests per second (RPS) limits for the given API based on its HTTP method." → "GET | 100", "POST | 100", "PUT | 50", "DELETE | 50".
- Custom limits table (verbatim subset): "GET | /api/{version}/issue/{issueidorkey} | 150", "GET | /api/{version}/issue/{issueidorkey}/changelog | 200", "GET | /api/{version}/user | 150", "GET | /api/search/user | 200", "POST | /api/{version}/search/approximate-count | 150", "POST | /api/{version}/expression/evaluate | 150", "POST | /api/{version}/permissionscheme/{schemeid}/permission | 100", "GET | /servicedeskapi/servicedesk/{servicedeskid}/customer | 5", "PUT | /api/{version}/component/{id} | 500". (The Jira page's table also lists Confluence-looking paths such as "GET | /api/content/{id}/state | 400".)
- Burst 429 example: "HTTP/1.1 429 Too Many Requests", "Retry-After: 1", "X-RateLimit-Limit: 350", "X-RateLimit-Remaining: 0", "X-RateLimit-Reset: 2026-01-01T01:01:01Z", "RateLimit-Reason: jira-burst-based".
- The burst bucket is shared by every app and API token on the tenant — CHANGE-2753 (2025-08-28): "Per-tenant and per-API limits. All programmatic requests (from apps or API tokens) for a tenant share the same API bucket, regardless of the number of API token tokens or installed apps." and "Limits are applied consistently across all Cloud plans and are not affected by the number of seats or installed apps." (https://developer.atlassian.com/changelog/#CHANGE-2753)
- Staff burst announcement, https://community.developer.atlassian.com/t/action-required-update-your-apps-to-comply-with-jira-cloud-burst-api-rate-limits/97202 (#1, 2025-11-21): "These limits are enforced on a per-tenant and per-API basis." and "Rate limits are enforced using industry-standard token bucket algorithms, allowing for short bursts of activity but maintaining a sustainable average rate over time."
- Bucket CAPACITY (burst buffer) per endpoint is **not published**; only the steady-state RPS is.

### 3.2 Confluence burst
- "Quota and burst rate limits are enforced independently. Burst limits are evaluated over short time windows (typically seconds) to prevent traffic spikes, while quota limits are evaluated hourly. Even if you remain within your hourly quota, a rapid surge of requests can trigger burst limiting. Burst limits reset quickly, allowing normal operations to resume within seconds. Certain high-impact endpoints (Permissions, Search, Admin operations) enforce additional burst protections for system stability." No numbers published.

### 3.3 Jira per-issue write limits
- "Per-issue rate limiting restricts the number of write operations (create, update, delete) that can be performed on a single Jira issue within specific time windows."
- "Short window: 20 write operations per 2 seconds" / "Long window: 100 write operations per 30 seconds"
- "RateLimit-Reason header: jira-per-issue-on-write"
- "For jira-per-issue-on-write: Add delays between writes to the same issue, but you can continue making other API requests normally."

### 3.4 Other endpoint-specific limits (Jira OpenAPI spec, https://developer.atlassian.com/cloud/jira/platform/swagger-v3.v3.json, version 1001.0.0-SNAPSHOT-0c0db68a…)
- User search: 429 description on 8 operations (e.g. GET /rest/api/3/user/assignable/search, GET /rest/api/3/user/picker): "Returned if the rate limit is exceeded. User search endpoints share a collective rate limit for the tenant, in addition to Jira's normal rate limiting you may receive a rate limit for user search. Please respect the Retry-After header."
- Bulk edit/move/transition/delete/watch (https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-bulk-operations/): "The bulk edit and move APIs are subject to the usual rate limiting infrastructure in Jira. For more information, refer to Rate limiting. Additionally, at any given time, only 5 concurrent requests can be sent across all users." and "A single request can accommodate a maximum of 1000 issues (including subtasks) and 200 fields." Tasks are asynchronous (`taskId`, poll `GET /rest/api/3/bulk/queue/{taskId}`; "Note: You can view task progress for up to 14 days from creation.").
- Forge License API (https://developer.atlassian.com/platform/forge/apis-reference/license-api/, Jul 14, 2026): "Per caller installationId | 1 request per 5 minutes", "Per tenant | 10 requests per minute", "Because all apps on the same tenant share the per-tenant limit, your app may hit a 429 Too Many Requests response even if it hasn't reached the per-installation limit. When this happens, wait for the number of seconds specified in the Retry-After response header before retrying." and "We recommend caching license data and using a much lower frequency than the hard limit — for example, once per hour per installationId."
- Undocumented reasons seen in the field (PARTNER): "jira-max-concurrent-threads-across-all-instances-per-tenant-per-user" (#167, 2026-03-31) and `"RateLimit-Reason":"jira-cost-based"` with `"X-RateLimit-Limit":"600000"` and a separate `"Retry-After":"89"` (#169, 2026-04-09).
- Gateway-level limit (Atlassian Developer Status incident, 2026-04-22..23, https://developer.status.atlassian.com/incidents/bb38p4wdd7mz): "If your app is impacted by this issue, the 429 response will contain the following headers: X-Failure-Category: FAILURE CLIENT RATE LIMITED X-Ratelimit-Limit: 360000 X-Ratelimit-Remaining: 0 Note that this gateway-level rate limiting does not return a Retry-After header. You can use the X-Ratelimit-Reset header to determine when the rate limit window resets and implement a backoff strategy accordingly." Resolution: "Our teams have implemented initial remediation by increasing certain rate limits".

---------------------------------------------------------------------------------------------------

## 4. Response headers and bodies near/at the limit

### 4.1 Enforced ("current") headers — Jira page table
- "X-RateLimit-Limit | The maximum request rate enforced for the current rate-limit scope. For request rate limits, this reflects the allowed requests per second."
- "X-RateLimit-Remaining | The remaining request capacity within the current rate-limit window. For request rate limits, this represents remaining requests in the current second."
- "X-RateLimit-Reset | Only returned with 429 responses. ISO 8601 timestamp when the current window resets." (Confluence table: "X-RateLimit-Reset | ISO 8601 timestamp when the current window resets" — no 429-only restriction.)
- "X-RateLimit-NearLimit | Returns true when less than 20% of capacity remains. Not used for request rate limiting." (Confluence: "Returns true when less than 20% of the quota remains")
- "RateLimit-Reason | Only returned with 429 responses. The reason for throttling:- jira-quota-global-based — Global pool quota exceeded- jira-quota-tenant-based — Per-tenant pool quota exceeded- jira-burst-based — Request rate limit exceeded- jira-per-issue-on-write — Per-issue write rate limit exceeded"
- Confluence: "RateLimit-Reason | The reason for throttling: • confluence-quota-global-based – Global Pool limits breached• confluence-quota-tenant-based – Per-Tenant Pool limits breached"
- "Retry-After | Only returned with 429 responses. Indicates how many seconds to wait before retrying."
- "Some transient 5xx responses (such as 503) may also include a Retry-After header. While these are not rate limit responses, you can handle them with similar retry logic."
- Example quota 429 (Jira): "Retry-After: 1847", "X-RateLimit-Limit: 100000", "X-RateLimit-Remaining: 0", "X-RateLimit-Reset: 2025-10-08T15:00:00Z", "RateLimit-Reason: jira-quota-global-based". (Confluence example uses "X-RateLimit-Limit: 40000" and "RateLimit-Reason: confluence-quota-global-based".)
- Staff (#145, 2026-02-05): "The X-RateLimit-* headers will continue to be used for burst rate limits and per-issue rate limits, they’re not going away. For points based quotas we will use the new Beta-RateLimit and Beta-Ratelimit-policy to give you clear view of points usage and tiers (RateLimit and RateLimit-policy at enforcement) And, yes Retry-After will be returned."
- Staff (#146): "X-RateLimit-NearLimit will continue to indicate when less than 20% of your hourly quota remains. At enforcement, you’ll also have the new RateLimit header with the r (remaining) parameter which gives you the exact number of points remaining as well when 20% of the hourly quota remains."
- PARTNER (#157, 2026-03-16): "The constants described in the docs for the RateLimit-Reason field do not match what is returned in real life, or at least not for Confluence." Staff (#160): "We will fix the documentation".

### 4.2 Structured headers (`RateLimit`, `RateLimit-Policy`; `Beta-` prefixed while not enforced)
- "Beta headers are informational only and do not trigger enforcement or throttling. You can use them now to monitor your usage and prepare for future enforcement. At enforcement Beta- prefix will be dropped from all beta headers."
- Format: "A response header may contain one or more policy entries. Each policy entry consists of: - A policy name (quoted string) - One or more attributes expressed as key=value pairs - Attributes are separated by ; - Multiple policy entries are separated by ," → `Beta-RateLimit: "<policy-name>";<attribute>=<value>[;<attribute>=<value>], ...`
- "Clients must not assume a fixed number or ordering." (Confluence: "Clients should parse these as comma-separated entries and must not assume a fixed number or ordering.")
- Parameters: "Beta-RateLimit-Policy | q | Total quota", "Beta-RateLimit-Policy | w | Time window in seconds", "Beta-RateLimit | t | Seconds until reset".
- `r`: Jira: "Beta-RateLimit | r | Remaining quota. Optionally included. If absent, your app is well within its limits." Confluence: "Remaining quota. Conditionally included, present only when usage exceeds ~80% of the quota. If absent, your app is well within its limits. Treat as optional in your parsing logic."
- Policy names: "global-app-quota | A quota applied to your app globally across all tenants (Tier 1)", "tenant-app-quota | A quota applied per tenant, per app (Tier 2)", Jira only: "jira-burst-based | A finer-grained quota applied per API endpoint, per HTTP request method".
- Examples (verbatim): normal: `Beta-RateLimit-Policy: "global-app-quota";q=65000;w=3600` + `Beta-RateLimit: "global-app-quota";t=3200`; near limit: `Beta-RateLimit: "global-app-quota";r=11000;t=600` ("Once usage approaches this threshold, responses will consistently include the remaining quota (r)."); exceeded: `Beta-RateLimit: "global-app-quota";r=0;t=50` + `Beta-Retry-After: 50`; multi-policy: `Beta-RateLimit-Policy: "global-app-quota";q=65000;w=3600,"jira-burst-based";q=100;w=1` and `Beta-RateLimit: "global-app-quota";t=200,"jira-burst-based";r=90;t=1`; mixed enforcement: `RateLimit-Policy: "global-app-quota";q=65000;w=3600` + `Beta-RateLimit-Policy: "jira-burst-based";q=100;w=1` ("In the case where one Rate Limit is enforced and the other isn't, a combination of Beta- prefixed and non-prefixed headers will be present in the response.").
- Confluence: "For points-based quota enforcement, only RateLimit and RateLimit-Policy will be used — the existing X-Beta-RateLimit-* and X-RateLimit-* headers will not be used for quota enforcement. Standard HTTP headers such as Retry-After continue to apply where relevant." and "The legacy X-Beta-RateLimit-* headers will not be used for points‑based quota."
- Legacy beta headers (Jira): "X-Beta-RateLimit-Reset | Only returned with responses which would be rate limited at enforcement."; "Beta-Retry-After | Only returned with responses which would be rate limited at enforcement. Indicates how many seconds to wait before retrying." (introduced by CHANGE-2410, 2025-03-13).
- Staff (#144): "This is exactly the direction we’re heading. The structured format was designed with this kind of consolidation in mind, unifying rate limit policies into a single set of headers."
- PARTNER field logs (#165, 2026-03-26) show real values: `Beta-Ratelimit-Policy: "tenant-app-quota";q=100100;w=3600`, `Beta-Ratelimit: "tenant-app-quota";r=17842;t=2107`, `X-Beta-Ratelimit-Nearlimit: true`, `X-Beta-Ratelimit-Reset: 2026-03-26T16:00Z` (ISO 8601 without seconds), and reported "no more Ratelimit-Policy header received" once r=0 and "the t in Ratelimit is completely off". (q=100100 matches Standard 100,000 + 10×10 users.)

### 4.3 GraphQL
- FAQ: "REST exposes usage via X-RateLimit-* headers - GraphQL exposes usage via the extensions.cost block". GraphQL enforcement "at a future date which will be announced" (no announcement found as of 2026-10-09).

### 4.4 Headers visible to the frontend?
- PARTNER scott.dudley (#1 and RoA thread #12): Forge "strips the rate-limit headers on the front end" (FRGE-1923, ECO-899).
- Staff (Suyash Kumar Tiwari, Principal PM, 2026-02-02, burst thread #13): "the supported and reliable way to read rate‑limit headers today is via a backend resolver using @forge/api, not directly from frontend requestJira calls." Then (#22, 2026-02-03): "You’re right that the earlier guidance to rely on backend resolvers adds unnecessary complexity for this use case. We’re working with our internal teams to determine how to surface the headers in Forge UI, without requiring any workarounds from you and will keep you updated on our progress."
- Current state (Oct 2026) of header exposure to bridge requestJira: NOT VERIFIED.

---------------------------------------------------------------------------------------------------

## 5. Documented client behaviour (what "good dosing" looks like per Atlassian)

From the Jira page "Best practices for handling rate limit responses" / "Implementing retry logic" / "Optimizing API usage" (Confluence has equivalents):
- "Check the Retry-After header for guidance on an appropriate retry delay."
- "Use exponential backoff with jitter: Implement retry logic that backs off exponentially rather than retrying immediately. Add random jitter to avoid the thundering herd problem. Only retry if the API is idempotent and the response includes a Retry-After header."
- Per reason: "For jira-burst-based: Reduce your request rate to the specific endpoint. Requests to other endpoints are unaffected. Remember this limit includes a burst buffer; design your app around the steady-state rate." / "For jira-quota-global-based or jira-quota-tenant-based: Pause all API requests until the window resets."
- Batching: "Single-issue bulk operations: APIs like bulk delete or move worklogs count as one operation against rate limits, even when processing multiple items." / "Multi-issue bulk operations: Issue bulk operations allow you to affect multiple issues with one request." / "Combined field updates: Merge multiple field updates into a single request instead of multiple calls."
- Resilience: "Share rate limit status between threads and services to coordinate behavior." / "Track your application's quota consumption to stay well within limits." / "Design your app to handle temporary failures and continue operating."
- Retry recipe: "Start with a base delay: Begin with a reasonable initial delay (e.g., 2 seconds)." / "Respect Retry-After: If present, use the Retry-After header value as the minimum delay." / "Double the delay on each retry: After each 429 response, double the delay up to some maximum (e.g., retry after 2, 4, 8, 16 seconds and so on)." / "Add jitter: Multiply the delay by a random factor (e.g; between 0.7 and 1.3) to spread out retry attempts amongst multiple concurrent apps or integrations and avoid the thundering herd problem." / "Set a retry limit: Cap the number of retries (e.g., 4 attempts) to prevent infinite loops."
- Pseudocode constants: "let maxRetries = 4;", "let lastRetryDelayMillis = 5000;", "let maxRetryDelayMillis = 30000;", "let jitterMultiplierRange = [0.7, 1.3];" and the line "retryDelayMillis += retryDelayMillis * randomInRange(jitterMultiplierRange);" (NOTE: this ADDS delay×U(0.7,1.3), i.e. total 1.7–2.3× — it contradicts the prose "multiply by 0.7–1.3"; also `lastRetryDelayMillis` is never updated in the pseudocode, so it never doubles. A grader must test "≥ Retry-After", not a specific jitter formula.)
- Optimising: "Request only the data you need: Use field filtering and pagination to reduce the amount of data transferred and the points consumed per request." / "Cache stable responses: Use ETags and conditional headers to avoid re-fetching unchanged data." / "Use bulk operations strategically: Bulk operations can reduce the number of HTTP calls and improve efficiency. However, check the point cost for your specific use case and ensure batching actually reduces overall quota usage." / "Leverage webhooks and context parameters: Use webhooks for event-driven updates instead of polling, and use context parameters to minimize the number of API requests needed." / "Distribute requests over time: Spread your requests evenly throughout the hour rather than sending large spikes at predictable times. Add random jitter to scheduled jobs to avoid thundering herd effects when many apps hit the API simultaneously." / "Coordinate across infrastructure: If your app uses multiple threads or nodes, share rate limit status between them to prevent accidental quota exhaustion." / "Avoid using excessive concurrency: While parallelism can improve performance, using it specifically to bypass rate limits will lead to more 429 responses and degraded performance overall."
- Confluence scheduling: "Example: Instead of scheduling all batch jobs to run at 00:00 UTC, stagger them with random delays or intervals (e.g., every 5–10 minutes) to smooth out traffic and reduce the risk of hitting your hourly quota early in the window." and "Schedule heavy jobs during off-peak hours: For large, ad-hoc operations, consider running them during periods of lower user activity." and "Use bulk operations thoughtfully: Bulk operations may consume more points per request, but can reduce the number of HTTP calls and improve efficiency. Always check the point cost and consider whether batching is optimal for your use case."
- FAQ best practices: "Request only the fields you need - Paginate large queries - Prefer metadata over full-content responses - Cache stable responses - Use exponential backoff with jitter when retrying after 429"
- Concurrency cap (Forge cost guide, https://developer.atlassian.com/platform/forge/optimise-forge-costs/): "Rate limits: Parallelisation is more efficient up to a point, but sending too many concurrent requests can trigger rate limit errors (HTTP 429). Avoid parallelising large numbers of requests at once — send at most 5–10 concurrent requests and batch the rest. See the Jira rate limiting documentation for details."
- Testing (docs): "Do not perform rate limit testing against Atlassian cloud tenants, as this may impact customers."
- Testing (staff, Suyash Kumar Tiwari, 2026-01-07, https://community.developer.atlassian.com/t/action-required-update-your-apps-to-comply-with-jira-cloud-burst-api-rate-limits/97202/7): "Simulate 429s and headers in your own unit/integration tests (mocks/fakes) to verify backoff, retry, and logging behavior." / "Optionally introduce a local “test quota” layer in staging that starts returning synthetic 429s earlier than Atlassian would, so you can see how your system behaves under sustained throttling without trying to push Atlassian Cloud to its limits." → **Atlassian itself recommends exactly what an offline emulator does.**

---------------------------------------------------------------------------------------------------

## 6. Per-operation costs and how bulk endpoints change cost

Official (only categories are published; no per-endpoint catalogue yet): see §2.2. "Most requests are dominated by object costs."

Bulk endpoint limits (official):
- Bulk fetch issues `POST /rest/api/3/issue/bulkfetch` (https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/): "By default you can request up to 100 issues in a single call. You can request up to 1000 issues in a single call when the request is shaped so that it can be served efficiently, that is, when all of the following are true: - the fields parameter explicitly names at least one field to include … - no more than 100 fields are explicitly included; - none of the included fields returns multiple values (for example comment, worklog, or attachment); and - the expand parameter does not include changelog, editmeta, operations, renderedFields, transitions, or versionedRepresentations. Requests that do not meet all of these conditions can include at most 100 issues; larger requests are rejected with a 400 error."
- Bulk fetch changelogs `POST /rest/api/3/changelog/bulkfetch`: "You can request the changelogs of up to 1000 issues and can filter them by up to 10 field IDs."
- Enhanced JQL search `GET/POST /rest/api/3/search/jql` (OpenAPI param `maxResults`): "The maximum number of items to return per page. To manage page size, API may return fewer items per page where a large number of fields or properties are requested. The greatest number of items returned per page is achieved when requesting `id` or `key` only. It returns max 5000 issues." (default 50). Pagination by `nextPageToken`. "Recent updates might not be immediately visible in the returned search results. If you need read-after-write consistency, you can utilize the reconcileIssues parameter to ensure stronger consistency assurances."
- Bulk permissions `POST /rest/api/3/permissions/check`: "A maximum of 1000 projects and 1000 issues can be checked." (This is what `@forge/api` `authorize()` calls for Jira — `@forge/auth@1.0.0` code: `requestJira('/rest/api/3/permissions/check', { method: 'post', … })`; Confluence: `requestConfluence(`/rest/api/content/${contentId}/permission/check`, { method: 'post', … })`, both via `asUser()`.)
- Bulk get users `GET /rest/api/3/user/bulk`: default maxResults 10 (OpenAPI).
- Bulk issue edit `POST /rest/api/3/bulk/issues/fields`: max 1000 issues & 200 fields per request; "only 5 concurrent requests can be sent across all users".
- Confluence v2 `GET /wiki/api/v2/pages` (https://developer.atlassian.com/cloud/confluence/openapi-v2.v3.json): `id` "Filter the results based on page ids. Multiple page ids can be specified as a comma-separated list." with schema maxItems 250; `limit` "Maximum number of pages per result to return." default 25, maximum 250. `GET /spaces/{id}/permissions` limit max 250.
- Forge cost guide table: "Fetch multiple Jira issues | GET /rest/api/3/issue/{issueId} × N | POST /rest/api/3/issue/bulkfetch"; "Update multiple issue fields | PUT /rest/api/3/issue/{issueId} × N | POST /rest/api/3/bulk/issues/fields/edit" [sic — the REST reference path is /rest/api/3/bulk/issues/fields]; "Fetch user details | GET /rest/api/3/user?accountId=X × N | GET /rest/api/3/user/bulk"; "Fetch multiple Confluence pages | GET /wiki/api/v2/pages/{id} × N | GET /wiki/api/v2/pages?id=X&id=Y&..."

How bulk changes points (derived from the published model; Atlassian has no per-endpoint table):
- Reads: bulk saves the per-request base point and burst tokens but every returned object is still charged (bulkfetch of 100 issues ≈ 1 + 100 points vs 100 × 2 = 200 for N single GETs). Reads of identity objects cost 2 each (a 200-member group page ≈ 401 points).
- Writes: a write is "only the base cost"; a bulk edit of 1000 issues is one POST (1 point) plus the polling GETs of `/bulk/queue/{taskId}`.
- PARTNER field measurements (not official): MathieuLOISELLE (#151, 2026-02-13) "These two APIs follow that rule well … project/search - search/jql Where the observed cost is always 1 + the number of returned results" while "project/{projectid}/version ~500 results → observed cost 1", "permissions/project ~1000 → 1", "user/search?query=term ~10 → 1". scott.dudley (#157): "The /api/v2/spaces/{spaceId}/permissions endpoint consumes two points. Not per call. Not per principal. Two points per permission." (480 permissions on a 2-user space → "nearly 1,000 points"). BurakAKTEPE (#154): 50 POST search/jql = 571 points (≈ 1 + ~10.4 issues each). → POST search is charged like a read (per object), not like a write, despite the "POST = write = 1 point" table row. Staff (#160): "I will review the point costs you highlighted and circle back." (no follow-up found).

---------------------------------------------------------------------------------------------------

## 7. Forge PLATFORM limits (enforced; separate from REST tiers)

Overview: https://developer.atlassian.com/platform/forge/platform-quotas-and-limits/ (Jul 13, 2026): "Forge has transitioned to a consumption-based pricing model (effective January 1, 2026). This means developers only pay for what they use while still benefiting from a generous free usage allowance. Platform limits remain in place to maintain fair use, predictable performance, and overall platform reliability." and "Fair usage: While we have removed most hard limits in favor of consumption pricing, strictly abusive or unstable usage patterns are not permitted. Atlassian reserves the right to contact you, throttle, or suspend apps that jeopardize the stability of the Forge platform or degrade performance for other users."

### 7.1 Invocations — https://developer.atlassian.com/platform/forge/limits-invocation/ (Sep 1, 2026)
- "User-led invocations have the following rate limits, which are applied on a fixed one-minute window:" — "Per user | 1,200 per minute | Maximum number of invocations per user on a single installation"; "Per install | 7,000 per minute and 300 per second (whichever is hit first) | Maximum number of invocations across all users on a single installation".
- "If a request is rate limited (429), wait until the current window resets before retrying." Sample uses `invoke(functionKey, payload, { rateLimitProperties: true })` and `error.metadata?.rateLimitProperties?.rateLimitReset`; web trigger sample uses `response.headers.get('X-Ratelimit-Reset')`.
- CHANGE-3420 (2026-09-01): "Per app installation: The limit is changing from 5,000 requests per minute (RPM) to 300 requests per second (RPS) or 7,000 RPM (whichever is hit first)." / "Per environment: The previous global limit of 30,000 RPM is being removed to provide more flexibility for apps with large numbers of installations." / "Per user: The limit remains unchanged at 1,200 RPM."
- RFC-130 (staff Lily Yang, https://community.developer.atlassian.com/t/rfc-130-upcoming-changes-to-invocation-rate-limits/100191): scheduled triggers and async/product events are not subject to these user-invocation limits ("Scheduled triggers | There are separate limits applied to the number of scheduled triggers per app so they will not be restricted by invocation rate limits"; "Atlassian app events and async events (via the async events API) | Rate limiting events are handled gracefully by the Forge platform and invocations are eventually consistent"). HeyJoe (#18): "The proposed limits here are soft limits, not hard limits, and we are retaining the ability to make adjustments if needed."
- Client-side limiter in `@forge/bridge@7.1.0` (npm package code, `out/invoke/invoke.js`): `withRateLimiter(_invoke, 500, 1000 * 25, 'Resolver calls are rate limited at 500req/25s')` — a per-page fixed window that throws `BridgeAPIError` before any network call. Same 500/25 s for `invokeRemote`/container `invokeService` (`out/invoke-endpoint/invoke-endpoint.js`).
- Runtime: "Runtime seconds (also includes UI modules invoked by Forge Remote) | 25 | Maximum runtime permitted before the app is stopped."; "Runtime seconds (async events and scheduled trigger module) | 900 | … Default timeout is 55 seconds. Use timeoutSeconds to extend it."; "Runtime seconds (web trigger, action and rovo:agentConnector modules) | 55"; "Single outbound request timeout (async events) | 180".
- Network: "Egress requests | 100 per runtime minute (rounded up) | Number of network requests per invocation, excluding those made using requestJira or requestConfluence."; "Egress requests | 50,000 requests per minute, per app for egress calls"; "Network requests | 3,000,000 requests per minute, per app and 100,000 requests per minute, per app, per tenant | The maximum number of requests per minute that an app can make for network calls, including those made using requestJira or requestConfluence."; "Offline user impersonation tokens | 1,000 requests per minute, per app".
- Memory/payload: "Memory | 1,024MB | Available memory per invocation. Default memory limit is 512MB."; "Payload size | 5MB"; "Front-end invocation request payload size | 500KB"; "Front-end invocation response payload size | 5MB"; "Log lines per invocation | 100 per runtime minute (rounded up)"; "Log size per invocation | 200 KB".
- Developer console shows invocation rate limiting: CHANGE-2551: "The new Rate limited label will be shown in the usual error metrics tab of the developer console. This rate limited error report only covers invocation rate limits that effectively prevent your app from running."

### 7.2 KVS and Custom Entity Store — https://developer.atlassian.com/platform/forge/limits-kvs-ce/ (Sep 28, 2026)
- "The limits listed below apply to the Key-Value Store or Custom Entity Store for each installation of your app." → "Request rate (RPS) | 1000"; "Read (10KB request per min) | 4000"; "Write (10KB request per min)* | 4000" (the asterisk has no footnote on the page).
- "Request sizes are rounded up to the nearest 10KB. Requests that are 10KB or smaller are counted as 1 request. Requests sized between 10KB and 20KB are counted as 2 requests."
- "Due to the 10KB rounding, Batch operations and Batch operations for custom entities are better than the equivalent individual operations at avoiding limits. For example, 10 individual writes of 1KB each will be counted as 100KB of limit use. However, if set in a single batch operation, it will count as 10KB of use."
- "If an installation of your app exceeds these limits due to bulk processing (for example, triggered by a bulk issue update in Jira), consider using the Async events API to queue your app's interactions with the KVS and Custom Entity Store."
- Value/keys: "Value size | 240 KiB", "Key length | 500", "Writes | 1 MB/s per key"; transactions: "Each transaction can contain a maximum of 25 operations."; "Transactions are treated as a single Write operation … The transaction will fail if it exceeds these limits, returning a TOO_MANY_REQUESTS error."; custom entities: "An app can have a maximum of 20 entities", "Each entity can have a maximum of 7 custom indexes and 50 attributes".
- Storage scope: "Internally, Forge automatically prepends an identifier to every key, mapping it to the right app and installation." (no cross-installation storage → no platform-native global counter for the Tier 1 pool).

### 7.3 Forge SQL — https://developer.atlassian.com/platform/forge/limits-sql/ (Dec 1, 2025)
- Per install: "Total stored data | 1 GiB (production installs) | 256 MiB (staging installs) | 128 MiB (development or custom environment installs)"; "Number of tables | 200"; "DML Requests per second (RPS) | 150"; "DDL Requests per minute (RPM) | 25"; "Total query execution time for all current invocations | 62.5 seconds (within each minute)".
- Per query: "Memory usage per query | 16 MiB"; "Request size | 1 MiB"; "Response size | 4 MiB"; "Per-connection timeout for SELECT queries | 5 seconds"; "Per-connection timeout for INSERT, UPDATE, and DELETE queries | 10 seconds".
- PARTNER (#166): SQL 429 body `{"message": "Limits for the current installation have been exceeded","name": "ForgeSQLError","responseDetails": {"status": 429,"statusText": "Too Many Requests","traceId": null},"code": "RATE_LIMIT_EXCEEDED","context": {}}` with no Retry-After.

### 7.4 Realtime — https://developer.atlassian.com/platform/forge/limits-realtime/ (Jun 25, 2026)
- "Operations per second | 50(3000 events per minute) | Maximum number of requests in one second for each installation. Once this limit is reached, requests after will fail with errors. Apps are required to handle retries. We recommend using a retry backoff strategy when re-attempting failed requests."
- Errors (https://developer.atlassian.com/platform/forge/realtime/error-handling-for-realtime-methods/): "RATE_LIMIT_EXCEEDED | The number of Realtime operations for this app installation has exceeded the allowed limit." `publish` returns `{ eventId: null, eventTimestamp: null, errors: [...] }` on error; `subscribe` rejects; "Operations that return a prevalidation error are not counted towards rate limiting."
- CHANGE-3246: "Starting June 26, 2026, a rate limit of 50 requests per app installation per second will be enforced for Forge Realtime." CHANGE-3326 (GA, 2026-06-29): "You can now issue tokens that grant only subscribe or publish access to a channel using the new permissions argument in signRealtimeToken."

### 7.5 Async events — https://developer.atlassian.com/platform/forge/limits-async-events/ (Feb 26, 2026) and API ref (Sep 1, 2026)
- "Event per request | 50", "Event per minute | 500", "Payload size | 200 KB", "Retry data size | 4 KB", "Cyclic invocation limit | 1000".
- Concurrency: "Concurrency counters are implicitly scoped to an app installation, but not to a specific queue, so a specific key can be used to control concurrency across multiple queues." / "If the concurrency field is not provided then there is no concurrency control. Event processing will be unbounded and limited only by the general per-installation invocation limits."
- Retry: "The maximum retryAfter value is 900 seconds (15 minutes). Any retryAfter values exceeding this limit are lowered to 900 seconds." / "FUNCTION_UPSTREAM_RATE_LIMITED | Rate limit upstream that caused the app to fail." / "It begins when the Async Event is successfully enqueued and lasts for 24 hours." / "Retention window can be extended by another 72 hours, to a total of 96 hours." / "Async Events are automatically retried within the retention window until they are successfully delivered. Retries use exponential backoff, with intervals reaching up to approximately 15 minutes between attempts." (CONTRADICTION: the storage queue guide says "with a maximum of four retries", https://developer.atlassian.com/platform/forge/storage-api-limit-handling/.) Delay: "The events processing can be delayed up to 15 minutes using the delayInSeconds setting."
- `@forge/events@3.0.7` code exposes `RateLimitError` ("Too many requests.") on push 429 and `InvocationErrorCode.FUNCTION_PLATFORM_RATE_LIMITED` / `FUNCTION_UPSTREAM_RATE_LIMITED`.

### 7.6 Scheduled triggers — https://developer.atlassian.com/platform/forge/manifest-reference/modules/scheduled-trigger/ (Jun 17, 2026) and limits page
- "Total number of scheduled trigger modules in an app | 5"; "Total number of scheduled trigger modules with fiveMinute intervals in an app | 1".
- "It then runs based on the configured interval fiveMinute, hour, day, or week."
- "Not all invocations for a single scheduled trigger will happen at once. To better improve overall performance, invocations will be distributed in batches evenly across the interval specified on any given module. Distribution is done by installations, so not all installations of an app will have their triggers invoked together."
- "There is a small chance of duplicated invocations, such scenarios should be handled in the apps code by the app developer."
- "If the function throws an error, nothing will happen, and the function invocation will not be retried. The function will be invoked the next time the schedule is due."
- CHANGE-3260: "By adding the appIsLicensed filter to your trigger modules in manifest.yml, the Forge platform will block trigger invocations for installations where the app's license is already inactive."

### 7.7 Web triggers — https://developer.atlassian.com/platform/forge/limits-web-trigger/ and runtime ref (Apr 17, 2026)
- "Get | 1,000 requests per minute per app, env, context"; "Delete | 500 requests per minute, web trigger ID"; "Create | 500 requests per minute per app, env, context".
- "The following response headers are included by default in web trigger responses sent back to the caller: - X-Ratelimit-Limit: The maximum number of requests allowed in the current window - X-Ratelimit-Remaining: The number of requests remaining in the current window - X-Ratelimit-Reset: The time (in seconds since epoch) when the rate limit window resets"

### 7.8 Forge LLMs — https://developer.atlassian.com/platform/forge/limits-llm/ (Sep 30, 2026)
- "The following limits apply for each installation of your app when using the Forge LLMs API:" → "Requests per minute | 100 | The number of prompts sent to any model in any given minute."; "Tokens per minute | 500,000 | The maximum number of tokens that a single model can process each minute."; "Inference time in minutes | 5 | The maximum time a model can process and generate responses before a timeout occurs, assuming the Async events API is used with a specified timeout equal or greater than 5 minutes. Otherwise the specified or default timeouts apply."; context windows "Haiku | 200K / 64K", "Sonnet | 1M / 128K", "Opus | 1M / 128K".
- CHANGE-3497 (2026-10-02): "The tokens per minute (TPM) rate limit for Forge LLMs has been increased from 50,000 to 500,000." / "The limit applies per installation of your app for each model."
- Models (https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-models/, Aug 3, 2026), all ACTIVE: claude-haiku-4-5-20251001, claude-sonnet-4-5-20250929, claude-sonnet-4-6, claude-sonnet-5, claude-opus-4-6, claude-opus-4-7, claude-opus-4-8, claude-opus-5. "Only text input/output is currently supported".
- Usage: "The Forge LLM API reports usage data per request (the number of input and output tokens consumed) in the API response." Response type `usage?: { input_tokens?, output_tokens?, total_tokens? }`, `finish_reason: string`.
- Adding LLM = major version: "Adding Forge LLMs—or a new model family—to an existing app triggers a major version upgrade requiring admin approval."
- Streams: "One way to detect incomplete responses, and therefore attempt a retry, is to check whether a completion choice object with a finish_reason property is missing when the stream ends" and "Exceptions are not thrown for finishing streams with incomplete responses." (https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-errors/)
- Validation: "temperature and top_p cannot be specified together"; models that reject them: claude-opus-4-7, claude-opus-4-8, claude-opus-5, claude-sonnet-5.
- `@forge/llm@1.0.7` throws `ForgeLlmAPIError` with `status`/`statusText` (no built-in retry).

### 7.9 Other
- Object Store (Preview): "Object Store requests per minute | 5000", "Pre-signed URL requests per second | 1000", "Maximum object size | 1 GB" (per installation).
- Resource bundles: "Bundle files | 5000", "Bundle size | 100" MB, "Bundle count | 50"; cumulative "Total files | 25000", "Total bundle size | 1 GB". App limits: "Modules per app | 150", "Resources per app | 10", "Alerts per app | 5".
- Suspension (https://developer.atlassian.com/platform/forge/exceeding-limits-and-suspended-apps/, Dec 1, 2025): "An app may be temporarily suspended if it negatively impacts the Forge platform, regardless of whether it’s in breach of any quotas or limits." Suspended app invocation error: "App is currently unavailable, please try again later".

---------------------------------------------------------------------------------------------------

## 8. Forge consumption pricing (billed, not hard-enforced) — https://developer.atlassian.com/platform/forge/forge-platform-pricing/ (Aug 12, 2026)

- "Forge uses a consumption-based pricing model, offering most capabilities for free within generous monthly usage limits. This page is the source of truth for current Forge pricing and will be updated as needed."
- Table (capability | unit | free monthly allowance | overage USD per unit):
  - "Forge Functions: Duration | $/GB-seconds | 200,000 GB-seconds | 0.000025"
  - "Key-Value Store: Reads | $/GB | 0.1 GB | 0.055"
  - "Key-Value Store: Writes | $/GB | 0.1 GB | 1.090"
  - "Logs: Writes | $/GB | 1 GB | 1.005"
  - "SQL: Compute duration | $/hr | 1 hr | 0.143"
  - "SQL: Compute requests | $/1M-requests | 100,000 requests | 1.929"
  - "SQL: Data stored | $/GB-hours | 730 GB-hours | 0.00076850"
  - "Object Store: Requests | $/1k-requests | 5,000 requests | 0.001353"
  - "LLM: Input | $/credits | 0 credits | Credit pricing varies by model. See Forge LLMs pricing."
  - "Containers: Compute | $/vCPU-hour | 0 vCPU-hours | 0.07177"; "Containers: Memory | $/GiB-hour | 0 GiB-hours | 0.00786"
- "Empty KVS reads count as 1KB towards your usage, whereas non-empty reads are based on actual size."
- "The free usage allowance is per app, per month. Each app receives its own quota for each billable capability. Usage above these thresholds will be billed monthly in arrears."
- "Usage will be measured across all environments which include production, staging, development and all custom development environments."
- "Effective April 4, 2026, Forge usage on up to the first five sandboxes associated with each production site where your app is installed is now exempt from billing." (also CHANGE-3135)
- Crash/OOM billing: "When this happens, you are charged for the lower of: - the function's configured timeout (set via timeoutSeconds), or - the measured execution time, including platform overhead."
- "Capabilities not listed above — including UI modules, Jira expressions, and Forge Remote — are free and do not contribute to your bill." (optimise guide)
- "Changes to pricing and or the introduction of new capabilities will be announced at least 3 months in advance."
- LLM pricing (https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-pricing/, Jul 23, 2026): "No free usage allowance: The Forge LLMs API does not include a free monthly usage quota. All token usage is billed." / "Forge LLMs usage is charged to the developer of the Forge app and counted toward your Forge monthly bill." / "Opus 4.6 | 50 credits | $5 | $25", "Sonnet 4.5 | 30 credits | $3 | $15", "Haiku 4.5 | 10 credits | $1 | $5" (credits per 1M tokens; effective $ per 1M input / output) / "Input credits: $0.10 per credit" / "Output credits: $0.50 per credit". (Newer models in the models table have no published credit rate on the pricing page.)
- Rovo: "Rovo credits (the cost of the AI interaction itself) are paid by the customer's organization from their pooled Rovo allowance." vs "Forge consumption-based pricing (compute, storage, and logs used to execute your action) is paid by the developer of the Forge app".
- Timeline (changelog): CHANGE-2995 (2026-01-01) "Forge platform pricing is now in effect for Forge apps."; CHANGE-2992 (2025-12-23) async compute charging delayed; CHANGE-3120 (2026-03-26) "A Forge app invocation is classified as asynchronous when invoked from the following Forge modules: Scheduled triggers, Async Events API, Trigger" with billing from 2026-07-01; CHANGE-3328 (2026-07-02) "usage of asynchronous Forge functions is now billed in line with Forge platform pricing" (invoices from 2026-08-01); CHANGE-3309 (2026-06-29) "we are doubling the free usage allowance for Forge functions from 100,000 GB-seconds per month to 200,000 GB-seconds per month."; CHANGE-3261 (2026-06-01) "All Forge LLM usage is now billable."
- Alerts: CHANGE-2982 "Developers will automatically receive notifications when the usage of any billable capability in the Forge app reaches key thresholds— at 50%, 75%, 90%, and 100% of free monthly usage allowance." (https://developer.atlassian.com/platform/forge/usage-alerts/: "100% | Your app has reached its full resource allocation and may be subject to limits or other enforcement depending on your plan and configuration.")
- Doc defect: optimise guide says "Input credits and output credits are charged at different rates ($0.0000001/credit for input, $0.0000005/credit for output)" — contradicts the pricing page's $0.10 / $0.50 per credit (pricing page declares itself "the source of truth").
- Stale doc: monitor-usage-metrics (Jun 30, 2025) still says "Currently, for all Lambda functions, the memory allocation by default is 0.5 GB" and "Compute in GB-sec = (Lambda Execution Time in seconds * 0.5) GB seconds" although memory is now configurable up to 1,024MB (`memoryMB`).

---------------------------------------------------------------------------------------------------

## 9. Observing consumption at runtime; background/scheduled sync and the shared pool

Runtime signals an app can read itself:
1. Backend `@forge/api` `requestJira/requestConfluence` returns a WHATWG `Response` (`api.[asApp | asUser]().requestJira(path[, options]) => Promise<Response>`), so headers `RateLimit`, `RateLimit-Policy` (or `Beta-` variants), `X-RateLimit-*`, `RateLimit-Reason`, `Retry-After` are readable. `@forge/api@8.2.0` has NO built-in 429 retry (code reviewed: `__fetchProduct` returns the response; only `forge-proxy-error` is turned into `ProxyRequestError`).
2. `RateLimit` `r` is present only after ~80% usage; `t` = seconds to reset; `X-RateLimit-NearLimit: true` at <20% remaining. Below 80% the app sees no remaining-count → must self-account.
3. Frontend: `invoke(key, payload, { rateLimitProperties: true })` returns `{ body, metadata }` with "rateLimitValue … rateLimitRemaining … rateLimitReset" per docs (https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/invoke/, Jun 19, 2026) — but `@forge/bridge@7.1.0` typings name the field `rateLimitLimit` (doc/typings mismatch). CHANGE-3314: "Currently, rateLimitProperties is the only supported field in the metadata."
4. Web trigger responses: `X-Ratelimit-Limit/Remaining/Reset` (epoch seconds).
5. LLM responses carry `usage` tokens.
6. Developer console (not runtime): "Usage and charges" per resource; "Note that the daily data refresh occurs at 12:00 UTC."; "Usage metrics are available for the last 60 days." (https://developer.atlassian.com/platform/forge/monitor-usage-metrics/); API metrics (status codes, latency); invocation "Rate limited" label. PARTNER (#1 of 97896) frontend requests are not counted in API metrics; staff: "this is not intended. I have created a ticket for tracking purposes - https://jira.atlassian.com/browse/ECO-1208."
7. Export APIs: metrics export supports only "FORGE_API_REQUEST_COUNT", "FORGE_API_REQUEST_LATENCY", "FORGE_BACKEND_INVOCATION_COUNT", "FORGE_BACKEND_INVOCATION_ERRORS", "FORGE_BACKEND_INVOCATION_LATENCY" (https://developer.atlassian.com/platform/forge/export-app-metrics/) — no points metric. App resource usage GraphQL API: "Rate limits of 5 requests/minute" (https://developer.atlassian.com/platform/forge/developer-space/export-app-resource-usage/). Staff (#2 of 98197): "there is an Export app metrics API that can be used which supports the export of API and Invocation metrics today". Staff (#84): "We will also make it easier to view point usage in the Developer Console. We plan to provide clearer insights into how many points your app is consuming, along with notifications as you approach key thresholds." — no shipped points dashboard found in the changelog by 2026-10-09.
8. Runs on Atlassian constraint: "Your app must not egress data, with the exception of egress for analytics purposes." (https://developer.atlassian.com/platform/forge/runs-on-atlassian/) → no external global rate-limit coordinator for RoA apps (PARTNER #4 of 98197 makes this point).

Background/scheduled sync vs the shared pool:
- Docs: "Distribute requests over time: Spread your requests evenly throughout the hour rather than sending large spikes at predictable times. Add random jitter to scheduled jobs to avoid thundering herd effects when many apps hit the API simultaneously." and the Confluence 00:00 UTC example (§5).
- Forge spreads scheduled triggers across installations (§7.6) but every installation's backend calls still draw from ONE Tier 1 pool.
- Staff: "Apps with higher per-request point costs can be more sensitive to concentrated usage from a single tenant." and #129: "That said, you’re right that even a capped number of successful invocations can still generate REST traffic. The points‑based model is designed with safeguards to tolerate such short‑lived spikes within an hour and to mitigate noisy‑neighbor effects for Tier 1 apps."
- Staff (Alan Braun #64): "In cases where usage from a single customer could impact other customers of the same app, we’ll proactively partner with those developers on per‑tenant quotas or other mitigations".
- Event loop guard: "Use the ignoreSelf filter property in your manifest to tell Forge to suppress events generated by your app's own actions" (optimise guide).

---------------------------------------------------------------------------------------------------

## 10. Adjacent security/robustness facts that interact with dosing

- Module-level state is shared across tenants (https://developer.atlassian.com/platform/forge/tenant-data-isolation/, Aug 1, 2026): "Forge apps run in a multi-tenant environment where the same runtime process can serve multiple Atlassian customers (tenants). This means module-level variables and in-memory caches are shared across tenant invocations unless you explicitly scope data to a single invocation." / "If you need in-process caching for performance, always key the cache by a tenant-specific identifier, such as cloudId." / "delegating persistence to Forge Storage, which is tenant-scoped by design." → caching permission checks or API results to save points MUST be tenant-partitioned; a module-level points counter is per warm process, not global, and may only hold non-tenant data.
- Bridge `requestJira` = user permissions only (no asApp) → admin operations needing app permissions go through resolvers (counted in points).
- `authorize()` (permission checks) calls POST permissions/check as the user (backend → counted).
- Staff-noted pattern risk (HeyJoe #127): product-triggered fan-out of invocations (Confluence `adfExport` per macro per page in CQL search) — "a function invocation being triggered from a CQL search is pretty undesirable behaviour for multiple reasons, not the least of which is the denial of service issue that you’ve raised."
- Adjacent (not tiers, useful for "boot speed"): CHANGE-3505 (2026-10-06): "Static macros allow you to render Forge macro content natively on Confluence pages without iframe overhead." and resource-limits guidance "Consider using multiple entry points instead of multiple resources." with "Smaller deploy size and faster load times — shared dependencies across entry points can be extracted into common chunks".

---------------------------------------------------------------------------------------------------

## 11. How an offline benchmark can test each concept objectively (and what the contract must STATE)

Ground rule: we never grade an unstated requirement. Atlassian leaves several knobs unpublished or contradictory (burst bucket capacity, forgiveness, frontend counting, per-endpoint object costs, header exposure in the browser), so the contract must publish the emulator's exact rules. Emulator = a virtual-clock gateway in front of mock Jira/Confluence that writes one log row per product call: (virtual time, cloudId/tenant, installationId, principal = asApp | asUser | bridge-user | remote, method, path, status, points charged, policy, reason, headers sent).

| Concept | What the app must do | What the grader observes (objective) | Must be STATED in the contract |
|---|---|---|---|
| T1 Tier 1 global pool | Budget all tenants against one 65,000 pts/h pool; reserve headroom for interactive work; never let one tenant/user exhaust it | Per-hour points ledger across ≥2 emulated tenants; count of `*-quota-global-based` 429s (target 0 in the nominal scenario); interactive success rate for the "small" tenant while the "large" tenant syncs; scenario completion within N virtual hours | 65,000/h per app shared by all tenants; window = top of the virtual UTC hour; no carry-over; hard wall (no forgiveness) — a deliberate, stated simplification; how the virtual clock is exposed |
| T2 Points cost model | Estimate cost before calls; prefer field-filtered, paginated, bulk calls; avoid identity-heavy reads | Total points for a fixed functional scenario vs a reference budget; ratio of bulk to N+1 calls; `fields=` present on search/bulkfetch | Cost table per emulated endpoint (base 1; +1 per core object; +2 per identity object incl. permission assignments; writes 1; unlisted 1); what counts as an "object" per endpoint (e.g. each issue returned by search/jql; each member in a group page) |
| T3 Quota 429 handling | Pause ALL product calls until reset; queue/defer work; show degraded UI; resume after reset | After an injected quota 429: no product call to any endpoint before reset (+ tolerance); deferred jobs complete after reset; UI state from a resolver reports "quota-paused" | Header set on quota 429 (`RateLimit`, `Retry-After`, `RateLimit-Reason` values incl. Confluence values); that an unknown reason string (e.g. `jira-cost-based`) may appear and must be treated as quota-class |
| T4 Burst buckets | Bound concurrency (doc: 5–10 in flight); pace per endpoint at steady-state; on `jira-burst-based` slow only that endpoint | Max in-flight requests; per-endpoint request rate; after a burst 429, the same endpoint is retried no sooner than Retry-After while other endpoints continue | Bucket model: per tenant × endpoint × method; steady-state RPS (GET/POST 100, PUT/DELETE 50, listed customs) AND the bucket capacity (unpublished by Atlassian — the contract must choose one); that the bucket is shared with simulated "other apps" traffic (CHANGE-2753) |
| T5 Per-issue writes | Coalesce field updates into one PUT; throttle writes to the same issue; use bulk edit for many issues | Writes per issue per 2 s / 30 s windows; behaviour after injected `jira-per-issue-on-write` | 20 writes / 2 s, 100 writes / 30 s; reason string |
| T6 Retry discipline | Retry only idempotent requests and only when a delay signal exists; honour Retry-After as a minimum; exponential backoff + jitter; cap attempts | For each 429/503 with Retry-After: delta-t to retry ≥ Retry-After; attempts ≤ cap; no automatic retry of non-idempotent POST (or use of an idempotency guard such as a pre-check); jitter present (variance across runs/tenants) | Which requests count as idempotent in the emulator; retry cap expected (doc example: 4); the 503 + Retry-After case; the gateway 429 WITHOUT Retry-After (use X-RateLimit-Reset) |
| T7 Header parsing | Parse comma-separated policy lists in any order, optional `r`, Beta- vs non-Beta, ISO times with/without seconds | Fuzzed header permutations; app's computed "remaining/near-limit" state (exposed via an admin resolver) matches emulator truth | Exact header grammar, that `r` appears only at ≥80% used, `NearLimit` semantics, both ISO forms |
| T8 Frontend vs backend accounting + security | Use bridge `requestJira` for user-scoped reads (fast boot, exempt from points per stated rule); route privileged/admin/storage work through resolvers; never expose secrets/app-level data to the browser | Principal of each call in the log; points charged only to backend calls; permission enforcement tests (a non-admin user cannot reach admin resolvers / asApp data) | Whether bridge calls are charged (recommend: exempt, per staff 2026-04-01, but subject to burst) and whether rate-limit headers are visible to the browser (recommend: hidden, per FRGE-1923 / staff 2026-02-02) |
| T9 Forge platform limits | Stay under invocation (1,200/user/min; 7,000/min & 300/s per install), bridge client limiter (500 invokes/25 s/page), KVS (1000 RPS; 4000 × 10 KB/min), SQL (150 DML RPS), Realtime (50 ops/s), async (500 events/min, 50/push), LLM (100 RPM; 500k TPM/model) | Platform 429/TOO_MANY_REQUESTS counts; KVS batch/transaction usage; async queue with concurrency key; UI boot = number of resolver invokes before first meaningful paint | Which limits are emulated, numbers, and the exact error shapes (KVS TOO_MANY_REQUESTS; SQL `RATE_LIMIT_EXCEEDED` with no Retry-After; Realtime `RATE_LIMIT_EXCEEDED` in `errors[]`; bridge `BridgeAPIError` message; invoke 429 with `rateLimitProperties`) |
| T10 Background sync dosing | Spread sync over the hour with jitter; per-tenant fair share; chunk via async events (retryAfter ≤ 900 s, re-enqueue beyond); idempotent against duplicate scheduled invocations; no retry storms | Points-per-minute histogram (no spike at :00); each tenant's share; duplicate trigger injected → no double writes; job resumes after an injected quota wall | Scheduled-trigger semantics (distributed across installations, possible duplicates, no retry on throw); async retry semantics; the fair-share policy the grader uses (e.g. no tenant > X% of the pool in any hour while another tenant has pending interactive work) |
| T11 LLM usage | Pick model tier per task; cap `max_completion_tokens`; cache by content hash (tenant-scoped); account tokens from `usage`; detect missing `finish_reason`; long jobs via async + Realtime (UI limit 25 s) | Credits consumed for the scenario (from emulator token counts × stated credit rates); cache hit rate on unchanged inputs; 429 handling; incomplete-stream recovery | Model list + credit rates (Haiku 4.5 10, Sonnet 4.5 30, Opus 4.6 50 credits/1M tokens; $0.10 in / $0.50 out per credit), RPM/TPM, the usage field, a stated monthly credit budget and kill switch behaviour |
| T12 Consumption dashboard (admin, UI Kit) | Show points used this hour (self-estimated + header-reported), NearLimit/paused state, 429s by reason, LLM credits, KVS bytes, projected Forge bill | Admin resolver's JSON vs emulator ground truth within a stated tolerance | The resolver schema, the tolerance, and the pricing formula (pricing-page numbers) used for the "bill" |

Scoring fairness notes:
- Use a virtual clock so an "hour" costs seconds; jitter must be allowed (grade "≥ minimum delay", not exact delays).
- Use deterministic seeds for injected faults; publish the fault catalogue (429 kinds, 503, missing Retry-After) — not their timings.
- Grade outcomes (points, 429s, fairness, correctness of data written) rather than code shape.
- Do not reproduce Atlassian bugs (bogus `t`, double Retry-After) unless the contract lists them as inputs the app must survive.

---------------------------------------------------------------------------------------------------

## 12. Discrepancies, open questions, and what I could not verify

Could not verify / unpublished:
1. Whether the phased points enforcement has reached 100% of apps by 2026-10-09 (no completion changelog; a partner still saw Beta headers on 2026-07-23).
2. The real "forgiveness" behaviour for occasional overage (staff says tolerated; partner found a hard wall; thresholds deliberately unpublished).
3. Burst bucket CAPACITY per endpoint (only steady-state RPS is published); Confluence burst numbers are not published at all.
4. A per-endpoint object-cost catalogue (Atlassian: "We plan to expand our catalog"); how POST read-like endpoints (search/jql, permissions/check) are charged — partner measurements say per object for search/jql.
5. Whether frontend `requestJira` is exempt *today* in every product (staff said exempt from March 2 and confirmed again 2026-04-01; "may include … in the future, with clear advance notice"; public docs silent).
6. Whether rate-limit headers are now readable by bridge `requestJira` in the browser (staff said "working with our internal teams" on 2026-02-03).
7. The partner-only "Quick Reference Guide: New points-based API rate limiting and tiered quotas for Jira & Confluence Cloud" (atlassianpartners.atlassian.net page 1180303372) — requires Atlassian login; not read.
8. The exact Tier 2 eligibility criteria and review SLA (not published; staff declined to publish thresholds).
9. Whether a developer-console points view now exists (staff promised; nothing found in the changelog or docs).
10. Connect-on-Forge quota independence (PARTNER #161 observed separate quotas for Connect and Forge back ends; unconfirmed by staff).
11. Whether the invocation-limit RFC-130 per-second sliding window shipped exactly as proposed (CHANGE-3420 confirms 300 RPS / 7,000 RPM per installation and removal of the 30k RPM environment limit; the docs still say "fixed one-minute window").
12. `rateLimitProperties` field name: docs say `rateLimitValue`, `@forge/bridge@7.1.0` typings say `rateLimitLimit`.

Contradictions found in primary sources (state the chosen interpretation in the contract):
- Jira retry prose (multiply delay by 0.7–1.3) vs pseudocode (adds delay×0.7–1.3, so 1.7–2.3×) vs "use Retry-After as the minimum delay".
- Jira "design … around the steady-state request rates (e.g., 10 requests/second for GET endpoints)" vs the default table "GET | 100".
- Jira `X-RateLimit-Reset` "Only returned with 429 responses" vs Confluence (no restriction) vs partner logs (present on near-limit responses as `X-Beta-Ratelimit-Reset`).
- `r` "Optionally included" (Jira) vs "present only when usage exceeds ~80%" (Confluence).
- Staff 2025-12-22 "including calls originating from the Forge UI" vs staff 2026-01-31/2026-04-01 frontend exempt.
- Async events: "retried … until they are successfully delivered" vs storage guide "with a maximum of four retries".
- LLM credit price: pricing page $0.10/$0.50 per credit vs optimise guide "$0.0000001/credit for input, $0.0000005/credit for output".
- Documented `RateLimit-Reason` constants vs production values for Confluence (partner #157; staff "We will fix the documentation").
- Jira burst custom-limit table lists Confluence-style paths (`/api/content/{id}/state`).

---------------------------------------------------------------------------------------------------

## 13. Source index (all fetched 2026-10-09; "last updated" as shown on the page)

- https://developer.atlassian.com/cloud/jira/platform/rate-limiting/ — Oct 9, 2026
- https://developer.atlassian.com/cloud/confluence/rate-limiting/ — Oct 9, 2026
- https://developer.atlassian.com/changelog/#CHANGE-2958 (2025-12-12), #CHANGE-3003 (2026-01-12), #CHANGE-3045 (2026-02-02), #CHANGE-3080 (2026-03-02), #CHANGE-2753 (2025-08-28), #CHANGE-2351 (2025-02-18), #CHANGE-2410 (2025-03-13), #CHANGE-2578 (2025-05-23), #CHANGE-3420 (2026-09-01), #CHANGE-3314 (2026-08-13), #CHANGE-2929 (2025-11-20), #CHANGE-3006 (2026-01-13), #CHANGE-3246 (2026-05-26), #CHANGE-3326 (2026-06-29), #CHANGE-3497 (2026-10-02), #CHANGE-2995, #CHANGE-2992, #CHANGE-3120, #CHANGE-3328, #CHANGE-3309, #CHANGE-3261, #CHANGE-2982, #CHANGE-3135, #CHANGE-3229, #CHANGE-3260, #CHANGE-2639, #CHANGE-2551, #CHANGE-2797, #CHANGE-2719, #CHANGE-882, #CHANGE-3505 — via https://dac-changelogs.services.atlassian.com/changes?apiGroups=universal&match=CHANGE-NNNN
- https://www.atlassian.com/blog/development/evolving-api-rate-limits — published 2025-12-12, modified 2026-04-20 (Alan Braun)
- https://community.developer.atlassian.com/t/2026-point-based-rate-limits/97828 (174 posts; staff posts #30,31,57,58,61,64,68,70,79,80,81,84,96,97,98,103,122,127,129,133,135,137,138,142,144,145,146,148,159,160,168)
- https://community.developer.atlassian.com/t/how-best-to-monitor-rate-limiting-from-within-a-runs-on-atlassian-roa-app/98197
- https://community.developer.atlassian.com/t/rate-limit-abuse-new-attack-vector/99654
- https://community.developer.atlassian.com/t/rfc-130-upcoming-changes-to-invocation-rate-limits/100191
- https://community.developer.atlassian.com/t/action-required-update-your-apps-to-comply-with-jira-cloud-burst-api-rate-limits/97202
- https://community.developer.atlassian.com/t/forge-monitor-api-metrics-dont-consider-frontend-requests/97896
- https://community.developer.atlassian.com/t/update-to-forge-consumption-on-sandbox/100069
- https://developer.status.atlassian.com/incidents/bb38p4wdd7mz (2026-04-22..23)
- https://developer.atlassian.com/platform/forge/platform-quotas-and-limits/ — Jul 13, 2026
- https://developer.atlassian.com/platform/forge/limits-invocation/ — Sep 1, 2026
- https://developer.atlassian.com/platform/forge/limits-kvs-ce/ — Sep 28, 2026
- https://developer.atlassian.com/platform/forge/limits-sql/ — Dec 1, 2025
- https://developer.atlassian.com/platform/forge/limits-llm/ — Sep 30, 2026
- https://developer.atlassian.com/platform/forge/limits-realtime/ — Jun 25, 2026
- https://developer.atlassian.com/platform/forge/limits-async-events/ — Feb 26, 2026
- https://developer.atlassian.com/platform/forge/limits-scheduled-trigger/ — Dec 1, 2025
- https://developer.atlassian.com/platform/forge/limits-web-trigger/ — Dec 1, 2025
- https://developer.atlassian.com/platform/forge/limits-resource/ — Jul 7, 2026
- https://developer.atlassian.com/platform/forge/limits-app-developer/ — Dec 1, 2025
- https://developer.atlassian.com/platform/forge/limits-object-store/ — Sep 28, 2026
- https://developer.atlassian.com/platform/forge/limits-containers/ — Jul 13, 2026
- https://developer.atlassian.com/platform/forge/exceeding-limits-and-suspended-apps/ — Dec 1, 2025
- https://developer.atlassian.com/platform/forge/forge-platform-pricing/ — Aug 12, 2026
- https://developer.atlassian.com/platform/forge/optimise-forge-costs/ — Aug 21, 2026
- https://developer.atlassian.com/platform/forge/monitor-usage-metrics/ — Jun 30, 2025
- https://developer.atlassian.com/platform/forge/monitor-api-metrics/ — Oct 30, 2024
- https://developer.atlassian.com/platform/forge/monitor-invocation-metrics/ — Jul 13, 2026
- https://developer.atlassian.com/platform/forge/usage-alerts/ — May 8, 2026
- https://developer.atlassian.com/platform/forge/export-app-metrics/ — Jul 13, 2026
- https://developer.atlassian.com/platform/forge/developer-space/export-app-resource-usage/ — Aug 6, 2026
- https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api/ — Jul 23, 2026
- https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-reference/ — Aug 3, 2026
- https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-pricing/ — Jul 23, 2026
- https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-models/ — Aug 3, 2026
- https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-api-errors/ — Jul 23, 2026
- https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/invoke/ — Jun 19, 2026
- https://developer.atlassian.com/platform/forge/apis-reference/ui-api-bridge/requestJira/ — May 27, 2026
- https://developer.atlassian.com/platform/forge/apis-reference/fetch-api-product.requestjira/ — Nov 8, 2024
- https://developer.atlassian.com/platform/forge/runtime-reference/web-trigger/ — Apr 17, 2026
- https://developer.atlassian.com/platform/forge/realtime/error-handling-for-realtime-methods/ — Jun 25, 2026
- https://developer.atlassian.com/platform/forge/apis-reference/license-api/ — Jul 14, 2026
- https://developer.atlassian.com/platform/forge/runtime-reference/async-events-api/ — Sep 1, 2026
- https://developer.atlassian.com/platform/forge/storage-api-limit-handling/ — Aug 21, 2026
- https://developer.atlassian.com/platform/forge/manifest-reference/modules/scheduled-trigger/ — Jun 17, 2026
- https://developer.atlassian.com/platform/forge/tenant-data-isolation/ — Aug 1, 2026
- https://developer.atlassian.com/platform/forge/runs-on-atlassian/ — Aug 24, 2026
- https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-bulk-operations/ ; https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/ ; https://developer.atlassian.com/cloud/jira/platform/swagger-v3.v3.json ; https://developer.atlassian.com/cloud/jira/platform/bulk-operation-additional-examples-and-faqs/ (Oct 9, 2026)
- https://developer.atlassian.com/cloud/confluence/openapi-v2.v3.json
- npm (packed 2026-10-09): @forge/api 8.2.0, @forge/bridge 7.1.0, @forge/kvs 2.0.7, @forge/sql 4.0.7, @forge/llm 1.0.7, @forge/events 3.0.7, @forge/realtime 1.0.1, @forge/auth 1.0.0 — in `pkgs/tiers-agent/`.

---------------------------------------------------------------------------------------------------

## Verification

An independent fact-check was run on 2026-10-09. I fetched every cited URL myself with curl rather than relying on the researcher's downloads. Discourse posts came from `/raw/{topic}/{post}`, with author, title and date from `/posts/by_number/{topic}/{post}.json`; every post of threads 97828, 97202 and 99654 was scanned. Changelog entries came from `https://dac-changelogs.services.atlassian.com/changes?...&match=CHANGE-NNNN`, with a keyword search over all entries since 2025-08. The @forge/bridge 7.1.0 and 7.1.1-next.7 tarballs came from registry.npmjs.org. Public bug status came from `ecosystem.atlassian.net/rest/api/2/issue/FRGE-1923`. Page dates are the "Last updated" stamp served today. Working copies are in `scratchpad/forge2/verify-tiers/dl/`.

**Tally: 47 confirmed, 1 outdated, 0 refuted, 0 unverifiable.** Every design-critical quote appears verbatim at its URL. The one exception is the bridge "500req/25s" string: it is in the package source, not on the npmjs.com page that was cited. Several confirmations carry a correction that changes wording or grading. Those are listed first.

### V.1 The outdated claim, which flips a design recommendation

- **Claim 46** says the backend resolver is the supported way to read rate-limit headers, that bridge `requestJira` headers are not exposed, and that staff were "working on it" in Feb 2026. **Verdict: OUTDATED.**
  - The quote is real: burst thread #13, Suyash Kumar Tiwari, Atlassian Staff, 2026-02-02.
  - He walked it back the next day in #22 (2026-02-03): "You're right that the earlier guidance to rely on backend resolvers adds unnecessary complexity for this use case."
  - The bug **FRGE-1923** ("Forge bridge doesn't forward rate limit headers", https://ecosystem.atlassian.net/browse/FRGE-1923) was **closed Fixed on 2026-04-09**. The closing comment reads: "The changes for both Jira & Confluence are now live in production." The reporter answered the same day: "I just tried it out and I can confirm it works now!"
  - This invalidates three items above: §4.4 line 195, §4.4 line 197 ("NOT VERIFIED", now resolved), and table row T8's "(recommend: hidden, per FRGE-1923 / staff 2026-02-02)".
  - Fix: make the emulator EXPOSE rate-limit headers to bridge `requestJira`. Hiding them is legitimate only if the contract labels it a deliberate departure from production.

### V.2 Corrections and caveats that change contract wording or grading

1. **Point costs (claims 5 and 6).**
   - Docs: each request costs a base of 1 point, plus a per-object cost taken from the table. That cost is 1 for core and other objects and 2 for identity and access objects, so "1 per object" is wrong.
   - Field data disagrees with the table. #151 (2026-02-13) measured `search/jql` and `project/search` at "always 1 + the number of returned results". In the same post, project versions (about 500 results), `permissions/project` (about 1000) and `user/search` (about 10) each cost 1 point.
   - Action: the contract must publish its OWN per-endpoint cost table and must not claim that table is what production charges.
2. **"5–10 concurrent" (claim 22).** This is advice inside the `Promise.all` section of a cost-optimisation page: "Avoid parallelising large numbers of requests at once — send at most 5–10 concurrent requests and batch the rest." It is not a cap the platform enforces. Make the maximum a contract rule. Note that an undocumented, enforced concurrency reason was seen in the field: `jira-max-concurrent-threads-across-all-instances-per-tenant-per-user` (#167, 2026-03-31).
3. **X-RateLimit-NearLimit (claim 16).** It is still listed in the Jira "Current headers (enforcement active)" table, but two changelog entries undercut it:
   - CHANGE-3045: "For points-based quota enforcement, only RateLimit and RateLimit-Policy are used, the existing X-Beta-RateLimit-* and X-RateLimit-* headers will not be used."
   - CHANGE-3080: "We plan to discontinue sending quota rate limit values via the X-RateLimit-* headers in the future."
   - Action: grade on `RateLimit` `r`/`t` and treat NearLimit as optional legacy.
4. **The burst section contradicts itself (claim 10).** The table gives GET 100 rps. The same page also says "Design your app around the steady-state request rates (e.g., 10 requests/second for GET endpoints)", and its worked example uses a 100-token bucket that refills at 10 per second. Publish both the refill rate and the bucket capacity in the contract.
5. **Retry recipe (claim 21).** Three problems in the docs:
   - The jitter prose and the pseudocode disagree.
   - The pseudocode retries a 429 that has NO Retry-After (`else if (statusCode == 429)`), which contradicts "Only retry if the API is idempotent and the response includes a Retry-After header".
   - The pseudocode never updates `lastRetryDelayMillis`.
   - Action: grade only three things: delay ≥ Retry-After or reset, attempts ≤ the cap, and no blind retry of non-idempotent writes.
6. **Gateway 429 (claim 15).** Incident bb38p4wdd7mz (posted 2026-04-22 06:57 UTC, resolved 2026-04-23 16:14 UTC) listed these headers: `X-Failure-Category: FAILURE CLIENT RATE LIMITED`, `X-Ratelimit-Limit: 360000` and `X-Ratelimit-Remaining: 0`. It listed no RateLimit-Reason. X-RateLimit-Reset was the only timing signal, not the only header. Emulate that header set.
7. **Async-event retries (claim 38).** Two Atlassian pages disagree:
   - async-events-api (Sep 1, 2026): events are "retried within the retention window until they are successfully delivered". The window is 24 h, extendable to 96 h.
   - storage-api-limit-handling (Aug 21, 2026): "with a maximum of four retries".
   - With `retryAfter` capped at 900 s, four retries cover only one hour.
   - Action: for a quota wall of about an hour, the safe pattern is to re-enqueue a fresh event. The contract must state which retry rule the emulator applies.
8. **LLM pricing (claim 41).**
   - The three rates match the pricing page (Jul 23, 2026).
   - The models page (Aug 3, 2026) lists eight ACTIVE models. Five of them have no published credit rate: claude-sonnet-4-6, claude-sonnet-5, claude-opus-4-7, claude-opus-4-8 and claude-opus-5.
   - claude-sonnet-4-5's tentative retirement date ("Not sooner than September 29, 2026") has passed. haiku-4-5's is "Not sooner than October 15, 2026".
   - Action: price by tier (Haiku 10, Sonnet 30, Opus 50 credits per 1M tokens) and state that this is an assumption for the newer models.
9. **Bridge limit of 500 per 25 s (claim 32).**
   - The string is in the SOURCE: `out/invoke/invoke.js` line 25 of 7.1.0 (published 2026-09-28). It is unchanged in 7.1.1-next.7 (2026-10-09).
   - It is NOT on the npmjs.com README that the claim cites.
   - It is a fixed-window counter held in module scope, so each loaded bridge (each iframe) has its own. It throws `BridgeAPIError` before `callBridge`.
10. **Bridge requestJira (claim 47).**
    - Resolver invocations cost no points (#129). Only the product REST calls inside them are charged.
    - Object Store calls from the browser (`@forge/bridge` object-store helpers) still go through a backend `functionKey`, which mints presigned URLs.
11. **Frontend points exemption (claims 24 and 25).**
    - This is staff-only. It is not on the Jira or Confluence rate-limit pages (both updated Oct 9, 2026) or on the requestJira page (May 27, 2026).
    - The field data conflicts:
      - #154 (2026-02-27) measured 50 UI-only POST `search/jql` calls at 571 points.
      - #163 (2026-03-18) could not reproduce that for Forge, but found Connect `AP.request` was charged.
      - #168 (staff, 2026-04-01): "We confirmed that Forge front-end traffic is exempted."
    - Action: keep the exemption as a stated contract assumption.
12. **Enforcement is phased (claims 1, 7, 8 and 19).**
    - CHANGE-3080 started a phased rollout on 2026-03-02.
    - On 2026-07-23 (#177) a partner still saw only Beta- headers.
    - #169 (2026-04-09) showed `global-app-quota` with r=0 alongside `Retry-After: 89` and a reset 1.5 minutes away.
    - Action: the emulator's deterministic hard wall at the hourly reset is a STATED choice, not a universal production fact.
13. **Storage scoping (claim 35).** The quote proves that keys are scoped per installation. "No platform-native cross-tenant counter" is an inference that holds for KVS and Custom Entities as documented. The only cross-tenant signal is the `global-app-quota` RateLimit header, and its `r` appears only past about 80% usage.

### V.3 Per-claim verdicts

Abbreviations: JRL = https://developer.atlassian.com/cloud/jira/platform/rate-limiting/ (Oct 9, 2026). CRL = https://developer.atlassian.com/cloud/confluence/rate-limiting/ (Oct 9, 2026). T97828 = community thread 2026-point-based-rate-limits/97828. T97202 = action-required burst thread 97202.

| # | Claim (short) | Verdict | Evidence I saw |
|---|---|---|---|
| 1 | Tier 1 is the default; one 65,000 pts/h quota per app, shared across all tenants | confirmed | JRL: "Your app shares a single 65,000 point hourly quota across all tenants. This is the default tier for all apps. Most apps operate comfortably within the Global Pool." CRL is identical. Enforcement is phased (V.2-12). |
| 2 | Staff: 65k is per app | confirmed | T97828 #148, MaheshPopudesi, "Atlassian Staff", 2026-02-11, verbatim. The JRL FAQ now reads "single shared hourly quota across all tenants (65,000 pts/hr)". |
| 3 | Tier 2 by review only; Free 65k; Std 100k+10u; Prem 130k+20u; Ent 150k+30u; cap 500k | confirmed | The JRL tier table and "Per-tenant rate limits are capped at 500,000 points per hour for Standard, Premium, and Enterprise editions." FAQ: "No, not on demand. Quota increases require a review by Atlassian". |
| 4 | Hourly, resets at the top of each UTC hour, no carry-over | confirmed | JRL: "reset at the top of each UTC hour". CRL: "There is no carry-over between hours; unused quota does not accumulate." |
| 5 | Base 1 + per-object cost; writes cost the base only | confirmed (reword) | JRL verbatim. The per-object cost comes from the table (1 or 2). See V.2-1 for the field deviations. |
| 6 | Identity/access objects cost 2; 8-user group page = 17 | confirmed (caveat) | JRL table row and "1 (base) + 8 users = 17 points (1 + 8 × 2)". #151 measured `user/search` (about 10 results) at 1 point. |
| 7 | Quota exhausted → denied until reset, no gradual throttling | confirmed | JRL FAQ verbatim. CRL: "There is no partial throttling once the quota is reached". |
| 8 | Docs hint at spike tolerance; staff claimed forgiveness; a partner measured a hard wall | confirmed | JRL verbatim. Staff #31 (2025-12-17): "The model will forgive occasional hourly spikes for the app". #97 (2025-12-24): "occasional overage forgiveness". Partner #163 (scott.dudley, 2026-03-18): "access to all APIs is slammed shut and it does not reset until the next hour. I have repeated this 5-6 times". |
| 9 | Three independent limit systems | confirmed | JRL verbatim. |
| 10 | Token bucket per tenant and endpoint; no user scaling; GET/POST 100, PUT/DELETE 50; GET issue 150; capacity unpublished | confirmed (caveat) | JRL verbatim, plus "independent of the number of users" and "GET /api/{version}/issue/{issueidorkey} 150". Only example capacities are given (100 tokens, "X-RateLimit-Limit: 350"). Internal 10-vs-100 inconsistency (V.2-4). |
| 11 | Design for the steady-state rate, not the burst buffer | confirmed | JRL verbatim. |
| 12 | All apps and API tokens on a tenant share the burst bucket | confirmed | CHANGE-2753 (2025-08-28) moreDetails verbatim. The current JRL is consistent: burst "controls how many requests a single tenant can send per second to a given REST API endpoint". |
| 13 | Per-issue writes: 20/2 s and 100/30 s; `jira-per-issue-on-write` | confirmed | JRL "Short window : 20 write operations per 2 seconds", "Long window : 100 write operations per 30 seconds". |
| 14 | RateLimit-Reason only on 429; Jira and Confluence values; undocumented values seen | confirmed | JRL table verbatim. CRL lists `confluence-quota-global-based` and `confluence-quota-tenant-based`. #167 (2026-03-31): `jira-max-concurrent-threads-across-all-instances-per-tenant-per-user`. #169 (2026-04-09): `jira-cost-based`. |
| 15 | Retry-After only on 429, in seconds; some 503s carry it; gateway 429 had no Retry-After | confirmed (reword) | JRL: "Only returned with 429 responses. Indicates how many seconds to wait" and "Some transient 5xx responses (such as 503) may also include a Retry-After header." Incident page verbatim (V.2-6). |
| 16 | NearLimit is true below 20% remaining | confirmed (caveat) | JRL verbatim. Slated for discontinuation for quota values (V.2-3). |
| 17 | `r` appears only past about 80% usage, so local self-accounting is needed | confirmed | CRL verbatim. JRL: "Optionally included. If absent, your app is well within its limits." Partner #165 saw `r=17842` of `q=100100`. |
| 18 | Policy-list grammar (q, w, r, t); policy names; no fixed count or order | confirmed | JRL "Header format" and parameter and policy tables, verbatim. |
| 19 | Beta- prefix = informational; unprefixed = enforced; mixed responses possible | confirmed | CHANGE-3045 (2026-02-02) verbatim. JRL example "Multiple quotas with one out of two rate limit enforced". CHANGE-3080: Beta- means "enforcement has not yet begun for your app". |
| 20 | Reaction depends on the reason (quota pause-all, burst per endpoint, per-issue per issue) | confirmed | JRL "Respect the rate limit reason" bullets, verbatim. |
| 21 | Retry only idempotent requests with Retry-After; backoff and jitter; Retry-After is the minimum; cap of 4 | confirmed (caveat) | JRL verbatim. Prose and pseudocode conflict (V.2-5). |
| 22 | Cost guide caps concurrency at 5–10 | confirmed (reword: advice, not an enforced cap) | optimise-forge-costs (Aug 21, 2026) verbatim (V.2-2). |
| 23 | Spread work over the hour, with jitter on scheduled jobs | confirmed | JRL "Distribute requests over time" verbatim. |
| 24 | Staff: bridge `requestJira` (no resolver) is exempt from points; backend and remote calls count; frontend may be counted later | confirmed (staff-only) | T97828 #133, staff, 2026-01-31, verbatim, incl. "any Jira or Confluence API calls made by a Forge backend or remote service are included". Absent from JRL, CRL and the requestJira doc (V.2-11). |
| 25 | Staff re-confirmed the exemption after a partner report | confirmed | T97828 #168, staff, 2026-04-01, a reply to #154: "We confirmed that Forge front-end traffic is exempted." |
| 26 | Invocations are a separate system from REST points | confirmed | T97828 #129, staff, 2026-01-29, verbatim. CHANGE-3006: invocation limits are "unrelated to the upcoming Jira/Confluence point-based rate limits". |
| 27 | Staff: Tier 1 lets one tenant hurt the others | confirmed | T99654 #2, MaheshPopudesi, staff, 2026-03-19, verbatim. |
| 28 | Bulk edit/move: 1000 issues, 200 fields, 5 concurrent across all users, async taskId; single-issue bulk ops count as one | confirmed | The bulk-ops reference states each fact verbatim, including `GET /rest/api/3/bulk/queue/{taskId}`. JRL "Single-issue bulk operations … count as one operation". JRL also warns to "ensure batching actually reduces overall quota usage". |
| 29 | `bulkfetch`: 100 by default, 1000 only with an explicit field list, >100 otherwise → 400; changelog `bulkfetch`: 1000 issues, 10 field IDs | confirmed | api-group-issues verbatim, plus "larger requests are rejected with a 400 error" and "up to 1000 issues and can filter them by up to 10 field IDs". New since CHANGE-3413 (2026-08-28). |
| 30 | `search/jql` with `nextPageToken`, max 5000 per page for id/key; POST search charged 1 + results | confirmed | swagger-v3 (1001.0.0-SNAPSHOT-0c0db68a…) `maxResults`: "It returns max 5000 issues." Partner #151: "always 1 + the number of returned results". #154: 50 POST `search/jql` = 571 points. |
| 31 | Invocations: 1,200/user/min; 7,000/min or 300/s per install | confirmed | limits-invocation (Sep 1, 2026) table verbatim ("user-led", "fixed one-minute window"). CHANGE-3420 (2026-09-01). |
| 32 | Bridge throws above 500 invokes per 25 s, before any network call | confirmed (source, not the README) | @forge/bridge 7.1.0 `out/invoke/invoke.js:25` `withRateLimiter(_invoke, 500, 1000 * 25, 'Resolver calls are rate limited at 500req/25s')`. `out/utils/index.js` throws `BridgeAPIError`. Same in 7.1.1-next.7. |
| 33 | 25 s general/UI resolver; 55 s web trigger; 900 s async/scheduled (default 55) | confirmed | limits-invocation "Additional invocation limits" table verbatim. |
| 34 | KVS/CE: 1000 RPS; 4000 read + 4000 write 10 KB units/min; batches are cheaper; 25 ops per transaction | confirmed | limits-kvs-ce (Sep 28, 2026) verbatim, incl. "Each transaction can contain a maximum of 25 operations" and `TOO_MANY_REQUESTS`. |
| 35 | Storage is scoped per app and installation, so there is no cross-tenant counter | confirmed (inference part flagged) | limits-kvs-ce verbatim (V.2-13). |
| 36 | Module-level caches are shared across tenants | confirmed | tenant-data-isolation (Aug 1, 2026) verbatim. |
| 37 | Realtime: 50 ops/s per install, combined; `RATE_LIMIT_EXCEEDED`; app must retry | confirmed | limits-realtime (Jun 25, 2026) verbatim. The error-handling page lists `RATE_LIMIT_EXCEEDED`, with publish `errors[]`. Enforced since 2026-06-26 (CHANGE-3246). |
| 38 | Async events: 50 per push, 500/min, 200 KB; `retryAfter` ≤ 900; `FUNCTION_UPSTREAM_RATE_LIMITED`; concurrency key per installation | confirmed (caveat) | async-events-api (Sep 1, 2026) verbatim. limits-async-events (Feb 26, 2026) table. "Concurrency counters are implicitly scoped to an app installation". Retry-count conflict (V.2-7). |
| 39 | Scheduled triggers: ≤5, one fiveMinute; spread across installs; duplicates possible; a throw is not retried | confirmed | scheduled-trigger manifest page (Jun 17, 2026) verbatim. limits-scheduled-trigger (Dec 1, 2025): "Total number of scheduled trigger modules in an app 5" and "with fiveMinute intervals … 1". |
| 40 | LLM: 100 RPM; 500k TPM per model per install (since 2026-10-02); 5-min inference via async | confirmed | limits-llm (Sep 30, 2026) table verbatim. CHANGE-3497 (2026-10-02): "The limit applies per installation of your app for each model." |
| 41 | LLM: no free allowance; credits per 1M tokens Haiku 10 / Sonnet 30 / Opus 50; $0.10 in, $0.50 out; billed to the developer | confirmed (caveat) | forge-llms-api-pricing (Jul 23, 2026) verbatim. Newer models have no published rate (V.2-8). |
| 42 | Each LLM response reports input and output tokens | confirmed | forge-llms-api (Jul 23, 2026) verbatim. |
| 43 | Quotas retired for consumption pricing on 2026-01-01; limits and REST limits are separate | confirmed | platform-quotas-and-limits (Jul 13, 2026) verbatim, plus "(effective January 1, 2026)". |
| 44 | Monthly free allowance per app; overage billed in arrears; listed prices | confirmed | forge-platform-pricing (Aug 12, 2026) table: 200,000 GB-s / 0.000025; KVS reads 0.1 GB / 0.055; KVS writes 0.1 GB / 1.090 (19.8× reads); logs 1 GB / 1.005; SQL 1 hr / 0.143 and 100,000 req / 1.929 per 1M. "keep your app within the free tier". Note that usage in dev and staging is billed too. |
| 45 | `invoke` `rateLimitProperties`; web triggers return X-Ratelimit-Limit/Remaining/Reset | confirmed | invoke doc (Jun 19, 2026) verbatim, with fields `rateLimitValue`, `rateLimitRemaining`, `rateLimitReset` (epoch s). bridge 7.1.0 `out/types.d.ts` has `rateLimitLimit` (mismatch confirmed). web-trigger runtime ref (Apr 17, 2026): "included by default in web trigger responses". CHANGE-3314 (2026-08-13). |
| 46 | Backend resolver is the only reliable way to read headers; frontend hidden | **outdated** | V.1: FRGE-1923 Fixed and live for Jira and Confluence on 2026-04-09. |
| 47 | Bridge `requestJira` always runs as the current user, with no asApp | confirmed (reword) | requestJira doc (May 27, 2026) verbatim, plus "use the requestJira method from the @forge/api package in a back-end function, and call it with api.asApp()". Invocations themselves cost 0 points (V.2-10). |
| 48 | Atlassian PM recommends simulating 429s with mocks and a test-quota layer | confirmed | T97202 #7, Suyash Kumar Tiwari, staff, Principal PM, 2026-01-07, verbatim, plus "local 'test quota' layer in staging". JRL: "Do not perform rate limit testing against Atlassian cloud tenants". |

### V.4 Newer sources found during verification (not cited in the claims)

- https://ecosystem.atlassian.net/browse/FRGE-1923 (closed Fixed on 2026-04-09). Drives V.1.
- CHANGE-3413 (2026-08-28): bulkfetch supports 1000 issues. CHANGE-3365 (2026-07-30): Forge LLMs are GA. CHANGE-3420 (2026-09-01): updated invocation limits. CHANGE-3314 (2026-08-13): invoke metadata. CHANGE-3246 (2026-05-26): Realtime limit enforced from 2026-06-26. CHANGE-3309 (2026-06-29): 200k GB-s allowance. CHANGE-3120 (2026-03-26): async invocations billable from 2026-07-01. CHANGE-3260 (2026-06-17): `appIsLicensed` trigger filter.
- https://developer.atlassian.com/platform/forge/storage-api-limit-handling/ (Aug 21, 2026), for the "maximum of four retries" conflict.
- https://developer.atlassian.com/platform/forge/runtime-reference/forge-llms-models/ (Aug 3, 2026), for the eight active models.
- T97828 #151, #154, #163, #165, #167, #169 and #177, plus T97202 #22: field data and staff walk-backs.
- @forge/bridge 7.1.1-next.7 (2026-10-09): the limiter and the `rateLimitLimit` typing are unchanged.
