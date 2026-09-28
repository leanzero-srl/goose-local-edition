# goose distributed tensor rank: one tensor-parallel rank of mlx_lm.server. Runs after
# rank_env.py (the shared prelude: `spec`, `emit`, the backend env, `mx`, `report_memory`).
#
# What it adds around mlx_lm.server, and why:
# - in-process memory caps at THIS node's GPU ceiling (max_recommended_working_set_size — the
#   budget preflight planned against is at most that), applied after mlx_lm.server's own
#   set_wired_limit, so an over-allocation raises in MLX instead of the kernel wiring past the
#   ceiling (exo panicked a 96 GB M3 Ultra that way); MLX's free-buffer cache at the plan's
#   transient allowance (Q-79), not at the ceiling's remainder;
# - every prompt-cache entry owns exactly its bytes (Q-79, `compact`): mlx_lm's extracted
#   entries kept whole padded batch buffers alive while counting one row, so the cache's byte
#   bound was enforced on a figure several times smaller than what it held;
# - a generation thread that dies ends the rank with RANK_FATAL and a non-zero exit (Q-79: the
#   Studio rank's Metal OOM exited 0);
# - rank 0's HTTP surface: /v1/models serves ONLY the goose model's names — its id, then every
#   other name goose's identity gives the same model (mlx_lm lists the HF cache, and a request
#   naming any of those would make every rank try to load it), the goose id maps to each rank's
#   OWN --model path (paths differ per node) and an alias is answered as the id, /goose/progress exposes the
#   generation loop's step counter (the supervisor's liveness measure), POST /goose/admission lets
#   the memory watchdog stop admitting new requests, and GET /goose/admission answers when it
#   admits again (rank_admission.py, Q-397: the 503 names the hold, a client waits on the lift);
# - the thinking switch (rank_thinking.py, Q-135): a chat request's `enable_thinking` resolved the
#   way the single engine resolves it (off unless pinned or asked for), so "auto" renders the same
#   prompt here as on Rapid-MLX instead of the template's own default (on, effort xhigh);
# - the generation budget (rank_budget.py): a request with no max_tokens generates until the model
#   stops or the launch's context window is full, never to mlx_lm's 512 default; an explicit one is
#   held inside the window. Rank 0 settles it BEFORE the request is shared, so every rank receives
#   the same integer — a peer running an older wrapper (its own goosed embeds its program) still
#   reads an int, never an absence it would crash on;
# - the doorbell (Q-66): mlx_lm's generation thread polls its request queue every 0.1 s while
#   idle and shares the (empty) answer through an all_sum every time; JACCL waits on a completion
#   queue created WITHOUT a completion channel (jaccl/rdma.cpp `create_cq(..., nullptr, nullptr,
#   0)`), so a waiting rank spins `ibv_poll_cq` — the worker ranks, parked in that all_sum while
#   rank 0 sleeps in queue.get, burned a whole core idle (Studio ~100%, MacBook 0.1%). With the
#   doorbell, an idle rank 0 shares only a request that exists, ringing one byte per worker first,
#   and an idle worker parks in recv(1) (kernel-blocking, no clock). While a batch runs every step
#   is shared exactly as upstream. The fork's pipeline runner has done the same since 286ed77f7;
# - the prompt cache's bounds (plan.rs RankPlan::prompt_cache_limit_bytes / prompt_cache_entries):
#   mlx_lm 0.31.3 trims its LRU prompt cache to `--prompt-cache-bytes` MINUS the live batch's KV,
#   and only when a request is admitted (server.py:795-798); `run` builds the cache without
#   max_bytes (server.py:1743), so the inserts between admissions are unbounded, and the default
#   `--prompt-cache-size` (10) evicts by entry count on its own. goose hands the flag the plan's
#   whole KV charge (live + cached), builds the cache with the same `max_bytes`, and sizes the
#   count so it never evicts before the bytes do. Every rank gets the same two numbers: each rank
#   runs its own cache over the same requests, and one that evicted differently would reuse a
#   different prefix than its peers (E2E #1, 2026-09-25: the agent's 48,647-token system prefix
#   was evicted and a 55,977-token turn was read cold, ~2.5 min on the split). An older
#   requester's spec (`prompt_cache_bytes` alone) runs its own wrapper's policy here unchanged:
#   that one number as the flag, mlx_lm's default count, no max_bytes — so both ranks still evict
#   alike.
# - the prefill's memory (Q-104, rank_prefill.py): a launch whose spec carries `prefill` sizes
#   every prefill step's chunk to the plan's workspace (the attention scores of a head_dim-256
#   layer exist whole: rows × heads × chunk × width), moves a batch whose every row finished its
#   prompt into generation without mlx_lm's deep copy of the whole padded KV, charges the live
#   batch at its PROJECTED padded KV when the prompt cache yields room, and rank 0 holds a request
#   the batch it would join cannot fit inside the plan's KV charge until the batch drains enough.
# - the rank's own account of its loop (Q-114, rank_state.py): its step count, where the loop waits
#   (poll / doorbell / share / batch), the rings, the batch's rows and each generating row's token
#   trail, published at every step and printed by the reporter; and, on SIGTERM, every thread's
#   Python stack. Both land in the rank's durable log (goosed's rank_log.rs), so a stall leaves each
#   rank's position behind — Q-114's could not be told apart for want of rank 1's.
# - the prompt cache's nearest-entry search in linear time (Q-162, rank_prompt_search.py): mlx_lm's
#   copied the whole path at every node it walked, and E2E #3e's 259,408-token compaction call sat
#   in it on the CPU with no step and no GPU time until goosed's hang rule stopped the split; the
#   search is published as the loop's `cache_lookup`, which the rule reads as progress while the
#   rank's CPU time advances.
# - every batch cache counter a step advanced is evaluated with the step (Q-114, `settle_counters`):
#   mlx_lm's lazy `left_padding -= N` pinned one Metal buffer per unread linear-attention layer per
#   decode step, and the 27B hit MLX's 499,000-buffer limit at ~10.5k generated tokens.
# - the group's formation handshake (Q-136, rank_formation.py) before the doorbell's port all_sum:
#   after an abnormal end the next group's first message could go nowhere and every collective
#   then paired one message off.
# - a streamed chat answer's tool calls stream as they are written (Q-141, rank_tool_stream.py):
#   mlx_lm sends nothing while a call is written — E2E #3c's 12,556-token call reached goose as 18
#   minutes of silence — so a relay sends the call's open frame and its arguments as OpenAI
#   `tool_calls` deltas, the single engine's shape, and the call the client assembles is the one
#   mlx_lm's parser reads from the whole text.
# - what a streamed chat answer's client has not been sent is visible (Q-146, rank_stream_watch.py):
#   E2E #3d's 10k-token agent call sent nothing for 17+ minutes and no one could read what the model
#   wrote, so /v1/status carries each streamed request's parser state, the streamer's position and
#   verdict, generated vs sent characters and the last words written, and a GOOSE_RANK_WITHHELD
#   line marks each span the handler withholds text in.
# - a chat request's stable prefix is kept (Q-142, rank_boundary.py): a spec that asks for it
#   (`transient_tail_boundary`) declares `rapid_mlx_transient_tail` + `..._on_tool` on /v1/models as
#   the single engine does, so goose names the turn-context block its request ends on; every rank
#   then ends a prompt segment where the prompt stops agreeing with the conversation minus that
#   block, and mlx_lm's own segment snapshot stores the entry the next request extends. Upstream
#   kept only the system prompt reusable across an agent's tool steps on this hybrid model (E2E
#   #3c: 31,385 of ~59k tokens read from cache per call).
# - that prefix stays cached across a concurrent request (Q-182): rank 0 admits a request into a
#   live batch only while the batch leaves the cache one prefix as wide as it (rank_prefill.py
#   `kept_prefix_bytes`), and on a spec that asks for it (`keep_newest_prefix`) the cache evicts the
#   newest stable prefix last (rank_boundary.py `pop_keeping_newest_prefix`). E2E #3h: a tool-label
#   helper joined each ~70–95k-token agent call, mlx_lm's type-count eviction kept the previous
#   call's unreusable end entry and dropped the prefix, and six calls re-read the whole prompt.
# - the prefix kept is the conversation's (Q-294, rank_boundary.py `KeptEntry`, on a spec
#   that asks for `keep_conversation_prefix`): the key a request naming its transient tail cut, not
#   the newest "user" entry, and a batch leaves room for it. E2E #3o turn 7: eight end-of-turn
#   helper rows took the newest-"user" protection with their own context segments, the cache was
#   trimmed below the agent's 3.96 GB prefix, and its next call read 108,801 tokens cold.
# - the chat's stable head — its system prompt and tools — is an entry of its own, kept after the
#   conversation prefix (Q-347, rank_boundary.py `cut_at_head`, on a spec that asks for
#   `keep_stable_head`): every agent request ends a segment where mlx_lm's own system segment
#   would end, and a batch leaves room for it too. E2E #3p: the head's only entry was evicted five
#   minutes into the chat, and the first request after the compaction re-read 44,053 tokens cold.
# - a request that names no sampling field samples as on the single engine (Q-159,
#   rank_sampling.py): mlx_lm filled the absence with its `--temp` 0.0 — greedy; E2E #3d wrote one
#   answer of 54 identical tool calls over 40 minutes. Rank 0 resolves each field request > goose's
#   profile > the checkpoint's generation_config.json > the single engine's fallback before the
#   request is shared (every rank samples from the arguments rank 0 shares, on the seed mlx_lm
#   synchronises at the generation loop's start), names a missing or unreadable config, and
#   /v1/status carries each request's `sampling` (the values that reached the sampler, and their
#   layer) and the engine's `sampling_defaults`.
# - a runaway tool-call answer ends (Q-161): a tool request's decode is held to the Qwen3-Coder XML
#   call's skeleton on a launch that asks for it (rank_xml_guard.py — the single engine's lz.7 guard;
#   the checkpoint writes `!` where its turn should end), and a streamed answer that writes a call
#   word for word again ends there, the repeat unsent (`StreamedToolCalls`) — E2E #3e's turn 0 was
#   one 221,604-token answer of the same write + mkdir pair, 324 times. Reopened on 3.0.57 (E2E
#   #3f): mlx_lm's GenerationBatch handed the tool request a departed tool-less row's processor
#   list, so the guard ran on one token; each row now keeps its own (`row_processors`,
#   rank_batch.py), and a streamed answer whose text outside its calls has become one span written
#   over and over ends there (`verbatim_cycle`, rank_stream_watch.py) — /v1/status names the last
#   such stop (`last_engine_stop`).
# - a request field the engine cannot honour is a named 400 on rank 0, before any rank sees the
#   request (Q-177, rank_request.py): mlx_lm's validator raised a bare ValueError its do_POST never
#   caught, so `top_logprobs` 12 closed the connection with no reply; `n`, `response_format`, a
#   `seed`, a malformed `stop` or `stream_options` are refused by name; and any other exception that
#   escapes the handler before a response began is a 500 naming it, never a dropped connection.
# - a request whose client went away ends at the next step (Q-181, `client_left`): mlx_lm noticed a
#   closed connection only when a write failed, and nothing is written while an answer's text is
#   withheld (a call's typed value, a held repeat, mlx_lm's own tool state) or while a non-streamed
#   answer is generated. After goose's Stop the split generated 48,466 tokens over 4,413 s for a
#   client that was gone, and would have run to max_tokens (222,148). Every piece the handler
#   receives — each prompt-progress report and each generated token — first reads the socket
#   without waiting: an EOF or a reset from the client is the signal (no clock). The handler stops
#   its generation context (rank 0 shares the row's removal with every rank at the next step, as
#   for any stop), and /v1/status's `last_engine_stop` and a GOOSE_RANK_CANCELLED_BY_CLIENT line
#   name it.
# - a dropped request leaves the batch at the next prompt step, and every row the batch holds is
#   listed (Q-231, `PromptStepBudget` / `DepartureContext` / `row_left`): mlx_lm removed stopped
#   rows and took new requests only after its whole step loop — about five steps on a group, each a
#   prompt slice of seconds — so E2E #3m's user turn waited 38.9 s behind three fact checks goose
#   had already dropped, while /v1/status listed none of them (a request left the table when its
#   handler did). Each row now names its client, its engine stop, whether it is held for room, and
#   stays listed (`leaving`) until its batch row is gone; GOOSE_RANK_STATE names each row's request
#   and GOOSE_RANK_ROW_LEFT says how long a stopped row held the batch.
# - a request whose client has left stops counting as running the moment anyone reads its socket,
#   and one not yet given batch work never gets any (Q-403, `client_departure` / `still_wanted`):
#   E2E #3r (2026-09-28) — goose dropped the end-of-turn reviewers at 13:40:37.6Z while one prompt
#   step read seven of them (steps 23862, 13:40:34.8 → 13:41:17.6Z) and an eighth (req-580) waited
#   held for room. Nothing read their sockets until that step ended, so /v1/status went on counting
#   eight handlers and the chat's own call, 4 s later, was picked with `free_slots 0`; then the
#   held req-580 was released into the emptied batch, read a 2,048-token chunk and only then was
#   found gone (13:41:24.36Z) — the chat's 207,104-token call held behind it the whole time. Now
#   /v1/status reads every live request's socket before it counts (a row whose client left is
#   listed, stopped `cancelled_by_client`, and not counted; its generation is told to stop and its
#   handler, wherever it waits, is woken to end), and rank 0 reads a held or arriving request's
#   socket before it shares it: a departed one is named once (phase `queued`) and never reaches
#   the batch. What cannot move is the step already running: MLX's forward pass is not
#   interruptible, so a row in it leaves at that step's end, as Q-231 already does.
# - a tool parameter typed through a union or a reference converts as that type (Q-232,
#   rank_tool_schema.py): mlx_lm's qwen3_coder read `{"type": ["string", "null"]}` as neither string
#   nor number and literal_eval'd the value — `10m` raised a SyntaxError and the whole call was lost,
#   `true` under ["boolean", "null"] was dropped, and a `$ref` object parameter arrived as its JSON
#   text. The parser's `_get_arguments_config` is wrapped so the whole-call parse and the streamer
#   read the type Rapid-MLX's parser reads (single engine, pipeline fork).
# - a parameter value that holds the text `</parameter>` arrives whole (Q-372, rank_tool_stream.py
#   `install_positional_parameters`): mlx_lm cut every value at its first `</parameter>` and the
#   call still succeeded — a `write` of `const close = "</parameter>";` became a file ending at
#   `const close = "`. The whole-call parse and the streamer read values positionally, as the
#   single engine's parser does (Rapid-MLX `tool_call_scan.py`).
# - a non-streamed answer whose tool call the parser refuses is a named 500 (Q-233,
#   `NamedToolCallFormatter`): mlx_lm's ToolCallFormatter skipped a ValueError (the call silently
#   gone from a 200) and let any other refusal escape after its 200 was buffered — a SyntaxError
#   (literal_eval of a word) closed the connection with no reply. The 500 carries the parser's words
#   and the call's text; a streamed answer is untouched (its streamer already leaves the refused
#   call's arguments unterminated). Q-177's 500 now covers every failure before a byte reached the
#   client, a buffered-but-unsent status line included.
group =mx.distributed.init(strict=True, backend=spec["backend"])
emit("RANK_GROUP", {"rank": group.rank(), "size": group.size(), "mlx": mx.__version__})
# MLX's counters from before the load, so the weights arriving are the load's progress (measured
# 2026-09-24: a reporter thread read 21.9 of 31.0 GB active mid `mx.eval(model.parameters())` —
# the eval releases the GIL).
threading.Thread(target=report_memory, daemon=True).start()
if group.rank() != spec["rank"] or group.size() != spec["size"]:
    raise SystemExit(
        f"goose rank wrapper: MLX reports rank {group.rank()} of {group.size()}, "
        f"goose launched rank {spec['rank']} of {spec['size']}"
    )
