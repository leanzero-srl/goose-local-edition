# Operations: clean-up and leftovers

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Clean up when done (MANDATORY, every machine)

Mihai 2026-10-27: "we always make a ton of clutter on these laptops when working with goose and we should always clean up". The laptop had reached 85G free; the leftovers came to ~300G. Last step of every job, on the Mac that did the work: `bash ~/.claude/skills/goose-clean/scripts/after-work.sh` (dry-run report; `--go` deletes only pure caches), then delete this skill's own leftovers below by EXACT path (never a glob in $HOME). KEEP `~/.cache/rapid-mlx/prefix_cache` (owner, 2026-10-27). Not essential = deletable once its job is done; nothing goes to the SSD as a parking spot.

This skill's leftovers: Weights converted or downloaded locally (`~/goose-builds/convert/out/*`, `~/.goose/models/_bf16/*`, local dirs under ~/.goose/models when it is NOT the SSD mount): delete once the copy on `/Volumes/AI-workhorse/models/goose` is verified (same `du -sk`, same file count). `~/.cache/huggingface/hub`: delete models no longer in use (Hugging Face has them). 2026-10-27: convert/out 38G, Nanbeige-33B 33G and the HF hub 31G removed from the laptop.
