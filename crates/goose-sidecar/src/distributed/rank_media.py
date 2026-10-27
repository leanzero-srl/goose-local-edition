# goose distributed rank: MEDIA input (images, video) for an omni MiMo-V2.6 checkpoint.
# Concatenated after rank_admission.py, before rank_pipeline_split.py (launch.rs TENSOR_PROGRAM).
# A model folder without `vision_tower.*` weights (every text-only build) runs none of it and
# keeps declaring ["text", "tools"], so goose still sends such an engine the named placeholder.
#
# What it does, and why (Session 12, 2026-10-27: the local MiMo Gauntlet could not see its own
# screenshots; the cloud model did):
# - the fork's `rapid_mlx.mlx_lm_models.mimo_v2_media` holds the vision tower (parity with the
#   HF reference: cosine 0.99998+ per layer in float32), Xiaomi's preprocessing and the MARKER
#   scheme. This file only wires it into mlx_lm.server.
# - rank 0 (HTTP + the vision tower) prepares each request as it leaves the request queue, in
#   the generation thread (MLX is used from that thread only): every image_url / video_url part
#   becomes the model's own text layout and the request carries a manifest (kind, key, rows per
#   item) plus the features the layer-0 process lacks. The request is then pickled to every rank
#   as before, so a split needs no new channel.
# - every rank expands each <|image_pad|> / <|video_pad|> the template renders into the item's
#   marker ids (mimo_v2_media.expand), so all ranks hold the same prompt tokens; the process that
#   runs layer 0 (pipeline: the LAST rank; tensor/single: every rank) stores the shipped features
#   and the model swaps marker rows for them before layer 0.
# - /v1/models declares "vision" and "video" (rank_wrapper.py), which is what goose's providers
#   read (openai.rs `image_input`: a declared "vision" engine gets the image parts).
# - a media failure (unreadable image, missing features) is the request's error on every rank,
#   never a silent text-only answer.
# Knobs: GOOSE_MIMO_MEDIA=0 (off), GOOSE_MIMO_IMAGE_NORM=imagenet|clip, GOOSE_MIMO_IMAGE_MAX_PIXELS,
# GOOSE_MIMO_VIDEO_MAX_TOKENS, GOOSE_MIMO_VISION_BITS (0 = bf16, 8 = 8-bit).

media_enabled = False
media_capabilities = []


def media_wanted(model_dir):
    if os.environ.get("GOOSE_MIMO_MEDIA", "1") == "0":
        return False
    try:
        from rapid_mlx.mlx_lm_models import mimo_v2_media  # noqa: F401
    except Exception as failure:  # an older fork pin: text only, said once
        emit("RANK_MEDIA", {"rank": spec["rank"], "enabled": False, "why": f"{type(failure).__name__}: {failure}"})
        return False
    return mimo_v2_media.has_vision(model_dir)


