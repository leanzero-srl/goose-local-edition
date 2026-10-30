# Omni media: audio + video input to a local model (2026-10-30)

## Contents
- What works, measured
- Wire path
- Traps

## What works, measured
- mlx-community/gemma-4-e4b-it-8bit (8.3 GB, audio + vision) on mlx-vlm 0.7.1 (the lz.18 pin): audio verbatim,
  video-as-frames correct (motion, colour change, on-screen text), audio+frames in one prompt correct. Served by
  fork branch lz/omni-media (636c6f04c): /v1/models ['text','vision','video','audio','tools'].
- mlx-community/Qwen3-Omni-30B-A3B-Instruct-4bit: audio OK on 0.7.1 AND 0.7.11; vision on 0.7.1 = "!!!!" / blank /
  Metal GPU Address Fault with audio (upstream Blaizzy/mlx-vlm #2099); 0.7.11 frames OK, audio+frames and native
  video = empty output. Vendoring 0.7.11's qwen3_omni_moe package alone under 0.7.1 does NOT fix it (the fix also
  lives in 0.7.11's generate code) -> Qwen3-Omni needs the whole mlx-vlm pin at >=0.7.11.
- Native `video` through mlx-vlm is unreliable on both versions -> the engine samples video into frame images.
- MiMo-V2.6 omni: two-Studio split only; audio not wired in the split media path.
- IN THE APP (packaged dev build, Gabee, 2026-10-30 session 2) the HTTP proof above does NOT carry over:
  (1) the sidecar mounts Gemma-4 with `--text-only` (every vision checkpoint except qwen4_exp) -> /v1/models
  ['text','tools'] -> goose withholds media; Engine > Sampling > Text-only lane OFF + Remount gives the MLLM lane.
  (2) On the MLLM lane every goose media request is refused 400 "Vision-request prompt tokens (21193) exceeds the
  per-batch cap (8192 ...)": goose's system prompt + 26 tools is ~21k tokens and the cap counts the WHOLE prompt of a
  media request (also true for images). Measured with --vision-prefill-token-budget 131072, app argv, E4B 8bit:
  video 2,512 tok 11.1 GB peak / 6,867 tok 14.0 GB (correct); audio 18,554 tok 21.7 GB, 31 s (verbatim); video ~22k
  tok > 33 GB (job killed). So raising the budget is unsafe for video; the planned fix is a goose-side media
  pre-pass (short side call per media item, text into the agent turn): HANDOFF-omni-release.md.
- Evidence: /Volumes/AI-workhorse/media/goose/2026-10-30-omni-media/ (packaged-app-gabee/ for the app probe). Branch state + release chain:
  goose omni-media HANDOFF-omni.md.

## Clip-set proof, 3.0.136 (2026-10-30 night, Gabee M3 Max, lz.19 app argv)
Harness: goose's REAL pre-pass via the ignored test `live_prepass_of_real_clips` (goose-providers openai.rs,
env OMNI_LIVE_URL/MODEL/CLIPS/OUT); input = the exact 16 kHz mono WAV the installed renderer makes
(decode.mjs over CDP: wav/mp3/m4a all decode). Scripts, clips, rows, scores:
/Volumes/AI-workhorse/media/goose/2026-10-30-omni-proof/ (run_clips.sh, score.py = WER + cut energy).
Gemma 4 E4B 8bit, WER (numbers/Mr normalised), wall, engine peak Metal:
- short say 9 s 0 %, 1.3 s | 2 speakers m4a 34 s 1 % (colour/color) | music+speech 20 s 0 % | noisy 20 s 0 %
- long say 130 s (wav = mp3 = m4a, identical) 0.7 %, 12.7 s, 2 of 4 piece boundaries damaged
- LibriSpeech real reading 135 s 6.3 % (mostly names/compounds; "really" lost at a cut), 10.6 s
- Romanian 46 s 5.6 % (names: Prahovei>Prajovei, Tâmpa>Timpa), 5.4 s
- long under pink noise 3.2 %, long under music 2.0 % (all 9 errors at the 4 cuts)
- video 20 s: colours, both words, colour change right; the moving square called static; sound track not heard
- memory: 8.9 GB idle, 9.96 GB peak on audio, 14.6 GB after the 16-frame video.
100 ms quiet-span cut (6a611e3bb4): long say 0.5 %, music 0.9 %, noisy 2.7 %; the first word of a piece is
still misheard sometimes ("The owner" > "Leona") - piece onsets, not cuts, are the residual.
Qwen3-Omni-30B-A3B-Instruct-4bit (21.8 GB, SSD models/goose/mlx-community/) on lz.19 = NOT SERVABLE for goose:
goose's pre-pass shape (text then audio) returns "\n" on every clip; audio-first works on 9 s and 15 s pieces
(with a leaked "assistant\n" prefix) but returns "\n"/"ent"/" by the" on 10, 20, 28 s and noisy pieces
(mlx-vlm 0.7.1; upstream #1768/#1770 open, latest mlx-vlm 0.7.11 fixes only frames). Missing for it: mlx-vlm
pin >= 0.7.11 (vision), a fix for empty/garbled audio output, media-before-text order. Peak 22-24 GB.

## Wire path
desktop drop/picker -> ImageData{kind,name} -> MessageContent `media` -> ACP audio block / blob resource (video)
-> goosed `MessageContent::Media` -> OpenAI `input_audio{data,format}` / `video_url` data URL (withheld behind a
named placeholder when /v1/models capabilities lack audio/video) -> fork chat.py allow_audio (checkpoint config
has audio_config / thinker_config.audio_config) -> api/media_parts.py: audio -> temp file + {"type":"audio"}
marker, video -> label + JPEG data-URL frames (1 fps, max 16; RAPID_MLX_VIDEO_FRAME_FPS / _MAX_FRAMES) ->
MLLMScheduler `audios` -> prepare_inputs(audio=). Audio requests skip pixel/feature/boundary caches (image-keyed).

## Traps
- The ignored test's filter is `live_prepass_of_real_clips` (no --exact: its path is openai::media_input_tests::...).
- zsh: `$D:a` in a string is a history modifier: write `${D}`.
- Never `npm i -g pnpm` on a Mac: use `source bin/activate-hermit` (pnpm 10.30.3).
- mlx-vlm 0.7.1 already ships mlx-audio + opencv as deps: the [mtp] env has every decoder; no extra install.
- The image loader refuses local paths in image_url: send frames as data URLs.
- /v1/models ids are SERVED names: read cfg.model_path before checking the checkpoint config.
- The engine's media cap is `--vision-prefill-token-budget` (default 8192, mllm_batch_generator
  `_prefill_cap_violation`); text-only requests on the MLLM lane are chunked and exempt.
- Replay goose-sized requests without the app: packaged-app-gabee/mkreq.py (system prompt of N tokens + audio/video)
  and budget_probe.sh (engine by hand, prints prompt_tokens, peak_memory_gb from /v1/status, answer).
- Probe quickly with `python -m mlx_vlm.generate --model <dir> --audio x.wav --image f*.png --prompt ...` inside
  `uvx --from 'rapid-mlx[mtp]@git+https://github.com/leanzero-srl/Rapid-MLX@<tag>' python ...` (the app's env).
