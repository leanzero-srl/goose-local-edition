#!/usr/bin/env node
// product_probe_v2.mjs — sb-6 browser-truth probe for VendorSync Pro (the hard tier).
// Usage: node product_probe_v2.mjs <load|sync|error|empty|viz|viz-fallback|note|under-load> <baseUrl>
//        node product_probe_v2.mjs --selfcheck
// Prints EXACTLY ONE JSON object to stdout; all diagnostics go to stderr.
// Exit 0 always, including failed checks and the hard cap (timedOut:true);
// nonzero exit only when the probe itself crashes (or --selfcheck fails).
//
// sb-6 additions over product_probe.mjs (which stays untouched; this file is additive):
//   viz           analytic 3D scene grading against the frozen spec-v3 contract — every expected
//                 pixel recomputed node-side from BENCH_VIZ_BUCKETS, compared to readPixels.
//   viz-fallback  glKill init script: WebGL unavailable, 2D table + notice graded.
//   note          the causal optimistic-paint proof (F10a): the note POST is HELD via
//                 page.route, so paint-before-response is proven, not timed.
//   under-load    frontend-under-load hook: reads exercised while a sync is in flight,
//                 latencies stamped page-side.
// Red-team amendments applied here (SB6-REDTEAM.md): F1(b,c) F4 F6 F9 F10a F10b F11 F12
// F17 F18 F22 F23 — each marked at its implementation site.

import { createRequire } from 'module';
import { execSync } from 'child_process';
import { join, dirname } from 'path';
import { mkdirSync } from 'fs';

const err = (...a) => console.error('[probe]', ...a);

function loadPlaywright() {
  const attempts = [];
  try {
    return createRequire(import.meta.url)('playwright');
  } catch (e) {
    attempts.push('local: ' + e.message);
  }
  try {
    const g = execSync('npm root -g', { encoding: 'utf8' }).trim();
    return createRequire(join(g, '__probe__.js'))('playwright');
  } catch (e) {
    attempts.push('npm-root-g: ' + e.message);
  }
  try {
    const g = join(dirname(process.execPath), '..', 'lib', 'node_modules');
    return createRequire(join(g, '__probe__.js'))('playwright');
  } catch (e) {
    attempts.push('execPath: ' + e.message);
  }
  throw new Error('cannot resolve playwright: ' + attempts.join(' | '));
}

// ── sb-6 viz: pinned scene contract (must mirror spec-build-v3 §5 verbatim) ──────────────────
const VIZ = {
  fovYDeg: 50, near: 0.1, far: 200,
  yaw0: 35, pitch0: 27, dist0: 30, target: [0, 3, 0],
  dragDegPerPx: 0.35, wheelK: 0.0012,
  pitchMin: 5, pitchMax: 85, distMin: 10, distMax: 90,
  cellPitch: 1.5, half: 0.5, hPerCount: 0.25,
  bg: [15, 23, 42],                                     // #0F172A
  statusOrder: ['settled', 'pending', 'refunded', 'failed'],
  status: { settled: [22, 163, 74], pending: [245, 158, 11],
            refunded: [139, 92, 246], failed: [220, 38, 38] },
  sideFactor: 0.62, tol: 8,
  minTopAreaPx2: 25,                                    // F12: smaller tops are excluded, not graded
};
const VIZ_LAUNCH_ARGS = [
  '--use-angle=swiftshader', '--enable-unsafe-swiftshader',   // deterministic software GL (verified)
  '--force-color-profile=srgb', '--force-device-scale-factor=1',
  '--js-flags=--random-seed=1357',
];
const sideColor = (c) => c.map((v) => Math.round(v * VIZ.sideFactor));

// ── vector / projection math (verified pipeline, spec-basis form) ────────────────────────────
const deg = (d) => (d * Math.PI) / 180;
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (a) => { const l = Math.hypot(...a); return [a[0] / l, a[1] / l, a[2] / l]; };

// F11: yaw normalized to [0,360) renders identically to unbounded yaw — compare angular distance.
const angDist = (a, b) => {
  const d = Math.abs(a - b) % 360;
  return Math.min(d, 360 - d);
};

function cameraEye(yaw, pitch, dist) {
  const t = deg(yaw), p = deg(pitch), T = VIZ.target;
  return [T[0] + dist * Math.cos(p) * Math.sin(t),
          T[1] + dist * Math.sin(p),
          T[2] + dist * Math.cos(p) * Math.cos(t)];
}
function cameraBasis(eye) {
  const f = norm(sub(VIZ.target, eye));
  const r = norm(cross(f, [0, 1, 0]));
  const u = cross(r, f);
  return { f, r, u };
}
// world point → CSS px inside the canvas (per the spec's exact formulas), or null if zc <= 0.1
function projectPt(eye, basis, W, H, p) {
  const q = sub(p, eye);
  const xc = dot(q, basis.r), yc = dot(q, basis.u), zc = dot(q, basis.f);
  if (zc <= VIZ.near) return null;
  const k = 1 / Math.tan(deg(VIZ.fovYDeg) / 2), aspect = W / H;
  return { x: ((k / aspect) * (xc / zc) + 1) / 2 * W,
           y: (1 - k * (yc / zc)) / 2 * H };
}
function rayAABB(o, d, mn, mx) {                        // slab test: entry distance, or null
  let t0 = -Infinity, t1 = Infinity;
  for (let k = 0; k < 3; k++) {
    if (Math.abs(d[k]) < 1e-9) { if (o[k] < mn[k] || o[k] > mx[k]) return null; continue; }
    let a = (mn[k] - o[k]) / d[k], b = (mx[k] - o[k]) / d[k];
    if (a > b) [a, b] = [b, a];
    t0 = Math.max(t0, a); t1 = Math.min(t1, b);
  }
  return t0 <= t1 && t1 > 0 ? Math.max(t0, 0) : null;
}
const shoelace = (pts) => {
  let s = 0;
  for (let i = 0; i < pts.length; i++) {
    const a = pts[i], b = pts[(i + 1) % pts.length];
    s += a.x * b.y - b.x * a.y;
  }
  return Math.abs(s) / 2;
};
// Bars from the expected /api/buckets payload (BENCH_VIZ_BUCKETS): zero-count cells draw nothing.
function barGeom(buckets) {
  const D = buckets.days.length, bars = [];
  for (const cell of buckets.cells) {
    if (!cell.count) continue;
    const i = buckets.days.indexOf(cell.day);
    const j = VIZ.statusOrder.indexOf(cell.status);
    const x = (i - (D - 1) / 2) * VIZ.cellPitch;
    const z = (j - 1.5) * VIZ.cellPitch;
    const h = cell.count * VIZ.hPerCount;
    bars.push({ key: `${cell.day}|${cell.status}`, day: cell.day, status: cell.status,
                count: cell.count, i, j, x, z, h,
                mn: [x - VIZ.half, 0, z - VIZ.half], mx: [x + VIZ.half, h, z + VIZ.half] });
  }
  return bars;
}
function firstHit(eye, P, bars) {                       // nearest box on the forward ray eye→P→∞
  const d = norm(sub(P, eye));
  let best = null;
  bars.forEach((b, i) => {
    const t = rayAABB(eye, d, b.mn, b.mx);
    if (t != null && (best === null || t < best.t)) best = { index: i, t };
  });
  return best;
}
// F12: projected top-face polygon area (CSS px²) — the exclusion guard for rounding-fragile tops.
function topFaceAreaPx2(eye, basis, W, H, b) {
  const corners = [
    [b.x - VIZ.half, b.h, b.z - VIZ.half], [b.x + VIZ.half, b.h, b.z - VIZ.half],
    [b.x + VIZ.half, b.h, b.z + VIZ.half], [b.x - VIZ.half, b.h, b.z + VIZ.half],
  ].map((p) => projectPt(eye, basis, W, H, p));
  if (corners.some((c) => !c)) return null;
  return shoelace(corners);
}
// Inverse of projectPt: the world-space ray direction through a CSS pixel (pickAnalytic's math).
function unprojectDir(basis, W, H, sx, sy) {
  const k = 1 / Math.tan(deg(VIZ.fovYDeg) / 2), aspect = W / H;
  const cx = ((sx / W) * 2 - 1) / (k / aspect);
  const cy = (1 - (sy / H) * 2) / k;
  return norm([basis.f[0] + cx * basis.r[0] + cy * basis.u[0],
               basis.f[1] + cx * basis.r[1] + cy * basis.u[1],
               basis.f[2] + cx * basis.r[2] + cy * basis.u[2]]);
}

// Every sample with its analytic expectation. Occlusion-filtered, deterministic (verified).
// F12: tops with projected face area < 25 px² go to skippedSmall instead of the graded set.
// F12 clearance guard (measured on the reference): a sample is graded only if the WHOLE 3×3
// majority window provably lies on the one expected surface — rays cast ±2 px around the
// center must first-hit the same bar (and for tops, enter through the TOP plane; for
// background samples, hit nothing). This removes the two real sub-pixel failure modes: the
// sliver top (eye barely above the face → the face projects ~1 px tall but wide enough to
// pass the area gate) and the sample sitting within a pixel of an occluder's silhouette.
function buildVizSamples(buckets, yaw, pitch, dist, W, H) {
  const bars = barGeom(buckets);
  const eye = cameraEye(yaw, pitch, dist), basis = cameraBasis(eye);
  const proj = (p) => projectPt(eye, basis, W, H, p);
  const inCanvas = (p) => p && p.x >= 2 && p.x <= W - 3 && p.y >= 2 && p.y <= H - 3;
  const CLEAR_PX = 2;
  const OFFS = [[-CLEAR_PX, 0], [CLEAR_PX, 0], [0, -CLEAR_PX], [0, CLEAR_PX]];
  function clearAt(sx, sy, wantIndex, wantTop) {
    for (const [ox, oy] of OFFS) {
      const d = unprojectDir(basis, W, H, sx + ox, sy + oy);
      let best = null;
      bars.forEach((b, k) => {
        const t = rayAABB(eye, d, b.mn, b.mx);
        if (t != null && (best === null || t < best.t)) best = { k, t };
      });
      if (wantIndex == null) {
        if (best) return false;                      // background sample: nothing may intrude
        continue;
      }
      if (!best || best.k !== wantIndex) return false;
      if (wantTop) {
        const py = eye[1] + best.t * d[1];
        if (Math.abs(py - bars[wantIndex].h) > 1e-6) return false;  // entered via a side face
      }
    }
    return true;
  }
  const tops = [], above = [], sides = [], occludedTops = [], skippedSmall = [];
  bars.forEach((b, i) => {
    const T = [b.x, b.h, b.z], pT = proj(T);
    if (inCanvas(pT)) {
      const area = topFaceAreaPx2(eye, basis, W, H, b);
      const hit = firstHit(eye, T, bars), dT = Math.hypot(...sub(T, eye));
      if (hit && hit.index !== i && hit.t < dT - 1e-6)
        occludedTops.push({ i, key: b.key, css: pT, occluder: hit.index, area });
      else if (eye[1] <= b.h + 0.05)
        // Top face BACKFACES the camera (bar taller than the eye): the pixel at the
        // projected top-center legitimately shows the bar's own SIDE face. Analytically
        // exact: the y=h plane is visible iff eye.y > h.
        skippedSmall.push({ i, key: b.key, backfacing: true,
                            area: area == null ? null : +area.toFixed(1) });
      else if (area == null || area < VIZ.minTopAreaPx2
               || !clearAt(pT.x, pT.y, i, true))
        skippedSmall.push({ i, key: b.key, area: area == null ? null : +area.toFixed(1) });
      else
        tops.push({ i, key: b.key, status: b.status, cx: pT.x, cy: pT.y, area,
                    expect: VIZ.status[b.status], kind: 'top' });
    }
    const Q = [b.x, b.h + 0.6, b.z], pQ = proj(Q);       // just above the top: must be sky
    if (inCanvas(pQ) && !firstHit(eye, Q, bars) && clearAt(pQ.x, pQ.y, null))
      above.push({ i, cx: pQ.x, cy: pQ.y, expect: VIZ.bg, kind: 'above' });
    const S = [b.x, 0.6 * b.h, b.z], pS = proj(S);       // inside the column: its own surface
    const hS = inCanvas(pS) && firstHit(eye, S, bars);
    if (pS && hS && hS.index === i && clearAt(pS.x, pS.y, i, false))
      sides.push({ i, cx: pS.x, cy: pS.y,
                   expectAny: [sideColor(VIZ.status[b.status]), VIZ.status[b.status]], kind: 'side' });
  });
  const sky = [];                                        // background over the grid gaps
  const D = buckets.days.length;
  const maxH = Math.max(0.25, ...bars.map((b) => b.h));
  for (const [gx, gz] of [[-0.3, -0.35], [0.3, -0.35], [-0.3, 0.35], [0.3, 0.35], [0, 0]]) {
    const G = [gx * D * VIZ.cellPitch, maxH + 2.0, gz * 4 * VIZ.cellPitch];
    const p = proj(G);
    if (inCanvas(p) && !firstHit(eye, G, bars) && clearAt(p.x, p.y, null))
      sky.push({ cx: p.x, cy: p.y, expect: VIZ.bg, kind: 'sky' });
  }
  const corners = [[3, 3], [W - 4, 3], [3, H - 4], [W - 4, H - 4]]
    .map(([cx, cy]) => ({ cx, cy, expect: VIZ.bg, kind: 'corner' }));
  const grid = [];                                       // blind coverage grid, no expectation
  for (let ix = 0; ix < 6; ix++) for (let iy = 0; iy < 4; iy++)
    grid.push({ cx: (ix + 0.5) * W / 6, cy: (iy + 0.5) * H / 4, kind: 'grid' });
  return { bars, eye, basis, tops, above, sides, sky, corners, grid, occludedTops, skippedSmall };
}

