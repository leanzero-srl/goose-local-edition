

Got everything from primary sources. Here's the answer.

## How long can they stay on Data Center

Atlassian's official position is a single **End of Life** date with a 3-year phased wind-down, not separate "end of sale" and "end of support" dates. The three dates that matter:

| Date | What happens |
|---|---|
| **30 Mar 2026** (23:59 PST) | End of sale to **new** customers — no new DC subscriptions or Marketplace apps |
| **30 Mar 2028** (23:59 PST) | End of sale to **existing** customers — last day to buy new DC licences, apps, or expansions |
| **28 Mar 2029** (23:59 PST) | **End of life** — all DC licences expire, products go **read-only**, support ends |

So the direct answer for Harbourline: **they can run Jira Data Center until 28 March 2029**, keeping technical support and security fixes for critical issues the whole way. After that it's read-only and unsupported. Extensions past 28 Mar 2029 are **"by exception only"** — not something to plan around.

Two things worth saying to Aoife explicitly:
- As an **existing** customer they're on the more favourable track — they can still purchase/expand DC until **30 Mar 2028**, and the new-customer cutoff (30 Mar 2026) has already passed and doesn't touch them.
- That leaves roughly **2.5 years** from today (25 Sep 2026) to EOL. Realistic planning target is well inside that, because the last stretch is read-only with no security fixes.

Sources (both official Atlassian):
- https://www.atlassian.com/licensing/data-center-end-of-life
- https://www.atlassian.com/blog/announcements/atlassian-ascend (the "Atlassian Ascend" announcement, 8 Sep 2025)

Note: Bitbucket DC and Jira Align DC are explicitly **excluded** from EOL — not relevant to a Jira-only scope, but good to know it wasn't a blanket "all of Data Center" order.

## Does JCMA support Jira 9.12?

**Yes — comfortably.** The official supported-versions page says the assistant is **pre-installed on Jira 8.14 and later**, and can be manually installed back to Jira Software Data Center 7.6. Since 9.12 > 8.14, JCMA is already on their instance — no install step, just update it to the latest version before running.

Source (official):
- https://support.atlassian.com/migration/docs/update-or-install-the-jira-cloud-migration-assistant/ ("Supported versions")

So the migration tooling is not a blocker at all — 9.12 is squarely supported. The real readiness work is the data hygiene (the duplicate/no-email/service accounts) and the FRT project size, not version compatibility.

Let me save these dates so they're grounded next time rather than re-researched.

