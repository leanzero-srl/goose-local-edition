# Corrected MacBook-to-MacBook active steering

28 October 2026, 23:26 UTC. API control behavior is proved on both installed corrected bundles (source bf5966b19fdf, signed CLI349b242b64a4a48072fbbfcecda0c9a9a2c32fef5639ae6c045003b34d7bee6f). Visible UI confirmation remains outstanding because the Mac is locked. Neither Studio was touched.

## Cloud-only configuration

The original disposable laptop session20261028_1 was changed through supported session/set_config_option to provider openrouter, model deepseek/deepseek-v4.1-flash. The earlier explicit swarm cloud node pinned the main reply but could still append MLX candidates for background helpers. Direct OpenRouter avoids that fallback for this test session. No global defaults or other sessions changed, and no credentials were copied. The existing credential was reused. Process/config endpoint reads and provider source establish the official https://openrouter.ai default; live controller sharing reports openrouter but does not report an origin, which was not invented.

Both settings calls timed out while observing their replies. Neither was repeated. Actual persisted effects and the shared Agent update path were independently reconciled; the original nine messages, title and global config hash remained equal. Private laptop receipt: ~/.local/share/goose-upgrade-snapshots/cloud-controller-session-20261028_1/direct-openrouter-pin.json. This is verified effect, not successful RPC acknowledgement or a claim of visible UI selection.

## One start and one steer

Gabriela’s corrected desktop controller facade sent one durable idle Send, command a05418b5-7e7f-43aa-b987-b7b6a0ce160d at revision73623. It created run af7248e1-1d3d-4fd6-bd43-300f73440005 on the original laptop. The prompt requested300 numbered cloud sentences and no tools/files/local models. Readiness first reported no real output, then observed actual assistant partials on the same run and live direct OpenRouter.

Exactly one steer followed:28402eaa-8a91-4ce3-b01b-d213a4f767e9 at snapshot73723. Its snapshot contains real assistant words ending during line98. The command reached delivered with the same authenticated controller device, work, run and exact persisted message ID. History contains the interrupted list ending midline102 at73731, one matching steering user message at73733, and a later assistant message at73741. The owner returned idle/no run at73746.

Actual assistant response: “Active remote steering confirmed. Do not use tools, change files or load local models.”

The literal expected-response assertion remains FALSE: it expected only “Active remote steering confirmed.” The prompt used an unquoted “reply exactly:” followed by the prohibition, and the model repeated that trailing text. No assertion was rewritten and no model call was repeated to manufacture a passing result. The independent reviewer read both raw private command records and confirmed actual interruption, single delivery and instruction consumption; this trace supplies no evidence of a controller defect. API behavior is proved; exact text and visible UI are separate, unpassed checks.

Private Gabee records remain mode0600 under ~/.local/share/goose-upgrade-snapshots/controller-134-20261028T211658Z/: cloud-active-start-final-command.json and cloud-active-steer-final-command.json. They retain original commands, snapshots, receipts, histories and the failed literal assertion. Earlier rejected command6d1f0c2d… remains a failure and was not retried. No new inference is needed merely to repeat this proof. When the owner unlocks the Mac, inspect the actual Cloud controller test conversation and these resulting messages.
