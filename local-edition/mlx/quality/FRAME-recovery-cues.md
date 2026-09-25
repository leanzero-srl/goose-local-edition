# FRAME — every second of a Link break has a cue (Q-47, Q-48, Q-49, Q-50; with Q-39, Q-40)

Owner, 2026-09-25: *"what happens during these seconds? do we have any proper state management so it's clear to
the user? it's a long time to wait 13s without having any clear visual cue."*

## The journey as recorded (harness/recovery.mjs, 1 sample/s, 3.0.36, break at 9.0 s)
| t | kill Link on Studio | relaunch Studio app |
|---|---|---|
| 0–9 s (own turn writing) | bar: "reading ANOTHER request's 140-token prompt — your message waits" (false) | same (false) |
| 9–13 s | story freezes mid-sentence, the false busy bar stays | same |
| 11.6 / 13.4 s | main knows: "timeout: no answer within 1500 ms" — the screen does not | same |
| 13.8–23.4 s | bar BLANK. No cue at all for ~10 s | 13.4–19.9 s blank; 18.9 main "engine returned 502"; 19.9 main falls to "single/off unreachable" (this Mac!) |
| ~22–23 s | error glued into the story, jargon, no Retry | same, plus bar "Loading … on Work's Mac Studio" (true) |
| after | busy bar blinks for goose's own follow-up calls | back to ready at 31.9 s |

## The cause
There is no state for "the Mac that serves chat stopped answering". Three reads each see it and each drop it:
- backend `observe_route`: peer unreachable over Link → `failed` (red, wrong), and the status call itself waits
  on the dead relay, so the renderer's read fails first;
- renderer `phaseOf`: a failed route read → `null` (no claim) → the bar hides;
- main's monitor: a failed route read → falls back to THIS Mac's engine ("single/off").
And the turn's error is plain text appended to the partial answer, so the notice path never sees it.

## The rule to install
One named state, **reconnecting**, meaning "a route is published, its Mac does not answer right now".
- backend (mlx-backend): `observe_route` → `reconnecting` when the peer's status op is refused over Link or the
  relay gives no answer (`failed` stays for "the peer answered and its engine failed"); the route status answers
  from what it has instead of outliving the reader.
- renderer (panel-surgeon): a route read error while the route is the engine, or route state `reconnecting` →
  readiness `reconnecting` → amber bar "Lost contact with Work's Mac Studio — reconnecting…" + spinner +
  "Run on this Mac instead"; chip dot amber; tray "Reconnecting to Work's Mac Studio"; main's snapshot keeps
  `remote` and never falls to the local engine while a route is published.
- transcript: a trailing `linkRelayFailed` / Link-peer error splits off the partial answer into a notice
  "Work's Mac Studio dropped off LeanZero Link mid-answer — the answer above stops there" + Retry; the raw
  text goes behind "Details".
- busy bar (Q-39/40/50): goose's own requests for this chat are never "another request"; "waits its turn" only
  when the engine reports a request waiting.

## Proof
Unit: derivation over (route read error, route `reconnecting`, route `failed`) → the amber named state; the
transcript split over the two recorded error texts; busy over own-turn + own-background + real waiting.
Live: re-record both scenarios with recovery.mjs on the installed build; PASS = every 1-s sample from the break
to "ready" has a bar that names the Mac and says reconnecting/loading, and the dropped turn ends in a notice with
Retry. Confidence: high on the UI state and the split; LOWER on the backend's "answers without outliving the
reader" — it depends on where the status call actually blocks (relay_get vs peer_op), which the surgeon must
measure, not assume.
