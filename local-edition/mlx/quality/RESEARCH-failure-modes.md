# Reported failure modes: MLX single/split serving, exo, LM Link, Tailscale userspace

Research for the reliability matrix (`RELIABILITY.md`, R1–R6). Collected 2026-09-25 from primary sources:
GitHub issues and PRs, vendor changelogs and docs. Every claim below links to its source.

**How much to trust these sources.** Most entries are user bug reports, and many from 2026 were
visibly written with AI help. The panic signatures, stacks and log lines are quoted from the reports.
The root-cause analyses in them are the reporter's own and usually **not confirmed by a maintainer**.
Each entry's status field says `OPEN`, `CLOSED (fixed by …)` or `maintainer-confirmed`. **UNVERIFIED**
marks anything I looked for and could not find a primary source for.

---

## 1. exo and MLX distributed (JACCL / ring / Thunderbolt 5)

### 1.1 Kernel panics (IOGPUFamily): the class that hit the owner's 96 GB Studio

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| D1 | Kernel panic in Apple's IOGPUFamily kext during MLX inference. 9 panics in 6 days, 4 of them in one 2 h window | Mac Studio **M3 Ultra 96 GB**, ~50 GB model, repeated inference, repeated model load/unload, several MLX processes | `panic(...): "completeMemory() prepare count underflow" @IOGPUMemory.cpp:550`; a second variant: `"Memory object unexpectedly not found in fPendingMemorySet" @IOGPUGroupMemory.cpp:219` | CLOSED as a duplicate of #3186, no fix | https://github.com/ml-explore/mlx/issues/3346 |
| D2 | The same panic on long-context prefill (~173k tokens) | one large Metal allocation during prefill | same `IOGPUMemory.cpp:550`; Apple FB22091885 | OPEN (Apple kext bug) | https://github.com/ml-explore/mlx/issues/3186 |
| D3 | **Trigger isolated by one reporter:** concurrency plus prompt-cache eviction churn, not prefill size. A single sequential stream ran 111 min with no panic. **Two concurrent streams plus eviction churn panicked in 102–108 s, 3 of 3 cold boots.** Not calling `mx.set_wired_limit` gave 10/10 runs with no panic (4.2 h) | two concurrent `/v1/chat/completions` streams, unique 12–16k prompts that overflow `--prompt-cache-bytes` | `IOGPUMemory.cpp:550` | user A/B, not maintainer-confirmed. A second user reports the "don't call set_wired_limit" mitigation held for 2 days in production on an **M3 Ultra 96 GB** (IOGPUFamily 162.11, macOS 27 beta) | https://github.com/ml-explore/mlx/issues/3186 (comments by ronm92130 2026-07-07 and BrunoCerberus 2026-08-28) |
| D4 | Two more trigger paths from crash forensics: (1) `mx.clear_cache()` on the main thread while a generate thread still holds GPU buffers; (2) two processes running MLX generate at the same time | thread or process concurrency | same | user report plus a userspace guard (metal-guard) | https://github.com/ml-explore/mlx/issues/3186 (Harperbot, 2026-04-12) |
| D5 | Panic in `mlx_lm.server` on an agentic session with unbounded KV growth: wired 80.14 GB of 96 GB, free 0.01 GB, `memoryPressure=false` | **M3 Ultra 96 GB**, context grows past 58k across tool-call turns | `IOGPUMemory.cpp:550` | CLOSED on the theory that bounding `--max-kv-size`/`--prompt-cache-bytes` fixes it; D6 disputes that | https://github.com/ml-explore/mlx-lm/issues/883 |
| D6 | The panic recurs **with** the #883 bounds in place, under continuous batching | `--decode-concurrency 2 --prompt-concurrency 2`, variable-length requests admitted mid-batch | same, three times in one day | OPEN | https://github.com/ml-explore/mlx-lm/issues/1666 |
| D7 | The same panic in exo distributed inference over TB5 (M4 Max 128 GB; a comment adds an M5 128 GB) | exo serving Gemma-4-31B bf16 on 2 nodes | same, "panicking process: exo" | OPEN | https://github.com/exo-explore/exo/issues/1972 |
| D8 | A **watchdog** panic, not the IOGPU one: "no checkins from watchdogd in 92 seconds" on an M3 Ultra 512 GB under exo while a 397B download ran | sustained memory pressure; the peer node's exo had been killed for excessive disk writes | `panic(...): watchdog timeout: no checkins from watchdogd in 92 seconds` | CLOSED without a stated fix | https://github.com/exo-explore/exo/issues/1939 |
| D9 | LM Studio's MLX engine hits the same kext panic after updating to 0.4.20+1. The panicking task was LM Studio's bundled `node` | intensive MLX inference, M5 Pro 64 GB | `IOGPUMemory.cpp:550` | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/2249 |
| D10 | A related panic on the teardown path in LM Studio MLX (M3 Ultra 256 GB) | normal interactive use; 2 panics in 16 h | `"IOGPUGroupMemory::remove_memory_object() memory object not found" @IOGPUGroupMemory.cpp:323` | user report | https://github.com/ml-explore/mlx/issues/3186 (ferraro, 2026-05-11) |