# Before any collective (rank_formation.py, Q-136): a message the last group's connection swallowed
# is absorbed here, where nothing pairs by it, instead of shifting every collective by one.
if spec.get("formation") is not None:
    form_group(group, spec["formation"])


import faulthandler  # noqa: E402
import select  # noqa: E402
import signal  # noqa: E402
import socket  # noqa: E402
from queue import Empty as QueueEmpty  # noqa: E402

# goosed stops a rank with SIGTERM (a hang, a stop, a memory death). The rank first writes every
# thread's Python stack to stderr — its durable log — then dies of the signal as before (`chain`:
# the default action runs after the dump, so the exit status still names SIGTERM). Q-114's rank 1
# sat at 0% CPU for 20 s before its SIGTERM and nothing said which line it was blocked on.
faulthandler.register(signal.SIGTERM, all_threads=True, chain=True)


def rank0_host():
    """Rank 0's address as the launch told every rank: the JACCL coordinator, or ring host 0."""
    coordinator = os.environ.get("MLX_JACCL_COORDINATOR")
    if coordinator:
        return coordinator.rsplit(":", 1)[0]
    with open(os.environ["MLX_HOSTFILE"]) as hostfile:
        return json.load(hostfile)[0][0].rsplit(":", 1)[0]


class Doorbell:
    """One byte from rank 0 to every worker before each collective an idle loop would otherwise
    spin in. Rank 0 listens on an ephemeral port of its launch address; the port reaches the
    workers through ONE all_sum right after the group forms (every rank runs this at the same
    point). A closed socket (rank 0 gone) ends the worker."""

    def __init__(self):
        self.peers = []
        self.link = None
        host = rank0_host()
        server = socket.create_server((host, 0)) if group.rank() == 0 else None
        port = server.getsockname()[1] if server is not None else 0
        port = int(mx.distributed.all_sum(mx.array([port], dtype=mx.int32)).item())
        if server is not None:
            for _ in range(group.size() - 1):
                peer, _ = server.accept()
                peer.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                self.peers.append(peer)
            server.close()
        else:
            self.link = socket.create_connection((host, port))
            self.link.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        emit("RANK_DOORBELL", {"rank": group.rank(), "port": port})

    def ring(self):
        for peer in self.peers:
            peer.sendall(b"\x01")

    def wait(self):
        if self.link.recv(1) != b"\x01":
            emit("RANK_DOORBELL_CLOSED", {"rank": group.rank()})
            raise SystemExit("goose rank wrapper: rank 0 closed the doorbell")


# Only a launch that asked for it (the spec's `doorbell`): a peer running an older wrapper never
# joins the port all_sum, so the requester refuses that pairing before it forms (launch.rs).
doorbell = Doorbell() if spec.get("doorbell") else None

import copy  # noqa: E402
import traceback  # noqa: E402

# sysexits EX_SOFTWARE: the generation thread died (its RANK_FATAL line names why).
RANK_FATAL_EXIT = 70

import mlx_lm  # noqa: E402
import importlib  # noqa: E402
# NOT `import mlx_lm.generate as ...`: mlx_lm/__init__.py re-exports the `generate` FUNCTION under the
# same name, and `import a.b as c` binds getattr(a, "b") — the function, not the module (3.0.44's
# split died at startup on it: 'function' object has no attribute 'PromptProcessingBatch').
mlx_generate = importlib.import_module("mlx_lm.generate")  # noqa: E402
import mlx_lm.server as server  # noqa: E402
from mlx_lm.tool_parsers import qwen3_coder  # noqa: E402
from mlx_lm.models.cache import PromptTrie, PromptTrieResult  # noqa: E402
import uuid  # noqa: E402
from collections import deque  # noqa: E402

for owner, name in (
    (server, "run"),
    (server.ResponseGenerator, "_next_request"),
    (server.APIHandler, "do_GET"),
    (server.APIHandler, "do_POST"),
    (server.APIHandler, "validate_model_parameters"),
    (server.APIHandler, "_set_completion_headers"),
    (server.APIHandler, "handle_completion"),
    (server.APIHandler, "handle_chat_completions"),
    (server.APIHandler, "generate_response"),
    (server, "ToolCallFormatter"),
    (server, "process_message_content"),
    (qwen3_coder, "parse_tool_call"),
    (qwen3_coder, "_parse_xml_function_call"),
    (qwen3_coder, "_convert_param_value"),
    (qwen3_coder, "_get_arguments_config"),
    (server.ModelProvider, "load"),
    (server.ResponseGenerator, "generate"),
    (server.ResponseGenerator, "_tokenize"),
    (server.ResponseGenerator, "_share_request"),
    (server.ResponseGenerator, "_generate"),
    (server, "LRUPromptCache"),
    (server.LRUPromptCache, "insert_cache"),
    (server.LRUPromptCache, "fetch_nearest_cache"),
    (PromptTrie, "search"),
    (server.LRUPromptCache, "trim_to"),
    (server.LRUPromptCache, "CacheOrder"),
    (server, "BatchGenerator"),
    (server.BatchGenerator, "close"),
    (server.BatchGenerator, "remove"),
    (server.BatchGenerator, "insert_segments"),
    (server, "GenerationContext"),
    (server.GenerationContext, "stop"),
    (server, "TimeBudget"),
    (server.BatchGenerator, "prompt_cache_nbytes"),
    (ArraysCache, "advance"),
):
    if not hasattr(owner, name):
        raise SystemExit(
            f"goose rank wrapper: mlx_lm {mlx_lm.__version__} has no {getattr(owner, '__name__', owner)}.{name}; "
            "the wrapper was written against mlx_lm 0.31.3"
        )

# Q-232 (rank_tool_schema.py): before any parse or streamer reads a tool's parameters.
qwen3_coder._get_arguments_config = typed_arguments_config(qwen3_coder._get_arguments_config)
# Q-372 (rank_tool_stream.py): a value runs to its last `</parameter>` before the next declared header.
install_positional_parameters(qwen3_coder)

prefill = spec.get("prefill")
if prefill is not None:
    for owner, name in (
        (mlx_generate.PromptProcessingBatch, "prompt"),
        (mlx_generate.PromptProcessingBatch, "split"),
        (mlx_generate.PromptProcessingBatch, "filter"),
        (server.BatchGenerator, "next"),
        (BatchKVCache, "step"),
    ):
        if not hasattr(owner, name):
            raise SystemExit(
                f"goose rank wrapper: mlx_lm {mlx_lm.__version__} has no {owner.__name__}.{name}; "
                "the prefill plan was written against mlx_lm 0.31.3"
            )
    if "prompt_cache_limit_bytes" not in spec:
        raise SystemExit(
            "goose rank wrapper: the spec carries a prefill plan but no prompt_cache_limit_bytes "
            "(the KV charge its admission is measured against)"
        )

