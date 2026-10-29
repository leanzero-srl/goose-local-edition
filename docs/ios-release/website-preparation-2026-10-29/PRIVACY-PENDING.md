# INTERNAL: privacy hunks awaiting retention/operator facts

Website base: `c8cdb789d52fc35eefa06b852864cb1706c2a058`. These are proposed replacement/insert fragments, deliberately not included in the applyable support patch. They are not a complete policy and must not be pasted into a public page without the unresolved sections below. There are no public-facing placeholder values.

## Exact target: privacy-policy/page.tsx

Replace the scope bullet that currently names Goose Swarm and LeanZero Link with:

```tsx
<li><strong>Goose Swarm</strong> (our desktop app), <strong>Goose Remote</strong> (our iPhone and iPad companion) and the <strong>LeanZero Link</strong> sign-in service.</li>
```

In `#roles`, add Goose Remote alongside Goose Swarm in the existing controller-scope sentence. Preserve the already published LeanZero SRL legal identification and privacy contact.

Replace the `#goose-link` heading with:

```tsx
<h2 id="goose-link">4. Goose Swarm, Goose Remote and LeanZero Link</h2>
```

Before the existing desktop `LeanZero Link connects your own Macs` paragraph, add `h3 id="desktop-link"` with text `Desktop Link sign-in`. Reconcile each existing desktop bullet against the deployed legacy path rather than applying its token/event rules to mobile. In particular the existing absolute “there is no self-service deletion yet” must be replaced, not left next to a contradictory new subsection. Proposed replacement for that deletion bullet:

```tsx
<li><strong>Account deletion:</strong> Goose Remote provides the in-app deletion flow described below. If you cannot access the app, contact office@leanzero.net for help with your account.</li>
```

Insert after the desktop Link list, before the next horizontal rule:

```tsx
<h3 id="goose-remote">Goose Remote on iPhone and iPad</h3>
<p>
  Goose Remote connects to the Macs you select. LeanZero Link keeps your email
  address and account, device and connection records so you can sign in,
  discover your computers and manage access. Resend delivers email codes for
  sign-in and account deletion. The configured mesh infrastructure processes
  keys and network information needed to connect your devices.
</p>
<p>
  Session messages and management requests go to the selected Mac. Its configured
  AI services and tools can receive messages and session context. Using a cloud
  provider can send content beyond your own devices. The app&apos;s sharing details
  describe available session information and may not identify every onward recipient.
</p>
<p>
  The app saves protected drafts, session snapshots and delivery-recovery
  information locally. Credentials use device-bound Keychain storage. Provider
  credentials you enter are sent to the selected Mac and may remain in protected
  local recovery storage while a request is unresolved.
</p>
<p>
  You can remove local access, revoke a connected device or initiate deletion
  of your Link account from the app. Confirming account deletion removes account
  access while cleanup proceeds. If cleanup is interrupted, the app retains a
  recovery reference and lets you check the request. Deleting the Link account
  does not erase projects, model files or session databases on your Macs.
</p>
```

These paragraphs do not assert complete erasure, a retention duration, universal local processing, absence of all telemetry, or a complete recipient list.

## Missing completion, not optional polish

The inserted subsection must also explain actual remaining record categories, permitted purposes and verified deletion schedules: registry tombstones/recovery history, shared Resend contacts/sent-email history, Sanity historical copies/digests and backups. Current source has no tombstone purge schedule; token expiry is not record deletion. Do not reuse website CloudWatch or legacy OTP TTLs for the controller registry. Resolve actual control-plane/relay operators, locations, transfers and logging first; “configured mesh infrastructure” alone is not the final recipient disclosure.

Update the policy's retention summary and affected legal-purpose rows consistently after those decisions are implemented/verified. Update its effective date only when the completed policy is released. In sibling `layout.tsx`, add Goose Remote to both existing description fields while retaining `pageSocial` and canonical `/privacy-policy`. In shared `subprocessors.ts`, change only verified purposes/scope; native OTP verification does not call the legacy Sanity activity/contact functions. Review rendered privacy, trust-center and DPA consumers of that shared list together. No new retention/legal promise is supplied by these pending fragments.
