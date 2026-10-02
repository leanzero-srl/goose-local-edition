# SB7.2 — Meridian Payments Landscape

Build and finish the Meridian Payments Console in this workspace: a payments product with a 3D
field, not a visual demo. `SB7-CONTRACT.md` is the behavioural contract; `VISUAL-CONTRACT.md`
amends its 3D field (tower geometry, default camera, inspection, animation) and wins where they
differ. `STARTER.md` describes the editable starter already present. Vendor documentation
{DOCS_URL}; base {BASE_URL}; API key {API_KEY}. Backend: Python standard library; frontend:
self-contained HTML/CSS/JavaScript and raw WebGL; nothing to install. No reference application
is provided.

You have a budget of 150 model calls. The harness stops the session at the budget and scores
whatever exists; plan to implement, test, and finish well inside it — polishing beyond the
scored behaviours earns nothing.

## Definition of done

Done means each of these works against the real vendor; nothing else is scored:

1. Both services boot from the three commands, own their databases, and survive the seeded
   vendor outage and SIGKILLs (SB7 §1, §10).
2. The first sync loads all 12,288 payments within 120 s through the documented walk, faults
   and conditional re-syncs (§3 Sync).
3. The API answers with the exact shapes, Berlin-day buckets, error envelope and latency (§3 API).
4. Webhooks: signature check, idempotent version-ordered apply, atomic refund groups (§4).
5. Event ledger, transactional outbox, relay and idempotent notifier deliver exactly once (§3, §6).
6. Approval workflow: roles, durable states, one vendor payment per approval (§5).
7. The UI journeys work in a browser with a clean console: table, money, dates, sync, notes,
   feed, drafts (§7).
8. The 3D field: towers, colours, pixel-true heights, picking, camera and coast, labels, brush,
   SSE diffs, budgets and a truthful `vs7dbg` (§8 and VISUAL-CONTRACT).
9. Inspection, the update animation and the legible overview (VISUAL-CONTRACT).
10. `DECISIONS.md` documents D1–D3 as implemented (§9).
11. Consistency holds throughout and at quiescence (§10).

## Score bands

Tests earn the score; the 3D field (structure, legibility, inspection, interaction, animation,
rendering performance) carries about half of it. Visual conditions also cap it:

- No visible, data-backed 3D in the recorded browser state: maximum 0.599.
- Missing tower parts, wrong payment/currency mapping, or failing layout/height/scene-truth
  checks: maximum 0.699.
- Failing GPU picking, camera, coast, labels, brush, draw budget, incremental stream, inspector
  framing or overview legibility: maximum 0.799.
- Without BOTH verified event animation AND every concurrent-history and crash-recovery check
  passing: maximum 0.899.

Text and control readability earns points but never caps. The grader records screenshots and
a short clip of its own browser session; do not build a video exporter.

## Completion handoff

Work only with this workspace, the supplied documentation and the vendor API. When the
definition of done holds, stop your temporary test processes and return a final summary of
files, checks and remaining limitations; that response hands the artifact to the external
scorer, which starts after your session ends. Do not wait for or poll grader results.
