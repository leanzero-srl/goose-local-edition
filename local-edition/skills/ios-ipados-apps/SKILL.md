---
name: ios-ipados-apps
description: Design, build, test, submit, and maintain iPhone and iPad apps. Use for native iOS/iPadOS development, TestFlight and App Store preparation, release incidents, compatibility updates, and support after launch.
---

# iOS and iPadOS apps

Build the requested product and prove it on the devices it supports. Treat App Store readiness, backend compatibility, and support after release as part of the product. This skill works in Claude and Codex; it does not depend on a particular agent runner or connector.

## Start with the actual assignment

Read the repository instructions, existing app, design system, release history, and outstanding incidents. Establish whether the user wants research, a plan, implementation, a beta, a submission, or a production release. Preserve existing authorization; a planning request does not authorize deployment. Creating or using this skill alone does not authorize customer messages, purchases, account changes, submissions, or production releases.

For a new app, record the audience, essential journeys, iPhone/iPad layouts, oldest supported OS, offline behavior, account/data boundaries, business model, backend owners, and release destination. Infer routine choices; ask only for decisions that materially change the product. Do not turn a focused companion into a desktop feature clone.

Use current Apple primary sources. Recheck dated SDK deadlines, submission requirements, review guidelines, entitlements, and commercial rules before a release. Distinguish Apple requirements, design recommendations, project choices, and policy interpretations. Never promise approval or describe an internal checklist as Apple certification.

Read only the reference for the current work:

- [Development and verification](references/development.md): architecture, adaptive UI, device/network testing, build evidence.
- [App Store preparation](references/app-store.md): policy classification, privacy, signing, review access, beta and submission.
- [Support after release](references/operations.md): compatibility, diagnostics, incidents, updates, account and dependency maintenance.

## Architecture and implementation

Prefer the existing stack. For a new native app, SwiftUI with an explicitly owned state model is a reasonable default; choose UIKit or a cross-platform stack when the product warrants it. Native Apple controls are appropriate on Apple platforms. A house ban on browser-native HTML controls is not a ban on SwiftUI navigation, pickers, sheets, or accessibility behavior.

Keep domain state, transport, persistence, and presentation separate. Cancellation must reflect user intent: leaving a screen or suspending a controller must not cancel a remote job. Treat scene lifecycle, connection loss, process death, and reinstall as ordinary states with explicit recovery.

For remote-control products, the execution owner remains authoritative. Separate client/controller capabilities from compute and job capabilities; enforce authorization at the receiving service. Show which host owns the work, stale data, command acceptance/delivery, and unsupported controls. Retry mutation requests with durable idempotency and status lookup; never infer failure from a dropped response or claim an agent obeyed a nudge merely because it received it.

Keep credentials in Keychain and out of URLs, analytics, logs, crash breadcrumbs, screenshots, and shared preferences. Redact diagnostics by default. Design revocation, account changes, cache cleanup, and deletion before shipping. Do not disable transport validation globally to make development networking work.

## Proof before calling it ready

List the actual user-visible fields and transitions for each journey, then verify them. A successful build or mock server test does not prove a real-device/backend journey. Use simulator coverage for iteration and physical iPhone/iPad proof for lifecycle, networking, permissions, accessibility, keyboard, and performance that simulation cannot establish.

Test at least one failure path for each critical write: interruption, duplicate submission, stale state, authorization loss, and partial completion where applicable. Test migrations from a released app and backend compatibility with installed older clients. Keep evidence tied to app version/build, commit, OS/device, backend version, and environment; report unverified gaps plainly.

Independently review sensitive writes, authentication, data handling, and externally published material in proportion to their scope. Use a fresh reviewer with code, actual artifacts, and primary sources; confirm findings before changing the product.

## Release and support contract

Prepare the concrete archive, metadata, review access, privacy answers, export classification, and rollback/containment plan before a submission decision. If release authority is already explicit, proceed within it; otherwise ask only at the external action boundary. Do not repeatedly ask for previously authorized steps.

Ship with a supported client/backend version matrix, an owner for support, a reachable support URL, redacted diagnostics, and a response procedure. Pausing phased release does not remove an update from devices; an App Store repair normally needs a newer build. Preserve backend compatibility while installed clients catch up. Do not add monitoring schedules or contact users without authorization.

## Durable maintenance

Keep project-specific runbooks, bundle IDs, signing team references, test evidence, release receipts, and open work in the project. Keep this skill generic. Never store secrets or signing assets in it. Edit the canonical skill on its owner machine, then regenerate or copy its discoverable mirrors rather than maintaining divergent versions.

This installation is laptop-owned. Canonical source: `~/.claude/skills/ios-ipados-apps/`; Codex discovers the `~/.agents/skills/ios-ipados-apps` link. The Goose repository's `local-edition/skills/ios-ipados-apps/` is a versioned snapshot, updated from the canonical source after edits.

2026-10-26: Created from the Goose remote-controller architecture work. The general rules above apply beyond Goose; its session/Link design belongs in its project plan.

2026-10-26: Simulator setup proved that an SDK can exist without a runtime, and a new runtime can support only one device model. Added compatible-runtime discovery and the distinction between sandbox permission errors and actual simulator failures.

2026-10-26: Added real simulator Keychain entitlement verification and interrupted-command recovery proof after the native app exposed a storage failure that mocked tests missed.

2026-10-26: Added the combined private-network TLS proof after localhost certificate tests passed but ATS rejected the actual mesh address. Documented supported Network framework investigation without claiming it was already proven.
