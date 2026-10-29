# Latest controller notarization receipt

29 October 2026. Source commit `57419bcc521ce6f3ef0cc00dde59b735415496c3`, native desktop version 3.0.134.

The owner approved exactly `/tmp/goose-controller-corrected-signed-57419bcc521c/Goose-Swarm-signed.zip`, SHA-256 `b1703dfda06d48ac052a5f43fdb3c952785fc4a04075bd84f0f25a56127c7872`. Root submitted it once at `2026-10-29T07:10:57.736Z`, submission `94f23745-7c84-4e9a-a459-1961427aaea8`, job `laptop-20261029-091056-2b5d`. No repeated submission or re-signing is authorized by this follow-up.

As of `2026-10-29T07:16:25.926Z`, Apple reports In Progress. Stapling has not run. See `notary-receipt-57419bcc521c.json` for the latest recorded state. Polling backs off between checks; no installed Mac restart is required for Apple's eventual online ticket acceptance.

## Operator verification

The follow-up worker independently compared `/tmp/goose-notarize-latest-57419bcc521c.cjs` with the previously reviewed `notarize-corrected-bf5966.cjs`. Only stage, approved ZIP, immutable signing receipt, source commit and signed-CLI fingerprint pins changed; all secure-prompt, exclusive-submit, state and staple-validation logic is identical. Latest operator SHA-256 is `5e2546c508edacfad769479ce325d70d80621d737af8477d8ea6df1b7d9dd7b7`; durable copy `notarize-latest-57419bcc521c.cjs`.

The unchanged secure-prompt wrapper SHA-256 is `b170e22d64a10217c3ba72e15b5a5c8a177e8671359ddcf4fa9657babb64429e`, matching the committed `notary-secure-prompt.py`. Existing `~/.leanzero/apple/notary.env` is mode0600. Credentials are sourced with shell tracing disabled and never printed or put in password argv. The password is supplied only to the already-reviewed echo-disabled terminal prompt.

Recovery uses the existing saved submission: `node /tmp/goose-notarize-latest-57419bcc521c.cjs info` after loading the existing credential environment with tracing disabled. Do not run submit again. After the persisted Apple status is Accepted, the same operator's `staple` action runs through resource-guarded node-dispatch with normal keychain/fingerprint permissions. It requires strict Apple team signature, stapler validation, Gatekeeper acceptance and a complete bundle fingerprint that permits only `Contents/CodeResources` to change. It preserves the approved pre-upload ZIP and signing receipt and creates no replacement distribution ZIP.

No installed-app replacement/restart, desktop rebuild, native archive build, export, App Store upload or publication is part of this worker's notarization follow-up.
