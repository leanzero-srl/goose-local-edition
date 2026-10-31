#!/bin/zsh
# Seed for brief 2026-10-31-1. Usage: setup.sh <WORK>   (WORK = <round dir>/work, created if missing; must be empty)
# Copies three folders of LeanZero's open-source jira-issue-data-toolkit at a PINNED commit (never the live
# checkout), makes it a git repo with the import as commit 1, then commits a colleague's "refactor" that plants
# the bug (index and total part properties swapped in collectMigrationTags). The original repo is not touched.
set -euo pipefail
WORK=${1:?usage: setup.sh <WORK>}
SEED=${0:A:h}
SRC=$HOME/Projects/leanzero-jira-issue-data-toolkit
PIN=b5069106575e9187997a9887f0545dbd33d75b3c
mkdir -p "$WORK"; [ -z "$(ls -A "$WORK")" ] || { echo "WORK not empty: $WORK" >&2; exit 1; }
git -C "$SRC" archive "$PIN" mend_comments recover_truncated_content sync_issue_comments | tar -x -C "$WORK"
cd "$WORK"
git init -q -b main
git add -A
GIT_AUTHOR_DATE='2026-10-27T10:12:00+02:00' GIT_COMMITTER_DATE='2026-10-27T10:12:00+02:00' \
  git -c user.name='Ioana Marin' -c user.email='ioana@example.com' commit -qm 'Import comment sync tooling from the toolkit'
git apply "$SEED/colleague-refactor.patch"
git add -A
GIT_AUTHOR_DATE='2026-10-30T17:48:00+02:00' GIT_COMMITTER_DATE='2026-10-30T17:48:00+02:00' \
  git -c user.name='Radu Pop' -c user.email='radu@example.com' commit -qm 'Tidy collectMigrationTags: keyed lookup for migration props'
# Proof the seed is what the brief expects: the diff test fails, the splitter test passes.
if node sync_issue_comments/src/commentDiffer.test.js >/dev/null 2>&1; then echo "SEED BROKEN: commentDiffer test passes" >&2; exit 1; fi
node sync_issue_comments/src/adfSplitter.test.js >/dev/null
echo "seeded $WORK: $(git log --oneline | wc -l | tr -d ' ') commits, commentDiffer.test.js fails as planted"
