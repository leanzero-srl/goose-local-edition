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
- `08-29 10:53` **ui** — static UX audit found: 7x duplicate paint of one long string, 2 of 86 controls with no accessible name (both real, unfixed)
- `08-29 11:15` **ui** — S1+S3 VERIFIED IN THE RUNNING APP: readSwarmRun across a live append gave events 3->4, thinkBytes 25->50, fullThinking accumulated BOTH chunks (no rolling window), generation stable at 1
- `08-29 11:35` **ui** — SCHEDULED behind the triage workflow: BenchmarkView.tsx:818 sets aria-invalid with no aria-describedby/aria-errormessage — the field turns red and says nothing
- `08-29 11:41` **backend** — NEVER git stash while a workflow is editing the tree — I stashed 40 in-flight files with the gate agent still running; popped in seconds but it could have corrupted the run. Use git show HEAD:file or a separate worktree for baselines.
- `08-29 11:45` **engine** — REVIEW FINDING: 4 superseded planner fns kept under cfg(test) 'because tests pin them' — defensible, but their tests now pin code that CANNOT run in production; they pass forever and guard nothing. Either mark them as historical or delete fn+test together.
- `08-29 11:45` **engine** — the diff's '-fn strip_integrate_verify_test_deps' etc were a MOVE not a deletion — all four still exist, now cfg(test)-gated. A -fn line in a diff is not evidence of removal.
- `08-29 11:59` **backend** — agenda reconciled: 106 -> 44 open. 33 DROPPED with evidence, 26 IMPLEMENTED matched by title, 2 verified by hand (thinking_bytes rendered; ENGINE_PHASE maps 11). The rest need per-item verification — title matching is not proof.
- `08-29 12:03` **backend** — TRAP CONFIRMED REAL: 'which goose' is 1.38.0 from June with NO swarm subcommand; the documented 'goose swarm verify' was unrunnable. Use ./target/release/goose (1.41.0).
- `08-29 12:55` **ui** — VERIFIED in the fresh app: aria-current works — 'no control marks the current view' is gone. It had reported for hours only because the running process was a 2h-old zombie the bundle check could not see.
- `08-29 12:55` **ui** — NEW on current code: 1 of 72 controls has NO accessible name; 1 disabled control gives no reason; duplicate paint of 2 long strings
- `08-29 13:17` **ui** — SCREENSHOT DEFECT: 'YOUR FLEET' badge drawn inside the bar at labelWidth+filled-8 overprinted the row label at low scores — at 1.6% the bar is a few px wide. Fixed: badge needs a bar wider than the badge.
- `08-29 13:17` **ui** — INSPECTOR: fixed 50/50 grid wasted half the modal on an empty OUTPUT pane through all of OPEN/RESEARCH; now single-column until Output has content. 'supervisor reading' -> 'being reviewed' with a tooltip; its comment claimed frozen counters which stopped being true today.
- `08-29 13:20` **ui** — THE REALTIME DEFECT, named by the tick: workhorse/slice-boot-wrapper digest ADVANCED, cell text did NOT. Cause: I applied lastSubstantiveLine to the TRANSCRIPT path only; the THINKING path still handed a 2400-char block to a one-line row. OPEN/RESEARCH are pure reasoning so every lane falls through to it.
