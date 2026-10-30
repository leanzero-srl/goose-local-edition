# HANDOFF: omni-media release (session 2, 2026-10-30 18:30) - NOT released yet

Read with HANDOFF-omni.md (first session) and PROGRESS.md. Skills: goose-feature-dev, goose-mlx-inference
(references/omni-media.md has the measured table below).

## Where it stands
- Fork Rapid-MLX: TAGGED + PUSHED `v0.14.3-lz.19` = lz/omni-media 813e97e0b (vendored qwen3_omni dropped; a real
  regression fixed: stream_chat read `self._is_mllm` on engines built without __init__ -> 8 test_api_models failures;
  ruff on the new lines). Suite head vs lz.18 on the laptop: only those 8 were new; focused rerun head = base.
  `uvx --from 'rapid-mlx[mtp] @ git+...@v0.14.3-lz.19' rapid-mlx serve --help` OK.
- goose `omni-media` (origin, ~/wt/goose-omni on workhorse + laptop), rebased on origin/main 7d16de1ffb, head = see
  `git log -1`. Commits this session: pin lz.19 (+ lz.17/lz.18 appended to SUPERSEDED_ENGINE_LAUNCHERS, the lz.18 bump
  forgot lz.17; test every_older_pinned_tag_is_a_superseded_launcher), eslint redeclare fix, self-test phase 3C, sb7.2
  manifest refrozen (main carries the unreleased ledgerd charge d5fa44f640/e2e456e903/7522c90b2a; packaging REFUSED
  without it), release notes draft docs/releases/3.0.135-audio-and-video-input.md, CSP fix (below).
- Gate on the laptop (before the CSP commit): fmt, clippy --workspace --all-targets -D warnings, lint:check, vitest
  560/5,259 green. cargo test goose/providers/provider-types/sidecar: 3 env failures, none in touched code
  (key_connection only_a_refused_loopback... = laptop kernel RSTs a full backlog, 3/3 isolated; providers.rs codex +
  claude-code = live external CLIs). sqlx LINKEDIT trap: `--config 'profile.dev.package."*".strip=false'` after the
  subcommand + `cargo clean -p sqlx-macros -p sqlx-macros-core` once.
- Gemma weights: /Volumes/AI-workhorse/models/goose/mlx-community/gemma-4-e4b-it-8bit (sha matches; laptop copy gone).
- Packaged dev app (7bbd8cfb6e, NO CSP fix): ~/goose-builds/omni-pkg/Goose Swarm.app on the workhorse (delete after).

## PACKAGED-APP PROBE (Gabee, isolated profile, 17:10-18:25) - 3 release blockers found, 1 fixed
1. FIXED (3b7d320480, red/green csp.test.ts): playback refused in the packaged app, "MEDIA_ELEMENT_ERROR: Media load
   rejected by URL safety check": media-src lacked data:/blob: in BOTH policies (src/utils/csp.ts header + index.html
   meta). Needs a repackage to verify the play button live.
2. OPEN (product): the app mounts Gemma-4 with `--text-only` by default (sidecar: every vision checkpoint except
   qwen4_exp), so /v1/models = ['text','tools'] and goose withholds audio/video. Media needs Engine > Sampling >
   "Text-only lane" OFF + Remount (then ['text','vision','video','audio','tools']). Decide: keep the default and make the
   withheld notice tell the user exactly that, or auto text_only=false for audio-bearing checkpoints (golden-formula
   call, goose-knob-turning). The notice on the text lane was NOT seen: that run hit a Metal error first (item 4).
