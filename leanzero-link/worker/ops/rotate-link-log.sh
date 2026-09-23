#!/bin/sh
# Daily copy-truncate rotation for the self-hosted Link sign-in service log.
# The log carries sign-in emails and client IPs; archives older than RETAIN_DAYS are deleted,
# so the retention the privacy policy states is enforced, not aspirational.
set -eu
LOG="${LINK_LOG:-$HOME/.leanzero/link-server.log}"
ARCH="${LINK_LOG_ARCHIVE:-$HOME/.leanzero/link-log-archive}"
RETAIN_DAYS="${RETAIN_DAYS:-30}"
mkdir -p "$ARCH"; chmod 700 "$ARCH"
if [ -s "$LOG" ]; then
  STAMP=$(date +%Y%m%d-%H%M%S)
  cp "$LOG" "$ARCH/link-server-$STAMP.log" && chmod 600 "$ARCH/link-server-$STAMP.log"
  : > "$LOG"   # launchd opened it O_APPEND, so the writer continues at offset 0
fi
find "$ARCH" -name 'link-server-*.log' -type f -mtime +"$RETAIN_DAYS" -delete
echo "rotated; archives kept: $(find "$ARCH" -name 'link-server-*.log' | wc -l | tr -d ' ')"
