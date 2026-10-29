# Corrected controller notarization receipt

29 October 2026. Source commit `bf5966b19fdf4835b8feadcbe95b2951c7f3498b`, app version 3.0.134.

The owner approved submitting exactly `/tmp/goose-controller-corrected-signed-bf5966b19fdf/Goose-Swarm-signed.zip`, SHA-256 `8261f27b92581b9d26c8b12bdb7e8a1f938228cb7f730a4d8f7d76f5df3b873e`. This authorizes Apple notarization, not publication or App Store submission. The superseded `3d124c48` archive was not submitted.

Apple submission `d4b59b60-4db2-43e9-921a-fe3bcdcc7f8f` was created by the single attempt beginning `2026-10-29T05:27:33.972Z`. Apple accepted the exact archive, confirmed at `2026-10-29T05:32:44.007Z`; its own log matches the approved SHA and says “Ready for distribution.” The staging app was stapled at `2026-10-29T05:33:04.649Z`. Strict Apple team signature verification, stapler validation and Gatekeeper assessment passed; Gatekeeper reports `accepted / Notarized Developer ID`. See `notary-receipt-bf5966b19fdf.json` for machine-readable state.

## Safeguards and proof

The operator pins the exact ZIP, immutable signing receipt, source commit, signed CLI and signing fingerprint. It creates the attempt record exclusively before submission and refuses any subsequent submit when that record exists. An unknown outcome must be reconciled with Apple history; it is never retried blindly. The pre-upload signing receipt and ZIP are preserved. No rebuilding, resigning, replacement of installed apps, main merge or public release was performed.

Independent review found password exposure through the historical operator's command arguments. The corrected operator omits the password argument and uses a secure terminal prompt: only a password prompt with terminal echo disabled can receive it, and the notarytool child has no Apple credential environment variables. The reviewer independently reran a successful secure-prompt fixture and an echo-enabled refusal fixture; both passed and the synthetic secret appeared in neither output. Read-only live Apple history authentication then succeeded before submission.

The reviewed operator SHA-256 is `f1f62c6b1bd9803dfef66ee113f3f7bad132c9a36d1087bbd594068f34432041`; secure-prompt wrapper SHA-256 is `b170e22d64a10217c3ba72e15b5a5c8a177e8671359ddcf4fa9657babb64429e`. The exact reviewed files are preserved as `notarize-corrected-bf5966.cjs` and `notary-secure-prompt.py`; `notary-secure-prompt-fixture.py` is the non-secret test fixture.

## Recovery

Private active state and redacted logs live in `/tmp/goose-controller-corrected-signed-bf5966b19fdf`. Operator runtime paths are `/tmp/goose-controller-notary-bf5966.cjs` and `/tmp/goose-notary-secure-prompt.py`. Restore those files from the committed helpers if needed, verifying the hashes above. Credentials stay in the existing mode-0600 `~/.leanzero/apple/notary.env`; do not print them or put them in argv.

After sourcing that existing local credential environment with tracing disabled, `node /tmp/goose-controller-notary-bf5966.cjs info` queries only the saved submission. `node /tmp/goose-controller-notary-bf5966.cjs staple` refuses until the persisted Apple status is Accepted. Its complete fingerprint comparison permits only `Contents/CodeResources`, where stapler stores its public ticket; strict Apple team signatures, stapler validation and Gatekeeper assessment must pass. The operator does not create a replacement distribution ZIP.

## Final artifact identity

Node-dispatch validation job `laptop-20261029-073256-a6dc` exited 0 at `2026-10-29T05:33:05Z`. Only `Contents/CodeResources` changed in the complete bundle fingerprint. Post-staple fingerprint SHA-256 is `89a3976616dfa2dce5787de6ea97e3453e2884ce08537f20477f925121752a06`; the signed CLI remains `349b242b64a4a48072fbbfcecda0c9a9a2c32fef5639ae6c045003b34d7bee6f`. The original submitted ZIP remains `8261f27b92581b9d26c8b12bdb7e8a1f938228cb7f730a4d8f7d76f5df3b873e`; its pre-upload signing receipt remains `0ecf05a4e59b4dda8941f01b069fb58340c0c7a926833f9fdde965541126844b`. No replacement delivery archive was created.

Apple returned 43 ticket entries and one warning: it could not unpack the Mammoth dependency test fixture `Contents/Resources/bundled-mcps/leanzero-documents/node_modules/mammoth/test/test-data/empty.zip`. The actual fixture is a valid 22-byte empty ZIP: Python zipfile reports zero entries, so it contains no executable. SHA-256 `8739c76e681f900923b900c9df0ef75cf421d39cabb54650c4b9ad19b6a76d85`. The original warning is retained in `notary-apple-log-bf5966b19fdf.json` and the receipt; it did not prevent Apple's Accepted verdict.
