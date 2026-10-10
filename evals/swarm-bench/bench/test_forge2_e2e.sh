#!/bin/zsh
# Forge 2.0's empty-entrant end-to-end check (forge2/SPEC.md §5): run_build --forge2 launched the way the desktop
# launches it, on the app's bundled runtime, with a fake goose whose `run` exits at once, so the harness scores the
# untouched v1 starter. Modelled on ~/goose-builds/loop-state/e2e/run_e2e.sh: `benchmark-config` goes to the
# installed engine (the OpenRouter provider snapshot), so it needs the installed Goose Swarm app and the network
# for the model listing. Heavy (dev site, scoring site, probe, Chromium): never while a paid run is scoring.
#
#   zsh evals/swarm-bench/bench/test_forge2_e2e.sh <out dir> [port]      port defaults to 8851 (the app uses 8850)
#
# Passes when run_build exits 0 (never REFUSED) and its verdict is a forge-2.0 score with no harness gap, the
# tier's call budget recorded, every public file rendered as published, the forge2 kit graded, and the v1
# starter LOW (≤ 0.30).
set -euo pipefail
OUT=${1:?usage: test_forge2_e2e.sh <out dir> [port]}
PORT=${2:-8851}
SB=${0:A:h:h}
APP="/Applications/Goose Swarm.app/Contents/Resources"
RT="$HOME/Library/Application Support/Goose/benchmark/runtime"
BM="$APP/bundled-mcps"
PY="$RT/python/bin/python3"
mkdir -p "$OUT"
OUT=${OUT:A}
FAKE="$OUT/fakegoose"
print -r -- "#!/bin/zsh
if [[ \"\$1\" == benchmark-config ]]; then exec \"$APP/bin/goose\" \"\$@\"; fi
exit 0" > "$FAKE"
chmod +x "$FAKE"
export GOOSE_SWARM_RENDER_NODE="$RT/node/bin/node" GOOSE_SWARM_PLAYWRIGHT_MODULE="$BM/leanzero-web-search/node_modules/playwright"
export GOOSE_SWARM_CHROMIUM_EXECUTABLE="$BM/$("$PY" -c 'import json,sys; print(json.load(open(sys.argv[1]))["executable"])' "$BM/browser.json")"
export BENCH_BROWSER_MODULE="$GOOSE_SWARM_PLAYWRIGHT_MODULE" BENCH_BROWSER_EXECUTABLE="$GOOSE_SWARM_CHROMIUM_EXECUTABLE"
export BENCH_FFMPEG="$RT/ffmpeg/ffmpeg" BENCH_FFPROBE="$RT/ffprobe/ffprobe" PATH="$RT/node/bin:$PATH"
export BENCH_GOOSE="$FAKE" GOOSE_SWARM_BENCHMARK=1 BENCH_PRODUCT=1
export BENCH_SPEC="$SB/forge2/public/spec-build-forge2.md" GOOSE_SWARM_RENDER_PROBE="$SB/bench/forge2_probe.mjs"
cd "$SB"
"$PY" -B -u bench/run_build.py --entrant openrouter-e2e-fake --only-rep 0 --timeout 0 --out "$OUT" \
  --provider openrouter --model deepseek/deepseek-v4.1-flash --forge2 --port "$PORT"
"$PY" -B - "$OUT/openrouter-e2e-fake-r0" "$PORT" <<'EOF'
import json
from pathlib import Path
import sys
sys.path.insert(0, 'bench')
import forge2_kit, forge2_site, isolated_tiers, run_build
tier = isolated_tiers.FORGE20
work, port = Path(sys.argv[1]), int(sys.argv[2])
verdict = json.loads((work / 'verdict.json').read_text())
score, budget = verdict.get('score'), verdict.get('budget') or {}
published = {name: run_build.render_public_contract((run_build.ROOT / source).read_text(), port, forge2_site)
             for name, source in tier.public}
checks = [
    ('a forge-2.0 verdict', str(verdict.get('scorerVersion')).removesuffix('-rc') == tier.version,
     verdict.get('scorerVersion')),
    ('no harness gap', verdict.get('status') == 'scored' and not verdict.get('harness_missing')
     and not verdict.get('probe_unavailable'),
     {key: verdict.get(key) for key in ('status', 'harness_missing', 'probe_unavailable')}),
    ("the tier's budget, ended by the model", (budget.get('max_calls'), budget.get('stopped_by'))
     == (tier.call_budget, 'model_finished'), budget),
    ('public files as published', all((work / name).is_file() and (work / name).read_text() == text
                                      for name, text in published.items()),
     [name for name, text in published.items() if not (work / name).is_file() or (work / name).read_text() != text]),
    ('graded on the forge2 kit', (verdict.get('kit') or {}).get('lock_sha256') == forge2_kit.lock_sha256(),
     verdict.get('kit')),
    ('the v1 starter scores low', isinstance(score, (int, float)) and score <= 0.30, score),
]
for name, ok, seen in checks:
    print(f"{'PASS' if ok else 'FAIL'}  {name}: {seen}")
sys.exit(0 if all(ok for _name, ok, _seen in checks) else 1)
EOF
