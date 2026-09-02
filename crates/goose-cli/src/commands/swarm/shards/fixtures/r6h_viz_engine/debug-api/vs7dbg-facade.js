/* vs7dbg facade — synchronous, truthful graded reads over the other shards.
 *
 * Owns window.vs7dbg (the 8 graded methods) and nothing else. It READS shared
 * state (layoutBasis, digestSums, brushSet, frames) and DELEGATES to sibling
 * functions (getCamera, setCameraCore, renderFrame, updateLabels, pickCore,
 * pickPixelCore). It never writes shared state and never throws: every method
 * answers from live scene truth at call time.
 *
 * Assembly note: this section precedes the Boot section; boot() re-assigns the
 * same _vs7Facade object to window.vs7dbg (idempotent).
 */

/* Round a float64 sum to 4 decimals (the graded sceneDigest precision);
 * normalizes -0 to 0 so JSON output never shows "-0". */
function _dbgRound4(v) {
  const r = Math.round((Number(v) || 0) * 1e4) / 1e4;
  return r === 0 ? 0 : r;
}

const _vs7Facade = {
  /* Locked layout basis {d0: 'YYYY-MM-DD', D0: 96, R0: max in-day count at
   * load}; null/absent before the first non-empty render locks it. */
  layout() {
    const b = (typeof layoutBasis !== 'undefined') ? layoutBasis : null;
    if (!b) return null;
    return { d0: b.d0, D0: b.D0, R0: b.R0 };
  },

  /* Float64 sums over ALL current records (h, h^2, x, z, x*h, z*h) rounded to
   * 4 decimals + brushedCount derived from the shared brush set at read time.
   * Must agree with rendered pixels — it is a pure read of digestSums. */
  sceneDigest() {
    const d = (typeof digestSums !== 'undefined' && digestSums) ? digestSums : null;
    const bs = (typeof brushSet !== 'undefined' && brushSet) ? brushSet.size : 0;
    return {
      count: d ? d.count : 0,
      Sh: _dbgRound4(d ? d.Sh : 0),
      Sh2: _dbgRound4(d ? d.Sh2 : 0),
      Sx: _dbgRound4(d ? d.Sx : 0),
      Sz: _dbgRound4(d ? d.Sz : 0),
      Sxh: _dbgRound4(d ? d.Sxh : 0),
      Szh: _dbgRound4(d ? d.Szh : 0),
      brushedCount: bs,
    };
  },

  /* Live camera state in degrees and deg/s — the same values the projection
   * used for the pixels on canvas (delegates to the camera shard). */
  camera() {
    return getCamera();
  },

  /* Apply pitch/distance clamps + cancel any coast (setCameraCore), then
   * render ONE frame synchronously and re-cull labels — all before this call
   * returns, so pixels, frames() and the DOM labels agree immediately. */
  setCamera(yaw, pitch, distance) {
    setCameraCore(yaw, pitch, distance); // clamps [5,85]/[15,340], cancels coast
    renderFrame();                       // synchronous frame (counts in frames())
    if (typeof updateLabels === 'function') updateLabels(); // re-cull; idempotent
  },

  /* GPU-truth pick at device pixel (round(sx*DPR), Hdev-1-round(sy*DPR));
   * index = stable arrival index n; null on background. 0 default-FBO draws. */
  pick(sx, sy) {
    return pickCore(sx, sy);
  },

  /* Raw [r,g,b,a] bytes from the pick FBO at the same device-pixel mapping;
   * decode(r + 256g + 65536b) - 1 must equal pick's index. */
  pickPixel(sx, sy) {
    return pickPixelCore(sx, sy);
  },

  /* The ONE shared brush set as record ids sorted ascending — identical to
   * viz3d.brush() since both read the same brushSet. */
  brush() {
    const bs = (typeof brushSet !== 'undefined') ? brushSet : null;
    if (!bs) return [];
    return Array.from(bs).sort();
  },

  /* Total frames rendered since load — the render shard's monotonic counter,
   * read as-is so it agrees with the wrapper's counted default-FBO draws. */
  frames() {
    return (typeof frames !== 'undefined' && typeof frames === 'number') ? frames : 0;
  },
};

/* Expose immediately so vs7dbg is synchronous and available from t=0 (layout()
 * answers null, sceneDigest() zeros, camera() defaults before data lands);
 * boot() re-assigns the same object per the assembly order. */
if (typeof window !== 'undefined') {
  window.vs7dbg = _vs7Facade;
}
