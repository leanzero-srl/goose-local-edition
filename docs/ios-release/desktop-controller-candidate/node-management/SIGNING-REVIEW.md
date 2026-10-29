# Node-management signing comparison preparation

29 October 2026. Parent independent source/control review: PASS for this bounded operator change. No new package was signed, installed, uploaded or notarized by this preparation.

The previous full-sign validator classified any changed Mach-O with preserved path/mode/xattrs as a candidate signing change and then verified its signature. Only CLI and main executable had executable-byte equivalence checks. This operator adds the missing comparison for every changed Mach-O, retaining the existing full fingerprint, signature/team, resource negative controls and ZIP roundtrip checks.

Inspected both retained full-sign artifacts57419bcc521c and bf5966b19fdf:29 changed Mach-O paths each;27 thin arm64 and2 thin x86_64. The latter are Argon2 prebuilds in the bundled document/search MCP packages. There are no universal binaries in those actual inventories. The new comparator changes only the original header guard to admit these two thin64 CPU types with a minimum header length. It retains strict load-command coverage, removed-signature requirement, read-only __LINKEDIT bounds and whole normalized-byte equality. Unsupported CPU/magic fails explicitly. No universal parser or broader normalization was added.

[Control receipt](signature-comparison-controls.json) proves actual prior unsigned/signed arm64 helper and x86_64 Argon2 pairs compare equally. A real helper __TEXT,__text code byte at derived offset2360 was flipped in a temporary stripped copy and rejected. Unsupported CPU and universal magic were rejected. No installed app was changed; codesign --remove-signature operated only on temporary copies. New signing runs repeat the helper corruption control and write `machOEquivalence` results keyed by every changed bundle path before a success receipt.

Exact reviewed files:

- sign-only.cjs SHA256 dc8d76089549e9565693a2c4b5fb85a021cd145f0fb712e848fdd16cac3e3ba4
- signature-equivalence.cjs SHA256 91b9f66db0fcd8c624c6fe4fb51cd3ac0f345a5c8cc01701eae097cfabd232a2
- signature-comparison-controls.json SHA256 abdcaf30693e00655d2556bd8869105a7c26bca2633d3aa078240da389d6bc72

These are exact operator copies; embedded paths remain the reviewed prerequisites. Before use, place the comparator at `/tmp/goose-node-signature-equivalence.cjs` (hash pinned in the wrapper), the wrapper at `/tmp/goose-node-management-sign-only.cjs`, and the unchanged metadata validator at `/tmp/goose-controller-signing-metadata-validator.cjs` (existing hash pinned). Existing repository helpers, signing keychain and fresh `/tmp/goose-controller-corrected-signed-…` stage are still required. Execute only against the independently verified fresh unsigned package/build receipt, under the resource guard. The preparation receipt is not new-artifact equivalence proof; that comes from the resulting per-path comparison and signed archive receipt.
