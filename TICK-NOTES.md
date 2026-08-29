# Tick notes — every finding, newest last

Appended by `loop-state/note.sh`. The tick prints only the newest three; this is the whole list.

- `08-29 09:52` **engine** — splice_briefs keyed a slice-less task by the empty string; REVIEW-added tasks produced a duplicate id and Dag::from_specs rejected the plan
- `08-29 09:52` **ui** — foldEvents' BUILD lane dropped 7 of 11 digest fields; it is the FIFTH lane path and it wins in laneSources, so all of BUILD had no thinking/transcript/judging
- `08-29 09:52` **engine** — REVIEW passed response:None, so wants_structured_reply was false: the terminator was unreachable and the lane had no final_output tool to call
- `08-29 10:08` **ui** — S1 caching on path+size let a REPLACED run log read as an append — old file's head spliced onto the new file's tail; identity must be inode+birthtime
- `08-29 10:08` **backend** — tick.py liveness used a run-dir NAME blacklist that was one marker behind twice (-ENDED-, then -STOPPED-); it now reads .swarm/heartbeat + pgrep
- `08-29 10:08` **backend** — tick lanes were silently suppressed by an age>=1800 filter — an EMPTY lane list reads as 'no nodes working'; it now says how many were hidden
- `08-29 10:15` **engine** — REVIEW de-dup keyed on a 120-char lowercase PREFIX, so any rewording read as a new finding; now keys on (kind, identifiers) with basename normalisation
