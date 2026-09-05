# Distributing the desktop app outside the App Store (macOS notarization)

Short version: **yes, you need to notarize.** On macOS 10.15+ Gatekeeper blocks any app that is
not signed with a **Developer ID** certificate *and* notarized by Apple — the user gets "…can't be
opened because Apple cannot check it for malicious software" or "…is damaged", and has to dig through
System Settings to override it. Ad-hoc / self-signed signatures (what `just release-fork` uses today)
work on *this* machine but not on someone else's. Notarization is what makes the DMG open cleanly on
any Mac.

This is already wired into the build — `forge.config.ts` signs with your Developer ID and notarizes
the app whenever `APPLE_TEAM_ID` is set, and `just release-notarized <version>` drives the whole
thing end to end (build → sign → notarize app → build DMG → notarize + staple DMG → verify). What is
missing is only the Apple **identity**, which is yours to create.

## What you need (one-time)

1. **Apple Developer Program membership** — $99/year, https://developer.apple.com/programs/ .
   Notarization and the Developer ID certificate both require it. (Apple only; there is no
   third-party substitute for Gatekeeper acceptance.)

2. **A "Developer ID Application" certificate** in your login keychain. Easiest path:
   Xcode → Settings → Accounts → (your team) → Manage Certificates → **+** → *Developer ID
   Application*. Or create it in the developer portal and double-click the `.cer` to install.
   Verify it landed:
   ```
   security find-identity -p codesigning -v | grep "Developer ID Application"
   ```
   (This is a *different* certificate from the "Goose Local Dev" self-signed one used by
   `release-fork`, and different again from the "Apple Distribution" cert the App Store uses.)

3. **An app-specific password** for the notary service — https://appleid.apple.com →
   Sign-In & Security → App-Specific Passwords → **+**. Looks like `abcd-efgh-ijkl-mnop`. This is
   NOT your real Apple password. (Alternatively an App Store Connect API key, but the app-specific
   password is the simplest.)

4. **Your Team ID** — https://developer.apple.com/account → Membership details (a 10-char code like
   `A1B2C3D4E5`).

That's the entire list. No entitlements work (they already exist), no per-binary signing work (the
build signs goosed, tailscaled, tailscale, node and uvx for you), no Apple-side app registration
(notarization does not need an App ID or provisioning profile — that is only for the App Store).

## How to build a notarized DMG

```
APPLE_TEAM_ID=A1B2C3D4E5 \
APPLE_ID=you@apple.example \
APPLE_ID_PASSWORD=abcd-efgh-ijkl-mnop \
just release-notarized 2.0.5
```

The recipe refuses up front if any of the three vars is missing or the Developer ID cert is not in the
keychain, so you cannot accidentally ship an un-notarized build. It ends by running
`stapler validate` and `spctl` — both must report *accepted / Notarized Developer ID*. The result is
`ui/desktop/out/make/Goose-<version>.dmg`, which drag-installs on any Mac with no prompt.

## What the build actually does (so nothing is a black box)

- `pnpm run make` → `@electron/osx-sign` re-signs the **entire** bundle with your Developer ID and the
  **hardened runtime** (required for notarization), applying `entitlements.plist` (JIT for node's V8,
  network client/server for goosed + the embedded tailscaled, file access, etc.). It walks
  `Contents/Resources/bin`, so the bundled goosed / tailscaled / tailscale / node / uvx are all signed
  the same way — a single un-hardened Mach-O anywhere in the app makes Apple reject the whole thing.
- `@electron/notarize` uploads the app to Apple's notary service, waits for the pass, and staples the
  ticket into the `.app`.
- The recipe then builds the DMG from that stapled app and notarizes + staples the **DMG** as well, so
  the downloaded file itself carries the ticket (works even on a Mac that is offline on first launch).

## Notes

- **Auto-update stays consistent.** Once you sign with Developer ID, `release-notarized` also produces
  the Developer-ID-signed `Goose.zip` + `latest-mac.yml` for the Squirrel feed, so the update path and
  the DMG share one signing identity. After you have the Apple account, this recipe supersedes
  `release-fork` as the canonical release — `release-fork` remains only for signing-less local builds.
- **Apple Silicon only**, matching the product (MLX). The recipe builds the arm64 DMG; there is no
  Intel or Windows path by design.
- The first notarization submission can take a few minutes (Apple-side); `--wait` blocks until it
  finishes and prints the log URL if it is ever rejected.

## LeanZero setup (2026-09-05) — how our Macs actually sign and notarize

Everything below is DONE on the workhorse (Mac Studio) and is the source for every other Mac.

- **Identity:** `Developer ID Application: Mihai Perdum (ZZ8MTZ6NRZ)`, G2 Sub-CA, valid to 2031-09-06.
  Created from a CSR generated on the workhorse; the private key never left it.
- **Bundle, never in git:** `~/.leanzero/apple/` — `goose-developer-id.p12` (cert + key), `p12-password.txt`,
  `notary.env` (`APPLE_TEAM_ID` / `APPLE_ID` / `APPLE_ID_PASSWORD` = an app-specific password),
  `DeveloperIDG2CA.cer`, and the CSR/key/cer the p12 was built from.
- **Keychain:** a DEDICATED `goose-signing` keychain (password in `signing-keychain-password.txt`), the identity
  imported with `codesign` in its ACL and the `apple-tool:,apple:,codesign:` partition set, first in the user
  search list. Result: `codesign`, forge's osx-sign and notarytool never prompt and never need a login password.
  (The login keychain was NOT the right place: `set-key-partition-list` there needs the user's password.)
- **One command per release:** `just release-notarized <version>` — the recipe sources `notary.env`, unlocks
  `goose-signing`, builds goosed, signs the whole bundle with hardened runtime, notarizes + staples the app,
  builds and notarizes + staples the DMG, verifies with `stapler validate` and `spctl`, and rebuilds the
  auto-update zip + manifest. Under hermit, put the corepack pnpm shim first on PATH (hermit's own pnpm is broken).
- **Any other Mac (the MacBook):** `bash ui/desktop/scripts/bootstrap-signing.sh` — pulls the bundle over the
  `workhorse` SSH alias if it is not already in `~/.leanzero/apple`, creates the same keychain, imports the
  identity, and PROVES it: signs a scratch binary and reads back `Authority=Developer ID Application`, then
  authenticates to Apple with `notarytool history`. Idempotent. After it, `just release-notarized` works there.
- **Proof that this works end to end:** the 2.0.2 build on 2026-09-05 (see local-edition/mlx/NOW.md for the
  `spctl` verdict of that run).