### 1.2 Wired memory and teardown leaks

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| D11 | Wired "VRAM" is not released after sessions end. Killing exo does not free it; auto-restart re-leaks it within ~3 min | end or SIGTERM sessions on a 2× M3 Ultra TB5 cluster | GPU memory stays at 100% after `pkill -9` | CLOSED by exo PR #1889 ("Try harder to clean up processes nicely"); a comment reports recurrence with runners stuck in `RunnerShuttingDown` | https://github.com/exo-explore/exo/issues/1872 · https://github.com/exo-explore/exo/pull/1889 |
| D12 | **SIGKILL of a hung JACCL rank leaves the Metal/RDMA wired pages held by the kernel until reboot** | a peer is lost under an established queue pair; SIGTERM is never acted on because the thread is blocked in native code | the reporter states it directly | OPEN | https://github.com/ml-explore/mlx/issues/3910 |
| D13 | After a lost JACCL completion, **about 92–94 GiB stays wired on the M5 until reboot** | MLX 0.32.2 NAX MoE regression (#4352) under heterogeneous TP (M3 Ultra + M5 Max) | `all_reduce made no progress for 30001ms` (their patched timeout) | OPEN; reverting #4352 fixes it | https://github.com/ml-explore/mlx/issues/4403 |
| D14 | exo's macOS app memory creeps up with **no model loaded**; the workaround is to restart every few hours | idle | memory growth shown in mactop | OPEN | https://github.com/exo-explore/exo/issues/2262 |
| D15 | Distributed OOM on nodes with different RAM: the 64 GB node overfills, then another rank dies on a GPU timeout | pipeline split across 192/128/64 GB machines | `[METAL] Command buffer execution failed: Caused GPU Timeout Error (00000002:kIOGPUCommandBufferCallbackErrorTimeout)` | OPEN | https://github.com/ml-explore/mlx/issues/1804 |

### 1.3 Rank hangs and deadlocks

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| D16 | **JACCL never detects a lost peer.** Survivors busy-wait forever at 100% CPU. Taking the IP interfaces down does *not* stop RDMA traffic | one rank SIGSTOPped or dead | ranks in state `Rs+` at 99–100% CPU; no exception, nothing on stderr | OPEN; fix PR #4530 is open (side-channel liveness; "peer is gone" error in ~0.25 s) | https://github.com/ml-explore/mlx/issues/4278 · https://github.com/ml-explore/mlx/pull/4530 |
| D17 | Same class: `MeshImpl::recv` spins forever on a UC queue pair after a TB link drops mid-operation | peer lost after init | `sample` shows the main thread in `Event::wait` and the StreamThread in `jaccl::MeshImpl::recv` | OPEN | https://github.com/ml-explore/mlx/issues/3910 |
| D18 | **Ring backend** (TCP): killing one rank left the survivors hung; in current main they fail instead | peer close; `recv()==0` read as EAGAIN | 100% CPU spin, no log | FIXED by PR #4060 (merged 2026-08-09) plus PR #3742 (catchable error, merged 2026-08-17). v0.32.0 still hangs | https://github.com/ml-explore/mlx/pull/4060 · https://github.com/ml-explore/mlx/pull/3742 · https://github.com/ml-explore/mlx/issues/3862 |
| D19 | Ring `all_sum` on the CPU stream deadlocks on about 3–4% of calls. On the GPU stream, a timing skew between ranks gets the waiting rank killed by the ~5 s Metal watchdog | repeated gradient-sized all_sum, ≥3 ranks, gigabit LAN | ranks blocked in `recvfrom`/`sendto`; `kIOGPUCommandBufferCallbackErrorTimeout` | OPEN | https://github.com/ml-explore/mlx/issues/4475 |
| D20 | GPU stuck in `fence_wait` at 100% until reboot (JACCL + `METAL_FAST_SYNCH=1`); more likely with 4 nodes and 2 models in parallel | nondeterministic | GPU pegged, runner spinning | OPEN (partial fixes #3141/#3144; a commenter reports `HazardTrackingModeDefault` removes it) | https://github.com/ml-explore/mlx/issues/3142 |
| D21 | Pipeline split + JACCL deadlocks at `Fence::wait` → `condition_variable::wait` on the first warmup prefill; the fence patch is not enough | macOS 26.5, 2× M3 Ultra | identical `sample` stack on both ranks; no CPU spin | OPEN | https://github.com/exo-explore/exo/issues/2173 |
| D22 | **Long-context requests (45–52k tokens, typical of agentic harnesses) to a 2-node TP/JACCL instance hang at the prefill→first-token step or return garbled text.** Short prompts never trigger it | large system prompt plus tool schemas | stall at 49,152/52,417 prefill with near-idle CPU | OPEN (candidate fix e9835615 adds its own deadlock) | https://github.com/exo-explore/exo/issues/2208 |
| D23 | Pipeline prefill of ~50k tokens freezes both ranks silently; the control plane stays up, cancel is ignored | memory-tight node (24 GB) just after KV evictions / context compaction | both ranks silent after `Starting prefill` | OPEN | https://github.com/exo-explore/exo/issues/2199 |
| D24 | Pipeline deadlock at 32+ concurrent requests: the `all_gather` agreement collective blocks | high concurrency | 3/32 succeed, cluster dead | OPEN; a maintainer could not reproduce on 2 Mac Studios | https://github.com/exo-explore/exo/issues/2108 |
| D25 | **Distributed `mlx_lm.server` over JACCL: two concurrent HTTP requests kill both ranks** | two simultaneous POSTs | `_pickle.UnpicklingError: invalid load key`, `[jaccl] Recv failed with error code -12`, `rank 0 exited with code 255`, client gets an empty reply | OPEN; serializing admission avoids it | https://github.com/ml-explore/mlx-lm/issues/1737 |

### 1.4 JACCL / RDMA link faults

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| D26 | **Unplugging the TB cable mid-collective SIGSEGVs both ranks** (4/4) | link loss | `EXC_BAD_ACCESS` in `libthunderboltrdma.dylib tbt_post_recv` ← `jaccl::Connection::post_recv` | OPEN | https://github.com/ml-explore/mlx/issues/4192 |
| D27 | The same SIGSEGV with **no unplug, no model, ports still `PORT_ACTIVE`**, on the 8th 256 MiB all_sum. `mlx.launch` printed exit 0 while a rank exited 255 | periodic large all_sum | `tbt_post_recv`, fault address ending `…008`; Apple FB24371487 | OPEN | https://github.com/ml-explore/mlx/issues/4319 |
| D28 | JACCL crashes several times a day, **including at idle after 23 min with no inference**; needs a full reboot of every node | ~10 model load/unload cycles a day | `[jaccl] Recv failed with errno=2` / `errno=60`; `Changing queue pair to RTR failed with errno 22` on all nodes at once | OPEN | https://github.com/exo-explore/exo/issues/1847 |
| D29 | RDMA ports stop working after the cluster sits idle overnight; still dead after an exo restart and a reboot | inactivity | `Changing queue pair to RTR failed with errno 96` | OPEN | https://github.com/exo-explore/exo/issues/1973 |
| D30 | RTR errno 22 on every init since the JACCL refactor #3412: the GID filter accepts only IPv4-mapped GIDs, while TB exposes only `fe80::` GIDs | any JACCL init without an IPv4 address on the TB port | `ValueError: [jaccl] Changing queue pair to RTR failed with errno 22` | OPEN (workaround: an IPv4 address on each TB port) | https://github.com/ml-explore/mlx/issues/3467 |
| D31 | Random JACCL errors whose only recovery is re-plugging the cable or rebooting; **`ibv_devinfo` hangs when this happens** (proposed as a detector) | — | `ibv_devinfo` hangs | OPEN (enhancement) | https://github.com/exo-explore/exo/issues/1712 · https://github.com/exo-explore/exo/issues/1711 |
| D32 | Null protection-domain SIGSEGV at init when RDMA is absent or the port is down | TB link without RDMA (e.g. a 40 Gb/s cable on M4 minis) | `ibv_reg_mr_iova2` ← `jaccl::SharedBuffer::register_to_protection_domain` | OPEN; #4530 reports that `ibv_alloc_pd` returns NULL on `PORT_DOWN` | https://github.com/exo-explore/exo/issues/2186 · https://github.com/ml-explore/mlx/pull/4530 |
| D33 | **Silent output corruption:** TP over JACCL returns token salad once the prompt goes past one prefill chunk (~2k tokens). Deterministic per server instance; decode is always clean | needle test at temperature 0 | "0.0.00…" output above ~2.6–4.5k prompt tokens | mlx#4342 CLOSED; exo#2270 reports it unchanged on mlx 0.32.0.dev + macOS 26.6.2; related #3149 (consecutive send/recv with different shapes) OPEN | https://github.com/ml-explore/mlx/issues/4342 · https://github.com/exo-explore/exo/issues/2270 · https://github.com/ml-explore/mlx/issues/3149 |

### 1.5 Orchestration state (exo control plane): the analogues of our relaunch and switch races

| # | failure | signature | status | source |
|---|---|---|---|---|
| D34 | After an exo crash and auto-restart, peers reconnect over **Tailscale instead of Thunderbolt**: latency goes from <1 ms to 40–70 ms and throughput from 24 to 2.5–7.9 tok/s. `/v1/models` answers while `/v1/chat/completions` fails | "connected", but slow or connection refused | OPEN; link-preference PR #2215 was closed unmerged | https://github.com/exo-explore/exo/issues/1723 · https://github.com/exo-explore/exo/pull/2215 |
| D35 | The ring hostfile uses LAN addresses even when a TB link exists (8.7 vs 25.4 tok/s) | wrong interface chosen | OPEN | https://github.com/exo-explore/exo/issues/2295 |
| D36 | After ~48 h, SIGKILLed runners (memory pressure) start a cascade: the tensor-shard connection is never restored and both nodes spin at 97–99% CPU with no inference | `Runner terminated with signal=9`; HTTP 200 with no body | OPEN | https://github.com/exo-explore/exo/issues/1823 |
| D37 | Runner churn deadlock: 16 runners (9 `ShuttingDown`), 98 `CreateRunner` tasks pending, requests hang | state frozen across samples | OPEN | https://github.com/exo-explore/exo/issues/1934 |
| D38 | The runner process is gone but the instance stays registered `READY` with `shards: 0`; every request times out | stale state | OPEN | https://github.com/exo-explore/exo/issues/2120 |
| D39 | A departed node's identity lingers for 10+ min and blocks RDMA placement; restarting nodes adds more ghosts | `no RDMA-connected cycles available` | OPEN | https://github.com/exo-explore/exo/issues/2194 |
| D40 | A peer disconnect crashes the process (`BrokenResourceError`), or kills only the API: the process stays alive while the port stops listening | zombie that systemd won't restart | CLOSED by PR #2102 | https://github.com/exo-explore/exo/issues/2101 |
| D41 | After a transient `EHOSTUNREACH` (right after boot, **wake** or a link change), discovery permanently drops that interface | nodes never rediscover | OPEN | https://github.com/exo-explore/exo/issues/2191 |
| D42 | Running `macmon` during MLX inference makes Metal throw on the completion-handler thread and crashes the process | `mlx::core::gpu::check_error` → `std::terminate` → SIGABRT | OPEN | https://github.com/exo-explore/exo/issues/2088 |

---

## 2. LM Studio LM Link and remote/headless serving

LM Link runs on Tailscale's tsnet ("Tailscale's internal end-to-end encrypted connections"). A device that
crashes without telling the discovery server keeps showing as "disconnected", and LM Studio's FAQ admits
this: https://lmstudio.ai/docs/lmlink/basics/faq

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| L1 | A chat over LM Link idles ~10 min, then the next message gets an empty answer and the link drops and re-establishes. Loading the same model locally works | idle period, then a new turn in a long context | "This message contains no content…" / "Cannot auto reload last used model… no longer exists" | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1983 |
| L2 | The Link drops at 50% of a remote model load and the model is unloaded | a remote load from headless lms; conflicts with another Tailscale on the same host | `LM Link connection entered error state peer_keepalive_timeout`; workaround `TS_DEBUG_OMIT_LOCAL_ADDRS=1` | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/2184 |
| L3 | **Two tailscales on one host:** with system Tailscale using an exit node, LM Link cannot connect | Tailscale exit node enabled | connection timeout | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1692 |
| L4 | Link won't start without an interactive session (service or Task Scheduler) | daemon launched non-interactively | `[LMLinkProvider] Auto relink attempt failed (reason: offline): Error: tsnet_up_failed`; `ECONNRESET` on the WS server | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1639 |
| L5 | Large messages or attachments over Link fail | ~10–15 images in one chat | `Failed to send message write EPIPE`; still present in 0.4.20 | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1869 |
| L6 | Models loaded remotely over Link are never unloaded on the host and RAM fills up | switching many models from the remote client | host memory clogged | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/2032 |
| L7 | The client addresses a *local* model while the user targets a remote one | model selection on a Link client | `Cannot find model "<local id>"` | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1605 |
| L8 | "Internal server error" on the Link tab after months of working; restarts don't help | unknown | — | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1945 |
| L9 | Headless daemon keeps an orphaned "in progress" download after the client disconnects; only `lms daemon down && up` clears it | SSH client killed mid-`lms get` | "This download is already in progress" at 0% | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/1848 |
| L10 | The server drops a long (70k-token) request after ~2 min, retries it 3 times, then returns 503 | slow prefill | `Client disconnected` then 503 | OPEN | https://github.com/lmstudio-ai/lmstudio-bug-tracker/issues/693 |
| L11 | MLX 8-bit multi-turn tool-call streaming is aborted mid-stream while the client is still waiting | tool calls, streaming | `[LM STUDIO SERVER] Client disconnected. Stopping…` | CLOSED | https://github.com/lmstudio-ai/lms/issues/460 |

**What the changelogs say was fixed.** The LM Studio 0.4.x release notes carry almost nothing about Link
reliability. The relevant entries are: 0.4.5 "Fixed a bug where LM Link connector was not included in
in-app updater"; 0.4.6 "Updated go version to 0.25.7 for LM Link"; 0.4.7 "Add notification UI when LM
Link versions are incompatible between devices"; 0.4.15 "Fixed REST API requests hanging when HTTP/2
clients sent upgrade headers"
(https://lmstudio.ai/changelog/lmstudio/lmstudio-v0.4.5, …-v0.4.6, …-v0.4.7, …-v0.4.15). The
connection fixes are in the **Bionic** changelog: 1.1.0 "More reliable LM Link connections during
high-volume activity" and "Fixed LM Link connections to newly discovered peers"; 1.1.3 "Smoother LM
Link connection states and remote model listings" (https://lmstudio.ai/changelog). No entry names
disconnect, reconnect or sleep specifically.

**UNVERIFIED:** I found **no primary report of LM Link failing on sleep/wake**, and no report of a
request hanging with no error after the peer drops. L1 is the closest (idle → empty answer →
re-link). The owner's own observation stays the evidence for that class.

---

## 3. Tailscale: userspace networking, SOCKS5 / HTTP proxy, DERP, sleep

Docs: userspace mode is "primarily for serverless environments"; the SOCKS5 proxy is general and the HTTP
proxy handles only HTTP (https://tailscale.com/kb/1112/userspace-networking). Without a direct path,
traffic goes through peer relays and then falls back to DERP (https://tailscale.com/kb/1232/derp-servers).

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| T1 | **SOCKS5 drops the response after a client half-close.** `handleTCP` returns when the first copy pump ends, so a client that sends its request and does `Shutdown(Write)` gets 0 response bytes | request, half-close, then a response | backend receives 8 MiB + EOF; client receives 0 bytes | CLOSED 2026-09-17 (fix merged; the version it ships in is **UNVERIFIED**) | https://github.com/tailscale/tailscale/issues/20883 |
| T2 | **Userspace uploads are ~5× slower than downloads** because netstack has RACK disabled: reordering is treated as loss (8 of 9 retransmits were spurious) | the netstack side is the TCP *sender* (for us: MacBook → Studio request bodies and model copies) | upload 22–45 Mbps vs 160–245 down | OPEN; a maintainer says enabling RACK "tanks macos <-> tsnet on windows performance" | https://github.com/tailscale/tailscale/issues/20893 |
| T3 | tailscaled memory grows until the container is killed, with `--tun=userspace-networking --socks5-server --outbound-http-proxy-listen` and a **long-lived, thin streamed response running for hours** | a long SSE-like stream through the proxy | RSS climbs to the limit | CLOSED as cannot-reproduce | https://github.com/tailscale/tailscale/issues/7571 |
| T4 | Proxied TCP through userspace SOCKS5 stalls every ~20–26 s during magicsock rebind cycles, with no RST | failing IPv4 STUN on an IPv6-only interface | the client waits out its timeout | OPEN | https://github.com/tailscale/tailscale/issues/19791 |
| T5 | HTTP POST bodies over 840 bytes hang forever in userspace mode on Cloud Run; `TS_DEBUG_MTU=1024` fixed it for one commenter | MTU/path issue | hang, no error | OPEN | https://github.com/tailscale/tailscale/issues/9894 |
| T6 | **The control plane reports healthy while the data plane is dead:** `BackendState: Running`, `Online: true`, but magicsock's receive loop has stopped. `down/up` and pkill don't help; only an app relaunch does | after boot; after a hung NE session | health: "The MagicSock function ReceiveIPv4 is not running" plus a DERP unreachable warning | OPEN; fix PR #21396 open | https://github.com/tailscale/tailscale/issues/20616 · https://github.com/tailscale/tailscale/pull/21396 |
| T7 | Peers reachable only via a DERP region go dead until `tailscale debug break-derp-conns` | stale DERP path | `tailscale status` shows active via relay; `tailscale ping` times out | CLOSED: fixed in 1.98.8 | https://github.com/tailscale/tailscale/issues/19981 |
| T8 | If the IPv6 disco ping fails, IPv4 endpoints are never tried and the connection **stays on DERP indefinitely** | IPv4-only peer, dual-stack other side | `sendto: network is unreachable` for the IPv6 endpoint | OPEN | https://github.com/tailscale/tailscale/issues/20102 |
| T9 | **macOS sleep/wake:** the TUN write fails and TCP dies while ICMP and `tailscale ping` still work; `down/up` doesn't fix it; a second repro happened without sleep | wake from sleep (Network Extension build) | `wg: Failed to write packets to TUN device: write /dev/tun: no space left on device` | OPEN. **Applies only to the NE/TUN build, not to userspace mode** (my inference) | https://github.com/tailscale/tailscale/issues/20859 |
| T10 | **Kernel panic caused by tailscaled** (Homebrew CLI tailscaled on a Mac mini M4 Pro, 19 days uptime with sleep/wake) and by the NE on a Mac Studio M4 Max | an interface refcount leak until wraparound | `dlil_if_ref: wraparound refcnt for ifp=… @dlil_subr.c:410`, panicked task `tailscaled` | OPEN (Apple feedback filed) | https://github.com/tailscale/tailscale/issues/18953 |
| T11 | Endpoints aren't re-advertised after sleep or an IP change, so there is no direct connection; the workaround is `tailscale down && up` | wake, or a network move | stuck relayed or unreachable | OPEN | https://github.com/tailscale/tailscale/issues/6985 |
| T12 | Linux: nothing reconnects after resume; some reports are DNS-only (`no upstream resolvers set, returning SERVFAIL`); one headscale user fixed it by moving the control server to a bigger host | resume | peers unreachable | OPEN | https://github.com/tailscale/tailscale/issues/10688 |
| T13 | Windows Modern Standby: traffic is dead for ~10 min after resume while the GUI says connected | resume | self-heals after ~10 min | OPEN | https://github.com/tailscale/tailscale/issues/20636 |
| T14 | libtailscale (tsnet-class embedding) never recovers once iOS reclaims its sockets on suspend | app suspended ~30 s | dead until restart | OPEN | https://github.com/tailscale/tailscale/issues/21353 |
| T15 | **headscale:** after a network switch or sleep, a macOS client can't reconnect because the server holds the old stream | wake / network change | `node has an open stream(…), rejecting new stream` | CLOSED (2024) | https://github.com/juanfont/headscale/issues/1942 |
| T16 | **headscale:** a superseded session's reconnect-grace timer marks a freshly reconnected node offline | fast reconnect | node online=false right after "node has connected" | CLOSED 2026-08-30 | https://github.com/juanfont/headscale/issues/3428 |
| T17 | **headscale:** deleting a node doesn't close its long-poll, so the client is a zombie that thinks it is healthy | `nodes delete` / ephemeral GC | `tailscale status` up; nothing logged | CLOSED 2026-09-09 | https://github.com/juanfont/headscale/issues/3410 |
| T18 | headscale's embedded DERP STUN binds and logs "started" but never answers | v0.29.2 | no STUN replies, so no NAT discovery | CLOSED 2026-07-14 | https://github.com/juanfont/headscale/issues/3379 |

**UNVERIFIED:** I found no primary report of a *fixed idle timeout* that kills long-lived streaming TCP
through the userspace SOCKS5/HTTP proxy. T3 (memory on long thin streams) and T4 (periodic stalls) are
the closest. I also found no Tailscale changelog line tying a fix to T3.

---

## 4. MLX single-server serving under long agentic workloads

| # | failure | trigger | signature | status | source |
|---|---|---|---|---|---|
| S1 | Metal OOM aborts the whole server instead of returning an HTTP error | KV growth over ~12 tool-call turns | `libc++abi: terminating … [METAL] Command buffer execution failed: Insufficient Memory (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory)` | OPEN | https://github.com/ml-explore/mlx-lm/issues/854 |
| S2 | The prompt cache grows to 23–26 GB and the server aborts. Client disconnects mid-stream leave `BrokenPipeError` and appear to leave cache state behind | large prompts plus interrupted streams | `Prompt Cache: 10 sequences, 26.28 GB`, then the OOM abort | OPEN | https://github.com/ml-explore/mlx-lm/issues/1390 |
| S3 | `fetch_nearest_cache` deep-copies the KV, so **reusing** a cached conversation briefly takes 2× its memory | continuing the longest conversation | OOM abort on the completion-handler thread (uncatchable) | OPEN | https://github.com/ml-explore/mlx-lm/issues/1395 |
| S4 | **Long-running server memory grows past the prompt-cache cap on hybrid-attention models (Qwen3.8-27B 4-bit)**; RSS under-reports it, so footprint must be measured instead | multi-day serving | true footprint above 30 GB for a 16 GB model with an 8 GB cap | OPEN | https://github.com/ml-explore/mlx-lm/issues/1807 |
| S5 | **Invisible wedge:** the generation thread dies while the HTTP listener keeps accepting, and every later request hangs with 0 bytes | an exception in `extract_cache` (OOM) | no error to the client; `/v1/models` fine | OPEN | https://github.com/ml-explore/mlx-lm/issues/1909 |
| S6 | Metal handle-count exhaustion (a count of live buffers, not bytes) wedges the server invisibly | 145k-token history sent to a 128k server; continuous batching | `[metal::malloc] Resource limit (499000) exceeded` in `BatchGenerator._generate` | CLOSED | https://github.com/ml-explore/mlx-lm/issues/1672 |
| S7 | The same handle exhaustion at a steady ~55 per day on Rapid-MLX 0.13.1 (hybrid Qwen3.6-35B-A3B); each one aborts the whole batch | sustained 2–4 in flight | `Resource limit (499000) exceeded` in `_step`/`async_eval` | CLOSED 2026-09-04 | https://github.com/raullenchai/Rapid-MLX/issues/2836 |
| S8 | **Rapid-MLX: client-disconnect aborts leak scheduler `running` slots** until every slot is a ghost; `/v1/models` and `/metrics` stay 200 while nothing completes (4 times in 6 days) | streaming clients disconnecting | `running=4` forever, 0 completions | CLOSED 2026-08-10 | https://github.com/raullenchai/Rapid-MLX/issues/1759 |
| S9 | **Rapid-MLX: a leaked admission reservation** (client disconnects between `return StreamingResponse` and the first body read) makes every `replace_group` load report busy until restart | disconnect race | loads refused as busy | CLOSED 2026-08-29 | https://github.com/raullenchai/Rapid-MLX/issues/2465 |
| S10 | Rapid-MLX wedges for hours under Metal-cap backpressure (0 KV attributed): mass 503s, streams send the role chunk then nothing, then an uncaught OOM SIGABRT | long agent chats plus JSON extraction plus keep-warm pings | "retry after pressure drops" forever | CLOSED 2026-07-11 | https://github.com/raullenchai/Rapid-MLX/issues/1058 |
| S11 | Rapid-MLX SIGSEGV on **every** shutdown (a dyld TLS finalization race with the MLX StreamThread) | Ctrl+C / kill after one request | `EXC_BAD_ACCESS` at `0x350`; a "Python quit unexpectedly" .ips on each stop | CLOSED 2026-09-17 | https://github.com/raullenchai/Rapid-MLX/issues/3495 |
| S12 | Loading a large model mmaps the checkpoint and copies it into Metal buffers at the same time, so resident memory briefly doubles and the serve gets jetsam-killed when another tenant is resident | load with ~40 GB of other resident memory | exit 137 ×4; swap full, compressor at 116 GB | OPEN | https://github.com/raullenchai/Rapid-MLX/issues/3709 |
| S13 | **A server spawned from a launchd LaunchAgent (XPC-service context) is pinned to a background GPU performance state:** GPU shows ~99% but a 30k prefill goes from 4–9 s to more than 60 s | `$XPC_SERVICE_NAME` set in the spawner | prefill ~100× slower, "stall" | OPEN (stopgap: relaunch via `ssh localhost`) | https://github.com/raullenchai/Rapid-MLX/issues/1363 |
| S14 | `mlx_lm.server` never calls `mx.clear_cache()` on a model swap and keeps every model it has loaded in the buffer pool | a second model load | `cache=` grows by the old model's size | CLOSED 2026-08-18 | https://github.com/ml-explore/mlx-lm/issues/1757 |
| S15 | A one-shot eval of a model over 300 GB trips the IOGPU command-buffer watchdog at load | huge model load | crash; chunked eval fixes it | OPEN | https://github.com/ml-explore/mlx-lm/issues/1572 |
| S16 | Ollama MLX runner memory grows with each identical prompt (`ollama ps` size rises) | 63 repeated prompts, gemma4:31b-mlx, 128k context | growing model size | CLOSED | https://github.com/ollama/ollama/issues/17783 |
| S17 | The wired-limit knob: mlx-lm's README says to raise `iogpu.wired_limit_mb` to a value "larger than the size of the model in megabytes but smaller than the memory size of the machine" | — | — | documentation | https://github.com/ml-explore/mlx-lm/blob/main/README.md |

**Our engine.** The local Rapid-MLX fork calls `mx.set_wired_limit` in `rapid_mlx/mllm_batch_generator.py:1389`,
`rapid_mlx/distributed/pipeline_qwen4.py:705` and `rapid_mlx/models/deepseek_v41_native/load.py:193`, and
calls `mx.clear_cache()` periodically in `rapid_mlx/scheduler.py` and `engine_core.py` (grep of
`~/Projects/Rapid-MLX`, HEAD 2f02ac645). That wired-limit-plus-periodic-clear_cache profile is exactly
D3's panic arm. **UNVERIFIED:** whether the text lane we serve goes through a path that sets the wired
limit. Check `mx.get_wired_limit()`-equivalent behaviour or the `vm_stat` wired count at mount.

---

## 5. Test proposals for our six rows

Detectors used below (all per Mac, sampled every 30 s by the harness):
- **panic census:** new files in `/Library/Logs/DiagnosticReports/*.panic`, plus `log show --last boot --predicate 'eventMessage CONTAINS "panic"'`. The strings to match are `IOGPUMemory.cpp:550`, `IOGPUGroupMemory`, `watchdog timeout`, `dlil_if_ref`.
- **crash census:** new `*.ips` whose stack contains `tbt_post_recv`, `check_error`, `ibv_reg_mr`, `Fence::wait`, or the engine's name.
- **wired:** `vm_stat` "Pages wired down" × page size. **engine footprint:** `footprint <pid>` or `vmmap --summary <pid>` (not RSS; S4 shows RSS lies). **tailscaled footprint** the same way.
- **rank spin:** a rank pid at ≥95% CPU while the engine's token counter is flat for 3 samples in a row. On the first such sample, capture `sample <pid> 5` and look for `MeshImpl::recv`, `Fence::wait` or `recvfrom`.
- **invisible wedge:** `/v1/models` returns 200 while a 1-token canary completion returns nothing within the soak rule. The canary is what is judged, never the health endpoint (S5, S8, D34, D36).
- **path:** `tailscale status --json` → per-peer `CurAddr` (direct) vs `Relay` (DERP), plus `tailscale debug` health warnings (T6 "ReceiveIPv4 is not running").
- **correctness canary:** a needle-in-haystack prompt at temperature 0 at 400 / 2.1k / 2.7k / 4.5k / 7.3k / 15k / 30k tokens, answer checked against the known code (D33). A throughput number is never the pass criterion.

### R1: Split long soak (D1–D6, D12, D16–D17, D19–D23, D25–D28, D33)
- **Trigger:**
  1. **Correctness first.** Run the needle ladder on the split before the soak and every hour during it. D33 says JACCL TP can silently return garbage above ~2k-token prompts while decode looks fine.
  2. **Growth.** Drive the goose agentic session so context climbs from 2k to >50k tokens through tool-call turns, and let compaction fire at least once. D22 and D23 are exactly this shape: hang at prefill → first token, or freeze after evictions or compaction.
  3. **Idle gap.** Stop sending for 30 min, then send one turn. D28 crashed at idle after 23 min; D29 died after an overnight idle.
  4. **Concurrency.** Send two turns at once from two sessions. D25 killed both ranks this way on distributed `mlx_lm.server`.
- **Measure:** the correctness canary; rank spin with a captured `sample` stack; per-rank exit codes read from the engine itself (D27: `mlx.launch` printed exit 0 while a rank exited 255); wired memory flat after warm-up on both Macs; the panic and crash census on both Macs; engine footprint. **Fail if** any canary answer is wrong, any rank spins with flat tokens, any panic or ips file appears, or wired memory grows monotonically after the first hour.

### R2: Remote single under heavy load over Link (D3, D6, S1–S10, T1–T5)
- **Trigger:**
  1. **The panic recipe.** Two or more concurrent streaming turns from the MacBook, each with a *unique* 12–16k prompt, enough to push the Studio engine's prefix cache into eviction churn. This is D3's reproducer, 102–108 s to panic on stock wiring, and the Studio in question is the 96 GB M3 Ultra class of D1 and D5. Run it for at least 10× D3's time-to-crash (≥20 min) per arm.
  2. **Disconnect churn.** At the same time, abort about 20% of streams mid-body from the client side (S8, S9, S2).
  3. **Bulk transfer.** At the same time, copy a model MacBook → Studio over Link. The netstack side is then the TCP sender (T2, ~5× slower upload) and competes with request bodies.
  4. **Half-close.** One run where the client half-closes after the request body (T1). Confirm our relay never depends on SOCKS half-close semantics, or that our tailscaled already has the fix.
- **Measure:**
  1. The Studio panic census and log matches for `Resource limit (499000)` (S6, S7).
  2. Engine `running`/admission counters return to 0 within one soak-rule window after all clients stop. This is the S8 ghost-slot check.
  3. Engine footprint vs prompt-cache cap (S4).
  4. tailscaled footprint on both Macs, flat after warm-up (T3).
  5. Relay 5xx other than admission 503.
  6. TTFT and inter-token-gap p95/p99 per concurrency level, and whether the path is direct or DERP for the whole run.
- **Add an arm** with the engine's `set_wired_limit` call disabled (D3's mitigation). If stock panics and that arm doesn't, we have our own measured cause.

### R3: Link disconnect/reconnect (L1–L3, T6–T8, T15–T17, S8–S9, D34, D39)
- **Trigger:** each one idle and mid-stream, from each side.
  1. Kill tailscaled's pid (per-pid, never a group).
  2. Link off/on in the UI.
  3. Wi-Fi off/on on the MacBook (network change).
  4. Block UDP 41641 with a pf rule to force DERP, then lift it and time the upgrade back to direct (T7, T8).
  5. Restart headscale.
  6. Delete and re-register the node in headscale (T17 zombie).
  7. Quit the peer app so its identity must go stale (D39, and the LM Link FAQ's crashed-but-still-listed device).
  8. After a 10+ min idle, send a turn (L1).
- **Measure:**
  1. Time from the break to the first error byte at the client. It must be bounded and name the break; a silent hang is a fail (L1, T6).
  2. Time from restore to the first good token, with no click.
  3. `tailscale status --json` Online/CurAddr/Relay, and health warnings; flag "Running + Online while ReceiveIPv4 not running" as the T6 signature.
  4. headscale `nodes list` online bit vs reality (T16).
  5. Engine side: the orphaned request released its slot and admission reservation (S8, S9).
  6. The UI's device list must not show a dead peer as available (D38, D39 class).

### R4: Relaunch/restore races (D11–D13, D28–D31, D34–D38, D40, S5, S11)
- **Trigger:**
  1. Relaunch the app on either Mac during mount, during split start, and with both Macs relaunching together, 20 times.
  2. SIGKILL one rank mid-generation, then relaunch.
- **Measure:**
  1. **Wired memory returns to its pre-mount baseline** after teardown on both Macs. D12 and D13 report SIGKILLed JACCL ranks leaving 90+ GB wired until reboot; D11 reports exo re-leaking after restart.
  2. The **interface actually used** by the restored split is Thunderbolt, not the tailnet or LAN (D34, D35), and tok/s is within 10% of the pre-relaunch baseline.
  3. JACCL re-init success rate across 20 cycles, plus `ibv_devinfo` answering promptly (D31 detector) and any RTR `errno 22/60/96` (D28–D30).
  4. "Serving" is claimed only after the 1-token canary succeeds (S5, D36–D38, D40).
  5. The crash census after each stop (S11: a shutdown SIGSEGV must not be scored as a runtime failure, and must not hide one).

### R5: Switch races (D1 load/unload churn, D4, D37, S9, S12, S14, L6)
- **Trigger:**
  1. Start placement B while placement A mounts.
  2. Click Run twice.
  3. Stop during provisioning.
  4. Two windows each starting a different placement.
  5. Alternate single ↔ split on different models 20 times (D1 names repeated load/unload as a panic accelerator; D4 names clear_cache racing a live generate thread).
- **Measure:**
  1. A `pgrep` census on both Macs: exactly one engine and no orphan rank (D37 runner churn).
  2. Wired and footprint match the mounted model ± margin after every switch, with no retained previous model (S14, L6).
  3. `vm_stat` swap/compressor and `log show --predicate 'eventMessage CONTAINS "jetsam"'` during each load (S12 load transient).
  4. No "busy" refusal once idle (S9).
  5. The panic census.

### R6: Sleep/wake (T9–T15, D29, D41, S13, L1)
- **Trigger:**
  1. `pmset sleepnow` on the Studio, idle and mid-stream (remote single), then wake it via `pmset schedule wake` or over the network.
  2. Close and open the MacBook lid mid-stream.
  3. The same during a split (TB link across sleep).
- **Measure:**
  1. A stream in flight at sleep ends with a named error within a bound, not a hang.
  2. Time from wake to the first good token.
  3. Direct vs DERP after wake, and time to return to direct (T11, T8).
  4. tailscaled health, including "ReceiveIPv4 is not running" (T6).
  5. A panic census on both Macs for `dlil_if_ref` over multi-day runs (T10 is a Homebrew tailscaled panic that followed sleep/wake).
  6. For the split: JACCL re-init after wake, errno 96/22 (D29), and discovery not permanently dropping an interface after a transient `EHOSTUNREACH` on wake (D41).
  7. **Prefill tok/s after wake vs baseline**, and whether the engine process runs in an XPC-service context. `launchctl procinfo <pid>` or the `XPC_SERVICE_NAME` env would show it. S13 measured a ~100× prefill collapse for launchd-spawned servers, which matters wherever our sidecar is started by launchd.
- **UNVERIFIED:** no primary source found for LM Link or MLX-engine-specific sleep/wake failures, so R6's engine-side expectations rest on the owner's observation plus S13 and D29.

---

## 6. Gaps: things I looked for and did not find

- No primary report of LM Link misbehaving specifically on sleep/wake, or of a request hanging after a peer drops (closest: L1, L2).
- No official LM Studio changelog entry for a Link disconnect/reconnect fix; only Bionic 1.1.0/1.1.3's generic "more reliable / smoother connection states".
- No report of a fixed idle timeout on long-lived streams through the Tailscale userspace SOCKS5/HTTP proxy.
- No maintainer confirmation of the D3 root cause (wired limit + concurrency + cache churn). It is one user's A/B plus one production data point.
- Which Tailscale release carries the T1 SOCKS5 half-close fix.
