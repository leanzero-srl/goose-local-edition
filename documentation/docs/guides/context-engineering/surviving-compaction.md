---
title: Surviving Compaction
description: The scratchpad, the project ledger and recall — what goose keeps when the context is summarized, and how it writes its own memories
---

When a session grows past the context limit, goose summarizes the older part of the conversation and continues from the summary. A summary keeps the shape of the work and loses the thread: which file was mid-edit, which number decided something, what the user corrected. goose keeps three things outside the conversation so that a compaction is a re-read, not a restart.

## The scratchpad (this session)

The `todo` extension's content is goose's scratchpad. It is stored with the session, shown to the model in every turn's context inside a `<scratchpad>` block, and survives compaction verbatim. goose is told to keep it current after every meaningful step in five parts — Goal, Done, In flight, Next, Facts (paths, commands, numbers, decisions and why) — and when the turn context says compaction is near, a `<scratchpad-notice>` asks it to refresh the scratchpad first. The notice fires in the last quarter of the room before the compaction threshold, and only when the scratchpad extension is on.

## The project ledger (this project)

`.goose/ledger.md` is the project's dated log: one line per finding, decision, attempt that is not coming back, or fact learned. The `ledger` extension appends to it with `ledger_append(kind, text)` and reads it with `ledger_read(query?, limit?)`; the newest five entries ride in every turn's context in a `<ledger>` block, so after a compaction — or in a new session next week — goose sees what the project has already been through. It is the chronological complement of project memory: memory holds facts deduplicated by headline and recalled by topic; the ledger holds what happened, in order.

## Recall (what matches the request)

On every request the `recall` extension searches memory with the request's own words and adds the matching memories in full, names matching skills, and points at the newest earlier session that discussed the same thing. Three more things happen in the same pass:

- **A skill is loaded without a call** when the request names it — two of its terms sit in the skill's name or keywords — and its body fits a thirty-second of the model's context window. It appears in a `<loaded-skill>` block; larger skills are named for `load_skill` instead.
- **A correction is noticed.** When the request opens like a correction of what goose just did ("No, don't…", "That's not what I…", "Why did you…"), a `<correction>` block quotes the action and asks goose to save the rule and its reason with `remember_memory` before continuing.
- **An answer is noticed.** When goose's last message asked a question and the request answers it, an `<answered>` block quotes both and asks goose to save the answer if it is a durable fact — a path, a host, a convention, a preference — so it never asks again.

Every recall shows the person one line in the transcript — `recalled: memories …, skills …, loaded …, correction noticed` — and `goose recall "<request>"` prints the whole block for any request.

## After a compaction

The summary carries the conversation; the turn context carries the scratchpad, the ledger's tail and whatever recall found for the current request, all intact. goose is told to trust those over the summary where they differ.