if media_wanted(spec["model_dir"]):
    import uuid as media_uuid  # noqa: E402
    from queue import Queue as MediaQueue  # noqa: E402

    import mlx_lm.server as media_server  # noqa: E402
    import mlx_lm.utils as media_utils  # noqa: E402
    from rapid_mlx.mlx_lm_models import mimo_v2_media  # noqa: E402

    media_enabled = True
    media_capabilities = ["vision", "video"]
    media_vocab = None
    media_rank, media_size = int(spec["rank"]), int(spec["size"])
    media_pipeline = spec.get("pipeline_split") is not None
    # Who runs layer 0: a pipeline's LAST rank (rank 0 holds the last layers), or every rank.
    media_is_layer0 = (media_rank == media_size - 1) if media_pipeline else True
    media_frontend = [None]
    media_current = {"manifest": None, "thread": None, "calls": 0}
    media_applied = set()

    def media_vocab_size():
        global media_vocab
        if media_vocab is None:
            from pathlib import Path as _P

            media_vocab = int(json.loads((_P(spec["model_dir"]) / "config.json").read_text())["vocab_size"])
        return media_vocab

    def media_prepare(item):
        """Rank 0, generation thread: rewrite the request's media parts, attach the manifest."""
        messages = None
        try:
            _, request, _ = item
            messages = getattr(request, "messages", None)
            if not messages or not any(
                isinstance(m.get("content"), list)
                and any(mimo_v2_media.part_kind(p) for p in m["content"])
                for m in messages
            ):
                return item
            if media_frontend[0] is None:
                rank0_is_layer0 = (media_size == 1) or not media_pipeline
                media_frontend[0] = mimo_v2_media.MediaFrontend(
                    spec["model_dir"], media_vocab_size(), local_is_layer0=rank0_is_layer0
                )
            started = time.monotonic()
            manifest = media_frontend[0].prepare(messages)
            if manifest is not None:
                manifest["id"] = media_uuid.uuid4().hex
                messages[0][mimo_v2_media.MEDIA_KEY] = manifest
                emit(
                    "RANK_MEDIA_PREPARED",
                    {
                        "items": [(k, n) for k, _, n in manifest["items"]],
                        "shipped": len(manifest["ship"]),
                        "evicted": len(manifest["evict"]),
                        "seconds": round(time.monotonic() - started, 2),
                    },
                )
        except Exception as failure:  # noqa: BLE001
            if messages:
                messages[0][mimo_v2_media.MEDIA_KEY] = {"error": f"{type(failure).__name__}: {failure}"}
            emit("RANK_MEDIA_FAILED", {"error": str(failure)[:300]})
        return item

    class MediaRequests(MediaQueue):
        """Rank 0's request queue: each request is prepared as the generation loop takes it."""

        def get(self, *args, **kwargs):
            return media_prepare(super().get(*args, **kwargs))

        def get_nowait(self):
            return media_prepare(super().get_nowait())

    original_rg_init = media_server.ResponseGenerator.__init__

    def media_rg_init(self, *args, **kwargs):
        original_rg_init(self, *args, **kwargs)
        if media_rank == 0:
            self.requests = MediaRequests()

    media_server.ResponseGenerator.__init__ = media_rg_init

    original_process_message_content = media_server.process_message_content

    def media_process_message_content(messages):
        manifest = messages[0].get(mimo_v2_media.MEDIA_KEY) if messages else None
        if manifest is not None and "error" in manifest:
            raise ValueError(f"media input: {manifest['error']}")
        if manifest is not None and manifest["id"] not in media_applied:
            mimo_v2_media.apply_manifest(
                manifest,
                media_vocab_size(),
                is_layer0=media_is_layer0,
                shipped_locally=(media_rank == 0 and media_frontend[0] is not None
                                 and media_frontend[0].local_is_layer0),
            )
            media_applied.add(manifest["id"])
            if len(media_applied) > 4096:
                media_applied.clear()
                media_applied.add(manifest["id"])
        media_current.update(manifest=manifest, thread=threading.get_ident(), calls=0)
        return original_process_message_content(messages)

    media_server.process_message_content = media_process_message_content

    def media_tokenizer(tokenizer):
        """Every apply_chat_template that returns tokens expands the media placeholders of the
        request whose messages were just processed (first render: every item must appear)."""
        original_apply = tokenizer.apply_chat_template

        def apply_chat_template(*args, **kwargs):
            out = original_apply(*args, **kwargs)
            manifest = media_current["manifest"]
            if (
                manifest is None
                or media_current["thread"] != threading.get_ident()
                or not kwargs.get("tokenize", False)
                or not isinstance(out, list)
            ):
                return out
            media_current["calls"] += 1
            return mimo_v2_media.expand(
                out, manifest, media_vocab_size(), require_all=media_current["calls"] == 1
            )

        tokenizer.apply_chat_template = apply_chat_template
        return tokenizer

    original_media_load_tokenizer = media_utils.load_tokenizer

    def media_load_tokenizer(*args, **kwargs):
        return media_tokenizer(original_media_load_tokenizer(*args, **kwargs))

    media_utils.load_tokenizer = media_load_tokenizer
    emit(
        "RANK_MEDIA",
        {
            "rank": media_rank,
            "enabled": True,
            "layer0": media_is_layer0,
            "encodes": media_rank == 0,
            "capabilities": media_capabilities,
        },
    )