served = spec["served_id"]
# Every other name of the served model (goose's one identity, model_identity.rs ServedNames), rank
# 0's only; an older requester's spec carries none (Q-131).
served_aliases = [name for name in spec.get("served_aliases", []) if name != served]
served_names = [served, *served_aliases]
state = {
    "steps": 0,
    "inflight": 0,
    # Handlers inside do_POST whose request has no row in `live` yet (reading, validating): part of
    # /v1/status's num_running (Q-403). `posting` marks the handler thread until its row exists.
    # A request refused by the memory hold (below) never becomes one: it is answered 503 before.
    "before_row": 0,
}
posting = threading.local()
# rank_admission.py (Q-397): what the memory watchdog holds and lifts, and the 503 that names it.
admission = Admission()
# rank_sampling.py (Q-159): rank 0's layers under each request's own sampling fields. Only rank 0
# resolves — the workers sample from the arguments it shares.
sampling_defaults = None
if group.rank() == 0:
    sampling_defaults = SamplingDefaults(spec.get("sampling_defaults"), spec["model_dir"])
    emit("RANK_SAMPLING_DEFAULTS", sampling_defaults.report())
    if sampling_defaults.error is not None:
        emit(
            "RANK_GENERATION_CONFIG_UNREAD",
            {"error": sampling_defaults.error, "in_force": SINGLE_ENGINE_FALLBACK},
        )
# Where this rank's generation loop is (rank_state.py), published at each step for the reporter.
loop = LoopState(group.rank())
lock = threading.Lock()
# Rank 0's live request table (rank_live.py): the instants each request was measured at, keyed by
# a per-process counter. `generate` returns once the request is tokenized; mlx_lm then delivers
# the prompt progress (processed, total) and the tokens through the iterator the handler drains.
live = {}
live_ids = iter(range(1, 1 << 62))
# The same requests' stream watches (rank_stream_watch.py, Q-146): None for a request that is not
# a streamed chat answer — /v1/status's `stream: null`.
watches = {}
# The same requests' `sampling` rows (rank_sampling.py `sampling_row`).
samplings = {}
# The same requests' handling (Q-231, rank_live.py `row_handling`): the client that sent each, the
# engine's named stop of its answer, and whether its handler and its batch row have left. A
# request whose answer ended while its row still holds the batch stays listed until the row leaves.
# Rank 0 also keeps what a departure needs to reach the request wherever it is (Q-403,
# `client_departure`): the client's connection, the answer queue its handler waits on, its
# generation context once the loop has handed it back, and how its client left (None while there).
handling = {}
# Which request each row of the batch serves (uid → request id), kept by the generation thread of
# every rank (the id rides the shared request, `goose_request_id`), and the request
# `_next_request` handed the loop last: mlx_lm inserts it before it takes another. `in_batch` is the
# same requests' ids, read under `lock` by the handlers: a request is kept listed after its answer
# ended only while its id is here — never on an absence of word from the loop.
batch_rows = {}
in_batch = set()
arriving = [None]
loop.requests = batch_rows
# Rank 0's requests held for room (`rank0_request`), by id, as a tuple replaced whole each time.
held_ids = [()]
# Whether the batch's last step read prompt tokens (`TrackedBatchGenerator.next`): the same on every
# rank, since every rank steps the same batch (`PromptStepBudget` ends the step loop on it).
last_step = {"read_prompt": False}


def row_left(uid, how):
    """The generation thread, as a row leaves the batch (it finished, or the loop removed it). A
    request whose handler has already left is unlisted now — it held the batch until here — and
    the rank's log says how long it held it after its answer ended (RANK_ROW_LEFT)."""
    request_id = batch_rows.pop(uid, None)
    now = time.monotonic()
    with lock:
        in_batch.discard(request_id)
        handled = handling.get(request_id)
        if handled is None or not handled["handler_left"]:
            return
        live.pop(request_id, None)
        watches.pop(request_id, None)
        samplings.pop(request_id, None)
        handling.pop(request_id, None)
    stopped = handled["stopped"]
    emit(
        "RANK_ROW_LEFT",
        {
            "request_id": request_id,
            "uid": uid,
            "how": how,
            "stopped": None if stopped is None else stopped["reason"],
            "held_after_stop_s": None
            if handled["stopped_at"] is None
            else round(now - handled["stopped_at"], 3),
            "held_after_answer_s": round(now - handled["handler_left_at"], 3),
        },
    )


def apply_caps():
    info = mx.device_info()
    ram = int(info["memory_size"])
    ceiling = int(info["max_recommended_working_set_size"])
    memory_limit = ceiling
    wired_limit = ceiling
    # MLX's free-buffer cache holds the plan's transient allowance and no more
    # (RankPlan::mlx_cache_limit_bytes, planned × (RUNTIME_OVERHEAD_RATIO − 1)). An older
    # requester's spec carries no figure: its own rule, the ceiling less the planned bytes.
    if "mlx_cache_limit_bytes" in spec:
        cache_limit = int(spec["mlx_cache_limit_bytes"])
    else:
        cache_limit = max(0, memory_limit - int(spec["planned_bytes"]))
    mx.set_memory_limit(memory_limit)
    mx.set_wired_limit(wired_limit)
    mx.set_cache_limit(cache_limit)
    emit(
        "RANK_CAPS",
        {
            "ram": ram,
            "memory_limit": memory_limit,
            "wired_limit": wired_limit,
            "cache_limit": cache_limit,
            "planned": int(spec["planned_bytes"]),
        },
    )


original_run = server.run


def run(host, port, model_provider, *args, **kwargs):
    # The goose id resolves to THIS rank's own --model path on every rank (paths differ per node),
    # so a request naming it loads nothing new and the response echoes the id that was asked for.
    model_provider._model_map[served] = model_provider.cli_args.model
    apply_caps()
    return original_run(host, port, model_provider, *args, **kwargs)


server.run = run

# The generation thread loads the model (server.py `_generate` → `load_default`) after rank 0's
# HTTP server is already up, so a request can arrive before the tokenizer exists. mlx_lm queues
# such a request until the load is done; the thinking resolution below needs the template first,
# so its handler waits for the same moment. A load that fails ends the rank (RANK_FATAL), and the
# waiting handler with it.
model_loaded = threading.Event()
original_load = server.ModelProvider.load


def load(self, *args, **kwargs):
    loaded = original_load(self, *args, **kwargs)
    if xml_skeleton_guard and skeleton["spec"] is None:
        arm_skeleton_guard(self.tokenizer)
    model_loaded.set()
    return loaded


server.ModelProvider.load = load

def owned(value):
    """`value` with every array replaced by one that owns exactly its own bytes."""
    if isinstance(value, mx.array):
        return mx.contiguous(value)
    if isinstance(value, (list, tuple)):
        return type(value)(owned(item) for item in value)
    return value


def arrays_in(value):
    if isinstance(value, mx.array):
        yield value
    elif isinstance(value, (list, tuple)):
        for item in value:
            yield from arrays_in(item)


def compact(prompt_cache):
    """Give a prompt-cache entry its own buffers before the cache keeps it. mlx_lm 0.31.3 hands
    the cache an UNEVALUATED `contiguous(batch keys[i, :, pad:idx])` (BatchKVCache.extract) and a
    VIEW `state[i:i+1]` (ArraysCache.extract): both keep the whole batch buffer alive — every row,
    padded to the longest — while `nbytes` counts one row's tokens. Measured (Q-79): a 5-row
    buffer held 40 MiB for an entry counted at 0–8 MiB, and E2E #2's ranks grew 43 → 77 GB while
    the cache reported 16 GB. Contiguous + eval copies exactly the entry and drops the batch
    buffer; `nbytes` is then the truth the byte bound is enforced on. Eviction is unchanged
    (nbytes was the logical size before and is the same size after), so a peer rank whose
    wrapper does not compact still evicts alike."""
    layers = [layer for layer in prompt_cache if not layer.empty()]
    for layer in layers:
        layer.state = owned(layer.state)
    mx.eval([array for layer in layers for array in arrays_in(layer.state)])


# The BatchGenerator mlx_lm's generation loop is serving (it makes at most one at a time): its
# live KV is what the prompt cache shares the plan's KV charge with between admissions.
live_batch = []
# The bounded prompt cache mlx_lm's `run` built (one per process).
prompt_caches = []

# Q-114, measured: mlx_lm 0.31.3's `ArraysCache.advance` decrements a batch cache's
# `left_padding` (every batch cache carries one — `_make_cache` sets it even for one row) and
# `lengths` LAZILY, and each `-= N` materialises N as a new constant: one Metal buffer. The
# qwen3_5 27B has 48 linear-attention layers and only the first one's counter is ever read
# (`create_ssm_mask(..., cache[ssm_idx])`), so the other 47 grow an unevaluated chain that pins
# one buffer per decode step each — until MLX's `resource_limit` (499,000) refuses the next
# allocation: `[metal::malloc] Resource limit (499000) exceeded` from inside a step's
# async_eval, at ~10.5k generated tokens (E2E #3b 10,447; repro A1 10,522, B2 10,537). MLX
# 0.32.2's eval_impl then deadlocks in its own error path (it synchronizes the CPU stream behind
# a fence wait whose GPU signal the aborted step never committed), swallowing the error: the
# rank stops in `async_eval`, the peer spins in the step's all_sum, and the peer's GPU times out.
# Every counter a step advanced is evaluated with the step (one async_eval of the step's scalars),
# so each chain is one node long and holds nothing. Values and collectives are unchanged.
upstream_advance = ArraysCache.advance
advanced_counters = []


def advance(self, N):
    upstream_advance(self, N)
    advanced_counters.extend(c for c in (self.left_padding, self.lengths) if c is not None)


def settle_counters():
    if advanced_counters:
        mx.async_eval(advanced_counters)
        advanced_counters.clear()


ArraysCache.advance = advance


class TrackedBatchGenerator(server.BatchGenerator):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        if not isinstance(getattr(self, "_prompt_tokens_counter", None), int):
            raise SystemExit(
                f"goose rank wrapper: mlx_lm {mlx_lm.__version__}'s BatchGenerator counts no "
                "`_prompt_tokens_counter`; a step's prompt reading was written against mlx_lm 0.31.3"
            )
        live_batch[:] = [self]
        loop.new_batch()

    def insert_segments(self, *args, **kwargs):
        uids = super().insert_segments(*args, **kwargs)
        with lock:
            for uid in uids:
                batch_rows[uid] = arriving[0]
                in_batch.add(arriving[0])
        return uids

    def remove(self, uids, *args, **kwargs):
        loop.drop(uids)
        caches = super().remove(uids, *args, **kwargs)
        for uid in uids:
            row_left(uid, "removed")
        return caches

    def close(self):
        if live_batch and live_batch[0] is self:
            live_batch.clear()
        super().close()

    @property
    def prompt_cache_nbytes(self):
        # What mlx_lm's admission trim (server.py:795-798) and the live bound hold the prompt cache
        # beside: with a prefill plan, the batch's projected padded KV (rank_prefill.py) when it
        # exceeds what its caches hold now — the rows still to be read grow into it.
        held = super().prompt_cache_nbytes
        if prefill is None:
            return held
        return max(held, batch_kv_charge(prefill, *batch_shape(self)))

    def next(self):
        read_before = self._prompt_tokens_counter
        responses = super().next()
        last_step["read_prompt"] = self._prompt_tokens_counter > read_before
        settle_counters()
        for response in responses[1]:
            loop.fold(response.uid, response.token, response.finish_reason)
            if response.finish_reason is not None:
                row_left(response.uid, response.finish_reason)
        # Generation grows every row one token a step, past what the last admission charged: the
        # cache yields before the batch outgrows the plan's KV charge. Every rank holds the same
        # batch and the same cache, so every rank evicts alike.
        if prefill is not None and prompt_caches:
            room = prompt_cache_limit - self.prompt_cache_nbytes
            if prompt_caches[0].nbytes > room:
                prompt_caches[0].trim_to(n_bytes=room)
        return responses


server.BatchGenerator = TrackedBatchGenerator


