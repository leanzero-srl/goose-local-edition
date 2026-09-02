/* Boot — assembly entry point for web/viz.js (owned by the debug-api shard).
 *
 * Frozen order: initViz -> loadRecords -> SSE connect (onStreamMessage) ->
 * bindCameraInput + bindClickInput -> assign window.viz3d and window.vs7dbg ->
 * first render. Never throws; the console stays clean — every failure path is
 * already surfaced by a panel state (#viz-error / #viz-empty / 3D-unavailable)
 * set by the shards that own those states.
 */

let _dbgBooted = false; // single-shot: a second boot() call is a no-op

async function boot() {
  if (_dbgBooted) return;
  _dbgBooted = true;
  try {
    // 1. GL context + DPR-sized backing store on the main thread. initViz()
    //    returns false and shows the visible '3D unavailable' notice itself
    //    when both webgl/webgl2 are null — everything else keeps working.
    const glOk = initViz();
    if (glOk && typeof initBrushFlagBuffer === 'function') {
      initBrushFlagBuffer(); // one-time flag-buffer capacity, outside any graded window
    }

    // 2. Initial columnar dataset (one fetch of /api/viz/records). Resolves on
    //    failure too — it flips #viz-error itself and local state stays empty.
    await loadRecords();

    // 3. Live diffs: one EventSource on GET /api/stream; each message is one
    //    atomic batch handled by the data-stream shard's onStreamMessage.
    if (typeof EventSource !== 'undefined') {
      const es = new EventSource('/api/stream');
      es.onmessage = onStreamMessage;
    }

    // 4. Input wiring, both on the same canvas: orbit camera (drag/wheel/
    //    dblclick/coast) and click-to-brush (5 px / 300 ms).
    const canvas = document.getElementById('viz3d');
    if (canvas) {
      bindCameraInput(canvas);
      bindClickInput(canvas);
    }

    // 5. Public facades. The brush shard already exposed viz3d at top level;
    //    boot re-assigns the SAME object per the assembly contract.
    if (typeof viz3d !== 'undefined') window.viz3d = viz3d;
    if (typeof _vs7Facade !== 'undefined') window.vs7dbg = _vs7Facade;

    // 6. First render — coalesces with loadRecords' own requestRender() when
    //    data arrived, so at most one frame is drawn for the initial scene.
    requestRender();
  } catch (_err) {
    /* boot never throws and logs nothing: panel states already show what
     * failed, and a dead backend must keep the rest of the page working. */
  }
}

/* Self-start: run once the DOM is ready (script may load before or after it). */
(function startBoot() {
  const run = function () { boot().catch(function () {}); };
  if (typeof document !== 'undefined' && document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', run, { once: true });
  } else {
    run();
  }
})();
