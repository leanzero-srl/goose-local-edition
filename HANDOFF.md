# HANDOFF: rel/3.0.130 (prepare only, do NOT release, do NOT push to main)

DROP THIS FILE (`git rm HANDOFF.md`) before the coordinator merges to main.

Worktree ~/Projects/goose-rel130 (Gabee), branch rel/3.0.130 (--no-track) from origin/main 1770006a8 (3.0.129).
Commits on top:
- merge feat/memfree (fast-forward to 122a76a41)
- 5e25d9e6a merge feat/job-strategies: ONE conflict, MacLoadPanel.tsx MacLoadBars tail: kept both
  (FreeMemoryEntry block, then cancelError + {dialog}). Strategies' HANDOFF.md was dropped from the merge.
- 0078d5234 D11/D12: RunJobDialog jobTargets() now returns every online Mac {mac,nodeId,takesJobs};
  jobs-off ones are disabled options with hint "jobs are off on this Mac" (runJob.jobsOff, 16 locales);
  canSend false for a disabled target. D12 root cause was lz/Combobox: non-free-text field showed the raw
  value; it now shows the selected option's label (free text unchanged). ComboboxOption.disabled added
  (aria-disabled, pick() refuses). Affects NodeModelCell too: its field now shows shortModelName (the label).
  Tests: Combobox.test.tsx +2, RunJobDialog.test.tsx +2 and option list updated.
- a6aed374b docs/releases/3.0.130-job-placement-queue-and-free-up-memory.md

## Gate (scratchpad gate.sh; log gate.log in this session's scratchpad)
Uses CARGO_TARGET_DIR=~/Projects/goose-strategies/target (reuse), strip=false config, heavy-run.
Done so far: fmt 0, clippy (4 crates, all-targets, -D warnings) 0, test -p leanzero-link 0,
goose --lib `acp::server::link link_serve swarm_router memfree` 0 (144 passed).
Pending at write time: goose-server remote_executor, goose-sidecar memfree, just check-acp-schema,
ui lint:check, typecheck, i18n:check, full vitest. Rerun command:
  zsh /private/tmp/claude-501/-Users-mihaiperdum-Projects/9235260e-4bbb-41c2-aefc-cd516bd51e28/scratchpad/gate.sh
Known: waitClock.test.ts may trip on git-ignored ui/desktop/src/swarm-bench/ (not tracked, not ours).

## Open
- Not pushed. Coordinator merges after the 3.0.129 live test (more fixes may join); version bump not done.
- Release notes "Verified" section lists the gate; update if a step fails.
