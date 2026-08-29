# Tick notes — every finding, newest last

Appended by `loop-state/note.sh`. The tick prints only the newest three; this is the whole list.

- `08-29 09:52` **engine** — splice_briefs keyed a slice-less task by the empty string; REVIEW-added tasks produced a duplicate id and Dag::from_specs rejected the plan
- `08-29 09:52` **ui** — foldEvents' BUILD lane dropped 7 of 11 digest fields; it is the FIFTH lane path and it wins in laneSources, so all of BUILD had no thinking/transcript/judging
- `08-29 09:52` **engine** — REVIEW passed response:None, so wants_structured_reply was false: the terminator was unreachable and the lane had no final_output tool to call
