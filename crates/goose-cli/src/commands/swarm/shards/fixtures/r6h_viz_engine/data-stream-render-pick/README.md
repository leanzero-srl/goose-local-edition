PROVIDES: loadRecords(): Promise<void> — fetch GET /api/viz/records once, build records/instanceGeom/digestSums, lock {d0,D0:96,R0} on first non-empty response, requestRender; failure → setPanelState('error')
PROVIDES: applyBatch(batch): Set<string> — minimal-diff SSE apply (status flip = 1 instance color; create appends at n=count with r=current in-day count); uploads ≤ |S|·24 bytes via bufferSubData, no realloc; returns S
PROVIDES: heightFor(amountMinor, currency): number — clamp(0.9+0.55·log10(a/10^exp), 0.2, 4.2), exp EUR2/USD2/JPY0/KWD3
PROVIDES: topColorRGB(status): [r,g,b] — exact hex normalized 0..1 (settled 5,150,105 / pending 217,119,6 / refunded 124,58,237 / failed 185,28,28)
PROVIDES: onStreamMessage(event): void — SSE onmessage; JSON parse, monotonic-batch guard, calls applyBatch synchronously
PROVIDES: initViz(): boolean — webgl2-then-webgl {antialias:false,alpha:false} on #viz3d main thread, DPR backing store, programs/buffers/pick-FBO; false + '3D unavailable' notice if both null; never throws
PROVIDES: renderFrame(): void — 1 default-FBO instanced draw of all N columns (≤8), bg #101828, depth ON, increments frames, marks pickDirty, calls updateLabels() if defined
PROVIDES: requestRender(): void — coalesces to next rAF; no idle loop (0 draws at rest over 500 ms)
PROVIDES: pickCore(sx,sy): {id,index}|null — device px (round(sx·DPR), Hdev−1−round(sy·DPR)); first call after invalidation = 1 offscreen draw + 1 readPixels (≤4 draws/refresh), later calls from CPU cache; 0 default-FBO draws
PROVIDES: pickPixelCore(sx,sy): [r,g,b,a] — raw pick-FBO bytes, same mapping; decode(r+256g+65536b)−1 == pick index
PROVIDES: setPanelState('ready'|'empty'|'error'|'unavailable'): void — flips #viz-empty/#viz-error, creates+shows #viz-unavailable notice when needed
PROVIDES: bindClickInput(canvas): void — pointerup ≤5 px & ≤300 ms of pointerdown = click; instance → viz3d.toggleBrush(id), background → viz3d.clearBrush()
ASSUMES: sibling declares shared `camera` {yaw,pitch,distance,vyaw,vpitch} (degrees/deg/s) — I read it in renderFrame/vizEnsurePickFresh with VIZ_CAM_DEFAULT fallback
ASSUMES: sibling declares `brushSet` (Set<string>) and `brushFlag` (Uint8Array[N]) — I read brushSet.size for the uBrush uniform; brush shard uploads flags into my GL buffer via top-level `vizGL` + `flagBuf` (bufferSubData, ≤ stride+4096, no realloc); I preserve brushFlag contents on buffer growth
ASSUMES: boot (debug-api) wires `new EventSource('/api/stream')` with `.onmessage = onStreamMessage` after loadRecords; calls initViz → loadRecords → bindCameraInput + bindClickInput in that order
ASSUMES: index.html provides #viz3d, #viz-empty, #viz-error (I create #viz-unavailable myself if absent); sibling's updateLabels() is callable at runtime (renderFrame guards with typeof)
ASSUMES: assembly concatenates pieces in order data-stream.js → streaming.js → [camera] → render-core.js → pick-buffer.js; all my cross-references are runtime (hoisted functions / top-level lets), so order within that range is safe
UNFINISHED: none — overflow growth (>16384 streamed creates beyond N) falls back to a one-time bufferData realloc, which the fixture's headroom guarantees never triggers in grading
CHECKED_WITH: node --check on each piece (data-stream.js, streaming.js, render-core.js, pick-buffer.js) — all printed OK, no errors
WRITES: records — {count:int, id:string[], amount_minor:int[], currency:string[], status:string[], created_at:string[], day:string[] (server Berlin date, never recomputed), version:int[]} all length N in stable-arrival order
WRITES: instanceGeom — Float32Array, stride 6 floats per stable index n: [x, z, h, topR, topG, topB] (colors 0..1), length geomCap·6 (geomCap = N+16384 headroom); x=(d−(D0−1)/2)·1.2, z=(r−(R0−1)/2)·1.2
WRITES: layoutBasis — {d0:'YYYY-MM-DD' (first day present), D0:96, R0:max in-day count at load}; null until first non-empty response, never changes after
WRITES: digestSums — {count:int, Sh, Sh2, Sx, Sz, Sxh, Szh} float64 sums over ALL current records (brushedCount derived by readers from brushSet)
WRITES: frames — int, total rendered frames since load, monotonic, +1 per renderFrame
