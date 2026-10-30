# omni-media: audio + video input to a local omni model (branch omni-media)

## CAUSE (30-min rule)
- Feature: owner 2026-10-30 "our goose could understand audio, video and everything else that an omni can do".
  Attach/drag/paste audio + video in the desktop composer -> ACP message parts -> provider request
  (OpenAI content parts) -> LeanZero MLX engine (Rapid-MLX fork) -> the model's processor.
- Why no shorter path: the engine accepts `input_audio` in request validation but DROPS it on the MLLM chat
  path (vllm_mlx/api/utils.py raises "Audio content blocks are not supported on this path"; MLXMultimodalLM.generate
  never forwards `audio`). goose has no audio/video message content at all. Every layer needs a change.
- Expected duration: probe 15 min (+ ~20 GB download), engine 1-2 h, goose engine+provider 1-2 h, desktop 1-2 h,
  live proof 1 h. Long jobs: model download (node-run), cargo builds (workmule via node-run), pnpm package.
- Cheaper path checked: mlx-vlm 0.7.1 (the fork's pin) already has `prepare_inputs(processor, images, audio, videos)`
  and the qwen3_omni_moe / gemma4 models; the engine already parses `video_url`. No new model code needed.

## Hardware tonight
- ep3 build owns the workhorse until ~23:00 (and the SSD heavy lock). No heavy jobs there, no split.
- Model + probe on workmule via node-run.

## Model choice (web-searched 2026-10-30, not from memory)
- Primary: mlx-community/Qwen3-Omni-30B-A3B-Instruct-4bit (21.8 GB, qwen3_omni_moe, audio+video+image).
  Known upstream: mlx-vlm issue #2099 audio+video together -> Metal fault in chunked prefill (each alone works).
- Fallback: mlx-community/gemma-4-e4b-it-4bit (gemma4 E4B: image, video, audio).
- MiMo-V2.6-Flash MOPD omni: two-Studio split only (vision proven, audio/video never wired) -> second, not tonight.

## Log
- 15:20 worktrees: ~/wt/goose-omni (omni-media off origin/main c043a5b7f4), ~/Projects/Rapid-MLX-wt-omni-media
  (lz/omni-media off v0.14.3-lz.18). Probe next.
- 15:26 PROBE (workmule, mlx-vlm 0.7.1 = the fork's pin, mlx_vlm.generate CLI, 4bit weights on workmule's
  internal disk ~/omni-probe/models because ep3 holds the SSD heavy lock):
  AUDIO alone: correct ("Thursday, 4 in the afternoon, pineapple") -> the model + processor hear speech.
  VIDEO native: garbage ("median.median..."); FRAMES as images: "!!!!"; FRAMES+AUDIO: Metal GPU Address Fault.
  = upstream mlx-vlm #2099 (deepstack masks vs chunked prefill; fix PR #2256). Probing prefill-step 65536 and
  mlx-vlm 0.7.11 next (the judge run 2026-10-30 had working frames on 0.7.11, 8bit).
- 15:27 goose: MessageContent::Media(MediaContent{kind audio|video, data b64, mime_type, name}) added in
  goose-provider-types; formatter/ACP/desktop next.
- 15:35 PROBES 2-6 (workmule):
  mlx-vlm 0.7.11 standalone: AUDIO verbatim transcript; FRAMES-as-images describe the blue->red square
  (partly wrong: invents black frames, misses motion + "GATE OPEN"); AUDIO+FRAMES together and native VIDEO
  return empty. 0.7.1 single image: blank. prefill-step 65536 on 0.7.1: still "!!!!" (not chunking alone).
  Vendored 0.7.11 qwen3_omni_moe package (rapid_mlx/models/qwen3_omni_vendored, aliased over mlx_vlm's) under
  0.7.1's generate loop: fault gone but still "!!!!" / blank -> the fix also lives in 0.7.11's generate code.
  => Qwen3-Omni needs the mlx-vlm pin at 0.7.11 (affects every model: own fork branch + tests), OR use a model
  whose vision already works on 0.7.1. Probing both: fork lz.18 with an mlx-vlm 0.7.11 override, and
  gemma-4-e4b-it-8bit (audio + frames) on 0.7.1.
  Decision for goose: video goes to the engine as a video_url part, the ENGINE samples frames into image parts
  (model-agnostic; native video paths are broken on both mlx-vlm versions); audio goes as input_audio.
- 15:35 workmule cargo: llama-cpp-sys-2's httplib fails against Homebrew OpenSSL 4 (only openssl@4 installed).
  Installed keg-only openssl@3 on workmule, build with OPENSSL_ROOT_DIR=/opt/homebrew/opt/openssl@3.
- 15:50 ENGINE (fork lz/omni-media 73cb25592): input_audio -> temp file + {"type":"audio"} marker ->
  mlx_vlm.prepare_inputs(audio=); video_url -> label + sampled frame image_url parts before the template;
  /v1/models tags "video" (vision models) + "audio" (audio encoder in config); audio requests skip the
  image-keyed caches + text-only prefill. tests/test_media_parts.py 9 pass; 1,311 related fork tests pass.
  Qwen3-Omni still needs the mlx-vlm pin question settled (0.7.1 vision broken on it, see 15:35).
- 15:50 GOOSE (1b53648067): Media content end to end in Rust (formats, ACP intake + replay, provider asks
  /v1/models for audio/video) and desktop (types, prompt.ts, adapter, useFileDrop/picker, MediaAttachment card,
  UserMessage). Compile: workmule went BUSY (another session's gverify engine hold since 15:30, 5 GB headroom),
  so cargo check runs on the laptop (job workhorse-20261030-155003-af47, own target ~/wt/goose-omni/target).
  BLOCKED for GPU probes until workmule frees (or the workhorse after ~23:00).
- 16:00 PROVEN over HTTP (laptop, fork 636c6f04c, gemma-4-e4b-it-8bit on the fork's mlx-vlm 0.7.1): audio,
  video and both answered from content (see HANDOFF-omni.md). Desktop vitest green; clippy green on touched crates.
  Owner now wants a notarized RELEASE: the chain (fork tag, pin bump, packaged-app proof, merge, release-notarized,
  install) is written step by step in HANDOFF-omni.md; heavy steps wait for the workhorse (~23:00) or a free Mac.
- 16:05 RELEASE CHAIN started (session 2). Fork: vendored qwen3_omni dropped (17df97b85, pushed); fork suite head vs lz.18
  running on the laptop (node-run workhorse-20261030-160224-dc06, ~/forksuite-omni.log). Lint re-run FOUND a real error:
  BenchmarkVideo.tsx `/* global HTMLVideoElement */` now redeclares the eslint.config.js global -> removed (d5f0f46a00),
  lint re-running. Pin bump to lz.19 staged (not committed until the tag exists); found the lz.18 bump never appended
  lz.17 to SUPERSEDED_ENGINE_LAUNCHERS (lz.17 installs never migrate) -> both lz.17 + lz.18 appended, new test
  every_older_pinned_tag_is_a_superseded_launcher.
- 16:15 lint:check GREEN on the laptop after d5f0f46a00. Gate (fmt/clippy workspace/tests/lint/vitest) running on the laptop
  (node-run workhorse-20261030-160856-c529, ~/gate-omni.log; first try caught a double comma in my pin edit -> fixed by
  autosquash). Gemma weights copying to /Volumes/AI-workhorse/models/goose/mlx-community/ over Tailscale (Thunderbolt link
  192.168.0.2 times out; ~20 min). Release notes drafted: docs/releases/3.0.135-audio-and-video-input.md (covers the 50
  main commits since v3.0.134 too). Plan: packaged dev app built on the laptop, live media test + notarized build on the
  workhorse after ep3 (~23:00): Developer ID + notary.env are on the workhorse; codesign over ssh is unproven, so the
  notarized build runs as a local nohup here.
- 16:25 GATE (laptop, rebased 2db0211c2a): fmt OK, clippy --workspace --all-targets -D warnings OK, lint:check OK, vitest OK.
  cargo test goose/providers/provider-types/sidecar: 3 failures, none in touched code: key_connection
  only_a_refused_loopback... ("a full queue answered: Connection reset by peer" - the laptop kernel RSTs a full backlog;
  file untouched by omni-media; rerunning isolated), providers.rs test_codex_provider (ChatGPT account refuses
  gpt-5.2-codex) and test_claude_code_provider ("Stream yielded no message") = live external CLIs. sqlx LINKEDIT trap
  hit once (strip=false flag fixes it). Gemma on the SSD, sha match, laptop copy deleted. Laptop disk was 15 GB free
  (target/) -> 24 GB. Packaging (debug goose-cli + pnpm package) on the laptop: node-run workhorse-20261030-162344-da9d.
  Fork suite head arm at shard 40/86 (memory-guard shards take 9 min each).
- 16:32 PACKAGE BLOCKER found + fixed: `pnpm run package` refused "Packaged benchmark differs from release manifest:
  bench/score_sb7.py" - main carries the unreleased SB7.2 ledgerd self-exit charge (d5fa44f640/e2e456e903/7522c90b2a,
  2026-10-29) without a manifest refreeze. Per goose-benchmark-iteration release recipe: scorer tests 184 passed;
  admission (score_sb72.admit) and run_build/bench_isolation unchanged -> no site sync, no empty-entrant e2e; sb7.2
  refrozen in a clean worktree (7b20f73052), forge2 already matches; sb7.1/forge manifests are old receipts, not shipped.
  key_connection test fails 3/3 isolated on the laptop (kernel RST on a full backlog) = environment, untouched file.
  Repackaging (node-run workhorse-20261030-163030-1a49).
- 16:44 packaged dev app (7bbd8cfb6e tree, debug goose-cli) built on the laptop (vitest 560 files/5,259 pass incl. benchReleaseManifest), copied to the workhorse ~/goose-builds/omni-pkg/. Waiting: fork suite base arm (laptop), then tag lz.19; ep3 4K render on the workhorse (live media test + notarized build wait for it).
- 17:04 FORK TAGGED v0.14.3-lz.19 = 813e97e0b (pushed). Fork suite head vs lz.18: the only new failures were 8 test_api_models streaming tests - a REAL regression: stream_chat read self._is_mllm on engines the tests build without __init__ (AttributeError) -> getattr default False (b25e42091); ruff I001/format fixed on the new lines; focused rerun head = base (923 passed, 0 new; media 9 passed). uvx --from ...@v0.14.3-lz.19 rapid-mlx serve --help OK.
- 18:24 PACKAGED-APP PROBE on Gabee (isolated): 3 blockers - CSP refused playback (FIXED 3b7d320480), app mounts Gemma --text-only by default (media withheld), MLLM lane refuses every goose media request (21k-token prompt > 8,192 media cap; raising it blows memory for video). NOT RELEASED. Plan + numbers: HANDOFF-omni-release.md. Gabee restored (installed Goose relaunched, single-guard OK).
- 18:35 SESSION 3 (release). Rebased on origin/main 6e34d1e378. Built (690bafef26): MEDIA PRE-PASS in goose-providers
  openai.rs (each attached audio/video clip, and images on an engine that declares audio = the MLLM lane, goes to the
  engine ALONE: one-line system, instruction + the person's words, max_tokens 1024, no tools; the agent turn carries a
  labelled "[Attached audio X - transcript made from the attachment by <model>, not the file itself:]" part; cache by
  host|model|kind|mime|sha256, 64 entries; failure = named reason; the cap 400 "exceeds the per-batch cap" now also
  triggers the images-as-placeholders retry). SIDECAR: ModelDirFacts.hears_audio (config audio_config /
  thinker_config.audio_config, the engine's model_hears_audio) -> auto text_only OFF for audio-bearing checkpoints;
  ACP MlxLocalModel.hearsAudio; the Text-only lane switch reads auto per model. Gate + ACP regen on the laptop
  (node-run workhorse-20261030-183217-35d2, ~/gate2-omni.log). Memory probe of the exact side requests (default cap,
  2-min audio + 60 s video) on Gabee (node-run workhorse-20261030-183333-ddf1, ~/omni-test/prepass_probe.out).
  Test media: /Volumes/AI-workhorse/media/goose/2026-10-30-omni-media/release-media/.
- 18:50 MEASURED (Gabee M3 Max 64 GB, gemma-4-e4b-it-8bit, app argv, DEFAULT 8,192 media cap, exact side requests):
  engine idle 8.88 GB metal. 2-min speech WHOLE = only its first 30 s heard (842 prompt tokens) -> Gemma 4 audio hears
  30 s per clip -> pre-pass cuts PCM WAV into 30 s pieces (images.rs split_pcm_wav) and the desktop re-encodes attached
  audio to 16 kHz mono WAV (utils/speechWav.ts): 4 pieces x ~800 tokens, 2-3 s each, peak 9.94 GB, every word incl.
  the 3 code words (lighthouse/volcano/saxophone). 60 s video (16 frames, 4,333 tokens): peak 14.61 GB (RSS 10.3),
  14-20 s; at the engine default temperature 2 of 3 runs were WRONG ("No video was provided", all-ALPHA) -> side
  request pinned temperature 0: ALPHA->BRAVO->CHARLIE in order on 2/2 runs (frame counts approximate). Evidence:
  Gabee ~/omni-test/{prepass_probe,var_probe,probe3}.out (copied to the SSD release-media/ at the end).
  Also fixed: waitClock vitest scanned the gitignored src/swarm-bench mirror (failed on any Mac that had packaged).
  Gate + package on the laptop: node-run workhorse-20261030-184609-0bdd (~/gate2-omni.log then ~/pkg-omni.log).
- 22:20 FINAL. v3.0.135 published 20:05 (notarized); the independent check found its transcription request carried
  the question and Gemma answered it inside the last audio piece (2 of 3 runs wrong) -> v3.0.136 (instruction only,
  quiet cuts, converted-audio 50 MB cap) built from 3.0.135 + fix (main's unfrozen bench change 1c71262a7d left out),
  proven in the packaged candidate and the INSTALLED app on Gabee (wav + mp3: full transcripts, right answers; video;
  image; peak 15.5 GB), published 21:49, installed on Gabee + workhorse, merged into main (ed0d7d1303, gate green);
  second independent check: PASS (only the "19 facts" wording, fixed: Brandt -> Brand). Evidence:
  /Volumes/AI-workhorse/media/goose/2026-10-30-omni-media/packaged-app-gabee-3.0.13{5,6}*.
