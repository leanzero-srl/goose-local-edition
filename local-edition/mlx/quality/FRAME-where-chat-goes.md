# FRAME — "where chat goes" has one source (Q-4, Q-5, Q-8, Q-12, Q-15, Q-17, Q-27, Q-28)

## The symptom cluster (round 1, 3.0.35)
- Q-12 dead end: chip "swarm" → Change Provider → "Switch models" offers only "Goose Swarm: chat — each turn
  goes to an idle node of your pool"; the 27B and the Mac it runs on are never named.
- Q-5: the chip names the provider id, not the model.
- Q-4: the top NODES strip says "mihai-mlx … unmounted" under a green "Serving from Work's Mac Studio · ready".
- Q-8: the green bar stays up while all is well and never names the model.
- Q-17: "· ready" while the Studio reads another client's 39k prompt (a turn would queue).
- Q-15: under "Serving on Work's Mac Studio" the Engine tab shows THIS Mac's memory.
- Q-27: the tray title "Remote · Idle" never names the Mac.
- Q-28: Run on this Mac is not offered while the split runs.

## The cause
Five surfaces each decide where chat goes, from different reads, at different times:

| surface | reads | knows the route? | knows the split? | knows activity? |
|---|---|---|---|---|
| NodesStrip (BaseChat.tsx:601) | saved swarm config + local engine poll | no | no | no |
| ComposerReadinessStrip (ComposerReadiness.tsx:195) | remote store, distributed store, own 3 s local poll | yes | yes | no |
| model chip (ModelsBottomBar.tsx:207) | session model id | no | no | no |
| context counter (ChatInput.tsx) | swarmContextLimit (fixed af06267e7) | yes | yes | — |
| Engine tab memory (MlxEngineView.tsx:1030) | this Mac's status | ignores it | — | — |
| tray title (mlxTray.ts:144) | main's snapshot | yes (engine kind) | yes | yes |

Only main's monitor sees everything all the time (it reads the engine that serves chat, its activity and its
runs, while any engine answers). The renderer surfaces each poll a subset.

## The rule to install
ONE derivation, `chatServedBy`, computed where the three reads already meet (ComposerReadinessStrip's reads:
remote store, distributed store, the local status poll, plus main's activity for busy/idle), published to a
tiny store every chat surface subscribes to:
`{ engine: 'single'|'remote'|'split'|'none', model, where: <Mac name(s)>, phase, activity, contextWindow,
  peerNodeId, busyWithOthers }`.
- chip: `Qwen3.8-27B · Work's Mac Studio` + phase dot; its menu = the models on your Macs (My Macs' facts),
  each with the Run it ways for that model (the same start/switch code PlacementCard runs, extracted into a
  hook — never a second copy); "Open Engine" at the bottom. LM Studio's pattern (loaded model named in the
  chat's picker; LM Link devices in the same list).
- readiness bar: only when something needs the user (loading, failed, no model, busy with another client's
  request) — never a permanent "ready".
- NodesStrip: removed from the chat (it contradicts the bar and has no action); the pool is under Providers.
- Engine tab memory: follows `engine` — the peer's facts under the peer heading.
- tray title: the Mac's name, not "Remote".
- Q-28: Run on this Mac while the split runs = the switch (stop the split, then mount).

## Proof plan
Unit: the derivation over (single running / remote ready / remote mounting / split up / split foreign /
nothing) → one expected object each. Live (installed build, both Macs): the critic re-walks the chat and
Engine tab with the Studio serving, the split serving, this Mac serving, and a route mounting; every surface
names the same model, Mac and state; the chip moves the model to the other Mac in ≤ 2 clicks.

## Confidence
High on the derivation and the removals. LOWER on the chip menu's Run actions: extracting PlacementCard's
switch (stopForSwitch/startWay/startSplitFor, ~200 lines with Link/provision/discovery) into a shared hook
without changing its behaviour — it needs its 25 card tests to stay green unchanged, and R5 re-run after.
