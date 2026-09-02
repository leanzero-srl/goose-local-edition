/* ============================================================================
 * viz.js piece — Data → scene  (shard: data-stream-render-pick)
 * Fetches /api/viz/records once, builds and maintains the per-instance scene
 * state: stable arrival index n, locked layout basis {d0, D0, R0}, x/z/h
 * geometry with currency-exponent heights, exact status colors, float64
 * digest sums.  Sole writer of: records, instanceGeom, layoutBasis,
 * digestSums (frames is written by the render piece, same shard).
 * ==========================================================================*/

/* ---- constants (this shard's; uniquely prefixed so they cannot collide) --- */
const DS_EXP = { EUR: 2, USD: 2, JPY: 0, KWD: 3 };                 // minor-unit exponents
const DS_STATUS_RGB = {                                            // exact status hexes (0..255)
  settled:  [5, 150, 105],     // #059669
  pending:  [217, 119, 6],     // #D97706
  refunded: [124, 58, 237],    // #7C3AED
  failed:   [185, 28, 28]      // #B91C1C
};
const DS_DELTA = 1.2;          // cell pitch, world units
const DS_D0 = 96;              // layout span (fixed)
const DS_H_MIN = 0.2, DS_H_MAX = 4.2;
const DS_BG = [16 / 255, 24 / 255, 40 / 255];   // #101828 background

/* ---- shared state (I am the ONLY writer of these four) -------------------- */
// Columnar full collection, all arrays length N in stable-arrival order.
let records = { count: 0, id: [], amount_minor: [], currency: [], status: [], created_at: [], day: [], version: [] };
// Float32Array, stride 6 floats per stable index n: [x, z, h, topR, topG, topB]
// (colors normalized 0..1), length geomCap*6; x/z from Δ=1.2 pitch on the locked basis.
let instanceGeom = new Float32Array(0);
// {d0: 'YYYY-MM-DD', D0: 96, R0: max in-day count at load}; locked on first
// non-empty /api/viz/records render; never changes after.
let layoutBasis = null;
// float64 sums over ALL current records (brushedCount is derived by readers).
let digestSums = { count: 0, Sh: 0, Sh2: 0, Sx: 0, Sz: 0, Sxh: 0, Szh: 0 };

/* ---- internal scene bookkeeping (not shared state) ------------------------ */
let dayCounts = {};            // Berlin day -> count so far (gives in-day rank of a create)
let idToIndex = new Map();     // record id -> stable arrival index n
let geomCap = 0;               // capacity (slots) backing instanceGeom/idNumArr/flag buffer
let idNumArr = new Float32Array(0);   // idNumArr[n] = n + 1 (pick encoding), pre-filled

/* ---- per-instance transform helpers --------------------------------------- */

// h = clamp(0.9 + 0.55*log10(amount_minor / 10^exp(currency)), 0.2, 4.2)
function heightFor(amountMinor, currency) {
  const exp = Object.prototype.hasOwnProperty.call(DS_EXP, currency) ? DS_EXP[currency] : 2;
  const aMajor = amountMinor / Math.pow(10, exp);
  let h = 0.9 + 0.55 * Math.log10(aMajor);
  if (h < DS_H_MIN) h = DS_H_MIN;
  else if (h > DS_H_MAX) h = DS_H_MAX;
  return h;
}

// Exact status hex as normalized 0..1 floats [r, g, b].
function topColorRGB(status) {
  const c = DS_STATUS_RGB[status] || [0, 0, 0];
  return [c[0] / 255, c[1] / 255, c[2] / 255];
}

// d = Berlin day − d0 in calendar days (exact; both are YYYY-MM-DD date strings).
function vizDayIndex(day) {
  if (!layoutBasis || !day) return 0;
  if (day === layoutBasis.d0) return 0;
  const a = Date.parse(layoutBasis.d0 + 'T00:00:00Z');
  const b = Date.parse(day + 'T00:00:00Z');
  return Math.round((b - a) / 86400000);
}

// Safety net for the degenerate case where the initial fetch was empty and
// streamed creates arrive before any non-empty /api/viz/records response.
function vizEnsureBasisLocked() {
  if (layoutBasis || records.count === 0) return;
  let d0 = records.day[0];
  const counts = {};
  for (let i = 0; i < records.count; i++) {
    const d = records.day[i];
    if (d < d0) d0 = d;                       // YYYY-MM-DD: lexicographic == chronological
    counts[d] = (counts[d] | 0) + 1;
  }
  let R0 = 1;
  for (const k in counts) if (counts[k] > R0) R0 = counts[k];
  layoutBasis = { d0: d0, D0: DS_D0, R0: R0 };
}

