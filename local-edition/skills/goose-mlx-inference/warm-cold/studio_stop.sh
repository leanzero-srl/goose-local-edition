#!/bin/zsh
# On the Studio: stop.sh <label> — SIGTERM the uv parent (forwarded to python), then the python child, per pid.
label=$1
uvpid=$(cat $HOME/q108/$label.pid 2>/dev/null) || { echo "no pid file"; exit 1; }
py=$(pgrep -P $uvpid)
echo "uv=$uvpid python=$py"
kill -TERM $uvpid 2>/dev/null
for i in {1..40}; do kill -0 $uvpid 2>/dev/null || { [[ -z $py ]] || ! kill -0 $py 2>/dev/null; } && break; sleep 1; done
[[ -n $py ]] && kill -0 $py 2>/dev/null && { echo "python $py still alive, TERM"; kill -TERM $py; sleep 5; }
[[ -n $py ]] && kill -0 $py 2>/dev/null && echo "STILL ALIVE: $py"
lsof -nP -tiTCP:18571 -sTCP:LISTEN >/dev/null && echo "18571 STILL LISTENING" || echo "stopped; 18571 free"
