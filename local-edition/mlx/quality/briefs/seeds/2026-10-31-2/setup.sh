#!/bin/zsh
# Seed for brief 2026-10-31-2. Usage: setup.sh <WORK>. Copies the client's three files into <WORK>/from-client.
# How they were made (2026-10-31): audit CSV by a seeded python script (random.seed(31), 40 rows, 270 issues);
#   the DOCX by `textutil -convert docx` from HTML; the PDF by the bundled chrome-headless-shell --print-to-pdf.
#   The quote's total (7,965.00) is DELIBERATELY wrong: the lines sum to 7,765.00.
set -euo pipefail
WORK=${1:?usage: setup.sh <WORK>}; SEED=${0:A:h}
mkdir -p "$WORK"; [ -z "$(ls -A "$WORK")" ] || { echo "WORK not empty: $WORK" >&2; exit 1; }
mkdir -p "$WORK/from-client"
cp "$SEED/audit-2026-10-24.csv" "$SEED/accessibility-statement-draft.docx" "$SEED/agency-quote-PN-2026-118.pdf" "$WORK/from-client/"
ls -la "$WORK/from-client"