# Q-231: a row whose client has left leaves at the loop's next removal, and a request that arrives
# while a prompt is read is taken at the next prompt step — not after mlx_lm's whole step loop.
#
# mlx_lm 0.31.3's `_generate` runs `batch_generator.next()` in a loop bounded by its TimeBudget, and
# only after the loop does it remove the rows whose context says stop and take the next request. On
# a distributed group the budget is a COUNT of steps, the same on every rank, re-fitted every ten
# loops to 0.5 s of the steps it measured — decode steps (~0.1 s on the 27B split): about five. A
# step that reads a prompt slice (up to `--prefill-step-size` tokens per row) costs seconds, so five
# of them held everything else out. E2E #3m turn 3 (2026-09-27): goose's end-of-turn fact checker
# sent three requests (1,775 / 1,770 / 5,453 tokens, POST 20:16:14Z); goose dropped all three at
# 20:16:17.067Z for the user's turn (turn_priority), rank 0 named the departures at 20:16:17.892Z
# (GOOSE_RANK_CANCELLED_BY_CLIENT, 380 tokens read), and the loop then read the three prompts to
# their end — `steps` 1116 on both ranks from 20:16:14Z to 20:16:59Z, the cache +3 user segments —
# before it removed them; the user's 88,660-token call (sent 20:16:20.9Z) waited 38.9 s. Two changes:
# - `PromptStepBudget`: the step loop ends after a step that read prompt tokens, so its removals
#   and the next request are handled between prompt slices. It changes which step each rank runs
#   when (every rank must end the loop at the same step, or the collectives no longer pair), so
#   only a launch whose every rank runs it asks for it (`prefill_step_yields`). A step that only
#   decodes runs under mlx_lm's budget unchanged, and a loop ended here is not counted in the
#   budget's fit (every rank skips it alike), so decode keeps its measured step count.
# - `DepartureContext`: rank 0's generation loop reads a context's stop flag right after it hands
#   the request a piece; the flag now also reads the client's socket (`client_left`), so a client
#   that has already gone is removed at that loop's end instead of the handler's next reading, and
#   the handler — which may already be waiting for a piece that will never come — is woken on its
#   answer queue to name the stop. Rank 0 decides and shares the removal (server.py
#   `_share_object`), so it needs no tag.
class PromptStepBudget(server.TimeBudget):
    def __iter__(self):
        last_step["read_prompt"] = False
        return super().__iter__()

    def __next__(self):
        if last_step["read_prompt"]:
            raise StopIteration()
        return super().__next__()


if spec.get("prefill_step_yields"):
    server.TimeBudget = PromptStepBudget


class DepartureContext(server.GenerationContext):
    """mlx_lm's GenerationContext whose stop flag is also true once the client the handler set
    (`client`) has left. Every other rank's context has no client: its removals are rank 0's."""

    client = None
    answers = None

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        # The request the loop builds this context for (`_next_request`'s `arriving`, the id
        # `insert_segments` names the row by).
        self.__dict__["request_id"] = arriving[0]

    @property
    def _should_stop(self):
        if self.__dict__.get("stopped") or self.__dict__.get("departed"):
            return True
        request_id = self.__dict__.get("request_id")
        if request_id is not None and group.rank() == 0:
            # Q-403: a departure named before this context reached its handler — the handler may
            # already have ended (woken while it waited), and then nobody would ever stop the row.
            with lock:
                handled = handling.get(request_id)
                orphaned = request_id in in_batch and (
                    handled is None or handled["departed"] is not None
                )
            if orphaned:
                self.__dict__["departed"] = "handler_ended"
                return True
        client = self.client
        if client is None:
            return False
        try:
            how = client_left(client)
        except (ValueError, OSError):
            # The handler closed the socket after the answer ended: the handler itself ended the
            # request (and, if the client left, named it), so no departure is left to read here.
            return False
        if how is None:
            return False
        # The row leaves at this loop's end and nothing more reaches its answer queue: a handler
        # already waiting on it is woken to name the stop (`ClientDeparted`).
        self.__dict__["departed"] = how
        self.answers.put(ClientDeparted(how))
        return True

    @_should_stop.setter
    def _should_stop(self, value):
        self.__dict__["stopped"] = value


server.GenerationContext = DepartureContext

if prefill is not None:
    upstream_prompt = mlx_generate.PromptProcessingBatch.prompt
    upstream_split = mlx_generate.PromptProcessingBatch.split
    overruns = {"reported": False}

    def prompt(self, tokens):
        # One step of the prompt batch: `tokens` holds each row's next slice (at most mlx_lm's
        # step). Its chunk is what the plan's workspace affords this many rows at the width the
        # slice reaches; every rank computes it from the same batch, so the collectives pair.
        if tokens:
            rows = len(tokens)
            width = cache_width(self.prompt_cache) + max(len(t) for t in tokens)
            chunk = prefill_chunk(prefill, rows, width, BatchKVCache.step)
            over = chunk_overruns(prefill, rows, width, chunk)
            if over and not overruns["reported"]:
                overruns["reported"] = True
                emit(
                    "RANK_PREFILL_OVER",
                    {"rows": rows, "width": width, "chunk": chunk, "over_bytes": over},
                )
            self.prefill_step_size = chunk
        return upstream_prompt(self, tokens)

    def split(self, indices):
        return moving_split(self, indices, upstream_split)

    mlx_generate.PromptProcessingBatch.prompt = prompt
    mlx_generate.PromptProcessingBatch.split = split

from mlx_lm.models.cache import can_trim_prompt_cache, trim_prompt_cache  # noqa: E402


class LinearPromptTrie(PromptTrie):
    """mlx_lm's trie with its search in linear time (rank_prompt_search.py, Q-162). The search is
    the rank's own CPU work between the request's share and the batch step — no step, no GPU, no
    collective — so it is published as its own place in the loop (`cache_lookup`), which goosed's
    hang rule reads as progress while the rank's CPU time advances."""

    def search(self, model, tokens):
        at = loop.at
        loop.publish(CACHE_LOOKUP)
        try:
            return PromptTrieResult(*nearest_prompt(self._trie, model, tokens))
        finally:
            loop.publish(at)


class LookupPromptCache(server.LRUPromptCache):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        if not isinstance(getattr(self, "_trie", None), PromptTrie) or self._trie._trie:
            raise SystemExit(
                f"goose rank wrapper: mlx_lm {mlx_lm.__version__}'s LRUPromptCache keeps no empty "
                "PromptTrie at `_trie`; the prompt search was written against mlx_lm 0.31.3"
            )
        if kept_entries and not isinstance(getattr(getattr(self, "_lru", None), "_lrus", None), dict):
            raise SystemExit(
                f"goose rank wrapper: mlx_lm {mlx_lm.__version__}'s LRUPromptCache keeps no "
                "CacheOrder at `_lru`; the kept entries were written against mlx_lm 0.31.3"
            )
        self._trie = LinearPromptTrie()

    # Q-164: the prompt's last token is always read, so a generation always has a segment.
    #
    # mlx_lm 0.31.3's `_generate` trims the prompt's segments by what the nearest cache entry holds;
    # when an entry's key IS the whole prompt (an identical request after one answered in a single
    # token, or a prompt ending exactly at a segment snapshot), every segment is consumed and
    # `BatchGenerator.insert_segments` reads `seq[-1]` of an empty list — IndexError in the
    # generation thread of every rank (each holds the same cache), RANK_FATAL, the split gone
    # (2026-09-27 08:23:23 and 09:01:06 on 8091). Such a lookup now leaves the last token to read:
    # a trimmable entry (plain KV) is copied and trimmed by one token; a hybrid entry (the qwen3_5
    # linear-attention state cannot be rolled back) is passed over for the nearest entry of the
    # prompt minus its last token. Any other lookup is upstream's, unchanged. Every rank runs this
    # on its own cache over the same shared requests — the same decision, the same segments —
    # exactly as it runs mlx_lm's own lookup.
    def fetch_nearest_cache(self, model, tokens):
        try:
            entry = self._trie.get(model, tokens) if tokens else None
        except KeyError:
            entry = None
        if entry is None:
            return super().fetch_nearest_cache(model, tokens)
        if can_trim_prompt_cache(entry.prompt_cache):
            cache = copy.deepcopy(entry.prompt_cache)
            trim_prompt_cache(cache, 1)
            return cache, tokens[-1:]
        cache, rest = super().fetch_nearest_cache(model, tokens[:-1])
        return cache, [*rest, tokens[-1]]


server.LRUPromptCache = LookupPromptCache

# Q-182 (rank_boundary.py `pop_keeping_newest_prefix`): the prompt cache evicts the stable prefix
# the latest conversation request left only when nothing else is left to evict. It changes what is
# evicted — every rank runs its own cache over the same requests and must reuse the same prefix —
# so only a launch whose every rank runs it asks for it (`keep_newest_prefix`).
#
# Q-294 (rank_boundary.py `KeptEntry`): the entry kept is the stable prefix a request that
# names its transient tail cut — goose's agent requests — not whichever request inserted the newest
# "user" entry (an end-of-turn helper's took the protection on E2E #3o), and rank 0 admits a request
# into a live batch only while the batch leaves room for it (`room_for`). Tracked on every rank over
# the same requests, it changes what every rank evicts, so it rides its own ask
# (`keep_conversation_prefix`), which supersedes `keep_newest_prefix`.
#
# Q-347 (rank_boundary.py `cut_at_head`): the chat's stable head — the system prompt and tools every
# request of the chat opens with — is cut by every agent request and kept after the conversation
# prefix, and a batch leaves room for it beside the prefix. It changes where prefill chunks end and
# what is evicted, so it rides its own ask (`keep_stable_head`).
conversation_prefix = KeptEntry() if spec.get("keep_conversation_prefix") else None
stable_head = KeptEntry() if spec.get("keep_stable_head") else None
kept_entries = tuple(entry for entry in (conversation_prefix, stable_head) if entry is not None)
if kept_entries or spec.get("keep_newest_prefix"):
    if not hasattr(server.LRUPromptCache.CacheOrder, "pop"):
        raise SystemExit(
            f"goose rank wrapper: mlx_lm {mlx_lm.__version__}'s LRUPromptCache.CacheOrder has no "
            "pop; the kept prefix was written against mlx_lm 0.31.3"
        )
if kept_entries:
    keep_entries(server.LRUPromptCache.CacheOrder, *kept_entries)
elif spec.get("keep_newest_prefix"):
    keep_newest_prefix(server.LRUPromptCache.CacheOrder)


def cache_inserting(tokens, prompt_cache):
    """What the prompt cache is about to hold, as the kept entries see it (Q-294, Q-347)."""
    nbytes = sum(layer.nbytes for layer in prompt_cache)
    for entry in kept_entries:
        entry.inserted(tokens, nbytes)


def cache_inserted(prompt_cache):
    """A kept entry the insert replaced, or dropped as a trimmable entry's prefix, is no longer
    held (mlx_lm's `insert_cache` drops those without its eviction order)."""
    if kept_entries:
        forget_dropped(prompt_cache._lru, kept_entries)


