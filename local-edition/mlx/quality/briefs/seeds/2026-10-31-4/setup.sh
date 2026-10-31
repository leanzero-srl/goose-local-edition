#!/bin/zsh
# Seed for brief 2026-10-31-4. Usage: setup.sh <WORK>. Generates ~1 MB of data at setup (not committed):
# <WORK>/export/tickets-2026-10.csv (2,400 rows) and <WORK>/export/dockgw-2026-10.log (9,000 lines), byte-identical
# every run (random.Random(1031)); the grader numbers print to stdout and are copied in the brief's notes.expected_numbers.
set -euo pipefail
WORK=${1:?usage: setup.sh <WORK>}; SEED=${0:A:h}
mkdir -p "$WORK"; [ -z "$(ls -A "$WORK")" ] || { echo "WORK not empty: $WORK" >&2; exit 1; }
python3 -I "$SEED/gen_data.py" "$WORK/export"