3. OPEN (blocker): on the MLLM lane every goose chat request with media is REFUSED 400 "Vision-request prompt tokens
   (21193) exceeds the per-batch cap (8192 ...)": goose's system prompt + 26 tools is ~21k tokens, the cap counts the
   whole prompt of a media request (mllm_batch_generator._prefill_cap_violation; --vision-prefill-token-budget).
   Pre-existing for IMAGES on any MLLM-lane VLM too. Measured (Gabee, budget raised to 131072, app argv, E4B 8bit):
     video, 2,512 prompt tokens: 6 s, peak 11.1 GB, correct (blue square moves, frames);
     video, 6,867 tokens: 10 s, peak 14.0 GB, correct;
     audio, 18,554 tokens: 31 s, peak 21.7 GB, verbatim transcript (Thursday 4 pm, pineapple);
     video, ~22k tokens: job killed past 33 GB (single forward; Gemma's bidirectional image attention).
   => raising the budget is NOT safe for video. Proposed fix (goose side, confidence moderate-high): a MEDIA PRE-PASS in
   goose-providers openai.rs for engines fronting Rapid-MLX (asks_engine_for_image_input): per media item, a side call
   to the same engine with a SHORT prompt (no tools, the user's text of that message + "transcribe / describe in
   detail", max_tokens ~1024, well under 8,192 tokens), cached by sha256(data)+kind in-process; the main agent turn
   carries a text part "[Attached audio <name> - what the model heard: ...]" instead of the media. Text-only requests on
   the MLLM lane are chunked and exempt from the cap. Fallback on a side-call error: the named placeholder + reason.
   Rejected: raising the budget (memory above), engine chunked media prefill (bidirectional image mask across a cache
   offset; low confidence today).
4. Unexplained: the first text-lane mount on Gabee failed its first request with "[METAL] Command buffer execution
   failed: Ignored (for causing prior/excessive GPU errors)"; the MLLM-lane remount worked. Re-check on the workhorse.
5. Minor: Engine view says "No models in the models folder yet" while serving from an NFS models_dir (models-ssd).

## Next steps, in order
1. Build the media pre-pass (item 3) + tests (side call replaces media; cache hit; error fallback; withheld path
   unchanged); decide item 2 (at least an actionable notice); fix release notes (Text-only lane off, the pre-pass,
   the numbers above; nothing about "measured in the app" until it is).
2. Rebase on origin/main, full gate on the laptop (gate script: scratch copy in the evidence dir below is the CDP
   driver; gate = fmt, clippy workspace, cargo test 4 crates w/ strip=false, lint:check, vitest).
3. `cargo build -p goose-cli` + `just copy-binary debug` + `pnpm run package` (laptop via node-run, ~5 min), copy the
   .app to the test Mac (tar over ssh). Live test on the WORKHORSE after ep3 (tmux ep3 / node-probe; ~23:00) or Gabee
   if idle (check `ioreg HIDIdleTime`, quit her Goose, restore with `open -a "Goose Swarm"`).
   Test recipe that works: isolated GOOSE_PATH_ROOT + GOOSE_USER_DATA_DIR + GOOSE_DISABLE_KEYRING=1, config
   `mlx_engine: {model_id: mlx-community/gemma-4-e4b-it-8bit, models_dir: <dir holding mlx-community/>, port: 8391}`;
   onboarding card `[data-testid=onboarding-use-swarm]` + "Get Started"; `#/leanzero-swarm` ->
   `[data-testid=placement-run-local]` (Run) then `placement-save-node-local` (Save as node); Sampling tab ->
   `[aria-label="Text-only lane"]` off, Save, Remount; `#/nodes` "New chats start on" -> the gemma node (Auto = the
   swarm pool = "no `swarm` block" error); `window.electron.addProject(dir)` stands in for the native folder dialog;
   "New session in work"; then omni_live.mjs pick / drop / play / send / wait / shot.
4. Then HANDOFF-omni.md step 5 (merge ff, notes, `just release-notarized 3.0.135` as a LOCAL nohup on the workhorse:
   Developer ID + ~/.leanzero/apple/notary.env are there, codesign over ssh fails errSecInternalComponent; workhorse
   disk ~30 GB free, rm ~/goose-builds/omni-pkg first; VIGIL per goose-feature-dev LONG RUNS), publish, install, repeat
   the live check on the installed app, independent check agent, skills, cleanup (laptop ~/wt/goose-omni/target,
   ~/forksuite-omni, ~/omni-probe; workmule ~/omni-probe 22 GB; Gabee ~/omni-test 1.8 MB; worktrees after merge).

## Traps paid for this session
- The app keeps earlier sessions' chat views MOUNTED (hidden): `querySelector('input[type=file]')` hits a hidden
  composer. omni_live.mjs marks the VISIBLE chat-input and its own file input first.
- Gabee's Chrome listens on 9333 (another session's automation) and the workhorse's 9333 is taken too: Electron's
  devtools bind fails silently ("Cannot start http server for devtools"); use 9353 + an ssh -L tunnel on 19353.
- node-probe reads "benchmark running" from mtimes under Goose/benchmark/runs/build - a cleanup rm -rf there trips it.
- `git push` to GitHub failed twice with "Permission denied (publickey)" while `ssh -T` worked; a retry passed.
- Thunderbolt workhorse-tb (192.168.0.2) timed out from the laptop; no `laptop-tb` alias on the workhorse.
- The fork's commit-msg hook (and goose's) rejects any Co-Authored-By: Claude line.

Evidence: /Volumes/AI-workhorse/media/goose/2026-10-30-omni-media/packaged-app-gabee/ (screenshots a1/a2/g4/g6/g9/g10,
app engine logs, probe-engine.log, out_req_*.json, mkreq.py, budget_probe.sh, omni_live.mjs).
