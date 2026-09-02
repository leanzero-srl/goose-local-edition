// ============================================================================
// viz.js piece — Labels: 12 top-a_major DOM labels (assembly section: "Labels")
// Candidates: the 12 records with highest a_major = amount_minor/10^exp(currency)
//   (ties id ASC). a_major never changes after creation, so the candidate list
//   is cached and rebuilt only when records.count changes (streamed appends).
// Anchor: A = project(x, h, z) — instance top-center under the live camera.
// Eligible iff A non-null, inside the canvas, AND pickCore(A.sx, A.sy) returns
//   THIS instance (occlusion-culled through the render-pick shard's pick buffer).
// Geometry: DOM in #viz-labels, class viz-label, data-id, border-box exactly
//   110×18 CSS px single-line ellipsized, top-left at (A.sx+10, A.sy−9).
// Culling: priority order (a_major DESC, id ASC); show iff eligible AND its rect
//   intersects no already-shown rect (any overlap → cull; hidden/absent, never
//   nudged). Called every rendered frame (by renderFrame) and after
//   vs7dbg.setCamera — see README ASSUMES.
// ============================================================================

const LBL_COUNT = 12;               // candidate count
const LBL_W = 110, LBL_H = 18;      // border-box CSS px
const LBL_OX = 10, LBL_OY = -9;     // top-left offset from anchor A
const LBL_CUR_EXP = { EUR: 2, USD: 2, JPY: 0, KWD: 3 }; // minor-unit exponents

let lblHost = null;                 // #viz-labels container (lazy)
const lblPool = new Map();          // record id → DOM element (hidden when culled)
let lblCandidates = null;           // {n, idx: [stable indices in priority order]}

function lblAMajor(i) {
  const exp = LBL_CUR_EXP[records.currency[i]];
  return records.amount_minor[i] / Math.pow(10, exp === undefined ? 2 : exp);
}

function lblEnsureCandidates() {
  const n = records.count;
  if (lblCandidates && lblCandidates.n === n) return lblCandidates.idx;
  const idx = new Array(n);
  for (let i = 0; i < n; i++) idx[i] = i;
  idx.sort((a, b) => {
    const aa = lblAMajor(a), ab = lblAMajor(b);
    if (ab !== aa) return ab - aa;           // a_major DESC
    const ia = records.id[a], ib = records.id[b];
    return ia < ib ? -1 : ia > ib ? 1 : 0;   // ties id ASC
  });
  lblCandidates = { n: n, idx: idx.slice(0, LBL_COUNT) };
  return lblCandidates.idx;
}

/** Amount in its OWN currency: digits = minor units, decimals = exponent. */
function lblFormatAmount(minor, cur) {
  const exp = LBL_CUR_EXP[cur] === undefined ? 2 : LBL_CUR_EXP[cur];
  const a = Math.abs(minor);
  const scale = Math.pow(10, exp);
  let s = String(Math.floor(a / scale)).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  if (exp > 0) s += '.' + String(a % scale).padStart(exp, '0');
  const sym = cur === 'EUR' ? '€' : cur === 'USD' ? '$' : cur === 'JPY' ? '¥' : cur + ' ';
  return (minor < 0 ? '-' : '') + sym + s;
}

function lblHideAll() {
  for (const el of lblPool.values()) el.style.display = 'none';
}

function lblPlace(id, amountMinor, currency, x, y) {
  let el = lblPool.get(id);
  if (!el) {
    el = document.createElement('div');
    el.className = 'viz-label';
    el.setAttribute('data-id', id);
    el.style.position = 'absolute';
    el.style.width = LBL_W + 'px';
    el.style.height = LBL_H + 'px';
    el.style.boxSizing = 'border-box';
    el.style.whiteSpace = 'nowrap';
    el.style.overflow = 'hidden';
    el.style.textOverflow = 'ellipsis';
    lblHost.appendChild(el);
    lblPool.set(id, el);
  }
  const text = lblFormatAmount(amountMinor, currency);
  if (el.textContent !== text) el.textContent = text;
  el.style.left = x + 'px';
  el.style.top = y + 'px';
  el.style.display = 'block';
}

/**
 * updateLabels(): re-cull the 12 candidates in priority order under the live
 * camera. Must run with a live GL context (pickCore backs eligibility).
 */
function updateLabels() {
  if (typeof gl === 'undefined' || !gl) return; // 3D unavailable → no labels
  if (!lblHost) lblHost = document.getElementById('viz-labels');
  if (!lblHost) return;
  const cv = camGetCanvas();
  if (!cv) return;
  const W = cv.clientWidth, H = cv.clientHeight;
  if (!(W > 0) || !(H > 0)) { lblHideAll(); return; }

  const idx = lblEnsureCandidates();
  const shownRects = [];
  const shownIds = new Set();
  for (let c = 0; c < idx.length; c++) {
    const i = idx[c];
    const o = i * 6; // instanceGeom stride 6: [x, z, h, topR, topG, topB]
    const A = project(instanceGeom[o], instanceGeom[o + 2], instanceGeom[o + 1]); // (x, h, z)
    if (!A || A.sx < 0 || A.sx >= W || A.sy < 0 || A.sy >= H) continue; // anchor inside canvas
    const hit = pickCore(A.sx, A.sy); // occlusion through the pick buffer
    if (!hit || hit.index !== i) continue; // must return THIS instance
    const rx = A.sx + LBL_OX, ry = A.sy + LBL_OY;
    let clash = false;
    for (let s = 0; s < shownRects.length; s++) {
      const r = shownRects[s];
      const ox = Math.min(rx + LBL_W, r.x + LBL_W) - Math.max(rx, r.x);
      const oy = Math.min(ry + LBL_H, r.y + LBL_H) - Math.max(ry, r.y);
      if (ox > 0 && oy > 0) { clash = true; break; } // overlap ≥ any px → cull
    }
    if (clash) continue; // hidden or absent — never nudged
    shownRects.push({ x: rx, y: ry });
    shownIds.add(records.id[i]);
    lblPlace(records.id[i], records.amount_minor[i], records.currency[i], rx, ry);
  }
  for (const [id, el] of lblPool) if (!shownIds.has(id)) el.style.display = 'none';
}
