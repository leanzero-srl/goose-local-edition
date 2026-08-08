#!/bin/sh
# Void the row created by a deliberate rotation kill. L344/F538: killing a cell does NOT abort its
# row — the sweep records the dead unit as a COMPLETED measurement with a real-looking score and
# void=False. Three such rows landed unvoided this morning and one OVERWROTE the campaign's best
# result. The kill must own its row.
RUNS=/Users/mihaiperdum/Projects/goose/evals/swarm-bench/runs/nodeloop
CELL="$1"; OUT="$2"
i=0
while [ "$i" -lt 40 ]; do
  RES="$RUNS/$CELL/nodeloop-result.json"
  if [ -f "$RES" ] && [ "$(find "$RES" -mmin -4 2>/dev/null | wc -l | tr -d ' ')" = "1" ]; then
    python3 - "$RES" <<'PY' >> "$OUT" 2>&1
import json, sys, datetime
p = sys.argv[1]
d = json.load(open(p))
if d.get("void"):
    print(f"{datetime.datetime.now():%H:%M:%S} already void: {p}")
else:
    d["void"] = True
    d["void_reason"] = ("ROTATION KILL: stopped deliberately because a 1-node cell cannot advance "
                        "F533, which needs 3-node replicates on the frozen binary. Harness artefact, "
                        "not a measurement — its score reflects when it was killed.")
    json.dump(d, open(p, "w"), indent=1)
    print(f"{datetime.datetime.now():%H:%M:%S} VOIDED {p} (was score={d.get('score')})")
PY
    exit 0
  fi
  i=$((i + 1)); sleep 5
done
echo "$(date '+%H:%M:%S') no fresh row for $CELL within 200s — nothing voided" >> "$OUT"
