# Decisions only Mihai can take — the operator loop flags, never acts

## K1 — GOOSE_FAST_MODEL (fleet decision, prepared and waiting)

Every compaction and tool-pair summary currently runs as a FULL 27B generation on the worker's
own node — up to 10 serialized calls at the end of a long worker's turn, minutes of wall in no
budget, stealing a worker slot (measured; see TIERB-GAPS.md sibling reports). Core already
routes ALL of it through `complete_fast`, which honors `GOOSE_FAST_MODEL` — one env var
activates the fix. What only you can decide (per the never-reconfigure rule and the
27B-is-the-reference policy):

1. WHICH small model to load for auxiliary work (a 4B/8B instruct class; needs a fresh
   web-check at decision time, never from memory), and
2. WHERE it lives (a 4th alias on an existing node, or nowhere — declining is a valid answer;
   the swarm then keeps today's behavior, no change).

The engine-side plumbing is ready: the K7 guard (shipped) makes the fast path safe under
force-tool arms, and the prefill think-suppression is the known working mechanism for keeping
a Qwen-class fast model from reasoning on summaries. Say the word and the wiring lands in one
tick; until then nothing changes.
