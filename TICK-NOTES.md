# Tick notes — every finding, newest last

Appended by `loop-state/note.sh`. The tick prints only the newest three; this is the whole list.

- `08-29 09:52` **engine** — splice_briefs keyed a slice-less task by the empty string; REVIEW-added tasks produced a duplicate id and Dag::from_specs rejected the plan
- `08-29 09:52` **ui** — foldEvents' BUILD lane dropped 7 of 11 digest fields; it is the FIFTH lane path and it wins in laneSources, so all of BUILD had no thinking/transcript/judging
- `08-29 09:52` **engine** — REVIEW passed response:None, so wants_structured_reply was false: the terminator was unreachable and the lane had no final_output tool to call
- `08-29 10:08` **ui** — S1 caching on path+size let a REPLACED run log read as an append — old file's head spliced onto the new file's tail; identity must be inode+birthtime
- `08-29 10:08` **backend** — tick.py liveness used a run-dir NAME blacklist that was one marker behind twice (-ENDED-, then -STOPPED-); it now reads .swarm/heartbeat + pgrep
- `08-29 10:08` **backend** — tick lanes were silently suppressed by an age>=1800 filter — an EMPTY lane list reads as 'no nodes working'; it now says how many were hidden
- `08-29 10:15` **engine** — REVIEW de-dup keyed on a 120-char lowercase PREFIX, so any rewording read as a new finding; now keys on (kind, identifiers) with basename normalisation
- `08-29 10:18` **engine** — worker_timeout_secs:420 and planner_timeout_secs:900 are still in config.yaml but BOTH are dead — they reach run_agent_in as idle_secs which is ignored; now guarded by effective_idle_budget + a test so re-arming fails the build
- `08-29 10:25` **ui** — S1 now replayed against a REAL archived run (its run.jsonl grown in 25 appends, every transcript in 15) — synthetic tests alone only prove self-consistency
- `08-29 10:36` **backend** — disk hit 16GB free of 926; target/debug was 27GB and target/release only 9GB — removing debug alone reclaimed to 42GB and kept the release build incremental
- `08-29 10:36` **ui** — /Applications/Goose.app/Contents/MacOS/ was EMPTY — the installed app had no binary at all, which is why no UI assessment could ever attach
- `08-29 10:51` **ui** — the active nav row was styled-only — bg-background-tertiary vs transparent, ZERO of 19 controls carried aria-current/aria-selected; found by probing the RUNNING app over CDP, not by reading code
