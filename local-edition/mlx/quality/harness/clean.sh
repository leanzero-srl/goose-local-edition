#!/bin/zsh
# clean.sh [--kill] — between runs: list (default) or kill leftovers on BOTH Macs, per PID (gate 4: never killpg).
# A leftover is an ORPHAN (PPID 1) engine/rank/launcher process, or a harness process of ours (driver, load,
# ladder, sampler, ssh helpers) that outlived its run. What the running Goose Swarm app owns is left alone:
# its goosed, its tailscaled, and any engine whose parent is alive. Load-bearing services on the Studio —
# headscale, the LeanZero Link worker, mcp-web-search, mcp-doc-processor, the system Tailscale — never match.
# An ORPHAN MCP (Q-138) is a bundled LeanZero MCP (bundled-mcps/leanzero-*) whose goose is gone (PPID 1): on
# 2026-09-26 six of them spun at 100% CPU for up to 3.4 days, deaf to SIGTERM, so --kill escalates them to
# SIGKILL per pid when they are still there after the TERM.
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
    *bundled-mcps/leanzero-*) [ "$ppid" = 1 ] && kind="orphan-mcp" ;;
    *"bin/rapid-mlx serve"*|*rapid_mlx*|*mlx_lm.server*|*mlx.launch*|*rank_wrapper*|*.goose/distributed/*|*"import base64,sys"*) [ "$ppid" = 1 ] && kind="orphan-engine" ;;
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
    mcps=$(echo "$out" | grep '^orphan-mcp|' | cut -d'|' -f2 | tr '\n' ' ')
    if [ -n "${mcps// }" ]; then
      sleep 2
      deaf="for p in $mcps; do ps -o command= -p \$p 2>/dev/null | grep -q 'bundled-mcps/leanzero-' && kill -9 \$p && echo \$p; done"
      if [ $host = macbook ]; then k=$(zsh -c "$deaf"); else k=$(ssh workhorse "zsh -c $(printf %q "$deaf")"); fi
      [ -n "$k" ] && echo "   orphan MCPs deaf to SIGTERM, SIGKILLed (per pid): $(echo $k | tr '\n' ' ')"
    fi
  fi
done
# Ports a stopped engine must not still hold.
for host in macbook studio; do
  cmd='lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | awk "/:(8090|8091|8190) / {print \$1, \$2, \$9}"'
  if [ $host = macbook ]; then l=$(zsh -c "$cmd"); else l=$(ssh workhorse "zsh -c $(printf %q "$cmd")"); fi
  echo "== $host model ports held: ${l:-none}"
done
# Disk: agents' worktrees each carry their own cargo target (4–29 GB); on 2026-09-25 they filled the disk to
# 378 MB free mid-loop. Report free space on both Macs and every worktree whose branch is already merged.
echo "== macbook free: $(df -h / | awk 'NR==2 {print $4}') · studio free: $(ssh workhorse "df -h / | awk 'NR==2 {print \$4}'")"
repo=/Users/mihaiperdum/Projects/goose
for b in $(git -C $repo branch --merged main | tr -d ' +*' | grep '^worktree-agent-'); do
  w=$repo/.claude/worktrees/${b#worktree-}
  [ -d "$w" ] && [ -z "$(git -C "$w" status --porcelain 2>/dev/null)" ] && [ "$(git -C $repo rev-list --count main..$b)" = 0 ] && echo "   merged worktree (removable once its agent has finished): $w ($(du -sh "$w" 2>/dev/null | cut -f1))"
done