// ── selfcheck: the analytic math vs hand-computed cases; no browser, no app ──────────────────
function selfcheck() {
  const failures = [];
  const near = (a, b, tol, what) => {
    if (a == null || Math.abs(a - b) > tol) failures.push(`${what}: got ${a} want ${b}`);
  };
  // Case 1 — yaw 0, pitch 0, dist 10, T=(0,3,0): eye=(0,3,10), basis is the identity frame.
  // (0,3,0) is the target → exact canvas center; (1,3,0): ndcx = (k·H/W)·(1/10).
  {
    const eye = cameraEye(0, 0, 10), basis = cameraBasis(eye);
    near(eye[0], 0, 1e-9, 'case1 eye.x'); near(eye[1], 3, 1e-9, 'case1 eye.y'); near(eye[2], 10, 1e-9, 'case1 eye.z');
    const p1 = projectPt(eye, basis, 800, 480, [0, 3, 0]);
    near(p1 && p1.x, 400, 1e-6, 'case1 center.x'); near(p1 && p1.y, 240, 1e-6, 'case1 center.y');
    const p2 = projectPt(eye, basis, 800, 480, [1, 3, 0]);
    near(p2 && p2.x, 451.4681660922294, 1e-6, 'case1 off.x'); near(p2 && p2.y, 240, 1e-6, 'case1 off.y');
  }
  // Case 2 — yaw 90, pitch 0, dist 20: eye=(20,3,0), f=(-1,0,0), r=(0,0,-1).
  // (0,3,5): xc=-5, zc=20; (30,3,0) is behind the camera (zc=-10) → null.
  {
    const eye = cameraEye(90, 0, 20), basis = cameraBasis(eye);
    const p1 = projectPt(eye, basis, 800, 480, [0, 3, 5]);
    near(p1 && p1.x, 271.32958476942645, 1e-6, 'case2 side.x'); near(p1 && p1.y, 240, 1e-6, 'case2 side.y');
    if (projectPt(eye, basis, 800, 480, [30, 3, 0]) !== null) failures.push('case2 behind-camera not null');
  }
  // Case 3 — pitch 30, dist 12: eye=(0,9,10.392…), |T−eye|=12 exactly; the target projects to
  // the canvas center at ANY camera; (0,5,0): yc=√3, zc=11. Plus the occlusion + grid pipeline.
  {
    const eye = cameraEye(0, 30, 12), basis = cameraBasis(eye);
    const p1 = projectPt(eye, basis, 1000, 500, [0, 3, 0]);
    near(p1 && p1.x, 500, 1e-6, 'case3 center.x'); near(p1 && p1.y, 250, 1e-6, 'case3 center.y');
    const p2 = projectPt(eye, basis, 1000, 500, [0, 5, 0]);
    near(p2 && p2.x, 500, 1e-6, 'case3 up.x'); near(p2 && p2.y, 165.58193310214486, 1e-6, 'case3 up.y');
    // occlusion: from eye (0,3,10) toward (0,2,0), the taller box at z=3 intercepts the ray at
    // t = 0.65·√101 (z-slab entry at z=3.5, where the ray's y=2.35 ≤ 4) before the target box.
    const eyeA = cameraEye(0, 0, 10);
    const bars = [
      { mn: [-0.5, 0, -0.5], mx: [0.5, 2, 0.5] },
      { mn: [-0.5, 0, 2.5], mx: [0.5, 4, 3.5] },
    ];
    const hit = firstHit(eyeA, [0, 2, 0], bars);
    if (!hit || hit.index !== 1) failures.push(`case3 occluder index: got ${hit && hit.index} want 1`);
    else near(hit.t, 0.65 * Math.sqrt(101), 1e-6, 'case3 occluder t');
    // barGeom on a 2-day grid: zero-count cell omitted, x/z/h per the frozen formulas.
    const g = barGeom({ days: ['2026-03-01', '2026-03-02'], statuses: VIZ.statusOrder,
      cells: [{ day: '2026-03-01', status: 'settled', count: 3 },
              { day: '2026-03-01', status: 'failed', count: 0 },
              { day: '2026-03-02', status: 'pending', count: 2 }] });
    if (g.length !== 2) failures.push(`case3 barGeom length: got ${g.length} want 2`);
    else {
      near(g[0].x, -0.75, 1e-9, 'case3 bar0.x'); near(g[0].z, -2.25, 1e-9, 'case3 bar0.z');
      near(g[0].h, 0.75, 1e-9, 'case3 bar0.h');
      near(g[1].x, 0.75, 1e-9, 'case3 bar1.x'); near(g[1].z, -0.75, 1e-9, 'case3 bar1.z');
      near(g[1].h, 0.5, 1e-9, 'case3 bar1.h');
    }
    near(shoelace([{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }, { x: 0, y: 1 }]), 1, 1e-12, 'shoelace unit square');
    near(angDist(-7, 353), 0, 1e-9, 'angDist(-7,353)');
    near(angDist(35, -7), 42, 1e-9, 'angDist(35,-7)');
    near(angDist(350, 10), 20, 1e-9, 'angDist(350,10)');
  }
  const ok = failures.length === 0;
  process.stdout.write(JSON.stringify({ selfcheck: ok ? 'ok' : 'fail', cases: 3, failures }) + '\n');
  process.exit(ok ? 0 : 1);
}

const args = process.argv.slice(2);
if (args.includes('--selfcheck')) selfcheck();

// Optional: --block-api makes the error scenario usable when the caller cannot kill the
// backend — the document loads, its data fetches are refused, and the app's own error UI
// is what gets measured. Without it, a dead backend simply fails the navigation.
const blockApi = args.includes('--block-api');
const positional = args.filter((a) => !a.startsWith('--'));
const scenario = positional[0];
const baseUrl = positional[1];
const SCENARIOS = ['load', 'sync', 'error', 'empty', 'viz', 'viz-fallback', 'note', 'under-load'];
if (!SCENARIOS.includes(scenario) || !baseUrl) {
  err(`usage: node product_probe_v2.mjs <${SCENARIOS.join('|')}> <baseUrl> [--block-api] | --selfcheck`);
  process.exit(2);
}
const isViz = scenario.startsWith('viz');

// F18: the viz scenarios carry SwiftShader startup plus a long scripted interaction phase;
// 90s is the measured near-miss on a contended machine, so they get 150s.
const HARD_MS = isViz ? 150000 : 90000;
const startedAt = Date.now();
const budgetLeft = () => HARD_MS - (Date.now() - startedAt);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const result = { scenario, baseUrl, timedOut: false };
// F18: phases merge into `result` as they complete, so a cap hit ships everything measured
// instead of zeroing the tier; the scorer reads absent sections as PROBE UNAVAILABLE.
const merge = (o) => Object.assign(result, o);
let printed = false;
let browser = null;

function emit(extra, cb) {
  if (printed) return;
  printed = true;
  process.stdout.write(JSON.stringify({ ...result, ...extra }) + '\n', cb || (() => {}));
}

const hardTimer = setTimeout(() => {
  err(`hard ${HARD_MS / 1000}s cap hit — emitting partial result`);
  emit({ timedOut: true }, async () => {
    try {
      await Promise.race([browser && browser.close(), sleep(1500)]);
    } catch {}
    process.exit(0);
  });
}, HARD_MS);

// ---------- page-side functions (self-contained; serialized by playwright) ----------

// Installed before any page script: stamps the exact moment the first data row lands,
// so the reported time is the app's render time, not the probe's polling latency.
function initFirstDataStamp() {
  window.__probeFirstDataMs = null;
  const check = () => {
    if (window.__probeFirstDataMs != null) return true;
    let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
      (r) => r.querySelectorAll('td,th').length >= 2
    );
    if (rows.length === 0) {
      rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
        (r) =>
          r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
          !r.querySelector('[role="columnheader"]')
      );
    }
    if (rows.some((r) => (r.textContent || '').trim().length > 0)) {
      window.__probeFirstDataMs = performance.now();
      return true;
    }
    return false;
  };
  const start = () => {
    if (check()) return;
    const mo = new MutationObserver(() => {
      if (check()) mo.disconnect();
    });
    mo.observe(document.documentElement, { childList: true, subtree: true, characterData: true });
  };
  if (document.documentElement) start();
  else document.addEventListener('readystatechange', start, { once: true });
}

function pageFirstDataMs() {
  if (window.__probeFirstDataMs != null) return window.__probeFirstDataMs;
  // Rendered-means-seen here too: innerText of a display:none row falls back to textContent in
  // Chromium, so without the rects check this timer fires on rows the user never saw.
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
    (r) => r.querySelectorAll('td,th').length >= 2 && visible(r)
  );
  if (rows.length === 0) {
    rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
      (r) =>
        r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
        !r.querySelector('[role="columnheader"]') &&
        visible(r)
    );
  }
  rows = rows.filter((r) => (r.innerText || '').trim().length > 0);
  return rows.length > 0 ? performance.now() : null;
}

