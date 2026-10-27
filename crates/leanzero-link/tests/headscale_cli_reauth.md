# Desktop Headscale reauthentication fixture

Opt-in provider/control-plane proof, using the actual Goose-bundled CLI and daemon.
No tsnet substitution, production worker, live account, original profile, or system daemon writes.
It pins official Headscale 0.29.3 darwin-arm64 by SHA256. Supply the public release binary locally.
The temporary server, database, users, one-use keys, daemon state/socket and logs are private and removed.
Personal Tailscale and an existing Goose mesh are compared before/after; missing guards refuse the run.
The DERP map is served over loopback like the iOS fixture; its unavailable loopback relay proves no peer data transfer.

Run through node-dispatch on a host admitted by node-probe, with that host's bundle and guard socket:

```
python3 crates/leanzero-link/tests/headscale_cli_reauth.py \
  --headscale /tmp/headscale \
  --bundle-bin '/Applications/Goose Swarm.app/Contents/Resources/bin' \
  --personal-cli /Applications/Tailscale.app/Contents/MacOS/Tailscale \
  --guard-socket "$HOME/.leanzero/tailscale/tailscaled.sock" \
  --skip-reset-control --probe-resume-after-error
```

The production-shaped positive arm skips the preceding reset control entirely and uses a fresh key.
The optional reset control has a separate key; every phase records provider key usage.
`--probe-resume-after-error` requires the exact new provider key binding before testing normal key-free resume.
The script still exits nonzero for the original CLI error, even when the resume probe succeeds.
Raw Self.Online is recorded, not promoted to a success gate absent from normal MeshEngine join/resume.

## Measured 2026-10-27

Headscale 0.29.3; bundled Tailscale/tailscaled 1.98.8 (matching hashes on workmule and Gabee).
Clean positive arm: legacy node ID1, preauth1; new preauth3 was unused immediately before forced reauthentication.
The CLI returned exit1, `backend error: authkey already used`, while the provider changed preauth1→3.
Node ID, public node key, machine key, IPv4/IPv6 remained identical; exactly one usable provider node remained.
The new key became used. No reset control had run, so prior control-key reuse was ruled out.
After exact issued binding verification, normal key-free resume exited0 and retained that complete binding/Running state.
A separate raw-liveness probe observed Self.Online false around the ambiguity/resume; normal mesh success does not require it.
All completed fixture runs preserved personal and original Goose identity projections and stopped owned PIDs.

Primary receipts (local task evidence):
- `/tmp/goose-headscale-cli-reauth-final.log`: exact production90s timeout; clean failure.
- `/tmp/goose-headscale-cli-reauth-resume.log`: exact-binding/key-free resume success; original CLI failure retained.
- `/tmp/goose-headscale-cli-reauth-transition.log`: raw false Self.Online; older diagnostic rejected that optional field.
- `/tmp/goose-headscale-cli-reauth-final-raw.log`: later guard preflight refused before test startup because original socket stopped answering; not an application test result.

Rust repair stays in PreparedHost consumption: JoinFailed → Running/key → exact h1 issued-enrollment confirmation → normal resume → Running/samekey → existing activation/lease gates.
Confirmation errors use a distinct typed error preserving both causes, so outer enrollment-expired handling cannot remint a possibly spent key.
No worker contract, provider key policy, peer-admission predicate, MeshEngine success semantics, or original identity changed.
Rust execution and clippy are separate parent-coordinated offhost gates; this fixture is provider evidence, not full product proof.
