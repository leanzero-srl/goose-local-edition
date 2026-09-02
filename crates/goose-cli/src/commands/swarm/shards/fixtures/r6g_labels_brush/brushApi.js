// ── web/viz.js · section 8 (Brush) · piece: brush API + window.vs7 bridge ───
// toggle/clear upload at most INSTANCE_STRIDE bytes per touched instance via
// writeInstance (scene shard) — no realloc, never a geometry rebuild. The dim
// is the per-instance flag plus the uBrushActive uniform; dropping the uniform
// to 0 when the set empties costs no geometry upload.

function toggleBrush(id) {
  const n = idToIndex(id);
  if (n < 0) return; // unknown id: no instance exists, nothing to flip
  if (brushSet.has(id)) {
    brushSet.delete(id);
    dimFlags[n] = 0;
  } else {
    brushSet.add(id);
    dimFlags[n] = 1;
  }
  uBrushActive = brushSet.size > 0 ? 1 : 0; // drops to 0 when the set becomes empty (no geometry upload)
  writeInstance(n);   // flip ONLY this instance's flag byte: <= INSTANCE_STRIDE bytes, no realloc
  updateBrushCount(); // #brush-count always shows the size
  requestRender();    // demand render — dim/undim visible on the next frame
  notifyBrush(brushIdsAsc(), id);
}

function clearBrush() {
  const members = [];
  for (const id of brushSet) {
    const n = idToIndex(id);
    if (n >= 0) members.push(n);
  }
  brushSet.clear();
  uBrushActive = 0; // pixels back to full status hex
  for (const n of members) { dimFlags[n] = 0; writeInstance(n); } // zero member flags, no realloc
  updateBrushCount();
  requestRender();
  notifyBrush([], null);
}

function onBrushChange(cb) {
  brushCallbacks.push(cb); // fired after every change with (idsAsc, clickedId | null)
}

// The single brush bridge shared with app.js — assigned at load.
window.vs7 = { toggleBrush, onBrushChange };
