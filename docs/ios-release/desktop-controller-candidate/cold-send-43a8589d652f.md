# Cold owner send candidate — 29 October 2026

Source43a8589d652f72c145f57cbe667b0b9ec2a71976 fixes cold preparation self-invalidating an admitted controller command. Full gate laptop-20261029-111601-1870 passed CLI build,26 affected tests and full workspace/all-targets Clippy with warnings denied. The actual first phone rejection, including command11a2f615 and owner revision history, remains in physical-iphone-cold-send-rejection-2026-10-29.json. No retry or draft change was made.

Stamped CLI build laptop-20261029-113026-440f verified4176 runtime Git blobs against source/treecfc2004805b779ee7e13e287c0385b46a98ddd35, then embedded the exact commit. Input CLI SHA2563d244ac27cf450a8619273fbeeff3a3edd392b4f85ca864a5336b147ed103d8c. Runtime manifest is /tmp/goose-cold-send-runtime-manifest.json; its hash is in the build receipt.

Whole-app staging/signing laptop-20261029-113206-e91a used the reviewed, newly verified stapled574 archive. All original signature/staple/fingerprint/Mach-O checks were retained. Every non-iOS UI source input matches574; no Electron rebuild was needed. Exact bundle changes are the new CLI, the Electron main executable signature, and outer CodeResources; every other fingerprint entry remains identical. Packaging laptop-20261029-113323-721c verified strict signature and exact archive-roundtrip fingerprint.

Complete signed archive: /tmp/goose-controller-incremental-43a8589d652f/Goose-Swarm-signed.zip, SHA25636f291982387c88f357f077e1b2231333f42922a97a537857a6be6f1fe56cfb2. App and release-receipt.json are alongside it. The copied build-receipt-43a8589d652f.json records every pin and equivalence result.

This is a new complete signed bundle, not an installed-CLI swap. It is not yet notarized or installed. Root owns the original-app upgrade/profile proof and actual cold phone delivery. The prepared disposable session20261029_1 must remain cold after restart until the phone sends its new approved marker. The old session’s edited draft and rejected receipt remain untouched. No App Store/native archive or model inference was performed by the build worker.
