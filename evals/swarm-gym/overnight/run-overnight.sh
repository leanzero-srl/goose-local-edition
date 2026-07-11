#!/bin/bash
# Overnight exploratory: build COMPLEX apps (the vetted archetypes + a ledger) one at a time on the warm
# fleet, using the freshly-built binary that captures real reasoning + tool-result output. Each run creates
# a session that shows up in the desktop app, and builds its app under overnight/<name>/. Sequential (the
# 3-node fleet runs one build at a time); no backgrounded children (respecting the no-nested-& rule).
set -u
ROOT=/Users/mihaiperdum/Projects/goose
BIN="$ROOT/target/debug/goose"
OVN="$ROOT/evals/swarm-gym/overnight"
LOG="$OVN/progress.log"

export LMSTUDIO_HOST=http://localhost:1234
export LMSTUDIO_API_KEY=lm-studio
export CONTEXT7_API_KEY=ctx7sk-9639db77-28c1-44b5-b567-527a4d3895ed
export GOOSE_SWARM_SMOKE=1
export GOOSE_SWARM_SPLIT=1
export GOOSE_SWARM_PREREVIEW=1

echo "=== OVERNIGHT RUN START $(date) — binary: $BIN ===" >> "$LOG"
for app in tracker sheet vcs ledger; do
  spec=$(cat "$OVN/specs/$app.txt")
  dir="$OVN/$app"
  rm -rf "$dir"
  mkdir -p "$dir"
  {
    echo ""
    echo "=== [$app] START $(date) in $dir ==="
  } >> "$LOG"
  cd "$dir" || continue
  "$BIN" swarm run "$spec" --output-format json >> "$LOG" 2>&1
  code=$?
  loc=$(find "$dir" -type f \( -name '*.py' -o -name '*.ts' -o -name '*.rs' \) \
        -not -path '*/.swarm/*' -not -path '*/node_modules/*' -not -path '*/target/*' -not -path '*/dist/*' 2>/dev/null \
        | xargs wc -l 2>/dev/null | tail -1)
  files=$(find "$dir" -type f -not -path '*/.swarm/*' -not -path '*/node_modules/*' -not -path '*/target/*' 2>/dev/null | wc -l | tr -d ' ')
  echo "=== [$app] END $(date) exit=$code files=$files loc:[$loc] ===" >> "$LOG"
done
{
  echo ""
  echo "=== OVERNIGHT RUN COMPLETE $(date) ==="
} >> "$LOG"