function pageAnalyzeLoad() {
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';

  function dataRows() {
    let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
      (r) => r.querySelectorAll('td,th').length >= 2
    );
    if (rows.length === 0) {
      rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
        (r) =>
          r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
          !r.querySelector('[role="columnheader"]')
      );
    }
    return rows;
  }
  // RENDERED means SEEN (the sb-5 run-9 lesson): a row counts only if the browser would paint
  // it; the raw DOM count stays as a diagnostic so the gap itself is visible in the output.
  const domRows = dataRows();
  const rows = domRows.filter(visible);
  const cellsOf = (r) => Array.from(r.querySelectorAll('td,th,[role="cell"],[role="gridcell"]'));
  const renderedRowCount = rows.length;
  const domRowCount = domRows.length;

  const dateRe =
    /(\d{4}-\d{2}-\d{2})|(\d{1,2}[/.\-]\d{1,2}[/.\-]\d{2,4})|\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\.?\s+\d{1,2}\b|\b\d{1,2}\s+(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*\b/i;
  let dateCol = 0;
  if (rows.length) {
    const idx = cellsOf(rows[0]).findIndex((c) => dateRe.test(c.innerText || ''));
    if (idx >= 0) dateCol = idx;
  }
  const dateTexts = rows
    .slice(0, 3)
    .map((r) => ((cellsOf(r)[dateCol] || {}).innerText || '').trim())
    .filter(Boolean);

  // sb-6: harvest amount-cell texts for v_currency_rendered — exponent truth, not a grep.
  const curRe = /\b(EUR|USD|JPY|KWD)\b|[€$¥]|د\.ك/;
  const amountTexts = rows.slice(0, 12).map((r) => {
    const cell = cellsOf(r).find((c) => curRe.test(c.innerText || '') && /\d/.test(c.innerText || ''));
    return cell ? (cell.innerText || '').trim().slice(0, 40) : null;
  }).filter(Boolean);

  const statusWordRe =
    /^(pending|processing|in.progress|complete(d)?|success(ful)?|paid|unpaid|failed|failure|error|synced|syncing|active|inactive|overdue|refunded|cancel(l)?ed|declined|posted|settled|new|open|closed)$/i;
  let statusIdx = -1;
  const table = rows.length ? rows[0].closest('table') : null;
  if (table) {
    let headerCells = Array.from(table.querySelectorAll('thead th, thead td'));
    if (headerCells.length === 0) {
      const firstTr = table.querySelector('tr');
      if (firstTr && rows.indexOf(firstTr) === -1) headerCells = Array.from(firstTr.children);
    }
    statusIdx = headerCells.findIndex((h) => /status|state/i.test(h.innerText || ''));
  }
  const statusStyles = {};
  for (const r of rows) {
    const cells = cellsOf(r);
    const cell =
      statusIdx >= 0
        ? cells[statusIdx]
        : cells.find((c) => statusWordRe.test((c.innerText || '').trim()));
    if (!cell) continue;
    const label = (cell.innerText || '').trim();
    if (!label || label.length > 40 || statusStyles[label]) continue;
    let carrier = cell;
    let descended = true;
    while (descended) {
      descended = false;
      for (const ch of carrier.children) {
        if ((ch.innerText || '').trim() === label) {
          carrier = ch;
          descended = true;
          break;
        }
      }
    }
    const cs = getComputedStyle(carrier);
    let bg = cs.backgroundColor;
    let node = carrier;
    while (node && node !== r.parentElement && (bg === 'rgba(0, 0, 0, 0)' || bg === 'transparent')) {
      node = node.parentElement;
      if (node) bg = getComputedStyle(node).backgroundColor;
    }
    statusStyles[label] = { color: cs.color, backgroundColor: bg };
  }

  let totalClaimedInDom = null;
  {
    // Counts live in split markup ("<div>Payments: <strong>247</strong></div>"), so scan
    // element innerText, not raw text nodes. Strip money/dates first: a currency total
    // would otherwise swamp the row count under a largest-integer rule.
    const scrub = (s) =>
      s
        .replace(/\d{4}-\d{2}-\d{2}[T\s0-9:.+Z-]*/g, ' ')
        .replace(/[€$£¥₹]\s?[\d,]+(?:\.\d+)?/g, ' ')
        .replace(/[\d,]+(?:\.\d+)?\s?(?:EUR|USD|GBP|CHF|JPY|KWD)\b/gi, ' ')
        .replace(/\b\d+(?:\.\d+)+\b/g, ' ');
    const kw =
      '(?:payments?|records?|results?|rows|items|entries|transactions|invoices?|count|total)';
    const pats = [
      /\bof\s+([\d,]+)\b/gi,
      new RegExp('\\b([\\d,]+)\\s+' + kw + '\\b', 'gi'),
      new RegExp('\\b' + kw + '\\b[^0-9a-z]{0,12}([\\d,]+)\\b', 'gi'),
    ];
    const take = (raw) => {
      const t = scrub(raw);
      for (const re of pats) {
        re.lastIndex = 0;
        let m;
        while ((m = re.exec(t))) {
          const v = parseInt(m[1].replace(/,/g, ''), 10);
          if (Number.isFinite(v) && (totalClaimedInDom === null || v > totalClaimedInDom))
            totalClaimedInDom = v;
        }
      }
    };
    const els = Array.from(document.querySelectorAll('body *')).filter((el) => {
      if (el.closest('script,style,td,th,[role="cell"],[role="gridcell"]')) return false;
      if (el.querySelector('table,tbody')) return false;
      if (!visible(el)) return false;
      const t = el.innerText || '';
      return t.length > 0 && t.length <= 250 && /\d/.test(t);
    });
    for (const el of els) take(el.innerText || '');
  }

  const paginationControls = (() => {
    const shortCtl = /^(prev(ious)?|next|first|last|page\s*\d+|[«»‹›]|[<>]{1,2})$/i;
    const ctl = Array.from(document.querySelectorAll('button, a, [role="button"]')).some((el) => {
      if (!visible(el)) return false;
      const own = (el.innerText || '').trim();
      const aria = (el.getAttribute('aria-label') || '').trim();
      return shortCtl.test(own) || /^(prev(ious)?|next)( page)?$/i.test(aria);
    });
    if (ctl) return true;
    if (document.querySelector('nav[aria-label*="pag" i], [class*="pagin" i]')) return true;
    return /showing\s+[\d,]+(\s*(?:[-–—]|to)\s*[\d,]+)?\s+of\s+[\d,]+/i.test(document.body.innerText || '');
  })();

  const filterControl = (() => {
    const re = /filter|status/i;
    const els = Array.from(
      document.querySelectorAll('select, input, button, [role="combobox"], [role="listbox"], [role="radiogroup"]')
    );
    for (const el of els) {
      if (!visible(el)) continue;
      let name =
        (el.getAttribute('aria-label') || '') +
        ' ' +
        (el.getAttribute('placeholder') || '') +
        ' ' +
        (el.name || '') +
        ' ' +
        (el.id || '') +
        ' ' +
        (el.title || '') +
        ' ' +
        (el.className && el.className.baseVal === undefined ? el.className : '');
      if (el.tagName === 'BUTTON') name += ' ' + (el.innerText || '');
      if (el.labels) for (const l of el.labels) name += ' ' + (l.innerText || '');
      const wrap = el.closest('label');
      if (wrap) name += ' ' + (wrap.innerText || '');
      const parent = el.parentElement;
      // Buttons match only on their OWN text/label/class (the measured vacuous-pass hole).
      if (el.tagName !== 'BUTTON' && parent && (parent.innerText || '').length < 60)
        name += ' ' + parent.innerText;
      if (re.test(name)) return true;
      if (
        el.tagName === 'SELECT' &&
        Array.from(el.options).filter((o) =>
          /^(all|pending|processing|complete(d)?|paid|failed|synced|active|refunded|cancel(l)?ed|settled)$/i.test(
            (o.text || '').trim()
          )
        ).length >= 2
      )
        return true;
    }
    // The class-name fallback must see a VISIBLE element.
    return Array.from(document.querySelectorAll('[class*="filter" i]')).some(visible);
  })();

  const nav = performance.getEntriesByType('navigation')[0];
  const pageWeightBytes = Math.round(
    ((nav && nav.transferSize) || 0) +
      performance.getEntriesByType('resource').reduce((s, e) => s + (e.transferSize || 0), 0)
  );

  const styling = (() => {
    const hasStylesheet =
      Array.from(document.querySelectorAll('style')).some((s) => (s.textContent || '').trim().length > 0) ||
      Array.from(document.querySelectorAll('link[rel~="stylesheet" i]')).some((l) => l.href);
    const bodyFontFamily = getComputedStyle(document.body).fontFamily;
    const els = document.querySelectorAll(
      'header, thead, th, table, button, [class*="summary" i], [class*="card" i], [class*="header" i]'
    );
    const bgs = new Set();
    for (const el of els) {
      const bg = getComputedStyle(el).backgroundColor;
      if (bg && bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent') bgs.add(bg);
    }
    return { hasStylesheet, bodyFontFamily, distinctBackgroundCount: bgs.size };
  })();

  return {
    renderedRowCount,
    domRowCount,
    totalClaimedInDom,
    dateTexts,
    amountTexts,
    statusStyles,
    paginationControls,
    filterControl,
    pageWeightBytes,
    styling,
  };
}

function pageHorizontalScroll() {
  const sw = Math.max(
    document.documentElement ? document.documentElement.scrollWidth : 0,
    document.body ? document.body.scrollWidth : 0
  );
  return sw > window.innerWidth + 1;
}

function pageSyncState() {
  const cands = Array.from(
    document.querySelectorAll('button, input[type="button"], input[type="submit"], [role="button"], a')
  );
  const acc = (el) =>
    (el.innerText || el.value || '') +
    ' ' +
    (el.getAttribute('aria-label') || '') +
    ' ' +
    (el.title || '');
  const el =
    document.getElementById('sync-now') ||
    cands.find((c) => /sync/i.test(acc(c)) && c.getClientRects().length > 0);
  if (!el || !el.getClientRects().length) return { found: false };
  return {
    found: true,
    text: (el.innerText || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 60),
    disabled:
      el.disabled === true ||
      el.hasAttribute('disabled') ||
      el.getAttribute('aria-disabled') === 'true' ||
      el.getAttribute('aria-busy') === 'true' ||
      el.getAttribute('data-state') === 'syncing',
  };
}

function pageClickSync() {
  const cands = Array.from(
    document.querySelectorAll('button, input[type="button"], input[type="submit"], [role="button"], a')
  );
  const acc = (el) =>
    (el.innerText || el.value || '') +
    ' ' +
    (el.getAttribute('aria-label') || '') +
    ' ' +
    (el.title || '');
  const el =
    document.getElementById('sync-now') ||
    cands.find((c) => /sync/i.test(acc(c)) && c.getClientRects().length > 0);
  if (!el || !el.getClientRects().length) return false;
  el.scrollIntoView({ block: 'center' });
  el.click();
  return true;
}

function pageViewSnapshot() {
  // Same rendered-means-seen rule as pageAnalyzeLoad.
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
    (r) => r.querySelectorAll('td,th').length >= 2 && visible(r)
  );
  if (rows.length === 0) {
    rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
      (r) =>
        r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
        !r.querySelector('[role="columnheader"]') &&
        visible(r)
    );
  }
  let lastSyncText = null;
  const re = /last\s*sync|synced|updated|refreshed|as of|never/i;
  const named = document.getElementById('last-sync');
  if (named && named.getClientRects().length) lastSyncText = (named.innerText || '').trim().slice(0, 150);
  if (!lastSyncText) {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    let n;
    while ((n = walker.nextNode())) {
      const t = (n.textContent || '').trim();
      if (!t || t.length > 150 || !re.test(t)) continue;
      const p = n.parentElement;
      if (!p || p.closest('script,style,td,th,button,[role="cell"],[role="gridcell"]')) continue;
      if (!(p.getClientRects && p.getClientRects().length)) continue;
      lastSyncText = t;
      break;
    }
  }
  let hash = 0;
  const text = rows.map((r) => r.innerText || '').join('|');
  for (let i = 0; i < text.length; i++) hash = (hash * 31 + text.charCodeAt(i)) | 0;
  return { rowCount: rows.length, lastSyncText, tableHash: hash };
}

function pageErrorBanner() {
  const re = /error|unable|unreachable|failed|try again|retry/i;
  const els = Array.from(document.querySelectorAll('body *'));
  for (const el of els) {
    if (el.children.length > 0 && !el.matches('[role="alert"],[class*="error" i],[class*="alert" i],[class*="banner" i]'))
      continue;
    if (el.closest('td,th,[role="cell"],[role="gridcell"],script,style')) continue;
    if (!(el.getClientRects && el.getClientRects().length)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 300 && re.test(t)) return t.slice(0, 120);
  }
  return null;
}

