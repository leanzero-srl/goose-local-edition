# HANDOFF: omni-media (audio + video input) -> a notarized Goose Swarm release

Owner goal (2026-10-30): goose understands audio + video like an omni model, SHIPPED as a notarized release.
Read PROGRESS.md (same dir) for the measured history. Load skills goose-feature-dev + goose-mlx-inference first.

## State (2026-10-30 ~16:00)
- goose branch `omni-media` (origin, worktree ~/wt/goose-omni on workhorse, also ~/wt/goose-omni on the laptop
  and workmule). Head 32b5a56e49. NOT merged into main.
  Rust: `MessageContent::Media(MediaContent{kind audio|video, data b64, mime_type, name})` (goose-provider-types
  conversation/message.rs); OpenAI format sends `input_audio` / `video_url` data URL or `MediaContent::placeholder()`
  (options `withhold_audio` / `withhold_video`, ImageReport `audio_sent/video_sent/media_withheld`); goose-providers
  openai.rs `media_format_options` asks a goose MLX engine's /v1/models (`capabilities` lacks audio/video -> withhold);
  ACP intake: `ContentBlock::Audio` -> audio media, blob resource `audio/*|video/*` -> media
  (`attached_media_from_blob`), replay via `media_content_block` (acp/server.rs, load_session.rs); anthropic,
  databricks, bedrock, snowflake, context_mgmt, cognirunner events send/skip a placeholder.
  Desktop: types/message.ts (MediaContent, getMediaContent, mediaKindOf, ImageData.kind/name), acp/prompt.ts
  (`mediaToAcpContentBlock`: audio block; video = resource blob uri attachment:///name), acp/adapter/messages.ts,
  hooks/useFileDrop.ts (`readMediaFile`, 50 MB cap), ChatInput.tsx (drop + picker + tray), components/media/
  MediaAttachment.tsx (drawn play/pause card, solid node-5/node-2 badges), UserMessage.tsx.
  GATE so far (laptop): cargo check + clippy -D warnings green for goose-provider-types/goose-providers/goose;
  8 media tests pass (`cargo test ... --lib -- media`); cargo fmt applied; vitest 560 files / 5,259 pass;
  lint:check had 2 no-undef errors -> fixed in 32b5a56e49 (eslint globals), RE-RUN lint:check to confirm.
  NOT yet run: full workspace clippy (goose-cli etc.), goose-self-test.yaml, i18n (no new i18n keys; plain strings).
- fork Rapid-MLX branch `lz/omni-media` (origin, worktree ~/Projects/Rapid-MLX-wt-omni-media), head 636c6f04c,
  off v0.14.3-lz.18. rapid_mlx/api/media_parts.py (input_audio -> temp file + {"type":"audio"} marker; video ->
  label + JPEG data-URL frames, 1 fps, max 16, env RAPID_MLX_VIDEO_FRAME_FPS / _MAX_FRAMES), batched.py +
  mllm_scheduler.py + mllm_batch_generator.py carry `audios` to `prepare_inputs(audio=)`, audio requests skip
  pixel/feature/boundary caches + text-only prefill; chat.py allow_audio; /v1/models tags video + audio.
  tests/test_media_parts.py 9 pass, 1,361 related fork tests pass. Also holds rapid_mlx/models/qwen3_omni_vendored
  (0.7.11 model code, NOT wired, insufficient alone) -> delete it before tagging unless the pin moves.
- PROVEN over HTTP (laptop, fork 636c6f04c, `rapid-mlx serve` by hand = dev probe only, NOT the app):
  mlx-community/gemma-4-e4b-it-8bit (8.3 GB, ~/omni-probe/models on the LAPTOP internal disk): /v1/models
  ['text','vision','video','audio','tools']; audio -> verbatim transcript (Thursday 4 pm, pineapple);
  video -> blue square moves right, red square, "GATE OPEN"; both in one message -> both right.
  Evidence: /Volumes/AI-workhorse/media/goose/2026-10-30-omni-media/ (media + logs).
- Qwen3-Omni-30B-A3B (workmule ~/omni-probe/models, 22 GB): audio OK on 0.7.1, vision BROKEN on mlx-vlm 0.7.1
  (needs 0.7.11 = whole-pin bump, own change). Not in this release; ledger it.

## Remaining, in order (each step: commit; PROGRESS.md line)
1. Fork release: drop qwen3_omni_vendored, run the fork suite (~25 min, node-run, NOT workhorse before 23:00),
   tag `v0.14.3-lz.19` on lz/omni-media (or merge into the lz.18 line first if main moved), push tag.
2. goose pin bump (DoD in goose-mlx-inference CORE card "Engine pin bump"): ENGINE_LAUNCHER -> lz.19, old one
   appended to SUPERSEDED_ENGINE_LAUNCHERS, PINNED_PROFILES row (engine_profile.rs),
   PIPELINE_SINGLE_LINE_REVIEWED_THROUGH (distributed/provision.rs) + fork manifest reviewed_through, both argv
   goldens; `uvx --from git+...@v0.14.3-lz.19 rapid-mlx serve --help`; ignored test live_mount_of_the_real_engine.
3. Rebase omni-media on origin/main (another session commits there daily); full gate: fmt, clippy --all-targets
   -D warnings (workspace), cargo test -p goose -p goose-providers -p goose-provider-types, lint:check, test:run.
   Workmule cargo needs `OPENSSL_ROOT_DIR=/opt/homebrew/opt/openssl@3` + CXXFLAGS include (only openssl@4 there;
   llama-cpp-sys httplib fails) - unresolved; the LAPTOP builds fine (own CARGO_TARGET_DIR=~/wt/goose-omni/target).
4. Live proof in the PACKAGED dev app (goose-feature-dev "THE ONE RECIPE"): `pnpm run package` (~4 min), quit the
   installed app, launch the package with its own GOOSE_PATH_ROOT/USER_DATA_DIR + CDP port, mount gemma-4-e4b-it-8bit
   (weights must be under the app's models dir: move to /Volumes/AI-workhorse/models/goose/mlx-community/ after ep3,
   ai-disk-guard first; delete the laptop copy), attach speech.wav then clip.mp4 (CDP: drop/picker can't be
   synthesised -> use window.electron + a DataTransfer on the composer drop target), screenshot + transcript to
   the SSD evidence dir; the answer must name pineapple/Thursday and GATE OPEN. Quit by pid, restore installed app,
   `goose-single-guard --check`. If the laptop is used: tell the coordinator first (a window opens on his screen).
5. Merge omni-media -> main (fast-forward), version bump, docs/releases/<ver>-goose-swarm.md, then
   `just release-notarized <ver>` (laptop has Developer ID + notary.env + gh; >=12 GB free; vigil per skill),
   tag, push, `just publish-release <ver> <notes>`; verify assets (dmg, 2 zips, latest-mac.yml);
   install with ~/goose-builds/loop-state/install_goose_release.sh <ver> auto on each Mac (not mid-benchmark);
   repeat step 4's audio + video check on the INSTALLED app.
6. Cleanup: laptop ~/omni-probe (weights 8.3 GB, uv caches stay), workmule ~/omni-probe (22 GB Qwen weights),
   ~/wt/goose-omni/target on the laptop (cargo), worktrees after merge; `bash ~/.claude/skills/goose-clean/scripts/after-work.sh`.

## Traps paid for today
- zsh does not word-split `$P` command strings: write probe scripts in bash with arrays.
- node-jobs status field is `status`, not `state` (an until-loop on "state" never ends).
- Workmule can go BUSY from another session's engine hold (gverify serve.sh ... hold, 4 h): check node-probe.
- The fork's image loader refuses local file paths in image_url: frames must be data URLs.
- `/v1/models` id is the SERVED name; capability reads must resolve cfg.model_path.
- CLI: no attach path exists (images are path-detected in the formatter); media not added there on purpose.
