#!/bin/zsh
# serve.sh <label> <cache:on|off> <mtp:on|off> [spec] — ONE engine on :18571, pid in ~/q108/<label>.pid
label=$1; cache=$2; mtp=$3; SPEC=${4:-"rapid-mlx[mtp] @ git+https://github.com/leanzero-srl/Rapid-MLX@eb9ed506d329a8f73a7e3d1fe0079983ab78219c"}
export PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
if lsof -nP -tiTCP:18571 -sTCP:LISTEN >/dev/null; then echo "REFUSED: 18571 already listening"; exit 3; fi
if ps -axo command | grep -v grep | grep -q "rapid-mlx serve"; then echo "REFUSED: another rapid-mlx engine is running"; exit 4; fi
M=$HOME/.goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx
args=(serve "$M" --port 18571 --served-model-name q85 --max-concurrent-requests 8 --enable-auto-tool-choice --tool-call-parser qwen3_coder_xml --reasoning-parser deepseek_r1 --text-only)
[[ $cache == on ]] && args+=(--enable-prefix-cache) || args+=(--disable-prefix-cache)
[[ $mtp == on ]] && args+=(--speculative-config "{\"method\":\"mtp\",\"model\":\"$M\",\"num_speculative_tokens\":3}")
echo "args: ${args[@]}" > $HOME/q108/$label.log
nohup uv tool uvx --from "$SPEC" rapid-mlx "${args[@]}" >> $HOME/q108/$label.log 2>&1 &
echo $! > $HOME/q108/$label.pid
echo "started uv pid $(cat $HOME/q108/$label.pid) cache=$cache mtp=$mtp"
