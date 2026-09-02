// ── web/viz.js · section 7 (Labels) · piece: DOM label sync + culling ───────
// Called at the end of every renderFrame by the scene shard (and therefore
// after vs7dbg.setCamera, drags and stream batches). For each candidate in
// priority order: anchor A = project(x, h, z) of the top-center; eligible iff
// A is inside the canvas AND pickAt(A) returns that instance (occlusion
// culled through the pick buffer); shown iff its 110x18 rect at
// (A.sx+10, A.sy-9) overlaps no already-shown rect by >= 1 px. Culled labels
// are hidden/absent — never nudged, never given an alternate position.

const LABEL_W = 110; // CSS px, border-box
const LABEL_H = 18;  // CSS px, border-box
const LABEL_DX = 10; // rect top-left = (A.sx + 10, A.sy - 9)
const LABEL_DY = -9;

let labelHost = null;        // #viz-labels element (absolutely positioned over the canvas)
const labelEls = new Map();  // record id -> persistent .viz-label element (reused across frames)

function ensureLabelEl(id) {
  let el = labelEls.get(id);
  if (!el) {
    el = document.createElement('div');
    el.className = 'viz-label';
    el.setAttribute('data-id', id);
    // Inline so the 110x18 border-box / single-line ellipsis contract holds
    // regardless of styles.css:
    el.style.position = 'absolute';
    el.style.boxSizing = 'border-box';
    el.style.width = LABEL_W + 'px';
    el.style.height = LABEL_H + 'px';
    el.style.whiteSpace = 'nowrap';
    el.style.overflow = 'hidden';
    el.style.textOverflow = 'ellipsis';
    el.style.display = 'none';
    labelHost.appendChild(el);
    labelEls.set(id, el);
  }
  return el;
}

function updateLabels() {
  if (!S || !S.gl || S.Wcss <= 0 || S.Hcss <= 0) return; // pre-boot: nothing to cull against
  const host = document.getElementById('viz-labels');
  if (!host) return;
  labelHost = host;

  const cands = labelCandidates(); // priority order: a_major DESC, id ASC
  const candIds = new Set(cands.map((n) => rec.ids[n]));
  const placed = [];               // rects of already-shown labels this pass
  const shown = new Set();

  for (const n of cands) {
    const A = project(rec.x[n], rec.h[n], rec.z[n]); // top-center anchor, live camera
    if (!A) continue; // zc <= 0.5: does not project
    if (A.sx < 0 || A.sx > S.Wcss || A.sy < 0 || A.sy > S.Hcss) continue; // inside canvas
    // Occlusion through the pick buffer; clamp the query one CSS px inside so
    // an anchor exactly on the right/bottom edge cannot index past the
    // device-pixel cache. A failed pick culls (safe default), never crashes.
    let hit = null;
    try {
      hit = pickAt(Math.min(A.sx, S.Wcss - 1), Math.min(A.sy, S.Hcss - 1));
    } catch (err) {
      hit = null;
    }
    if (!hit || hit.index !== n) continue;

    const l = A.sx + LABEL_DX, t = A.sy + LABEL_DY;
    const r = l + LABEL_W, b = t + LABEL_H;
    let clash = false;
    for (const p of placed) {
      if (l < p.r && p.l < r && t < p.b && p.t < b) { clash = true; break; } // >= 1 px overlap
    }
    if (clash) continue; // cull: hidden or absent, never nudged

    placed.push({ l, t, r, b });
    const id = rec.ids[n];
    shown.add(id);
    const el = ensureLabelEl(id);
    el.textContent = formatAmount(rec.amountMinor[n], rec.currency[n]);
    el.style.left = l + 'px';
    el.style.top = t + 'px';
    el.style.display = 'block';
  }

  // Sync the DOM: hide culled candidates, drop elements no longer candidates.
  for (const [id, el] of labelEls) {
    if (shown.has(id)) continue;
    if (candIds.has(id)) el.style.display = 'none';
    else { el.remove(); labelEls.delete(id); }
  }
}