if "prompt_cache_limit_bytes" in spec:
    prompt_cache_limit = int(spec["prompt_cache_limit_bytes"])
    # A spec that asks for it (`prompt_cache_live_bound`, tag mlxLmServerBounded) bounds cached
    # + live at EVERY insert, not only at admission (mlx_lm's own trim, server.py:795-798): the
    # plan charges the KV once — state + prompt cache — and a cache refilled to the whole charge
    # while a batch still holds its live KV overran it by that batch (E2E #2: 16–17.3 GB cached
    # beside 1–7 GB live). It changes what is evicted, so only a launch whose every rank runs it
    # asks for it (an older peer's goosed cannot read the tag and refuses the rank).
    live_bound = bool(spec.get("prompt_cache_live_bound"))
    prompt_cache_flags = [
        "--prompt-cache-size",
        str(int(spec["prompt_cache_entries"])),
        "--prompt-cache-bytes",
        str(prompt_cache_limit),
    ]

    class BoundedPromptCache(LookupPromptCache):
        def __init__(self, max_size):
            super().__init__(max_size, prompt_cache_limit)
            prompt_caches[:] = [self]

        def insert_cache(self, model, tokens, prompt_cache, *, cache_type="assistant"):
            compact(prompt_cache)
            cache_inserting(tokens, prompt_cache)
            super().insert_cache(model, tokens, prompt_cache, cache_type=cache_type)
            cache_inserted(self)
            if live_bound and live_batch:
                self.trim_to(n_bytes=prompt_cache_limit - live_batch[0].prompt_cache_nbytes)

    server.LRUPromptCache = BoundedPromptCache
else:
    prompt_cache_flags = ["--prompt-cache-bytes", str(int(spec["prompt_cache_bytes"]))]

    class CompactPromptCache(LookupPromptCache):
        def insert_cache(self, model, tokens, prompt_cache, *, cache_type="assistant"):
            compact(prompt_cache)
            cache_inserting(tokens, prompt_cache)
            super().insert_cache(model, tokens, prompt_cache, cache_type=cache_type)
            cache_inserted(self)

    server.LRUPromptCache = CompactPromptCache

original_generate_loop = server.ResponseGenerator._generate


def _generate(self):
    # mlx_lm's generation thread: an exception ends the thread, and a worker rank's main thread
    # only joins it (server.py `run`: `response_generator.join()`), so the process then exits 0
    # with the traceback as its last words — E2E #2's Studio rank died of a Metal OOM and was
    # reported "exit status: 0". The death is named and the exit is not a success; on rank 0 the
    # HTTP server would otherwise keep accepting requests nothing will ever answer.
    try:
        original_generate_loop(self)
    except BaseException as death:
        message = f"{type(death).__name__}: {death}"
        traceback.print_exc()
        emit(
            "RANK_FATAL",
            {
                "rank": group.rank(),
                "thread": "generation",
                "error": message,
                "out_of_memory": "Insufficient Memory" in message
                or "OutOfMemory" in message,
            },
        )
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(RANK_FATAL_EXIT)


server.ResponseGenerator._generate = _generate

original_next = server.ResponseGenerator._next_request


# Rank 0's requests waiting for room in the batch (settled: tokenized, budgeted), oldest first.
held = deque()


def conversation_prefix_bytes():
    return conversation_prefix.nbytes if conversation_prefix is not None else 0


def stable_head_bytes():
    return stable_head.nbytes if stable_head is not None else 0


def room_for(prompt_tokens):
    batch = live_batch[0] if live_batch else None
    rows, width = batch_shape(batch) if batch is not None else (0, 0)
    return admits(
        prefill,
        prompt_cache_limit,
        rows,
        width,
        prompt_tokens,
        conversation_prefix_bytes(),
        stable_head_bytes(),
    )


def publish_held():
    held_ids[0] = tuple(getattr(request[1], "goose_request_id", None) for request, _ in held)


def drop_departed_held():
    """Rank 0 (Q-403): every held request nobody waits for any more leaves the hold unshared. E2E
    #3r's req-580 was released into the batch 40 s after goose dropped it, read a 2,048-token chunk,
    and held the chat's own call behind it for 6.5 s."""
    kept, dropped = [], []
    for item in held:
        if still_wanted(item[0]):
            kept.append(item)
        else:
            dropped.append(getattr(item[0][1], "goose_request_id", None))
    if not dropped:
        return
    held.clear()
    held.extend(kept)
    publish_held()
    emit("RANK_ADMISSION", {"dropped_departed": dropped, "still_held": len(held)})


def rank0_request(self, timeout):
    """The request rank 0 shares now, or None: the oldest held one once the batch has room for
    it, else the next arrival — held instead when the batch it would join cannot fit it (behind
    any request already held, so arrivals keep their order). A request whose client has left is
    never shared (Q-403): the hold drops it, and an arrival answered on its own queue is followed
    by the next arrival at once, so the request behind it is not left for another step."""
    drop_departed_held()
    if held and room_for(held[0][1]):
        request, tokens = held.popleft()
        publish_held()
        emit("RANK_ADMISSION", {"released_tokens": tokens, "still_held": len(held)})
        return request
    while True:
        try:
            if timeout is None or held:
                request = self.requests.get_nowait()
            else:
                request = self.requests.get(timeout=timeout)
        except QueueEmpty:
            return None
        request, tokens = settle(self, request)
        if request is not None:
            break
        timeout = None
    if not held and room_for(tokens):
        return request
    held.append((request, tokens))
    publish_held()
    rows, width = batch_shape(live_batch[0]) if live_batch else (0, 0)
    emit(
        "RANK_ADMISSION",
        {
            "held_tokens": tokens,
            "held": len(held),
            "rows": rows,
            "width": width,
            "charge": batch_kv_charge(prefill, rows + 1, max(width, tokens)),
            "kept_prefix": kept_prefix_bytes(prefill, max(width, tokens)),
            "conversation_prefix": conversation_prefix_bytes(),
            "stable_head": stable_head_bytes(),
            "limit": prompt_cache_limit,
        },
    )
    return None


def _next_request(self, timeout=None):
    # `timeout` is None exactly while a batch runs (mlx_lm's `_generate`), and every rank's loop
    # state is the same, so every rank takes the same branch here. With a prefill plan rank 0
    # decides which request is shared (rank0_request); the workers receive exactly what it shares.
    # Each wait is published before it is entered (rank_state.py), so a rank that stops inside one
    # has already said which.
    loop.mode = "busy" if timeout is None else "idle"
    loop.rows, loop.width = batch_shape(live_batch[0]) if live_batch else (0, 0)
    loop.held = len(held)
    if prefill is not None and group.rank() == 0:
        loop.publish("poll")
        request = rank0_request(self, timeout)
        if doorbell is None or timeout is None:
            loop.publish("share")
            request = original_share_request(self, request)
        elif request is not None:
            doorbell.ring()
            loop.rings += 1
            loop.publish("share")
            request = original_share_request(self, request)
    elif doorbell is None or timeout is None:
        loop.publish("share")
        request = original_next(self, timeout)
    elif group.rank() == 0:
        loop.publish("poll")
        try:
            request = self.requests.get(timeout=timeout)
        except QueueEmpty:
            request = None
        if request is not None:
            doorbell.ring()
            loop.rings += 1
            loop.publish("share")
            request = self._share_request(request)
    else:
        loop.publish("doorbell")
        doorbell.wait()
        loop.rings += 1
        loop.publish("share")
        request = self._share_request(None)
    state["steps"] += 1
    loop.steps = state["steps"]
    loop.publish("batch" if timeout is None or request is not None else "idle")
    mark_tool_request(request)
    if request is not None:
        arriving[0] = getattr(request[1], "goose_request_id", None)
    return request


server.ResponseGenerator._next_request = _next_request

original_generate = server.ResponseGenerator.generate


class ClientGone(BaseException):
    """The client closed the connection while its answer was being generated (Q-181). A
    BaseException, so mlx_lm's handler (`except Exception`) lets it through to do_POST."""


class ClientDeparted(Exception):
    """Put on a request's answer queue by rank 0's generation loop when it found the client gone
    (`DepartureContext`, Q-231): the loop removes the row, so no further piece will come, and
    mlx_lm's `_inner` raises this to the handler waiting for one, which names the stop."""

    def __init__(self, how):
        super().__init__(how)
        self.how = how


class RequestTap:
    """The response generator as mlx_lm's own `generate` uses it (`self.requests.put` only),
    keeping the answer queue it creates: the handler hands it to its context, so the generation
    loop can wake a handler whose client it found gone (Q-231)."""

    def __init__(self, generator, handled):
        self.generator = generator
        self.handled = handled
        self.answers = None

    @property
    def requests(self):
        return self

    def put(self, item):
        self.answers = item[0]
        # Before the request is queued: from here a departure can wake the handler (Q-403).
        with lock:
            self.handled["answers"] = item[0]
        self.generator.requests.put(item)


def client_left(connection):
    """How the client left — "eof", or the reset the socket reports — else None. A readiness poll
    that does not wait, then a peek that consumes nothing: a client still reading has sent nothing
    after its request, and a pipelined next request is data, not an end."""
    poller = select.poll()
    poller.register(connection, select.POLLIN)
    if not poller.poll(0):
        return None
    try:
        peeked = connection.recv(1, socket.MSG_PEEK)
    except OSError as reset:
        return f"{type(reset).__name__}: {reset}"
    return None if peeked else "eof"


def request_phase(entry):
    """Where a live request is, as its /v1/status row says it (rank_live.py `live_request`)."""
    if entry["first_token"] is not None:
        return "generation"
    return "queued" if entry["prefill_started"] is None else "prefill"


def client_departure(request_id, how, phase):
    """Rank 0 (Q-403): the client of `request_id` has left, seen by whoever read its socket first
    — its handler at a piece, the generation loop at a step (through the handler it wakes),
    /v1/status, or the admission of a request not yet given batch work. Named once
    (GOOSE_RANK_CANCELLED_BY_CLIENT, `last_engine_stop`); from then on its row is not counted as
    running, its generation context is told to stop (the loop removes the row at its step's end),
    and its handler, wherever it waits — for the context or for a piece — is woken to end."""
    with lock:
        handled = handling.get(request_id)
        if handled is None or handled["departed"] is not None:
            return
        handled["departed"] = how
        entry = live[request_id]
        watch = watches.get(request_id)
        ctx, answers = handled["ctx"], handled["answers"]
    stop = {
        "request_id": request_id,
        "reason": "cancelled_by_client",
        "how": how,
        "phase": phase,
        "prompt_tokens": entry["prompt_tokens"],
        "prefilled": entry["prefilled"],
        "completion_tokens": entry["completion"],
    }
    if watch is not None:
        withholding = watch.withholding()
        stop.update(
            withholding=None if withholding is None else withholding[0],
            generated_chars=watch.generated_chars,
            sent_chars=watch.sent_chars,
            tail=watch.tail,
        )
        watch.stop = stop
    record_stop(stop)
    if ctx is not None:
        ctx.stop()
    if answers is not None:
        answers.put(ClientDeparted(how))


def read_departure(request_id, connection, phase):
    """Rank 0 (Q-403): reads `connection` without waiting and names the departure when its client
    has left. True when the request is gone — just now, or its handler already closed the
    connection (the handler ended the request itself)."""
    try:
        how = client_left(connection)
    except (ValueError, OSError):
        return True
    if how is None:
        return False
    client_departure(request_id, how, phase)
    return True


def still_wanted(request):
    """Rank 0, before a queued or held request is given batch work (Q-403): False once nobody
    waits for its answer — its client has left (named here if nobody saw it first) or its handler
    has already ended. A request that reached the queue without this wrapper's `generate` carries
    no id and no connection to read, so there is nothing to judge it by."""
    request_id = getattr(request[1], "goose_request_id", None)
    if request_id is None:
        return True
    with lock:
        handled = handling.get(request_id)
        if handled is None or handled["departed"] is not None:
            return False
        connection = handled["connection"]
    if connection is None:
        return True
    return not read_departure(request_id, connection, "queued")


