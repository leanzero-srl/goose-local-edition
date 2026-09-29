#!/bin/zsh
# wait until the background turn task output file shows the end event
until grep -q '"ev":"end"' "$1" 2>/dev/null; do sleep 5; done; echo done