function pageEmptyState() {
  const re = /no\s+payments|nothing|empty|no\s+(records|results|data|items|transactions)/i;
  const els = Array.from(document.querySelectorAll('body *'));
  for (const el of els) {
    if (el.children.length > 0) continue;
    if (el.closest('script,style')) continue;
    if (!(el.getClientRects && el.getClientRects().length)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 200 && re.test(t)) return t.slice(0, 120);
  }
  return null;
}

function pageBlankAndBody() {
  const t = document.body ? (document.body.innerText || '').trim() : '';
  return { blankPage: t.length < 20, bodyTextLength: t.length };
}

// ---------- sb-6 page-side functions ----------

// viz: force preserveDrawingBuffer (readPixels-after-composite trap), record context
// acquisitions, count draw calls, stamp draw times (F10b: the camera-visible ladder is
// measured page-side from these stamps, never through probe RPC), watch for context loss.
function glInstrument() {
  window.__probeViz = { contexts: [], drawCalls: 0, contextLost: 0, drawTs: [] };
  const wrap = (proto, offscreen) => {
    const orig = proto.getContext;
    proto.getContext = function (type, attrs) {
      if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
        attrs = Object.assign({}, attrs || {}, { preserveDrawingBuffer: true });
        const gl = orig.call(this, type, attrs);
        if (gl && !gl.__probeSeen) {
          gl.__probeSeen = true;
          window.__probeViz.contexts.push({ type, offscreen, canvasId: this.id || null });
          for (const fn of ['drawArrays', 'drawElements', 'drawArraysInstanced', 'drawElementsInstanced'])
            if (typeof gl[fn] === 'function') {
              const d = gl[fn].bind(gl);
              gl[fn] = (...a) => {
                window.__probeViz.drawCalls++;
                if (window.__probeViz.drawTs.length < 5000)
                  window.__probeViz.drawTs.push(performance.now());
                return d(...a);
              };
            }
          if (this.addEventListener)
            this.addEventListener('webglcontextlost', () => window.__probeViz.contextLost++);
        }
        return gl;
      }
      return orig.call(this, type, attrs);
    };
  };
  wrap(HTMLCanvasElement.prototype, false);
  if (typeof OffscreenCanvas !== 'undefined') wrap(OffscreenCanvas.prototype, true);
}
// viz-fallback: WebGL unavailable, canvas-2D alive (verified: leaves 2D contexts working).
function glKill() {
  const kill = (proto) => {
    const orig = proto.getContext;
    proto.getContext = function (type, ...rest) {
      if (/webgl/i.test(String(type))) return null;
      return orig.call(this, type, ...rest);
    };
  };
  kill(HTMLCanvasElement.prototype);
  if (typeof OffscreenCanvas !== 'undefined') kill(OffscreenCanvas.prototype);
}

// One readPixels of the whole framebuffer, then indexed lookups; CSS→backing + y-flip verified.
// F12: every expectation sample is judged by 3×3 majority (≥5 of 9 neighbors match), so a
// half-pixel rounding shift near a face edge cannot flip a verdict.
function pageSampleScene(arg) {
  const canvas = document.getElementById('viz3d');
  if (!canvas) return { found: false };
  const rect = canvas.getBoundingClientRect();
  const gl = canvas.getContext('webgl2') || canvas.getContext('webgl') ||
             canvas.getContext('experimental-webgl');
  if (!gl) return { found: true, glReadable: false, rect: { w: rect.width, h: rect.height } };
  const W = canvas.width, H = canvas.height;
  const px = new Uint8Array(W * H * 4);
  gl.readPixels(0, 0, W, H, gl.RGBA, gl.UNSIGNED_BYTE, px);
  const at = (cx, cy) => {
    const bx = Math.min(W - 1, Math.max(0, Math.round(cx * (W / rect.width))));
    const by = Math.min(H - 1, Math.max(0, H - 1 - Math.round(cy * (H / rect.height))));
    const o = (by * W + bx) * 4;
    return [px[o], px[o + 1], px[o + 2]];
  };
  const near = (a, b, t) => Math.abs(a[0] - b[0]) <= t && Math.abs(a[1] - b[1]) <= t && Math.abs(a[2] - b[2]) <= t;
  const out = [];
  for (const s of arg.samples) {
    const got = at(s.cx, s.cy);
    let ok = null;
    if (s.expect || s.expectAny) {
      let hits = 0;
      for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) {
        const c = at(s.cx + dx, s.cy + dy);
        const hit = s.expect ? near(c, s.expect, arg.tol)
                             : s.expectAny.some((e) => near(c, e, arg.tol));
        if (hit) hits++;
      }
      ok = hits >= 5;
    }
    out.push({ kind: s.kind, i: s.i, key: s.key, got, ok });
  }
  return { found: true, glReadable: true,
           rect: { left: rect.left, top: rect.top, w: rect.width, h: rect.height },
           backing: { w: W, h: H }, dpr: window.devicePixelRatio,
           instrument: window.__probeViz || null, samples: out };
}

function pageVizReady() {
  const v = window.__probeViz || { drawCalls: 0, contexts: [] };
  const c = document.getElementById('viz3d');
  const d = window.vsdbg;
  return { canvas: !!c, drawCalls: v.drawCalls, contexts: (v.contexts || []).length,
           vsdbg: !!d, vsdbgVersion: d ? d.version : null };
}

function pageVsdbgSnapshot(arg) {           // scene + camera + frames + spot projections/picks
  const d = window.vsdbg;
  if (!d) return null;
  const safe = (fn) => { try { return fn(); } catch (e) { return { __err: String(e).slice(0, 80) }; } };
  return {
    version: d.version,
    camera: safe(() => d.camera()),
    frames: safe(() => d.frames()),
    scene: safe(() => { const s = d.scene(); return { days: s.days, statuses: s.statuses,
      bars: s.bars.map((b) => ({ key: b.key, count: b.count, x: b.x, z: b.z, h: b.h })) }; }),
    projections: (arg.points || []).map((p) => safe(() => d.project(p[0], p[1], p[2]))),
    picks: (arg.picks || []).map((q) => safe(() => d.pick(q[0], q[1]))),
  };
}

// F1(b): the selected status is read from data-value ONLY — the spec pins it, and any looser
// read (aria-label, textContent) is passable by a dropdown whose collapsed markup lists all
// four labels with a no-op click handler.
function pageStatusFilterValue() {
  const el = document.getElementById('status-filter');
  if (!el) return null;
  const v = el.getAttribute('data-value');
  return v == null ? null : v.trim().toLowerCase().slice(0, 40);
}

// F1(c): the counter-assertion — picking must drive the PRODUCT, not a label. Rendered row
// statuses and the "showing X–Y of TOTAL" readout are re-snapshotted after every pick.
function pageTableStatusSnapshot() {
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  const words = ['settled', 'pending', 'refunded', 'failed'];
  let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
    (r) => r.querySelectorAll('td,th').length >= 2 && visible(r)
  );
  if (rows.length === 0) {
    rows = Array.from(document.querySelectorAll('[role="row"]')).filter(
      (r) =>
        r.querySelectorAll('[role="cell"],[role="gridcell"]').length >= 2 &&
        !r.querySelector('[role="columnheader"]') &&
        visible(r)
    );
  }
  const statuses = [];
  for (const r of rows.slice(0, 60)) {
    const cells = Array.from(r.querySelectorAll('td,th,[role="cell"],[role="gridcell"]'));
    const cell = cells.find((c) => words.includes((c.innerText || '').trim().toLowerCase()));
    statuses.push(cell ? (cell.innerText || '').trim().toLowerCase() : null);
  }
  let readoutTotal = null;
  const m = /showing\s+[\d,]+\s*(?:[-–—]|to)\s*[\d,]+\s+of\s+([\d,]+)/i.exec(document.body.innerText || '');
  if (m) readoutTotal = parseInt(m[1].replace(/,/g, ''), 10);
  return { rowCount: rows.length, statuses, readoutTotal };
}

function pageTooltipState() {
  const el = document.getElementById('viz-tooltip');
  if (!el || !el.getClientRects().length) return { visible: false };
  const cs = getComputedStyle(el);
  if (cs.visibility === 'hidden' || cs.display === 'none' || +cs.opacity === 0) return { visible: false };
  return { visible: true, text: (el.textContent || '').trim().slice(0, 80) };
}

// v_filter exercise: drive the #status-filter custom dropdown the way a user would — open the
// control, click the option naming the target status ('' = the All/clear option) — and report
// the control's data-value afterwards. Options are looked up inside the control first
// (data-option/data-value/[role=option]), then anywhere by exact visible text, so a portaled
// dropdown still works; the CONTAINER itself never qualifies as an option.
function pageDriveStatusFilter(arg) {
  const target = ((arg && arg.value) || '').toLowerCase();
  const el = document.getElementById('status-filter');
  if (!el) return { control: false };
  const visible = (n) => !!(n.getClientRects && n.getClientRects().length) &&
    getComputedStyle(n).visibility !== 'hidden';
  const opener = el.querySelector('button, [role="button"], [aria-haspopup]') || el;
  opener.click();
  const inScope = Array.from(el.querySelectorAll('[data-option], [data-value], [role="option"], li'));
  let option = inScope.find((n) => n !== el && visible(n) &&
    ((n.dataset && n.dataset.option != null && n.dataset.option.toLowerCase() === target) ||
     (n.dataset && n.dataset.value != null && n !== opener && n.dataset.value.toLowerCase() === target)));
  if (!option) {
    const wantText = target === '' ? /^(all|any|clear)\b/i : null;
    const everywhere = Array.from(document.querySelectorAll('[role="option"], li, button, a'));
    option = everywhere.find((n) => {
      if (!visible(n) || n === opener || n === el || el.isSameNode(n)) return false;
      if (n.id === 'currency-filter' || (n.closest && n.closest('#currency-filter'))) return false;
      const t = (n.innerText || '').trim().toLowerCase();
      return target === '' ? (wantText.test(t) && t.length < 24) : t === target;
    });
  }
  if (!option) {
    opener.click();                                   // close what we opened
    return { control: true, optionFound: false };
  }
  option.click();
  const dv = el.getAttribute('data-value');
  return { control: true, optionFound: true,
           dataValue: dv == null ? null : dv.trim().toLowerCase().slice(0, 40) };
}

// F10b: sub-second ladders are stamped page-side. Input = first pointer move after arming;
// visible = the MutationObserver's performance.now() at the mutation that made the tooltip
// visible. The probe only collects the stamps, so RPC polling latency never enters the rung.
function pageArmTooltipWatch() {
  const w = { input: null, visibleAt: null };
  window.__probeTtWatch = w;
  const onMove = () => { if (w.input == null) w.input = performance.now(); };
  window.addEventListener('pointermove', onMove, { capture: true, passive: true });
  window.addEventListener('mousemove', onMove, { capture: true, passive: true });
  const visible = () => {
    const el = document.getElementById('viz-tooltip');
    if (!el || !el.getClientRects().length) return false;
    const cs = getComputedStyle(el);
    return cs.visibility !== 'hidden' && cs.display !== 'none' && +cs.opacity !== 0;
  };
  const mo = new MutationObserver(() => {
    if (w.visibleAt == null && visible()) { w.visibleAt = performance.now(); mo.disconnect(); }
  });
  mo.observe(document.documentElement, { childList: true, subtree: true, attributes: true, characterData: true });
}
function pageReadTooltipWatch() {
  const w = window.__probeTtWatch;
  if (!w) return null;
  return { ms: w.input != null && w.visibleAt != null ? +(w.visibleAt - w.input).toFixed(1) : null };
}