def generate(
    self, request, generation_args, progress_callback=None, watch=None, client=None, address=None
):
    # `watch` (rank_stream_watch.py): a streamed chat request's account of what its client has
    # been sent, passed by the Q-141 relay below; None for every other request. `client`: the
    # handler's connection, read for the client's departure at every piece (Q-181); `address`: its
    # peer, listed on the request's /v1/status row (Q-231).
    with lock:
        request_id = f"req-{next(live_ids)}"
        # Rides the shared request to every rank's generation loop (`_next_request`), which names
        # the batch row it becomes (`batch_rows`).
        request.goose_request_id = request_id
        watches[request_id] = watch
        samplings[request_id] = sampling_row(
            generation_args, getattr(request, "sampling_sources", None)
        )
        entry = {
            "arrived": time.monotonic(),
            "max_tokens": generation_args.max_tokens,
            "prompt_tokens": None,
            "cached_tokens": None,
            "prefill_started": None,
            "prefilled": 0,
            "first_token": None,
            "last_token": None,
            "completion": 0,
        }
        live[request_id] = entry
        if getattr(posting, "before_row", False):
            posting.before_row = False
            state["before_row"] -= 1
        handled = {
            "client": None if address is None else f"{address[0]}:{address[1]}",
            "stopped": None,
            "stopped_at": None,
            "handler_left": False,
            "handler_left_at": None,
            "connection": client,
            "answers": None,
            "ctx": None,
            "departed": None,
        }
        handling[request_id] = handled

    # The generation context, once mlx_lm hands it back: every piece arrives after it.
    started = []

    def still_there(phase):
        """Raises ClientGone, the generation told to stop, once the client has left."""
        if client is None:
            return
        how = client_left(client)
        if how is not None:
            departed(how, phase)

    def departed(how, phase):
        # Named here unless /v1/status or rank 0's admission saw it first (Q-403).
        client_departure(request_id, how, phase)
        started[0].stop()
        raise ClientGone(how)

    def progress(processed, total):
        # mlx_lm 0.31.3 counts only what it computes: `total` is the prompt past the restored prefix
        # (the batch's `_currently_processing` sums the segments `fetch_nearest_cache` left). The
        # row reports the prompt POSITION (rank_live.py), so the prefix goes back in (Q-338: E2E
        # #3p's turn 7, 113,824 of 114,948 cached, read "1,124 of 114,948" and no prefill rate).
        # Only drained after the context set `prompt_tokens` below.
        entry["prefilled"] = entry["prompt_tokens"] - total + processed
        still_there("prefill")
        if progress_callback is not None:
            progress_callback(processed, total)

    def leave():
        # A row still in the batch keeps the request listed (`leaving`) until the generation loop
        # removes it (`row_left`): until then it holds the engine as any other row does.
        with lock:
            if request_id in in_batch:
                handled = handling[request_id]
                handled["handler_left"] = True
                handled["handler_left_at"] = time.monotonic()
                return
            live.pop(request_id, None)
            watches.pop(request_id, None)
            samplings.pop(request_id, None)
            handling.pop(request_id, None)

    tap = RequestTap(self, handled)
    try:
        ctx, tokens = original_generate(tap, request, generation_args, progress)
    except ContextFull as full:
        leave()
        raise Refused(400, str(full), "context_length_exceeded") from None
    except EmptyPrompt as empty:
        leave()
        raise Refused(400, str(empty), "empty_prompt") from None
    except ClientDeparted as departure:
        # Q-403: its client left before the loop gave it batch work; named where it was seen, and
        # rank 0 never shares it.
        leave()
        raise ClientGone(departure.how) from None
    except BaseException:
        leave()
        raise
    ctx.answers = tap.answers
    ctx.client = client
    started.append(ctx)
    with lock:
        handled["ctx"] = ctx
        gone = handled["departed"] is not None
    if gone:
        # Seen gone between the loop handing the context back and here: the row leaves at its
        # first step's end, and the ClientDeparted on its queue ends this handler at its first read.
        ctx.stop()
    entry["max_tokens"] = generation_args.max_tokens
    # The generation thread hands back the context as it takes the request into its batch: from
    # here the engine is reading the prompt, though mlx_lm reports the first progress only after
    # its first chunks (measured: 6,144 of 15,249 tokens, 30 s in, on a 2-rank 27B).
    entry["prefill_started"] = time.monotonic()
    entry["prompt_tokens"] = len(ctx.prompt)
    if ctx.prompt_cache_count >= 0:
        entry["cached_tokens"] = ctx.prompt_cache_count
        # The read starts after the restored prefix, as the pipeline's does (pipeline_rank.py).
        entry["prefilled"] = ctx.prompt_cache_count

    if watch is not None:
        watch.request_id = request_id
        watch.sequences = ctx.sequences

    def counted():
        try:
            pieces = iter(tokens)
            while True:
                # Resumed: the relay and the handler have done all they do with the last piece.
                if watch is not None:
                    watch.settle()
                try:
                    response = next(pieces)
                except StopIteration:
                    break
                except ClientDeparted as departure:
                    departed(
                        departure.how, "prefill" if entry["first_token"] is None else "generation"
                    )
                now = time.monotonic()
                if entry["first_token"] is None:
                    entry["first_token"] = now
                    entry["prefilled"] = entry["prompt_tokens"]
                entry["last_token"] = now
                entry["completion"] += 1
                if watch is not None:
                    watch.take(response)
                still_there("generation")
                yield response
        finally:
            if watch is not None:
                watch.end()
            leave()

    return ctx, counted()


server.ResponseGenerator.generate = generate

original_share_request = server.ResponseGenerator._share_request


def settle(self, request):
    """(request, prompt tokens), or (None, 0) for a request answered on its own queue.

    Rank 0's generation thread, the model loaded, before the request reaches the other ranks: the
    prompt is counted with mlx_lm's own _tokenize on a COPY (_tokenize rewrites messages in place
    — tool-call arguments become dicts — and a second pass over the same objects would fail), and
    the budget replaces the client's absence (None, kept by validate_model_parameters below). A
    request that cannot be counted or has no room is answered on its own queue and never shared,
    as mlx_lm answers a tokenization failure. A request whose client has already left is not
    counted at all (Q-403, `still_wanted`): its handler is woken, and nothing is shared."""
    if not still_wanted(request):
        return None, 0
    rqueue, completion, args = request
    try:
        prompt = original_tokenize(
            self, self.model_provider.tokenizer, copy.deepcopy(completion), args
        )[0]
        if not prompt:
            raise EmptyPrompt()
        args.max_tokens = generation_budget(spec["context_window"], len(prompt), args.max_tokens)
    except Exception as refusal:
        rqueue.put(refusal)
        return None, 0
    return request, len(prompt)


class EmptyPrompt(ValueError):
    """Q-164: a prompt with no token (a text completion of ""): nothing to generate from, and
    mlx_lm 0.31.3 would hand its batch an empty segment."""

    def __init__(self):
        super().__init__("the prompt is empty: it tokenizes to no token, so there is nothing to answer")


def _share_request(self, request):
    if request is not None and group.rank() == 0:
        request = settle(self, request)[0]
    return original_share_request(self, request)


original_tokenize = server.ResponseGenerator._tokenize
server.ResponseGenerator._share_request = _share_request

# Q-142 (rank_boundary.py): only a launch whose every rank cuts at the boundary asks for it — the
# cut ends a prefill chunk, and a peer that did not cut would run a different number of steps.
transient_tail_boundary = bool(spec.get("transient_tail_boundary"))


def _tokenize(self, tokenizer, request, args):
    """mlx_lm's tokenization, with a segment ending at the stable boundary of a request that names
    its transient tail (rank_boundary.py), and one ending at the chat's stable head (Q-347).
    `settle` counts with the upstream one."""
    tail = getattr(request, "transient_tail", None)
    if not tail or request.request_type != "chat" or not tokenizer.has_chat_template:
        return original_tokenize(self, tokenizer, request, args)
    # Upstream rewrites the messages in place (content lists joined, tool-call arguments parsed);
    # the stable conversation is rendered from the same rewrite of an untouched copy.
    messages = copy.deepcopy(request.messages)
    prompt, segments, segment_types, initial_state = original_tokenize(
        self, tokenizer, request, args
    )
    server.process_message_content(messages)
    template_args = self.model_provider.cli_args.chat_template_args
    if args.chat_template_kwargs:
        template_args = {**template_args, **args.chat_template_kwargs}
    head = cut_stable_head(tokenizer, messages, request.tools, template_args, prompt)
    try:
        stable = stable_messages(messages, tail)
    except TailIgnored as ignored:
        if group.rank() == 0:
            emit("RANK_TRANSIENT_TAIL_IGNORED", {"why": str(ignored), "tail_chars": len(tail)})
        segments, segment_types = cut_at_head(segments, segment_types, head)
        return prompt, segments, segment_types, initial_state
    future = tokenizer.apply_chat_template(
        [*stable, {"role": "assistant", "content": BOUNDARY_PROBE}],
        tools=request.tools,
        tokenize=True,
        add_generation_prompt=False,
        **template_args,
    )
    boundary = stable_boundary(prompt, future)
    if conversation_prefix is not None and 0 < boundary < len(prompt):
        conversation_prefix.cut(prompt[:boundary])
    segments, segment_types = cut_at_boundary(segments, segment_types, boundary)
    segments, segment_types = cut_at_head(segments, segment_types, head)
    return prompt, segments, segment_types, initial_state


def cut_stable_head(tokenizer, messages, tools, template_args, prompt):
    """Where the chat's stable head ends in `prompt` — mlx_lm's own system-segment end, rendered
    from the same messages, tools and template switches — recorded for the cache to keep. 0 on a
    spec that keeps no head, or for a conversation that opens on no system message."""
    probe = head_probe(messages) if stable_head is not None else None
    if probe is None:
        return 0
    head = head_end(
        prompt,
        tokenizer.apply_chat_template(
            probe, tools=tools, tokenize=True, add_generation_prompt=False, **template_args
        ),
    )
    if head:
        stable_head.cut(prompt[:head])
    return head


original_chat_request = server.APIHandler.handle_chat_completions


def handle_chat_completions(self):
    # The tail rides the request every rank receives (`_share_request` pickles it whole).
    request = original_chat_request(self)
    request.transient_tail = self.body.get(TRANSIENT_TAIL)
    return request


if transient_tail_boundary:
    server.ResponseGenerator._tokenize = _tokenize
    server.APIHandler.handle_chat_completions = handle_chat_completions

# Q-161 (rank_xml_guard.py): a tool request's decode held to the XML call's skeleton. It changes
# what is sampled, so only a launch whose every rank runs it asks for it (`xml_skeleton_guard`);
# every rank builds the same rules from its own copy of the same tokenizer when the model loads.
xml_skeleton_guard = bool(spec.get("xml_skeleton_guard"))
skeleton = {"spec": None}


def arm_skeleton_guard(tokenizer):
    try:
        skeleton["spec"] = skeleton_spec(tokenizer, tokenizer.eos_token_ids)
    except GuardUnarmed as why:
        emit("RANK_XML_GUARD", {"rank": group.rank(), "armed": False, "why": str(why)})
        return
    emit(
        "RANK_XML_GUARD",
        {"rank": group.rank(), "armed": True, "rules": skeleton["spec"].report()},
    )


def mark_tool_request(request):
    """Every rank, as a shared request leaves `_next_request`: mlx_lm makes a request's logits
    processors from its arguments alone, so whether it carries tools rides the arguments (the
    single engine guards tool requests only)."""
    if xml_skeleton_guard and request is not None:
        request[2].tool_request = bool(request[1].tools)


