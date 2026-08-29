@AGENTS.md

## Durable context in this repo — read the one that matches what you are doing

Compaction destroys conversation knowledge, so it lives in files instead. Path-scoped rules under
`.claude/rules/` load automatically when you touch a matching file; these four you should open yourself:

- **`NOW.md`** — the current thread. Read this BEFORE `SWARM-AGENDA.md`, which is 2,400 lines of history.
- **`EXPERIMENTS-LEDGER.md`** — what was tried, what it measured, why it is not coming back. **Read before
  proposing an engine change**: several ideas here have been tried twice because the first failure lived
  only in a compacted conversation.
- **`RUN-LEDGER.md`** — one row per run, in comparable numbers, so runs are judged by measurement rather
  than recollection.
- **`TICK-NOTES.md`** — every finding, newest last.

