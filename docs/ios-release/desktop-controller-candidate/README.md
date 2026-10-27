# Controller host upgrade desktop package — 27 October 2026

A complete normal arm64 Goose Swarm.app was built once on Gabee in an isolated staging tree. No installed app, original profile, keychain contents, permissions, or running benchmark was replaced or launched by this packaging task.

## Provenance

- Desktop source snapshot: `b018f72f51f0746c4219f6f08015da6951fdbd42` (the two fixture corrections are the only changes from runtime source `adebf78f42333cd9682a31555043f80b3f992adf`). Root later merged upstream skill documents only; application bytes remain unchanged.
- Tested CLI input: `/tmp/goose-controller-reauth-fixed`, SHA-256 `a1b0cd01dadc71585d50554af23ebb5cefe60a37d919de3a2f9c16aba6d63151`. The packager and signer each assert the embedded CLI has this hash before signing.
- App identity `net.leanzero.goose-swarm`, version `3.0.132`, Electron `41.0.0`; no version bump or release publication.
- Normal packaging command: `pnpm run package`, pinned pnpm `10.30.3`, isolated Node `24.15.0`. The normal prehook verifies frozen benchmark manifests and bundled MCP revisions; the complete SDK/main/preload/renderer build ran.
- MCP payload was copied with `ditto` from public installed resources into staging, then checked against repository commit/lock/patch pins. Mesh binaries were copied into staging from the same public installed bundle; version `1.98.8`. No signing secrets or profile state crossed machines.
- The checkout's existing `ui/desktop/bundled-mcps` symlink points into the installed app, so packaging was deliberately performed in a separate git-archive tree. The link and original app were untouched.
- Unsigned archive SHA-256: `c3a7939f488e7d906288a95d7e36992b57afb5269a426cc8266d5828f752eb25`, matched after transfer.

## Validation

Typecheck passed. The first full `vitest run src` on Gabee recorded 5281 passed, 4 failed, 3 skipped (563 files). Two failures were stale fixtures: the My Macs permission text and a real-time 3-second enrollment wait; commit `b018f72f5` corrects those assertions without changing production behavior. The unchanged evidence-clip and AlertBox failures passed with one worker, consistent with contention. The final scoped rerun covered all four failed files plus LinkedSessionsView: 50 passed, 1 optional real-recording test skipped. This is not represented as a second all-green full-suite run.

Independent review confirmed the two fixture corrections retain the actual permission scope and the production 2-second poll path. No clock gate or assertion was weakened. The app contains the host register, recover-node-secret, and lease route markers. Live profile/phone verification belongs to the rollout, not this packaging receipt.

Before signing, the app census found 15985 regular files and 41 symlinks, no symlink target outside the app, and no inode shared with the installed app. Local signing uses the existing Developer ID Application identity for team `ZZ8MTZ6NRZ` and the repository entitlements. Strict/deep signature verification passed. After removing signatures from disposable copies, both CLI files are 461,234,276 bytes and differ in exactly one byte (offset 1954), inside the read-only __LINKEDIT LC_SEGMENT_64.vmsize field. The original allocation is 166,526,976 bytes and the signed allocation is 163,840,000 bytes; actual linkedit payload is 162,914,404 bytes in both. Every other byte is identical. The accompanying Mach-O-aware validator permits only that allocation field after validating format, protections and bounds. Independent review concurred. Negative controls changing executable content, executable permissions or file bounds are rejected.

## Notarization boundary

Automatic approval review refused the combined signing/notarization command before execution because notarization uploads the complete proprietary app bundle to Apple and explicit authorization for that destination was not present. The safe local-sign-only command was separately approved. The user then explicitly said “Approve Apple notarization upload” for this complete staging bundle. The notarization-only command succeeded: stapler validation passed, strict/deep signature verification passed, and Gatekeeper returned accepted / Notarized Developer ID for team ZZ8MTZ6NRZ. No public or App Store release, Gatekeeper override, quarantine removal, installation, or app launch was performed by this packaging task.

## Delivered candidate

- Normal signed/stapled app: `/tmp/goose-controller-signed-stage/Goose Swarm.app`.
- Metadata-preserving archive: `/tmp/Goose-Swarm-controller-notarized.zip` (447 MiB display size), SHA-256 `520c8596fc790e8e4431e378aacef412458d905a19c7701209829a87ec9e94e0`.
- Signed embedded CLI SHA-256: `788ddf7e19b22f52eca6326e5fc511631b2d8687a80bced6561345dad8ca700a`.
- Exact signed CLI equivalence SHA-256 after narrowly normalizing the validated allocation field: `bb786135d4b73987e47299267ff0163c43beb8db53478a6697df92dfa8b45855`.
- Final census: 15,986 regular files, 41 internal symlinks, no escaping target or installed-app shared inode.
- [Machine-readable signing receipt](receipt.json) and [equivalence validator](cli-signature-equivalence.cjs).

The archive and exact hashes were handed to the host-upgrade lane for its authorized idle-host install and real normal-launch checks. This receipt does not claim original-profile, phone-to-owner, or four-Mac integration success.