if xml_skeleton_guard:
    if not hasattr(server, "_make_logits_processors"):
        raise SystemExit(
            f"goose rank wrapper: mlx_lm {mlx_lm.__version__} has no "
            "mlx_lm.server._make_logits_processors; the XML skeleton guard was written against "
            "mlx_lm 0.31.3"
        )
    original_logits_processors = server._make_logits_processors

    def _make_logits_processors(args):
        processors = original_logits_processors(args)
        if getattr(args, "tool_request", False) and skeleton["spec"] is not None:
            processors = [*processors, XmlSkeletonGuard(skeleton["spec"])]
        return processors

    server._make_logits_processors = _make_logits_processors

# Q-161 (rank_batch.py `row_processors`): each generating row runs its own logits processors — the
# guard above ran on one token of E2E #3f's tool request, because mlx_lm's GenerationBatch.filter
# left a departed tool-less row's `[]` in front of it. Which row runs which processors decides what
# is sampled, so only a launch whose every rank runs it asks for it (`row_processors`).
if spec.get("row_processors"):
    mlx_generate.GenerationBatch.__init__ = generation_init(mlx_generate.GenerationBatch.__init__)
    mlx_generate.GenerationBatch.filter = generation_filter(mlx_generate.GenerationBatch.filter)


# A BaseException so mlx_lm's handle_completion (`except Exception` → 404) lets it through to
# do_POST, which answers with the status it names.
class Refused(BaseException):
    def __init__(self, status, message, code=None, param=None):
        super().__init__(message)
        self.status = status
        self.message = message
        self.code = code
        self.param = param


def send_json(handler, status, payload):
    handler._set_completion_headers(status)
    handler.end_headers()
    handler.wfile.write(json.dumps(payload).encode())
    handler.wfile.flush()


original_get = server.APIHandler.do_GET
original_post = server.APIHandler.do_POST
original_validate = server.APIHandler.validate_model_parameters


def do_GET(self):
    if self.path == "/goose/progress":
        return send_json(
            self,
            200,
            {
                "steps": state["steps"],
                "inflight": state["inflight"],
                "admission_open": admission.open,
                "active": mx.get_active_memory(),
                "peak": mx.get_peak_memory(),
            },
        )
    if self.path == "/v1/status":
        # num_running counts the requests someone still waits for (Q-403): every live row whose
        # client is there and whose answer has not ended — queued, held, reading or writing — and
        # every handler that has not reached `generate` yet. mlx_lm does not split queued from
        # batched, so num_waiting carries none of them (the sum is the busy fact goose's router
        # sizes its free slots by). A row whose client left, or whose answer ended while it still
        # holds the batch (`leaving`, Q-231), is listed and not counted: nobody waits for it, and
        # the loop drops it at its step's end — before it takes any request that arrives now. So
        # every live request's socket is read first, without waiting: E2E #3r's chat call was
        # picked with `free_slots 0` 4 s after goose had closed all eight reviewers, because
        # nothing read their sockets until the 43 s prompt step they were in ended.
        # The request table tells prefill from generation per request. A streamed chat request's
        # row carries `stream` (rank_stream_watch.py, Q-146): what its client has and has not been
        # sent, and the last words written; null for any other request. Each row says who sent it
        # and, when it waits or has ended, on what (rank_live.py `row_handling`, Q-231).
        with lock:
            unread = [
                (request_id, handled["connection"], request_phase(live[request_id]))
                for request_id, handled in handling.items()
                if handled["departed"] is None
                and not handled["handler_left"]
                and handled["connection"] is not None
            ]
        for request_id, connection, phase in unread:
            read_departure(request_id, connection, phase)
        now = time.monotonic()
        held_for_room = set(held_ids[0])
        with lock:
            rows = [
                live_request(request_id, now=now, **entry)
                for request_id, entry in live.items()
            ]
            waited_for = 0
            for row in rows:
                request_id = row["request_id"]
                watch = watches.get(request_id)
                row["stream"] = None if watch is None else watch.report()
                row["sampling"] = samplings.get(request_id)
                handled = handling[request_id]
                row.update(
                    row_handling(
                        handled["client"],
                        handled["stopped"],
                        handled["stopped_at"],
                        handled["handler_left"],
                        live[request_id]["arrived"],
                        request_id in held_for_room,
                    )
                )
                waited_for += not handled["handler_left"] and handled["departed"] is None
            num_running = waited_for + state["before_row"]
            last_stop = engine_stops["last"]
        body = live_status({"num_running": num_running, "num_waiting": 0}, rows)
        body["sampling_defaults"] = sampling_defaults.report()
        # The last answer the engine ended itself (Q-161, Q-181): its row leaves /v1/status with it.
        body["last_engine_stop"] = last_stop
        return send_json(self, 200, body)
    if self.path == ADMISSION_PATH:
        answer = admission.wait_open()
        try:
            return send_json(self, 200, answer)
        except OSError:
            # The client left while it waited (goose's Stop): nobody is left to answer.
            self.close_connection = True
            return None
    if self.path.startswith("/v1/models"):
        return send_json(
            self,
            200,
            {
                "object": "list",
                "data": [
                    {
                        "id": name,
                        "object": "model",
                        "owned_by": "goose-distributed",
                        "context_window": spec["context_window"],
                        # The single engine's shape (Rapid-MLX routes/models.py): mlx_lm.server
                        # refuses every non-text content part, so no "vision" (Q-260: goose sends
                        # an engine that declares it reads text only a placeholder per image).
                        "capabilities": ["text", "tools"],
                        **(
                            {"request_extensions": list(TRANSIENT_TAIL_EXTENSIONS)}
                            if transient_tail_boundary
                            else {}
                        ),
                    }
                    for name in served_names
                ],
            },
        )
    return original_get(self)


def do_POST(self):
    if self.path == ADMISSION_PATH:
        length = int(self.headers.get("Content-Length") or 0)
        answer = admission.set(json.loads(self.rfile.read(length) or b"{}"))
        if not admission.open:
            # Memory is short: MLX's free-buffer cache goes back to the OS now (per process, no
            # collective, nothing a peer must mirror).
            mx.clear_cache()
        return send_json(self, 200, answer)
    if not admission.open:
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        return send_json(self, 503, admission.refusal())
    with lock:
        state["inflight"] += 1
        state["before_row"] += 1
    posting.before_row = True
    try:
        original_post(self)
    except Refused as refusal:
        error = {"message": refusal.message}
        if refusal.code is not None:
            error["code"] = refusal.code
            error["type"] = "invalid_request_error"
        if refusal.param is not None:
            error["param"] = refusal.param
        send_json(self, refusal.status, {"error": error})
    except ClientGone:
        # Named where it was seen (GOOSE_RANK_CANCELLED_BY_CLIENT); nobody is left to answer.
        self.close_connection = True
    except Exception as failure:
        # Q-177: http.server answers an escaped exception by closing the socket. Until a byte of
        # the response reached the client the client is told what failed; after it, the response
        # already begun is all there is. http.server buffers the status line and headers from
        # send_response until end_headers writes them and empties the buffer (Q-233: mlx_lm's
        # non-streamed answer buffers its 200 before generating), so an EMPTY buffer is a response
        # on the wire, and a pending one is discarded here — this handler serves one request.
        if getattr(self, "_headers_buffer", None) == []:
            raise
        self._headers_buffer = []
        named = f"{type(failure).__name__}: {failure}"
        error = {"message": named, "type": "server_error"}
        if isinstance(failure, ToolCallUnparsed):
            error["code"] = "tool_call_unparsed"
            error["tool_text"] = failure.tool_text
        emit("RANK_REQUEST_FAILED", {"path": self.path, "error": named, "code": error.get("code")})
        send_json(self, 500, {"error": error})
    finally:
        with lock:
            state["inflight"] -= 1
            if posting.before_row:
                state["before_row"] -= 1
        posting.before_row = False


def apply_sampling(handler):
    """Each sampling field the request left out or sent null, set to its resolved layer's value
    (mlx_lm had filled it with its own CLI default — temperature 0.0, greedy); returns each
    field's layer. An unset field keeps the sampler's off value mlx_lm fills an absence with."""
    cli = handler.response_generator.cli_args
    sampler_off = {"temperature": cli.temp, "top_p": cli.top_p, "top_k": cli.top_k, "min_p": cli.min_p}
    layers = {}
    for key, (value, layer) in sampling_defaults.resolve(handler.body).items():
        if layer != "request":
            setattr(handler, key, sampler_off.get(key, 0.0) if value is None else value)
        layers[key] = layer
    return layers


def refuse_request(refusal):
    raise Refused(400, str(refusal), refusal.code, refusal.param) from None


def validate_model_parameters(self):
    # rank_request.py (Q-177): a field refused here never reaches `_share_request`.
    try:
        refused_request_fields(self.path, self.body)
    except RequestRefused as refusal:
        refuse_request(refusal)
    if self.stream_options is not None:
        # OpenAI's meaning of an absent include_usage; mlx_lm indexes the key after the last
        # token, so `{}` ended a streamed answer on a KeyError, with no [DONE].
        self.stream_options = {
            **self.stream_options,
            "include_usage": bool(self.stream_options.get("include_usage")),
        }
    # rank_sampling.py (Q-159): before mlx_lm validates, so a null resolves instead of failing.
    self.sampling_sources = apply_sampling(self)
    # mlx_lm read an absent max_tokens as its `--max-tokens` default; the absence is kept instead
    # (None rides the shared request to every rank), and _tokenize turns it into the room left.
    absent = all(
        self.body.get(key) is None for key in ("max_completion_tokens", "max_tokens")
    )
    if absent:
        self.max_tokens = spec["context_window"]
    try:
        original_validate(self)
    except ValueError as invalid:
        refuse_request(validator_refusal(self.body, invalid))
    if absent:
        self.max_tokens = None
    if self.requested_model not in (*served_names, "default_model"):
        also = f" (also answering to {', '.join(repr(n) for n in served_aliases)})" if served_aliases else ""
        raise Refused(
            404,
            f"model '{self.requested_model}' is not served here; this distributed engine serves '{served}'{also}",
        )
    # The shared request names the model to every rank, and only the served id maps to each rank's
    # own --model path (`run`): an alias is answered as the served id, so no rank — a peer running
    # an older wrapper included — ever loads by a name it cannot resolve.
    if self.requested_model in served_aliases:
        self.requested_model = served
    if self.adapter is not None or self.body.get("draft_model") not in (None, "default_model"):
        raise Refused(400, "adapters and draft models are not supported by the distributed engine")
    tail = self.body.get(TRANSIENT_TAIL)
    if transient_tail_boundary and tail is not None and not isinstance(tail, str):
        raise Refused(
            400,
            f"{TRANSIENT_TAIL} must be the exact text the last user or tool message ends on, "
            f"not {type(tail).__name__}",
            "invalid_request_error",
        )
    if self.path in ("/v1/chat/completions", "/chat/completions"):
        # rank_thinking.py: the single engine's thinking resolution. Set on rank 0 before the
        # request is shared, so every rank renders the same prompt.
        provider = self.response_generator.model_provider
        if provider.tokenizer is None:
            model_loaded.wait()
        tokenizer = provider.tokenizer
        try:
            self.chat_template_kwargs = resolved_template_kwargs(
                self.body, template_reasons(tokenizer.chat_template)
            )
        except ThinkingRefused as refusal:
            raise Refused(400, str(refusal), "unsupported_parameter")


server.APIHandler.do_GET = do_GET
server.APIHandler.do_POST = do_POST
server.APIHandler.validate_model_parameters = validate_model_parameters

