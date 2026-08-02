#!/bin/sh
# Fleet state every 30s, appended with a timestamp. The engine's event log goes SILENT for 12-13
# minutes during the skeleton draft (F85), and during that window the only source of truth about
# whether the fleet is working is LM Studio itself. A spot check answers "right now"; this answers
# "for how long, and did all three stay busy".
#
# READ-ONLY. It never loads, unloads or re-aliases anything — Mihai manages the fleet.
OUT="${1:-../runs/nodeloop/fleet-samples.tsv}"
[ -f "$OUT" ] || printf 'ts\tgenerating\tprocessing\tidle\tdetail\n' > "$OUT"
while :; do
  LINE=$(~/.lmstudio/bin/lms ps 2>/dev/null | awk 'NR>1 && NF{printf "%s=%s ", substr($1,1,5), $3}')
  G=$(printf '%s' "$LINE" | grep -o 'GENERATING' | wc -l | tr -d ' ')
  P=$(printf '%s' "$LINE" | grep -o 'PROCESSINGPROMPT' | wc -l | tr -d ' ')
  I=$(printf '%s' "$LINE" | grep -o 'IDLE' | wc -l | tr -d ' ')
  printf '%s\t%s\t%s\t%s\t%s\n' "$(date '+%H:%M:%S')" "$G" "$P" "$I" "$LINE" >> "$OUT"
  sleep 30
done
