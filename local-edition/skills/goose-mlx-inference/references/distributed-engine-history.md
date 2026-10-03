# Distributed engine history (moved here from the mlx-jaccl-cluster skill, 2026-10-03)

Verbatim record of goose's DISTRIBUTED engine work, 2026-09-23..26, previously kept in
`~/.claude/skills/mlx-jaccl-cluster/SKILL.md`. That skill now holds only the raw two-Mac MLX/JACCL layer
(topology, procedure, measured all_sum, mlx_lm sharding, the Step 1b soak). Read SKILL.md's "The
distributed engine" section first for the current shape; this file is the dated evidence behind it.
Scripts referenced below now live in `../scripts/` (`link-live-test.sh`, `tailscale_identity.py`).

## Distributed engine — owner requirements & guardrail design (2026-09-23, not yet built)
Owner: "running exo … was crashing or kernel panicking my workhorse because it has 96gb and because of how silly this tensor sharding works … work out some guardrails and automatic memory decongestion on the nodes". And: the Providers › LeanZero MLX UI must SHOW when we're doing distributed inference.
Guardrails (every one per node, all measured, none a typed absolute):
1. Honest memory first — the sidecar's measure() under-reported (0.0 GB "free" with 88% free per memory_pressure; file cache counted as used). Fix landed/landing in goose-sidecar memory.rs (OS memorystatus / host_statistics64).
2. Pre-flight fit per node: this node's shard bytes + KV for the context we'll allow + measured runtime overhead vs the node's available memory with a headroom ratio; the pipeline split is UNEVEN, layers allotted in proportion to each node's usable memory (96 GB workhorse gets fewer layers than a 128/256 GB node). Refuse with the numbers when it can't fit.
3. Hard in-process caps on every rank: mx.set_memory_limit / mx.set_wired_limit / mx.set_cache_limit as ratios of the node's RAM (MTPLX's measured 75% allocation / 60% wired as the starting receipt) so MLX raises an allocation error instead of the kernel swapping or panicking.
4. Decongestion before mount: on each node, unload that node's idle single engine (with the owner's consent in the UI), clear MLX buffer caches; file cache is reclaimable and needs no action.
5. Runtime watchdog: sample each node's memorystatus on the poll; WARN → stop admitting new requests and shrink the prefix cache; CRITICAL → graceful stop through the local half + verify the remote half gone, reported loudly — never ride it into a panic.
6. Context/KV ceiling derived from the smallest node's remaining headroom.
UI: Engine tab + tile + tray show the mode — "Single · this Mac" vs "Distributed · N nodes · JACCL|ring" with a per-node strip (memory, layers held, state, link).

### 2026-09-23 — Flash landed
- rapid-mlx/Qwen3.8-Flash-Next-4bit: 98 GB, 28/28 shards, ~/.goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit; SHA256SUMS.txt ships in the repo — verify with `shasum -a 256 -c SHA256SUMS.txt` (log ~/goose-builds/flash-sha256-verify.log).
- Model code: rapid_mlx/models/qwen4_exp.py (+ qwen4_exp_cache.py) in our fork. Fork clone is NOT on the MacBook by default; the workhorse's ~/Projects/Rapid-MLX is the stale v0.13.4-lz.2 line. Pipeline split work: branch lz/pipeline-qwen4 cloned at v0.14.3-lz.2 into ~/Projects/Rapid-MLX (MacBook).
- Order: soak (27B) finishes → checksum OK → single-node Flash reference on the MacBook (27B unmounted briefly) → copy to workhorse over TB5 → pipeline run, token-by-token vs reference → soak.
- Checksum 2026-09-23: 33/34 OK incl. all 28 safetensors, config, tokenizer; only README.md mismatches (doc edited after the sums were cut — harmless).

## Pipeline split for qwen4_exp (Qwen3.8-Flash-Next) — built 2026-09-23, NOT yet run on two Macs
- Code: ~/Projects/Rapid-MLX (this MacBook, fork leanzero-srl/Rapid-MLX), branch `lz/pipeline-qwen4` off tag v0.14.3-lz.2. Isolated: `rapid_mlx/distributed/pipeline_qwen4.py` + `tests/test_pipeline_qwen4.py`; nothing else imports it. Venv: `~/Projects/Rapid-MLX/.venv` (mlx 0.32.2, mlx-lm 0.31.3, `-e .[dev]`) — never the jaccl-smoke venv.
- Dry run (headers only, safe): `.venv/bin/python -m rapid_mlx.distributed.pipeline_qwen4 plan --model ~/.goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit --node m4max:128 --node m3ultra:96` (GiB; add `:FREE_GIB` to use a measured free figure). Result 2026-09-23: rank0 layers [0,19) 59.9 GiB of 96 budget, rank1 [19,48) 45.7 GiB of 72 → both ~63%; full 262,144 context fits.
- Run one rank per node: `mlx.launch --hostfile h.json -- <venv>/bin/python -m rapid_mlx.distributed.pipeline_qwen4 run --model <same path on both> --prompt "..." --max-tokens N [--context C] [--split 19]`. Ranks all_gather their measured budgets and checkpoint size, so the split is computed identically everywhere and refuses on a checkpoint mismatch.
- Architecture facts that decide the split: the 51B-param PLE n-gram table is NOT model-level — it is `layers.1.ple.*` (layer id 2), 31.18 GiB at 4-bit; every other layer 1.36 GiB; embed 0.33, mixer+lm_head 0.34, mtp 1.37 and vision 0.42 excluded. Wire = the HC stream `hc_count*hidden*2 B` = 20,480 B per token per hop (+4 B token all_sum per step).
- Guardrails: `mx.set_memory_limit` is only a GUIDELINE in MLX 0.32 (docstring) — the hard stop is the per-step guard (vm pressure CRITICAL or `mx.get_active_memory()` > budget), broadcast in the step's all_sum so all ranks stop together. Wired limit = min(0.60×RAM, max_recommended_working_set_size).
- TRAP: `mlx.launch` exits 0 even when a rank died with code 1 (measured). Never trust its exit code — check the ranks' own output / artefacts.
- TRAP: local ring tests must pick `--starting-port` BELOW `net.inet.ip.portrange.first` (49152). Inside the ephemeral range a rank's outgoing connect can take a later rank's listen port → `[ring] Couldn't bind socket (error: 48)`.
- TRAP: rank 0 exiting first tears down the others (launcher cleanup) — the run CLI ends with an all_sum barrier so the last rank finishes writing its output.
- 2026-09-23 changelog: built + parity-proven locally (2 and 3 ranks, ring, 127.0.0.1): prefill logits max|diff| 0.0, 64 greedy tokens identical, batch of 2 left-padded prompts, 5 splits. Next: first real run across TB5 (JACCL) once the owner frees the 98 GB model load.

