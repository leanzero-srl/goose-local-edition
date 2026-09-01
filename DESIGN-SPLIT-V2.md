# THE SPLIT v2 — deterministic assembly, verified shards, one writer per shared state

Mihai, 2026-09-02 00:1x: *"research the split. Very important to get this right. It's a bet but it is the crux of a
swarm."* This is the research's verdict mapped onto the engine, ranked by how confident the evidence makes us (never by
size). v1 (2c S1–S6, 30b1c4fb2) stands: fatness measured, N shards in `.swarm/shards/<module>/<shard>/` with a README
(PROVIDES / ASSUMES / UNFINISHED / CHECKED_WITH), a merger fed a code dossier, gaps through the scheduler door. What
changes is WHO does the mechanical work and WHAT is checked before the merge.

## What the evidence says (sources in the research note, TICK-NOTES 09-02 00:1x)

- Declare the contract before anyone writes; a NEUTRAL party applies the writes (ATM, arXiv 2607.00041; SpecDB,
  arXiv 2605.31097 — 23,779 lines of Rust passing TPC-C, generation ordered by a dependency graph with `reference` and
  symmetric `cooperate` edges for modules that "must be co-designed").
- Per-unit tests from the decomposition (Parsel, arXiv 2212.10561: HumanEval 67→85 by testing minimal groups of
  implementations against constraints).
- Parallel writers make conflicting implicit decisions and the merger inherits them (Cognition, "Don't build
  multi-agents"); coding has fewer truly parallel tasks than research and every subtask needs an objective, an output
  format and clear boundaries (Anthropic, multi-agent research system); "each teammate owns a different set of files…
  three focused teammates often outperform five scattered ones" (Claude Code agent teams).
- Merge at the DECLARATION level, text inside bodies, and CHECK the result — an incorrect merge "still produces a single
  merged program version without conflicts" (Mergiraf semistructured merge, arXiv 2608.11345; ASE 2024 merge-tool
  evaluation: Spork is best only if a wrong merge costs no more than an unhandled one).

## The mechanisms, ranked by confidence

1. **HIGH — ASSEMBLE, then glue.** A shard owns WHOLE top-level declarations. Code concatenates the pieces in the declared
   interface's order (commutative declarations; a duplicate signature is a real conflict → `merge_duplicate_definition`);
   the merger model writes ONLY the glue: imports, shared-state initialisation, wiring, UNFINISHED fills. Event
   `merge_assembled{module, pieces, order, glue_lines}`; `check_merge` runs unchanged after. Retyping 500 lines at
   19 tok/s (~30 min) is where `merge_piece_dropped` was manufactured.
2. **HIGH — verify a shard at completion, against stubs of its ASSUMES.** Parse every piece; scan free identifiers;
   names not defined in the folder, not in the declared interface, not language globals → `shard_undefined_ref{shard,
   names}` (MILD: feeds the dossier and the merger's gap list, never a retry). JS: `node --check` + the free-identifier
   scan; Python: `py_compile` + the scan; other languages: "unchecked" said, never green.
3. **HIGH — PROVIDES must be backed.** `shard_provides_unbacked{shard, names}` when a README's PROVIDES has no parsed
   DEFINITION in the folder (definitions, not mentions — the rule `check_merge` already uses). The shard stays Done; the
   merger's brief lists the name as a GAP.
4. **MEDIUM — one WRITER per shared state.** The split declaration names, for every shared state (e.g. `instanceData`),
   its SHAPE (fields, types, stride/offsets) and the single shard that writes it; readers declare the shape they read.
   Two writers = a `cooperate` edge = one shard, not two. Code checks the READMEs: `shard_shared_state_writers{state,
   shards}` when more than one PROVIDES a write.
5. **MEDIUM — measure interface fidelity before moving it.** `interface_leak{shard, assumption}` for every README ASSUMES
   not covered by the declared interface, and `merge_gap` items that were already UNFINISHED in a README (predictable)
   vs discovered at merge. Only when leaks are near zero does the declaration move into synthesis's own call.
6. **MEDIUM — size by the fleet, not by 8.** Shard count derives from the free hosts at split time and the declared
   responsibility clusters (a derivation, never a literal): three focused shards on three nodes beat eight queued.

## What r6g's first split exercise measures (the vigil reads these)

- Piece survival: `merge_piece_dropped` + `merge_signature_mismatch` counts — any drop under retype is a class assembly
  wins by construction.
- Retype ratio: merged-file lines ÷ summed piece lines, and merger emit minutes vs dossier-read minutes (≈1.0 and
  emit-dominated = paying a 27B to copy).
- Interface leak: README ASSUMES not in the declared interface; gaps that were UNFINISHED before the merger started.

## Gates this design lives under

No caps, clocks or counts (gates 1/5) — every terminator is a check on the tree; MILD — events, never aborts (Mihai
2026-08-29); general — the assembly and the free-identifier scan are per language, "unchecked" is said for the rest,
never faked green (VA-060); one door — assembled files walk `check_merge` and promotion like every other final file.
