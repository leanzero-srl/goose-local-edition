#!/bin/bash
# fleet_reload.sh — reload the benchmark fleet's three models after an LM Studio unload.
# Mihai, 2026-08-17: "next time they're unloaded find the models we started with and load
# them yourself so we don't waste time." This is that instruction as an instrument.
#
# Ground truth (captured from pool_resolved + lms ls, 2026-08-17):
#   host              lms-link device id                    identifier prefix   quant
#   Local (mihai)     — (bare path targets local)           mihai-              Q8_0
#   Mac.lan (gabee)   e27216b3585b334cb8251c5ed1462f4e      gabee-              Q6_K
#   WorksMacStudio    271990023602ce917f4d773c7e5486bd      workhorse-          Q8_0
# Context 65536 (the recorded fit for Mac.lan; kept uniform), parallel 2.
# The three DISTINCT identifiers are load-bearing: identical aliases collapse the pool.
set -u
MODEL_KEY="qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp"
LOCAL_PATH="DavidAU/Qwen3.6-27B-Fable-Fusion-711-Uncensored-Heretic-NM-DAU-NEO-MAX-MTP-GGUF/Qwen3.6-27B-Fable-Fus-711-UnHeretic-NM-DAU-NEO-MAX-NEO-MTP-Q8_0.gguf"
CTX=65536

loaded() { lms ps 2>/dev/null | grep -c "$1"; }

echo "== fleet_reload $(date '+%F %T') =="
if [ "$(loaded gabee-)" -eq 0 ]; then
  echo "-- loading gabee on Mac.lan"
  lms link set-preferred-device e27216b3585b334cb8251c5ed1462f4e >/dev/null
  lms load "$MODEL_KEY" --identifier "gabee-$MODEL_KEY" --context-length $CTX --parallel 2 -y
else echo "-- gabee already loaded"; fi

if [ "$(loaded workhorse-)" -eq 0 ]; then
  echo "-- loading workhorse on WorksMacStudio.lan"
  lms link set-preferred-device 271990023602ce917f4d773c7e5486bd >/dev/null
  lms load "$MODEL_KEY" --identifier "workhorse-$MODEL_KEY" --context-length $CTX --parallel 2 -y
else echo "-- workhorse already loaded"; fi

if [ "$(loaded mihai-)" -eq 0 ]; then
  echo "-- loading mihai locally (bare path targets the local host)"
  lms load "$LOCAL_PATH" --identifier "mihai-$MODEL_KEY" --context-length $CTX --parallel 2 -y
else echo "-- mihai already loaded"; fi

echo "== final state =="
lms ps 2>/dev/null | grep fable-fusion || echo "NOTHING LOADED — check LM Studio on each host"
N=$(lms ps 2>/dev/null | grep -c fable-fusion)
echo "loaded: $N of 3"
[ "$N" -eq 3 ] || exit 1