// F10b for the camera-visible-within-250ms budget: input stamped by a page-side listener for
// the named event kinds, visibility taken from the glInstrument draw stamps.
function pageArmDrawWatch(arg) {
  const w = { input: null };
  window.__probeDrawWatch = w;
  const onInput = () => { if (w.input == null) w.input = performance.now(); };
  for (const t of arg.kinds) window.addEventListener(t, onInput, { capture: true, passive: true });
}
function pageReadDrawLatency() {
  const w = window.__probeDrawWatch, v = window.__probeViz;
  if (!w || w.input == null || !v || !Array.isArray(v.drawTs)) return { input: false, ms: null };
  const t = v.drawTs.find((x) => x >= w.input);
  return { input: true, ms: t != null ? +(t - w.input).toFixed(1) : null };
}

function pageFallbackTable() {              // #viz-fallback cell values, keyed day|status
  const t = document.getElementById('viz-fallback');
  if (!t || !t.getClientRects().length) return { visible: false };
  const cells = {};
  for (const td of t.querySelectorAll('[data-day][data-status]'))
    cells[`${td.getAttribute('data-day')}|${td.getAttribute('data-status')}`] =
      parseInt((td.textContent || '').replace(/[^\d-]/g, ''), 10);
  return { visible: true, cells };
}

function pageVizPanelState() {
  const vis = (id) => { const e = document.getElementById(id);
    return !!(e && e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden'); };
  return { vizEmpty: vis('viz-empty'), vizError: vis('viz-error'), canvas: vis('viz3d'),
           toggle: vis('viz-toggle'), fallback: vis('viz-fallback') };
}

// F22: the notice is harvested in BOTH runs — the scorer credits it only when it is visible in
// viz-fallback and absent in the normal viz run, and nearViz kills the static-footer freeload.
function pageVizNotice() {
  const re = /2d|fallback|unavailable|not\s+supported|webgl/i;
  const visEl = (el) => !!(el.getClientRects && el.getClientRects().length) &&
                        getComputedStyle(el).visibility !== 'hidden';
  const anchor = document.getElementById('viz3d') || document.getElementById('viz-toggle') ||
                 document.getElementById('viz-fallback');
  const panel = anchor ? (anchor.closest('section,article,aside,div') || anchor.parentElement) : null;
  for (const el of document.querySelectorAll('body *')) {
    if (el.children.length > 0 || el.closest('script,style,td,th')) continue;
    if (!visEl(el)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 200 && re.test(t)) {
      let nearViz = false;
      if (panel) nearViz = panel.contains(el) ||
        !!(panel.parentElement && panel.parentElement.contains(el));
      return { notice: t.slice(0, 120), nearViz };
    }
  }
  return { notice: null, nearViz: false };
}

// F6: scroll management — the canvas must be fully inside the viewport before any mouse math.
function pageScrollCanvasIntoView() {
  const c = document.getElementById('viz3d');
  if (!c) return false;
  c.scrollIntoView({ block: 'center' });
  return true;
}
function pageCanvasRect() {
  const c = document.getElementById('viz3d');
  if (!c) return null;
  const r = c.getBoundingClientRect();
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, w: r.width, h: r.height,
           viewportW: window.innerWidth, viewportH: window.innerHeight,
           scrollY: window.scrollY };
}

// ---------- note-scenario page functions (F10a/F10b) ----------

function pageNoteCellRect() {
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  let rows = Array.from(document.querySelectorAll('tbody tr')).filter(
    (r) => r.querySelectorAll('td,th').length >= 2 && visible(r)
  );
  if (!rows.length) return { found: false };
  const table = rows[0].closest('table');
  let noteIdx = -1;
  if (table) {
    const headerCells = Array.from(table.querySelectorAll('thead th, thead td'));
    noteIdx = headerCells.findIndex((h) => /note/i.test(h.innerText || ''));
  }
  const row = rows[0];
  const cells = Array.from(row.querySelectorAll('td,th'));
  const cell = noteIdx >= 0 && cells[noteIdx] ? cells[noteIdx] : cells[cells.length - 1];
  if (!cell) return { found: false };
  window.__probeNoteRow = row;
  window.__probeNoteCell = cell;
  cell.scrollIntoView({ block: 'center' });
  const r = cell.getBoundingClientRect();
  return { found: true, x: r.left + r.width / 2, y: r.top + r.height / 2, headerMatched: noteIdx >= 0 };
}
function pageFindNoteEditor() {
  const editable = (el) =>
    !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' ||
             el.getAttribute('contenteditable') === 'true' || el.isContentEditable);
  const a = document.activeElement;
  if (editable(a)) return { found: true, kind: a.tagName === 'INPUT' || a.tagName === 'TEXTAREA' ? a.tagName.toLowerCase() : 'contenteditable' };
  const row = window.__probeNoteRow;
  if (row) {
    const el = row.querySelector('input, textarea, [contenteditable="true"]');
    if (el && el.getClientRects().length) { el.focus(); return { found: true, kind: el.tagName.toLowerCase() }; }
  }
  return { found: false };
}
// Armed AFTER typing, right before confirm: input = the confirm keydown/click, painted = the
// first mutation that lands the value as row TEXT (input values are not innerText, so a still-
// open editor does not stamp). preexisting flags contenteditable editors whose draft already
// reads as text — the scorer discounts paintMs there and leans on the held-request proof.
function pageArmNoteWatch(arg) {
  const row = window.__probeNoteRow;
  const w = { input: null, paintedAt: null,
              preexisting: !!(row && (row.innerText || '').includes(arg.value)) };
  window.__probeNoteWatch = w;
  const onConfirm = (e) => {
    if (w.input == null && (e.type === 'click' || e.key === 'Enter')) w.input = performance.now();
  };
  window.addEventListener('keydown', onConfirm, { capture: true });
  window.addEventListener('click', onConfirm, { capture: true });
  const mo = new MutationObserver(() => {
    if (w.paintedAt == null && row && (w.input != null || !w.preexisting) &&
        (row.innerText || '').includes(arg.value)) {
      w.paintedAt = performance.now();
      mo.disconnect();
    }
  });
  mo.observe(document.documentElement, { childList: true, subtree: true, characterData: true, attributes: true });
}
function pageNoteMidState(arg) {
  const row = window.__probeNoteRow;
  const vis = (el) => !!(el && el.getClientRects && el.getClientRects().length);
  const saving = document.querySelector('[data-state="saving"]');
  return {
    painted: !!(row && (row.innerText || '').includes(arg.value)),
    saving: vis(saving),
  };
}
function pageNoteAfterState(arg) {
  const row = window.__probeNoteRow;
  const vis = (el) => !!(el && el.getClientRects && el.getClientRects().length);
  return {
    painted: !!(row && (row.innerText || '').includes(arg.value)),
    saved: vis(document.querySelector('[data-state="saved"]')),
    saving: vis(document.querySelector('[data-state="saving"]')),
  };
}
function pageReadNoteWatch() {
  const w = window.__probeNoteWatch;
  if (!w) return null;
  return { preexisting: w.preexisting,
           ms: w.input != null && w.paintedAt != null ? +(w.paintedAt - w.input).toFixed(1) : null };
}

// ---------- under-load page functions ----------

function pageSyncInFlight() {
  const el = document.getElementById('sync-now');
  if (!el) return false;
  return el.disabled === true || el.hasAttribute('disabled') ||
         el.getAttribute('aria-disabled') === 'true' ||
         el.getAttribute('data-state') === 'syncing';
}
function pageArmTableWatch() {
  const w = { input: null, mutatedAt: null };
  window.__probeTblWatch = w;
  const onClick = () => { if (w.input == null) w.input = performance.now(); };
  window.addEventListener('click', onClick, { capture: true });
  const target = document.querySelector('tbody') || document.body;
  const mo = new MutationObserver(() => {
    if (w.input != null && w.mutatedAt == null) { w.mutatedAt = performance.now(); mo.disconnect(); }
  });
  mo.observe(target, { childList: true, subtree: true, characterData: true });
}
function pageReadTableWatch() {
  const w = window.__probeTblWatch;
  if (!w) return null;
  return { ms: w.input != null && w.mutatedAt != null ? +(w.mutatedAt - w.input).toFixed(1) : null };
}
function pageClickPager(arg) {
  const el = document.getElementById(arg.which);
  if (!el || !el.getClientRects().length || el.disabled === true || el.hasAttribute('disabled'))
    return false;
  el.click();
  return true;
}

// ---------- scenarios ----------

