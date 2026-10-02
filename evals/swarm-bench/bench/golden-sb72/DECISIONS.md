# Decisions

The three corners the spec leaves unstated, decided and shipped.

## D1

A streamed mutation of a brushed record drops it from the brush set — the row the user
selected no longer holds the state they selected, so the selection is cleared for that
record rather than silently carrying a different meaning forward. The dim lifts when the
set empties.

## D2

A rejected draft is terminal: the four-eyes decision is final and the draft is locked
read-only (the API answers 409 to any further transition). To retry the payment, create a
fresh draft — reusing a rejected one would blur the audit trail.

## D3

Before the first sync completes, the table renders immediately in an empty state with a
progress note and the live sync status. Data appears as the initial sync lands — the
layout never flashes and the rest of the console is fully interactive from first paint.
