/* ============================================================================
 * viz.js piece — Streaming (SSE with byte accounting)  (shard: data-stream-render-pick)
 * Consumes GET /api/stream via onStreamMessage; applyBatch touches exactly the
 * minimal changed instance set S and uploads ≤ |S|·stride + 4096 bytes with no
 * realloc (>4096-byte bufferData).  Boot wires: new EventSource('/api/stream')
 * with .onmessage = onStreamMessage.
 * ==========================================================================*/

let vizLastBatch = -1;      // monotonic batch guard (no replay)

// SSE onmessage for GET /api/stream. Parses the atomic batch, calls applyBatch
// synchronously (well within the 250 ms apply budget). Never throws.
function onStreamMessage(event) {
  try {
    const msg = JSON.parse(event.data);
    if (!msg || !Array.isArray(msg.records)) return;
    const b = msg.batch | 0;
    if (b <= vizLastBatch) return;              // stale/replayed batch: ignore
    vizLastBatch = b;
    applyBatch({ batch: b, records: msg.records });
  } catch (err) { /* malformed message: ignore */ }
}

// Upload one instance's 24 bytes (stride) into the GL buffer — no realloc.
function vizUploadInstance(idx) {
  if (!vizGL || !instBuf) return;
  const gl = vizGL;
  gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
  gl.bufferSubData(gl.ARRAY_BUFFER, idx * 24, instanceGeom.subarray(idx * 6, idx * 6 + 6));
}

// Ensure JS arrays + GL instance buffers hold at least n slots. The initial
// allocation (and the practically-unreachable overflow growth) uses bufferData;
// normal creates just need the pre-allocated headroom and cost nothing here.
function vizEnsureCapacity(n) {
  if (n <= geomCap && instBuf) return;
  if (n > geomCap) {
    const oldCap = geomCap, oldGeom = instanceGeom;
    geomCap = Math.max(n, oldCap * 2);
    instanceGeom = new Float32Array(geomCap * 6);
    instanceGeom.set(oldGeom.subarray(0, oldCap * 6));
    idNumArr = new Float32Array(geomCap);
    for (let i = 0; i < geomCap; i++) idNumArr[i] = i + 1;   // idNum = n + 1 pre-filled
  }
  if (vizGL) vizUploadAllInstances();   // initial alloc / overflow realloc (preserves flags)
}

// Apply one SSE batch to exactly the minimal changed instance set S.
// A status flip touches 1 instance (color only); a create appends at n = count
// with r = current in-day count. Nothing re-ranks, nothing re-sorts.
function applyBatch(batch) {
  const S = new Set();
  const list = (batch && Array.isArray(batch.records)) ? batch.records : [];
  const wasEmpty = records.count === 0;

  for (const rec of list) {
    if (!rec || typeof rec.id !== 'string') continue;
    const idx = idToIndex.get(rec.id);

    if (idx !== undefined && idx < records.count) {
      // ---- mutation: only status/note/version change per vendor contract ---
      let h = instanceGeom[idx * 6 + 2];
      if (rec.status !== undefined && rec.status !== records.status[idx]) {
        records.status[idx] = rec.status;
        const c = topColorRGB(rec.status);
        instanceGeom[idx * 6 + 3] = c[0];
        instanceGeom[idx * 6 + 4] = c[1];
        instanceGeom[idx * 6 + 5] = c[2];
      }
      if (rec.amount_minor !== undefined && rec.amount_minor !== records.amount_minor[idx]) {
        // defensive: amounts never change per vendor docs; keep digest truthful if they do
        const oldH = h;
        records.amount_minor[idx] = rec.amount_minor;
        if (rec.currency !== undefined) records.currency[idx] = rec.currency;
        h = heightFor(rec.amount_minor, records.currency[idx]);
        instanceGeom[idx * 6 + 2] = h;
        const x = instanceGeom[idx * 6 + 0], z = instanceGeom[idx * 6 + 1];
        digestSums.Sh += h - oldH; digestSums.Sh2 += h * h - oldH * oldH;
        digestSums.Sxh += x * (h - oldH); digestSums.Szh += z * (h - oldH);
      }
      if (rec.version !== undefined) records.version[idx] = rec.version | 0;
      S.add(rec.id);
      vizUploadInstance(idx);                    // ≤ stride bytes, no realloc
    } else {
      // ---- create: append at n = count, r = current in-day count -----------
      const n = records.count;
      vizEnsureCapacity(n + 1);
      vizEnsureBasisLocked();
      const day = rec.day;
      const r = dayCounts[day] | 0;
      dayCounts[day] = r + 1;
      const d = vizDayIndex(day);
      const x = (d - (layoutBasis.D0 - 1) / 2) * DS_DELTA;
      const z = (r - (layoutBasis.R0 - 1) / 2) * DS_DELTA;
      const h = heightFor(rec.amount_minor, rec.currency);
      const c = topColorRGB(rec.status);
      records.id[n] = rec.id;
      records.amount_minor[n] = rec.amount_minor;
      records.currency[n] = rec.currency;
      records.status[n] = rec.status;
      records.created_at[n] = rec.created_at;
      records.day[n] = day;
      records.version[n] = rec.version | 0;
      instanceGeom[n * 6 + 0] = x;
      instanceGeom[n * 6 + 1] = z;
      instanceGeom[n * 6 + 2] = h;
      instanceGeom[n * 6 + 3] = c[0];
      instanceGeom[n * 6 + 4] = c[1];
      instanceGeom[n * 6 + 5] = c[2];
      records.count = n + 1;
      idToIndex.set(rec.id, n);
      digestSums.count++;                        // float64 incremental update
      digestSums.Sh += h; digestSums.Sh2 += h * h;
      digestSums.Sx += x; digestSums.Sz += z;
      digestSums.Sxh += x * h; digestSums.Szh += z * h;
      S.add(rec.id);
      vizUploadInstance(n);                      // idNum pre-filled at slot n; ≤ stride bytes
    }
  }

  if (S.size > 0) {
    if (wasEmpty && records.count > 0) setPanelState('ready');
    pickDirty = true;                            // scene changed -> next pick re-draws offscreen
    requestRender();                             // pixels + labels within one rAF (< 250 ms)
  }
  return S;
}
