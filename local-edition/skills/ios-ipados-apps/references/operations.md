# Support after release

## Establish before launch

Name the support owner, support URL/contact route, supported OS/device range, app/backend compatibility window, release procedure, and incident owner. These are project decisions; do not invent contractual response-time promises or paid support tiers. Prepare an in-app way to report app/build, OS, connectivity state and a diagnostic reference without forcing users to send full conversations or credentials.

Retain each release's commit, signed archive provenance, build number, dependency versions, dSYMs, App Store/TestFlight receipt and compatibility notes. Use the project's real backup/artifact store. Do not keep bulky duplicate archives on a working SSD when the canonical release storage already has them. Record where irreplaceable signing/recovery material is securely backed up; never put it in source or the skill.

A minimal compatibility record answers: which installed mobile versions can read/write each backend version; which capability is absent on older hosts; whether an old client is read-only; token/schema migration; deprecation date and recovery. Roll out additive backend support before clients need it. A mobile release does not force every user to update immediately.

## Companion-host protocol upgrades

For a mobile companion, inspect the versions actually running on every relevant host, including offline enrolled hosts. A replacement test profile does not migrate an existing device. Preserve each host's own identity, original session store and permission choices; prove active work is idle before a required restart. Update a signed desktop app as a whole, not by replacing an internal executable. Verify the real network provider retains the expected device identity across reauthentication; mocks cannot establish that. Separate durable verified identity from a short-lived online lease when designing offline behavior, and independently review any admission-policy change.

## Observe without collecting the user's work

Use appropriate first-party diagnostics first: [Xcode crash reports](https://developer.apple.com/documentation/xcode/diagnosing-issues-using-crash-reports-and-device-logs), [MetricKit](https://developer.apple.com/documentation/metrickit), and [App Store Connect analytics](https://developer.apple.com/app-store-connect/analytics/). Verify what is available for the current OS, user sharing settings, app state and account permissions. Do not promise real-time or exhaustive telemetry; MetricKit reports are delivered on the platform's schedule and coverage varies.

Track user outcomes, such as reconnect success, command acknowledgement/delivery, sync completeness, auth failures, crashes/hangs and excessive energy use. Keep event payloads minimal. No prompts, transcripts, model responses, provider tokens, host paths or personal node names in analytics by default. If additional diagnostic data is needed, make capture explicit, scoped, redactable and time-bounded; state retention and deletion.

Separate service outage, network loss, expired credentials, outdated client, host offline, job failure and rendering bug. They have different owners and remedies. A lower crash count with no use is not an improvement; compare exposure and app versions.

## Incident procedure

1. Establish impact from actual affected builds/accounts/devices and backend revisions. Preserve minimal redacted evidence. For a remote mutation with uncertain delivery, query its operation ID before suggesting another send.
2. Contain through already authorized mechanisms: pause rollout, disable only the affected capability through an existing reviewed remote-config mechanism, or recover a backend while preserving client compatibility. Do not invent an unreviewed remote-code hotfix. Security/account actions retain their own authorization requirements.
3. Reproduce the user's exact path, including state migrations, permission denial, backgrounding and network changes. Fix the common cause and verify both affected and unaffected versions.
4. Release a newer tested build or compatible server repair through the project's release authority. Keep support/incident communication accurate; send it only if authorized. Do not claim a repair is installed on all devices when it has merely been submitted.
5. Verify real recovery using the same outcome metric and user path. Record cause, detection gap, regression proof and one durable procedural correction where warranted.

Apple [phased release](https://developer.apple.com/help/app-store-connect/update-your-app/release-a-version-update-in-phases/) applies to eligible version updates, not a first-release safety switch. It can be paused, but users may still download the update manually. A pause cannot downgrade installed apps. Check the current schedule and pause allowance in the live documentation when operating a release; do not freeze those numbers into automation.

An [expedited review request](https://developer.apple.com/distribute/app-review/) is an option for qualifying urgent fixes, not a promised timeline. Removing an app from sale can stop acquisition but does not repair installed copies or necessarily stop their backend traffic. Removal during phased release ends phasing for that version; reinstatement makes it available to everyone. Do not use removal/reinstatement as pause/resume. Check the live consequences before acting; a fresh controlled rollout requires an eligible new version. Select containment for the actual failure.

## Ongoing maintenance

At each release and when Apple announces changes, review upcoming submission deadlines, supported OS behavior, Xcode/SDK changes, entitlements, privacy/SDK requirements, accessibility, payment rules and export information. Test OS betas in isolation against the supported release path; a beta is not automatically an appropriate production upload toolchain.

Review networking/crypto and other dependency vulnerabilities, licenses and release notes. Pin and verify updates; test reconnect and old-client behavior before rolling out protocol changes. Keep certificates/provisioning, APNs configuration if used, review accounts, support/privacy URLs, and external service credentials current. Separate their expiry/failure alerts from actual service health.

Verify retention on the running deployment, not just in a repository default. Record enabled service paths without exposing credentials, the loaded cleanup schedule and a recent execution result. Token validity, record TTL, physical removal and provider backups are different facts; a working TTL sweep does not prove registry history is purged. Describe scheduled log rotation with its actual age calculation rather than promising an exact erasure deadline.

Account deletion must reach the authoritative identity/data services and revoke devices/tokens; verify caches and support/diagnostic stores according to the stated retention policy. For products controlling user-owned hosts, explicitly distinguish deleting service-held account data from destructive deletion of the user's own local work. Never wipe remote work as an undocumented consequence of logout.

When support reveals a repeatable failure, add a focused regression and update the owning project runbook. Update this generic skill only for a reusable platform lesson. Automated monitoring or recurring checks require the user's scheduling instruction; creating an app is not authorization to create background automations or email customers.
