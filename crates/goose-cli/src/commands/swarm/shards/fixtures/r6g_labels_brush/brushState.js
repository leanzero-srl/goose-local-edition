// ── web/viz.js · section 8 (Brush) · piece: shared brush state ──────────────
// The ONE brush set of record ids, shared with app.js through window.vs7
// (assigned in the brushApi piece). dimFlags mirrors membership per instance
// (1 = member); the scene shard's writeInstance(n)/appendInstance() read
// dimFlags[n] as the flag byte (or equivalently brushSet.has(rec.ids[n])) —
// both are kept identical here, so a streamed status flip of a brushed record
// re-uploads it still flagged 1 (D1: keeps it brushed; n and id never change).

const brushSet = new Set(); // record ids — single source of truth
let uBrushActive = 0;       // derived uniform: 1 iff brushSet.size > 0 (shader dim = uBrushActive * (1 - flag))
const dimFlags = new Uint8Array(65536); // per-instance dim flag, headroom far beyond N=12,288
const brushCallbacks = [];  // onBrushChange listeners

function idToIndex(id) { return rec.ids.indexOf(id); } // n = stable arrival index; ids never re-sort

function brushIdsAsc() { return [...brushSet].sort(); } // ascending by id (vs7dbg.brush() shape)

function updateBrushCount() {
  const el = document.getElementById('brush-count');
  if (el) el.textContent = String(brushSet.size);
}

function notifyBrush(idsAsc, clickedId) {
  for (const cb of brushCallbacks) {
    try { cb(idsAsc, clickedId); } catch (err) { console.error('onBrushChange callback', err); }
  }
}
