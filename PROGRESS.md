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