# Q-141 (rank_tool_stream.py): a streamed chat answer's tool calls reach the client as the model
# writes them. mlx_lm's own handle_completion runs unchanged around a relay of its token stream:
# while a call is written the relay sends its open frame and argument fragments, in the handler's
# thread, between mlx_lm's own frames; when the call ends it sends the remainder of the parser's own
# serialization. The formatter mlx_lm then runs on the same text is handed a parser that answers
# "already delivered" (no calls), so no call is ever sent twice; finish_reason, text, reasoning and
# usage stay mlx_lm's.
original_handle_completion = server.APIHandler.handle_completion
tool_stream_refusals = set()


def delivered(tool_text, tools):
    return []


# The last answer the engine ended itself, for /v1/status once its row has left: the request, why
# (`tool_call_repeated` / `text_cycle`, Q-161; `cancelled_by_client`, Q-181), and the words.
engine_stops = {"last": None}


def record_stop(stop):
    with lock:
        engine_stops["last"] = stop
        handled = handling.get(stop["request_id"])
        if handled is not None:
            handled["stopped"] = stop
            handled["stopped_at"] = time.monotonic()
    emit(f"RANK_{stop['reason'].upper()}", stop)


def engine_stop(watch, reason, detail):
    stop = {"request_id": watch.request_id, "reason": reason, **detail}
    watch.stop = stop
    record_stop(stop)


class StreamedCall:
    """One call of a streamed answer: its streamer, its id, and — until the relay sends it — the
    frames built for it (`index` is None while the call is held, Q-161)."""

    def __init__(self):
        self.stream = ToolCallStream(qwen3_coder._convert_param_value, qwen3_coder._get_arguments_config)
        self.id = str(uuid.uuid4())
        self.index = None
        self.held = []


class StreamedToolCalls:
    """The response generator as one streamed chat request's handler sees it.

    Q-161: a call the model writes word for word again in the same answer ends the answer. E2E
    #3e's turn 0 (sampling from generation_config) was ONE answer of 649 calls — the same `write`
    of notes/kickoff.md and the same `mkdir -p …/notes`, 324 times each, then a call cut by the
    context — 221,604 tokens over ~6.5 h; #3d's (greedy) was 54 identical `ledger_append`s. Nothing
    reaches the model between the calls of one answer, so a call it repeats verbatim learns nothing
    and runs nothing new (goose answers such a call "Not run: identical to call #1"), and each copy
    makes the next more certain: measured on the split, after 4, 6 and 8 calls of that pair the
    model put p≈0.88–1.0 on `\\n<tool_call>` at the junction and 0.04–0.07 on `<|im_end|>`. The
    relay holds a call while its text is still word for word the start of a call this answer
    already closed, sends it the moment it differs, and when it closes identical the call is never
    sent: the relay names it (GOOSE_RANK_TOOL_CALL_REPEATED, with the words), stops the generation
    on every rank (the handler's own `ctx.stop()`, which rank 0 shares), and the answer ends there
    with the calls it made — finish_reason `tool_calls`. The measure is the model's own output, not
    a count or a clock: the first verbatim repeat, whatever its length or position."""

    def __init__(self, handler, upstream, watch):
        self.handler = handler
        self.upstream = upstream
        self.watch = watch
        self.parse = None
        self.tools = None
        self.ctx = None
        self.index = 0
        # The text of every call this answer closed, in order (the streamer's, between the markers).
        self.closed = []
        # The answer's text outside its calls and its reasoning, as written.
        self.outside = ""

    def __getattr__(self, name):
        return getattr(self.upstream, name)

    def generate(self, request, generation_args, progress_callback=None):
        ctx, tokens = self.upstream.generate(
            request,
            generation_args,
            progress_callback,
            watch=self.watch,
            client=self.handler.connection,
            address=self.handler.client_address,
        )
        if not ctx.has_tool_calling:
            return ctx, tokens
        if ctx.tool_parser is not qwen3_coder.parse_tool_call:
            parser = getattr(ctx.tool_parser, "__module__", repr(ctx.tool_parser))
            if parser not in tool_stream_refusals:
                tool_stream_refusals.add(parser)
                emit("RANK_TOOL_STREAM_UNSUPPORTED", {"parser": parser})
            self.watch.unstreamed = (
                f"the tool parser {parser} has no streamer: mlx_lm sends the call whole when it closes"
            )
            return ctx, tokens
        self.parse = ctx.tool_parser
        self.tools = request.tools
        self.ctx = ctx
        ctx.tool_parser = delivered
        return ctx, self.relay(tokens)

    def relay(self, tokens):
        call = None
        for gen in tokens:
            if gen.state == "tool":
                if call is None:
                    call = StreamedCall()
                    self.watch.streamer = call.stream
                self.feed(call, gen.text)
            elif call is not None:
                repeated = self.finish(call)
                call = None
                if repeated:
                    # The `</tool_call>` that closed the repeat is not handed on: the handler ends
                    # the answer on the calls already sent, and its `finally` stops nothing twice.
                    self.ctx.stop()
                    tokens.close()
                    return
            if gen.state == "normal" and self.cycled(gen.text):
                # The piece that completed the cycle is not handed on; the handler ends the answer
                # on what it has sent (finish `stop`, or `tool_calls` when a call closed before).
                self.ctx.stop()
                tokens.close()
                return
            yield gen
        if call is not None:
            self.finish(call)

    def cycled(self, text):
        """True when the answer's text outside its calls has become one span written over and over
        (rank_stream_watch.py `verbatim_cycle`): the engine names it and the answer ends. Q-161:
        E2E #3f turn 0 left its one `write` call and wrote `!\\n</parameter>\\n</function>\\n`,
        then `!\\n</function>\\n`, 3,251 chars and counting, never another call — the call-repeat
        stop has nothing to compare. A span holds a line break, so the text is read when one
        arrives."""
        self.outside += text
        if "\n" not in text:
            return False
        cycle = verbatim_cycle(self.outside)
        if cycle is None:
            return False
        unit, copies = cycle
        engine_stop(
            self.watch,
            "text_cycle",
            {
                "unit": unit,
                "copies": copies,
                "outside_chars": len(self.outside),
                "calls": len(self.closed),
                "generated_chars": self.watch.generated_chars,
                "tail": self.watch.tail,
            },
        )
        return True

    def repeat_of(self, text, whole):
        """The 1-based position of the first call this answer closed whose text `text` is (whole) or
        begins (not whole), else None."""
        for position, earlier in enumerate(self.closed, start=1):
            if earlier == text if whole else earlier.startswith(text):
                return position
        return None

    def feed(self, call, text):
        opened, fragment = call.stream.feed(text, self.tools)
        frame = None
        if opened is not None:
            frame = {"id": call.id, "type": "function", "function": {"name": opened, "arguments": fragment}}
        elif fragment:
            frame = {"function": {"arguments": fragment}}
        if call.index is not None:
            if frame is not None:
                self.send(call.index, frame)
            return
        if frame is not None:
            call.held.append(frame)
        position = self.repeat_of(call.stream.text, whole=False)
        if position is None:
            self.release(call)
        else:
            self.watch.holding = (
                f"the call so far is word for word the start of call {position} of this answer: "
                "it is sent the moment it differs, and ends the answer unsent if it closes identical"
            )

    def release(self, call):
        call.index = self.index
        self.index += 1
        self.watch.holding = None
        for frame in call.held:
            self.send(call.index, frame)
        call.held = []

    def finish(self, call):
        """Ends a call; True when it closed word for word a call this answer already made."""
        stream = call.stream
        self.watch.streamer = None
        self.watch.holding = None
        position = self.repeat_of(stream.text, whole=True)
        self.closed.append(stream.text)
        if position is not None:
            engine_stop(
                self.watch,
                "tool_call_repeated",
                {
                    "name": stream.name,
                    "repeat_of": position,
                    "calls": len(self.closed) - 1,
                    "chars": len(stream.text),
                    "generated_chars": self.watch.generated_chars,
                    "tail": self.watch.tail,
                },
            )
            return True
        if call.index is None:
            self.release(call)
        rest, why = stream.close(self.parse, self.tools)
        if why is not None:
            emit(
                "RANK_TOOL_CALL_UNPARSED",
                {"name": stream.name, "why": why, "chars": len(stream.text), "sent_chars": len(stream.sent)},
            )
            return False
        self.send(call.index, {"function": {"arguments": rest}})
        return False

    def send(self, index, tool_call):
        frame = self.handler.generate_response("", None, tool_calls=[{"index": index, **tool_call}])
        self.handler.wfile.write(f"data: {json.dumps(frame)}\n\n".encode())
        self.handler.wfile.flush()


class ClientBound:
    """The response generator as any other request's handler sees it: its connection is read for
    the client's departure at every piece (Q-181)."""

    def __init__(self, upstream, handler):
        self.upstream = upstream
        self.handler = handler

    def __getattr__(self, name):
        return getattr(self.upstream, name)

    def generate(self, request, generation_args, progress_callback=None):
        return self.upstream.generate(
            request,
            generation_args,
            progress_callback,
            client=self.handler.connection,
            address=self.handler.client_address,
        )


def handle_completion(self, request, stop_words):
    # Each field's layer rides the request to `generate` (and, pickled, to every rank — unread there).
    request.sampling_sources = self.sampling_sources
    if not (self.stream and self.object_type.startswith("chat.completion")):
        upstream = self.response_generator
        self.response_generator = ClientBound(upstream, self)
        try:
            return original_handle_completion(self, request, stop_words)
        finally:
            self.response_generator = upstream
    # rank_stream_watch.py (Q-146): every frame this handler builds is counted, and the request's
    # /v1/status row reads what was generated against what was sent.
    watch = StreamWatch(emit)
    upstream = self.response_generator
    self.response_generator = StreamedToolCalls(self, upstream, watch)
    self.generate_response = watch.counting(self.generate_response)
    try:
        return original_handle_completion(self, request, stop_words)
    finally:
        self.response_generator = upstream
        del self.generate_response


server.APIHandler.handle_completion = handle_completion


class ToolCallUnparsed(Exception):
    """The tool parser refused a call of a non-streamed answer (Q-233): its words and the call."""

    def __init__(self, parser, refusal, tool_text):
        super().__init__(
            f"the model's tool call could not be parsed by {parser} — "
            f"{type(refusal).__name__}: {refusal}"
        )
        self.tool_text = tool_text


upstream_tool_call_formatter = server.ToolCallFormatter


class NamedToolCallFormatter(upstream_tool_call_formatter):
    """mlx_lm's formatter, except that on a non-streamed answer any refusal of its parser is a
    ToolCallUnparsed (do_POST's named 500) instead of a skipped call or a dropped connection. A
    streamed answer's formatter is mlx_lm's own, unchanged."""

    def __init__(self, tool_parser, tools, streaming=False):
        if not streaming and tool_parser is not None:
            tool_parser = refusing_unparsed(tool_parser)
        super().__init__(tool_parser, tools, streaming)


def refusing_unparsed(parser):
    name = getattr(parser, "__module__", None) or repr(parser)

    def parse(tool_text, tools):
        try:
            return parser(tool_text, tools)
        except Exception as refusal:
            emit(
                "RANK_TOOL_CALL_UNPARSED",
                {
                    "stream": False,
                    "parser": name,
                    "why": f"{type(refusal).__name__}: {refusal}",
                    "chars": len(tool_text),
                },
            )
            raise ToolCallUnparsed(name, refusal, tool_text) from refusal

    return parse


server.ToolCallFormatter = NamedToolCallFormatter

sys.argv = [
    "mlx_lm.server",
    "--model",
    spec["model_dir"],
    "--host",
    "127.0.0.1",
    "--port",
    str(spec["port"]),
    *prompt_cache_flags,
    *(["--prefill-step-size", str(int(prefill["step"]))] if prefill is not None else []),
]
server.main()
