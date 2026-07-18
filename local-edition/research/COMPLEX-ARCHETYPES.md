> **HARD DIRECTIVE (Mihai 2026-07-18): NEVER put a line-count / LOC target in a generated spec or any prompt fed to goose.** Pinning the model to a LOC number stifles it and makes it dumber — let it produce whatever size the feature set genuinely needs. Describe FEATURES, BEHAVIOR, and acceptance criteria; NEVER a line count. (LOC may be MEASURED afterward as an observation, never IMPOSED as a target.)

# Complex archetypes — the new standing bar (2026-06-30)

The user raised the bar: the prior eval apps (70-450 LOC CLI tools) are "literally nothing — a couple of
methods." From here on, eval apps must be genuinely COMPLEX + FEATURE-DENSE (many modules,
rich command surfaces). Three diverse archetypes (data-app / algorithmic-engine / systems-tool), three
languages (Python / TypeScript / Rust), all VERIFIABLE by golden output. Run ONE AT A TIME (3-node fleet),
study each against all 7 points + measure ACTUAL scope/complexity + RUN end-to-end with golden values + watch for a NEW
failure mode at scale (cross-module integration, partial completion, time blowup) distinct from the
recursive-depth ceiling. Expect 60-120+ min each — that is acceptable for this complexity (the 15-25 target
was for the small tools; record the real time).

## ARCHETYPE A — DATA APP AT SCALE (Python + SQLite, broad+conventional)
LANG=Python — a CLI issue tracker "tracker" backed by SQLite. ENTITIES: projects and issues. An issue has:
title, description, status (open/in-progress/review/done), priority (low/medium/high/critical), assignee,
labels (zero or more), a project, created/updated timestamps, and dependencies (an issue can be blocked-by
other issues). COMMANDS: 'project add NAME', 'project list'; 'issue add --project P --title T [--desc D
--priority --assignee --label ...]' (prints the new id), 'issue show ID', 'issue set ID
--status/--priority/--assignee/--label/--unlabel', 'issue close ID'; 'issue list' with FILTERS --project
--status --priority --assignee --label and --sort by priority|updated; 'issue search TEXT' (matches title or
description); dependency: 'issue block ID --by OTHER' and 'issue unblock ID --by OTHER'; 'ready' (issues whose
every blocker is done and that are not themselves done); 'blocked' (issues with at least one non-done
blocker); a transition RULE: setting status to done / closing is REJECTED (non-zero exit) if the issue still
has a non-done blocker; REPORTS: 'report status' (count per status), 'report assignee', 'report priority';
'export CSV_PATH' and 'import CSV_PATH'. Validate all inputs and exit non-zero with a message on bad input.
Persist to a SQLite file (path via --db or a default in the cwd).
VERIFY: add a project + several issues with deps; check 'ready'/'blocked' are correct; check a done-with-open-
blocker is rejected (non-zero); check the report counts; export then re-import round-trips.

## ARCHETYPE B — ALGORITHMIC ENGINE (TypeScript, deep+structured)
LANG=TypeScript, built with tsc to a runnable dist entry. A spreadsheet calculation engine "sheet". A sheet is
a grid of cells (columns A,B,C...; rows 1,2,...) loaded from a JSON file mapping cell -> value, where a value
is a number, a string, or a FORMULA starting with '='. FORMULAS support: + - * / ^ with precedence and
parentheses; cell references (A1) and ranges (A1:A5); and functions SUM, AVG, MIN, MAX, COUNT, IF(cond,a,b),
AND, OR, NOT, ABS, ROUND(x,n), CONCAT. Implement a real TOKENIZER + recursive-descent PARSER (do NOT use
eval), a DEPENDENCY GRAPH with topological recalculation, and CYCLE DETECTION. Error values: divide-by-zero
is #DIV/0, an unknown function is #NAME, a reference to an empty/invalid cell in arithmetic is #REF, and a
cell in a dependency cycle is #CYCLE. COMMANDS: 'eval FILE' (print every non-empty cell and its computed
value), 'get FILE CELL', 'deps FILE CELL' (the cells CELL transitively depends on), 'set FILE CELL VALUE'
(update the file, recalc, print the changed cells). Validate inputs; exit non-zero on a malformed file or bad
cell reference.
VERIFY: a grid with =A1+B2, =SUM(A1:A3), =IF(...), and a deliberate cycle; check the computed values + the
#CYCLE/#DIV0 errors. (This is the DEEP archetype where the recursive ceiling lives —
tests whether clear module boundaries make depth tractable, or whether it still fails like APP6/APP8.)

## ARCHETYPE C — SYSTEMS TOOL (Rust, different paradigm)
LANG=Rust, a CLI content-addressable version store "vcs" (no networking), built with cargo. COMMANDS: 'init'
(create a .vcs/ store in the cwd); 'add FILE...' (stage files — hash each file's content with SHA-256 and
store it as a blob object named by its hash under .vcs/objects/); 'commit -m MSG' (snapshot the staged files
into a tree object, create a commit object with the tree hash, the parent commit if any, the message, and a
timestamp; print the new commit hash; advance HEAD); 'log' (walk from HEAD through parents, printing each
commit hash, message, timestamp); 'cat-file HASH' (print a stored object's content); 'ls-files' (list files
in the current commit's tree); 'checkout HASH' (restore the working files from that commit's tree); 'status'
(files staged vs in HEAD: added/modified/unchanged); 'diff HASH1 HASH2' (files added/removed/modified between
two commits); 'tag NAME HASH' and 'tags'. Objects are content-addressed by SHA-256 under .vcs/objects/.
Validate inputs and exit non-zero on a bad hash or missing object.
VERIFY: init; add 2 files + commit; modify one + commit; log shows 2 commits; cat-file a blob hash; checkout
the first commit restores the old content; diff the two commits shows the modified file.
(Different paradigm — content hashing + a commit DAG + persistence — tests the Rust path at real scale.)

## What to measure (the user wants to understand the wall)
Per app: ACTUAL LOC (src + tests, excl node_modules/.swarm/dist), # modules, the 7 points, RUN end-to-end with
the golden checks above (REAL process exit), TIME, and especially: did coherence hold across many modules at
this size, or did a NEW failure mode appear (a module integrated against a wrong sibling contract, a feature
silently dropped, partial completion, a 2-hour time blowup)? That integration-at-scale failure mode — if it
appears — is the real wall beyond the recursive-depth ceiling, and the next improvement target.
