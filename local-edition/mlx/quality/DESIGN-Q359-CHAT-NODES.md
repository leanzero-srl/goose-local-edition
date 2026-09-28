# Q-359 — one node or several on a chat (design, 2026-09-28)

Owner, 2026-09-28: "have one running and then add more nodes if I want to a chat or have each chat with their own
node provided the node is available … the logic around detecting available nodes … should continue to stay, here I
am asking only as augmentation to see if the user wants to add one or X nodes on a session."

## Today (verified at d29c0bb19)
- A chat runs on ONE node (`node:<id>` from the chip's "Run this chat on", NodesChipMenu.tsx:149-160 →
  useRunChatOn.ts; router one-node chain swarm_router.rs:1836-1855), or on a saved STRATEGY (`strategy:<id>`:
  the Chat chain under failover/overflow/share, swarm_router.rs:1856-1887, resolve.rs:115-125; a chat sticks to one
  node under share, resolve.rs:180-197), whose DELEGATES run on `strategy:<id>@build` (summon.rs:1621,
  :1923-1935). Parallel: tool calls in one message run together (agent.rs:2558); background delegates up to
  GOOSE_MAX_BACKGROUND_TASKS=5 (summon.rs:423-427, :1791-1917). `swarm` (Any node, Auto): pool + live remote
  route, most-free-slots then sticky (swarm_router.rs:945-1013, :249-268).
- ONE Mac model at a time for this Mac's goose, across both Macs: route to a peer up → own engine refused
  (`sidecar_routed_away`, swarm_router.rs:272-285, :509); residency is one answer (residency.rs:155-206); loader
  stops every serving way first (switch.rs:223-232, nodes_loader.rs:717-751); strategies sharing two Mac models are
  refused (nodes/mod.rs:764-793, strategyFit.ts:15-16).
- Availability detection (KEEP, one derivation): nodeGlance.blockerOf (nodeGlance.ts:298-351), residency + build
  holder (residency.rs:88-137, nodeGlance.ts:464-465), router turn-time probes (swarm_router.rs:508-535, :592-652),
  build fleet discovery (fleet_order.rs).

## Meaning of "several nodes on one chat"
(a) MAIN: the chat answers on its LEAD (first node); its delegates share every node of the set, equal weights,
sticky per delegate (prompt cache survives) = the existing Build role under `share` (resolve.rs:12-15), busy nodes
skipped. (b) Overflow not offered (share already skips full nodes; overflow piles 8 on the lead,
MAX_CONCURRENT_REQUESTS engine.rs:145). (c) Failover = opt-in switch "If <lead> can't run, answer on the next node",
default OFF; every move announced by the existing `nodes.fellBack` turn line. (d) Role split: not for chats.

## Control
Chip menu top section:
```
Run this chat on
  THIS CHAT'S NODES
  [27B · Work's Mac Studio · Serving · answers]  [Flash · this Mac · Not loaded ×]  [Claude Sonnet · Ready ×]
  Delegates share these nodes. The chat answers on 27B · Work's Mac Studio.
  [ ] If 27B · Work's Mac Studio can't run, answer on the next node
  + Add a node to this chat…        Save as a strategy…
  ─ STRATEGIES … NODES … (today's list; picking one = "only this", clears the set)
```
"Add a node to this chat" = OverlayDialog mounting WithMacs + useNodeFacts + nodeGlance like the Nodes page (the
dropdown cannot mount useMacs — design §13 item 9). New pure `chatNodeAvailability(glance, lead, set, residency)` →
addable / not addable-with-reason; a non-addable node is shown at full strength with its reason and no Add button
(never greyed).

## Mapping (no second routing copy)
Chat-owned strategy: `NodeStrategy.chat: Option<sessionId>`, stored in nodes.strategies via ONE door, new
`nodes/setChatNodes {session, nodes[], answerOnNext}` that builds the strategy in Rust and sets the chat's model to
`strategy:<id>` in the same call. Chat role `[lead]` failover load-if-not-loaded (+ rest of the set if answerOnNext);
Build role = whole set, share, weight 1 each. Router/resolve/summon/@build/context-window/loader/served/strategyFit
unchanged; router only labels a chat-owned strategy "this chat's nodes". A chat strategy cannot be "New chats start
on". Save as a strategy names it and clears `chat`. Strategies tab lists no chat-owned sets but says "2 chats have
their own node sets". Remove node gains "Also take it out of 2 chats' node sets" (Q-259 pattern). Deleting a chat
removes its set (`nodes::forget_chat`). Adding to a chat on a named strategy makes a chat copy: "Adds to this chat
only; Everyday stays as it is".

## Words (Add dialog / chip)
Serving → "Serving" · Not loaded → "Not loaded · starts in about {duration} · median of {n} loads" / "First start
not measured yet" · Loading → "Loading · {phase}" (loadPhaseWord) · Waiting → nodes.turnWaiting · same Mac as lead →
"Runs on {mac}, where {lead} answers this chat. A Mac runs one model at a time for goose" · lead is a split →
"{lead} runs across both Macs, so no Mac is free for another model" · in use by another chat → "{mac} is answering
chat "{chat}" on {node}. Adding it stops that after its answer" (addable) · before C3 → "Can't run beside {lead}
yet: your Macs serve goose one model at a time" · other blockers → nodeGlance's own lines.
Delegate card (ToolCallWithResponse via nodes/servedLast on its subagent session): "on Flash · this Mac" /
"Loading 27B · Work's Mac Studio for this delegate: Loading weights" / "on Claude Sonnet: 27B · Work's Mac Studio
can't run (not connected)". Failures: every node out → loud refusal naming each; lead can't run, switch off → turn
ends offering [Answer on {next} for now]; a removed node → the chat is told, never silently Auto.

## Slices
| slice | owns | confidence |
|---|---|---|
| C0 contract | nodes/mod.rs (chat field, setChatNodes, forget_chat, validation), nodes/acp.rs, custom_requests.rs, custom_dispatch.rs, nodes.fixture.json, ui/desktop/src/acp/nodes.ts, components/nodes/model.ts | high |
| C1 router label | swarm_router.rs (chain_plan label only) | high |
| C2 desktop | NodesChipMenu.tsx, ModelsBottomBar.tsx, new AddChatNodeDialog.tsx + chatNodeAvailability.ts, ToolCallWithResponse.tsx, RemoveConfirmDialog.tsx, StrategiesTab.tsx | medium |
| C4 sibling holds | nodes_loader/holds.rs — open_replies skips same-root replies (holds.rs:370), sync delegates share the parent's root → siblings invisible to each other's switches | medium-high |
| C3 per-Mac rule | switch.rs, PlacementCard.tsx, residency.rs (set of serving ways), swarm_router.rs (drop sidecar_routed_away), two_mlx_ways → share-a-Mac, strategyFit.ts, mlx_serving_intent.rs, mlx_placement.rs, glance "Also serving" | LOW-MEDIUM — SCHEDULED waits on: (1) this Mac's decode tok/s while relaying the Studio's stream, (2) wall time 4 delegates on 2 Macs vs 1; plus proof of how the Studio's mount op treats its own open replies |

Order: C0 → C1 + C2 + C4 → C3 after its measurement. Before C3 it already works for a Mac model + cloud nodes.
Live: J7-lite (27B split + Claude Sonnet, 4 async delegates alternate, served records name them); J7a–d after C3.
Research: exo (instances per device), LM Studio LM Link, Open WebUI multi-Ollama random selection (#788, #22345 —
avoid), OpenRouter ordered fallbacks.
