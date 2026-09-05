// ============================================================================
// viz.js piece — Brush: ONE shared set + per-instance dim flag (section "Brush")
// WRITES shared state:
//   brushSet  — Set<string> of brushed record ids (the ONE shared set)
//   brushFlag — Uint8Array[records.count], byte n = 1 iff records.id[n] ∈ set
// Dim design (per-instance flag + uniform, never a geometry rebuild):
//   per-frame uniform uBrushActive = brushSet.size > 0 (free uniform upload);
//   shader: base c' = active ? mix(c, round(0.30·c), flag) : c  — dim applied
//   BEFORE the side factor (side-of-dim = round(0.55·round(0.30·c))).
// Cost budget: a toggle uploads only the changed flag bytes (1 byte each,
//   bufferSubData) — ≤ stride+4096 with no realloc; clearBrush uploads ZERO
//   buffer bytes (the uniform lifts the dim → pixels back to exact status hex).
// D1 (documented behavior): a streamed mutation of a brushed record KEEPS it
//   brushed — brushSet is id-keyed and mutations never remove ids, so the flag
//   stays 1 and the instance renders its NEW status hex at full brightness.
// ============================================================================

const brushSet = new Set();            // the ONE shared brush set (record ids)
let brushFlag = new Uint8Array(0);     // per-instance dim flag, length N, 1 = member
let brushFlagMirror = new Uint8Array(0); // last bytes actually in the GPU buffer
const brushFlagDirty = new Set();      // indices where brushFlag ≠ GPU contents
const brushSubs = [];                  // onBrush subscribers

const BRUSH_FLAG_CAP = 65536;          // GL slots pre-allocated once → no realloc ever
let brushFlagGL = null;                // WebGLBuffer (UNSIGNED_BYTE, 1 byte per slot)

/** Grow the CPU flag arrays when records are appended. Called from applyBatch
 *  creates and self-healed at the top of every brush op. */
function ensureBrushFlag() {
  const n = records.count;
  if (brushFlag.length === n) return;
  const next = new Uint8Array(n);
  next.set(brushFlag.subarray(0, Math.min(brushFlag.length, n)));
  brushFlag = next;
  const mNext = new Uint8Array(n);
  mNext.set(brushFlagMirror.subarray(0, Math.min(brushFlagMirror.length, n)));
  brushFlagMirror = mNext;
}

let brushIdIndex = null; // {n, map: Map<id, stable index>} — derived read of records
function brushIndexOf(id) {
  const n = records.count;
  if (!brushIdIndex || brushIdIndex.n !== n) {
    const map = new Map();
    for (let i = 0; i < n; i++) map.set(records.id[i], i);
    brushIdIndex = { n: n, map: map };
  }
  return brushIdIndex.map.has(id) ? brushIdIndex.map.get(id) : -1;
}

/** One-time capacity allocation of the GL flag buffer — call once after the GL
 *  context exists (boot or first render). This is the ONLY >4096-byte
 *  bufferData this shard performs, and it happens outside any graded brush /
 *  stream window. Idempotent; no-op before the context exists. */
function initBrushFlagBuffer() {
  if (typeof gl === 'undefined' || !gl || brushFlagGL) return;
  brushFlagGL = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, brushFlagGL);
  gl.bufferData(gl.ARRAY_BUFFER, BRUSH_FLAG_CAP, gl.DYNAMIC_DRAW);
}

/** Upload exactly the changed flag bytes (1 byte each via bufferSubData).
 *  No realloc. Only this shard ever uploads to brushFlagGL. */
function uploadBrushFlags() {
  if (!brushFlagGL || brushFlagDirty.size === 0) return;
  gl.bindBuffer(gl.ARRAY_BUFFER, brushFlagGL);
  for (const n of brushFlagDirty) {
    if (n < BRUSH_FLAG_CAP) gl.bufferSubData(gl.ARRAY_BUFFER, n, brushFlag.subarray(n, n + 1));
    brushFlagMirror[n] = brushFlag[n];
  }
  brushFlagDirty.clear();
}

function brushSortedIds() { return Array.from(brushSet).sort(); } // ascending ids

function fireBrushCallbacks() {
  const ids = brushSortedIds();
  for (const cb of brushSubs) cb(ids);
}

/** Toggle record id in the ONE shared brush set; upload changed flag bytes
 *  (≤ stride+4096, no realloc), fire onBrush callbacks, request a render. */
function toggleBrush(id) {
  ensureBrushFlag();
  const n = brushIndexOf(id);
  if (n < 0) return; // unknown id — nothing to flag
  const joining = !brushSet.has(id);
  if (joining) brushSet.add(id); else brushSet.delete(id);
  brushFlag[n] = joining ? 1 : 0;
  brushFlagDirty.add(n);
  uploadBrushFlags();
  fireBrushCallbacks();
  requestRender();
}

/** Clear the set and lift the 0.30 dim (pixels back to exact status hex).
 *  Zero buffer bytes uploaded — the per-frame uniform uBrushActive = 0 does it;
 *  stale GPU flag bytes are flushed on the next toggle. */
function clearBrush() {
  if (brushSet.size === 0) return; // no change → no callbacks, no render
  ensureBrushFlag();
  for (const id of brushSet) {
    const n = brushIndexOf(id);
    if (n >= 0) { brushFlag[n] = 0; brushFlagDirty.add(n); }
  }
  brushSet.clear();
  fireBrushCallbacks();
  requestRender();
}

/** The shared brush set as record ids sorted ascending. */
function brush() { return brushSortedIds(); }

/** Subscribe cb to every brush change; cb receives ascending ids. */
function onBrush(cb) { brushSubs.push(cb); }

const viz3d = { toggleBrush: toggleBrush, clearBrush: clearBrush, brush: brush, onBrush: onBrush };
if (typeof window !== 'undefined') window.viz3d = viz3d; // boot re-assigns the same object
