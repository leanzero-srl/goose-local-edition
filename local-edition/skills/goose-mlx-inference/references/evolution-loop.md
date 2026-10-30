# Evolution loop, fan protocol, one home

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## The evolution loop (this is the durable memory — update as you go)
1. Every experiment/change lands as one row in `local-edition/mlx/experiments.jsonl`
   (`void_reason` string when not a pass, never a bare fail) AND a dated entry in `LEDGER.md`
   (newest first: Did / Learned-and-it-changed-the-design / gate defects found).
2. A loss becomes a RULE change: new gate in `gates.py` (with BLOCK+ALLOW self-test in
   `gates_selftest.py` — `python3 gates_selftest.py` must pass) or a line here. Never note-and-ignore.
3. Findings triage ON ARRIVAL into `QUEUED-FIXES.md`: IMPLEMENT / DROP / SCHEDULED(condition).
   Report the ratio, not the count.
4. Solutions evaluated-but-unpicked go to `TRUMP-CARDS.md` with the ONE idea worth stealing.
   Improvement hunts fan ONE agent per card — brief = repo + idea + our exact pain point, nothing else.
5. `NOW.md` changes in the SAME commit as any thread change. After a compaction: read NOW.md first,
   then LEDGER.md head, then `git log --oneline -10`; never resume from a summary.

## Fan protocol (Claude subagents on this workstream)
Fan → reduce in code → adversarially verify → synthesize. A brief carries ONLY: goal, exact files,
constraints, verification. No campaign history — sharp and fast beats informed and diluted.

## One home, and the merged references (2026-10-03)
- THIS repo file is the only goose-mlx-inference skill. `~/.claude/skills/goose-mlx-inference` is a SYMLINK to
  `~/Projects/goose/local-edition/skills/goose-mlx-inference` on both Macs (projsync skips symlinked skills; git
  syncs this file). Until 2026-10-03 a second, diverged copy lived at that global path (workhorse-owned); its
  unique sections are in `references/remote-single-and-engine-fixes.md` (Remote single over Link, the hidden
  generation caps, Q-85, Q-142, Q-508) — read it before touching remote single or the split's cache.
- `references/distributed-engine-history.md` — the dated 2026-09-23..26 record of the distributed engine (rank
  launcher, Link control plane, Local Network privacy, Flash/pipeline server, image input, compaction, Q-65,
  Q-66/71, Q-75, Q-114), moved verbatim from the `mlx-jaccl-cluster` skill, which now keeps only the raw two-Mac
  MLX/JACCL layer. `scripts/link-live-test.sh` + `scripts/tailscale_identity.py` moved with it.
- `references/prompt-lookup-ngram.md` — the per-model prompt-lookup (n-gram) lever: lanes (inside MTP / suffix / unavailable on hybrids), the 2026-10-26 numbers (27B Q8 code edit 42.8 -> 134.8 tok/s, 99.3% accepted; summarise/chat no gain), the streaming space-drop bug and why Flash-Next gets no MTP.
- `references/qwen4-flash-offload.md` — Flash-Next n-gram (PLE) table on disk: tag v0.14.3-lz.14-ple.1 `--qwen4-ple-offload`, 104 -> 72 GB at ~equal decode; goose lever + auto rule (70% RAM budget); expert offload STOPPED by the owner; laptop test blocked on disk space.
- Edit this file from either Mac and COMMIT it in the goose repo; an uncommitted edit only reaches the other Mac
  as a projsync wip snapshot.
