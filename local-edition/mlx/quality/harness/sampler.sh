#!/bin/zsh
# sampler.sh <out-dir> [interval-s] — until killed (by pid): one CSV row per Mac per interval.
# Fields: t, mac, wired_gib, pressure_free, engine_pid, engine_footprint_mb, tailscaled_footprint_mb,
# engine_running, engine_waiting, new_panic_files, new_ips_files, and for the SPLIT: rank count, the ranks'
# summed footprint and CPU% (rank spin = CPU pinned while no tokens move — R1). Read-only on both Macs.
out=$1; iv=${2:-30}; mkdir -p "$out"; csv="$out/samples.csv"
[ -s "$csv" ] || echo "t,mac,wired_gib,pressure_free,engine_pid,engine_fp_mb,tailscaled_fp_mb,running,waiting,new_panic,new_ips,ranks,rank_fp_mb,rank_cpu" > "$csv"
probe='
since=${SINCE:-0}
wired=$(vm_stat | awk "/wired down/ {gsub(\"\\\\.\",\"\",\$4); printf \"%.2f\", \$4*16384/1073741824}")
free=$(/usr/bin/memory_pressure -Q 2>/dev/null | awk -F": " "/percentage/ {print \$2}")
epid=$(pgrep -f "bin/rapid-mlx serve" | head -1)
efp=$( [ -n "$epid" ] && footprint -p $epid 2>/dev/null | awk "/phys_footprint:/ {print \$2, \$3; exit}" )
tpid=$(pgrep -f "Goose Swarm.app/Contents/Resources/bin/tailscaled" | head -1)
tfp=$( [ -n "$tpid" ] && footprint -p $tpid 2>/dev/null | awk "/phys_footprint:/ {print \$2, \$3; exit}" )
st=$(curl -s -m 3 127.0.0.1:8090/v1/status 2>/dev/null | python3 -c "import json,sys
try:
  d=json.load(sys.stdin); print(d.get(\"num_running\",\"\"), d.get(\"num_waiting\",\"\"))
except Exception: print(\"- -\")")
np=$(find /Library/Logs/DiagnosticReports ~/Library/Logs/DiagnosticReports -name "*.panic" -newermt "@$since" 2>/dev/null | wc -l | tr -d " ")
ni=$(find /Library/Logs/DiagnosticReports ~/Library/Logs/DiagnosticReports -name "*.ips" -newermt "@$since" 2>/dev/null | wc -l | tr -d " ")
rp=$(pgrep -f "\.goose/distributed/" | tr "\n" " ")
rn=$(echo $rp | wc -w | tr -d " ")
rfp=0; rcpu=0
for q in $rp; do f=$(footprint -p $q 2>/dev/null | awk "/phys_footprint:/ {v=\$2; if (\$3==\"GB\") v*=1024; if (\$3==\"KB\") v/=1024; print int(v); exit}"); rfp=$((rfp + ${f:-0})); c=$(ps -o %cpu= -p $q | tr -d " "); rcpu=$(echo "$rcpu + ${c:-0}" | bc); done
echo "$wired,$free,$epid,$efp,$tfp,${st% *},${st#* },$np,$ni,$rn,$rfp,$rcpu"'
since=$(date +%s)
while true; do
  t=$(date +%Y-%m-%dT%H:%M:%S)
  l=$(SINCE=$since zsh -c "$probe" 2>/dev/null)
  r=$(ssh -o ConnectTimeout=5 workhorse "SINCE=$since zsh -c $(printf %q "$probe")" 2>/dev/null || echo "ssh-failed")
  echo "$t,macbook,$l" >> "$csv"; echo "$t,studio,$r" >> "$csv"
  sleep $iv
done
