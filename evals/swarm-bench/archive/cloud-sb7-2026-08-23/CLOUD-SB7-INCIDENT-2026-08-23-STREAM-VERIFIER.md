# Cloud SB7 smoke verifier incident — 2026-08-23

## Scope and disposition

Campaign `cloud-sb7-20260823-0a905-s1` made one smoke episode for each of the five frozen entrants. No full build, score, or publication began. The budget ledger settled eight requests, retained no outstanding reservations, and recorded an upper-bound spend of `$0.034278575` (`$0.03080575` Google, `$0.003472825` DeepSeek, `$0` Z.AI).

The two Gemini and two DeepSeek attempts completed the frozen contract. Their `FAILED` states are verifier defects, not provider or model failures. GLM-5.3 is different: its request was queued but never admitted, and the provider returned insufficient balance before any tool work.

No failed attempt is to be overwritten or rerun merely to make the gate green. Any recovery must keep the raw logs and their hashes immutable, carry the cumulative budget ledger, and bind an additive adjudication to the exact old evidence.

## Four false failures

The immutable logs prove one exact shell request, the frozen command bytes, one paired successful response, the exact nonce file/hash, the exact final marker (including valid stream fragmentation), two admitted and two terminal provider requests, zero outstanding reservations, process exit zero, and one final `complete` event.

The verifier nevertheless rejected them for four independent reasons:

1. `goose run --output-format stream-json` emits a three-line session banner unless `--quiet` is set. The JSON parser correctly rejected arbitrary non-JSON, but the launcher had failed to request the promised JSON-only stream.
2. Stream serialization emits the built-in tool as `name: "shell"` plus `_meta.goose_extension: "developer"`. The verifier only recognized the internal registry name `developer__shell`.
3. The shell result's display text combines stderr and stdout. Apple's sandboxed `/usr/bin/python3` shim emitted `xcodebuild` warnings on stderr, while `structuredContent.stdout` was the exact nonce marker and `structuredContent.exit_code` was zero. The verifier inspected the display text instead of the structured result.
4. `process_group_members()` launched `ps` in the process group it was measuring. The inspector therefore observed itself, making `stop_group_members()` incapable of proving an empty group.

Immutable stream/evidence hashes:

- `gemini-3.7-flash`: stream `cae169120941a8c4bd69df99628bd03f901930793a30f5bc73bbcae2e66ea5aa`; evidence `a0a8102c724374f8d305f5fedccfffc5ad0e86373660224cdb13caeb5730969d`
- `gemini-3.1-pro-preview`: stream `0737b682d93e21a915591b1306dac81516f4dc6ca250e855516a51db25e9e9e9`; evidence `0dbecb30c0b9128172d533a29bb5c0ecc5529a8894bfe17b5bde5dae19ef4f21`
- `deepseek-v4-flash`: stream `2d0081941a173a68b987a8656d08e1503fed44cd8e99ded14c79a5e10f6bb3a5`; evidence `fa05856694ffe8c777b6ea336b0f0b3eda2cbb2a32cb5ce04b8177471c3e64e3`
- `deepseek-v4-pro`: stream `21909bc9a09cf091348fcf797fa8e1a5cb105c6640458b0f8c4d96f1071b7ba4`; evidence `9de178e1f1f6b7c6fe7f8f46579cace91ca5113ec87e28a46976b466dc5004ed`

## GLM-5.3 endpoint defect

The GLM stream hash is `1dbbfaf0d48f0241dee5f2296bfca3ad99649568a765a215fd9f30440c7276be`; its sealed evidence hash is `bba747f3baecda62e79868bdf550b03b06fa572584e47bfddcea4d701a86a69a`. Its lifecycle contains only `queued,error`, with no admission, terminal usage, settlement, nonce, or spend.

The manifest selected Z.AI's general endpoint. Current Z.AI documentation says Coding Plan keys must instead use `https://api.z.ai/api/coding/paas/v4`, and explicitly attributes error 1113 / insufficient balance after buying a Coding Plan to use of the wrong base URL. The same key's authenticated `/models` roster returns HTTP 200 and includes exact `glm-5.3` on both endpoints; roster visibility therefore cannot prove chat-completion entitlement.

Primary references:

- <https://docs.z.ai/guides/overview/quick-start>
- <https://docs.z.ai/devpack/tool/others>
- <https://docs.z.ai/devpack/faq>
- <https://z.ai/blog/glm-5.3>

## Source correction

Future launchers use `--quiet`; normalize the built-in shell identity only when its extension metadata proves `developer`; require exact structured stdout and integer exit code zero; run process-group inspection in a new session; and inject provider base URLs through a validated, non-secret manifest field. The GLM entrant now selects the dedicated Coding Plan endpoint. The scorer, fixtures, task, thresholds, expected checks, and publication semantics remain unchanged.
