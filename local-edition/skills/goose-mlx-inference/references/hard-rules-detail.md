# Hard rules: full text

Moved verbatim from SKILL.md on 2026-10-30 (skills audit). Read by section; the CORE card in SKILL.md wins on conflict.

## Hard rules (Mihai's, verbatim intent)
- **Git identity on this repo is ALWAYS leanzero: `leanzero.srl <office@leanzero.net>`** (repo-local
  `git config`; set on 2026-08-30, matches the other LeanZero repos). Never the client or
  personal identities.
- LM Studio / LM Link / `lms` surfaces are NEVER removed or reconfigured. The engine is additive.
- Tests use **qwen3.5-9b 4-bit only, freshly downloaded through our own path**. The fleet's 27B is
  off-limits for mounting.
- Before ANY model mount or bench: `python3 local-edition/mlx/gates.py mount --model-path <dir> --port <p>`
  — exit 1 = BLOCKED, stop. Session start: `gates.py snapshot`; session end: `gates.py verify-fleet`.
- Sidecar ports: never 1234/11434. Models live in ONE configurable `models_dir` (default `~/.goose/models`).
- WHERE THE WEIGHTS PHYSICALLY ARE (2026-10-11): on BOTH Macs they live on the workhorse's AI-workhorse SSD
  (`/Volumes/AI-workhorse/models/goose/{rapid-mlx,Mihai-LeanZero}`). Workhorse: `~/.goose/models/<publisher>` are
  symlinks there. Laptop: `~/.goose/models` is a read-only NFS mount (rclone serve nfs on 192.168.0.2:12049,
  Thunderbolt only; macOS SMB cannot serve the external SSD) kept by LaunchAgent net.leanzero.models-mount (machine-sync/models-mount.sh). No cable = empty
  models dir = "model not found" on the laptop, by design (on the road is cloud). Load is bounded by the SSD
  (~950 MB/s); MLX keeps no fd/mmap of the safetensors after load, so inference speed is unaffected. Downloads to
  the laptop fail (read-only): download on the workhorse. list_local_models follows symlinks since a20f78b5b.
  EMPTINESS IS NOT A MOUNT TEST (2026-10-20, review of lz/restore-off): the laptop's UNMOUNTED models dir is not
  always empty — macOS leaves `.DS_Store` on the mount point (machine-sync/models-mount.sh), a download leaves
  `.locks`, a publisher folder can outlive its last model. Before any code calls a model "gone" (and acts on it),
  the same dir must list at least one real `publisher/name/config.json` with dot-entries ignored
  (`hf::model_folder_gone`'s positive control); a dangling publisher/model link (SSD unmounted) proves nothing.
- RESTORE AT LAUNCH is the owner's tunable `GOOSE_MLX_RESTORE_AT_LAUNCH` (config.yaml, default OFF; Settings › App ›
  "Local model at launch"). The switch is read before anything else in `servingIntent`, so a broken `mlx_engine:`
  block can never turn into a red "Restore failed"; only the launch restore's read (`atLaunch: true`) may forget a
  single-engine record whose model is provably gone, and never one the engine is serving. Deleting a model clears a
  record naming that copy (here: single + split; on a Link peer: that peer's remote single, and the split when it runs
  a rank on `link:<peer>`; an ssh-alias split peer cannot be matched to a Link node id, so its record stays).
- Engine changes in `swarm.rs` go through the `swarm-surgeon` agent (see `.claude/agents/`); the six
  swarm invariants and development gates apply unchanged. The knob-turning skill's "never touch
  crates/goose/**" rule does NOT block this branch's sanctioned feature work — but swarm gates do run.
