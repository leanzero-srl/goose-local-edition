#!/bin/zsh
# clean.sh [--kill] — between runs: list (default) or kill leftovers on BOTH Macs, per PID (gate 4: never killpg).
# A leftover is an ORPHAN (PPID 1) engine/rank/launcher process, or a harness process of ours (driver, load,
# ladder, sampler, ssh helpers) that outlived its run. What the running Goose Swarm app owns is left alone:
# its goosed, its tailscaled, and any engine whose parent is alive. Load-bearing services on the Studio —
# headscale, the LeanZero Link worker, mcp-web-search, mcp-doc-processor, the system Tailscale — never match.
mode=${1:-list}
probe='
me=$$
ps -axo pid=,ppid=,etime=,command= | while read pid ppid et cmd; do
  [ "$pid" = "$me" ] && continue
  case "$cmd" in
    *headscale*|*leanzero-link-worker*|*mcp-web-search*|*mcp-doc-processor*|*/Applications/Tailscale.app*|*clean.sh*) continue ;;
  esac
  kind=""
  case "$cmd" in
    *"bin/rapid-mlx serve"*|*rapid_mlx*|*mlx_lm.server*|*mlx.launch*|*rank_wrapper*|*.goose/distributed/*) [ "$ppid" = 1 ] && kind="orphan-engine" ;;
    *"quality/harness/"*|*"ux-audit/"*|*r1.mjs*|*recovery.mjs*|*load.py*|*ladder.py*|*canary.py*|*sampler.sh*) kind="harness" ;;
  esac
  [ -n "$kind" ] && echo "$kind|$pid|$ppid|$et|${cmd[1,160]}"
done'
for host in macbook studio; do
  if [ $host = macbook ]; then out=$(zsh -c "$probe"); else out=$(ssh -o ConnectTimeout=5 workhorse "zsh -c $(printf %q "$probe")"); fi
  echo "== $host: $(echo -n "$out" | grep -c . ) leftover(s)"
  echo "$out" | grep . | sed 's/^/   /'
  if [ "$mode" = "--kill" ] && [ -n "$out" ]; then
    pids=$(echo "$out" | cut -d'|' -f2 | tr '\n' ' ')
    if [ $host = macbook ]; then kill $=pids 2>/dev/null; else ssh workhorse "kill $pids" 2>/dev/null; fi
    echo "   killed (per pid): $pids"
  fi
done
# Ports a stopped engine must not still hold.
for host in macbook studio; do
  cmd='lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | awk "/:(8090|8091|8190) / {print \$1, \$2, \$9}"'
  if [ $host = macbook ]; then l=$(zsh -c "$cmd"); else l=$(ssh workhorse "zsh -c $(printf %q "$cmd")"); fi
  echo "== $host model ports held: ${l:-none}"
done