// Full float64 recompute of the digest over ALL current records.
function vizRebuildDigestSums() {
  const N = records.count;
  let Sh = 0, Sh2 = 0, Sx = 0, Sz = 0, Sxh = 0, Szh = 0;
  if (layoutBasis) {
    for (let i = 0; i < N; i++) {
      const x = instanceGeom[i * 6 + 0];   // float32-stored values; error << tolerance
      const z = instanceGeom[i * 6 + 1];
      const h = instanceGeom[i * 6 + 2];
      Sh += h; Sh2 += h * h; Sx += x; Sz += z; Sxh += x * h; Szh += z * h;
    }
  }
  digestSums = { count: N, Sh: Sh, Sh2: Sh2, Sx: Sx, Sz: Sz, Sxh: Sxh, Szh: Szh };
}

/* ---- initial scene build (one fetch) -------------------------------------- */

function vizBuildScene(data) {
  const N = Math.max(0, data.count | 0);
  records = {
    count: N,
    id:           (data.id || []).slice(0, N),
    amount_minor: (data.amount_minor || []).slice(0, N),
    currency:     (data.currency || []).slice(0, N),
    status:       (data.status || []).slice(0, N),
    created_at:   (data.created_at || []).slice(0, N),
    day:          (data.day || []).slice(0, N),
    version:      (data.version || []).slice(0, N)
  };

  // per-day counts -> in-day rank (serve order is (created_at ASC, id ASC), so the
  // position within a day in serve order IS the load-time rank) and R0.
  dayCounts = {};
  let R0 = 1;
  for (let i = 0; i < N; i++) {
    const d = records.day[i];
    dayCounts[d] = (dayCounts[d] | 0) + 1;
    if (dayCounts[d] > R0) R0 = dayCounts[d];
  }

  // Lock the layout basis on the first non-empty response. Never changes after.
  if (N > 0 && !layoutBasis) {
    layoutBasis = { d0: records.day[0], D0: DS_D0, R0: R0 };
  }

  idToIndex = new Map();
  geomCap = Math.max(N + 16384, 1024);          // headroom so streamed creates never realloc
  instanceGeom = new Float32Array(geomCap * 6);
  idNumArr = new Float32Array(geomCap);
  for (let i = 0; i < geomCap; i++) idNumArr[i] = i + 1;

  const halfD = (layoutBasis ? layoutBasis.D0 : DS_D0 - 1) / 2;
  const halfR = (layoutBasis ? layoutBasis.R0 : 1 - 1) / 2;
  const rankSeen = {};
  let Sh = 0, Sh2 = 0, Sx = 0, Sz = 0, Sxh = 0, Szh = 0;
  for (let i = 0; i < N; i++) {
    const day = records.day[i];
    const r = rankSeen[day] | 0;                // in-day rank at load (0-based)
    rankSeen[day] = r + 1;
    const d = vizDayIndex(day);
    const x = (d - halfD) * DS_DELTA;
    const z = (r - halfR) * DS_DELTA;
    const h = heightFor(records.amount_minor[i], records.currency[i]);
    const c = topColorRGB(records.status[i]);
    instanceGeom[i * 6 + 0] = x;
    instanceGeom[i * 6 + 1] = z;
    instanceGeom[i * 6 + 2] = h;
    instanceGeom[i * 6 + 3] = c[0];
    instanceGeom[i * 6 + 4] = c[1];
    instanceGeom[i * 6 + 5] = c[2];
    idToIndex.set(records.id[i], i);
    Sh += h; Sh2 += h * h; Sx += x; Sz += z; Sxh += x * h; Szh += z * h;   // float64
  }
  digestSums = { count: N, Sh: Sh, Sh2: Sh2, Sx: Sx, Sz: Sz, Sxh: Sxh, Szh: Szh };

  vizEnsureCapacity(N);                          // allocate/refresh GL instance buffers (no-op if none)
}

// GET /api/viz/records once (columnar). Builds scene state, locks the basis on
// the first non-empty response, requests a render; failure flips #viz-error.
async function loadRecords() {
  try {
    const res = await fetch('/api/viz/records', { headers: { 'Accept': 'application/json' } });
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const data = await res.json();
    vizBuildScene(data);
    setPanelState(records.count > 0 ? 'ready' : 'empty');
    requestRender();
  } catch (err) {
    setPanelState('error');
  }
}
