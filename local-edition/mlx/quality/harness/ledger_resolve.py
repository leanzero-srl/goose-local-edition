#!/usr/bin/env python3
"""ledger_resolve.py — resolve git conflict blocks in FINDINGS-LEDGER.md after merging an agent branch.

Every agent edits its own row's status while main appends new rows, so almost every merge conflicts in the
ledger (2026-09-26: three merges in a row). Per conflict block: one row per id; for an id on both sides the
row whose status is further along wins (fixed/shipped/proven > framed/cutting > the rest), ties go to the
incoming branch (the agent that just worked the row); rows sorted by id. BUT a side whose row still equals the
merge BASE (git stage :1) did not touch it, so the side that changed it wins whatever its status reads — the
rank alone once kept main's 'framed' over a lane's 'model behaviour — no goose cause' (Q-368, twice on
2026-09-28), because a free-text verdict ranks lowest. Exact duplicate rows outside the
blocks are dropped too. Refuses (exit 2) on a block that holds a non-row line, rather than guessing.
usage: ledger_resolve.py [path]   (default local-edition/mlx/quality/FINDINGS-LEDGER.md)
"""
import subprocess
import sys

path = sys.argv[1] if len(sys.argv) > 1 else 'local-edition/mlx/quality/FINDINGS-LEDGER.md'
text = open(path).read()


def rank(status):
    s = status.strip().lower()
    if s.startswith(('proven', 'shipped', 'fixed', 'refuted', 'dropped')):
        return 3
    if s.startswith(('framed', 'cutting', 'scheduled', 'parked')):
        return 2
    return 1


def row_id(line):
    cells = line.split('|')
    return cells[1].strip() if len(cells) > 9 else None


def id_key(i):
    try:
        return (0, int(i.split('-')[1]))
    except (IndexError, ValueError):
        return (1, i)


base_rows = {}
try:
    base_text = subprocess.run(['git', 'show', f':1:./{path}'], capture_output=True, text=True, check=True).stdout
    for line in base_text.splitlines():
        cells = line.split('|')
        if len(cells) > 9:
            base_rows[cells[1].strip()] = line
except (subprocess.CalledProcessError, FileNotFoundError):
    print('no merge base stage (:1) — resolving by status rank only', file=sys.stderr)

blocks = 0
while '<<<<<<< ' in text:
    a = text.index('<<<<<<< ')
    body_start = text.index('\n', a) + 1
    mid = text.index('\n=======\n', body_start - 1) + 1
    end = text.index('>>>>>>> ', mid)
    end_line = text.index('\n', end) + 1
    ours = text[body_start:mid].splitlines()
    theirs = text[mid + len('=======\n'):end].splitlines()
    best, order = {}, []
    for side, lines in (('ours', ours), ('theirs', theirs)):
        for line in lines:
            if not line.strip():
                continue
            i = row_id(line)
            if not i:
                sys.exit(f'refusing: a non-row line in a conflict block: {line[:120]!r}')
            status = line.split('|')[8]
            if i not in best:
                order.append(i)
                best[i] = line
            else:
                prev = best[i].split('|')[8]
                base = base_rows.get(i)
                if base is not None and best[i] == base and line != base:
                    best[i] = line
                elif base is not None and line == base and best[i] != base:
                    pass
                elif rank(status) > rank(prev) or (rank(status) == rank(prev) and side == 'theirs'):
                    best[i] = line
    order.sort(key=id_key)
    text = text[:a] + ''.join(best[i] + '\n' for i in order) + text[end_line:]
    blocks += 1

out, seen, dropped = [], set(), 0
for line in text.splitlines(keepends=True):
    if row_id(line) and line in seen:
        dropped += 1
        continue
    seen.add(line)
    out.append(line)
open(path, 'w').write(''.join(out))
print(f'{blocks} conflict block(s) resolved, {dropped} exact duplicate row(s) dropped')
