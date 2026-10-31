#!/bin/zsh
# Seed for brief 2026-10-31-3. Usage: setup.sh <WORK>. grader/ (the voice-note transcripts) is NEVER copied.
# How the media were made (2026-10-31, macOS 27 workhorse; re-run only to regenerate, the committed files are canonical):
#   screenshot: <goose>/ui/desktop/bundled-mcps/browser/chrome-headless-shell/*/chrome-headless-shell-mac-arm64/chrome-headless-shell \
#       --headless --disable-gpu --hide-scrollbars --window-size=520,560 --screenshot=booking-screenshot.png file://$SEED/clinic-booking/index.html
#   voice 1: say -v Moira -o voice-note-1.wav --data-format=LEI16@16000 "$(cat grader/voice-note-1.transcript.txt)"   (25.2 s, 810 KB)
#   voice 2: say -v Moira -o voice-note-2.m4a --data-format=aac "$(cat grader/voice-note-2.transcript.txt)"            (16.5 s, 99 KB)
set -euo pipefail
WORK=${1:?usage: setup.sh <WORK>}; SEED=${0:A:h}
mkdir -p "$WORK"; [ -z "$(ls -A "$WORK")" ] || { echo "WORK not empty: $WORK" >&2; exit 1; }
cp -R "$SEED/clinic-booking" "$WORK/clinic-booking"
mkdir -p "$WORK/inbox"
cp "$SEED/booking-screenshot.png" "$SEED/voice-note-1.wav" "$SEED/voice-note-2.m4a" "$WORK/inbox/"
cd "$WORK"; git init -q -b main; git add -A
git -c user.name='Harbour Physio' -c user.email='web@example.com' commit -qm 'Booking widget as it is live today'
ls -R "$WORK" | head -20
