#!/bin/zsh
# census.sh <label> — what runs on BOTH Macs right now: engine/rank processes, listening model ports,
# memory. One JSON line per call appended to $CENSUS_OUT (default ./census.jsonl). Read-only.
label=${1:-census}
out=${CENSUS_OUT:-./census.jsonl}
probe='
procs=$(ps -axo pid=,ppid=,rss=,etime=,command= | grep -E "rapid-mlx serve|rapid_mlx|rank_wrapper|mlx_lm|pipeline_qwen4|jaccl" | grep -v grep | awk "{printf \"%s|%s|%s|%s|\", \$1,\$2,\$3,\$4; for(i=5;i<=NF&&i<=9;i++) printf \"%s \", \$i; print \"\"}");
ports=$(lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | awk "/python|rapid|Python/ {print \$9}" | sort -u | tr "\n" " ");
pp=$(/usr/bin/memory_pressure -Q 2>/dev/null | awk -F": " "/percentage/ {print \$2}");
wired=$(vm_stat | awk "/wired down/ {gsub(\"\\\\.\",\"\",\$4); print \$4*16384/1073741824}");
printf "%s\n--PORTS %s\n--FREE %s\n--WIRED %s\n" "$procs" "$ports" "$pp" "$wired"'
local_out=$(zsh -c "$probe")
remote_out=$(ssh -o ConnectTimeout=5 workhorse "zsh -c '$(printf %s "$probe" | sed "s/'/'\\\\''/g")'" 2>&1)
python3 - "$label" "$out" <<PY
import json,sys,time
label,out=sys.argv[1],sys.argv[2]
def parse(t):
    d={"procs":[],"ports":"","free":"","wired_gib":""}
    for line in t.splitlines():
        if line.startswith("--PORTS"): d["ports"]=line[7:].strip()
        elif line.startswith("--FREE"): d["free"]=line[6:].strip()
        elif line.startswith("--WIRED"): d["wired_gib"]=line[7:].strip()
        elif line.strip(): d["procs"].append(line.strip())
    return d
rec={"t":time.strftime("%Y-%m-%dT%H:%M:%S"),"label":label,"macbook":parse('''$local_out'''),"studio":parse('''$remote_out''')}
open(out,"a").write(json.dumps(rec)+"\n")
for k in ("macbook","studio"):
    d=rec[k]; print(f"{label} {k}: {len(d['procs'])} engine procs · ports {d['ports'] or '-'} · free {d['free']} · wired {d['wired_gib']} GiB")
PY
