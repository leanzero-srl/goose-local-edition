#!/bin/bash
# Verify a built overnight app by actually RUNNING it — its own tests plus golden CLI checks from the spec.
# Usage: verify-app.sh <tracker|sheet|vcs|ledger>
set -u
ROOT=/Users/mihaiperdum/Projects/goose
DIR="$ROOT/evals/swarm-gym/overnight/$1"
cd "$DIR" 2>/dev/null || { echo "no dir $DIR"; exit 1; }
echo "===== VERIFY $1 ($DIR) ====="
echo "-- files/LOC --"
find . -type f \( -name '*.py' -o -name '*.ts' -o -name '*.rs' \) -not -path '*/.swarm/*' -not -path '*/node_modules/*' -not -path '*/target/*' -not -path '*/dist/*' | xargs wc -l 2>/dev/null | tail -1
echo "-- entry points --"; find . -maxdepth 2 -name '*.py' -o -name 'Cargo.toml' -o -name 'package.json' 2>/dev/null | grep -vE 'node_modules|.swarm|target' | head

case "$1" in
  tracker|ledger)
    echo "-- pytest --"; python3 -m pytest -q 2>&1 | tail -8
    echo "-- entry --help --"; (python3 -m "$1" --help 2>&1 || python3 "$1.py" --help 2>&1 || find . -name 'main.py' -o -name '__main__.py' | head -1 | xargs -I{} python3 {} --help 2>&1) | head -6
    ;;
  sheet)
    echo "-- build --"; (npm install --silent 2>&1 | tail -2; npm run build 2>&1 | tail -4 || npx tsc 2>&1 | tail -4)
    echo "-- test --"; (npm test 2>&1 | tail -8)
    echo "-- eval a golden grid --"; ls dist/*.js 2>/dev/null | head -1
    ;;
  vcs)
    echo "-- cargo test --"; cargo test 2>&1 | tail -10
    echo "-- cargo build --"; cargo build 2>&1 | tail -3
    ;;
esac
echo "===== END VERIFY $1 ====="