async function main() {
  // F17: a missing expectation must refuse, never improvise — grading a full-fixture app down
  // the empty path (or vice versa) is a harness failure the scorer must see as one.
  let buckets = null;
  if (isViz) {
    try {
      buckets = JSON.parse(process.env.BENCH_VIZ_BUCKETS);
    } catch {}
    if (!buckets || !Array.isArray(buckets.days) || !Array.isArray(buckets.cells) ||
        !Array.isArray(buckets.statuses)) {
      emit({ probeError: 'BENCH_VIZ_BUCKETS missing or unparsable' });
      return;
    }
  }

  const playwright = loadPlaywright();
  browser = await playwright.chromium.launch({
    headless: true,
    args: isViz ? VIZ_LAUNCH_ARGS : [],
  });
  const context = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    ...(isViz ? { reducedMotion: 'reduce' } : {}),      // determinism lever only (R3)
  });
  await context.addInitScript(initFirstDataStamp);
  if (scenario === 'viz') await context.addInitScript(glInstrument);
  if (scenario === 'viz-fallback') await context.addInitScript(glKill);
  if (blockApi) {
    await context.route('**/*', (route) => {
      const url = route.request().url();
      if (route.request().resourceType() === 'document') return route.continue();
      if (/\/(api|data|graphql)(\/|\?|$)/i.test(url) || /\.json(\?|$)/i.test(url))
        return route.abort('connectionrefused');
      return route.continue();
    });
  }
  const page = await context.newPage();

  // QUALITY SCREENSHOTS: same contract as product_probe.mjs — <epoch>-<name>.png per scenario
  // when BENCH_SHOTS_DIR is set; never fatal.
  const shotsDir = process.env.BENCH_SHOTS_DIR || '';
  const shotEpoch = Math.floor(Date.now() / 1000);
  async function saveShot(name) {
    if (!shotsDir) return;
    try {
      mkdirSync(shotsDir, { recursive: true });
      await page.screenshot({ path: join(shotsDir, `${shotEpoch}-${name}.png`), timeout: 5000 });
    } catch (e) {
      err('screenshot failed:', String((e && e.message) || e).slice(0, 200));
    }
  }
  async function saveShotMobile() {
    if (!shotsDir) return;
    try {
      await page.setViewportSize({ width: 375, height: 800 });
      await sleep(400);
      await page.screenshot({
        path: join(shotsDir, `${shotEpoch}-mobile.png`),
        timeout: 5000,
      });
      await page.setViewportSize({ width: 1280, height: 800 });
    } catch (e) {
      err('mobile screenshot failed:', String((e && e.message) || e).slice(0, 200));
    }
  }

  const consoleErrorTexts = [];
  page.on('console', (m) => {
    if (m.type() === 'error') consoleErrorTexts.push(m.text());
  });
  page.on('pageerror', (e) => consoleErrorTexts.push(String(e)));
  const consoleErrors = () => ({
    count: consoleErrorTexts.length,
    texts: consoleErrorTexts.slice(0, 3).map((t) => String(t).slice(0, 300)),
  });

  const clean = (s) =>
    String(s || '')
      // eslint-disable-next-line no-control-regex
      .replace(/\[[0-9;]*m/g, '')
      .split('\n')[0]
      .trim()
      .slice(0, 200);

  async function safeGoto(timeoutMs) {
    try {
      await page.goto(baseUrl, { waitUntil: 'domcontentloaded', timeout: timeoutMs });
      return null;
    } catch (e) {
      const msg = clean(e.message || e);
      err('goto failed:', msg);
      return msg;
    }
  }

  async function evalRetry(fn, fallback) {
    for (let i = 0; i < 2; i++) {
      try {
        return await page.evaluate(fn);
      } catch (e) {
        err('evaluate failed (attempt ' + (i + 1) + '):', clean(e.message || e));
        await sleep(300);
      }
    }
    return fallback;
  }

  async function waitIdle(capMs) {
    try {
      await page.waitForLoadState('networkidle', { timeout: capMs });
    } catch {
      err('networkidle not reached within', capMs, 'ms (continuing)');
    }
  }

  async function pollFirstData(capMs) {
    const deadline = Date.now() + Math.min(capMs, Math.max(budgetLeft() - 20000, 1000));
    while (Date.now() < deadline) {
      const v = await page.evaluate(pageFirstDataMs).catch(() => null);
      if (v != null) return Math.round(v);
      await sleep(100);
    }
    return null;
  }

  if (scenario === 'load') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, consoleErrors: consoleErrors() });
      return;
    }
    const timeToFirstDataMs = await pollFirstData(15000);
    await waitIdle(10000);
    const analysis = await page.evaluate(pageAnalyzeLoad).catch((e) => {
      err('analysis evaluate failed:', e.message);
      return {};
    });
    await saveShot('loaded');

    // v_filter: exercise the status filter (pick one concrete status, then restore All) and
    // emit the RAW before/after/restored table snapshots — the scorer holds the fixture
    // totals, so judgement lives there, not here (F21 raw-inputs discipline).
    let filterExercise = null;
    try {
      const fBefore = await page.evaluate(pageTableStatusSnapshot).catch(() => null);
      const drive = await page.evaluate(pageDriveStatusFilter, { value: 'refunded' })
        .catch(() => null);
      if (!drive || !drive.control) {
        filterExercise = { control: false };
      } else if (!drive.optionFound) {
        filterExercise = { control: true, optionFound: false };
      } else {
        await sleep(700);
        const fAfter = await page.evaluate(pageTableStatusSnapshot).catch(() => null);
        const restore = await page.evaluate(pageDriveStatusFilter, { value: '' })
          .catch(() => null);
        await sleep(700);
        const fRestored = await page.evaluate(pageTableStatusSnapshot).catch(() => null);
        filterExercise = {
          control: true, optionFound: true, picked: 'refunded',
          dataValueAfterPick: drive.dataValue,
          dataValueAfterRestore: restore && restore.optionFound ? restore.dataValue : null,
          before: fBefore, after: fAfter, restored: fRestored,
        };
      }
    } catch (e) {
      err('filter exercise failed:', clean(e.message || e));
    }

    let horizontalScroll = null;
    try {
      await page.setViewportSize({ width: 375, height: 812 });
      await page.reload({ waitUntil: 'domcontentloaded', timeout: 15000 });
      await waitIdle(5000);
      horizontalScroll = await page.evaluate(pageHorizontalScroll);
    } catch (e) {
      err('viewport375 check failed:', e.message.split('\n')[0]);
    }
    await saveShotMobile();
    emit({
      consoleErrors: consoleErrors(),
      timeToFirstDataMs,
      ...analysis,
      filterExercise,
      viewport375: { horizontalScroll },
    });
  } else if (scenario === 'sync') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, found: false, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(10000);
    await pollFirstData(5000);
    const before = await page.evaluate(pageViewSnapshot).catch(() => null);
    const state = await page.evaluate(pageSyncState).catch(() => ({ found: false }));
    if (!state.found) {
      emit({
        found: false,
        disabledDuringSync: null,
        completedWithinMs: null,
        viewRefreshed: null,
        consoleErrors: consoleErrors(),
      });
      return;
    }
    err('sync button found:', JSON.stringify(state.text));
    const clicked = await page.evaluate(pageClickSync).catch(() => false);
    const clickAt = Date.now();
    if (!clicked) {
      emit({
        found: true,
        buttonText: state.text,
        clicked: false,
        disabledDuringSync: null,
        completedWithinMs: null,
        viewRefreshed: null,
        consoleErrors: consoleErrors(),
      });
      return;
    }

    let disabledDuringSync = false;
    while (Date.now() - clickAt < 1200) {
      const s = await page.evaluate(pageSyncState).catch(() => null);
      if (s && (!s.found || s.disabled)) {
        disabledDuringSync = true;
        break;
      }
      await sleep(50);
    }

    let completed = false;
    let completedWithinMs = null;
    let failedAfterMs = null;
    let errorBanner = null;
    let everDisabled = disabledDuringSync;
    let buttonPresentAfter = true;
    const capMs = Math.min(70000, Math.max(budgetLeft() - 8000, 2000));
    while (Date.now() - clickAt < capMs) {
      const s = await page.evaluate(pageSyncState).catch(() => null);
      errorBanner = await page.evaluate(pageErrorBanner).catch(() => null);
      if (s && s.found && s.disabled) everDisabled = true;
      buttonPresentAfter = !!(s && s.found);
      const enabled = s && s.found && !s.disabled;
      const elapsed = Date.now() - clickAt;
      if (enabled && !errorBanner && (everDisabled || elapsed > 1500)) {
        completed = true;
        completedWithinMs = elapsed;
        break;
      }
      // Terminal failure: the app surfaced an error and is no longer working (button
      // re-enabled OR the whole view replaced by the error UI).
      if (errorBanner && everDisabled && (enabled || !buttonPresentAfter)) {
        failedAfterMs = elapsed;
        break;
      }
      await sleep(250);
    }

    let after = await page.evaluate(pageViewSnapshot).catch(() => null);
    let viewRefreshed =
      !!(before && after) &&
      (after.rowCount !== before.rowCount || (after.lastSyncText || '') !== (before.lastSyncText || ''));
    if (!viewRefreshed && before && budgetLeft() > 5000) {
      await sleep(1500);
      after = await page.evaluate(pageViewSnapshot).catch(() => after);
      viewRefreshed =
        !!(before && after) &&
        (after.rowCount !== before.rowCount || (after.lastSyncText || '') !== (before.lastSyncText || ''));
    }
    const tableHashChanged = !!(before && after) && after.tableHash !== before.tableHash;

    await saveShot('synced');
    emit({
      found: true,
      buttonText: state.text,
      disabledDuringSync,
      completed,
      completedWithinMs,
      failedAfterMs,
      buttonPresentAfter,
      errorBanner,
      viewRefreshed,
      tableHashChanged,
      rowCountBefore: before ? before.rowCount : null,
      rowCountAfter: after ? after.rowCount : null,
      lastSyncTextBefore: before ? before.lastSyncText : null,
      lastSyncTextAfter: after ? after.lastSyncText : null,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'error') {
    const navigationError = await safeGoto(15000);
    if (!navigationError) {
      await waitIdle(5000);
      await sleep(1000);
    }
    const banner = await evalRetry(pageErrorBanner, null);
    const blank = await evalRetry(pageBlankAndBody, { blankPage: true, bodyTextLength: 0 });
    await saveShot('error');
    emit({
      navigationError,
      errorStateVisible: banner != null,
      actionableText: banner,
      blankPage: blank.blankPage,
      bodyTextLength: blank.bodyTextLength,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'empty') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, emptyStateVisible: false, renderedRowCount: 0, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(10000);
    let emptyText = null;
    let rowCount = 0;
    const deadline = Date.now() + Math.min(8000, Math.max(budgetLeft() - 10000, 1000));
    while (Date.now() < deadline) {
      emptyText = await page.evaluate(pageEmptyState).catch(() => null);
      const snap = await page.evaluate(pageViewSnapshot).catch(() => null);
      rowCount = snap ? snap.rowCount : 0;
      if (emptyText != null || rowCount > 0) break;
      await sleep(250);
    }
    await saveShot('empty');
    emit({
      emptyStateVisible: emptyText != null,
      emptyStateText: emptyText,
      renderedRowCount: rowCount,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'viz') {
    const emptyRun = buckets.cells.every((c) => !c.count);
    const totalByStatus = {};
    for (const c of buckets.cells)
      totalByStatus[c.status] = (totalByStatus[c.status] || 0) + (c.count || 0);
    merge({ emptyRun });

    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, consoleErrors: consoleErrors() });
      return;
    }

    // F18: readiness poll instead of waitIdle — a health-polling app defeats networkidle.
    let ready = { canvas: false, drawCalls: 0, contexts: 0, vsdbg: false, vsdbgVersion: null };
    const readyDeadline = Date.now() + 15000;
    while (Date.now() < readyDeadline) {
      ready = await page.evaluate(pageVizReady).catch(() => ready);
      if (ready.canvas && (emptyRun || ready.drawCalls > 0)) break;
      await sleep(150);
    }
    await sleep(300);
    merge({ ready });
    const panel = await page.evaluate(pageVizPanelState).catch(() => ({}));
    merge({ panel });

    if (emptyRun) {                          // phantom-data guard: empty db ⇒ #viz-empty, no bars
      const probe = await page.evaluate(pageSampleScene,
        { samples: Array.from({ length: 24 }, (_, k) =>
            ({ cx: (k % 6 + 0.5) * 100, cy: (Math.floor(k / 6) + 0.5) * 80, kind: 'grid' })),
          tol: VIZ.tol }).catch(() => null);
      const dbg = await page.evaluate(pageVsdbgSnapshot, { points: [], picks: [] }).catch(() => null);
      await saveShot('viz');
      emit({
        vsdbgBarCount: dbg && dbg.scene && Array.isArray(dbg.scene.bars) ? dbg.scene.bars.length : null,
        gridGot: probe && probe.samples ? probe.samples.map((s) => s.got) : null,
        consoleErrors: consoleErrors(),
      });
      return;
    }

    // aspect from the MEASURED rect (R2): compute expectations after reading it
    const pre = await page.evaluate(pageSampleScene, { samples: [], tol: VIZ.tol })
      .catch((e) => { err('pre-sample evaluate failed:', clean(e.message || e)); return null; });
    if (!pre || !pre.found || !pre.glReadable) {
      await saveShot('viz');
      emit({ glReadable: !!(pre && pre.glReadable), canvasFound: !!(pre && pre.found),
             ...(pre ? {} : { samplingError: 'pre-sample evaluate failed' }),
             consoleErrors: consoleErrors() });
      return;
    }
    const W = pre.rect.w, H = pre.rect.h;
    const backingOk = Math.abs(pre.backing.w - Math.round(W * pre.dpr)) <= 1 &&
                      Math.abs(pre.backing.h - Math.round(H * pre.dpr)) <= 1;
    merge({ backingOk, canvasRect: { w: W, h: H }, dpr: pre.dpr });

    // Static analytic sampling at the default camera (readPixels needs no viewport visibility).
    const S = buildVizSamples(buckets, VIZ.yaw0, VIZ.pitch0, VIZ.dist0, W, H);
    const all = [...S.tops, ...S.above, ...S.sides, ...S.sky, ...S.corners, ...S.grid];
    const r1 = await page.evaluate(pageSampleScene, { samples: all, tol: VIZ.tol })
      .catch((e) => { err('scene-sample evaluate failed:', clean(e.message || e)); return null; });
    if (!r1 || !Array.isArray(r1.samples)) {
      // F18: an RPC failure here is harness, not app — ship what was measured, leave the
      // binding section ABSENT so the scorer reads it as PROBE UNAVAILABLE, never as 0.
      merge({ samplingError: 'scene-sample evaluate failed' });
      await saveShot('viz');
      emit({ consoleErrors: consoleErrors() });
      return;
    }
    const cnt = (k) => { const a = (r1.samples || []).filter((s) => s.kind === k);
                        return { ok: a.filter((s) => s.ok).length, total: a.length }; };
    const gridSamples = (r1.samples || []).filter((s) => s.kind === 'grid');
    const gridGot = gridSamples.map((s) => s.got);
    const isBg = (c) => c.every((v, ix) => Math.abs(v - VIZ.bg[ix]) <= VIZ.tol);
    const nonBg = gridGot.filter((c) => !isBg(c));
    const topGot = (r1.samples || []).filter((s) => s.kind === 'top' && s.ok).map((s) => s.got.join(','));
    merge({
      gl: r1.instrument || null, glReadable: true,
      // p_first_frame: the glInstrument stamp of the FIRST draw call, on the page's own
      // performance.now() clock (origin = navigation start) — clears never count.
      firstDrawMs: r1.instrument && Array.isArray(r1.instrument.drawTs) && r1.instrument.drawTs.length
        ? +r1.instrument.drawTs[0].toFixed(1) : null,
      contextCheck: { gridNonBg: nonBg.length, gridTotal: gridGot.length,
                      gridDistinct: new Set(nonBg.map((c) => c.join(','))).size,
                      corners: cnt('corner') },
      binding: { tops: cnt('top'), above: cnt('above'), sides: cnt('side'), sky: cnt('sky'),
                 occludedTops: S.occludedTops.length,
                 skippedSmall: S.skippedSmall.length,               // F12
                 statusesExpected: new Set(S.bars.map((b) => b.status)).size,
                 distinctTopColors: new Set(topGot).size },
    });

    // vsdbg truth: scene ≡ buckets, project ≡ probe math, camera ≡ defaults (F11 angular),
    // pick spots scaled to what is actually visible (F23).
    const spotStride = Math.max(1, Math.ceil(S.bars.length / 6));
    const spotBars = S.bars.filter((_, k) => k % spotStride === 0).slice(0, 6);
    const pickSpots = S.tops.slice(0, Math.min(4, S.tops.length));
    const dbg1 = await page.evaluate(pageVsdbgSnapshot,
      { points: spotBars.map((b) => [b.x, b.h, b.z]),
        picks: pickSpots.map((t) => [t.cx, t.cy]) }).catch(() => null);
    let vsdbgProjErr = null, vsdbgSceneOk = null, vsdbgPickOk = null, vsdbgCam = null;
    if (dbg1 && dbg1.scene && Array.isArray(dbg1.scene.bars)) {
      const want = new Map(S.bars.map((b) => [b.key, b]));
      const claims = dbg1.scene.bars;
      const matched = claims.filter((c) => { const w = want.get(c.key);
        return w && c.count === w.count && Math.abs(c.x - w.x) < 1e-6 &&
               Math.abs(c.z - w.z) < 1e-6 && Math.abs(c.h - w.h) < 1e-6; }).length;
      vsdbgSceneOk = { matched, claimed: claims.length, expected: want.size };
      const errs = spotBars.map((b, ix) => {
        const got = dbg1.projections[ix];
        const exp = projectPt(S.eye, S.basis, W, H, [b.x, b.h, b.z]);
        return Array.isArray(got) && exp ? Math.hypot(got[0] - exp.x, got[1] - exp.y) : 999;
      });
      vsdbgProjErr = errs.length ? +Math.max(...errs).toFixed(2) : null;
      vsdbgPickOk = { ok: dbg1.picks.filter((k, ix) => pickSpots[ix] && k === pickSpots[ix].key).length,
                      total: dbg1.picks.length };
    }
    if (dbg1 && dbg1.camera && !dbg1.camera.__err && dbg1.camera.yaw != null) {
      vsdbgCam = { ...dbg1.camera,
                   yawErrDeg: +angDist(dbg1.camera.yaw, VIZ.yaw0).toFixed(2),
                   pitchErr: dbg1.camera.pitch != null ? +Math.abs(dbg1.camera.pitch - VIZ.pitch0).toFixed(2) : null,
                   distErr: dbg1.camera.distance != null ? +Math.abs(dbg1.camera.distance - VIZ.dist0).toFixed(2) : null };
    }
    merge({ vsdbg: { present: !!dbg1, version: dbg1 ? dbg1.version : null,
                     sceneOk: vsdbgSceneOk, projMaxErrPx: vsdbgProjErr, picks: vsdbgPickOk,
                     camera: vsdbgCam } });

    // F22 differential half: is the "webgl unavailable"-style notice visible in the NORMAL run?
    const noticeViz = await page.evaluate(pageVizNotice).catch(() => null);
    merge({ noticeInViz: noticeViz ? noticeViz.notice : null });

    // ── interaction phase — F6: scroll the canvas into view, re-measure, refuse if clipped ──
    await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
    await sleep(200);
    let rect = await page.evaluate(pageCanvasRect).catch(() => null);
    const inViewport = rect && rect.top >= -0.5 && rect.left >= -0.5 &&
      rect.bottom <= rect.viewportH + 0.5 && rect.right <= rect.viewportW + 0.5;
    if (!rect || !inViewport) {
      merge({ interaction: { probeError: 'canvas not fully inside viewport after scrollIntoView',
                             rect } });
      await saveShot('viz');
      emit({ consoleErrors: consoleErrors() });
      return;
    }
    merge({ rectAfterScroll: { left: rect.left, top: rect.top, w: rect.w, h: rect.h } });

    // tooltip — F10b page-side stamps, F23 largest-area target instead of index 0
    let tooltip = { shown: false };
    const ttCandidates = [...S.tops].sort((a, b) => (b.area || 0) - (a.area || 0));
    if (ttCandidates.length) {
      const tt = ttCandidates[0];
      await page.evaluate(pageArmTooltipWatch).catch(() => {});
      await page.mouse.move(rect.left + tt.cx, rect.top + tt.cy);
      const tStart = Date.now();
      while (Date.now() - tStart < 1200) {
        const ts = await page.evaluate(pageTooltipState).catch(() => ({ visible: false }));
        if (ts.visible) {
          const stamp = await page.evaluate(pageReadTooltipWatch).catch(() => null);
          tooltip = { shown: true,
                      ms: stamp && stamp.ms != null ? stamp.ms : Date.now() - tStart,
                      msSource: stamp && stamp.ms != null ? 'page-stamp' : 'probe-wall',
                      text: ts.text,
                      wantPrefix: `${S.bars[tt.i].count} ${tt.status}` };
          break;
        }
        await sleep(40);
      }
      await page.mouse.move(rect.left + 2, rect.top + rect.h + 40);   // off-canvas: must hide
      await sleep(250);
      const ts2 = await page.evaluate(pageTooltipState).catch(() => ({ visible: false }));
      tooltip.hides = !ts2.visible;
    }
    merge({ tooltip });

    // picking — F1: data-value only, plus the table counter-assertion after every pick
    const stride = Math.max(1, Math.ceil(S.tops.length / 5));
    const targets = S.tops.filter((_, k) => k % stride === 0).slice(0, 5);
    const picks = [];
    for (const t of targets) {
      await page.mouse.click(rect.left + t.cx, rect.top + t.cy);
      const pStart = Date.now();
      let got = null;
      while (Date.now() - pStart < 1200) {
        got = await page.evaluate(pageStatusFilterValue).catch(() => null);
        if (got === t.status) break;
        await sleep(80);
      }
      await sleep(300);                       // table refresh settle
      const snap = await page.evaluate(pageTableStatusSnapshot).catch(() => null);
      const rowsUniform = !!snap && snap.rowCount > 0 &&
        snap.statuses.every((s) => s === t.status);
      const totalOk = !!snap && snap.readoutTotal === (totalByStatus[t.status] || 0);
      picks.push({ want: t.status, got, ok: got === t.status,
                   rowsUniform, totalOk, tableTotal: snap ? snap.readoutTotal : null });
    }
    // depth pair on a cross-status occlusion, largest occluded-top area first (F23)
    let depthPair = { available: false, ok: null };
    const dpCands = S.occludedTops
      .filter((o) => {
        const b = S.bars.find((x) => x.key === o.key);
        return b && S.bars[o.occluder].status !== b.status;
      })
      .sort((a, b) => (b.area || 0) - (a.area || 0));
    if (dpCands.length) {
      const dp = dpCands[0];
      await page.mouse.click(rect.left + dp.css.x, rect.top + dp.css.y);
      await sleep(400);
      const got = await page.evaluate(pageStatusFilterValue).catch(() => null);
      const snap = await page.evaluate(pageTableStatusSnapshot).catch(() => null);
      const want = S.bars[dp.occluder].status;
      depthPair = { available: true, want, got, ok: got === want,
                    rowsUniform: !!snap && snap.rowCount > 0 && snap.statuses.every((s) => s === want),
                    totalOk: !!snap && snap.readoutTotal === (totalByStatus[want] || 0) };
    }
    merge({ picking: { picks, correct: picks.filter((p) => p.ok).length, total: picks.length,
                       depthPair } });

    // 2D toggle: #viz-toggle → #viz-fallback cells must equal expected buckets
    let toggle = { present: false };
    if (panel.toggle) {
      await page.click('#viz-toggle').catch(() => {});
      await sleep(300);
      const ft = await page.evaluate(pageFallbackTable).catch(() => ({ visible: false }));
      if (ft.visible) {
        const nonZero = buckets.cells.filter((c) => c.count > 0);
        const okCells = nonZero.filter((c) => ft.cells[`${c.day}|${c.status}`] === c.count).length;
        toggle = { present: true, visible: true, okCells, totalCells: nonZero.length };
      } else toggle = { present: true, visible: false };
      await page.click('#viz-toggle').catch(() => {});
      await sleep(300);
    }
    merge({ toggle });

    // camera: wheel → reprojection at the computed distance. F6: rect re-measured before AND
    // after — an app that lets the wheel scroll the page is caught, and all later mouse math
    // uses the fresh rect. F10b: camera-visible latency from page-side draw stamps.
    rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
    let cx0 = rect.left + rect.w / 2, cy0 = rect.top + rect.h / 2;
    await page.mouse.move(cx0, cy0);
    await page.evaluate(pageArmDrawWatch, { kinds: ['wheel'] }).catch(() => {});
    await page.mouse.wheel(0, 400);                       // dist 30·exp(0.48) ≈ 48.5
    await sleep(300);
    const wheelLat = await page.evaluate(pageReadDrawLatency).catch(() => null);
    const rectAfterWheel = await page.evaluate(pageCanvasRect).catch(() => null);
    const pageScrolledOnWheel = !!(rectAfterWheel && Math.abs(rectAfterWheel.top - rect.top) > 1);
    if (pageScrolledOnWheel) {                            // recover: re-center before continuing
      await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
      await sleep(200);
    }
    rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
    cx0 = rect.left + rect.w / 2; cy0 = rect.top + rect.h / 2;
    const distT = Math.min(VIZ.distMax, VIZ.dist0 * Math.exp(VIZ.wheelK * 400));
    const Sw = buildVizSamples(buckets, VIZ.yaw0, VIZ.pitch0, distT, W, H);
    const rw = await page.evaluate(pageSampleScene, { samples: Sw.tops, tol: VIZ.tol })
      .catch(() => ({ samples: [] }));
    const f = (arr) => { const a = arr.filter((s) => s.ok).length; return { ok: a, total: arr.length }; };
    merge({ wheel: { proj: f(rw.samples || []), expectedDist: +distT.toFixed(2),
                     paintMs: wheelLat ? wheelLat.ms : null,
                     pageScrolled: pageScrolledOnWheel } });

    await page.mouse.dblclick(cx0, cy0);                  // reset before the drag
    await sleep(250);

    // drag — F4: 60 delivered move events over ~2 s (frames-per-drag is the unit, the spec's
    // event-driven renderer draws per move); F9: a mid-drag framebuffer sample proves the
    // scene actually left baseline, gating the reset anchor against a fully static app.
    const dbgF0 = await page.evaluate(pageVsdbgSnapshot, { points: [], picks: [] }).catch(() => null);
    const draws0 = (await page.evaluate(pageVizReady).catch(() => ({ drawCalls: 0 }))).drawCalls;
    const MOVES = 60;
    const baselineFive = S.tops.slice(0, 5).map((t) => ({ cx: t.cx, cy: t.cy, kind: 'mid' }));
    const baselineFiveGot = (r1.samples || [])
      .filter((s) => s.kind === 'top').slice(0, 5).map((s) => s.got);
    let midGot = null;
    const tDrag = Date.now();
    await page.mouse.move(cx0, cy0);
    await page.mouse.down();
    for (let step = 1; step <= MOVES; step++) {           // +2px per move ⇒ +120px total
      await page.mouse.move(cx0 + step * 2, cy0, { steps: 1 });
      if (step === Math.floor(MOVES / 2) && baselineFive.length) {
        const rm = await page.evaluate(pageSampleScene, { samples: baselineFive, tol: VIZ.tol })
          .catch(() => null);
        midGot = rm && rm.samples ? rm.samples.map((s) => s.got) : null;
      }
      await sleep(28);
    }
    await page.mouse.up();
    const dragWallMs = Date.now() - tDrag;
    await sleep(250);
    const dbgF1 = await page.evaluate(pageVsdbgSnapshot, { points: [], picks: [] }).catch(() => null);
    const draws1 = (await page.evaluate(pageVizReady).catch(() => ({ drawCalls: 0 }))).drawCalls;
    const movedDuringDrag = !!(midGot && baselineFiveGot.length) &&
      midGot.some((g, ix) => baselineFiveGot[ix] &&
        g.some((v, ch) => Math.abs(v - baselineFiveGot[ix][ch]) > VIZ.tol));

    const yawT = VIZ.yaw0 - 120 * VIZ.dragDegPerPx;       // spec: yaw ← yaw − 0.35·Δx
    const S2 = buildVizSamples(buckets, yawT, VIZ.pitch0, VIZ.dist0, W, H);
    const S2f = buildVizSamples(buckets, VIZ.yaw0 + 120 * VIZ.dragDegPerPx, VIZ.pitch0, VIZ.dist0, W, H);
    const r2 = await page.evaluate(pageSampleScene,
      { samples: [...S2.tops, ...S2f.tops.map((s) => ({ ...s, kind: 'topflip' }))], tol: VIZ.tol })
      .catch(() => ({ samples: [] }));
    const camAfter = dbgF1 && dbgF1.camera && !dbgF1.camera.__err ? dbgF1.camera : null;
    const framesDelta = dbgF0 && dbgF1 && Number.isFinite(dbgF1.frames) && Number.isFinite(dbgF0.frames)
      ? dbgF1.frames - dbgF0.frames : null;

    await page.mouse.dblclick(cx0, cy0);                  // reset → baseline must restore
    await sleep(250);
    const r3 = await page.evaluate(pageSampleScene, { samples: S.tops, tol: VIZ.tol })
      .catch(() => ({ samples: [] }));

    merge({
      drag: {
        proj: f((r2.samples || []).filter((s) => s.kind === 'top')),
        projFlipped: f((r2.samples || []).filter((s) => s.kind === 'topflip')),
        cameraAfter: camAfter, expectedYaw: +yawT.toFixed(1),
        yawErrDeg: camAfter && camAfter.yaw != null
          ? +angDist(camAfter.yaw, yawT).toFixed(2) : null,           // F11
        framesDelta,
        movesDelivered: MOVES,                                         // F4: frames-per-drag unit
        framesPerMove: framesDelta != null ? +(framesDelta / MOVES).toFixed(3) : null,
        drawCallsDelta: draws1 - draws0, wallMs: dragWallMs,
        movedDuringDrag,                                               // F9: gates reset credit
        reset: f(r3.samples || []),
      },
    });

    await saveShot('viz');
    emit({ consoleErrors: consoleErrors() });

  } else if (scenario === 'viz-fallback') {
    // context created with addInitScript(glKill): WebGL null, canvas-2D alive.
    const navigationError = await safeGoto(20000);
    if (!navigationError) { await waitIdle(8000); await sleep(800); }
    const panel = await page.evaluate(pageVizPanelState).catch(() => ({ canvas: false }));
    const notice = await page.evaluate(pageVizNotice).catch(() => ({ notice: null, nearViz: false }));
    const ft = await page.evaluate(pageFallbackTable).catch(() => ({ visible: false }));
    const nonZero = buckets.cells.filter((c) => c.count > 0);
    const okCells = ft.visible
      ? nonZero.filter((c) => ft.cells[`${c.day}|${c.status}`] === c.count).length : 0;
    const snap = await page.evaluate(pageViewSnapshot).catch(() => null);
    await saveShot('viz-fallback');
    emit({
      navigationError, ...panel,
      notice: notice.notice, noticeNearViz: notice.nearViz,            // F22
      fallbackTable: { visible: !!ft.visible, okCells, totalCells: nonZero.length },
      renderedRowCount: snap ? snap.rowCount : 0,
      consoleErrors: consoleErrors(),
    });

  } else if (scenario === 'note') {
    // F10a: the note POST is HELD 800 ms via page.route, so "the cell painted while the
    // request was provably pending" is a causal fact, not a race against a <10 ms backend.
    const NOTE_HOLD_MS = 800;
    const noteNet = { requests: 0, releasedAt: null };
    await page.route('**/api/payments/**', async (route) => {
      const req = route.request();
      if (req.method() === 'POST' && /\/api\/payments\/[^/]+\/note(\?|$)/.test(req.url())) {
        noteNet.requests++;
        await sleep(NOTE_HOLD_MS);
        noteNet.releasedAt = Date.now();
        return route.continue().catch(() => {});
      }
      return route.continue().catch(() => {});
    });
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, editorOpened: false, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(10000);
    await pollFirstData(8000);

    const cell = await page.evaluate(pageNoteCellRect).catch(() => ({ found: false }));
    if (!cell.found) {
      emit({ editorOpened: false, noteCellFound: false, consoleErrors: consoleErrors() });
      return;
    }
    merge({ noteCellFound: true, headerMatched: !!cell.headerMatched });
    await sleep(200);
    // re-measure after the page-side scrollIntoView (F6 discipline applies here too)
    const cell2 = await page.evaluate(pageNoteCellRect).catch(() => cell);
    await page.mouse.click(cell2.x, cell2.y);
    let editor = { found: false };
    let deadline = Date.now() + 800;
    while (Date.now() < deadline) {
      editor = await page.evaluate(pageFindNoteEditor).catch(() => ({ found: false }));
      if (editor.found) break;
      await sleep(100);
    }
    if (!editor.found) {                                  // some editors open on double-click
      await page.mouse.dblclick(cell2.x, cell2.y);
      deadline = Date.now() + 800;
      while (Date.now() < deadline) {
        editor = await page.evaluate(pageFindNoteEditor).catch(() => ({ found: false }));
        if (editor.found) break;
        await sleep(100);
      }
    }
    if (!editor.found) {
      emit({ editorOpened: false, noteCellFound: true, consoleErrors: consoleErrors() });
      return;
    }
    merge({ editorOpened: true, editorKind: editor.kind });

    const newValue = 'probe note ' + shotEpoch;
    await page.keyboard.press('ControlOrMeta+a').catch(() => {});
    await page.keyboard.type(newValue, { delay: 5 }).catch(() => {});
    await page.evaluate(pageArmNoteWatch, { value: newValue }).catch(() => {});
    await page.keyboard.press('Enter').catch(() => {});
    const confirmAt = Date.now();

    await sleep(250);                                     // well inside the 800 ms hold window
    const mid = await page.evaluate(pageNoteMidState, { value: newValue }).catch(() => null);
    const heldDuringCheck = noteNet.requests > 0 && noteNet.releasedAt == null;
    merge({
      requestSeen: noteNet.requests,
      heldDuringCheck,
      paintedWhileHeld: !!(mid && mid.painted) && heldDuringCheck,
      savingWhileHeld: !!(mid && mid.saving) && heldDuringCheck,
    });

    deadline = Date.now() + NOTE_HOLD_MS + 3000;          // wait out the hold, then the save
    while (Date.now() < deadline && noteNet.requests > 0 && noteNet.releasedAt == null)
      await sleep(100);
    let after = null;
    deadline = Date.now() + 2000;
    while (Date.now() < deadline) {
      after = await page.evaluate(pageNoteAfterState, { value: newValue }).catch(() => null);
      if (after && after.saved) break;
      await sleep(150);
    }
    const stamp = await page.evaluate(pageReadNoteWatch).catch(() => null);
    await saveShot('note');
    emit({
      savedAfterRelease: !!(after && after.saved),
      valuePersisted: !!(after && after.painted),
      paintMs: stamp ? stamp.ms : null,                   // F10b page-side stamp
      paintPreexisting: !!(stamp && stamp.preexisting),   // contenteditable draft caveat
      holdMs: NOTE_HOLD_MS,
      confirmToEmitMs: Date.now() - confirmAt,
      consoleErrors: consoleErrors(),
    });

  } else if (scenario === 'under-load') {
    // Frontend-under-load hook: reads exercised WHILE a sync is in flight, latencies stamped
    // page-side. The API-side p95 with 8 concurrent readers stays owned by perf_probe; this
    // scenario proves the PAGE keeps answering, and reports how many interactions actually
    // overlapped the sync — an unproven overlap licenses nothing (the F8 principle).
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, syncStarted: false, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(10000);
    await pollFirstData(8000);
    const before = await page.evaluate(pageViewSnapshot).catch(() => null);
    const clicked = await page.evaluate(pageClickSync).catch(() => false);
    merge({ syncStarted: clicked, rowsBefore: before ? before.rowCount : null });
    if (!clicked) {
      emit({ consoleErrors: consoleErrors() });
      return;
    }
    await sleep(150);
    const interactions = [];
    const loopDeadline = Date.now() + 15000;
    while (Date.now() < loopDeadline && interactions.length < 8) {
      const inFlight = await page.evaluate(pageSyncInFlight).catch(() => false);
      await page.evaluate(pageArmTableWatch).catch(() => {});
      const which = interactions.length % 2 === 0 ? 'next' : 'prev';
      const ok = await page.evaluate(pageClickPager, { which }).catch(() => false);
      await sleep(600);
      const wch = await page.evaluate(pageReadTableWatch).catch(() => null);
      const snap = await page.evaluate(pageViewSnapshot).catch(() => null);
      interactions.push({
        which, clicked: ok, duringSync: inFlight,
        latencyMs: wch ? wch.ms : null,
        rows: snap ? snap.rowCount : 0,
      });
      if (!inFlight && interactions.length >= 4) break;   // sync over and enough samples
    }
    const overlapped = interactions.filter((x) => x.clicked && x.duringSync).length;
    const lats = interactions.filter((x) => x.clicked && x.duringSync && x.latencyMs != null)
      .map((x) => x.latencyMs).sort((a, b) => a - b);
    const p95 = lats.length ? lats[Math.min(lats.length - 1, Math.floor(0.95 * lats.length))] : null;
    // wait for the sync to finish (bounded) so rowsAfter reflects the completed state
    const syncDeadline = Date.now() + Math.min(60000, Math.max(budgetLeft() - 8000, 2000));
    while (Date.now() < syncDeadline) {
      const inFlight = await page.evaluate(pageSyncInFlight).catch(() => false);
      if (!inFlight) break;
      await sleep(400);
    }
    const afterSnap = await page.evaluate(pageViewSnapshot).catch(() => null);
    await saveShot('under-load');
    emit({
      interactions, overlapped, interactionsTotal: interactions.length,
      p95LatencyMs: p95,
      rowsAfter: afterSnap ? afterSnap.rowCount : null,
      consoleErrors: consoleErrors(),
    });
  }
}

main()
  .then(async () => {
    clearTimeout(hardTimer);
    if (!printed) emit({ probeIncomplete: true });
    try {
      await Promise.race([browser && browser.close(), sleep(3000)]);
    } catch {}
  })
  .catch(async (e) => {
    clearTimeout(hardTimer);
    err('PROBE CRASH:', e && e.stack ? e.stack : e);
    try {
      await Promise.race([browser && browser.close(), sleep(1500)]);
    } catch {}
    process.exit(1);
  });
