# HANDOFF — floating window ("engine glance" pill), branch fix/floating-window

## Owner spec (2026-10-27)
"if I press on X I expect it to go away. This should only show up if I choose a tick in the main UI when
minimizing. Also when I drag and drop it around it will still open the full window."

## Real cause (why a click / drag raised goose's main window)
Electron v41 `type:'panel'` is not an NSPanel (shell/browser/ui/cocoa/electron_ns_panel.mm only ORs
NSWindowStyleMaskNonactivatingPanel into the styleMask GETTER), so every mouse-down on the glance activates goose.
The glance was `focusable:false` -> `disableKeyOrMainWindow` -> canBecomeKeyWindow NO (electron_ns_window.mm), so the
activation had no key window to give the clicked window and AppKit made goose's MAIN window key and ordered it front,
before the release. `app.on('activate')` is NOT involved (it fires on Dock reopen and only creates a window when
there are none; the glance is a BaseWindow so it is never counted).

## Fixes (commits on this branch after e8f2e57a90)
1. engineGlanceWindow.ts: `focusable: mac` — the clicked glance becomes key itself, nothing else moves; still shown
   with showInactive (never takes focus by appearing).
2. engineGlanceDesktop.ts: shove-back (gooseRaised / revertRaise / RAISE_*) REMOVED. A press holds the window in place
   PRESS_HOLD_MS (1.5 s) so the click reaches its control. X: closes every time; if its press is what activated goose
   (did-become-active within PRESS_PAIR_MS 700 ms) and no goose window was in view, `app.hide()` hands the front back.
   Close still holds until a NEW process (turn/question/split/rank/route) starts, as before.
   main.log evidence if the fix ever fails: `engine glance: goose window N came forward right after a press on the glance`.
   On X with hide: `engine glance: closed from its X; goose hidden again, the press had activated it`.
3. OPT-IN: `GlancePrefs.desktopOptIn` (default false; a store from an older build with desktop:'away' reads as NOT
   opted in). `desktopFloatOn(prefs)` gates every rule, the tray "Show the floating glance", Settings' "Show it again".
   UI: Settings › App switch "Show a floating activity pill when goose is minimized or in the background" (+ When:
   background / whenever the engine works), and a tick "Float a pill when goose is minimized" under the sidebar card.
4. Controls: ⤢ (Maximize2) is the only opener, on pill and card. Pill = [⤢ open goose] [▴ show card] [✕]. Body = drag
   handle only. A press that travels >= DRAG_SLOP_PX (4 px) is never a click, even when it started on a button.
5. Strings: 4 new keys, 3 dropped, in 16 catalogs. Peer/rank display (6fd39b5348) untouched, now behind the opt-in.

## Tests
engineGlanceDesktop.test.ts: describe "OPT-IN" and "X goes away, a drag is not a click, only ⤢ opens" (each with a
negative control). engineGlanceClicks.test.tsx: pill rows incl. ⤢, drag-from-⤢ not a click (+ control), body drag.
Fixture `glancePush` defaults desktopOptIn:true. Results: see "Gate results" below.

## Packaged dev build (for the real-click check on the workhorse)
Built on the laptop (workhorse is running Gauntlet). Path on the workhorse after copy: see "Gate results".
Rebuild recipe (any Mac, from this worktree):
  source bin/activate-hermit && cd ui && pnpm install --frozen-lockfile && cd desktop
  cp "/Applications/Goose Swarm.app/Contents/Resources/bin/"{goose,tailscale,tailscaled} src/bin/
  pnpm run package        # -> ui/desktop/out/Goose-darwin-arm64/Goose.app (name per forge config)
Launch (ONE GOOSE PER MAC: quit the installed app first; never pkill by pattern):
  osascript -e 'tell application "Goose Swarm" to quit'
  open -n "<path>/Goose.app" --args --remote-debugging-port=9898
  ... checks ... then quit THAT pid, `open -a "Goose Swarm" --args --remote-debugging-port=9897`,
  `~/Projects/machine-sync/bin/goose-single-guard --check`.
Needs something live for the pill: start a chat turn on the local engine (or a split / hosted rank).

## 6-step REAL-click checklist (screen control, goose minimized, another app e.g. Finder in front)
1. Opt-in OFF (fresh profile or untick): start a turn, minimize goose, focus Finder -> NO pill anywhere.
2. Tick Settings › App "Show a floating activity pill…" (or the sidebar tick), minimize, focus Finder -> pill bottom-right.
3. Click ✕ -> pill gone at once; goose main window does NOT appear; Finder is frontmost again (menu bar says Finder).
   Pill stays gone through the rest of that turn (blur/focus Finder/other apps). main.log: "closed from its X; goose hidden again".
4. (Next turn, so it is back) DRAG the pill by its body to the top-left -> it moves and snaps; goose main window does NOT
   appear. Also press on ⤢ and drag off before releasing -> nothing opens. main.log must NOT contain "came forward right after a press".
5. Click ⤢ -> goose comes forward on the Engine tab (or the chat that needs you). Click ▴ -> the full card; − -> back to pill.
6. Quit goose, relaunch (new process) with the tick ON -> pill comes back on the next live work; untick -> never.
If step 3/4 fails (main window appears): read main.log for the warn line; the theory was AppKit key hand-off — next
lever would be a native NSPanel (no Electron API exists).

## Gate results (2026-10-27, run on the laptop via node-run; workhorse busy with Gauntlet)
- `pnpm run lint:check` (tsc + eslint --max-warnings 0 + i18n:check + locale validation, 16 catalogs): EXIT 0.
- Full `vitest run`: 5223 passed, 2 failed (both stale expectations of the new spec: the defaults test and the
  Settings description text) -> fixed in 15d5a04d4d; re-run of all glance suites (12 files, 243 tests): all pass.
- Packaged (`pnpm run package`, EXIT 0), commit 15d5a04d4d, goose/tailscale(d) from the laptop's installed app.
  App asar carries `desktopOptIn` and the new log line.
  PATH ON THE WORKHORSE: /Users/workhorse/Projects/goose-wt-floatwin/ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app
  (ad-hoc signed, same app name as the installed one: quit the installed app by pid, then
  `open -n "<that path>" --args --remote-debugging-port=9898`; it shares the installed app's settings, where
  desktopOptIn is absent -> step 1 starts OFF.)
- NOT verified: real clicks (no Accessibility from ssh anywhere); the AppKit key-handoff theory is from Electron v41
  source (electron_ns_panel.mm, electron_ns_window.mm, native_window_mac.mm SetFocusable/ShowInactive), not observed.
  Laptop worktree removed after the build (clutter rule).
