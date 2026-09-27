#!/usr/bin/env python3
"""i18n_resolve.py — resolve merge conflicts in ui/desktop/src/i18n/messages/*.json by KEY (three-way).

Every UI lane adds keys to all 16 catalogs, so parallel merges conflict in the same alphabetical spot
(2026-09-27: Q-185 × Q-192, then S1). Per conflicted catalog: start from ours, take every key theirs
added or changed versus the merge base, drop keys theirs deleted that ours left unchanged; write sorted
with the catalogs' own format (indent 2, sort_keys, trailing newline — checked to round-trip).
Run from the repo root during a conflicted merge; then `git add` the catalogs.
"""
import glob, json, subprocess, sys

def show(stage, path):
    return subprocess.run(['git', 'show', f':{stage}:{path}'], capture_output=True, text=True, check=True).stdout

def dump(d):
    return json.dumps(d, indent=2, ensure_ascii=False, sort_keys=True) + '\n'

done = 0
for p in sorted(glob.glob('ui/desktop/src/i18n/messages/*.json')):
    if '<<<<<<<' not in open(p).read():
        continue
    base, ours_txt, theirs = json.loads(show(1, p)), show(2, p), json.loads(show(3, p))
    ours = json.loads(ours_txt)
    if dump(ours) != ours_txt:
        sys.exit(f'refusing: {p} does not round-trip in the sorted format; resolve by hand')
    res = dict(ours)
    for k, v in theirs.items():
        if k not in base or base[k] != v:
            res[k] = v
    for k in base:
        if k not in theirs and ours.get(k) == base[k]:
            res.pop(k, None)
    open(p, 'w').write(dump(res))
    done += 1
print(f'{done} catalog(s) resolved by key')