## Copying a model to the other Mac over TB5 (goose replica, built 2026-09-23)
- Product: Models tab › Downloaded › "Copy to <device> · Thunderbolt" (goose worktree branch `worktree-agent-a3ca5db6c3f20278e`, commits a0f9f8b26..faef1ceb4, NOT merged yet). Control via the LeanZero Link mesh relay (`mlxEngine/{linkFacts,replicaTargets,replicate,replicaPull,replicaProgress,replicaCancel}`); bytes go straight over the cable: the sender binds a listener on 192.168.0.1:<ephemeral> ONLY, the receiver pulls file by file (.part → rename, Range resume, per-file SHA-256 vs the sender's own digest) into its ~/.goose/models/<org>/<name>.
- TB detection = the HARDWARE PORT name ("Thunderbolt N" in `networksetup -listallhardwareports`), not enN; speed = `system_profiler SPThunderboltDataType -json`, receptacle N ↔ port "Thunderbolt N" (measured on both Macs: laptop port 3/receptacle 3, Studio port 2/receptacle 2, both "80 Gb/s").
- Without goosed/mesh (e.g. to stage weights for a JACCL run): build `cargo build -p leanzero-link --release --example replica_probe` in that worktree, scp the binary to the workhorse, then
  `replica_probe serve 192.168.0.1 <model-root> <org/name>` on the MacBook (prints source= and token=), and on the workhorse
  `replica_probe pull <source> <token> <org/name> ~/.goose/models` (or `/tmp/jaccl-smoke/models` parent for JACCL). It exits when the receiver releases the offer.
- MEASURED 2026-09-23 22:39 (JACCL soak + a 105 GB HF download running): 1.61 GB synthetic over TB5 = 2.42 GB/s on the wire, 2.04 GB/s end to end incl. both SHA-256s; the same code over Wi-Fi LAN = 0.093 GB/s (26x slower). TCP over the TB IP is MTU 1500, single stream — far below the 9.3 GB/s RDMA figure; jumbo MTU / parallel streams are the untested levers (changing MTU is network config: ask first).
- SUPERSEDED by 84e738cf6 (every mesh peer is dialed through tailscaled's loopback SOCKS5 listener, `peer_dial.rs`; the 2026-09-24 hermetic two-Mac e2e ran the Link control plane over it). Historical: TRAP (existing Link defect, not replica): tailscaled runs `--tun=userspace-networking` with no `--socks5-server`/`--outbound-http-proxy-listen`, and the peer fabric reqwests `http://<mesh ip>:41226` directly — the host has NO route to LeanZero mesh IPs (`route -n get 100.64.0.2` → en0 default gateway). Per https://tailscale.com/kb/1112/userspace-networking outbound needs the proxy. So the mesh relay (and replicate's control plane) is unproven between two real Macs; the TB byte path is proven.
- Fork test baseline (2026-09-23, `pytest tests --ignore=tests/integrations`, integrations need anthropic/pydantic_ai/smolagents): 25 failed + 13 errors are PRE-EXISTING (deepseek_v41 dspark, model_registry, gemma4 tokenizer, etc. — need real models); before 25,602 passed, after 25,616 (+14 pipeline tests), identical failure set. qwen4 files: 140 passed / 2 skipped before and after.
- Pipeline split (2026-09-23): fork branch lz/pipeline-qwen4 (head fb912e632), rapid_mlx/distributed/pipeline_qwen4.py + tests/test_pipeline_qwen4.py, own venv. Only the 4-stream hyper-connection state crosses ranks: 20,480 B/token/hop + 4 B sampled token. The 51B n-gram table lives INSIDE layer 2 (31.2 GiB), not model-level. Budget = min(kern free×0.90, RAM×0.75), wired 0.60×RAM; mx memory limit is advisory in 0.32 → a per-step guard stops all ranks on the same step. Tiny-model parity: logits diff 0.0, 64 greedy tokens identical, 2/3 ranks, chunked prefill. Real-model dry run 128+96: rank0 L0–18 (59.9/96), rank1 L19–47 (45.7/72). Text-only (vision + MTP dropped).
- TRAPS: mlx.launch exits 0 even when a rank dies — check each rank's own log/exit. Local ring ports in 49152+ collide.

## Flash (qwen4_exp, 95.7 GiB text) on BOTH Macs — measured 2026-09-24
- Layout: both Macs carry `/tmp/flash-pipe/{repo,model,results}` symlinks (MacBook: ~/Projects/Rapid-MLX, ~/.goose/models/rapid-mlx/Qwen3.8-Flash-Next-4bit, ~/goose-builds/jaccl-smoke; workhorse: ~/Projects/Rapid-MLX-pipeline (clone of lz/pipeline-qwen4 — NOT the stale ~/Projects/Rapid-MLX), same model path under /Users/workhorse, ~/flash-pipe-results). /tmp is wiped on reboot: recreate them.
- Launch: `~/Projects/Rapid-MLX/scripts/flash_pipeline_launch.sh <jaccl|ring> <out-name> --max-tokens 128 --context 8192 [--split N] [--soak-minutes M] [--layer-limit N]`; then `scp "workhorse:flash-pipe-results/<out-name>/*"` back and `scripts/flash_pipeline_harness.py compare --ref A --pipe B`.
- Planner split from live memory (MacBook 92.7 GiB avail, workhorse 61.6): MacBook layers [0,20) 57.3 GiB weights, workhorse [20,48) 38.4 GiB. Load 9.5 s per side from warm file cache.
- Speeds (batch 1): prefill 2k prompt 683 tok/s JACCL / 716 ring; decode 21.3-23.7 tok/s both backends (compute-bound, one 20 KB hop per token). Batch 2: ~47 tok/s aggregate.
- Peak MLX memory: MacBook 61.0 GiB vs guard budget 83.4 (margin 22.4); workhorse 42.5 vs 55.5 (margin 13.0). Min node available during soak 27.3 / 21.0 GiB; vm pressure stayed normal (1).
- Parity: JACCL vs ring and split 20 vs split 13 (7 layers moved M4 Max→M3 Ultra) give BIT-IDENTICAL top-5 traces over 5 prompts × 128 tokens, so the two GPUs compute these kernels identically. Real-weight single-process vs split: first 8 layers (--layer-limit 8, 41.4 GiB) single on MacBook vs split 4 across both Macs = bit-identical traces (31 exact bf16 ties included).
- Full single-node reference does NOT fit on the MacBook with normal apps open: needs 96.4 GiB (98+ with the fork loader's MTP), measured available 97.0 → x0.90 headroom 87.3. The harness refuses (exit 3) and writes REFUSED.log. Needs ~12 GiB of apps closed.
- Soak 20 min JACCL: 261 iterations (131 single, 130 two-request batches), 0 rank deaths, 131/131 single requests reproduced their first-run tokens. Two-request left-padded batches match single-request greedy in only 52/260 rows (a batching-numerics property of the base model — the split itself is bitwise; not yet investigated).
- TRAP: `MACBOOK-FREE` appeared while ANOTHER session's `mlx_lm.benchmark` 27B (30 GB RSS) was still running. Check the processes (`ps -axo command | grep -E "mlx_lm|mlx\.launch" | grep -v grep`), never only the flag.
- TRAP: a wait loop that runs `pgrep -f "mlx.launch|..."` from `zsh -c` matches ITSELF (its own command line holds the pattern) and never reports clear.

## goose's DISTRIBUTED engine — BUILT 2026-09-24 (goose main, crates/goose-sidecar/src/distributed/)
- What exists: `goose_sidecar::distributed` (config, exec, probe, plan, preflight, launch + embedded `rank_wrapper.py`, supervisor) and ACP `_goose/unstable/mlxEngine/distributed{Status,Preflight,Start,Stop,ConfigUpdate}` (crates/goose/src/acp/server/mlx_distributed.rs), config key `mlx_distributed`, capability `mlxDistributed`. The single engine (engine.rs, Sidecar) is untouched apart from three visibility bumps; `mount` refuses (`distributedEngineActive`) while distributed owns the Mac, `distributedStart` refuses (code `singleEngineMounted`) while the single engine is mounted. goosed's exit teardown stops distributed ranks too.
- goose LAUNCHES THE RANKS ITSELF, not mlx.launch: per rank `MLX_RANK` + `MLX_IBV_DEVICES` (a FILE path holding the JSON matrix) + `MLX_JACCL_COORDINATOR` (or `MLX_HOSTFILE` for ring) — exactly what launch.py writes. Peer rank via `ssh -tt workhorse 'echo GOOSE_RANK_PID=$$; exec <python> -c <boot> <wrapper b64> <spec b64> goose-distributed-rank'`. The pid echo survives `exec`; killing the local ssh client kills the remote rank (pty HUP, measured <1 s). Proven over JACCL on the 27B first try.
- MEASURED: without mlx.launch a peer rank does NOT exit when rank 0 exits — stop = SIGTERM rank 0 → observe peer pid over ssh → SIGTERM it → SIGKILL after 5 s grace. Stop verified in ~1 s. A SIGSTOPped peer shows ps stat `Ts+`; SIGTERM still took it.
- MEASURED 27B tensor (qwen3_5, mlx_lm 0.31.3 server, JACCL): preflight 2.1 s; ready 3.1–9.3 s warm; per-rank peak 18.96 GB (MLX's own counter) vs plan 32.93 GiB at context 262,144 (prompt cache bound = one full context of KV); caps applied: MacBook memory 96 / wired 76.8 GiB, workhorse 72 / 57.6 GiB. Completion through goose's `omlx` provider: "Paris" in 1.17 s.
- MEASURED failure injection: SIGSTOP peer → `rankFrozen` → verified stop → restart → ready again in 9.6 s; SIGKILL peer → ssh session exits 255 → `rankDied` → restart in 9.3 s.
- Preflight traps: system_profiler -json prints NO trailing newline — every section marker is `echo; echo @@name`. `mlx_lm.server` answers `/v1/models` from the HF cache and a request naming another model makes every rank try to load it — the wrapper serves only the goose id and maps it to each rank's OWN --model path (`_model_map[served] = cli_args.model`), so per-node model dirs work. mlx_lm.server's main() sets `set_wired_limit(max_recommended)` itself — caps must be applied after it (the wrapper patches `server.run`).
- Flash (qwen4_exp) through goose: preflight runs the fork planner (`pipeline_qwen4 plan`, node figures ÷ 1.10 overhead) → [0,19)/[19,48), SHA256SUMS identical on both; START IS REFUSED BY NAME — the fork (lz/pipeline-qwen4 up to 4b94373cd) offers `{plan,run}` only, no OpenAI `serve` entry. That server is the next piece of work (fork side).
- Live tests: `cargo test -p goose-sidecar --test distributed_live -- --ignored --nocapture` (preflight dry run, start/complete/stop + wrapper surface, frozen/dead peer restarts, Flash dry run) and `cargo test -p goose --test mlx_distributed_acp_test -- --ignored` (ACP + omlx provider). Before any live start: nothing else may run `mlx_lm.server|pipeline_qwen4|mlx.launch` on either Mac (preflight's `foreignEngines` refuses anyway).
- 2026-09-24 (LIVE, 27B over JACCL) HANG RULE mid-stream — `hang_ratio_only: true` turns off the ps-stat-T fast path so only the ratio rule can see a SIGSTOPped peer. Event verbatim: "progress-ratio rule: samples 5, median 2183 ms, bound 21830 ms (10× median), silent 22323 ms — the rank-0 step counter (last Some(94)) and every rank's CPU time stood still; rank ps stats [Some("R"), Some("Ts+")]". Client: 33 chunks then EOF with NO [DONE] 23.3 s after the freeze (never a hung socket); `streamWithoutDone` event names the cut request; verified per-pid stop; restart; next request 200 at 30.6 s. Poll median ≈ 2.18 s (the 2 s cadence + ~0.19 s ssh).
- STREAM CUT (SIGKILL peer mid-stream): client EOF, no [DONE], 2.06 s after the kill; `rankDied` (ssh exit 255) + `streamWithoutDone`; restart 9.4 s; next request 200.
- WATCHDOG on real pressure: ballast = a python WE own on the workhorse (`os.urandom(1 GiB)` per step — incompressible; 3.8 s per GiB), killed by its pid. Reserves RAISED via config (`watchdog_warn_ratio` 0.467 = 44.8 GiB, `watchdog_critical_ratio` 0.363 = 34.8 GiB) so neither crossing came near the kernel: pressure level stayed 1 throughout. WARN at ballast 8 GiB (avail 44.7) → admission closed; a new request got 503 "not admitting new requests: workhorse: kernel pressure normal, available 44.7 GiB …"; the in-flight stream finished (1501 chunks + [DONE]); ballast killed → admission reopened, 200. CRITICAL at ballast 18 GiB (avail 34.3) → verified stop, restarts 0 with restart_on_failure on. After killing ballast 2: available 71.2 GiB vs 71.3 idle.
- TRAP: `kern.memorystatus_level` did not follow the ballast (68% at every step with 8 GiB held; 86–87% with 18 GiB) — never gate on it; the vm_stat/host_statistics64 figure fell ~0.5 GiB per GiB held.
- The swarm router's MlxSidecar probe now targets the distributed engine while THIS process's manager runs it (`probe_mlx`, 6 lines); another goosed or the CLI still sees only the configured single port (not live-proven).
- Tests: `cargo test -p goose-sidecar --test distributed_live -- --ignored --nocapture live_hang_rule` / `live_watchdog` (242 s; ~25 GiB ballast max).

## goose's distributed engine SETS ITSELF UP — 2026-09-24 (night), commits ff38b70f9 d61595bb6 66a28a80f
- UI: Providers › LeanZero MLX › Engine › Distributed › "Set up" → ONE field (ssh alias; answering ~/.ssh/config aliases offered as chips) → Detect → every field with its evidence, gaps in red, raw fields under Advanced → "Save and provision" → Preflight → Start.
- ACP: `mlxEngine/distributedPeerCandidates` (Host lines, each probed `echo GOOSE_PEER_SHELL; hostname -s` — bitbucket.org accepts the key, ignores the command, exits 0 with a banner: only a marker-first answer counts), `distributedDiscover {peers, modelId?}`, `distributedProvision {config?}` (progress on `distributedStatus.provision`). Code: crates/goose/src/acp/server/mlx_distributed_discover.rs (probe script + compose), crates/goose-sidecar/src/distributed/provision.rs.
- Detect sources: ComputerName (scutil), link = netpath (`parse_ifconfig_addrs` for a peer → `facts_from` → `choose_path`), netmask from prefix, service from -listnetworkserviceorder, `ibv_devinfo -v` (all devices; rdma_<iface>, PORT_ACTIVE, GID index), models = config.json dirs under the goose models dir, `~/<dir>/models` (TCC folders skipped: never list Desktop/Documents/Downloads/Library…) and the HF cache, matched by dir NAME + loaded files + sizes (+SHA256SUMS); ports = first free (lsof) above the single engine's port, coordinator below every node's ephemeral range.
- Real data: the workhorse holds the 27B at ~/jaccl-smoke/models AND ~/.lmstudio/models/leanzero (same weights, different README) — Detect picks the first and names the other. Both Macs also hold the Flash, so with no preference Detect names "2 models on every node — pick one"; the saved config's model is the preference on "Detect again".
- PYTHON: goose-managed venv per node `$HOME/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12` (fork env `rapid-mlx-pipeline-qwen4-py3.12` for qwen4_exp). uv found via `zsh -lc 'command -v uv'` first (workhorse Homebrew lives in .zprofile), then ~/.local/bin, /opt/homebrew/bin, /usr/local/bin, ~/.cargo/bin; no uv = loud fail naming those. MEASURED workhorse cold (env absent, mlx/mlx-lm/mlx-metal cleared from uv cache): 7.1 s; re-run "done already" 1.5 s; MacBook 5.1 s. Live: `cargo test -p goose-sidecar --lib live_provision -- --ignored --nocapture` (GOOSE_PROV_HOST=alias).
- MODEL PATHS: per-node paths already work (goose launches ranks itself; wrapper maps the served id to each rank's own --model) — the /tmp/jaccl-smoke same-path rule is mlx.launch-only. Missing on a peer → preflight names the Thunderbolt copy (needs Link sign-in; no button without it).
- PREFLIGHT change: a foreign SINGLE MLX server (the owner's rapid-mlx on :8090) is a WARN (its memory is already outside `available`); a foreign mlx.launch / pipeline_qwen4 / goose rank stays a FAIL.
- E2E (packaged app beside the installed one, 27B still on :8090): Ready 10 s (5 s warm on the 2nd run), per-rank peak 21.6 GiB (MacBook plan 36.2 of 44.2 GiB budget with the :8090 copy resident, available ~28 GiB while both 27Bs were up), bare completion 63+21 tok in 3.4 s, `goose run --provider omlx` 69–73 s (goose's agent prompt), stop verified ~1 s, 0 ranks left.
- Fixtures: `cargo test -p goose --lib capture_discover_fixtures -- --ignored` re-captures both Macs; `export_discovery_ui_fixture` rewrites the desktop's fixture from them.
- TRAP (2026-09-24, 3.0.19 live): macOS LOCAL NETWORK PRIVACY. The installed app opened normally (Finder/`open -a`) is denied LAN addresses (192.168.0.x over TB) — preflight's /sbin/ping fails; the SAME binary launched from Terminal passes (Terminal is the TCC responsible process and holds the permission). So any E2E proof run from a shell-launched app proves nothing about this. Affects ping, the rank processes, the JACCL coordinator TCP and the TB model copy. Tailscale 100.x and loopback are unaffected. Fix in flight: NSLocalNetworkUsageDescription + a prompt trigger + a named `localNetworkPermission` preflight check.
- TRAP: the distributed mlx_lm.server reports the model FOLDER as its id, the single engine serves a derived `--served-model-name` (e.g. mihai-qwen3.8-27b-atlassian-q8-mlx) → the swarm router refused the node ("No node can answer"). One naming function for both engines (fix in flight).
- 3.0.19 live via the UI: Detect 1.7 s (18 fields), Save+provision 3.9 s, Preflight pass, Start → Ready 10 s, Stop verified 1 s.

### 2026-09-24 — ONE served-id rule for both engines (d8f60daba, ea8ee809f) — supersedes the served-id TRAP above
- The ranks serve `engine::served_model_id(mlx_engine settings, config.model_id)` — the SAME function behind the single engine's `--served-model-name`. distributedStart reads the saved `mlx_engine` block (unreadable = error) and passes it to `DistributedManager::start(config, served)`; `DistributedConfig.model_id` is only what the ranks LOAD. Status/DTO carry `servedModelId`. Live: /v1/models on :8191 = `mihai-qwen3.8-27b-atlassian-q8-mlx`, owned_by goose-distributed; the HF id → wrapper 404 "not served here".
- mlx_lm 0.31.3 with an unknown request `model` (MEASURED standalone, SmolLM-135M, HF_HUB_OFFLINE=1): `load()` → `_model_map.get(name,name)` → `_load()` which DROPS the loaded model first, then fails the lookup → HTTP 404; the next default request reloads. A cached HF id (Llama-3.2-1B) is LOADED and swapped in (200). In a distributed run either would be rank-0-only → desync; the wrapper's validate refuses every id but the served one.
- UI: `acp/mlx-distributed.ts` keeps the latest status ANY read saw (failed read → null); ComposerReadiness/NoNodeNotice use `distributedFact` (mlxMount.ts) — owns the Mac + ready/serving + servedModelId === node id → ready; else "Distributed · 2 nodes · JACCL · <State> — <node>", never Mount. The tray reporter joins its while-owned loop when any read publishes an owning status.
- The router switch (`probe_mlx`) sees only THIS goosed's distributed manager. Each desktop window spawns its OWN goosed (main.ts createChat → startGooseServe), so a second window's chat probes the configured single port, not the distributed one — open gap.
- Live recipe that worked (packaged binary, isolated root, CDP 9444): seed config with `mlx_engine.port: 8190` + swarm device mihai-mlx; add a project without the native picker: `window.electron.addProject('<dir>')` over CDP, then "New session in work"; Set up → type alias → Detect → pick model (role=option) → Save and provision → Engine › Distributed › Start; Stop opens a CONFIRM dialog (click its "Stop"). Chat turn: router log `pick node=mihai-mlx model=mihai-qwen3.8-…` host :8191, reply "The capital of Romania is Bucharest."; stop verified, 0 ranks on both Macs.
- CROSS-WINDOW (9d45fa088): the owning goosed publishes `<state dir>/mlx-distributed-owner.json` {pid, base_url, served_model_id, model_id, backend, node_names} on an accepted start; withdraws it on stop, on a status read finding its run over, at exit. Readers (providers/mlx_distributed_owner.rs): Mine/Other/Stale(pid dead, named+ignored)/Unreadable(named, router refuses)/Absent; "up" = the engine's own /v1/models. Router + OMLX_HOST follow Other; other windows' start/stop/single-mount refuse `ownedByAnotherWindow` (a stop without a run SWEEPS ranks by marker — it would kill the owner's run). Status DTO `owner` {answering|notAnswering|stale|unreadable}. Live: 2 windows = 2 goosed (40579 owner, 44900); window 2 chat routed to :8191 ("Budapest"); window 2 Start → "owned by another window (goosed pid 40579)"; Stop in window 1 → window 2's banner flipped to "No model is mounted" within 2 s, record removed, 0 ranks. Second window over CDP: `window.electron.createChatWindow({dir})`, then target the new page id.

### 2026-09-24 — macOS LOCAL NETWORK PRIVACY was why the app's preflight ping failed (90de1b9e2, cd7907158)
- Symptom: Goose Swarm opened from Finder / `open -a` → preflight `ping 192.168.0.2` fails; the SAME binary started from a terminal passes. TN3179: "Command-line tools run from Terminal or over SSH, including any child processes they spawn" are exempt — a shell-launched app has the TERMINAL as its responsible process (Warp holds Local Network = allowed). Every earlier "E2E pass" of the packaged binary run from a shell PROVED NOTHING about the real app. Peers probed over ssh are exempt too.
- Root cause measured: the main executable carried stock Electron 41.0.0's LC_UUID (4C4C4450-5555-3144-A175-A5A5EB513DF3, identical to node_modules' Electron.app). TN3179: shared UUID → "may behave weirdly". Same 3.0.19 bundle with only its UUIDs rewritten + re-signed (Developer ID) → `192.168.0.2 ok` from `open`. Fix: forge `packageAfterCopy` rewrites every bundle executable's UUID (scripts/unique-macho-uuid.cjs) + NSLocalNetworkUsageDescription + main-process UDP-connect trigger (`window.electron.touchLocalNetwork()`).
- Refused signature (measured with the new build re-stamped to the stock UUID): `ping: sendto: No route to host`; preflight now names `localNetworkPermission` when this Mac's pings all say EHOSTUNREACH AND the ssh-probed peer pinged us back. A local rank dying on EHOSTUNREACH → event `localNetworkBlocked`, no restart. Replica pull → `localNetworkBlocked` flag.
- Tools: responsible pid = `responsibility_get_pid_responsible_for_pid()` (tiny C, scratchpad `lnp/resp`); per-app state = `/Library/Preferences/com.apple.networkextension.plist` NEPathRule by SigningIdentifier: (DenyMulticast, MulticastPreferenceSet) = (False,True) allowed, (True,True) denied, (True,False) undetermined. No way to reset on macOS.
- Test launch that is NOT Terminal-attributed: `open -n -a <App> --env GOOSE_PATH_ROOT=… --env GOOSE_USER_DATA_DIR=… --args --remote-debugging-port=<not 9333>` (open --env passes env through LaunchServices).
- macOS 26.7's Privacy & Security extension has NO `Privacy_LocalNetwork` anchor: the URL opens Privacy & Security, not the Local Network sub-pane. `osascript` to "System Settings" while the screen is locked HANGS (automation prompt) — don't.

## Full-model reference without 98 GB resident — layer streaming (2026-09-24)
- `scripts/flash_pipeline_harness.py stream --shape decode [--pair-singles] --pipe-dir <pipeline run> --out <dir>` holds ONE decoder layer at a time (lazy safetensors reads, drop, `mx.clear_cache`). Peak 33.8 GiB (the 31.2 GiB PLE layer + 2 GiB transient); ~2 s per 1.36 GiB layer; whole run ~2 min on the M4 Max.
- `--shape decode` replays greedy decode's exact shapes per layer (prompt minus last token in 2048 chunks, then one token at a time on its own cache; head per step on (batch,1)). Layer-major order is legal: layer i at step t needs only layer i-1 at step t plus its own cache.
- RESULT: full 48-layer single process vs the 2-Mac JACCL pipeline = 640/640 greedy tokens, top-5 logprobs identical to the 6th decimal. The split is exact on the real model.
- `--shape prefill` (one forward over prompt+generated) is NOT a parity reference: 620/640 argmax matches, flips only at margins <= 0.5, max |dlogprob| 4.1 on tail top-5 entries — prefill kernels (chunked GDN, multi-row qmm) vs decode kernels. Also lm_head over N rows vs 1 row changes argmax at exact bf16 ties. Compare like shapes only.
- Batching isolation (`--pair-singles`, full model, single process): a left-padded batch of 2 teacher-forced on the single-request tokens shifts the top-1 logprob by median 0.0005-0.02, max 0.40, and flips argmax 1-8 times per 128 at margins <= 0.625. That base-model behaviour is why free-running two-request batches matched single requests only 52/260 in the soak. Truncated 8 layers: pipeline batches == single-process batches, 10/10 tokens and traces.
- MEASURED: one decoder layer's prefill forward peaks 49x the chunk's HC-stream bytes above its weights (1.96 GiB for 2,101 tokens; MoE ~1.55 of it) — now `LAYER_TRANSIENT_STREAM_MULTIPLE` in the planner. The old guess said 0.33 GiB.
- TRAP (killed a real run): a lazy `recv` fused into GPU work makes a Metal command buffer wait on the transfer; if the upstream rank takes >~5 s (3 s ok, 6 s fails) the downstream rank dies with `kIOGPUCommandBufferCallbackErrorTimeout`. `receive_stream()` evals the recv on the host first. Any MLX pipeline code (mlx-lm's included) has this exposure when a stage is slow.
- TRAP: macOS went to vm pressure WARN on the M4 Max at 19.4 GiB available (15% of RAM) after a 45 GiB load; the budget is now available - 21% of RAM (lowest share measured normal).
- The goose app's 27B engine on :8090 was mounted (31 GB footprint) during this work and was left untouched; with it up the MacBook has ~60 GiB available, too little for the full pipeline split but enough for streaming.
- 2026-09-24 Local Network, part 2: on macOS 26.7 the id com.electron.goose sat in /Library/Preferences/com.apple.networkextension.plist as undecided-deny (DenyMulticast true, MulticastPreferenceSet false, Path null) and macOS NEVER raised the alert (UDP connect, renderer WebSocket, main fetch — only "LocalNetwork: found bundle id … by PID/UUID" in UserEventAgent; read it with `/usr/bin/log show` — plain `log` is a zsh builtin here). Answered apps carry MulticastPreferenceSet true + Path. Unique Mach-O UUIDs alone did not fix it. Fix: new bundle id net.leanzero.goose-swarm (3.0.21). Test LAN reach from the app: `window.electron.fleetProbe('http://192.168.0.2:<port>')` runs in MAIN (renderer fetch is CSP-blocked for LAN — inconclusive).

## Flash OpenAI server — `pipeline_qwen4 serve` (fork 272cb064, 2026-09-24)
- `mlx.launch --hostfile /tmp/flash-pipe/hostfile-jaccl.json --cwd /tmp/flash-pipe/repo -- /tmp/flash-pipe/repo/.venv/bin/python -m rapid_mlx.distributed.pipeline_qwen4 serve --model /tmp/flash-pipe/model --served-model-name <id> --port <p> --context 32768 --max-batch 2 [--split 19]`. Rank 0 prints `PIPELINE_READY {..."pid"...}` after kernels are warm; stop = SIGTERM that pid (every rank exits in <1 s; verify the remote pid over ssh). goose passes `emit` so the lines become `GOOSE_RANK_GROUP / GOOSE_RANK_CAPS / GOOSE_READY`.
- Surface: /v1/models (only the served id + context_window + tool/reasoning parser), /v1/chat/completions (SSE role → deltas → finish+usage → [DONE]; non-stream), /v1/status, /goose/progress, /goose/admission. Other model → 404. Images → named 400 (text only).
- Parsers: the checkpoint template's XML contract → qwen3_coder_xml, `<think>`+enable_thinking → deepseek_r1 (goose model_parsers.rs's rule), through the route layer's own StreamingPostProcessor. Measured on Flash: tools work streamed and non-streamed (`get_weather {"city": "Bucharest", "unit": "celsius"}`), reasoning_content carries the thought.
- TRAP (fork behaviour): deepseek_r1 streaming turns a tagless stream into CONTENT after 64 chars (NO_TAG_CONTENT_THRESHOLD) unless `_prompt_primed_thinking` is set; the template opens `<think>` in the prompt so Flash never emits it — the server sets the hook via `_should_start_in_thinking`. The single engine through goose likely shows the same leak on qwen3_5 streams (unverified there).
- Real Flash over JACCL: ttft 0.38 s on a 64-token prompt, decode 21.9 tok/s streamed; a queued pair shares decode steps (264 steps for 64 + 281 tokens).
- COST: an idle worker rank spins ~99% of one core (JACCL poll inside the header all_sum while rank 0 waits for requests); rank 0 idles at 0%. Not fixed.
- TRAP: a signal handler that does `queue.put` deadlocks if the main thread holds the queue lock inside `get()` — hand the put to another thread's loop (`call_soon_threadsafe`).

### 2026-09-24 — goose runs the qwen4_exp PIPELINE through the fork's server (goose-sidecar distributed/)
- Fork pinned BY COMMIT: `provision.rs` `PIPELINE_FORK_COMMIT` = 272cb0643 (lz/pipeline-qwen4). The env proof prints `mlx mlx_lm <commit>` (commit from the dist's PEP 610 `direct_url.json`; uv writes it for git installs — measured), so an env on an older pin FAILS the proof and the next provisioning upgrades it in place (measured: e7d49b3 env → reinstall → 272cb06). Preflight's runner check: `serve` in `--help` (argparse wraps `{plan,serve,run}` to line 2) + a managed env on the pinned commit (stale managed = FAIL, operator's own interpreter on another commit = WARN).
- PLAN: `python -m rapid_mlx.distributed.pipeline_qwen4 plan --json --model <rank0 dir> --node NAME:RAM_GIB:AVAIL_GIB ... --batch 2 [--context C]` — exit 0 fits, 2 does not (JSON either way). goose reads stages' bytes/budget/fits VERBATIM (no 0.90 headroom, no overhead multiplier). Fork budget = max(0, min(avail − RAM×0.21, RAM×0.75)).
- Derived context = the planner's fixed point: plan at full context → its split's `max_context` → re-plan there (re-balances) → repeat until `max_context == context`. MEASURED (Flash, 128:90 / 96:67, batch 2): 262144 (no fit, ceiling 16308) → 16308 → 73216 → fixed. The old two-pass would have stopped at 16308.
- RATIO DECISION: soak peaks (MacBook 61.0 / workhorse 42.5 GiB; the soak ran 130 two-request batches) vs the fork's CURRENT plan for that shape (split 20, ctx 8192, batch 2) = 61.50 / 42.66 GiB → 0.992 / 0.996. So RUNTIME_OVERHEAD_RATIO 1.10 is tensor-only now; multiplying the pipeline plan would double count. The same re-plan on the soak's recorded free figures says the workhorse does NOT fit (42.66 > 41.44 budget): that soak ran under the new 21% floor.
- FORK DEFECT (measured): a node whose available is below RAM×0.21 makes the planner crash — budget 0 → `ZeroDivisionError` in `StagePlan.utilization`, exit 1, no JSON. goose names it (exit, `--node` figures, last stderr lines) as a `plan` FAIL.
- LAUNCH: rank program = `rank_env.py` (backend env, HF_HUB_OFFLINE, emit, RANK_MEM reporter) + `pipeline_rank.py`, under `NodeConfig.pipeline_python`: parses `--model <node dir> --served-model-name <served> --host 127.0.0.1 --port <port> --context C --max-batch 2 --split <starts[1..]>` with the fork's OWN `add_arguments`, then `serve(options, emit=emit)`, then `os._exit(code)`. It never calls mx.distributed.init (serve does; MLX init("any") tries ring → mpi (size-1 world = nullptr) → jaccl, so the ONE backend env the prelude sets decides it).
- STOP (pipeline): SIGTERM rank 0 → its shutdown broadcast → the stop WAITS the grace window for the peer pid over ssh before any per-pid SIGTERM (tensor peers are still signalled after one look).
- 2026-09-24 SLOTS (goose ffa5ef671, fork ea6f8dee1): `DistributedConfig.slots` (default 2 = PIPELINE_DEFAULT_SLOTS) → planner `--batch S`, serve `--slots S --max-batch S`; `plan --json` gained only `"slots"`. Rank 0 /v1/status: num_running, num_waiting, slots, slots_in_use (worst rank's reservation in slot units, ceil), sequences_in_flight, kv_reserved_bytes ([] = idle), kv_budget_bytes (per rank). goose DTO: status waiting/slots/slotsInUse/sequencesInFlight/serverStatusError, node kvReservedGb/kvBudgetGb (tensor: slots fields absent). Batch 4 on Flash 128:90/96:67 ctx 32768 does NOT fit (72.2 vs 67.8 GB rank 0).

## Flash from goose's UI + KV slots (2026-09-24)
- goose main: a143a248e (runner → fork serve), d3dd305a4 (pin 9f861d9e1), d84adf4bd (derived context walks on available − 2% RAM), ffa5ef671 (slots). Fork pin now ea6f8dee1 (`--slots S --max-batch N`, /v1/status slots/slots_in_use/sequences_in_flight/kv_*).
- LIVE (packaged app, isolated GOOSE_PATH_ROOT + GOOSE_USER_DATA_DIR, CDP 9447): Set up → workhorse → Detect → pick Flash → Save and provision (fork env installed on both nodes in 8.6 / 11.9 s) → Preflight → Start → Ready in 17 s → chat on the swarm provider answered by Flash (thinking separated) → streamed 21.7 tok/s → Stop "verified" (rank 0 SIGTERM exit 0 in ~0.9 s, rank 1 left on the shutdown broadcast in 0.2-0.4 s, verified over ssh).
- 27B alongside (mounted in the isolated app on :18850, 31 GB footprint): preflight REFUSES — MacBook 59.1 GiB available → budget 32.2 GiB; best split rank 0 [0,7) needs 47.6 (148%), rank 1 [7,48) needs 74.4 of 49.2 (151%). Flash needs the 27B unmounted.
- TRAP (fixed d84adf4bd): a derived context planned at 100% of budget refuses at launch — the ranks re-measure 16 s later and see 0.5-0.6% of RAM less. The derivation now leaves 2% of RAM.
- TRAP: adding a swarm "LeanZero MLX" node sets mlx_engine.served_model_name; a running distributed engine keeps its old id and the composer says "the node wants …" — Stop/Start makes it serve the node's id.
- To start a chat on an isolated profile without the native folder dialog: `window.electron.addProject('<dir>')`, reload, click "New session here — <dir>".
- KV slots on real Flash (--slots 1, context 4096): two concurrent 2,177-token prompts ran one at a time — the second waited 20 s (never >1 in flight), both completed (233 tokens each, finish stop).
- Final UI run (goose main dce1ccec2, fork ea6f8dee1): re-provision is named when the env's fork commit is stale ("the goose-managed env is stale; provision it again") → Detect again → Save and provision (2.2 / 5.5 s). Start → Ready in 20 s, context 62,976 derived, "batch 2 = 2 full-context slots". Three concurrent 2.2k-token prompts: engine card "Slots 1 / 2 · Waiting 2 · KV 2.0 of 5.3 / 2.1 of 6.9 GiB"; A alone, then B+C batched; all three finished. Chat turn answered (389 decode steps). Stop verified (rank 0 exit 0 in 0.7 s, rank 1 on the broadcast in 0.18 s).
- NOT DONE: idle worker rank spins ~99% of a core (JACCL poll); vision path absent; batches only form between batches (no mid-batch join).
- FIXED (fork 286ed77f7, goose 42cb7ebbf): the idle worker spin. It was the header all_sum busy-polling while rank 0 waited for requests (rank 1: 60.17 CPU-s per 60 s idle over JACCL). Rank 0 now rings a one-byte TCP doorbell (address = JACCL coordinator / ring host 0, port shared by all_sum after warm-up) before every batch; workers park in recv → 0 CPU-s idle. SIGTERM or SIGKILL of rank 0 ends the blocked worker (socket closes). Measure CPU with `ps -o time=` deltas, not `%cpu` (a decaying average).

## goose's distributed engine over LeanZero Link — BUILT 2026-09-24 (3f0f64f1e..8670bfdb2)
The peer is NOT headless: it runs goose with Link signed in (same account) and the owner's switch
"Allow this Mac to serve as a distributed node" ON (config key == env `LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE`,
default OFF, read per request — flips without a reconnect). The ssh path is unchanged beside it.
- NAMING: a Link node is `link:<node id>` in `NodeConfig.ssh`, so every `node.host()` site routes to the peer.
- WIRE: `POST /v1/swarm/distributed/<op>` on the peer's control service (bearer node token or `?token=`,
  any `Origin` → 403). Gate order 501 (not wired) → 403 servingDisabled (names the node's hostname) → 400 →
  answer / 404 / 409 `{code,message}` / 500. Ops: discover, exec{op: NodeOp}, provisionStart/Poll,
  rankStart{runId,requester,node,spec,modelId,servedModelId,runner} → {rankId,pid,leaseMs},
  rankPoll{rankId,knownLines} → RankSnapshot{rankId,pid,lines,tail?,groupJoined,caps,memory,exit},
  rankStop{rankId,followRank0} → {report,exit}. Types: `goose-sidecar/src/distributed/link_control.rs`.
- NO RAW SCRIPTS over Link: `NodeOp` (probe/link/repair/sample/pidRow/signal/processList) is data; the PEER
  builds the script (`node_op.rs`) after authorizing it: interpreters must be goose-managed envs under ITS
  `$HOME/.goose/distributed`, a signal only reaches a pid carrying `goose-distributed-rank`, a TB repair is
  licensed from its own service listing. `LinkRoutedExec` refuses a raw script to a `link:` host.
- SESSION: where the ssh client stood, `/bin/sh -c 'IFS= read -r code; exit "${code:-255}"'`; the relay
  polls rankPoll (400 ms until the group joins, then 2 s) and writes the rank's exit code into it.
- LEASE = 15 s (= ssh ServerAlive 5 × 3, pinned by a test): the peer stops a rank no one polled for 15 s;
  both sides measure silence from the last ANSWERED poll. Control loss = `linkControlLost` / `linkControlRestored`
  events stating whether the data plane (rank 0's step counter) still moved.
- PEER-SIDE: `link_host.rs` spawns/stops the rank per pid (never killpg), verifies with `ps -p`, publishes
  `<state>/mlx-distributed-hosting.json` {pid, rank_pid, rank, requester, model_id} so other windows refuse
  their single engine (`hostingRank`); tile + tray say "Rank 1 of <host>'s distributed engine · <model> · JACCL".
  A record whose goosed is DEAD but whose rank pid still carries the marker is reclaimed per pid (8670bfdb2).

### Hermetic e2e recipe (proven 2026-09-24, both personal Tailscale identities byte-identical before/after)
- Throwaway Headscale on the TB IP 192.168.0.1:18471 + a stub worker (join-key/verify) on :18480; SHORT
  roots under /tmp/lzd (sun_path limit): A HOME=/tmp/lzd/a port 18490; B HOME=/tmp/lzd/b on the workhorse
  port 18491, reached through `ssh -f -N -L 18491`. Stand-in rank site (a fake mlx that joins and serves) =
  the session scratchpad `ld/kit`; ACP client acp.mjs drives both goosed.
- Results: switch off → servingDisabled; switch on applies live; Detect over Link 0 gaps, JACCL chosen;
  preflight all pass; Start → ready 2.9 s, B's rank PPID = goosed B; B's mount + B's own start refused
  hostingRank; 7 s tailscaled pause → lost/restored, run stays ready; Stop verified BY THE PEER; SIGKILL of
  B's rank → rankDied 137; 25 s pause → B's lease stopped it at 16.3 s; requester SIGKILLed → B's lease
  stopped it 16 s later; B goosed SIGTERM → B's exit path stopped it, A says "NOT verified from here" honestly.
- TEARDOWN per pid: goosed A (SIGTERM stops its tailscaled), headscale ($(cat /tmp/lzd/hs/pid)), the worker,
  the tunnel; `pgrep -fl /tmp/lzd/` empty on both; diff `tailscale status --json | python3 tailscale_identity.py`.

### Traps (each cost time)
- DETACHED goosed over ssh gets EHOSTUNREACH on the LAN/TB: an ad-hoc-signed process started from an ssh
  session loses Local Network access once the session ends (Apple-signed curl and Developer-ID node are
  fine). Keep the peer's goosed INSIDE a live `ssh -tt` session.
- `pgrep -f lzd` matches base64 `-LaunchArguments` of System Settings extensions — a teardown guard of
  `pgrep -f lzd || rm -rf /tmp/lzd` silently skipped the rm. Match the PATH: `pgrep -fl /tmp/lzd/`.
- `$!` / a wrapper pid is not goosed: kill the LISTENER pid (`lsof -nP -iTCP:<port> -sTCP:LISTEN -t`).
- A goosed killed outright (SIGKILL) takes its lease watcher with it: its rank lives on (rank 0 on the
  requester did too). The next goosed reclaims it via the hosting record; a Stop sweeps by marker.
- The personal CLI on the laptop is `tailscale` on PATH (Homebrew 1.98.x talks to the system daemon);
  `/usr/local/bin/tailscale` does not exist. Only `status` is ever run against it.

### LIVE test (waits on the owner: app build ≥ 8670bfdb2 on BOTH Macs, Link signed in + Connect on both)
1. `link-live-test.sh pre` (both Link daemons up, each sees the other online; personal snapshots).
2. Workhorse: Providers › LeanZero MLX › Engine › Distributed → turn ON "Allow this Mac to serve as a distributed node".
3. Laptop: same place → Set up → "Your Macs on LeanZero Link" shows Work's Mac Studio (RAM, en3 TB path,
   rdma_en3 active, models) → Use this Mac → Detect → Save and provision → Start.
4. Workhorse: the tile and the tray read "Rank 1 of Mihai Macbook's distributed engine · <model> · JACCL";
   mounting its single engine is refused (hostingRank). Run `link-live-test.sh during`.
5. Chat one turn on the laptop through the distributed model; then Stop → the stop steps say "verified gone by the peer".
6. `link-live-test.sh post` (0 ranks both, identities unchanged). Optionally: turn the switch off and Start → preflight names servingDisabled.

### LIVE result 2026-09-24 (installed Goose Swarm 3.0.22 on both, Link mesh 100.64.0.2 ↔ 100.64.0.3)
- Driven over each Mac's INSTALLED goosed ACP: secret = `GOOSE_SERVER__SECRET_KEY` from `ps -E -ww -o command= -p <goose serve pid>`
  (same user), url `wss://127.0.0.1:<--port>/acp?token=…` with NODE_TLS_REJECT_UNAUTHORIZED=0 (self-signed), methods
  `_goose/unstable/{config/upsert, mlxEngine/distributed{PeerCandidates,Discover,ConfigUpdate,Provision,Preflight,Start,Status,Stop}, mlxEngine/{status,mount,unmount}}`.
  Workhorse switch set with `config/upsert {"key":"LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE","value":true}` (= the UI switch; config.yaml line).
- 27B over Link: candidates list Work's Mac Studio ready (74.5/96 GiB, en3 80 Gb/s, rdma_en3); Detect 0 gaps, JACCL; provision 1.6 s;
  preflight ok (context 164,096); Start → ready 10 s; rank 1 pid's parent = the workhorse's installed goosed; workhorse hosting DTO
  "rank 1 · Mihai Macbook · serving", its mount refused hostingRank; chat 51 tok in 4.8 s; Stop verified by the peer (ps -p … verified here).
  No Local Network alert blocked the rank (JACCL group formed).
- Flash over Link: Detect 0 gaps, provision installed the pipeline env on both over Link (5.7 s), preflight FAIL memory on both:
  weights+workspace alone 53.3 / 56.1 GiB vs budgets 44.0 / 51.6 (available − RAM×0.21) → needs ≈80 GiB available on the MacBook,
  ≈76 on the workhorse (had 73.5 / 73.6 with normal apps + other sessions). Start refused preflightFailed, 0 ranks spawned.
- TRAPS: over ssh `screencapture` → "could not create image from display" and osascript System Events → -1719 (no permissions) —
  the workhorse's tile/tray PIXELS need the owner's eyes. Saving a config via distributedConfigUpdate REPLACES the saved one (no backup):
  capture `distributedStatus.status.config` first. Unmount/remount the owner's :8090 27B through ACP mlxEngine/unmount|mount.
- 2026-09-24 COMPACTION measured (workhorse, nothing loaded): `memory_pressure -l warn` (allocates until the kernel's WARN notification — level 2, ballast 30.4 GB) then kill by pid → available 72.4 → 78.0 GiB (77.4 settled): +5–5.6 GiB from macOS compressing idle apps + apps dropping caches. Owner wants goose to do this automatically ("Make room") and the 21%-of-RAM reserve loosened with live evidence. Flash over Link needs ~80 GiB free (MacBook) / ~76 (workhorse) under the 21% rule.
- Live Link 27B split proven 2026-09-24 on installed 3.0.22 (Detect via Link, rank 1 as a child of the workhorse's own goosed, chat OK, peer-verified stop). Opt-in set via ACP config/upsert LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE=true.

## IMAGE INPUT on the Flash split (2026-09-24, fork d40e9e363 dabcc67b2 2f7cdf27c a5cb98757; goose pin 8a23b10dd)
- What exists: the SINGLE engine already served Flash images — rapid-mlx routes a vision checkpoint to its MLLM lane (mlx-vlm 0.7.1 ships `mlx_vlm.models.qwen4_exp`: Qwen3-VL ViT + Qwen3VLProcessor), though goose's single engine passes `--text-only` by default. The split was text-only until this.
- Design that actually works: merging features before layer 0 is NOT enough. Qwen4-Exp rotates by Qwen3-VL multimodal (t,h,w) positions (interleaved mrope_section 11/11/10) in attention AND the QSA indexer (queries + compressed keys at each group's first token); tokens after an image are shifted by rope_delta. So rank 0 merges embeddings AND every rank gets the batch's RoPE table (flag all_sum + table all_sum per batch, `pipe.prepare_multimodal`). `rapid_mlx/models/qwen4_exp_vision.py` holds the tower loader (model.visual.* only, quantized by the config's `vision_tower.*` entries), merge, `MRopePositions`.
- Only rank 0 needs mlx-vlm (it measures VisionCost and all_sums it). goose's managed pipeline env now installs `mlx-vlm==0.7.1` (NOT the fork's `[vision]` extra — that pulls torch). Before the pin bump the workhorse's managed env and its fork clone venv had NO mlx_vlm (measured).
- Planner: rank 0 weight += tower 0.42 GiB; workspace = max(prefill, encode) with encode = heads·P²·4 B·1.25, P = max_pixels/16² = 3920 (measured peak 1.139 GiB; mx.clear_cache after encode makes max honest). Processor max_pixels 1,003,520 → ≤ ~980 tokens per image (1920x1080 → 943 tokens, the MCP-UI screenshot 2360x1142 → 945 + text = 973-token prompt).
- Tests: `tests/test_pipeline_qwen4_vision.py` (tiny ckpt with a quantized tower in checkpoint layout; mlx-lm's save_config DROPS vision_config — write config.json yourself). Single vs 2/3-rank: logits 0.0, 64 tokens identical; server image chat == single-process words. Cross-impl vs mlx-vlm's qwen4_exp: features/embeddings/position ids EQUAL; mrope at real dims within 1 bf16 ulp.
- LIVE real Flash, JACCL [0,6)/[6,48) (MacBook had the KV agent's 27B up, budget 49.9 GiB): screenshot answered "City: New York, Temperature: 78°F, Conditions: Clear Sky, with 9 MPH winds and 55% humidity" (all correct), prefill 186 tok/s on 973 tokens, decode 17.1 tok/s. Full 48-layer single-process stream reference: steps 0-50 top-1 logprob diff exactly 0.0, 95/96 argmax (the 1 is post-EOS).
- TRAP (cross-device, not the split): MLX decode SDPA gives different bits on M4 Max (applegpu_g16s) vs M3 Ultra (applegpu_g15d) at EXACTLY 1024 keys (probe: 1000/1025/1100 identical). A 2-Mac run whose decode passes key length 1024 diverges from a one-Mac reference from that step on. Probe: /tmp/sdpa_probe.py pattern (seeded q/k/v, sha1 of output per N).
- TRAP: `ruff format rapid_mlx/` reformats unrelated files (server.py) — format only your files.
- Commands: `harness prompts --image <png>` (adds kind "image" with ids + path); `pipe`/`stream` re-derive pixels from the file; `stream` reports `top1_logprob_diff_by_step`.

## MEMORY COMPACTION ("Make room") + the GPU-ceiling budget — goose f6fcc664b, fork 2ce699589 (in pin 2f7cdf27c), 2026-09-24
- Mechanism (goose-sidecar `distributed/compaction.rs`): Apple's `/usr/bin/memory_pressure -l warn` as ballast (checks
  kern.memorystatus_vm_pressure_level before EVERY page, pages are an incompressible JPEG fragment, a thread keeps them active;
  source: apple-oss-distributions/system_cmds memory_pressure.c). goose polls the LEVEL (0.2 s sh loop), releases the ballast
  by pid the instant it leaves 1 (2 = WARN intended, 4 = CRITICAL released + reported), a guardian subshell kills the ballast
  within 1 s if the script dies. Settle: 2 s samples until 3 samples set no new high > 0.1% RAM. Refuses beside ANY MLX
  python engine (`engineLoaded`) or when the level is not 1 (`notNormal`).
- MEASURED workhorse (nothing loaded): WARN at 3.73 GiB available (3.9% RAM) after 33 s with a 32 GiB ballast; 77.3 → 79.2
  (already compacted earlier: 72.4 → 78.0). A second compaction minutes later gains nothing (80.4 → 80.3). 42 s end to end
  over ssh. Negative control: this MacBook with another session's 27B → refused naming the pid.
- Budget rule, BOTH runners: min(available − RAM × 0.07, Metal max_recommended_working_set_size). Ceilings: M4 Max
  115,448,725,504 B (107.52 GiB), M3 Ultra 83,494,174,720 B (77.76 GiB); iogpu.wired_limit_mb 0 on both. In-process memory
  and wired limits = the ceiling. 0.07 = watchdog WARN reserve 0.05 + load drift 0.02. The old 21% floor was "lowest share
  seen NORMAL", not where WARN starts. Fork `plan --node NAME:RAM:FREE:CEILING`, JSON ratios {available_margin}.
- ACP: `_goose/unstable/mlxEngine/distributedMakeRoom {node, config?}`; status `compactions`; preflight node `ceilingBytes`,
  `wiredLimitMb`, `shortBytes`, `topApps`; node config `freeMemoryAutomatically` (absent = ON). Auto: preflight/start/restart
  compact SHORT nodes with the switch on, then preflight again; single-engine mount compacts when the gate BLOCKs and
  nothing of ours is loaded.
- Live tests: `cargo test -p goose-sidecar --features rustls-tls --test distributed_live -- --ignored --nocapture
  live_compaction_over_ssh` (GOOSE_COMPACT_HOST) / `live_compaction_refuses_beside_a_loaded_engine` /
  `live_flash_at_the_ceiling_rule_after_compaction`. Provision the fork env first:
  `GOOSE_PROV_HOST=workhorse|local GOOSE_PROV_ENV=pipeline cargo test -p goose-sidecar --lib live_provision -- --ignored`.
- TRAP: a wait loop `pgrep -f 'rapid-mlx serve|…'` matches another agent's harness zsh whose argv MENTIONS the engine — detect
  engines by python argv0 (awk over `ps -axo pid=,command=`), exactly like classify_foreign_engines.
- TRAP: fork `plan` at ≥ d40e9e363 (vision) imports mlx_vlm for the vision cost — the managed env needs mlx-vlm (main's pin
  8a23b10dd provisions it). An env without it crashes preflight's plan.
- LIVE APP recipe for the image chat (ready, not yet run): isolated profile /tmp/gvis (config seeds swarm device mihai-mlx → served `flash-vision`, mlx_distributed with ssh `workhorse` and pipeline_python = the FORK venvs: MacBook ~/Projects/Rapid-MLX/.venv, workhorse ~/Projects/Rapid-MLX-pipeline/.venv — an operator interpreter on another commit is only a WARN in 3.0.22's preflight, so the installed 3.0.22 binary runs the vision fork without a rebuild). `open -n -a "/Applications/Goose Swarm.app" --env GOOSE_PATH_ROOT=/tmp/gvis --env GOOSE_USER_DATA_DIR=/tmp/gvis/userdata --args --remote-debugging-port=9451`; drive with scratchpad cdp.mjs/click.sh: Providers → Start; `window.electron.addProject('/tmp/gvis/work')`.
- BLOCKER measured 2026-09-24 12:15-13:02: the KV-cache agent cycles a 27B `rapid-mlx serve` on :8093 (from ~/Projects/Rapid-MLX-kvq) back to back; with it loaded the MacBook has 34-37 GiB available → budget < the 31.2 GiB PLE layer, so Flash cannot be planned at all (plan exit 2, max_context null). Flash on 2 Macs needs the MacBook without a resident 27B.
- LIVE 2026-09-24 15:03 (Flash, JACCL, 7% rule, both Macs compacted first): MacBook freed 7.7 GiB (76.3 → 84.0, WARN at
  9.3 GiB available = 7.3% RAM), workhorse 3.2 GiB (74.6 → 77.8, WARN at 4.0). A foreign 27B (~33 GiB) loaded on the MacBook
  between compaction and preflight → plan [0,4) 40.8/42.5 GiB MacBook, [4,48) 67.5/71.0 workhorse, ctx 61,184. Ready 33 s;
  2k 200 in 5.8 s; 25.5k-token prompt 200 in 50.2 s; then MacBook at kernel WARN (41/184 samples; ~20 GiB avail during the
  LOAD transient, 13 → 5.4 while serving) → watchdog 503 on the concurrent pair. Workhorse 1/183 samples at WARN.
- BACK-OFF (goose 20ef879e6, fork 2f02ac645): margin 0.07 → 0.093 = the M4 Max's measured WARN share 7.3% + drift 2%. The
  kernel WARN point differs per Mac (M4 Max 7.3%, M3 Ultra ~4%) and during a model LOAD it fires at ~16% available (active
  file cache) — WARN during load is expected; WARN while serving is the failure. Rerun at 0.093 still owed (both Macs were
  taken by other sessions' 27B engines + the owner's :8090 remount). Idea not built: per-node margin from each node's own
  compaction-measured WARN point.
- CLEAN RERUN 2026-09-24 17:14 (installed 3.0.24 on both, workhorse over Link, driven via the installed goosed's ACP +
  CDP 9333): Make room MacBook 88.0 → 92.8 (+4.8, WARN at 6.2), workhorse over LINK 75.8 → 79.4 (+3.6, WARN at 4.2). Preflight
  at 9.3%: MacBook avail 92.88 / ceiling 107.52 / budget 80.97 / planned 66.07 [0,18); workhorse 79.40 / 77.76 / 70.48 / 56.31
  [18,48); ctx 262,144 derived. Ready 20 s; 2k 200 6.8 s; 26.6k 200 56.1 s; pair 200/200. Pressure level 1 in ALL 309/304
  one-second samples on both Macs (min available 11.2 / 16.4 GiB). Measure speed: 21.0 tok/s writing, 527 reading → card
  "measured". Stop verified both halves.
- DEFECT FOUND + FIXED (goose 527fd00de, NOT in 3.0.24): the hang rule stopped an IDLE Flash engine 20 s after the last
  request ("silent 20298 ms … ps stats [S, S]") — since the doorbell fork idle ranks burn 0 CPU. Now silence counts only
  while /goose/progress inflight > 0. Until 3.0.25+ ships, an idle Flash run in the app dies ~20 s after its last request.
- NEVER `git stash` in a goose worktree: the stash stack is shared across worktrees; a no-op stash + pop applied ANOTHER
  agent's stash (conflict in swarm.rs). It was kept (pop failed); reset the file per path.
- 2026-09-24 CLEAN FLASH RUN at the ceiling rule (installed 3.0.24, owner-approved quiet MacBook): Make room +4.8 (MacBook) / +3.6 GiB (workhorse via Link); budgets 81.0 / 70.5 GiB (GPU ceilings 107.5 / 77.8; margin 9.3% of RAM), planned 66.1 / 56.3; full 262,144 ctx; 26.6k prompt 56 s; 2 concurrent OK; pressure NORMAL in all ~300 1-s samples on both. Measured 21.0 tok/s write / 527 read.
- TRAP fixed in 527fd00de (3.0.25): after the idle-CPU fix, idle worker ranks show ~0 CPU, and the supervisor's stall detector killed an idle Flash engine ~22 s after its last request. Idle ≠ hung: liveness must key off in-flight requests.
- Link auto-reconnect on launch (9319c73d0, 07d11bda4): verified on installed 3.0.24 — both Macs came back "Running · 2 nodes" with no clicks.
- LOAD-BEARING, NEVER "CLEAN UP" (found 2026-09-24): the WORKHORSE hosts the live LeanZero Link backend — `~/.leanzero/bin/headscale serve` (127.0.0.1:8790, metrics 9790, server_url https://worksmacstudio.tailfc4700.ts.net) + the Link worker `npm exec tsx src/node-server.ts` from ~/Projects/goose/leanzero-link/worker (127.0.0.1:8791), exposed via the owner's Tailscale Funnel (:443/:8443/:10000). The mesh's 100.64.0.x addresses come from it. Killing them breaks Link on both Macs. Also running there, owner/other-session territory (ask before stopping): `target/debug/goose serve --port 3399` (since 2026-09-23 08:20) and mcp-web-search http-server.
- 2026-09-24 17:44 VERIFIED on installed 3.0.25 (has 527fd00de): Flash ready 18 s; request → 3 min idle → request → 2 min
  idle → request, all 200 in 7–8 s; state Ready throughout, 0 restarts, no hang event (events stayed preflight/launched/
  ready). Stop verified both halves. Link "mesh Running · 2 nodes" across the relaunch; placement card for the 27B, Chat:
  "Best: Work's Mac Studio alone ~21.9 tok/s (estimated)", workhorse chip M3 Ultra 60-core 819 GB/s ceiling 77.8 GB.
  Installed goosed ACP: port from `ps` of "Goose Swarm.app/.../goose serve --port N", secret GOOSE_SERVER__SECRET_KEY via ps -E.
- CLEANUP 2026-09-24: the manual test venvs (~/goose-builds/jaccl-smoke/.venv, workhorse ~/jaccl-smoke) and the /tmp/jaccl-smoke symlinks are GONE — goose's managed envs in ~/.goose/distributed/ replace them (mlx0.32.2-mlxlm0.31.3-py3.12, rapid-mlx-pipeline-qwen4-py3.12). The workhorse's 27B moved to the standard ~/.goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx (identical to the MacBook's, names+sizes). The result REPORTS stay in ~/goose-builds/jaccl-smoke/ (STEP1b-soak, FLASH-*, vision). For a manual experiment, use a goose-managed env's python, not a new venv.

## 2026-09-25 — the split capped EVERY answer at 512 tokens (Q-65) — fixed 39db807f0 (+ 330dc836c)
- CAUSE: goose sends NO max_tokens (formats/openai.rs omits it on purpose), and mlx_lm.server 0.31.3 fills an
  absent one with its `--max-tokens` argparse default 512 (server.py:1172/1844). A thinking 27B spent all 512 in
  <think> → "The model returned an empty response"; or stopped mid-plan. Seen as usage output_tokens == 512 exactly.
- FIX (rank_budget.py + rank_wrapper.py): validate_model_parameters keeps the absence (None); rank 0's generation
  thread settles it in `_share_request` BEFORE the request reaches other ranks: prompt counted by mlx_lm's own
  `_tokenize` on a DEEP COPY, budget = context_window − prompt (explicit max_tokens kept but held in the window;
  no room → 400 `context_length_exceeded`, which goose turns into compaction).
- TRAPS the design avoids: (1) `_tokenize` is NOT idempotent — process_message_content json.loads tool-call
  arguments in place; a second pass on the same objects raises. (2) A Link peer runs ITS OWN goosed's embedded
  program — so never let `None` cross ranks (an old peer's BatchGenerator does `_num_tokens >= None`). Settle on
  rank 0, share the int. (3) mlx_lm's handle_completion catches `Exception` → 404: the wrapper's Refused is a
  BaseException so it reaches do_POST with its own status.
- PROOF RIG (no split needed): run the real TENSOR program single-process with `mx.distributed.init` stubbed to a
  1-rank group, a 1B model (Llama-3.2-1B-4bit in the HF cache), window 2048: absent → 1993 (= 2048−55); HEAD's
  wrapper → 512. Script shape: import mlx.core, stub init, exec rank_env+rank_live+rank_budget+rank_wrapper with
  sys.argv = [boot, "", b64(spec), marker].
- The single engine (Rapid-MLX lz.4) has the same CLASS with another number: no `--max-tokens` → 32768
  (cli.py:3889), and the chat route reserves prompt+32768 against the window (chat.py:4687/4874). `--max-tokens
  <window>` from the sidecar would 400 EVERY chat request (prompt + window > window) — the fix has to be in the fork.
- 128000 in llm_request logs = the `swarm` session's ModelConfig (unknown name → DEFAULT_CONTEXT_LIMIT) cloned into
  the routed call; 330dc836c makes the routed call carry the node's probed window (or None).
- LIVE 2026-09-25 19:2x: the app's split stopped via its ACP (distributedStop, verified by peer), a dev goosed
  (target/debug/goose serve, isolated HOME + GOOSE_PATH_ROOT so no Link/tailscaled, config copied with the peer
  as ssh 'workhorse') ran distributedStart → ready; same no-max_tokens request: before 512 `length`, after 2645
  `stop`. Restored with the app's distributedStart (rank 1 back under the Studio's goosed). The INSTALLED app
  keeps the 512 cap until a release carries 39db807f0.

- 2026-09-25 TRAP (Q-65): mlx_lm.server fills an UNSET max_tokens with its --max-tokens default of 512 (server.py:1172/1844).
  goose sends none on purpose, so every split answer stopped at exactly 512 tokens: a thinking model returned "empty
  response" or stopped mid-plan. Fixed in rank_wrapper.py (39db807f0): rank 0 derives window − prompt. Symptom to grep:
  usage.output_tokens == 512 in ~/.local/state/goose/logs/llm_request.*.jsonl. The single Rapid-MLX engine has the
  same class at 32768 (Q-69). Also: an idle split holds the Studio's rank at ~100% CPU (Q-66, open).

## 2026-09-25 — Q-66 idle spin on the TENSOR split + Q-71 window 141,568 (branch q66-doorbell, 158981ad7)
- JACCL has NO completion channel (rdma.cpp `create_cq(..., nullptr, nullptr, 0)`) → any rank waiting in a collective
  spins ibv_poll_cq at 100%. mlx_lm.server's idle loop shares "no request" by all_sum every 0.1 s → the worker spins
  forever while rank 0 sleeps in queue.get. Ring backend spins the same (measured localhost: 10.01 CPU-s / 10 s).
- Fix = the pipeline fork's doorbell (286ed77f7) ported to rank_wrapper.py: ephemeral port shared by one all_sum,
  idle rank 0 shares only real requests after ringing a byte, worker parks in recv(1). Needs same-version wrappers →
  tensor spec tag `mlxLmServerDoorbell` makes an OLDER Link peer refuse at rank start (older_peer_refusal).
- LOCAL RIG (no second Mac, no GPU): scratchpad q66/run_pair.sh + probe.sh — two ranks of the real TENSOR program on a
  127.0.0.1 ring, `mx.set_default_device(mx.cpu)` with device_info kept from the GPU, and TMPDIR PER RANK (two CPU
  ranks race MLX's JIT cache: "ld: open() failed, errno=17" then "Could not load C++ shared library").
- Q-71: the split's context is DERIVED at each launch from each node's AVAILABLE memory (preflight
  budget = available − RAM×0.09). 141,568 at 20:36:44 = MacBook available 40.05 GiB because my dev single engine held
  ~30 GB then; 262,144 when the Mac is free. It is the true limit of THAT launch (rank_budget refuses a longer prompt
  with 400 context_length_exceeded); a relaunch with the memory back derives 262,144 again.
- 2026-09-25 Q-66: an idle JACCL split spins the worker rank at ~100% CPU — JACCL's recv polls without sleep and
  mlx_lm's idle loop joins a collective every 0.1 s. Fixed (158981ad7): rank 0 rings a doorbell byte before work,
  workers block on a socket (the pipeline runner already did this). Q-71: the split's context window is derived at
  EACH START from free memory (40 GiB free → 141,568; free machine → 262,144) and fixed for that split's life.
  A second 27B on the MacBook pushed the split into memory pressure: 503s at 21:03:18/21:04:53 (~2 s each).

## 2026-09-25 — Q-75 PIPELINE prefix cache (fork b7bd1afc2, tag lz-pipeline-qwen4.1; goose cc790b8cd, worktree branch)
- CAUSE (read + measured): fork pipeline_qwen4_serve.run_batch made a fresh `stage.make_cache` per batch — no reuse ever.
  Localhost 2-rank ring, tiny qwen4_exp, 20,165-token prompt: 3.11/2.10/2.68 s for three same-prefix requests, cached 0.
- DESIGN: each rank snapshots ITS OWN layers (`_PrefixStore`, by id); ONLY rank 0 decides (`_PrefixIndex`: restore id +
  length, snapshot position, evictions) and the batch header carries it (+ one all_sum of evicted ids). Hybrid → exact
  prefix only, same length on every rank or collectives desync. Boundary = single engine's `_compute_prefix_boundary`
  (transient tail honoured; /v1/models declares `request_extensions: ["rapid_mlx_transient_tail"]` so goose's omlx
  provider sends it). Bytes live INSIDE each rank's planned KV budget minus the admitted batch's reservation, trimmed
  LRU at every admission, snapshot pre-charged at planner formula(len + KVCache.step), then booked at MEASURED bytes.
  A request the cache acts on runs as a batch of ONE. `--no-prefix-cache` = control arm.
- AFTER: 2.42 s cold, 0.08 s / 0.29 s with cached 20,156. Wire: usage.prompt_tokens_details.cached_tokens;
  /v1/status.prefix_cache {enabled, entries, entry_tokens, bytes[rank], limit_bytes[rank], hits, misses,
  tokens_saved, stored, evicted, skipped{reason}}.
- TRAP: QSA's top-k block selection makes prefill tokens depend on CHUNK EDGES (no cache: --prefill-step 142/284 answer
  differently from one chunk, 63 does not). Exactness test = cached vs cold `--no-prefix-cache --prefill-step <boundary>`.
- TRAP: running a fork test/proof as a SCRIPT imports the venv's editable rapid_mlx (~/Projects/Rapid-MLX), not the cwd
  worktree — put the worktree on sys.path. goose's pipeline_rank.py refuses a fork without `prefill_chunks` (seen).

## 2026-09-26 — Q-114 ROOT CAUSE: the tensor split hangs at ~10.5k generated tokens (goose branch q114-gdn-padding-leak b589e04f6)
- CAUSE: mlx_lm 0.31.3 `ArraysCache.advance` does `left_padding -= N` LAZILY and each `-= N` allocates a constant = ONE Metal
  buffer. `_make_cache` gives every batch cache a left_padding (even 1 row). qwen3_5 27B: 48 linear-attention layers, only
  cache[0]'s counter is read (`create_ssm_mask(..., cache[ssm_idx])`) → 47 unread chains pin 47 buffers PER DECODE STEP → MLX
  `resource_limit` 499,000 (`mx.device_info(mx.gpu)["resource_limit"]`, both Macs) → `[metal::malloc] Resource limit (499000)
  exceeded` inside the step's async_eval on BOTH ranks. It is a STEP COUNT (10,447 / 10,522 / 10,537 generated), not a width.
- WHY SILENT: MLX 0.32.2 `eval_impl`'s catch path (transforms.cpp, #3675, unchanged on main) calls synchronize(CPU stream) while
  that stream waits on a fence event whose GPU signal the aborted step never committed → the thread blocks forever, the error
  never reaches Python. Signature: stuck rank `S` 0%, generation thread in `async_eval → eval_impl → synchronize`, CPU stream
  thread in `-[IOSurfaceSharedEvent waitUntilSignaledValue:timeoutMS:]`; peer `R` ~97% in `jaccl::MeshImpl::all_reduce`; the
  peer's kernel logs `(IOGPUFamily) Cmd queue … timed out!` ~5 s later. SECOND ROAD in: any stall of ONE rank > ~5 s (Metal's
  command-buffer wait) — a `vmmap -summary` of rank 0 did it (repro B1). A 10 s SIGSTOP usually ends LOUDLY instead (RANK_FATAL
  "Command buffer execution failed: GPU Timeout", exit 70) — which way it goes is timing.
- FIX: goose's rank_wrapper records every counter `advance` touched and async_evals them once per step (`settle_counters`).
  Measured: 21,074 tokens straight, [DONE], 11.48 tok/s. Isolated proof: `ArraysCache(2,left_padding=[0]).advance(1)` in a
  loop throws at 498,999; evaluated per step, 700k clean.
- TRAP (hang rule): "every rank's CPU advanced" is satisfied by a rank stuck at 0% that still answers goose's own
  /v1/status + /goose/progress polls, beside a spinning peer (B1 sat 4 min 19 s, no event). The rule now reads per-process GPU
  time: `/usr/sbin/ioreg -r -c IOGPUDeviceUserClient -l -w0` → the `{…}` block whose `"IOUserClientCreator" = "pid N, …"` →
  `"AppUsage"` `accumulatedGPUTime` ns (18 ms, no root, works on M4 Max + M3 Ultra; flat on both ranks in a stall).
- REFUTED with evidence: a late rank (JACCL all_sum survives one rank entering 20 s late, both ways); upstream #4552's fence
  deadlock (that is MLX_METAL_FAST_SYNCH=1; goose never sets it — non-fast = MTLSharedEvent waits); RDMA loss/desync (token
  trails + CRCs identical to the end).
- NEW TRAP (not fixed): after an ABNORMAL end, the NEXT group's first all_sum (the doorbell port) read 1065404974 /
  1065406545 (0x3F81… = a bf16 payload of the dead group) and rank 0 died at startup (GPU Timeout, exit 70); the restart
  after that was clean (2/2). Stale UC data survives into a fresh group.
- TOOLS that worked (no Developer Mode): `/usr/bin/sample <pid> 2 -mayDie -file out.txt` (own processes, also over ssh);
  `lldb -p` REFUSES non-interactively. To see an exception MLX swallows: build a DYLD-interposed `__cxa_throw` logger
  (clang++ -dynamiclib, `__DATA,__interpose`; prints type/what()/backtrace to stderr = the rank log) and load it into
  product-launched ranks with a `.pth` in the managed env's site-packages that re-execs `sys.orig_argv` with
  DYLD_INSERT_LIBRARIES when argv carries `goose-distributed-rank` (uv's python is ad-hoc signed, no hardened runtime).
  REMOVE both files afterwards (done 2026-09-26). Expect ~12/s `nanobind::builtin_exception` noise (StopIteration).
- TRAP: never `vmmap` a live rank — it suspends the process for seconds and kills the split (see second road).
- Repro harness + all captures: ~/goose-builds/q114/ (drive.py streams + logs and runs `capture.sh` at an 8 s stall: sample both ranks + ps; throwlog.cpp + the .pth; chain.py; late.py); product path
  `node local-edition/mlx/quality/harness/split-start.mjs --model 27B` (retry once if the picker times out). Live tests:
  `cargo test -p goose-sidecar --features rustls-tls --test distributed_live -- --ignored --nocapture live_q114` with
  GOOSE_DIST_LOCAL_PYTHON / GOOSE_DIST_REMOTE_PYTHON = the managed envs and GOOSE_DIST_REMOTE_MODEL=/Users/workhorse/.goose/models/Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx
  (the recorded_config defaults point at the deleted jaccl-smoke venvs).
