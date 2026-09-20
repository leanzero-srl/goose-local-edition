#!/usr/bin/env node
// product_probe_v3.mjs — sb-7 browser-truth probe for the Meridian payments console.
// Usage: node product_probe_v3.mjs <scenario> <baseUrl> [--block-api] | --selfcheck
// Prints EXACTLY ONE JSON object to stdout; all diagnostics go to stderr.
// Exit 0 always, including failed checks and the hard cap (timedOut:true);
// nonzero exit only when the probe itself crashes (or --selfcheck fails).
//
// SCENARIO TABLE — THIS list is the single authority (score_sb7.py's gather invokes a subset
// of exactly these names and must keep to them): boot | load | sync | flow | error | viz | feed.
// gather stores the boot emit under its "empty" evidence key; coast + stream evidence ride
// the viz emit (coast/stream/d1/streamApplied sections) — there are no separate scenarios.
//   boot    pre-sync-#1 first paint: D3 corner evidence (empty-with-progress
//           vs block), #notifications state (evidence key: preSyncState)     → j_empty_state, d_decisions_doc
//   load    first-use journey: rendered rows (evidence key: tableRendered),
//           money text, status styling, filters, pagination, 375px
//           responsive, notifications state, page weight                     → j_first_use, V tier
//   sync    #sync-now causal journey (evidence key: syncCausal)              → j_sync_journey
//   flow    approval workflow through the UI with role tokens (evidence
//           keys: roleTokenAccepted, draftCreated, draftListStates,
//           approveCausal, rejectCausal): F1 approve with the held-POST
//           optimistic-paint proof, F2 reject, D2 resubmit probe             → j_workflow_journey, j_workflow_reject
//   error   vendor/api down: app's own error UI, degraded notifications      → j_error_state
//   viz     the 3D instanced field, ONE session, incremental sections (§3):
//           contextReal, layout, digest, heightPixels (R9), gl (wrapper v2
//           counters), dragBudget (pinned window), idle (demand rendering),
//           picks + pickCounters (evidence key: pickOccluded), labels
//           (evidence key: labelSetExact), brush (evidence keys:
//           brushHighlight, brushCount), coast + cameraMath (τ=0.4 closed
//           form, cadence fact R8), stream (evidence key: streamApplied —
//           SSE diff byte accounting, D1), vs7dbgTruth                       → T tier, P idle/drag/apply
//   feed    #notifications degraded→live across the notifier heal, no
//           reload (evidence key: degradedThenLive)                          → j_notifications_feed
//
// ENV CONTRACT:
//   SB7_EXPECT_FILE   path to the expectation-pack JSON (fixtures_v3 PROBE_EXPECT, written by
//                     the harness; legacy alias BENCH_SB7_EXPECT accepted). REQUIRED for viz
//                     (F17: missing ⇒ probeError refuse, never improvise). Shape:
//                       { seed: "<16 hex>", count: N,
//                         records: { id:[], amount_minor:[], currency:[], status:[], day:[] },
//                           // columnar, load serve order (created_at instant ASC, id ASC) — §3.1
//                         tokens?:      { maker, checker, admin },
//                         approval?:    { drafts: [{amount_minor, currency, counterparty, note}, …] },
//                         heightCases?: [id × 6, ≥1 JPY and ≥1 KWD]   (derived from records if absent)
//                         labelCandidates?: [id × 12]                 (derived from records if absent)
//                         stream?:      { mutateIds: [...] } }
//   BENCH_SB7_TOKENS  JSON {maker, checker, admin} — flow scenario; falls back to pack.tokens.
//   BENCH_SHOTS_DIR   screenshot dir: <epoch>-<name>.png per scenario; never fatal.
//
// House rules carried from product_probe_v2.mjs (sb-6): playwright resolution ladder,
// SwiftShader-deterministic launch args, one-JSON emit with phase merge (F18), hard cap ships
// partials, rendered-means-seen visibility filters, causal-evidence discipline with pre-click
// baselines, screenshots to BENCH_SHOTS_DIR. The probe EMITS FACTS; score_sb7.py judges.
// sb-7 additions per sb7/DESIGN.md: §3.1 layout/digest/height-pixel (R9), §3.2 wrapper v2 with
// proxied extensions + bufferSubData accounting + FBO classification (R6), pinned budget window
// + first-class demand rendering (R5), §3.3 pick buffer with decisive targets and occlusion
// constructions (R4), §3.4 coast law with pinned drag timing (R7) and measured cadence (R8),
// §3.5 decisive-pose label culling, §3.6 brush doors, §3.7 stream diff byte accounting.

import { createRequire } from 'module';
import { execSync, execFileSync } from 'child_process';
import { createHash } from 'crypto';
import { inflateSync } from 'zlib';
import { join, dirname, relative, resolve } from 'path';
import { mkdirSync, readFileSync, writeFileSync, statSync } from 'fs';

const err = (...a) => console.error('[probe]', ...a);

function loadPlaywright() {
  const attempts = [];
  if(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE) return createRequire(import.meta.url)(process.env.GOOSE_SWARM_PLAYWRIGHT_MODULE);
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

// ── sb-7 pinned contract constants (must mirror sb7/DESIGN.md §3 verbatim) ───────────────────
const V7 = {
  fovYDeg: 50, near: 0.5, far: 1000, target: [0, 1, 0],
  yaw0: 30, pitch0: 40, dist0: 260,
  dragDegPerPx: 0.30, wheelK: 0.0012,
  pitchMin: 5, pitchMax: 85, distMin: 15, distMax: 340,
  tau: 0.4, stopDegPerS: 2,
  pitch: 1.2, half: 0.45,                                // Δ cell pitch; footprint 0.9×0.9
  hBase: 0.9, hK: 0.55, hMin: 0.2, hMax: 4.2,
  D0: 96,                                                 // frozen span (§2.3)
  exp: { EUR: 2, USD: 2, JPY: 0, KWD: 3 },
  bg: [16, 24, 40],                                       // #101828
  status: { settled: [5, 150, 105], pending: [217, 119, 6],
            refunded: [124, 58, 237], failed: [185, 28, 28] },
  sideF: 0.55, dimF: 0.30, tol: 8,
  drawBudget: 8, budgetMoves: 40,                         // §3.2: ΔD ≤ 8·max(ΔF,1) AND ≤ 8·(M+8)
  pickPassBudget: 4,                                      // §3.3 offscreen draws per refresh
  labelW: 110, labelH: 18, labelDx: 10, labelDy: -9, labelSlopPx: 2, labelCandidates: 12,
  depthGapNdc: 0.002, lateralPx: 3, pairMarginPx: 5,
  idleWindowMs: 500,
};
const VIZ_LAUNCH_ARGS = [
  '--use-angle=swiftshader', '--enable-unsafe-swiftshader',   // deterministic software GL (verified sb-6)
  '--force-color-profile=srgb', '--force-device-scale-factor=1',
  '--js-flags=--random-seed=1357',
];
const sideColor = (c) => c.map((v) => Math.round(v * V7.sideF));
const dimColor = (c) => c.map((v) => Math.round(v * V7.dimF));

// ── math (spec-basis form, carried structurally from v2; near/target/fov are sb-7's) ─────────
const deg = (d) => (d * Math.PI) / 180;
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (a) => { const l = Math.hypot(...a); return [a[0] / l, a[1] / l, a[2] / l]; };
const angDist = (a, b) => { const d = Math.abs(a - b) % 360; return Math.min(d, 360 - d); };
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

function cameraEye(yaw, pitch, dist) {
  const t = deg(yaw), p = deg(pitch), T = V7.target;
  return [T[0] + dist * Math.cos(p) * Math.sin(t),
          T[1] + dist * Math.sin(p),
          T[2] + dist * Math.cos(p) * Math.cos(t)];
}
function cameraBasis(eye) {
  const f = norm(sub(V7.target, eye));
  const r = norm(cross(f, [0, 1, 0]));
  const u = cross(r, f);
  return { f, r, u };
}
// world point → CSS px (canvas top-left), or null when zc ≤ near (§3.4: zc ≤ 0.5 does not project)
function projectPt(eye, basis, W, H, p) {
  const q = sub(p, eye);
  const xc = dot(q, basis.r), yc = dot(q, basis.u), zc = dot(q, basis.f);
  if (zc <= V7.near) return null;
  const k = 1 / Math.tan(deg(V7.fovYDeg) / 2), aspect = W / H;
  return { x: ((k / aspect) * (xc / zc) + 1) / 2 * W,
           y: (1 - k * (yc / zc)) / 2 * H, zc };
}
function unprojectDir(basis, W, H, sx, sy) {
  const k = 1 / Math.tan(deg(V7.fovYDeg) / 2), aspect = W / H;
  const cx = ((sx / W) * 2 - 1) / (k / aspect);
  const cy = (1 - (sy / H) * 2) / k;
  return norm([basis.f[0] + cx * basis.r[0] + cy * basis.u[0],
               basis.f[1] + cx * basis.r[1] + cy * basis.u[1],
               basis.f[2] + cx * basis.r[2] + cy * basis.u[2]]);
}
// GL NDC depth of a forward view distance zc (near 0.5 / far 1000): −1 at near, +1 at far.
function ndcDepth(zc) {
  const n = V7.near, f = V7.far;
  return (f + n) / (f - n) - (2 * f * n) / ((f - n) * zc);
}
function rayBox(o, d, mn, mx) {                        // slab test: entry distance, or null
  let t0 = -Infinity, t1 = Infinity;
  for (let k = 0; k < 3; k++) {
    if (Math.abs(d[k]) < 1e-9) { if (o[k] < mn[k] || o[k] > mx[k]) return null; continue; }
    let a = (mn[k] - o[k]) / d[k], b = (mx[k] - o[k]) / d[k];
    if (a > b) [a, b] = [b, a];
    t0 = Math.max(t0, a); t1 = Math.min(t1, b);
  }
  return t0 <= t1 && t1 > 0 ? Math.max(t0, 0) : null;
}

// Deterministic choices: every probe-chosen target/pose derives from the run seed (§4.1 spirit).
function seedRng(seedHex, label) {
  let h = 2166136261 >>> 0;
  const s = String(seedHex) + ':' + label;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; }
  return function mulberry32() {
    h = (h + 0x6D2B79F5) >>> 0;
    let t = h;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ── expectation model from the pack (§3.1: layout basis, transforms, digest) ─────────────────
const dayMs = 86400000;
const dayEpoch = (s) => {                               // "YYYY-MM-DD" → UTC day number (DST-free)
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(s));
  if (!m) return null;
  return Date.UTC(+m[1], +m[2] - 1, +m[3]) / dayMs;
};

function buildModel(pack) {
  const R = pack.records;
  const n = R.id.length;
  if (![R.amount_minor, R.currency, R.status, R.day].every((a) => Array.isArray(a) && a.length === n))
    throw new Error('pack.records arrays are not equal-length');
  let d0 = null;
  for (const d of R.day) if (d0 === null || d < d0) d0 = d;
  const e0 = dayEpoch(d0);
  const perDay = new Map();
  const items = new Array(n);
  const byId = new Map();
  for (let i = 0; i < n; i++) {
    const cur = R.currency[i];
    const exp = V7.exp[cur];
    if (exp === undefined) throw new Error('unknown currency in pack: ' + cur);
    const d = dayEpoch(R.day[i]) - e0;
    const r = perDay.get(d) || 0;
    perDay.set(d, r + 1);
    items[i] = { n: i, id: R.id[i], amount_minor: R.amount_minor[i], cur, status: R.status[i],
                 day: R.day[i], d, r };
    byId.set(R.id[i], i);
  }
  let R0 = 0;
  for (const c of perDay.values()) R0 = Math.max(R0, c);
  const model = { seed: pack.seed, d0, D0: V7.D0, R0, e0, perDay, items, byId, brush: new Set() };
  for (const it of items) placeItem(model, it);
  return model;
}
function placeItem(model, it) {
  it.x = (it.d - (V7.D0 - 1) / 2) * V7.pitch;
  it.z = (it.r - (model.R0 - 1) / 2) * V7.pitch;        // R0 locked at load — basis never moves
  const aMajor = it.amount_minor / Math.pow(10, V7.exp[it.cur]);
  it.aMajor = aMajor;
  it.h = clamp(V7.hBase + V7.hK * Math.log10(aMajor), V7.hMin, V7.hMax);
  it.mn = [it.x - V7.half, 0, it.z - V7.half];
  it.mx = [it.x + V7.half, it.h, it.z + V7.half];
}
// §3.7: apply one SSE batch to the model — flips update in place; creates append at
// n = count, r = current in-day count; NOTHING re-ranks; layout basis (d0, R0) stays locked.
function applyBatchToModel(model, records) {
  const touched = [];
  for (const rec of records || []) {
    const idx = model.byId.get(rec.id);
    if (idx !== undefined) {
      const it = model.items[idx];
      const dayChanged = rec.day != null && rec.day !== it.day;
      if (rec.amount_minor != null) it.amount_minor = rec.amount_minor;
      if (rec.currency != null) it.cur = rec.currency;
      if (rec.status != null) it.status = rec.status;
      placeItem(model, it);
      touched.push({ id: rec.id, n: idx, kind: 'update', dayChanged });
    } else {
      const d = dayEpoch(rec.day) - model.e0;
      const r = model.perDay.get(d) || 0;
      model.perDay.set(d, r + 1);
      const it = { n: model.items.length, id: rec.id, amount_minor: rec.amount_minor,
                   cur: rec.currency, status: rec.status, day: rec.day, d, r };
      placeItem(model, it);
      model.items.push(it);
      model.byId.set(rec.id, it.n);
      touched.push({ id: rec.id, n: it.n, kind: 'create' });
    }
  }
  return touched;
}
// §3.1 scene digest — index-free float64 sums, rounded to 4 decimals.
function expectDigest(model) {
  let Sh = 0, Sh2 = 0, Sx = 0, Sz = 0, Sxh = 0, Szh = 0;
  for (const it of model.items) {
    Sh += it.h; Sh2 += it.h * it.h; Sx += it.x; Sz += it.z;
    Sxh += it.x * it.h; Szh += it.z * it.h;
  }
  const r4 = (v) => +v.toFixed(4);
  return { count: model.items.length, Sh: r4(Sh), Sh2: r4(Sh2), Sx: r4(Sx), Sz: r4(Sz),
           Sxh: r4(Sxh), Szh: r4(Szh), brushedCount: model.brush.size };
}
const digestTolOk = (got, exp) => {
  if (!got || typeof got !== 'object') return false;
  for (const k of ['count', 'Sh', 'Sh2', 'Sx', 'Sz', 'Sxh', 'Szh']) {
    const g = got[k], e = exp[k];
    if (typeof g !== 'number') return false;
    if (Math.abs(g - e) > Math.max(0.5, 1e-4 * Math.abs(e))) return false;
  }
  return true;
};
function digestMaxDelta(got, exp) {
  if (!got || typeof got !== 'object') return null;
  let m = 0;
  for (const k of ['count', 'Sh', 'Sh2', 'Sx', 'Sz', 'Sxh', 'Szh']) {
    if (typeof got[k] !== 'number') return null;
    m = Math.max(m, Math.abs(got[k] - exp[k]));
  }
  return +m.toFixed(4);
}

// ── analytic pick model: nearest rendered surface along the pixel ray (§3.3) ─────────────────
// Screen-space binning keeps 12,288-box ray casts tractable: a box can only be hit through a
// pixel its projected AABB covers; boxes with any near-clipped corner go to the always-test set.
const BIN = 48;
function poseCtx(model, yaw, pitch, distance, W, H) {
  const eye = cameraEye(yaw, pitch, distance), basis = cameraBasis(eye);
  const nx = Math.ceil(W / BIN), ny = Math.ceil(H / BIN);
  const bins = Array.from({ length: nx * ny }, () => []);
  const globals = [];
  for (const it of model.items) {
    const cs = [
      [it.mn[0], 0, it.mn[2]], [it.mx[0], 0, it.mn[2]], [it.mn[0], 0, it.mx[2]], [it.mx[0], 0, it.mx[2]],
      [it.mn[0], it.h, it.mn[2]], [it.mx[0], it.h, it.mn[2]], [it.mn[0], it.h, it.mx[2]], [it.mx[0], it.h, it.mx[2]],
    ];
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity, behind = 0, clipped = false;
    for (const c of cs) {
      const q = sub(c, eye);
      const zc = dot(q, basis.f);
      if (zc <= V7.near) { behind++; clipped = true; continue; }
      const p = projectPt(eye, basis, W, H, c);
      x0 = Math.min(x0, p.x); x1 = Math.max(x1, p.x);
      y0 = Math.min(y0, p.y); y1 = Math.max(y1, p.y);
    }
    if (behind === 8) continue;                          // fully behind the near plane: not rendered
    if (clipped) { globals.push(it.n); continue; }
    if (x1 < -4 || x0 > W + 4 || y1 < -4 || y0 > H + 4) continue;
    const bx0 = clamp(Math.floor((x0 - 1) / BIN), 0, nx - 1), bx1 = clamp(Math.floor((x1 + 1) / BIN), 0, nx - 1);
    const by0 = clamp(Math.floor((y0 - 1) / BIN), 0, ny - 1), by1 = clamp(Math.floor((y1 + 1) / BIN), 0, ny - 1);
    for (let by = by0; by <= by1; by++) for (let bx = bx0; bx <= bx1; bx++) bins[by * nx + bx].push(it.n);
  }
  return { model, yaw, pitch, distance, W, H, eye, basis, bins, globals, nx, ny };
}
// SB7.1: true stepped silhouettes replace the old solid-cuboid pick assumption.
function towerParts(it, offset = 0) {
  const collar = { EUR: .31, USD: .35, JPY: .39, KWD: .43 }[it.cur];
  return [[.45,0,.12,'pedestal'],[.27,.12,.90,'shaft'],[.45,.90,1,'cap'],
          [collar,.76+offset,.84+offset,'collar']].map(([half,lo,hi,name]) => ({
    name, mn:[it.x-half,it.h*lo,it.z-half], mx:[it.x+half,it.h*hi,it.z+half],
  }));
}
function castPixel(ctx, sx, sy) {
  const { model, eye, basis, W, H } = ctx;
  const d = unprojectDir(basis, W, H, sx, sy);
  const bx = clamp(Math.floor(sx / BIN), 0, ctx.nx - 1), by = clamp(Math.floor(sy / BIN), 0, ctx.ny - 1);
  const cand = ctx.bins[by * ctx.nx + bx].concat(ctx.globals);
  const hits = [];
  for (const n of cand) {
    const it = model.items[n];
    const parts = towerParts(it);
    let t = null, surface = null;
    for (const part of parts) {
      const distance = rayBox(eye, d, part.mn, part.mx);
      if (distance != null && (t == null || distance < t)) {t = distance;surface=part;}
    }
    if (t == null) continue;
    const hp = [eye[0] + t * d[0], eye[1] + t * d[1], eye[2] + t * d[2]];
    const zc = dot(sub(hp, eye), basis.f);
    if (zc <= V7.near) continue;                         // near-clipped: not rendered
    hits.push({ n, t, zc, hitY: hp[1], factor: surfaceFactor(it,surface,hp) });
  }
  hits.sort((a, b) => a.t - b.t);
  return hits;
}
// The surface color the pixel shows for a hit (brush-aware): top face iff the ray entered
// through the y=h plane; §3.1 color rules (dim applied before the side factor).
function surfaceFactor(it,part,point) {
  if(Math.abs(point[1]-it.h)<1e-6)return 1;
  for(const axis of [0,2,1])if(Math.min(Math.abs(point[axis]-part.mn[axis]),Math.abs(point[axis]-part.mx[axis]))<1e-6)return axis===0?.55:axis===2?.72:.82;
  throw new Error('Ray hit has no box surface');
}
function surfColor(ctx, hit) {
  const it = ctx.model.items[hit.n];
  let base = V7.status[it.status];
  if (ctx.model.brush.size > 0 && !ctx.model.brush.has(it.id)) base = dimColor(base);
  return base.map(v=>Math.round(v*hit.factor));
}
// Decisive-target machinery (R4): unanimous 3×3 device-px neighborhood, ≥3 px lateral margin
// (radius-3 ring unanimity), ≥0.002 NDC depth gap over the runner-up on the center ray.
const O9 = [];
for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) O9.push([dx, dy]);
const O3 = [[-3, 0], [3, 0], [0, -3], [0, 3], [-3, -3], [3, -3], [-3, 3], [3, 3]];
function decisiveAt(ctx, sx, sy) {
  const center = castPixel(ctx, sx, sy);
  const front = center.length ? center[0].n : null;
  let unanimous9 = true, lateral3 = true;
  for (const [ox, oy] of O9) {
    const h = castPixel(ctx, sx + ox, sy + oy);
    if ((h.length ? h[0].n : null) !== front) { unanimous9 = false; break; }
  }
  if (unanimous9) for (const [ox, oy] of O3) {
    const h = castPixel(ctx, sx + ox, sy + oy);
    if ((h.length ? h[0].n : null) !== front) { lateral3 = false; break; }
  }
  let second = null;
  for (const h of center) if (front != null && h.n !== front) { second = h; break; }
  const depthGap = front == null ? null
    : second == null ? Infinity
    : ndcDepth(second.zc) - ndcDepth(center[0].zc);
  const decisive = unanimous9 && lateral3 && (front == null || depthGap >= V7.depthGapNdc);
  return { front, hit: center[0] || null, second, unanimous9, lateral3, depthGap, decisive, hits: center };
}
// ── label-culling expectation (§3.5): candidates, eligibility, priority-order culling ────────
function labelCandidates(model, packList) {
  if (Array.isArray(packList) && packList.length) {
    return packList.map((id) => model.items[model.byId.get(id)]).filter(Boolean)
      .sort((a, b) => (b.aMajor - a.aMajor) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  }
  return [...model.items]
    .sort((a, b) => (b.aMajor - a.aMajor) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    .slice(0, V7.labelCandidates);
}
const rectsOverlap = (a, b, margin) =>            // ≥(1+margin) px intersection on both axes
  Math.min(a.x + V7.labelW, b.x + V7.labelW) - Math.max(a.x, b.x) >= 1 + margin &&
  Math.min(a.y + V7.labelH, b.y + V7.labelH) - Math.max(a.y, b.y) >= 1 + margin;
const rectsClear = (a, b, margin) =>              // separated by ≥margin px on some axis
  Math.max(a.x, b.x) - Math.min(a.x + V7.labelW, b.x + V7.labelW) >= margin ||
  Math.max(a.y, b.y) - Math.min(a.y + V7.labelH, b.y + V7.labelH) >= margin;
// Expected label state at a pose: per candidate {eligible, rect, shown} via the frozen
// algorithm — anchor projects, inside canvas, pick(anchor)==instance, then greedy no-overlap.
function expectLabels(ctx, cands) {
  const rows = [];
  for (const it of cands) {
    const A = projectPt(ctx.eye, ctx.basis, ctx.W, ctx.H, [it.x, it.h, it.z]);
    const inCanvas = !!A && A.x >= 0 && A.x <= ctx.W && A.y >= 0 && A.y <= ctx.H;
    let pickSelf = false, unanimous = false;
    if (inCanvas) {
      const d = decisiveAt(ctx, A.x, A.y);
      pickSelf = d.front === it.n;
      unanimous = d.unanimous9;
    }
    const eligible = inCanvas && pickSelf;
    rows.push({ id: it.id, n: it.n, aMajor: it.aMajor, cur: it.cur,
                amount_minor: it.amount_minor, anchor: A ? { x: A.x, y: A.y } : null,
                rect: A ? { x: A.x + V7.labelDx, y: A.y + V7.labelDy } : null,
                inCanvas, pickSelf, unanimous, eligible, shown: false });
  }
  const placed = [];
  for (const r of rows) {                          // rows arrive already in priority order
    if (!r.eligible) continue;
    if (placed.some((p) => rectsOverlap(p.rect, r.rect, 0))) continue;
    r.shown = true;
    placed.push(r);
  }
  return rows;
}
// Decisive pose for label grading (R4): every candidate anchor 3×3-unanimous in our model AND
// every candidate pair ≥5 px overlapped or ≥5 px clear, so the legal ±2 px cannot flip the set.
function labelPoseDecisive(rows) {
  for (const r of rows) {
    if (!r.rect) continue;
    if (r.inCanvas && !r.unanimous) return false;
  }
  const vis = rows.filter((r) => r.rect && r.inCanvas);
  for (let i = 0; i < vis.length; i++) for (let j = i + 1; j < vis.length; j++) {
    const a = vis[i].rect, b = vis[j].rect;
    if (!rectsOverlap(a, b, V7.pairMarginPx) && !rectsClear(a, b, V7.pairMarginPx)) return false;
  }
  return true;
}

// ── selfcheck: identities + hand anchors; no browser, no app ─────────────────────────────────
// Console-error source attribution (r5 REPAIR r0 receipt: the render gate's finding carried
// consoleErrors.texts[0] with no file, so viz.js's ReferenceError parked as a known_bug while
// contract nits got fix shards). URL -> server-relative path; '' means unknown, never fabricated.
function urlToRelPath(u) {
  try {
    const parsed = new URL(String(u));
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return '';
    return parsed.pathname.replace(/^\/+/, '');
  } catch {
    return '';
  }
}
function stackFirstRelPath(stack) {
  const m = /https?:\/\/[^\s)]+/.exec(String(stack || ''));
  return m ? urlToRelPath(m[0].replace(/(:\d+){1,2}$/, '')) : '';
}

function selfcheck() {
  const failures = [];
  const near = (a, b, tol, what) => {
    if (a == null || Math.abs(a - b) > tol) failures.push(`${what}: got ${a} want ${b}`);
  };
  // anchors (same relative geometry as the verified sb-6 cases; target (0,1,0), fovY 50°)
  {
    const eye = cameraEye(0, 0, 10), basis = cameraBasis(eye);
    near(eye[1], 1, 1e-9, 'a1 eye.y'); near(eye[2], 10, 1e-9, 'a1 eye.z');
    const p0 = projectPt(eye, basis, 800, 480, [0, 1, 0]);
    near(p0 && p0.x, 400, 1e-6, 'a1 center.x'); near(p0 && p0.y, 240, 1e-6, 'a1 center.y');
    const p1 = projectPt(eye, basis, 800, 480, [1, 1, 0]);
    near(p1 && p1.x, 451.4681660922294, 1e-6, 'a1 off.x'); near(p1 && p1.y, 240, 1e-6, 'a1 off.y');
    const eye2 = cameraEye(90, 0, 20), basis2 = cameraBasis(eye2);
    const p2 = projectPt(eye2, basis2, 800, 480, [0, 1, 5]);
    near(p2 && p2.x, 271.32958476942645, 1e-6, 'a2 side.x'); near(p2 && p2.y, 240, 1e-6, 'a2 side.y');
    if (projectPt(eye2, basis2, 800, 480, [30, 1, 0]) !== null) failures.push('a2 behind-camera not null');
  }
  // identities: target→center at any pose; orthonormal basis; unproject∘project round-trip
  {
    const eye = cameraEye(30, 40, 260), basis = cameraBasis(eye);
    const c = projectPt(eye, basis, 1280, 720, V7.target);
    near(c && c.x, 640, 1e-6, 'id center.x'); near(c && c.y, 360, 1e-6, 'id center.y');
    near(dot(basis.r, basis.f), 0, 1e-12, 'id r·f'); near(dot(basis.u, basis.f), 0, 1e-12, 'id u·f');
    near(Math.hypot(...basis.u), 1, 1e-12, 'id |u|');
    const P = [7.2, 2.1, -12.6];
    const pr = projectPt(eye, basis, 1280, 720, P);
    const d = unprojectDir(basis, 1280, 720, pr.x, pr.y);
    const t = dot(sub(P, eye), basis.f) / dot(d, basis.f);
    const hp = [eye[0] + t * d[0], eye[1] + t * d[1], eye[2] + t * d[2]];
    near(Math.hypot(...sub(hp, P)), 0, 1e-6, 'id round-trip');
    near(ndcDepth(V7.near), -1, 1e-9, 'id ndc(near)'); near(ndcDepth(V7.far), 1, 1e-9, 'id ndc(far)');
  }
  // rayBox occlusion + decisiveAt on a two-box scene, via a real model
  {
    const model = buildModel({ seed: 'f0f0f0f0f0f0f0f0', records: {
      id: ['a', 'b'], amount_minor: [100000, 100000], currency: ['EUR', 'EUR'],
      status: ['settled', 'pending'], day: ['2026-04-01', '2026-04-01'] } });
    near(model.items[0].r, 0, 1e-12, 'm ranks0'); near(model.items[1].r, 1, 1e-12, 'm ranks1');
    near(model.items[0].h, 0.9 + 0.55 * 3, 1e-12, 'm h(1000 EUR)');
    const dg = expectDigest(model);
    near(dg.Sh, 2 * (0.9 + 0.55 * 3), 1e-9, 'm digest Sh');
    near(dg.Sx, 2 * (0 - (V7.D0 - 1) / 2) * V7.pitch, 1e-4, 'm digest Sx');
    const touched = applyBatchToModel(model, [
      { id: 'c', amount_minor: 500, currency: 'JPY', status: 'failed', day: '2026-04-01' }]);
    if (touched[0].kind !== 'create' || touched[0].n !== 2 || model.items[2].r !== 2)
      failures.push('m create append rank');
    near(model.items[2].h, Math.min(4.2, 0.9 + 0.55 * Math.log10(500)), 1e-12, 'm h(JPY 500)');
    // day 0 of the 96-day span sits at world x = −57: orbit to yaw 90 so it faces the camera
    const ctx = poseCtx(model, 90, 30, 60, 800, 600);
    const front = model.items[0];
    const pr = projectPt(ctx.eye, ctx.basis, 800, 600, [front.x, front.h / 2, front.z]);
    const hits = castPixel(ctx, pr.x, pr.y);
    if (!hits.length || hits[0].n !== 0) failures.push('m castPixel front');
    const dec = decisiveAt(ctx, pr.x, pr.y);
    if (typeof dec.decisive !== 'boolean' || dec.front !== 0) failures.push('m decisiveAt front');
  }
  // colors, tolerance arithmetic, angles, rng determinism, label rect predicates
  {
    const dimSettled = dimColor(V7.status.settled);
    near(dimSettled[1], Math.round(0.30 * 150), 0, 'c dim.g');
    const sideOfDim = sideColor(dimColor(V7.status.failed));
    near(sideOfDim[0], Math.round(0.55 * Math.round(0.30 * 185)), 0, 'c sideOfDim.r');
    for (const [name, c] of Object.entries(V7.status)) {
      const worst = sideColor(dimColor(c));
      if (!worst.some((v, i) => Math.abs(v - V7.bg[i]) > V7.tol))
        failures.push(`c ${name} composite aliases into background`);
    }
    near(angDist(-7, 353), 0, 1e-9, 'c angDist1'); near(angDist(350, 10), 20, 1e-9, 'c angDist2');
    const r1 = seedRng('abcd', 'x'), r2 = seedRng('abcd', 'x');
    near(r1(), r2(), 0, 'c rng deterministic');
    const A = { x: 0, y: 0 }, B = { x: 100, y: 0 }, C = { x: 130, y: 0 };
    if (!rectsOverlap(A, B, 5)) failures.push('c rectsOverlap');
    if (!rectsClear(A, C, 5)) failures.push('c rectsClear');
    if (rectsOverlap(A, C, 0) || rectsClear(A, B, 5)) failures.push('c rect predicates inverted');
  }
  // console-error source attribution: URL -> server-relative path, '' when unknown
  {
    if (urlToRelPath('http://127.0.0.1:54622/web/viz.js') !== 'web/viz.js') failures.push('s url relpath');
    if (urlToRelPath('about:blank') !== '') failures.push('s non-http not empty');
    if (urlToRelPath('http://127.0.0.1:54622/') !== '') failures.push('s bare root not empty');
    const stk = 'ReferenceError: x is not defined\n    at onLoad (http://127.0.0.1:54622/web/viz.js:1124:5)';
    if (stackFirstRelPath(stk) !== 'web/viz.js') failures.push('s stack line:col strip');
    if (stackFirstRelPath(undefined) !== '') failures.push('s missing stack not empty');
  }
  const ok = failures.length === 0;
  process.stdout.write(JSON.stringify({ selfcheck: ok ? 'ok' : 'fail', failures }) + '\n');
  process.exit(ok ? 0 : 1);
}

// ── CLI ──────────────────────────────────────────────────────────────────────────────────────
const args = process.argv.slice(2);
if (args.includes('--selfcheck')) selfcheck();
// --preflight: can THIS node resolve playwright and launch chromium? The scorer refuses to grade
// when this fails. r0 was scored under a node with no playwright: 30 of 99 checks came back
// PROBE-UNAVAILABLE, were excluded from the means, and a number was printed anyway.
if (args.includes('--preflight')) {
  (async () => {
    try {
      const pw = loadPlaywright();
      const b = await pw.chromium.launch({ headless: true, ...(process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE?{executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE}:{}) });
      await b.close();
      console.log(JSON.stringify({ ok: true, node: process.version, execPath: process.execPath }));
      process.exit(0);
    } catch (e) {
      console.log(JSON.stringify({ ok: false, node: process.version, execPath: process.execPath,
                                   error: String(e && e.message || e) }));
      process.exit(3);
    }
  })();
} else {

const blockApi = args.includes('--block-api');
const positional = args.filter((a) => !a.startsWith('--'));
const scenario = positional[0];
const baseUrl = positional[1];
const SCENARIOS = ['boot', 'load', 'sync', 'flow', 'error', 'viz', 'feed', 'sb71-visual'];
if (!SCENARIOS.includes(scenario) || !baseUrl) {
  err(`usage: node product_probe_v3.mjs <${SCENARIOS.join('|')}> <baseUrl> [--block-api] | --selfcheck`);
  process.exit(2);
}
const isViz = scenario === 'viz' || scenario === 'sb71-visual';

// F18 lineage: viz carries SwiftShader startup at N=12,288 plus the full scripted battery.
const HARD_MS = isViz ? 230000 : scenario === 'flow' ? 110000 : 90000;
const startedAt = Date.now();
const budgetLeft = () => HARD_MS - (Date.now() - startedAt);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const result = { scenario, baseUrl, timedOut: false };
// Phases merge into `result` as they complete, so a cap hit ships everything measured;
// the scorer reads absent sections on timedOut as PROBE UNAVAILABLE (score_sb7._viz_section).
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

// GL wrapper v2 (§3.2 R6): counts draws/readPixels CLASSIFIED by the framebuffer bound at
// call time (wrapped bindFramebuffer; null = default = scene draw), proxies getExtension so
// ANGLE_instanced_arrays / WEBGL_multi_draw entry points are counted too, and accounts
// bufferData bytes (realloc = bufferData > 4096) and bufferSubData bytes on ALL targets.
// Forces preserveDrawingBuffer (readPixels-after-composite trap) and records context attrs.
function glInstrument() {
  const P = { contexts: [], contextLost: 0,
              defDraws: 0, offDraws: 0, defReads: 0, offReads: 0,
              bufDataBytes: 0, bufSubBytes: 0, reallocs: 0, bufDataCalls: 0, bufSubCalls: 0,
              drawTs: [], sb71DrawTimes:new WeakMap(), rafTicks: 0, stream: [] };
  window.__p7 = P;
  const byteLen = (x) => (typeof x === 'number' ? x
    : x && typeof x.byteLength === 'number' ? x.byteLength : 0);
  const isFbTarget = (gl, t) =>
    t === gl.FRAMEBUFFER || (gl.DRAW_FRAMEBUFFER !== undefined && t === gl.DRAW_FRAMEBUFFER);
  function countDraw(gl) {
    if (gl.__p7fbo == null) {
      P.defDraws++;
      P.sb71DrawTimes.set(gl.canvas,performance.now());
      if (P.drawTs.length < 20000) P.drawTs.push(performance.now());
    } else P.offDraws++;
  }
  function wrapDrawFns(obj, gl, names) {
    for (const fn of names)
      if (typeof obj[fn] === 'function') {
        const d = obj[fn].bind(obj);
        obj[fn] = (...a) => { countDraw(gl); return d(...a); };
      }
  }
  const wrap = (proto, offscreen) => {
    const orig = proto.getContext;
    proto.getContext = function (type, attrs) {
      if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
        const asked = Object.assign({}, attrs || {});
        attrs = Object.assign({}, asked, { preserveDrawingBuffer: true });
        const gl = orig.call(this, type, attrs);
        if (gl && !gl.__p7seen) {
          gl.__p7seen = true;
          gl.__p7fbo = null;
          P.contexts.push({ type, offscreen, canvasId: (this && this.id) || null,
                            askedAttrs: { antialias: asked.antialias, alpha: asked.alpha } });
          wrapDrawFns(gl, gl, ['drawArrays', 'drawElements',
                               'drawArraysInstanced', 'drawElementsInstanced',
                               'drawRangeElements']);
          const oBind = gl.bindFramebuffer.bind(gl);
          gl.bindFramebuffer = (t, fb) => {
            if (isFbTarget(gl, t)) gl.__p7fbo = fb;
            return oBind(t, fb);
          };
          const oRead = gl.readPixels.bind(gl);
          gl.readPixels = (...a) => {
            if (gl.__p7fbo == null) P.defReads++; else P.offReads++;
            return oRead(...a);
          };
          const oBufData = gl.bufferData.bind(gl);
          gl.bufferData = (target, sizeOrData, ...rest) => {
            const b = byteLen(sizeOrData);
            P.bufDataBytes += b; P.bufDataCalls++;
            if (b > 4096) P.reallocs++;
            return oBufData(target, sizeOrData, ...rest);
          };
          const oBufSub = gl.bufferSubData.bind(gl);
          gl.bufferSubData = (target, offset, data, ...rest) => {
            let b = byteLen(data);
            if (rest.length >= 2 && typeof rest[1] === 'number' && data && data.BYTES_PER_ELEMENT)
              b = rest[1] * data.BYTES_PER_ELEMENT;          // (…, srcOffset, length) overload
            P.bufSubBytes += b; P.bufSubCalls++;
            return oBufSub(target, offset, data, ...rest);
          };
          const oExt = gl.getExtension.bind(gl);
          const proxied = {};
          gl.getExtension = (name) => {
            const ext = oExt(name);
            if (!ext) return ext;
            if (proxied[name]) return proxied[name];
            if (name === 'ANGLE_instanced_arrays' || name === 'WEBGL_multi_draw') {
              const shell = Object.create(ext);
              for (const k in ext) {
                const v = ext[k];
                if (typeof v === 'function') {
                  const bound = v.bind(ext);
                  shell[k] = /^(draw|multiDraw)/.test(k)
                    ? (...a) => { countDraw(gl); return bound(...a); }
                    : (...a) => bound(...a);
                } else shell[k] = v;
              }
              proxied[name] = shell;
              return shell;
            }
            return ext;                                       // WEBGL_draw_buffers etc: passthrough
          };
          if (this.addEventListener)
            this.addEventListener('webglcontextlost', () => P.contextLost++);
        }
        return gl;
      }
      return orig.call(this, type, attrs);
    };
  };
  wrap(HTMLCanvasElement.prototype, false);
  if (typeof OffscreenCanvas !== 'undefined') wrap(OffscreenCanvas.prototype, true);
  const oRaf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => oRaf((t) => { P.rafTicks++; return cb(t); });
}

// §3.7 stream observer: wraps EventSource so every SSE batch is recorded with arrival time,
// pre-apply counters+digest, and a bounded post-apply settle read (digest change = applied).
function streamInstrument() {
  const P = window.__p7 || (window.__p7 = { stream: [] });
  if (!Array.isArray(P.stream)) P.stream = [];
  const counters = () => ({
    defDraws: P.defDraws || 0, offDraws: P.offDraws || 0,
    bufDataBytes: P.bufDataBytes || 0, bufSubBytes: P.bufSubBytes || 0,
    bufDataCalls: P.bufDataCalls || 0, bufSubCalls: P.bufSubCalls || 0,
    reallocs: P.reallocs || 0,
  });
  const digest = () => {
    try { return window.vs7dbg && window.vs7dbg.sceneDigest ? window.vs7dbg.sceneDigest() : null; }
    catch (e) { return { __err: String(e).slice(0, 80) }; }
  };
  const digestKey = (d) => (d && d.__err == null)
    ? [d.count, d.Sh, d.Sh2, d.Sx, d.Sz, d.Sxh, d.Szh, d.brushedCount].join('|') : null;
  const Orig = window.EventSource;
  if (!Orig) { P.streamUnsupported = true; return; }
  function Wrapped(url, cfg) {
    const es = new Orig(url, cfg);
    (P.sb71Sources||(P.sb71Sources=[])).push(es);
    P.esUrls = (P.esUrls || []).concat([String(url)]).slice(0, 8);
    es.addEventListener('message', (ev) => {
      let parsed = null;
      try { parsed = JSON.parse(ev.data); } catch {}
      const entry = {
        t0: performance.now(), url: String(url), bytes: (ev.data || '').length,
        batch: parsed && parsed.batch != null ? parsed.batch : null,
        records: parsed && Array.isArray(parsed.records)
          ? parsed.records.slice(0, 16) : null,
        size: parsed && Array.isArray(parsed.records) ? parsed.records.length : null,
        c0: counters(), digest0: digest(), t1: null, c1: null, digest1: null, applyMs: null,
      };
      if (P.stream.length < 40) P.stream.push(entry);
      if (P.sb71ArmCapture) P.sb71ArmCapture(entry);
      const k0 = digestKey(entry.digest0);
      const t0 = entry.t0;
      // Harness fix: rAF starves at rest in headless Chromium and timers get
      // coarse-throttled (~300 ms), both of which billed detection latency to the app.
      // MessageChannel scheduling is unthrottled: the first tick lands within the next
      // task, so an instant apply measures as instant; after 600 ms fall back to a
      // coarse timer to keep the poll cheap.
      const mc = new MessageChannel();
      const poll = () => {
        const now = performance.now();
        const d = digest();
        if (digestKey(d) !== null && digestKey(d) !== k0) {
          entry.applyMs = +(now - t0).toFixed(1);
          entry.t1 = now; entry.c1 = counters(); entry.digest1 = d;
          return;
        }
        if (now - t0 > 3000) { entry.t1 = now; entry.c1 = counters(); entry.digest1 = d; return; }
        if (now - t0 < 600) { mc.port2.postMessage(0); } else setTimeout(poll, 80);
      };
      mc.port1.onmessage = poll;
      mc.port2.postMessage(0);
    });
    return es;
  }
  Wrapped.prototype = Orig.prototype;
  for (const k of ['CONNECTING', 'OPEN', 'CLOSED']) Wrapped[k] = Orig[k];
  window.EventSource = Wrapped;
}

function pageGlCounters() {
  const P = window.__p7 || {};
  return {
    contexts: P.contexts || [], contextLost: P.contextLost || 0,
    defDraws: P.defDraws || 0, offDraws: P.offDraws || 0,
    defReads: P.defReads || 0, offReads: P.offReads || 0,
    bufDataBytes: P.bufDataBytes || 0, bufSubBytes: P.bufSubBytes || 0,
    bufDataCalls: P.bufDataCalls || 0, bufSubCalls: P.bufSubCalls || 0,
    reallocs: P.reallocs || 0, rafTicks: P.rafTicks || 0,
    drawCount: (P.drawTs || []).length,
    lastDrawMs: P.drawTs && P.drawTs.length ? +P.drawTs[P.drawTs.length - 1].toFixed(1) : null,
  };
}
function pageDrawTs(arg) {
  const P = window.__p7 || {};
  const ts = P.drawTs || [];
  return ts.slice(Math.max(0, ts.length - ((arg && arg.tail) || 2000))).map((t) => +t.toFixed(2));
}
function pageStreamLog() {
  const P = window.__p7 || {};
  return { unsupported: !!P.streamUnsupported, esUrls: P.esUrls || [],
           entries: (P.stream || []).map((e) => ({ ...e })) };
}

// §3.8 vs7dbg surface — every call safe-wrapped; a missing surface reports null per member.
function pageVs7(arg) {
  const d = window.vs7dbg;
  if (!d) return { present: false };
  const safe = (fn) => { try { return fn(); } catch (e) { return { __err: String(e).slice(0, 80) }; } };
  const out = { present: true };
  const want = (arg && arg.want) || ['layout', 'digest', 'camera', 'frames', 'brush'];
  if (want.includes('layout')) out.layout = safe(() => d.layout());
  if (want.includes('digest')) out.digest = safe(() => d.sceneDigest());
  if (want.includes('camera')) out.camera = safe(() => d.camera());
  if (want.includes('frames')) out.frames = safe(() => d.frames());
  if (want.includes('brush')) out.brush = safe(() => d.brush());
  if (arg && Array.isArray(arg.picks))
    out.picks = arg.picks.map((q) => safe(() => d.pick(q[0], q[1])));
  if (arg && Array.isArray(arg.pickPixels))
    out.pickPixels = arg.pickPixels.map((q) => safe(() => {
      const v = d.pickPixel(q[0], q[1]);
      return v && typeof v.length === 'number' ? Array.from(v).slice(0, 4) : v;
    }));
  if (arg && arg.setCamera)
    out.setCamera = safe(() => d.setCamera(arg.setCamera[0], arg.setCamera[1], arg.setCamera[2]));
  return out;
}

// Framebuffer column scan for the height-pixel rung (R9): walks a device-pixel column around
// the projected top, returns the first row (from the top of the scan window) whose pixel is
// NOT background — plus the raw column so the scorer can audit the transition.
function pageColumnScan(arg) {
  const canvas = document.getElementById('viz3d');
  if (!canvas) return { found: false };
  const gl = canvas.getContext('webgl2') || canvas.getContext('webgl') ||
             canvas.getContext('experimental-webgl');
  if (!gl) return { found: true, glReadable: false };
  const rect = canvas.getBoundingClientRect();
  const W = canvas.width, H = canvas.height;
  const out = [];
  for (const col of arg.columns) {
    const bx = Math.min(W - 1, Math.max(0, Math.round(col.sx * (W / rect.width))));
    const y0 = Math.min(H - 1, Math.max(0, H - 1 - Math.round(col.yTop * (H / rect.height))));
    const y1 = Math.min(H - 1, Math.max(0, H - 1 - Math.round(col.yBot * (H / rect.height))));
    const lo = Math.min(y0, y1), hi = Math.max(y0, y1);
    const px = new Uint8Array((hi - lo + 1) * 4);
    gl.readPixels(bx, lo, 1, hi - lo + 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    const rows = [];
    for (let by = hi; by >= lo; by--) {                  // top of screen → down
      const o = (by - lo) * 4;
      rows.push([px[o], px[o + 1], px[o + 2]]);
    }
    out.push({ id: col.id, bx, cssYTop: col.yTop, rows });
  }
  return { found: true, glReadable: true, dpr: window.devicePixelRatio,
           rect: { w: rect.width, h: rect.height }, backing: { w: W, h: H }, columns: out };
}

// Single-pixel framebuffer samples (CSS coords, y-flip + DPR verified in sb-6) — used for
// brush dim truth, changed-instance stream pixels, and the context-real grid.
function pageSamplePixels(arg) {
  const canvas = document.getElementById('viz3d');
  if (!canvas) return { found: false };
  const gl = canvas.getContext('webgl2') || canvas.getContext('webgl') ||
             canvas.getContext('experimental-webgl');
  if (!gl) return { found: true, glReadable: false };
  const rect = canvas.getBoundingClientRect();
  const W = canvas.width, H = canvas.height;
  const px = new Uint8Array(W * H * 4);
  gl.readPixels(0, 0, W, H, gl.RGBA, gl.UNSIGNED_BYTE, px);
  const at = (cx, cy) => {
    const bx = Math.min(W - 1, Math.max(0, Math.round(cx * (W / rect.width))));
    const by = Math.min(H - 1, Math.max(0, H - 1 - Math.round(cy * (H / rect.height))));
    const o = (by * W + bx) * 4;
    return [px[o], px[o + 1], px[o + 2]];
  };
  return { found: true, glReadable: true, dpr: window.devicePixelRatio,
           rect: { w: rect.width, h: rect.height }, backing: { w: W, h: H },
           samples: (arg.points || []).map((p) => ({ ...p, rayX:(Math.round(p.cx*W/rect.width)+.5)*rect.width/W, rayY:(Math.round(p.cy*H/rect.height)+.5)*rect.height/H, got: at(p.cx, p.cy) })) };
}
// ---------- carried page functions (product_probe_v2 house style, proven) ----------

// Installed before any page script: stamps the exact moment the first data row lands.
// Rendered-means-seen: only rows the browser would paint count (sb-6.1 lesson).
function initFirstDataStamp() {
  window.__probeFirstDataMs = null;
  const visible = (el) =>
    !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  const check = () => {
    if (window.__probeFirstDataMs != null) return true;
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

  const curRe = /\b(EUR|USD|JPY|KWD)\b|[€$¥]|د\.ك/;
  const amountTexts = rows.slice(0, 12).map((r) => {
    const cell = cellsOf(r).find((c) => curRe.test(c.innerText || '') && /\d/.test(c.innerText || ''));
    return cell ? (cell.innerText || '').trim().slice(0, 40) : null;
  }).filter(Boolean);
  // sb-7: money truth needs (currency, minor, text) triples — pull the row's id when the app
  // exposes it (data-id / data-payment-id on the row) so the scorer can join against fixtures.
  const rowAmounts = rows.slice(0, 24).map((r) => {
    const id = r.getAttribute('data-id') || r.getAttribute('data-payment-id') || null;
    const cell = cellsOf(r).find((c) => curRe.test(c.innerText || '') && /\d/.test(c.innerText || ''));
    return { id, amountText: cell ? (cell.innerText || '').trim().slice(0, 40) : null };
  });

  const statusWordRe =
    /^(pending|failed|refunded|settled)$/i;
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
        (el.getAttribute('aria-label') || '') + ' ' + (el.getAttribute('placeholder') || '') +
        ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + (el.title || '') +
        ' ' + (el.className && el.className.baseVal === undefined ? el.className : '');
      if (el.tagName === 'BUTTON') name += ' ' + (el.innerText || '');
      if (el.labels) for (const l of el.labels) name += ' ' + (l.innerText || '');
      const wrap = el.closest('label');
      if (wrap) name += ' ' + (wrap.innerText || '');
      const parent = el.parentElement;
      if (el.tagName !== 'BUTTON' && parent && (parent.innerText || '').length < 60)
        name += ' ' + parent.innerText;
      if (re.test(name)) return true;
      if (
        el.tagName === 'SELECT' &&
        Array.from(el.options).filter((o) =>
          /^(all|pending|failed|refunded|settled)$/i.test((o.text || '').trim())
        ).length >= 2
      )
        return true;
    }
    return Array.from(document.querySelectorAll('[class*="filter" i]')).some(visible);
  })();

  const nav = performance.getEntriesByType('navigation')[0];
  const pageWeightBytes = Math.round(
    ((nav && nav.transferSize) || 0) +
      performance.getEntriesByType('resource').reduce((s, e) => s + (e.transferSize || 0), 0)
  );
  const externalRequests = performance.getEntriesByType('resource')
    .map((e) => e.name)
    .filter((u) => { try { return new URL(u).origin !== location.origin; } catch { return false; } })
    .slice(0, 5);

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
    renderedRowCount, domRowCount, totalClaimedInDom, dateTexts, amountTexts, rowAmounts,
    statusStyles, paginationControls, filterControl, pageWeightBytes, externalRequests, styling,
  };
}

function pageHorizontalScroll() {
  const sw = Math.max(
    document.documentElement ? document.documentElement.scrollWidth : 0,
    document.body ? document.body.scrollWidth : 0
  );
  return sw > window.innerWidth + 1;
}

function pageViewSnapshot() {
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

function pageSyncTruth({payments,summary}) {
  const exponent={EUR:2,USD:2,JPY:0,KWD:3},symbol={EUR:'€',USD:'$',JPY:'¥',KWD:'د.ك'};
  const visible=e=>e.checkVisibility({checkOpacity:true,checkVisibilityCSS:true});
  const numbers=text=>String(text).match(/[+−-]?\d[\d,. \u00a0\u202f]*/g)||[];
  const integer=text=>{
    const s=text.trim();
    if(!/^\d+$/.test(s)&&!/^\d{1,3}([,. \u00a0\u202f])\d{3}(?:\1\d{3})*$/.test(s))return null;
    return Number(s.replace(/[^0-9]/g,''));
  };
  const minor=(text,currency)=>{
    let s=text.trim(),negative=/^[-−]/.test(s);s=s.replace(/^[+−-]/,'');
    const e=exponent[currency];let whole,fraction='';
    if(e){const m=s.match(new RegExp('^(.*)[.,](\\d{'+e+'})$'));if(!m)return null;whole=integer(m[1]);fraction=m[2];}
    else whole=integer(s);
    return whole===null?null:(negative?-1:1)*(whole*10**e+Number(fraction));
  };
  const currencyShown=(text,currency)=>{
    const codes=text.match(/\b(?:EUR|USD|JPY|KWD)\b/g)||[];
    return !codes.some(code=>code!==currency)&&(codes.includes(currency)||text.includes(symbol[currency]));
  };
  const rows=Array.from(document.querySelectorAll('tbody tr[data-id], [role="row"][data-id]'))
    .filter(row=>row.querySelectorAll('td,[role="cell"],[role="gridcell"]').length>=5&&visible(row));
  const ids=rows.map(r=>r.getAttribute('data-id'));
  const membership=ids.length===payments.data.length&&new Set(ids).size===ids.length
    &&ids.every((id,i)=>id===payments.data[i].id);
  const observations=rows.map(row=>{
    const id=row.getAttribute('data-id'),record=payments.data.find(r=>r.id===id);
    const cells=Array.from(row.querySelectorAll('td,[role="cell"],[role="gridcell"]')).map(cell=>(cell.innerText||'').trim());
    const note=cells[4]||'';
    return {id,ok:!!record&&currencyShown(cells[1],record.currency)
      &&numbers(cells[1]).some(number=>minor(number,record.currency)===record.amount_minor)
      &&cells[2].toLowerCase()===record.status
      &&(record.note?note===record.note:['','—','-'].includes(note))};
  });
  const currencies=(summary.by_currency||[]).map(record=>{
    const card=Array.from(document.querySelectorAll('.cur-total[data-currency]')).find(e=>e.dataset.currency===record.currency&&visible(e));
    if(!card)return {currency:record.currency,ok:false};
    const text=card.innerText||'',values=numbers(text);
    return {currency:record.currency,ok:currencyShown(text,record.currency)&&values.some((amount,i)=>
      minor(amount,record.currency)===record.total_minor&&values.some((count,j)=>i!==j&&integer(count)===record.count))};
  });
  return {rows:observations,currencies,membership,ok:payments.data.length>0&&membership
    &&observations.every(r=>r.ok)&&currencies.length>0&&currencies.every(r=>r.ok)};
}

function pageSyncState() {
  const cands = Array.from(
    document.querySelectorAll('button, input[type="button"], input[type="submit"], [role="button"], a')
  );
  const acc = (el) =>
    (el.innerText || el.value || '') + ' ' + (el.getAttribute('aria-label') || '') + ' ' + (el.title || '');
  const el =
    document.getElementById('sync-now') ||
    cands.find((c) => /sync/i.test(acc(c)) && c.getClientRects().length > 0);
  if (!el || !el.getClientRects().length) return { found: false };
  return {
    found: true,
    text: (el.innerText || el.value || el.getAttribute('aria-label') || '').trim().slice(0, 60),
    disabled:
      el.disabled === true || el.hasAttribute('disabled') ||
      el.getAttribute('aria-disabled') === 'true' || el.getAttribute('aria-busy') === 'true' ||
      el.getAttribute('data-state') === 'syncing',
  };
}
function pageClickSync() {
  const cands = Array.from(
    document.querySelectorAll('button, input[type="button"], input[type="submit"], [role="button"], a')
  );
  const acc = (el) =>
    (el.innerText || el.value || '') + ' ' + (el.getAttribute('aria-label') || '') + ' ' + (el.title || '');
  const el =
    document.getElementById('sync-now') ||
    cands.find((c) => /sync/i.test(acc(c)) && c.getClientRects().length > 0);
  if (!el || !el.getClientRects().length) return false;
  el.scrollIntoView({ block: 'center' });
  el.click();
  return true;
}

function pageErrorBanner() {
  const re = /error|unable|unreachable|failed|try again|retry|degraded|offline|down/i;
  const bareStatus = /^(settled|pending|failed|refunded)\b[\s()0-9%·.,-]*$/i;
  const els = Array.from(document.querySelectorAll('body *'));
  for (const el of els) {
    if (el.children.length > 0 && !el.matches('[role="alert"],[class*="error" i],[class*="alert" i],[class*="banner" i]'))
      continue;
    if (el.closest('td,th,[role="cell"],[role="gridcell"],script,style')) continue;
    if (
      !el.closest('[role="alert"]') &&
      el.closest(
        'button,select,option,label,nav,[role="option"],[role="tab"],[role="listbox"],' +
          '[class*="legend" i],[class*="filter" i],[class*="chip" i],[class*="tag" i]'
      )
    )
      continue;
    if (!(el.getClientRects && el.getClientRects().length)) continue;
    const t = (el.innerText || '').trim();
    if (!t || t.length >= 300 || bareStatus.test(t)) continue;
    if (re.test(t)) return t.slice(0, 120);
  }
  return null;
}

function pageEmptyState() {
  const re = /no\s+payments|nothing|empty|not\s+synced|waiting|syncing|no\s+(records|results|data|items|transactions)/i;
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
// ---------- sb-7 page functions ----------

function pageNotificationsState() {
  const el = document.getElementById('notifications');
  if (!el) return { present: false };
  const visible = !!(el.getClientRects && el.getClientRects().length) &&
    getComputedStyle(el).visibility !== 'hidden';
  const entries = Array.from(el.querySelectorAll('li, [data-event-seq], [class*="notif" i] *'))
    .filter((n) => n.children.length === 0 && (n.innerText || '').trim().length > 0);
  return {
    present: true, visible,
    dataState: el.getAttribute('data-state'),
    entryCount: entries.length,
    texts: entries.slice(0, 6).map((n) => (n.innerText || '').trim().slice(0, 100)),
  };
}

// §3.5 label truth: every #viz-labels .viz-label with data-id, border-box rect relative to the
// canvas top-left, hidden-vs-shown per rendered-means-seen, text harvested for the money rule.
function pageLabelsRead() {
  const wrap = document.getElementById('viz-labels');
  const canvas = document.getElementById('viz3d');
  if (!wrap || !canvas) return { wrap: !!wrap, canvas: !!canvas, labels: [] };
  const cr = canvas.getBoundingClientRect();
  const labels = [];
  for (const el of wrap.querySelectorAll('.viz-label')) {
    const rects = el.getClientRects();
    const cs = getComputedStyle(el);
    const shown = !!rects.length && cs.visibility !== 'hidden' && cs.display !== 'none' &&
      +cs.opacity !== 0;
    const r = el.getBoundingClientRect();
    labels.push({
      id: el.getAttribute('data-id'), shown,
      x: +(r.left - cr.left).toFixed(2), y: +(r.top - cr.top).toFixed(2),
      w: +r.width.toFixed(2), h: +r.height.toFixed(2),
      text: (el.innerText || '').trim().slice(0, 60),
    });
  }
  const shownRects = labels.filter((l) => l.shown);
  let overlapViolations = 0;
  for (let i = 0; i < shownRects.length; i++) for (let j = i + 1; j < shownRects.length; j++) {
    const a = shownRects[i], b = shownRects[j];
    const ix = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
    const iy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
    if (ix >= 1 && iy >= 1) overlapViolations++;
  }
  return { wrap: true, canvas: true, inCanvasSpace: true, labels, overlapViolations };
}

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
           viewportW: window.innerWidth, viewportH: window.innerHeight, scrollY: window.scrollY };
}

// §3.2 pinned budget window: [dispatch of first pointermove after arming, dispatch of
// pointerup]; counters sampled synchronously at pointerup, then once more one rAF later.
function pageArmBudgetWatch() {
  const P = window.__p7 || {};
  const frames = () => {
    try { return window.vs7dbg && window.vs7dbg.frames ? window.vs7dbg.frames() : null; }
    catch { return null; }
  };
  const snap = () => ({ t: performance.now(), defDraws: P.defDraws || 0,
                        offDraws: P.offDraws || 0, frames: frames() });
  const w = { c0: null, cUp: null, c1: null, moves: 0 };
  window.__p7budget = w;
  const onMove = () => { w.moves++; if (!w.c0) w.c0 = snap(); };
  const onUp = () => {
    if (w.cUp) return;
    w.cUp = snap();
    requestAnimationFrame(() => { w.c1 = snap(); cleanup(); });
  };
  const cleanup = () => {
    window.removeEventListener('pointermove', onMove, true);
    window.removeEventListener('pointerup', onUp, true);
  };
  window.addEventListener('pointermove', onMove, { capture: true, passive: true });
  window.addEventListener('pointerup', onUp, { capture: true, passive: true });
}
function pageReadBudgetWatch() {
  return window.__p7budget || null;
}

// §3.4 coast sampler: on pointerup, sample vs7dbg.camera() every rAF until the stop threshold
// holds (|vyaw|<2 AND |vpitch|<2) twice in a row, or 4 s. Page-side stamps, never RPC polling.
function pageArmCoastWatch() {
  const cam = () => {
    try { return window.vs7dbg && window.vs7dbg.camera ? window.vs7dbg.camera() : null; }
    catch { return null; }
  };
  const w = { tUp: null, samples: [], settled: false, settleMs: null };
  window.__p7coast = w;
  const onUp = () => {
    if (w.tUp != null) return;
    w.tUp = performance.now();
    window.removeEventListener('pointerup', onUp, true);
    // Harness fix: v0 is defined "at release". This capture-phase listener runs BEFORE
    // the app's own pointerup handler computes the velocity, so the sample must be
    // deferred one task (MessageChannel — unthrottled) to land right after the full
    // dispatch; the first rAF sample alone arrived a frame late, under-reporting v0
    // and shrinking the settle budget.
    const mcU = new MessageChannel();
    mcU.port1.onmessage = () => {
      const c0 = cam();
      if (c0 && typeof c0.yaw === 'number' &&
          (!w.samples.length || w.samples[0].t !== 0)) {
        w.samples.unshift({ t: 0, yaw: c0.yaw, pitch: c0.pitch, distance: c0.distance,
                            vyaw: c0.vyaw, vpitch: c0.vpitch });
      }
    };
    mcU.port2.postMessage(0);
    let calm = 0;
    const tick = () => {
      const t = performance.now();
      const c = cam();
      if (c && typeof c.yaw === 'number') {
        if (w.samples.length < 400)
          w.samples.push({ t: +(t - w.tUp).toFixed(2), yaw: c.yaw, pitch: c.pitch,
                           distance: c.distance, vyaw: c.vyaw, vpitch: c.vpitch });
        const stopped = Math.abs(c.vyaw || 0) < 2 && Math.abs(c.vpitch || 0) < 2;
        if (stopped) {
          calm += 1;
          if (w.settleMs == null) w.settleMs = +(t - w.tUp).toFixed(1);
        } else {
          calm = 0;
          w.settleMs = null;
        }
        if (calm >= 2) {
          w.settled = true;
          return;
        }
      }
      if (t - w.tUp > 4000) return;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  };
  window.addEventListener('pointerup', onUp, { capture: true, passive: true });
}
function pageReadCoastWatch() {
  const w = window.__p7coast;
  if (!w) return null;
  const thin = w.samples.length > 120
    ? w.samples.filter((_, i) => i % 2 === 0 || i === w.samples.length - 1) : w.samples;
  return { tUp: w.tUp, settled: w.settled, settleMs: w.settleMs, samples: thin };
}

// §3.6 brush — table door: locate the row for a payment id, click it, and report its state.
function pageTableRowByld(arg) {
  const sel = `tr[data-id="${arg.id}"], tr[data-payment-id="${arg.id}"], ` +
              `[role="row"][data-id="${arg.id}"], [role="row"][data-payment-id="${arg.id}"]`;
  let row = document.querySelector(sel);
  if (!row) {
    row = Array.from(document.querySelectorAll('tbody tr, [role="row"]'))
      .find((r) => (r.innerText || '').includes(arg.id));
  }
  if (!row) return { found: false };
  if (arg.click) { row.scrollIntoView({ block: 'center' }); row.click(); }
  const r = row.getBoundingClientRect();
  const inViewport = r.top >= 0 && r.bottom <= window.innerHeight;
  return { found: true, dataBrushed: row.getAttribute('data-brushed'),
           inViewport, top: +r.top.toFixed(1),
           visible: !!row.getClientRects().length };
}
function pageBrushCount() {
  const el = document.getElementById('brush-count');
  if (!el) return { present: false };
  return { present: true, text: (el.innerText || '').trim().slice(0, 30),
           visible: !!el.getClientRects().length };
}

// ---------- flow (approval workflow) page functions ----------

function pageSetRoleToken(arg) {
  const el = document.getElementById('role-token');
  if (!el) return { found: false };
  const setter = Object.getOwnPropertyDescriptor(
    el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, 'value');
  if (setter && setter.set) setter.set.call(el, arg.token); else el.value = arg.token;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  const form = el.closest('form');
  const btn = (form && form.querySelector('button, input[type="submit"]')) ||
    document.querySelector('#role-token ~ button, [data-action="set-token"], #set-token');
  if (btn) btn.click();
  else el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  return { found: true, visible: !!el.getClientRects().length, confirmed: !!btn };
}

function pageDraftFormFill(arg) {
  const form = document.getElementById('draft-form');
  if (!form) return { found: false };
  const fire = (el) => {
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const fields = Array.from(form.querySelectorAll('input, textarea, select'));
  const nameOf = (el) => ((el.name || '') + ' ' + (el.id || '') + ' ' +
    (el.getAttribute('placeholder') || '') + ' ' + (el.getAttribute('aria-label') || '')).toLowerCase();
  const set = (re, value, exclude) => {
    const el = fields.find((f) => re.test(nameOf(f)) && (!exclude || !exclude.test(nameOf(f))));
    if (!el) return false;
    if (el.tagName === 'SELECT') {
      const opt = Array.from(el.options).find((o) =>
        (o.value || '').toUpperCase() === String(value).toUpperCase() ||
        (o.text || '').trim().toUpperCase() === String(value).toUpperCase());
      if (opt) el.value = opt.value; else el.value = String(value);
      fire(el);
      return true;
    }
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value');
    if (setter && setter.set) setter.set.call(el, String(value)); else el.value = String(value);
    fire(el);
    return true;
  };
  // counterparty is {name, country} (spec: separate name/country inputs, country ^[A-Z]{2}$);
  // a legacy string counterparty fills the name field alone.
  const cp = arg.counterparty;
  const cpName = cp && typeof cp === 'object' ? cp.name : cp;
  const cpCountry = cp && typeof cp === 'object' ? cp.country : null;
  const filled = {
    amount: set(/amount/, arg.amount_minor),
    currency: set(/currenc/, arg.currency, /countr/),
    counterparty: set(/counterpart|payee|recipient|beneficiar|name/, cpName,
                      /country|currenc|amount|note/),
    country: cpCountry == null ? null : set(/country/, cpCountry),
    note: set(/note|memo|desc/, arg.note),
  };
  let submitted = false;
  const btn = Array.from(form.querySelectorAll('button, input[type="submit"]'))
    .find((b) => b.getClientRects().length && !/cancel|reset/i.test(b.innerText || b.value || ''));
  if (btn) { btn.click(); submitted = true; }
  else if (form.requestSubmit) { form.requestSubmit(); submitted = true; }
  return { found: true, filled, submitted, fieldCount: fields.length };
}

function pageDraftList() {
  const list = document.getElementById('draft-list');
  if (!list) return { found: false, rows: [] };
  const rows = Array.from(list.querySelectorAll('[data-draft-id]')).map((r) => ({
    id: r.getAttribute('data-draft-id'),
    state: r.getAttribute('data-state'),
    visible: !!r.getClientRects().length,
    text: (r.innerText || '').trim().slice(0, 80),
  }));
  return { found: true, rows };
}

// Drives one workflow action on a draft row: named global button (#approve-btn/#reject-btn)
// after selecting the row, else a row-scoped button matching the verb.
function pageDraftAction(arg) {
  const list = document.getElementById('draft-list');
  const row = list ? list.querySelector(`[data-draft-id="${arg.id}"]`) : null;
  const verb = { submit: /submit/i, approve: /approve/i, reject: /reject/i }[arg.kind];
  const globalBtn = arg.kind === 'approve' ? document.getElementById('approve-btn')
    : arg.kind === 'reject' ? document.getElementById('reject-btn')
    : document.getElementById('submit-btn');
  let used = null;
  if (row) {
    row.scrollIntoView({ block: 'center' });
    const rowBtn = Array.from(row.querySelectorAll('button, [role="button"], a'))
      .find((b) => verb.test((b.innerText || '') + ' ' + (b.getAttribute('aria-label') || '')) &&
                   b.getClientRects().length);
    if (rowBtn) { rowBtn.click(); used = 'row-button'; }
    else if (globalBtn && globalBtn.getClientRects().length) {
      row.click();
      globalBtn.click();
      used = 'global-button';
    }
  } else if (globalBtn && globalBtn.getClientRects().length) {
    globalBtn.click();
    used = 'global-button-no-row';
  }
  return { rowFound: !!row, clicked: used != null, used,
           stateNow: row ? row.getAttribute('data-state') : null };
}
// ---------- scenarios ----------

async function main() {
  // F17: a missing expectation must refuse, never improvise.
  let pack = null;
  const packPath = process.env.SB7_EXPECT_FILE || process.env.BENCH_SB7_EXPECT || '';
  if (packPath) {
    try {
      pack = JSON.parse(readFileSync(packPath, 'utf8'));
    } catch (e) {
      pack = null;
      if (isViz) {
        emit({ probeError: 'expectation pack unreadable: ' + String(e.message || e).slice(0, 120) });
        return;
      }
    }
  }
  if (isViz && (!pack || !pack.records || !Array.isArray(pack.records.id))) {
    emit({ probeError: 'SB7_EXPECT_FILE missing or unparsable (viz requires the fixtures_v3 pack)' });
    return;
  }
  let tokens = null;
  try {
    tokens = process.env.BENCH_SB7_TOKENS ? JSON.parse(process.env.BENCH_SB7_TOKENS)
      : (pack && pack.tokens) || null;
  } catch {}
  if (scenario === 'flow' && (!tokens || !tokens.maker || !tokens.checker)) {
    emit({ probeError: 'flow requires tokens (BENCH_SB7_TOKENS or pack.tokens with maker+checker)' });
    return;
  }

  const playwright = loadPlaywright();
  browser = await playwright.chromium.launch({
    headless: true,
    ...(process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE?{executablePath:process.env.GOOSE_SWARM_CHROMIUM_EXECUTABLE}:{}),
    args: isViz ? VIZ_LAUNCH_ARGS : [],
  });
  const mediaDir=isViz?resolve(process.env.BENCH_MEDIA_DIR||join(dirname(process.env.BENCH_SHOTS_DIR||'.'),'bench-media')):null;
  if(mediaDir) mkdirSync(mediaDir,{recursive:true});
  const context = await browser.newContext({
    ...(mediaDir?{recordVideo:{dir:join(mediaDir,'raw'),size:{width:1280,height:800}}}:{}),
    viewport: { width: 1280, height: 800 },
    ...(isViz ? { reducedMotion: 'no-preference' } : {}), // SB7.1 grades actual update motion
  });
  await context.addInitScript(initFirstDataStamp);
  if (isViz) {
    await context.addInitScript(glInstrument);
    await context.addInitScript(streamInstrument);
  }
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
  async function finalizeMedia() {
    if(!mediaDir)return;
    const raw=await page.video().path();
    await context.close();
    const root=dirname(mediaDir),clip=join(mediaDir,'payment-towers.webm'),errors=[];
    let selected=clip;
    try {
      execFileSync(process.env.BENCH_FFMPEG||'ffmpeg',['-hide_banner','-loglevel','error','-sseof','-30','-i',raw,
        '-an','-vf','scale=960:-2','-c:v','libvpx-vp9','-b:v','450k','-threads','1','-y',clip],{timeout:60000});
    } catch(error) { selected=raw;errors.push('Clip encoding failed; full graded recording retained: '+String(error.message).slice(0,180)); }
    const bytes=readFileSync(selected),file=relative(root,selected);
    const manifest={schemaVersion:1,scorerVersion:'sb-7.1-rc',recording:'graded-browser',
      videos:[{file,caption:'Graded payment towers: structural inspection and a real payment-update animation',mimeType:'video/webm',
        scenario:'viz',sha256:createHash('sha256').update(bytes).digest('hex'),bytes:statSync(selected).size,
        selection:selected===clip?'Final 30 seconds of the graded browser recording':'Full graded browser recording',sourceFile:relative(root,raw)}],errors};
    const path=join(mediaDir,'media-manifest.json');writeFileSync(path,JSON.stringify(manifest,null,2)+'\n');
    result.sb71={...(result.sb71||{}),media:{manifest:relative(root,path),...manifest}};
  }

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
      await page.screenshot({ path: join(shotsDir, `${shotEpoch}-mobile.png`), timeout: 5000 });
      await page.setViewportSize({ width: 1280, height: 800 });
    } catch (e) {
      err('mobile screenshot failed:', String((e && e.message) || e).slice(0, 200));
    }
  }

  const consoleErrorTexts = [];
  const consoleErrorSources = [];    // sources[i] names texts[i]'s file; '' = unknown (old-probe shape)
  page.on('console', (m) => {
    if (m.type() !== 'error') return;
    consoleErrorTexts.push(m.text());
    const loc = m.location() || {};
    consoleErrorSources.push(urlToRelPath(loc.url));
  });
  page.on('pageerror', (e) => {
    consoleErrorTexts.push(String(e));
    consoleErrorSources.push(stackFirstRelPath(e && e.stack));
  });
  const consoleErrors = () => ({
    count: consoleErrorTexts.length,
    texts: consoleErrorTexts.slice(0, 3).map((t) => String(t).slice(0, 300)),
    sources: consoleErrorSources.slice(0, 3).map((s) => String(s).slice(0, 200)),
  });
  let navigations = -1;                                  // first goto is not a reload
  page.on('framenavigated', (f) => { if (f === page.mainFrame()) navigations++; });

  const clean = (s) =>
    String(s || '')
      // eslint-disable-next-line no-control-regex
      .replace(/\[[0-9;]*m/g, '')
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
  async function evalRetry(fn, fallback, arg) {
    for (let i = 0; i < 2; i++) {
      try {
        return arg === undefined ? await page.evaluate(fn) : await page.evaluate(fn, arg);
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

  if (scenario === 'boot') {
    // D3 corner: before sync #1 completes — empty-with-progress or block. The harness runs
    // this against a freshly booted tree (vendor may be refusing); the probe only reports.
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, preSyncState: { blocked: true, navigationError: true },
             consoleErrors: consoleErrors() });
      return;
    }
    const t0 = Date.now();
    const timeline = [];
    while (Date.now() - t0 < 6000) {
      const snap = await page.evaluate(pageViewSnapshot).catch(() => null);
      const emptyText = await page.evaluate(pageEmptyState).catch(() => null);
      const last = timeline[timeline.length - 1];
      const rows = snap ? snap.rowCount : 0;
      if (!last || last.rows !== rows || (last.emptyText || '') !== (emptyText || ''))
        timeline.push({ tMs: Date.now() - t0, rows, emptyText });
      await sleep(400);
    }
    const blank = await page.evaluate(pageBlankAndBody).catch(() => ({ blankPage: true, bodyTextLength: 0 }));
    const notif = await page.evaluate(pageNotificationsState).catch(() => ({ present: false }));
    const tablePresent = await page.evaluate(
      () => !!(document.getElementById('payments-table') || document.querySelector('table, [role="table"], [role="grid"]'))
    ).catch(() => false);
    const progressText = await page.evaluate(pageEmptyState).catch(() => null);
    const finalRows = timeline.length ? timeline[timeline.length - 1].rows : 0;
    await saveShot('boot');
    emit({
      preSyncState: {
        tablePresent,
        renderedRowCount: finalRows,
        progressText,
        emptyWithProgress: tablePresent && progressText != null,
        blocked: blank.blankPage && !tablePresent,
        timeline,
      },
      blankPage: blank.blankPage,
      bodyTextLength: blank.bodyTextLength,
      notifications: notif,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'load') {
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, consoleErrors: consoleErrors() });
      return;
    }
    const timeToFirstDataMs = await pollFirstData(15000);
    await waitIdle(10000);
    const analysis = await page.evaluate(pageAnalyzeLoad).catch((e) => {
      err('analysis evaluate failed:', clean(e.message || e));
      return {};
    });
    const notif = await page.evaluate(pageNotificationsState).catch(() => ({ present: false }));
    await saveShot('loaded');
    let horizontalScroll = null;
    try {
      await page.setViewportSize({ width: 375, height: 812 });
      await page.reload({ waitUntil: 'domcontentloaded', timeout: 15000 });
      await waitIdle(5000);
      horizontalScroll = await page.evaluate(pageHorizontalScroll);
    } catch (e) {
      err('viewport375 check failed:', clean(e.message || e));
    }
    await saveShotMobile();
    emit({
      consoleErrors: consoleErrors(),
      timeToFirstDataMs,
      ...analysis,
      tableRendered: { renderedRowCount: analysis.renderedRowCount || 0,
                       domRowCount: analysis.domRowCount || 0,
                       firstDataMs: timeToFirstDataMs },
      notifications: notif,
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
    await sleep(600);                                    // vacuity guard: self-mutating tables
    const before2 = await page.evaluate(pageViewSnapshot).catch(() => null);
    const tableSelfMutates = !!(before && before2) && before.tableHash !== before2.tableHash;
    const bannerBefore = await page.evaluate(pageErrorBanner).catch(() => null);
    const state = await page.evaluate(pageSyncState).catch(() => ({ found: false }));
    if (!state.found) {
      emit({ found: false, syncCausal: { found: false }, consoleErrors: consoleErrors() });
      return;
    }
    err('sync button found:', JSON.stringify(state.text));
    const summaryBefore=await page.request.get(baseUrl+'/api/summary').then(r=>r.ok()?r.json():null).catch(()=>null);
    const clickAt = Date.now();
    let observedSyncPost=false;
    const observeSync=request=>{
      const url=new URL(request.url());
      if(request.method()==='POST'&&url.origin===new URL(baseUrl).origin&&url.pathname==='/api/sync')observedSyncPost=true;
    };
    page.on('request',observeSync);
    const clicked = await page.evaluate(pageClickSync).catch(() => false);
    if (!clicked) {
      emit({ found: true, buttonText: state.text, clicked: false,
             syncCausal: { found: true, clicked: false }, consoleErrors: consoleErrors() });
      return;
    }
    let disabledDuringSync = false;
    while (Date.now() - clickAt < 1200) {
      const s = await page.evaluate(pageSyncState).catch(() => null);
      if (s && (!s.found || s.disabled)) { disabledDuringSync = true; break; }
      await sleep(50);
    }
    let completed = false, completedWithinMs = null, failedAfterMs = null, errorBanner = null;
    let everDisabled = disabledDuringSync, buttonPresentAfter = true, syncRequested = false;
    const syncRequestSeen = () => Promise.resolve(observedSyncPost);
    const capMs = Math.min(70000, Math.max(budgetLeft() - 8000, 2000));
    while (Date.now() - clickAt < capMs) {
      const s = await page.evaluate(pageSyncState).catch(() => null);
      errorBanner = await page.evaluate(pageErrorBanner).catch(() => null);
      if (errorBanner && bannerBefore && errorBanner === bannerBefore) errorBanner = null;
      if (!syncRequested) syncRequested = await syncRequestSeen();
      if (s && s.found && s.disabled) everDisabled = true;
      buttonPresentAfter = !!(s && s.found);
      const enabled = s && s.found && !s.disabled;
      const elapsed = Date.now() - clickAt;
      if (enabled && !errorBanner && (everDisabled || elapsed > 1500)) {
        completed = true;
        completedWithinMs = elapsed;
        break;
      }
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
    const tableHashChanged =
      !!(before2 && after) && after.tableHash !== before2.tableHash && !tableSelfMutates;
    if (!viewRefreshed && tableHashChanged) viewRefreshed = true;
    const paymentsUrl=await page.evaluate(()=>performance.getEntriesByType('resource')
      .filter(e=>{const u=new URL(e.name);return u.origin===location.origin&&u.pathname==='/api/payments';}).at(-1)?.name);
    const refreshed={};
    if(paymentsUrl)refreshed.payments=await page.request.get(paymentsUrl).then(r=>r.ok()?r.json():null).catch(()=>null);
    refreshed.summary=await page.request.get(baseUrl+'/api/summary').then(r=>r.ok()?r.json():null).catch(()=>null);
    let refreshedTruth={ok:false};
    if(refreshed.payments?.data?.length&&refreshed.summary?.last_sync
       &&Date.parse(refreshed.summary.last_sync)>Date.parse(summaryBefore?.last_sync)) {
      refreshedTruth=await page.evaluate(pageSyncTruth,refreshed);
    }
    if (!syncRequested) syncRequested = await syncRequestSeen();
    page.off('request',observeSync);
    viewRefreshed=syncRequested&&refreshedTruth.ok;
    if (completed && !(everDisabled || syncRequested || viewRefreshed || tableHashChanged)) {
      completed = false;
      completedWithinMs = null;
    }
    await saveShot('synced');
    emit({
      found: true, buttonText: state.text, disabledDuringSync, syncRequested,
      completed, completedWithinMs, failedAfterMs, buttonPresentAfter, errorBanner,
      viewRefreshed, tableHashChanged,
      rowCountBefore: before ? before.rowCount : null,
      rowCountAfter: after ? after.rowCount : null,
      syncCausal: { found: true, clicked: true, everDisabled, syncRequested, completed,
                    viewRefreshed, tableHashChanged:tableHashChanged&&refreshedTruth.ok, refreshedTruth,
                    lastSyncBefore:summaryBefore?.last_sync,lastSyncAfter:refreshed.summary?.last_sync },
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
    const notif = await evalRetry(pageNotificationsState, { present: false });
    await saveShot('error');
    emit({
      navigationError,
      errorStateVisible: banner != null,
      actionableText: banner,
      blankPage: blank.blankPage,
      bodyTextLength: blank.bodyTextLength,
      notifications: notif,
      notificationsDegraded: !!(notif && notif.present && notif.dataState === 'degraded'),
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'feed') {
    // §4.2 UI feed: the harness partitions/heals notifierd around this run; the probe watches
    // #notifications[data-state] flip degraded → live WITHOUT a reload (poll ≤ 5 s budget is
    // the scorer's judgment; timestamps are the facts).
    const navigationError = await safeGoto(20000);
    if (navigationError) {
      emit({ navigationError, degradedThenLive: { watched: false }, consoleErrors: consoleErrors() });
      return;
    }
    await waitIdle(8000);
    const t0 = Date.now();
    const timeline = [];
    let sawDegraded = false, liveAfterDegradedAt = null, firstDegradedAt = null, lastDegradedAt = null;
    const capMs = Math.min(75000, Math.max(budgetLeft() - 10000, 5000));
    while (Date.now() - t0 < capMs) {
      const n = await page.evaluate(pageNotificationsState).catch(() => ({ present: false }));
      const stateNow = n.present ? n.dataState : 'ABSENT';
      const last = timeline[timeline.length - 1];
      if (!last || last.state !== stateNow)
        timeline.push({ tMs: Date.now() - t0, state: stateNow, entryCount: n.entryCount || 0 });
      if (stateNow === 'degraded') {
        sawDegraded = true;
        if (firstDegradedAt == null) firstDegradedAt = Date.now() - t0;
        lastDegradedAt = Date.now() - t0;
      }
      if (stateNow === 'live' && sawDegraded && liveAfterDegradedAt == null)
        liveAfterDegradedAt = Date.now() - t0;
      if (liveAfterDegradedAt != null && Date.now() - t0 > liveAfterDegradedAt + 4000) break;
      await sleep(400);
    }
    const finalNotif = await page.evaluate(pageNotificationsState).catch(() => ({ present: false }));
    await saveShot('feed');
    emit({
      degradedThenLive: {
        watched: true,
        sawDegraded,
        liveAfterDegraded: liveAfterDegradedAt != null,
        firstDegradedAtMs: firstDegradedAt,
        degradedToLiveMs: liveAfterDegradedAt != null && lastDegradedAt != null
          ? liveAfterDegradedAt - lastDegradedAt : null,
        timeline: timeline.slice(0, 30),
        reloads: Math.max(0, navigations),
      },
      notifications: finalNotif,
      consoleErrors: consoleErrors(),
    });
  } else if (scenario === 'flow') {
    await flowScenario(page, tokens, pack, { saveShot, consoleErrors, safeGoto, waitIdle, finalizeMedia,
                                             pollFirstData, evalRetry, clean });
  } else if (scenario === 'sb71-visual') {
    const navigationError=await safeGoto(25000);
    if(navigationError) throw new Error(navigationError);
    await page.waitForFunction(()=>window.vs7dbg?.sceneDigest()?.count>0,null,{timeout:15000});
    await sleep(300);
    await sb71VisualScenario(page,buildModel(pack),{saveShot},pack);
    await finalizeMedia();
    emit({consoleErrors:consoleErrors()});
  } else if (scenario === 'viz') {
    await vizScenario(page, pack, { saveShot, consoleErrors, safeGoto, waitIdle, finalizeMedia,
                                    pollFirstData, evalRetry, clean });
  }
}
// §4.5 F1/F2 through the UI: maker creates+submits in #draft-form/#draft-list, checker
// approves via #approve-btn (the approve POST is HELD 800 ms so optimistic paint is causal
// fact, not a race), F2 is rejected, D2 probes resubmit-after-reject. Facts only.
async function flowScenario(page, tokens, pack, H) {
  const { saveShot, consoleErrors, safeGoto, waitIdle, pollFirstData } = H;
  const drafts = (pack && pack.approval && Array.isArray(pack.approval.drafts) && pack.approval.drafts.length >= 2)
    ? pack.approval.drafts
    : [
        { amount_minor: 125000, currency: 'EUR',
          counterparty: { name: 'Probe Rig F1', country: 'DE' }, note: 'probe draft F1' },
        { amount_minor: 98000, currency: 'USD',
          counterparty: { name: 'Probe Rig F2', country: 'US' }, note: 'probe draft F2' },
      ];

  const HOLD_MS = 800;
  const held = { approves: 0, releasedAt: null, submits: 0 };
  await page.route('**/api/drafts/**', async (route) => {
    const req = route.request();
    if (req.method() === 'POST' && /\/api\/drafts\/[^/]+\/approve(\?|$)/.test(req.url())) {
      held.approves++;
      await sleep(HOLD_MS);
      held.releasedAt = Date.now();
      return route.continue().catch(() => {});
    }
    if (req.method() === 'POST' && /\/api\/drafts\/[^/]+\/submit(\?|$)/.test(req.url()))
      held.submits++;
    return route.continue().catch(() => {});
  });

  const navigationError = await safeGoto(20000);
  if (navigationError) {
    emit({ navigationError, roleTokenAccepted: { found: false }, consoleErrors: consoleErrors() });
    return;
  }
  await waitIdle(10000);
  await pollFirstData(8000);
  const tableBefore = await page.evaluate(pageViewSnapshot).catch(() => null);

  const setRole = async (token) => {
    const r = await page.evaluate(pageSetRoleToken, { token }).catch(() => ({ found: false }));
    await sleep(700);
    return r;
  };
  const listIds = async () => {
    const l = await page.evaluate(pageDraftList).catch(() => ({ found: false, rows: [] }));
    return { list: l, ids: new Set((l.rows || []).map((r) => r.id)) };
  };
  const pollState = async (id, want, capMs) => {
    const t0 = Date.now();
    let last = null;
    while (Date.now() - t0 < capMs) {
      const l = await page.evaluate(pageDraftList).catch(() => ({ rows: [] }));
      const row = (l.rows || []).find((r) => r.id === id);
      last = row ? row.state : null;
      if (row && row.state === want) return { reached: true, state: row.state, ms: Date.now() - t0 };
      await sleep(300);
    }
    return { reached: false, state: last, ms: capMs };
  };

  // maker in, F1 created
  const roleSet = await setRole(tokens.maker);
  const before = await listIds();
  const formUsable = await page.evaluate(
    () => {
      const f = document.getElementById('draft-form');
      if (!f || !f.getClientRects().length) return false;
      const inputs = f.querySelectorAll('input, textarea, select');
      return inputs.length > 0 && !Array.from(inputs).every((i) => i.disabled);
    }).catch(() => false);
  merge({ roleTokenAccepted: { found: !!roleSet.found, confirmed: !!roleSet.confirmed,
                               draftListFound: !!before.list.found, formUsable } });
  if (!roleSet.found) {
    await saveShot('flow');
    emit({ consoleErrors: consoleErrors() });
    return;
  }

  const fill1 = await page.evaluate(pageDraftFormFill, drafts[0]).catch(() => ({ found: false }));
  let f1Id = null, f1State = null;
  {
    const t0 = Date.now();
    while (Date.now() - t0 < 6000 && !f1Id) {
      const now = await listIds();
      for (const r of now.list.rows || []) if (!before.ids.has(r.id)) { f1Id = r.id; f1State = r.state; }
      if (!f1Id) await sleep(300);
    }
  }
  merge({ draftCreated: { formFound: !!fill1.found, filled: fill1.filled || null,
                          submittedForm: !!fill1.submitted, draftId: f1Id, stateAtCreate: f1State } });
  if (!f1Id) {
    await saveShot('flow');
    emit({ consoleErrors: consoleErrors() });
    return;
  }

  // F1 submit (maker), then approve (checker) with the held-POST optimistic proof
  const subClick = await page.evaluate(pageDraftAction, { id: f1Id, kind: 'submit' })
    .catch(() => ({ clicked: false }));
  const submitted = await pollState(f1Id, 'submitted', 8000);
  merge({ f1Submit: { ...subClick, ...submitted, requestsSeen: held.submits },
          submitCausal: { clicked: !!subClick.clicked, reached: !!submitted.reached,
                          state: submitted.state, requestsSeen: held.submits } });

  await setRole(tokens.checker);
  const approveClick = await page.evaluate(pageDraftAction, { id: f1Id, kind: 'approve' })
    .catch(() => ({ clicked: false }));
  // optimistic paint, measured: poll the list inside the hold window; paintMs is the time to
  // the first 'approved' row seen while the POST is still provably held.
  const tApprove = Date.now();
  let paintMs = null, midRow = null;
  while (Date.now() - tApprove < HOLD_MS - 60) {
    const l = await page.evaluate(pageDraftList).catch(() => ({ rows: [] }));
    const row = (l.rows || []).find((r) => r.id === f1Id);
    if (row) midRow = row;
    if (row && row.state === 'approved' && held.releasedAt == null) {
      paintMs = Date.now() - tApprove;
      break;
    }
    await sleep(60);
  }
  const heldDuringCheck = held.approves > 0 && held.releasedAt == null;
  const approved = await pollState(f1Id, 'approved', 12000);
  merge({ optimistic: {
    holdMs: HOLD_MS, requestSeen: held.approves, heldDuringCheck,
    stateWhileHeld: midRow ? midRow.state : null,
    paintedWhileHeld: paintMs != null,
    paintMs,
    savedAfterRelease: !!approved.reached,
  } });

  // vendor round-trip: the approved payment lands in the table; the feed shows the journey
  let paymentAppeared = false, rowsAfter = null;
  {
    const t0 = Date.now();
    while (Date.now() - t0 < 20000) {
      const snap = await page.evaluate(pageViewSnapshot).catch(() => null);
      rowsAfter = snap ? snap.rowCount : null;
      if (tableBefore && snap && (snap.rowCount > tableBefore.rowCount ||
          snap.tableHash !== tableBefore.tableHash)) { paymentAppeared = true; break; }
      await sleep(1000);
    }
  }
  const notifMid = await page.evaluate(pageNotificationsState).catch(() => ({ present: false }));
  const notifTexts = (notifMid.texts || []).join(' | ');
  merge({ approveCausal: {
    clicked: !!approveClick.clicked, used: approveClick.used || null,
    requestSeen: held.approves > 0, stateAfter: approved.state, reachedApproved: approved.reached,
    paymentAppeared, rowsBefore: tableBefore ? tableBefore.rowCount : null, rowsAfter,
    notificationSeen: /approv/i.test(notifTexts), submittedNotificationSeen: /submit/i.test(notifTexts),
  },
  // hoisted for the journey checks: top-level booleans, same facts
  notificationSeen: /approv/i.test(notifTexts),
  paymentInTable: paymentAppeared });
  await saveShot('flow-approved');

  // F2: create, submit, reject; then the D2 resubmit probe
  await setRole(tokens.maker);
  const before2 = await listIds();
  await page.evaluate(pageDraftFormFill, drafts[1]).catch(() => ({}));
  let f2Id = null;
  {
    const t0 = Date.now();
    while (Date.now() - t0 < 6000 && !f2Id) {
      const now = await listIds();
      for (const r of now.list.rows || []) if (!before2.ids.has(r.id) && r.id !== f1Id) f2Id = r.id;
      if (!f2Id) await sleep(300);
    }
  }
  let rejectCausal = { draftId: f2Id, clicked: false };
  let d2Resubmit = { attempted: false };
  if (f2Id) {
    await page.evaluate(pageDraftAction, { id: f2Id, kind: 'submit' }).catch(() => ({}));
    await pollState(f2Id, 'submitted', 8000);
    await setRole(tokens.checker);
    const rejClick = await page.evaluate(pageDraftAction, { id: f2Id, kind: 'reject' })
      .catch(() => ({ clicked: false }));
    const rejected = await pollState(f2Id, 'rejected', 10000);
    // Harness fix: an optimistic app paints 'rejected' instantly, which made this read
    // land ~50 ms after the click — before any relay could run. The feed owns a ≤5 s
    // poll cadence, so the notification gets that window to appear.
    let rejNotifSeen = false;
    {
      const tN = Date.now();
      while (Date.now() - tN < 7000 && !rejNotifSeen) {
        const nf = await page.evaluate(pageNotificationsState)
          .catch(() => ({ present: false }));
        if (/reject/i.test((nf.texts || []).join(' | '))) rejNotifSeen = true;
        else await sleep(400);
      }
    }
    rejectCausal = { draftId: f2Id, clicked: !!rejClick.clicked, used: rejClick.used || null,
                     stateAfter: rejected.state, reachedRejected: rejected.reached,
                     notificationSeen: rejNotifSeen };
    // D2 corner: terminal vs resubmittable — attempt a maker resubmit, report what happened.
    await setRole(tokens.maker);
    const resub = await page.evaluate(pageDraftAction, { id: f2Id, kind: 'submit' })
      .catch(() => ({ clicked: false }));
    await sleep(1500);
    const l = await page.evaluate(pageDraftList).catch(() => ({ rows: [] }));
    const row = (l.rows || []).find((r) => r.id === f2Id);
    d2Resubmit = { attempted: true, controlClicked: !!resub.clicked,
                   stateAfter: row ? row.state : null };
  }
  merge({ rejectCausal, d2Resubmit });

  const finalList = await page.evaluate(pageDraftList).catch(() => ({ found: false, rows: [] }));
  merge({ draftListStates: finalList });
  await saveShot('flow');
  emit({ consoleErrors: consoleErrors() });
}
// ── viz probe-side helpers ───────────────────────────────────────────────────────────────────
const median = (a) => {
  if (!a.length) return null;
  const s = [...a].sort((x, y) => x - y);
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
};
// Draw timestamps → frame groups (draws within 4 ms belong to one rendered frame).
function frameGroups(ts) {
  const starts = [];
  for (let i = 0; i < ts.length; i++)
    if (i === 0 || ts[i] - ts[i - 1] > 4) starts.push(ts[i]);
  return starts;
}
function decodePickPixel(px) {
  if (!Array.isArray(px) || px.length < 3) return null;
  const idNum = px[0] + 256 * px[1] + 65536 * px[2];
  return idNum === 0 ? 0 : idNum;                        // 0 = background, else n+1
}
// Seed-deterministic search for §3.3's four occlusion constructions plus plain fronts.
function findPickTargets(ctx, model, seed) {
  const rng = seedRng(seed, 'picks');
  const order = model.items.map((_, i) => i);
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }
  const found = { occludedLowerN: null, occludedHigherN: null, partial: null,
                  background: null, fronts: [] };
  let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  const seen = [];
  for (const n of order) {
    if (found.occludedLowerN && found.occludedHigherN && found.partial && found.fronts.length >= 2) break;
    const it = model.items[n];
    const p = projectPt(ctx.eye, ctx.basis, ctx.W, ctx.H, [it.x, it.h, it.z]);
    if (!p || p.x < 8 || p.x > ctx.W - 8 || p.y < 8 || p.y > ctx.H - 8) continue;
    seen.push(p);
    x0 = Math.min(x0, p.x); x1 = Math.max(x1, p.x); y0 = Math.min(y0, p.y); y1 = Math.max(y1, p.y);
    const d = decisiveAt(ctx, p.x, p.y);
    if (!d.decisive || d.front == null) continue;
    const front = model.items[d.front];
    const target = { sx: +p.x.toFixed(2), sy: +p.y.toFixed(2), frontN: d.front,
                     frontId: front.id, depthGap: d.depthGap === Infinity ? null : +d.depthGap.toFixed(5) };
    if (d.front === n) {
      if (found.fronts.length < 2) found.fronts.push({ ...target, cls: 'front' });
      continue;
    }
    const occludedInHits = d.hits.some((h) => h.n === n);
    if (!occludedInHits) continue;                       // n does not lie on this ray at all
    if (front.n < n && !found.occludedLowerN)
      found.occludedLowerN = { ...target, cls: 'occluded-by-lower-n', occludedN: n, occludedId: it.id };
    else if (front.n > n && !found.occludedHigherN)
      found.occludedHigherN = { ...target, cls: 'occluded-by-higher-n', occludedN: n, occludedId: it.id };
    else if (!found.partial) {
      // partial: the occluded instance is directly visible somewhere else on screen
      for (const [ox, oy] of [[-6, 0], [6, 0], [0, -6], [0, 6], [-9, 0], [9, 0],
                              [0, -9], [0, 9], [-12, 0], [12, 0], [-6, -6], [6, -6]]) {
        const q = decisiveAt(ctx, p.x + ox, p.y + oy);
        if (q.decisive && q.front === n) {
          found.partial = { ...target, cls: 'partial-occlusion', occludedN: n, occludedId: it.id,
                            visibleAt: { sx: +(p.x + ox).toFixed(2), sy: +(p.y + oy).toFixed(2) } };
          break;
        }
      }
    }
  }
  if (seen.length >= 3 && Number.isFinite(x0)) {
    for (let tries = 0; tries < 400 && !found.background; tries++) {
      const sx = x0 + rng() * (x1 - x0), sy = y0 + rng() * (y1 - y0);
      if (sx < 8 || sx > ctx.W - 8 || sy < 8 || sy > ctx.H - 8) continue;
      const d = decisiveAt(ctx, sx, sy);
      if (d.decisive && d.front == null)
        found.background = { sx: +sx.toFixed(2), sy: +sy.toFixed(2), cls: 'background-in-hull',
                             frontN: null, frontId: null };
    }
  }
  return found;
}
// Loose sample points for pixel-motion evidence: center-ray front with ±1 px 4-neighbor
// agreement — enough to know WHICH instance's face the pixel shows, without the full
// decisive margins (harness fix: at far poses columns are ~2-3 px wide, so full
// decisiveness is unreachable while motion detection only needs a stable face pixel).
function findLoosePoints(ctx, model, seed, count) {
  const rng = seedRng(seed, 'loosepts');
  const order = model.items.map((_, i) => i);
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }
  const out = [];
  for (const n of order) {
    if (out.length >= count) break;
    const it = model.items[n];
    const p = projectPt(ctx.eye, ctx.basis, ctx.W, ctx.H, [it.x, it.h * 0.5, it.z]);
    if (!p || p.x < 8 || p.x > ctx.W - 8 || p.y < 8 || p.y > ctx.H - 8) continue;
    const c0 = castPixel(ctx, p.x, p.y);
    if (!c0.length) continue;
    const f = c0[0].n;
    let ok = true;
    for (const [ox, oy] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) {
      const h = castPixel(ctx, p.x + ox, p.y + oy);
      if (!h.length || h[0].n !== f) { ok = false; break; }
    }
    if (!ok) continue;
    out.push({ sx: +p.x.toFixed(2), sy: +p.y.toFixed(2), frontN: f,
               factor:c0[0].factor, top: Math.abs(c0[0].hitY - model.items[f].h) <= 1e-6 });
  }
  return out;
}
// A decisive point whose front is EXACTLY the wanted instance (for brush/height evidence).
function findDecisivePointFor(ctx, model, n) {
  const it = model.items[n];
  const cand = [[it.x, it.h, it.z], [it.x, it.h * 0.5, it.z],
                [it.x - 0.3, it.h, it.z], [it.x + 0.3, it.h, it.z], [it.x, it.h, it.z + 0.3],
                [it.x, it.h, it.z - 0.3], [it.x, it.h * 0.75, it.z], [it.x, it.h * 0.25, it.z],
                [it.x - 0.3, it.h * 0.5, it.z], [it.x + 0.3, it.h * 0.5, it.z]];
  for (const w of cand) {
    const p = projectPt(ctx.eye, ctx.basis, ctx.W, ctx.H, w);
    if (!p || p.x < 8 || p.x > ctx.W - 8 || p.y < 8 || p.y > ctx.H - 8) continue;
    const d = decisiveAt(ctx, p.x, p.y);
    if (d.decisive && d.front === n)
      return { sx: +p.x.toFixed(2), sy: +p.y.toFixed(2),
               factor:d.hit.factor, top: Math.abs((d.hit && d.hit.hitY) - it.h) <= 1e-6 };
  }
  return null;
}

// §3 — the instanced field, ONE session. Sections merge incrementally (F18): a cap hit ships
// everything measured; score_sb7 reads absent sections on timedOut as PROBE UNAVAILABLE.
// Browser screenshots are PNG RGB/RGBA, 8-bit, non-interlaced. Read their composited pixels,
// so a covered/transparent canvas cannot pass using an unseen WebGL framebuffer.
function screenshotPixels(png) {
  let width,height,color,depth,interlace;const data=[];
  for(let pos=8;pos+12<=png.length;) {
    const size=png.readUInt32BE(pos),kind=png.toString('ascii',pos+4,pos+8),body=png.subarray(pos+8,pos+8+size);
    if(kind==='IHDR'){width=body.readUInt32BE(0);height=body.readUInt32BE(4);depth=body[8];color=body[9];interlace=body[12];}
    if(kind==='IDAT')data.push(body);
    pos+=size+12;
  }
  if(depth!==8||![2,6].includes(color)||interlace!==0)throw new Error('Unsupported screenshot PNG encoding');
  const channels=color===2?3:4,stride=width*channels,raw=inflateSync(Buffer.concat(data)),pixels=Buffer.alloc(height*stride);
  const paeth=(a,b,c)=>{const p=a+b-c,pa=Math.abs(p-a),pb=Math.abs(p-b),pc=Math.abs(p-c);return pa<=pb&&pa<=pc?a:pb<=pc?b:c;};
  for(let y=0;y<height;y++){
    const filter=raw[y*(stride+1)];
    if(filter>4)throw new Error('Invalid screenshot PNG filter');
    for(let x=0;x<stride;x++){
      const a=x>=channels?pixels[y*stride+x-channels]:0,b=y?pixels[(y-1)*stride+x]:0,c=y&&x>=channels?pixels[(y-1)*stride+x-channels]:0;
      const predict=filter===0?0:filter===1?a:filter===2?b:filter===3?Math.floor((a+b)/2):paeth(a,b,c);
      pixels[y*stride+x]=(raw[y*(stride+1)+1+x]+predict)&255;
    }
  }
  return {width,height,at(x,y){x=Math.round(x);y=Math.round(y);if(x<0||y<0||x>=width||y>=height)return null;const o=y*stride+x*channels;return Array.from(pixels.subarray(o,o+3));}};
}

// SB7.1 geometry and animation evidence: independent rays versus the same visible framebuffer.
function inspectorPose(it, W, H, yawDegrees=35) {
  const target = [it.x, it.h / 2, it.z], yaw = deg(yawDegrees), pitch = deg(25), distance = 6;
  const eye = [target[0] + distance*Math.cos(pitch)*Math.sin(yaw),
    target[1] + distance*Math.sin(pitch), target[2] + distance*Math.cos(pitch)*Math.cos(yaw)];
  const f = norm(sub(target,eye)), r = norm(cross(f,[0,1,0])), u = cross(r,f);
  return {eye,basis:{f,r,u},W,H};
}
function towerRay(it, pose, x, y, offset=0) {
  const direction=unprojectDir(pose.basis,pose.W,pose.H,x,y);
  let hit=null;
  for(const part of towerParts(it,offset)) {
    const t=rayBox(pose.eye,direction,part.mn,part.mx);
    if(t!=null && (hit==null || t<hit.t)) hit={...part,t};
  }
  if(!hit) return {part:'void',rgb:V7.bg};
  const point=pose.eye.map((v,i)=>v+direction[i]*hit.t),factor=surfaceFactor(it,hit,point);
  return {part:hit.name,factor,rgb:V7.status[it.status].map(v=>Math.round(v*factor))};
}
function inspectorGrid(it,pose) {
  const corners=[];
  for(const x of [it.x-.48,it.x+.48]) for(const y of [-.04*it.h,1.04*it.h]) for(const z of [it.z-.48,it.z+.48])
    corners.push(projectPt(pose.eye,pose.basis,pose.W,pose.H,[x,y,z]));
  const left=Math.max(2,Math.floor(Math.min(...corners.map(p=>p.x)))-4);
  const right=Math.min(pose.W-3,Math.ceil(Math.max(...corners.map(p=>p.x)))+4);
  const top=Math.max(2,Math.floor(Math.min(...corners.map(p=>p.y)))-4);
  const bottom=Math.min(pose.H-3,Math.ceil(Math.max(...corners.map(p=>p.y)))+4);
  const points=[];
  for(let y=top;y<=bottom;y+=3) for(let x=left;x<=right;x+=3) points.push({cx:x,cy:y});
  return points;
}
const rgbNear=(a,b)=>Array.isArray(a)&&a.length>=3&&a.slice(0,3).every((v,i)=>Math.abs(v-b[i])<=8);
function seededSurfacePoints(ctx,skip=()=>false) {
  const points=[];
  for(let y=10;y<ctx.H;y+=7)for(let x=10;x<ctx.W;x+=7){
    if(skip(x,y))continue;
    const hit=castPixel(ctx,x+.5,y+.5)[0];if(!hit)continue;
    const expected=surfColor(ctx,hit);
    const stable=[[-.45,0],[.45,0],[0,-.45],[0,.45]].every(([dx,dy])=>{
      const nearby=castPixel(ctx,x+.5+dx,y+.5+dy)[0];
      return nearby&&nearby.n===hit.n&&rgbNear(surfColor(ctx,nearby),expected);
    });
    if(stable)points.push({cx:x,cy:y,n:hit.n,expected});
    if(points.length>=120)return points;
  }
  return points;
}
function geometryEvidence(it,pose,samples) {
  const groups=Object.fromEntries(['pedestal','shaft','cap','collar','void'].map(k=>[k,{matched:0,total:0,examples:[]}]));
  for(const sample of samples) {
    const x=sample.rayX,y=sample.rayY;
    const expected=towerRay(it,pose,x,y);
    const neighbors=O9.map(([dx,dy])=>towerRay(it,pose,x+dx,y+dy));
    if(neighbors.some(n=>n.part!==expected.part||!rgbNear(n.rgb,expected.rgb))) continue;
    const group=groups[expected.part],ok=rgbNear(sample.got,expected.rgb);
    group.total++;group.matched+=Number(ok);
    if(group.examples.length<3 || !ok && group.examples.every(e=>e.ok)) {
      if(group.examples.length>=3) group.examples.pop();
      group.examples.push({x:sample.cx,y:sample.cy,got:sample.got,expected:expected.rgb,ok});
    }
  }
  return groups;
}
function pageArmSb71Capture({id,points,source}) {
  const P=window.__p7;
  const capture={id,source,frames:[],event:null};P.sb71Capture=capture;
  const begin=(event)=>{
    if(!P.sb71Capture || P.sb71Capture.event) return;
    P.sb71Capture.event=event;
    for(const delay of [120,200,280,360,440,520,600,680,760,840,1150]) setTimeout(()=>{
      if(P.sb71Capture!==capture)return;
      const canvas=document.getElementById('viz3d');
      const gl=canvas&&(canvas.getContext('webgl2')||canvas.getContext('webgl'));
      if(!gl) return;
      gl.bindFramebuffer(gl.FRAMEBUFFER,null);
      const rect=canvas.getBoundingClientRect(),W=canvas.width,H=canvas.height;
      const px=new Uint8Array(W*H*4),at=performance.now();
      gl.readPixels(0,0,W,H,gl.RGBA,gl.UNSIGNED_BYTE,px);
      const samples=points.map(p=>{
        const x=Math.round(p.cx*W/rect.width),y=H-1-Math.round(p.cy*H/rect.height),o=(y*W+x)*4;
        return {...p,rayX:(x+.5)*rect.width/W,rayY:(H-y-.5)*rect.height/H,got:Array.from(px.subarray(o,o+3))};
      });
      P.sb71Capture.frames.push({elapsed:at-event.t0,renderElapsed:P.sb71DrawTimes.has(canvas)?P.sb71DrawTimes.get(canvas)-event.t0:null,samples,camera:window.vs7dbg?.camera()});
    },delay);
  };
  if(source==='live') P.sb71ArmCapture=(entry)=>{
    const record=(entry.records||[]).find(r=>r.id===id);
    if(record) { P.sb71ArmCapture=null; begin({t0:entry.t0,record,batch:entry.batch}); }
  };
  else document.addEventListener('click',function listener(event){
    if(event.target.closest('#replay-event')) {
      document.removeEventListener('click',listener,true);
      begin({t0:performance.now(),id});
    }
  },true);
}
function animationEvidence(it,pose,capture,minMoving=9) {
  const frames=[];
  for(const frame of capture?.frames||[]) {
    const renderTime=frame.renderElapsed;
    if(!Number.isFinite(renderTime)){frames.push({elapsed:frame.elapsed,validTiming:false,compared:0,matched:0,witnesses:0,positiveWitnesses:0,cameraFixed:false});continue;}
    const phase=clamp(renderTime/1000,0,1),offset=-.56*(1-phase*phase*(3-2*phase));
    let compared=0,matched=0,witnesses=0,witnessMatches=0,positiveWitnesses=0,positiveMatches=0;
    const examples=[];
    for(const sample of frame.samples) {
      const x=sample.rayX,y=sample.rayY;
      const desired=towerRay(it,pose,x,y,offset);
      const stationary=towerRay(it,pose,x,y,0);
      const robust=[-16,16].every(dt=>{
        const t=clamp((renderTime+dt)/1000,0,1),off=-.56*(1-t*t*(3-2*t));
        return O9.every(([dx,dy])=>
          rgbNear(towerRay(it,pose,x+dx,y+dy,off).rgb,desired.rgb));
      });
      if(!robust) continue;
      const ok=rgbNear(sample.got,desired.rgb); compared++;matched+=Number(ok);
      if(!rgbNear(desired.rgb,stationary.rgb)) {
        witnesses++;witnessMatches+=Number(ok);
        if(desired.part==='collar'){positiveWitnesses++;positiveMatches+=Number(ok);}
        if(examples.length<4) examples.push({x:sample.cx,y:sample.cy,got:sample.got,expected:desired.rgb,stationary:stationary.rgb,ok});
      }
    }
    const camera=frame.camera;
    const cameraFixed=!!camera&&Math.abs(camera.yaw-35)<.01&&Math.abs(camera.pitch-25)<.01&&Math.abs(camera.distance-6)<.01;
    frames.push({elapsed:frame.elapsed,renderElapsed:renderTime,frameAge:frame.elapsed-renderTime,compared,matched,witnesses,witnessMatches,positiveWitnesses,positiveMatches,cameraFixed,examples});
  }
  const moving=frames.filter(f=>f.elapsed<900),rest=frames.find(f=>f.elapsed>=1050);
  const eligible=moving.filter(f=>f.witnesses>=3&&f.positiveWitnesses>=3);
  const motionOk=eligible.length>=minMoving&&eligible.every(f=>f.witnessMatches/f.witnesses>=.9&&f.positiveMatches/f.positiveWitnesses>=.9)&&moving.every(f=>f.matched/Math.max(1,f.compared)>=.97&&f.cameraFixed);
  const settled=!!rest&&rest.matched/Math.max(1,rest.compared)>=.97&&rest.cameraFixed;
  return {ok:motionOk&&settled,eligibleMotionFrames:eligible.length,excludedMotionFrames:moving.filter(f=>f.witnesses<3||f.positiveWitnesses<3).map(f=>({elapsed:f.elapsed,witnesses:f.witnesses,positiveWitnesses:f.positiveWitnesses,reason:'Insufficient stable occupied-collar witnesses'})),frames,event:capture?.event||null};
}
function visibleMotionEvidence(it,pose,capture) {
  const first=capture.frames[0],rest=capture.frames[1],trials=[];
  if(!first||!rest)return {ok:false,error:'Missing composited capture'};
  // Chromium screenshot returns asynchronously. Its actual compositor instant is inside
  // the measured request interval, never the arbitrary midpoint of that interval.
  for(let elapsed=first.captureStart;elapsed<=first.captureEnd;elapsed+=16) {
    const evidence=animationEvidence(it,pose,{frames:[{...first,elapsed,renderElapsed:elapsed},{...rest,renderElapsed:rest.elapsed}]},1);
    trials.push(evidence);
  }
  const selected=trials.find(t=>t.ok)||trials.sort((a,b)=>b.frames[0].witnessMatches-a.frames[0].witnessMatches)[0];
  return {...selected,captureIntervals:capture.frames.map(f=>({start:f.captureStart,end:f.captureEnd,duration:f.captureDuration})),timing:'Compositor instant bounded by screenshot request/response; raw framebuffer trajectory independently timed'};
}
async function captureVisibleMotion(page,points) {
  await page.waitForFunction(()=>window.__p7?.sb71Capture?.event,null,{timeout:8000});
  const frames=[],cdp=await page.context().newCDPSession(page);
  for(const desired of [300,1170]) {
    const elapsed=await page.evaluate(()=>performance.now()-window.__p7.sb71Capture.event.t0);
    if(desired>elapsed)await sleep(desired-elapsed);
    const before=await page.evaluate(()=>({elapsed:performance.now()-window.__p7.sb71Capture.event.t0,camera:window.vs7dbg.camera()}));
    const rect=await page.evaluate(pageCanvasRect),image=await cdp.send('Page.captureScreenshot',{format:'png',fromSurface:true,optimizeForSpeed:true}),bitmap=screenshotPixels(Buffer.from(image.data,'base64'));
    const after=await page.evaluate(()=>performance.now()-window.__p7.sb71Capture.event.t0);
    frames.push({elapsed:(before.elapsed+after)/2,captureStart:before.elapsed,captureEnd:after,captureDuration:after-before.elapsed,camera:before.camera,
      samples:points.map(p=>({...p,rayX:Math.round(rect.left+p.cx)+.5-rect.left,rayY:Math.round(rect.top+p.cy)+.5-rect.top,got:bitmap.at(rect.left+p.cx,rect.top+p.cy)}))});
  }
  await cdp.detach();
  return {frames};
}
async function sb71VisualScenario(page,model,H,pack) {
  const checks=[];
  const add=(check,tier,score,detail,parts)=>checks.push({check,tier,score:Number.isFinite(score)?+score.toFixed(4):0,detail,parts});
  await page.evaluate(pageScrollCanvasIntoView);
  const rect=await page.evaluate(pageCanvasRect);
  const dom=await page.evaluate(()=>{
    const c=document.getElementById('viz3d'); if(!c)return {visible:false};
    const r=c.getBoundingClientRect(),s=getComputedStyle(c);
    const points=[];
    for(let x=1;x<6;x++)for(let y=1;y<4;y++){
      const element=document.elementFromPoint(r.left+r.width*x/6,r.top+r.height*y/4);
      points.push(element===c);
    }
    const failures=Array.from(document.querySelectorAll('#viz-nogl,#viz-error')).filter(e=>!e.hidden&&e.getClientRects().length&&getComputedStyle(e).display!=='none').map(e=>e.textContent.trim());
    return {visible:r.width>=300&&r.height>=240&&s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)>0,
      exposed:points.filter(Boolean).length,total:points.length,failures,rect:{width:r.width,height:r.height}};
  });
  const labelRects=await page.evaluate(()=>Array.from(document.querySelectorAll('#viz-labels .viz-label')).filter(e=>e.getClientRects().length&&getComputedStyle(e).display!=='none').map(e=>{const r=e.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom};}));
  let rootFacts={dom,matched:0,total:0,ids:[],labelRects};
  if(rect?.w&&rect?.h) {
    const ctx=poseCtx(model,V7.yaw0,V7.pitch0,V7.dist0,rect.w,rect.h);
    const points=seededSurfacePoints(ctx,(x,y)=>labelRects.some(r=>rect.left+x>=r.left-1&&rect.left+x<=r.right+1&&rect.top+y>=r.top-1&&rect.top+y<=r.bottom+1));
    const sampled=await page.evaluate(pageSamplePixels,{points});
    rootFacts.total=points.length;
    rootFacts.framebufferMatched=(sampled.samples||[]).filter(s=>rgbNear(s.got,s.expected)).length;
    const screenshot=screenshotPixels(await page.screenshot());
    rootFacts.matched=points.filter(p=>[-1,0,1].some(dx=>[-1,0,1].some(dy=>rgbNear(screenshot.at(rect.left+p.cx+dx,rect.top+p.cy+dy),p.expected)))).length;
    rootFacts.registrationTolerancePixels=1;
    rootFacts.screenshotSize={width:screenshot.width,height:screenshot.height};
    rootFacts.ids=[...new Set(points.map(p=>model.items[p.n].id))];
  }
  const admitted=dom.visible&&dom.exposed/Math.max(1,dom.total)>=.9&&!dom.failures.length&&rootFacts.total>=12&&rootFacts.ids.length>=8&&rootFacts.matched/rootFacts.total>=.95;
  add('s_visible_surface','S',Number(admitted),'Visible canvas, unobscured samples and independently predicted seeded-payment pixels from this browser',rootFacts);
  await H.saveShot('sb71-field');
  const candidates=['EUR','USD','JPY','KWD'].map(cur=>model.items.filter(it=>it.cur===cur&&!(pack.sb71_reserved_payment_ids||[]).includes(it.id)).sort((a,b)=>b.h-a.h||a.id.localeCompare(b.id))[0]);
  const cases=[],contexts=[],framing=[];
  for(const it of candidates.filter(Boolean)) {
    if(!await page.locator('#inspect-payment').count()||!await page.locator('#inspect-open').count())break;
    await page.locator('#inspect-payment').fill(it.id);
    await page.locator('#inspect-open').click();
    await page.evaluate(pageScrollCanvasIntoView);
    for(const yaw of [35,125]) {
      await page.evaluate(pageVs7,{want:[],setCamera:[yaw,25,6]});
      await sleep(180);
      const box=await page.evaluate(pageCanvasRect),pose=inspectorPose(it,box.w,box.h,yaw),points=inspectorGrid(it,pose);
      const sample=await page.evaluate(pageSamplePixels,{points});
      const groups=geometryEvidence(it,pose,sample.samples||[]);
      cases.push({id:it.id,currency:it.cur,height:it.h,yaw,groups});
      const corners=[];for(const x of [it.x-.45,it.x+.45])for(const y of [0,it.h])for(const z of [it.z-.45,it.z+.45])corners.push(projectPt(pose.eye,pose.basis,pose.W,pose.H,[x,y,z]));
      const bounds={left:Math.min(...corners.map(p=>p.x)),right:Math.max(...corners.map(p=>p.x)),top:Math.min(...corners.map(p=>p.y)),bottom:Math.max(...corners.map(p=>p.y))};
      const labels=await page.evaluate(()=>{const canvas=document.getElementById('viz3d').getBoundingClientRect();return Array.from(document.querySelectorAll('#tower-annotations [data-part]')).map(e=>{const r=e.getBoundingClientRect();return {part:e.dataset.part,text:e.textContent,left:r.left-canvas.left,right:r.right-canvas.left,top:r.top-canvas.top,bottom:r.bottom-canvas.top};});});
      const overlap=(a,b)=>a.left<b.right&&a.right>b.left&&a.top<b.bottom&&a.bottom>b.top;
      const expectedParts={cap:[.95,.90],collar:[.80,{EUR:.62,USD:.70,JPY:.78,KWD:.86}[it.cur]],shaft:[.45,.54],pedestal:[.06,.90]};
      const annotationOk=Object.entries(expectedParts).every(([name,[height,width]])=>{
        const a=labels.find(l=>l.part===name),anchor=projectPt(pose.eye,pose.basis,pose.W,pose.H,[it.x,it.h*height,it.z]);
        return a&&a.text.toLowerCase().includes(name)&&a.text.includes(width.toFixed(2))&&Math.abs((a.top+a.bottom)/2-anchor.y)<=24&&a.left>=0&&a.right<=box.w&&a.top>=0&&a.bottom<=box.h&&!overlap(a,bounds)&&!labels.some(b=>a!==b&&overlap(a,b));
      });
      const heightFraction=(bounds.bottom-bounds.top)/box.h,unclipped=bounds.left>=0&&bounds.right<=box.w&&bounds.top>=0&&bounds.bottom<=box.h;
      framing.push({id:it.id,yaw,bounds,labels,heightFraction,unclipped,annotationOk,ok:unclipped&&heightFraction>=.4&&heightFraction<=.9&&annotationOk&&Object.values(groups).every(g=>g.total>=3&&g.matched/g.total>=.97)});
      if(yaw===35)await H.saveShot('sb71-inspect-'+it.cur.toLowerCase());
    }
    const backend=await page.request.get(baseUrl+'/api/payments/'+encodeURIComponent(it.id)).then(r=>r.json());
    const fields=await page.evaluate(()=>Object.fromEntries(['id','currency','amount','status','version'].map(k=>[k,document.getElementById('inspect-'+k)?.textContent?.trim()||''])));
    const amount=fields.amount.replace(/[^0-9.,]/g,''),digits=amount.replace(/[^0-9]/g,'').replace(/^0+(?=\d)/,'');
    const exponent=V7.exp[it.cur],decimals=exponent===0?!/[.,]\d{1,2}$/.test(amount):new RegExp('[.,]\\d{'+exponent+'}$').test(amount);
    const moneyOk=digits===String(Math.abs(backend.amount_minor))&&decimals;
    contexts.push({id:it.id,currency:it.cur,fields,expected:{id:backend.id,currency:backend.currency,status:backend.status,version:backend.version,amount_minor:backend.amount_minor},moneyOk,
      ok:fields.id===backend.id&&fields.currency===backend.currency&&fields.status===backend.status&&fields.version===String(backend.version)&&moneyOk});
  }
  const groups=cases.flatMap(c=>Object.entries(c.groups).filter(([name])=>name!=='collar').map(([name,g])=>({currency:c.currency,name,...g})));
  const partScore=groups.length===32?groups.reduce((sum,g)=>sum+(g.total>=3?g.matched/g.total:0),0)/32:0;
  add('s_tower_geometry','S',partScore,'Four currencies: pedestal, shaft, cap and shoulder voids match independent stepped geometry', {cases});
  const collars=cases.map(c=>({currency:c.currency,...c.groups.collar}));
  add('s_currency_collar','S',collars.length===8?collars.reduce((sum,g)=>sum+(g.total>=3?g.matched/g.total:0),0)/8:0,'Currency-specific collar widths match projected pixel surfaces',{collars});
  add('q_payment_context','Q',contexts.length===4?contexts.filter(c=>c.ok).length/4:0,'Inspector identity, currency, money, status and version match the backend',{contexts});
  const presentation=[];
  for(const presentationId of ['tower-legend','inspect-details','inspector-controls','tower-annotations']) {
    await page.locator('#'+presentationId).scrollIntoViewIfNeeded().catch(()=>{});
    const evidence=await page.evaluate((id)=>{
    const ids=[id];
    const rgb=value=>{const n=value.match(/[\d.]+/g);return n?n.map(Number):[0,0,0,0];};
    const luminance=color=>color.slice(0,3).map(v=>{v/=255;return v<=.04045?v/12.92:Math.pow((v+.055)/1.055,2.4);}).reduce((a,v,i)=>a+v*[.2126,.7152,.0722][i],0);
    const background=element=>{
      const parents=[];for(let e=element;e;e=e.parentElement)parents.unshift(e);
      let color=[255,255,255];
      for(const e of parents){const layer=rgb(getComputedStyle(e).backgroundColor),alpha=layer.length>3?layer[3]:1;color=color.map((v,i)=>layer[i]*alpha+v*(1-alpha));}
      return color;
    };
    return ids.map(id=>{
      const root=document.getElementById(id);if(!root)return {id,ok:false};
      const walker=document.createTreeWalker(root,NodeFilter.SHOW_TEXT),nodes=[];
      while(walker.nextNode())if(/[\p{L}\p{N}]/u.test(walker.currentNode.textContent))nodes.push(walker.currentNode);
      const elements=nodes.map(node=>{
        const e=node.parentElement,s=getComputedStyle(e),range=document.createRange();range.selectNodeContents(node);
        const boxes=Array.from(range.getClientRects());
        const front=luminance(rgb(s.color)),back=luminance(background(e)),contrast=(Math.max(front,back)+.05)/(Math.min(front,back)+.05);
        const noClip=boxes.every(r=>{for(let p=e;p;p=p.parentElement){const b=p.getBoundingClientRect(),style=getComputedStyle(p);if(/hidden|clip|scroll|auto/.test(style.overflow)&& (r.left<b.left-1||r.right>b.right+1||r.top<b.top-1||r.bottom>b.bottom+1))return false;}return r.left>=0&&r.right<=innerWidth;});
        const exposed=boxes.every(r=>[.2,.5,.8].every(f=>{const x=r.left+r.width*f,y=r.top+r.height/2;if(y<0||y>=innerHeight)return false;const top=document.elementFromPoint(x,y);return getComputedStyle(e).pointerEvents==='none'||top===e||e.contains(top);}));
        return {boxes:boxes.map(r=>({left:r.left,top:r.top,width:r.width,height:r.height})),color:rgb(s.color).slice(0,3),text:node.textContent.trim(),fontSize:parseFloat(s.fontSize),contrast,noClip,exposed,ok:boxes.length>0&&s.visibility!=='hidden'&&Number(s.opacity)>0&&parseFloat(s.fontSize)>=12&&contrast>=4.5&&noClip&&exposed};
      });
      return {id,elements,ok:elements.length>0&&elements.every(e=>e.ok)};
    });
  },presentationId);
    const bitmap=screenshotPixels(await page.screenshot());
    for(const row of evidence)for(const e of row.elements||[]) {
      let ink=0;for(const r of e.boxes)for(let y=Math.ceil(r.top);y<r.top+r.height;y++)for(let x=Math.ceil(r.left);x<r.left+r.width;x++)if(rgbNear(bitmap.at(x,y),e.color))ink++;
      e.visibleInkPixels=ink;e.ok=e.ok&&ink>=3;
    }
    for(const row of evidence)row.ok=!!row.elements?.length&&row.elements.every(e=>e.ok);
    presentation.push(...evidence);
  }
  add('q_legible_presentation','Q',(presentation.filter(p=>p.ok).length/4)*(framing.length===8?framing.filter(f=>f.ok).length/8:0),'Visible child text: anatomy, payment context, controls and part callouts are readable',{elements:presentation,framing});
  let live={ok:false},replay={ok:false},corroboration={ok:false},semantics={ok:false};
  const target=candidates[3];
  try {
  if(cases.length===8&&target) {
    await page.evaluate(pageVs7,{want:[],setCamera:[35,25,6]});
    await sleep(100);
    const box=await page.evaluate(pageCanvasRect),pose=inspectorPose(target,box.w,box.h),points=inspectorGrid(target,pose);
    const before=await page.request.get(baseUrl+'/api/payments/'+encodeURIComponent(target.id)).then(r=>r.json());
    await page.evaluate(pageScrollCanvasIntoView);
    await page.evaluate(pageArmSb71Capture,{id:target.id,points,source:'live'});
    const note='SB7.1 payment update '+Date.now();
    const response=await page.request.post(baseUrl+'/api/payments/'+encodeURIComponent(target.id)+'/note',{data:{note}});
    const liveVisible=await captureVisibleMotion(page,points);
    await page.waitForFunction(()=>window.__p7?.sb71Capture?.frames?.length>=11,null,{timeout:10000}).catch(()=>{});
    const captured=await page.evaluate(()=>window.__p7.sb71Capture);
    const after=await page.request.get(baseUrl+'/api/payments/'+encodeURIComponent(target.id)).then(r=>r.json());
    const wire=captured?.event?.record;
    corroboration={ok:response.ok()&&before.id===target.id&&after.id===target.id&&after.note===note&&after.version>before.version&&wire?.id===target.id&&wire.version===after.version&&wire.status===after.status,
      requestStatus:response.status(),before:{id:before.id,version:before.version},after:{id:after.id,version:after.version,status:after.status,note:after.note},wire};
    target.status=after.status;
    live=animationEvidence(target,pose,captured);
    live.visible=visibleMotionEvidence(target,pose,liveVisible);live.visible.captureDurations=liveVisible.frames.map(f=>f.captureDuration);
    live.ok=live.ok&&live.visible.ok;
    await H.saveShot('sb71-live-update');
    if(await page.locator('#replay-event').isEnabled().catch(()=>false)) {
      await page.evaluate(pageArmSb71Capture,{id:target.id,points,source:'replay'});
      const replayWrites=[],onReplayRequest=request=>{if(['POST','PUT','PATCH','DELETE'].includes(request.method()))replayWrites.push({method:request.method(),url:request.url()});};
      page.on('request',onReplayRequest);
      await page.locator('#replay-event').click();
      const replayVisible=await captureVisibleMotion(page,points);
      await page.waitForFunction(()=>window.__p7?.sb71Capture?.frames?.length>=11,null,{timeout:5000}).catch(()=>{});
      replay=animationEvidence(target,pose,await page.evaluate(()=>window.__p7.sb71Capture));
      replay.visible=visibleMotionEvidence(target,pose,replayVisible);replay.visible.captureDurations=replayVisible.frames.map(f=>f.captureDuration);replay.ok=replay.ok&&replay.visible.ok;
      const replayAfter=await page.request.get(baseUrl+'/api/payments/'+encodeURIComponent(target.id)).then(r=>r.json());
      page.off('request',onReplayRequest);replay.writes=replayWrites;
      replay.noWrite=replayWrites.length===0&&JSON.stringify(replayAfter)===JSON.stringify(after);replay.ok=replay.ok&&replay.noWrite;
    }
  }
  if(target&&replay.noWrite) {
    const box=await page.evaluate(pageCanvasRect),pose=inspectorPose(target,box.w,box.h),points=inspectorGrid(target,pose);
    await page.evaluate(pageArmSb71Capture,{id:target.id,points,source:'live'});
    await page.request.post(baseUrl+'/api/payments/'+encodeURIComponent(target.id)+'/note',{data:{note:'SB7.1 restart first '+Date.now()}});
    await page.waitForFunction(()=>window.__p7?.sb71Capture?.event,null,{timeout:8000});
    const previous=await page.evaluate(()=>window.__p7.sb71Capture.event);
    await sleep(300);
    await page.evaluate(pageArmSb71Capture,{id:target.id,points,source:'live'});
    await page.request.post(baseUrl+'/api/payments/'+encodeURIComponent(target.id)+'/note',{data:{note:'SB7.1 restart second '+Date.now()}});
    await page.waitForFunction(()=>window.__p7?.sb71Capture?.frames?.length>=11,null,{timeout:8000});
    const restarted=await page.evaluate(()=>window.__p7.sb71Capture),restart=animationEvidence(target,pose,restarted);
    const backend=await page.request.get(baseUrl+'/api/payments/'+encodeURIComponent(target.id)).then(r=>r.json());
    restart.restartAfterMs=restarted.event.t0-previous.t0;restart.previousVersion=previous.record.version;
    restart.corroborated=backend.version===restarted.event.record.version&&backend.version>previous.record.version&&restart.restartAfterMs>0&&restart.restartAfterMs<1000;
    const suppression=[];
    for(const event of [restarted.event,previous]) {
      const before=await page.evaluate(pageGlCounters);
      const delivered=await page.evaluate(event=>{
        const sources=window.__p7.sb71Sources.filter(es=>new URL(es.url,location.href).pathname==='/api/stream');
        for(const source of sources)source.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({batch:event.batch,records:[event.record]})}));return sources.length>0;
      },event);
      await sleep(350);
      const after=await page.evaluate(pageGlCounters),pixels=await page.evaluate(pageSamplePixels,{points}),groups=geometryEvidence(target,pose,pixels.samples||[]);
      const details=await page.locator('#inspect-version').textContent();
      const unchanged=Object.values(groups).every(g=>g.total>=3&&g.matched/g.total>=.97);
      suppression.push({version:event.record.version,delivered,before,after,unchanged,visibleVersion:details,
        ok:delivered&&unchanged&&details===String(backend.version)});
    }
    await page.locator('#replay-event').click();await sleep(150);await page.locator('#field-view').click();await sleep(250);
    const idleBefore=await page.evaluate(pageGlCounters);await sleep(350);const idleAfter=await page.evaluate(pageGlCounters);
    const restored=await page.evaluate(()=>({camera:window.vs7dbg.camera(),annotationsHidden:document.getElementById('tower-annotations').hidden}));
    const c=restored.camera,exit={before:idleBefore,after:idleAfter,...restored,ok:idleBefore.defDraws===idleAfter.defDraws&&restored.annotationsHidden&&Math.abs(c.yaw-V7.yaw0)<.01&&Math.abs(c.pitch-V7.pitch0)<.01&&Math.abs(c.distance-V7.dist0)<.01};
    semantics={restart,suppression,exit,ok:restart.ok&&restart.corroborated&&suppression.every(s=>s.ok)&&exit.ok};
    await page.locator('#inspect-open').click();await page.evaluate(pageScrollCanvasIntoView);
  }
  } catch(error) {semantics={...semantics,ok:false,evidenceError:String(error)};}
  add('m_committed_event_replay','M',(Number(corroboration.ok&&live.ok)+Number(corroboration.ok&&replay.ok)+Number(semantics.ok))/3,'Actual vendor-backed note update triggers collar motion; replay reproduces it without camera movement',{corroboration,live,replay,semantics});
  await H.saveShot('sb71-final-inspector');
  merge({sb71:{checks}});
}

async function vizScenario(page, pack, H) {
  const { saveShot, consoleErrors, safeGoto } = H;
  const model = buildModel(pack);
  let streamApplied = 0;                                 // stream entries already folded in
  let modelDesync = false;
  const syncModelWithStream = async () => {
    const log = await page.evaluate(pageStreamLog).catch(() => ({ entries: [] }));
    const entries = log.entries || [];
    for (let i = streamApplied; i < entries.length; i++) {
      const e = entries[i];
      if (e.size != null && e.records && e.records.length === e.size) applyBatchToModel(model, e.records);
      else if (e.size != null && e.size > 0) modelDesync = true;   // oversized batch: records capped
    }
    streamApplied = entries.length;
    return log;
  };
  const debugSurfaceObservations = [];
  const vs7 = async (arg) => {
    try {
      const result = await page.evaluate(pageVs7, arg || {});
      debugSurfaceObservations.push({present: result.present, evaluationSucceeded: true});
      merge({debugSurfaceObservations});
      return result;
    } catch (error) {
      debugSurfaceObservations.push({evaluationSucceeded: false, error: String(error)});
      merge({debugSurfaceObservations});
      return {evaluationError: String(error)};
    }
  };
  const setCam = async (yaw, pitch, distance) => {
    await page.evaluate(pageVs7, { want: [], setCamera: [yaw, pitch, distance] }).catch(() => {});
    await sleep(250);                                    // re-render + label re-cull settle
  };

  const navigationError = await safeGoto(25000);
  if (navigationError) {
    emit({ navigationError, consoleErrors: consoleErrors() });
    return;
  }

  // readiness: canvas + first draws + the vs7dbg surface (health-poll apps defeat networkidle)
  let ready = { canvas: false, draws: 0, vs7dbg: false };
  const readyDeadline = Date.now() + 40000;
  while (Date.now() < readyDeadline) {
    const g = await page.evaluate(pageGlCounters).catch(() => null);
    const d = await vs7({ want: ['layout'] });
    ready = { canvas: !!(g && g.contexts.some((c) => !c.offscreen)),
              draws: g ? g.defDraws : 0, vs7dbg: !!(d && d.present) };
    if (ready.canvas && ready.draws > 0 && ready.vs7dbg) break;
    await sleep(300);
  }
  await sleep(500);
  merge({ ready });

  // contextReal (roster: #viz3d): context kind + attrs from the wrapper, backing store, and a
  // blind pixel grid that must show non-background structure — rendered means seen.
  const glc0 = await page.evaluate(pageGlCounters).catch(() => null);
  const gridPts = [];
  for (let ix = 0; ix < 6; ix++) for (let iy = 0; iy < 4; iy++)
    gridPts.push({ cx: 0, cy: 0, ix, iy, kind: 'grid' });
  let contextReal = { canvasFound: false };
  const pre = await page.evaluate(pageSamplePixels, { points: [] }).catch(() => null);
  if (pre && pre.found && pre.glReadable) {
    const W = pre.rect.w, Hc = pre.rect.h;
    for (const p of gridPts) { p.cx = (p.ix + 0.5) * W / 6; p.cy = (p.iy + 0.5) * Hc / 4; }
    const grid = await page.evaluate(pageSamplePixels, { points: gridPts }).catch(() => null);
    const got = grid && grid.samples ? grid.samples.map((s) => s.got) : [];
    const isBg = (c) => c && c.every((v, i) => Math.abs(v - V7.bg[i]) <= V7.tol);
    const nonBg = got.filter((c) => !isBg(c));
    await syncModelWithStream();
    const coverageCtx=poseCtx(model,V7.yaw0,V7.pitch0,V7.dist0,W,Hc);
    const seededPoints=seededSurfacePoints(coverageCtx);
    const seededPixels=await page.evaluate(pageSamplePixels,{points:seededPoints});
    const seededMatched=(seededPixels.samples||[]).filter(s=>rgbNear(s.got,s.expected));
    const mainCtx = (glc0 ? glc0.contexts : []).find((c) => !c.offscreen && c.canvasId === 'viz3d')
      || (glc0 ? glc0.contexts : []).find((c) => !c.offscreen);
    const backingOk = Math.abs(pre.backing.w - Math.round(W * pre.dpr)) <= 1 &&
                      Math.abs(pre.backing.h - Math.round(Hc * pre.dpr)) <= 1;
    contextReal = {
      canvasFound: true, glReadable: true, dpr: pre.dpr, rect: { w: W, h: Hc },
      backing: pre.backing, backingOk,
      contextType: mainCtx ? mainCtx.type : null,
      askedAttrs: mainCtx ? mainCtx.askedAttrs : null,
      offscreenContexts: (glc0 ? glc0.contexts : []).filter((c) => c.offscreen).length,
      contextLost: glc0 ? glc0.contextLost : null,
      coarseGrid:{nonBg:nonBg.length,total:got.length,distinct:new Set(nonBg.map(c=>c.join(','))).size},
      coverageMethod:'Independently predicted seeded surface pixels; blind grid retained diagnostically',
      gridNonBg:seededMatched.length,gridTotal:seededPoints.length,gridDistinct:new Set(seededMatched.map(s=>s.got.join(','))).size,
      seededCoverage:{ids:[...new Set(seededMatched.map(s=>model.items[s.n].id))],samples:seededPixels.samples},
    };
  } else {
    contextReal = { canvasFound: !!(pre && pre.found), glReadable: !!(pre && pre.glReadable) };
  }
  merge({ contextReal });
  if (!contextReal.glReadable) {
    await saveShot('viz');
    emit({ consoleErrors: consoleErrors() });
    return;
  }
  const Wc = contextReal.rect.w, Hcs = contextReal.rect.h;

  // layout (§3.1): vs7dbg.layout() vs the pack-derived basis — d0, D0=96, R0 locked at load.
  const dLayout = await vs7({ want: ['layout', 'frames'] });
  const gotLayout = dLayout.layout && !dLayout.layout.__err ? dLayout.layout : null;
  merge({ layout: {
    present: !!gotLayout, got: gotLayout,
    expect: { d0: model.d0, D0: V7.D0, R0: model.R0 },
    ok: !!gotLayout && gotLayout.d0 === model.d0 &&
        gotLayout.D0 === V7.D0 && gotLayout.R0 === model.R0,
  } });

  // digest (§3.1): app-reported sums vs float64 recomputation (stream-synced first).
  await syncModelWithStream();
  const dDigest = await vs7({ want: ['digest', 'brush'] });
  const gotDigest = dDigest.digest && !dDigest.digest.__err ? dDigest.digest : null;
  const expDigest = expectDigest(model);
  merge({ digest: {
    present: !!gotDigest, got: gotDigest, expect: expDigest,
    ok: !!gotDigest && digestTolOk(gotDigest, expDigest) && !modelDesync,
    maxDelta: gotDigest ? digestMaxDelta(gotDigest, expDigest) : null,
    modelDesync,
    brushInitial: Array.isArray(dDigest.brush) ? dDigest.brush.length : null,
  } });

  // Early D1 brush: keep the schedule's mutation target brushed for the whole session so the
  // driver's fire_d1_mutation always lands on a brushed record (§3.6 D1). Mirrored in-model
  // so every later pixel expectation is brush-aware.
  let rect = null;
  await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
  await sleep(200);
  rect = await page.evaluate(pageCanvasRect).catch(() => null);
  const inViewport = !!rect && rect.top >= -0.5 && rect.left >= -0.5 &&
    rect.bottom <= rect.viewportH + 0.5 && rect.right <= rect.viewportW + 0.5;
  merge({ rectAfterScroll: rect ? { left: rect.left, top: rect.top, w: rect.w, h: rect.h,
                                    inViewport } : null });
  const d1TargetId = pack.stream && Array.isArray(pack.stream.mutateIds) && pack.stream.mutateIds.length
    ? pack.stream.mutateIds[0] : null;
  let d1 = { targetId: d1TargetId, brushed: false, via: null };
  if (d1TargetId != null && model.byId.has(d1TargetId)) {
    // Harness fix: an arbitrary seeded target is frequently occluded at the frozen
    // default pose, and the table fallback can only reach rendered (page-1) rows — so
    // search seeded poses biased toward the target's azimuth for one where the target
    // is decisively front, click THROUGH the app's own pick path there, and restore
    // the frozen defaults afterwards (leaving a custom pose poisoned cameraMath).
    const n = model.byId.get(d1TargetId);
    const it0 = model.items[n];
    const rngD1 = seedRng(pack.seed || 'sb7', 'd1arm');
    const azimuth = Math.atan2(it0.x, it0.z) * 180 / Math.PI;
    const radiusD1 = Math.hypot(it0.x, it0.z);
    const posesD1 = [[V7.yaw0, V7.pitch0, V7.dist0]];
    for (let k = 0; k < 36; k++) {
      const pD1 = 16 + rngD1() * 32;
      const dD1 = (k % 2 === 0)
        ? clamp(36 + rngD1() * 130, 16, 340)
        : clamp((radiusD1 + 25 + rngD1() * 55) / Math.cos(deg(pD1)), 16, 340);
      posesD1.push([azimuth + (rngD1() - 0.5) * 40, pD1, dD1]);
    }
    const looseFor = (ctxA) => {
      const it2 = model.items[n];
      const cands = [[it2.x, it2.h, it2.z], [it2.x, it2.h * 0.5, it2.z],
                     [it2.x - 0.3, it2.h, it2.z], [it2.x + 0.3, it2.h, it2.z],
                     [it2.x, it2.h, it2.z + 0.3], [it2.x, it2.h, it2.z - 0.3],
                     [it2.x, it2.h * 0.75, it2.z], [it2.x - 0.3, it2.h * 0.5, it2.z],
                     [it2.x + 0.3, it2.h * 0.5, it2.z]];
      for (const w of cands) {
        const p2 = projectPt(ctxA.eye, ctxA.basis, Wc, Hcs, w);
        if (!p2 || p2.x < 8 || p2.x > Wc - 8 || p2.y < 8 || p2.y > Hcs - 8) continue;
        let ok = true;
        for (const [ox, oy] of [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]]) {
          const h2 = castPixel(ctxA, p2.x + ox, p2.y + oy);
          if (!h2.length || h2[0].n !== n) { ok = false; break; }
        }
        if (ok) return { sx: +p2.x.toFixed(2), sy: +p2.y.toFixed(2) };
      }
      return null;
    };
    const attemptsArm = [];
    for (const [py, pp, pd] of posesD1) {
      const ctxA = poseCtx(model, py, pp, pd, Wc, Hcs);
      const pt = findDecisivePointFor(ctxA, model, n);
      if (pt) { attemptsArm.push({ pose: [py, pp, pd], pt, kind: 'decisive' }); break; }
    }
    if (!attemptsArm.length) {
      // No fully decisive point at any pose — arm through loose target points with a
      // click-verify-undo loop: the app's own pick decides; a mistoggle is reverted by
      // clicking the same pixel again (same front, same toggle).
      for (const [py, pp, pd] of posesD1) {
        const ctxA = poseCtx(model, py, pp, pd, Wc, Hcs);
        const lp = looseFor(ctxA);
        if (lp) attemptsArm.push({ pose: [py, pp, pd], pt: lp, kind: 'loose' });
        if (attemptsArm.length >= 4) break;
      }
    }
    if (inViewport) {
      for (const att of attemptsArm) {
        const custom = att.pose[0] !== V7.yaw0 || att.pose[1] !== V7.pitch0 ||
          att.pose[2] !== V7.dist0;
        if (custom) await setCam(att.pose[0], att.pose[1], att.pose[2]);
        await page.evaluate(pageScrollCanvasIntoView);
        rect=await page.evaluate(pageCanvasRect);
        await page.mouse.click(rect.left + att.pt.sx, rect.top + att.pt.sy);
        await sleep(400);
        const bA = await vs7({ want: ['brush'] });
        const arr = Array.isArray(bA.brush) ? bA.brush : [];
        if (arr.includes(d1TargetId)) {
          d1.via = (custom ? '3d-click-posed' : '3d-click') + ':' + att.kind;
          break;
        }
        if (arr.length) {                                 // undo the mistoggle
          await page.evaluate(pageScrollCanvasIntoView);
        rect=await page.evaluate(pageCanvasRect);
        await page.mouse.click(rect.left + att.pt.sx, rect.top + att.pt.sy);
          await sleep(250);
        }
      }
      await setCam(V7.yaw0, V7.pitch0, V7.dist0);
    }
    if (!d1.via) {
      const row = await page.evaluate(pageTableRowByld, { id: d1TargetId, click: true })
        .catch(() => ({ found: false }));
      d1.via = row.found ? 'table-click' : 'unreachable';
      await sleep(400);
    }
    const b = await vs7({ want: ['brush'] });
    d1.brushed = Array.isArray(b.brush) && b.brush.includes(d1TargetId);
    if (d1.brushed) model.brush.add(d1TargetId);
  }
  merge({ d1Arm: d1 });
  await page.evaluate(pageScrollCanvasIntoView);
  rect=await page.evaluate(pageCanvasRect);

  // cameraMath (§3.4): defaults, wheel law with the distance clamp, wheel-consumed guard.
  {
    const c0 = await vs7({ want: ['camera'] });
    const cam0 = c0.camera && !c0.camera.__err ? c0.camera : null;
    const cameraMath = {
      defaults: cam0 ? {
        got: cam0,
        yawErrDeg: cam0.yaw != null ? +angDist(cam0.yaw, V7.yaw0).toFixed(3) : null,
        pitchErr: cam0.pitch != null ? +Math.abs(cam0.pitch - V7.pitch0).toFixed(3) : null,
        distErr: cam0.distance != null ? +Math.abs(cam0.distance - V7.dist0).toFixed(3) : null,
      } : { got: null },
    };
    if (inViewport) {
      const cx = rect.left + rect.w / 2, cy = rect.top + rect.h / 2;
      await page.mouse.move(cx, cy);
      await page.mouse.wheel(0, 400);
      await sleep(300);
      const r1 = await page.evaluate(pageCanvasRect).catch(() => null);
      const c1 = await vs7({ want: ['camera'] });
      const cam1 = c1.camera && !c1.camera.__err ? c1.camera : null;
      const want1 = clamp((cam0 ? cam0.distance : V7.dist0) * Math.exp(V7.wheelK * 400), V7.distMin, V7.distMax);
      await page.mouse.wheel(0, -400);
      await sleep(300);
      const c2 = await vs7({ want: ['camera'] });
      const cam2 = c2.camera && !c2.camera.__err ? c2.camera : null;
      const want2 = clamp(want1 * Math.exp(V7.wheelK * -400), V7.distMin, V7.distMax);
      cameraMath.wheel = {
        pageScrolled: !!(r1 && rect && Math.abs(r1.top - rect.top) > 1),
        step1: { expected: +want1.toFixed(3), got: cam1 ? cam1.distance : null,
                 clampHit: want1 === V7.distMax },
        step2: { expected: +want2.toFixed(3), got: cam2 ? cam2.distance : null },
      };
      // restore the default distance for everything downstream
      await setCam(cam0 && cam0.yaw != null ? cam0.yaw : V7.yaw0,
                   cam0 && cam0.pitch != null ? cam0.pitch : V7.pitch0, V7.dist0);
      // pitch clamp: a straight-down drag pushes pitch far past 85 (0.30°/px · ~192 px from
      // pitch 40 ⇒ unclamped ≈ 97°); a slow release (< 6 px/s) keeps a coast out of it.
      const pcx = rect.left + rect.w / 2;
      let pcy = rect.top + Math.max(20, rect.h * 0.15);
      await page.mouse.move(pcx, pcy);
      await page.mouse.down();
      for (let i = 0; i < 12; i++) {
        pcy += 16;
        await page.mouse.move(pcx, pcy, { steps: 1 });
        await sleep(20);
      }
      await sleep(250); pcy += 1; await page.mouse.move(pcx, pcy, { steps: 1 });
      await sleep(250); pcy += 1; await page.mouse.move(pcx, pcy, { steps: 1 });
      await page.mouse.up();
      await sleep(300);
      const cP = await vs7({ want: ['camera'] });
      const camP = cP.camera && !cP.camera.__err ? cP.camera : null;
      cameraMath.pitchClamp = {
        attempted: true,
        gotPitch: camP && camP.pitch != null ? +camP.pitch.toFixed(3) : null,
        vpitchAfter: camP ? +(+camP.vpitch || 0).toFixed(2) : null,
      };
      await setCam(V7.yaw0, V7.pitch0, V7.dist0);
    } else cameraMath.wheel = { skipped: 'canvas not fully in viewport' };
    merge({ cameraMath });
  }

  // dragBudget (§3.2 R5): M=40 moves, pinned window [first move, pointerup], slow release
  // < 6 px/s so no coast starts inside the window; counters at pointerup + one rAF.
  let dragBudget = { performed: false };
  if (inViewport) {
    await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
    await sleep(150);
    rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
    const cx = rect.left + rect.w / 2 - 80, cy = rect.top + rect.h / 2;
    const camBefore = (await vs7({ want: ['camera'] })).camera || null;
    await page.evaluate(pageArmBudgetWatch).catch(() => {});
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    let dx = 0;
    for (let step = 1; step <= 38; step++) {
      dx += 4;
      await page.mouse.move(cx + dx, cy, { steps: 1 });
      await sleep(20);
    }
    await sleep(250);
    dx += 1;
    await page.mouse.move(cx + dx, cy, { steps: 1 });     // ≥30 ms gaps, ~4 px/s tail
    await sleep(250);
    dx += 1;
    await page.mouse.move(cx + dx, cy, { steps: 1 });
    await page.mouse.up();
    await sleep(300);
    const w = await page.evaluate(pageReadBudgetWatch).catch(() => null);
    const camAfter = (await vs7({ want: ['camera'] })).camera || null;
    const expYaw = camBefore && camBefore.yaw != null ? camBefore.yaw - V7.dragDegPerPx * dx : null;
    dragBudget = {
      performed: true, moves: 40, totalPx: dx,
      watch: w,
      deltaDefaultDrawsAtUp: w && w.c0 && w.cUp ? w.cUp.defDraws - w.c0.defDraws : null,
      deltaDefaultDrawsAtRaf: w && w.c0 && w.c1 ? w.c1.defDraws - w.c0.defDraws : null,
      deltaFrames: w && w.c0 && w.c1 && typeof w.c0.frames === 'number' && typeof w.c1.frames === 'number'
        ? w.c1.frames - w.c0.frames : null,
      movesSeenByPage: w ? w.moves : null,
      cameraAfter: camAfter, expectedYaw: expYaw != null ? +expYaw.toFixed(2) : null,
      yawErrDeg: camAfter && camAfter.yaw != null && expYaw != null
        ? +angDist(camAfter.yaw, expYaw).toFixed(2) : null,
      velocityAfter: camAfter ? { vyaw: camAfter.vyaw, vpitch: camAfter.vpitch } : null,
    };
    // dblclick: reset to defaults AND zero all velocity
    await page.mouse.dblclick(rect.left + rect.w / 2, rect.top + rect.h / 2);
    await sleep(350);
    const camReset = (await vs7({ want: ['camera'] })).camera || null;
    dragBudget.dblclickReset = camReset ? {
      got: camReset,
      yawErrDeg: camReset.yaw != null ? +angDist(camReset.yaw, V7.yaw0).toFixed(3) : null,
      pitchErr: camReset.pitch != null ? +Math.abs(camReset.pitch - V7.pitch0).toFixed(3) : null,
      distErr: camReset.distance != null ? +Math.abs(camReset.distance - V7.dist0).toFixed(3) : null,
      velocityZero: Math.abs(camReset.vyaw || 0) < 1e-6 && Math.abs(camReset.vpitch || 0) < 1e-6,
    } : { got: null };
  }
  merge({ dragBudget });
  const cInval = await page.evaluate(pageGlCounters).catch(() => null);  // post-reset invalidation

  // idle (§3.2): at rest, 0 default-FBO draws over any 500 ms window — two windows, each
  // voided if a stream batch landed inside it (a pending batch legally draws).
  {
    await sleep(700);
    const windows = [];
    for (let k = 0; k < 2; k++) {
      const s0 = await page.evaluate(pageGlCounters).catch(() => null);
      const l0 = await page.evaluate(pageStreamLog).catch(() => ({ entries: [] }));
      await sleep(500);
      const s1 = await page.evaluate(pageGlCounters).catch(() => null);
      const l1 = await page.evaluate(pageStreamLog).catch(() => ({ entries: [] }));
      windows.push({
        ms: 500,
        defaultDraws: s0 && s1 ? s1.defDraws - s0.defDraws : null,
        rafTicks: s0 && s1 ? s1.rafTicks - s0.rafTicks : null,
        batchLanded: (l1.entries || []).length !== (l0.entries || []).length,
      });
      await sleep(200);
    }
    merge({ idle: { windows } });
  }

  // picks (§3.3): the four occlusion constructions + plain fronts, all decisive; pick() ==
  // decode(pickPixel()) == analytic front; real-pass counters around the first pick after the
  // dblclick invalidation. Harness fix: at the frozen default distance a 0.9-unit column
  // subtends ~2 device px — below decisiveAt's own 3×3+ring-3 unanimity bar — so the
  // constructions are searched at seeded CLOSE poses (the §3.1 pixel contract is explicitly
  // graded "at a close-up pose") and the pose is applied through the app's own setCamera.
  {
    await syncModelWithStream();
    let cam = { yaw: V7.yaw0, pitch: V7.pitch0, distance: V7.dist0 };
    let ctx = poseCtx(model, cam.yaw, cam.pitch, cam.distance, Wc, Hcs);
    let t = findPickTargets(ctx, model, pack.seed || 'sb7');
    const enough = (f) => f.occludedLowerN && f.occludedHigherN && f.partial &&
      f.background && f.fronts.length >= 2;
    if (!enough(t)) {
      const rngP = seedRng(pack.seed || 'sb7', 'pickpose');
      const kinds = (f) => [f.occludedLowerN, f.occludedHigherN, f.partial, f.background]
        .filter(Boolean).length + Math.min(f.fronts.length, 2);
      for (let k = 0; k < 140 && !enough(t); k++) {
        const pose = { yaw: rngP() * 360, pitch: 16 + rngP() * 50,
                       distance: 26 + rngP() * 90 };
        const ctx2 = poseCtx(model, pose.yaw, pose.pitch, pose.distance, Wc, Hcs);
        const t2 = findPickTargets(ctx2, model, (pack.seed || 'sb7') + ':pk' + k);
        if (kinds(t2) > kinds(t)) { t = t2; ctx = ctx2; cam = pose; }
      }
    }
    if (cam.yaw !== V7.yaw0 || cam.pitch !== V7.pitch0 || cam.distance !== V7.dist0) {
      await setCam(cam.yaw, cam.pitch, cam.distance);
    }
    const targets = [t.occludedLowerN, t.occludedHigherN, t.partial, t.background, ...t.fronts]
      .filter(Boolean);
    const cBefore = await page.evaluate(pageGlCounters).catch(() => null);
    const results = [];
    let cAfterFirst = null;
    for (let i = 0; i < targets.length; i++) {
      const tg = targets[i];
      const r = await vs7({ want: [], picks: [[tg.sx, tg.sy]], pickPixels: [[tg.sx, tg.sy]] });
      if (i === 0) cAfterFirst = await page.evaluate(pageGlCounters).catch(() => null);
      const pick = r.picks ? r.picks[0] : undefined;
      const px = r.pickPixels ? r.pickPixels[0] : undefined;
      const decodedNum = Array.isArray(px) ? decodePickPixel(px) : null;
      const decodedN = decodedNum == null ? null : decodedNum === 0 ? null : decodedNum - 1;
      const pickId = pick && typeof pick === 'object' && !pick.__err ? pick.id : pick === null ? null : undefined;
      results.push({
        ...tg,
        pick: pick && pick.__err ? { err: pick.__err } : pick,
        pickPixel: px,
        decodedN,
        analyticN: tg.frontN,
        pickAgrees: tg.frontN == null ? pickId === null
          : !!pick && typeof pick === 'object' && pick.id === tg.frontId,
        pixelAgrees: decodedN === tg.frontN,
      });
    }
    const cAfterAll = await page.evaluate(pageGlCounters).catch(() => null);
    const dOf = (a, b, k) => (a && b ? b[k] - a[k] : null);
    merge({ picks: {
      poseCamera: cam, found: {
        occludedLowerN: !!t.occludedLowerN, occludedHigherN: !!t.occludedHigherN,
        partial: !!t.partial, background: !!t.background, fronts: t.fronts.length,
      },
      results,
      agreeAll: results.length > 0 && results.every((r) => r.pickAgrees && r.pixelAgrees),
    } });
    merge({ pickCounters: {
      invalidationSnapshot: cInval, beforeFirstPick: cBefore,
      afterFirstPick: cAfterFirst, afterAllPicks: cAfterAll,
      sinceInvalidation: {
        offDraws: dOf(cInval, cAfterFirst, 'offDraws'),
        offReads: dOf(cInval, cAfterFirst, 'offReads'),
        defDraws: dOf(cInval, cAfterFirst, 'defDraws'),
      },
      duringPickCalls: {
        offDraws: dOf(cBefore, cAfterAll, 'offDraws'),
        offReads: dOf(cBefore, cAfterAll, 'offReads'),
        defDraws: dOf(cBefore, cAfterAll, 'defDraws'),
      },
      pickCalls: results.length,
    } });
    merge({ pickOccluded: {
      available: !!(t.occludedLowerN && t.occludedHigherN),
      lowerOk: !!t.occludedLowerN && results.find((r) => r.cls === 'occluded-by-lower-n')?.pickAgrees === true,
      higherOk: !!t.occludedHigherN && results.find((r) => r.cls === 'occluded-by-higher-n')?.pickAgrees === true,
      backgroundOk: !!t.background && results.find((r) => r.cls === 'background-in-hull')?.pickAgrees === true,
    } });
  }

  // labels (§3.5): exact shown-set graded ONLY at a decisive rest pose (R4) — bounded,
  // seed-deterministic pose search; app-side zero-overlap asserted at whatever pose renders.
  {
    await syncModelWithStream();
    const cands = labelCandidates(model, pack.labelCandidates);
    const rng = seedRng(pack.seed || 'sb7', 'labelpose');
    let pose = null, rows = null, tried = 0;
    // Harness fix: distances 200-340 leave every column ~2 px wide — anchor unanimity is
    // unreachable there; the pose search must include close-up poses at any azimuth.
    const poses = [[V7.yaw0, V7.pitch0, V7.dist0]];
    for (let k = 0; k < 90; k++)
      poses.push([rng() * 360, 22 + rng() * 45, 45 + rng() * 255]);
    for (const [py, pp, pd] of poses) {
      tried++;
      const ctx = poseCtx(model, py, pp, pd, Wc, Hcs);
      const r = expectLabels(ctx, cands);
      if (labelPoseDecisive(r) && r.filter((x) => x.shown).length >= 3) {
        pose = { yaw: +py.toFixed(2), pitch: +pp.toFixed(2), distance: +pd.toFixed(2) };
        rows = r;
        break;
      }
    }
    if (!pose) {
      merge({ labels: { decisivePoseFound: false, tried, candidates: cands.length } });
    } else {
      await setCam(pose.yaw, pose.pitch, pose.distance);
      await sleep(150);
      const dom = await page.evaluate(pageLabelsRead).catch(() => ({ labels: [] }));
      const domById = new Map((dom.labels || []).map((l) => [l.id, l]));
      const expShown = rows.filter((r) => r.shown);
      const domShownIds = (dom.labels || []).filter((l) => l.shown).map((l) => l.id);
      const perLabel = expShown.map((r) => {
        const d = domById.get(r.id);
        return {
          id: r.id, expectRect: r.rect, amount_minor: r.amount_minor, currency: r.cur,
          domShown: !!(d && d.shown),
          dx: d ? +(d.x - r.rect.x).toFixed(2) : null,
          dy: d ? +(d.y - r.rect.y).toFixed(2) : null,
          w: d ? d.w : null, h: d ? d.h : null,
          text: d ? d.text : null,
        };
      });
      const extraShown = domShownIds.filter((id) => !expShown.some((r) => r.id === id));
      merge({ labels: {
        decisivePoseFound: true, tried, pose,
        candidates: rows.map((r) => ({ id: r.id, eligible: r.eligible, shownExpected: r.shown })),
        expectedShownCount: expShown.length,
        domShownCount: domShownIds.length,
        setMatch: extraShown.length === 0 && perLabel.every((l) => l.domShown),
        extraShown, perLabel,
        wrapPresent: !!dom.wrap,
        overlapViolations: dom.overlapViolations != null ? dom.overlapViolations : null,
      } });
      merge({ labelSetExact: {
        graded: true,
        match: extraShown.length === 0 && perLabel.every((l) => l.domShown),
        expected: expShown.map((r) => r.id), got: domShownIds,
      } });
      await setCam(V7.yaw0, V7.pitch0, V7.dist0);
    }
  }

  // heightPixels (R9): 6 seeded instances (≥1 JPY, ≥1 KWD), device-pixel column scan at a
  // decisive close-up pose; measured top within ±3 px of the projected h; cross-checks digest.
  {
    await syncModelWithStream();
    let caseIds = Array.isArray(pack.heightCases) && pack.heightCases.length >= 6
      ? pack.heightCases.slice(0, 6).filter((id) => model.byId.has(id)) : null;
    if (!caseIds || caseIds.length < 6) {
      const rng = seedRng(pack.seed || 'sb7', 'heights');
      const order = model.items.map((_, i) => i);
      for (let i = order.length - 1; i > 0; i--) {
        const j = Math.floor(rng() * (i + 1));
        [order[i], order[j]] = [order[j], order[i]];
      }
      const picked = [];
      const one = (cur) => order.find((n) => model.items[n].cur === cur && !picked.includes(n));
      const j = one('JPY'), k = one('KWD');
      if (j != null) picked.push(j);
      if (k != null) picked.push(k);
      for (const n of order) {
        if (picked.length >= 6) break;
        if (!picked.includes(n)) picked.push(n);
      }
      caseIds = picked.map((n) => model.items[n].id);
    }
    const rng2 = seedRng(pack.seed || 'sb7', 'heightpose');
    const cases = [];
    for (const id of caseIds) {
      const n = model.byId.get(id);
      const it = model.items[n];
      let done = null;
      const azH = Math.atan2(it.x, it.z) * 180 / Math.PI;
      const radH = Math.hypot(it.x, it.z);
      let scanTries = 0, fallbackH = null;
      for (let attempt = 0; attempt < 600 && !done && scanTries < 8; attempt++) {
        // Harness fix: the orbit target is frozen at the field CENTER, so a close-up on
        // an off-center instance needs distance ≈ radius/cos(pitch) with the azimuth
        // aligned — fixed 75-150 distances left every column ~2 px, below the decisive
        // margins; the §3.1 height rung is graded "at a close-up pose" by contract.
        // Two placement families: eye-beyond-the-instance with a variable standoff, and
        // a radius-matched close orbit — alternating gives every field position a shot.
        const pitch = 12 + rng2() * rng2() * 48;   // low-pitch biased — where decisive top edges live
        const dist = (attempt % 4 < 2)
          ? clamp(28 + rng2() * 150, 16, 340)
          : clamp((radH + 20 + rng2() * 70) / Math.cos(deg(pitch)), 16, 340);
        const yaw = azH + (rng2() - 0.5) * (attempt % 2 === 1 ? 360 : 60);
        const ctx = poseCtx(model, yaw, pitch, dist, Wc, Hcs);
        // Anchor: ANY decisive point on the TOP face (center-only was unreachable for
        // occluded mid-field instances); the expected edge is cast-walked up the same
        // pixel column in the model, so the comparison is exact at any anchor.
        // A stable-top anchor is enough for a ±3 px measurement: our top face with a
        // 2 px identity margin and a laterally consistent edge (full click-decisiveness
        // with its NDC depth gap was unreachable for occluded short columns).
        const isOurTop = (sx2, sy2) => {
          const hh = castPixel(ctx, sx2, sy2);
          return hh.length && hh[0].n === n && Math.abs(hh[0].hitY - it.h) <= 1e-6;
        };
        const edgeAt = (sx2, sy0) => {
          let k3 = 0;
          while (k3 < 30 && isOurTop(sx2, sy0 - k3 - 1)) k3++;
          return sy0 - k3;
        };
        let anchor = null, edgeY = null;
        for (const [dxF, dzF] of [[0, 0], [0.3, 0], [-0.3, 0], [0, 0.3], [0, -0.3],
                                  [0.3, 0.3], [-0.3, 0.3], [0.3, -0.3], [-0.3, -0.3]]) {
          const pA = projectPt(ctx.eye, ctx.basis, Wc, Hcs,
                               [it.x + dxF, it.h, it.z + dzF]);
          if (!pA || pA.x < 12 || pA.x > Wc - 12 || pA.y < 16 || pA.y > Hcs - 16) continue;
          const ax = pA.x, ay = pA.y + 1;
          let solid = true;
          for (let dx2 = -2; dx2 <= 2 && solid; dx2++) {
            for (let dy2 = -1; dy2 <= 1 && solid; dy2++) {
              if (!isOurTop(ax + dx2, ay + dy2)) solid = false;
            }
          }
          if (!solid) continue;
          const e0 = edgeAt(ax, ay), eL = edgeAt(ax - 1, ay), eR = edgeAt(ax + 1, ay);
          if (Math.max(e0, eL, eR) - Math.min(e0, eL, eR) > 2) continue;
          anchor = { sx: ax, sy: ay };
          edgeY = e0;
          break;
        }
        if (!anchor || edgeY < 12) continue;
        let base = V7.status[it.status];
        if (model.brush.size > 0 && !model.brush.has(it.id)) base = dimColor(base);
        // reject poses where a same-colored face touches our edge from above
        const haE = castPixel(ctx, anchor.sx, edgeY - 2);
        if (haE.length) {
          if (haE[0].n === n) continue;
          const cAbove = surfColor(ctx, haE[0]);
          if (cAbove.every((v, i2) => Math.abs(v - base[i2]) <= V7.tol + 4)) continue;
        }
        scanTries++;
        await setCam(yaw, pitch, dist);
        const yTop = edgeY - 8, yBot = anchor.sy + 6;
        const scan = await page.evaluate(pageColumnScan,
          { columns: [{ id, sx: anchor.sx, yTop, yBot }] }).catch(() => null);
        const col = scan && scan.glReadable && scan.columns && scan.columns[0];
        if (!col) { done = { id, cur: it.cur, scanFailed: true }; break; }
        const scale = scan.backing.h / scan.rect.h;
        const matchesBase = (c) => c && c.every((v, i2) => Math.abs(v - base[i2]) <= V7.tol);
        const anchorMid = Math.min(col.rows.length - 2,
                                   Math.max(1, Math.round((anchor.sy - yTop) * scale)));
        let idx = -1;
        for (const aOff of [0, 1, -1]) {
          const a2 = anchorMid + aOff;
          if (a2 >= 0 && a2 < col.rows.length && matchesBase(col.rows[a2])) {
            idx = a2;
            while (idx > 0 && matchesBase(col.rows[idx - 1])) idx--;
            break;
          }
        }
        if (idx < 0) continue;                            // raster edge — try another pose
        const measuredCssY = yTop + idx / scale;
        const attemptResult = {
          id, cur: it.cur, amount_minor: it.amount_minor, h: +it.h.toFixed(4),
          pose: { yaw: +yaw.toFixed(2), pitch: +pitch.toFixed(2), distance: dist },
          projectedTopCssY: +edgeY.toFixed(2),
          measuredTopCssY: +measuredCssY.toFixed(2),
          deltaPx: +((measuredCssY - edgeY) * scale).toFixed(2),
          colorAtTop: col.rows[Math.max(idx, anchorMid - 1)],
          expectTopColor: base,
          colorOk: true,
          within3px: Math.abs((measuredCssY - edgeY) * scale) <= 3,
        };
        // An out-of-tolerance single measurement can be a raster corner case at one
        // pose — keep it as a fallback and let another pose confirm within budget.
        if (attemptResult.within3px) done = attemptResult;
        else fallbackH = fallbackH || attemptResult;
      }
      cases.push(done || fallbackH || { id, cur: it.cur, noDecisivePose: true });
    }
    merge({ heightPixels: {
      cases,
      jpyIncluded: cases.some((c) => c.cur === 'JPY'),
      kwdIncluded: cases.some((c) => c.cur === 'KWD'),
      okCount: cases.filter((c) => c.within3px && c.colorOk).length,
      total: cases.length,
    } });
    // explicit projection-error measurement: |rendered top − model-projected top| in device
    // px across the height cases — the worst is cameraMath's projMaxErrPx.
    const projDeltas = cases.filter((c2) => c2 && typeof c2.deltaPx === 'number')
      .map((c2) => Math.abs(c2.deltaPx));
    if (result.cameraMath) {
      result.cameraMath.projMaxErrPx = projDeltas.length
        ? +Math.max(...projDeltas).toFixed(2) : null;
      result.cameraMath.projSamples = projDeltas.length;
    }
    await setCam(V7.yaw0, V7.pitch0, V7.dist0);
  }

  // brush (§3.6): both doors, toggle semantics, dim-pixel truth (0.30 before the side factor),
  // #brush-count, row data-brushed + table navigation. Facts at every step.
  const expectPx = (it, factor) => {
    let base = V7.status[it.status];
    if (model.brush.size > 0 && !model.brush.has(it.id)) base = dimColor(base);
    return base.map(v=>Math.round(v*factor));
  };
  if (inViewport) {
    await syncModelWithStream();
    rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
    // Harness fix: decisive click targets are unreachable at the default distance
    // (~2 px columns); search seeded close poses for TWO decisively clickable targets
    // and run the whole brush exercise there through the app's own setCamera.
    const rng = seedRng(pack.seed || 'sb7', 'brush');
    const order = model.items.map((_, i) => i);
    for (let i = order.length - 1; i > 0; i--) {
      const j = Math.floor(rng() * (i + 1));
      [order[i], order[j]] = [order[j], order[i]];
    }
    const rngBp = seedRng(pack.seed || 'sb7', 'brushpose');
    const brushPoses = [[V7.yaw0, V7.pitch0, V7.dist0]];
    for (let k = 0; k < 30; k++)
      brushPoses.push([rngBp() * 360, 24 + rngBp() * 40, 36 + rngBp() * 110]);
    let T1 = null, T3 = null, brushPose = null;
    for (const [py, pp, pd] of brushPoses) {
      const ctxB = poseCtx(model, py, pp, pd, Wc, Hcs);
      let a = null, b2 = null;
      let scanned = 0;
      for (const n of order) {
        if (scanned++ > 3000 && !a) break;
        const it = model.items[n];
        if (it.id === d1TargetId || model.brush.has(it.id)) continue;
        const pt = findDecisivePointFor(ctxB, model, n);
        if (!pt) continue;
        if (!a) a = { n, id: it.id, pt };
        else if (!b2) { b2 = { n, id: it.id, pt }; break; }
      }
      if (a && b2) { T1 = a; T3 = b2; brushPose = [py, pp, pd]; break; }
    }
    if (brushPose && (brushPose[0] !== V7.yaw0 || brushPose[1] !== V7.pitch0 ||
                      brushPose[2] !== V7.dist0)) {
      await setCam(brushPose[0], brushPose[1], brushPose[2]);
    }
    const brushCtxPose = brushPose || [V7.yaw0, V7.pitch0, V7.dist0];
    const ctx = poseCtx(model, brushCtxPose[0], brushCtxPose[1], brushCtxPose[2],
                        Wc, Hcs);
    const countText = async () => (await page.evaluate(pageBrushCount).catch(() => ({}))).text || null;
    const brushNow = async () => {
      const b = await vs7({ want: ['brush'] });
      return Array.isArray(b.brush) ? b.brush : null;
    };
    const samplePt = async (T, top) => {
      const s = await page.evaluate(pageSamplePixels,
        { points: [{ cx: T.pt.sx, cy: T.pt.sy }] }).catch(() => null);
      const got = s && s.samples && s.samples[0] ? s.samples[0].got : null;
      const exp = expectPx(model.items[T.n], T.pt.factor);
      return { got, expect: exp, ok: !!got && got.every((v, i) => Math.abs(v - exp[i]) <= V7.tol) };
    };
    const brush = { available: !!(T1 && T3), counts: [] };
    if (T1 && T3) {
      brush.counts.push({ step: 'start', text: await countText() });
      // 3D door: click toggles T1 in
      await page.mouse.click(rect.left + T1.pt.sx, rect.top + T1.pt.sy);
      await sleep(450);
      let b = await brushNow();
      if (b && b.includes(T1.id)) model.brush.add(T1.id);
      // table navigation is an async journey (fetch + render + scroll) — poll for the
      // navigated row rather than reading a one-shot snapshot (harness fix: under
      // machine load a 450 ms one-shot raced the app's legitimate navigation)
      let row1 = { found: false };
      {
        const tNav = Date.now();
        while (Date.now() - tNav < 3000) {
          row1 = await page.evaluate(pageTableRowByld, { id: T1.id, click: false })
            .catch(() => ({ found: false }));
          if (row1.found && row1.inViewport) break;
          await sleep(300);
        }
      }
      brush.door3d = { targetId: T1.id, inBrush: !!(b && b.includes(T1.id)), brushAfter: b,
                       rowFound: row1.found, rowBrushed: row1.dataBrushed === 'true',
                       rowInViewport: !!row1.inViewport };
      brush.counts.push({ step: 'after-3d-click', text: await countText() });
      const memberPx = await samplePt(T1);
      const nonMemberPx = await samplePt(T3);
      merge({ brushHighlight: { memberPx, nonMemberPx,
                                brushSize: model.brush.size,
                                dimFactor: V7.dimF, graded: memberPx.got != null } });
      // table door: first visible row's payment id toggles in
      const t2id = await page.evaluate(() => {
        const r = document.querySelector('tbody tr[data-id], tbody tr[data-payment-id], [role="row"][data-id]');
        return r ? (r.getAttribute('data-id') || r.getAttribute('data-payment-id')) : null;
      }).catch(() => null);
      if (t2id && model.byId.has(t2id) && !model.brush.has(t2id)) {
        const rowClick = await page.evaluate(pageTableRowByld, { id: t2id, click: true })
          .catch(() => ({ found: false }));
        await sleep(450);
        b = await brushNow();
        if (b && b.includes(t2id)) model.brush.add(t2id);
        const row2 = await page.evaluate(pageTableRowByld, { id: t2id, click: false })
          .catch(() => ({ found: false }));
        brush.doorTable = { targetId: t2id, clicked: rowClick.found,
                            inBrush: !!(b && b.includes(t2id)),
                            rowBrushed: row2.dataBrushed === 'true' };
        brush.counts.push({ step: 'after-table-click', text: await countText() });
        // toggle T2 back out through the same door
        await page.evaluate(pageTableRowByld, { id: t2id, click: true }).catch(() => ({}));
        await sleep(400);
        b = await brushNow();
        if (b && !b.includes(t2id)) model.brush.delete(t2id);
        brush.doorTable.toggledOut = !!(b && !b.includes(t2id));
      } else brush.doorTable = { targetId: t2id, skipped: true };
      // toggle T1 back out through the 3D door — the table door's row scroll may have
      // moved the page, so re-center the canvas and re-read its rect first (harness
      // fix: clicking with the stale rect landed outside the canvas entirely)
      await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
      await sleep(150);
      rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
      await page.mouse.click(rect.left + T1.pt.sx, rect.top + T1.pt.sy);
      await sleep(450);
      b = await brushNow();
      if (b && !b.includes(T1.id)) model.brush.delete(T1.id);
      brush.toggleOff = { targetId: T1.id, removed: !!(b && !b.includes(T1.id)), brushAfter: b };
      brush.counts.push({ step: 'after-toggle-off', text: await countText() });
    }
    merge({ brush, brushCount: { present: (await page.evaluate(pageBrushCount)
      .catch(() => ({ present: false }))).present, steps: brush.counts } });
  } else merge({ brush: { available: false, reason: 'canvas not fully in viewport' } });

  // coast (§3.4): fast flick vs the closed-form law, pixel reality, cancel-by-pointerdown,
  // slow release, settle budget, and the R8 cadence fact — all from page-side stamps.
  if (inViewport) {
    await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
    await sleep(150);
    rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
    const cx = rect.left + rect.w / 2 - 100, cy = rect.top + rect.h / 2;
    await page.mouse.dblclick(rect.left + rect.w / 2, rect.top + rect.h / 2);   // reset + zero v
    await sleep(400);
    await syncModelWithStream();
    const ctx0 = poseCtx(model, V7.yaw0, V7.pitch0, V7.dist0, Wc, Hcs);
    // Harness fix: motion evidence needs stable non-background pixels, not full
    // decisiveness (unreachable at the default distance) — loose points suffice.
    const basePts = findLoosePoints(ctx0, model, (pack.seed || 'sb7') + ':coastpx', 5)
      .map((t) => ({ cx: t.sx, cy: t.sy }));
    const s0 = await page.evaluate(pageSamplePixels, { points: basePts }).catch(() => null);
    const got0 = s0 && s0.samples ? s0.samples.map((s) => s.got) : [];

    await page.evaluate(pageArmCoastWatch).catch(() => {});
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    for (let i = 1; i <= 8; i++) {                       // ≈800 px/s ⇒ v0 ≈ 240°/s
      await page.mouse.move(cx + i * 24, cy, { steps: 1 });
      await sleep(30);
    }
    await page.mouse.up();
    await sleep(200);
    const sMid = await page.evaluate(pageSamplePixels, { points: basePts }).catch(() => null);
    const gotMid = sMid && sMid.samples ? sMid.samples.map((s) => s.got) : [];
    const movedCount = got0.length ? gotMid.filter((g, i) =>
      g && got0[i] && g.some((v, ch) => Math.abs(v - got0[i][ch]) > V7.tol)).length : null;
    let w = null;
    {
      const deadline = Date.now() + 4800;
      while (Date.now() < deadline) {
        w = await page.evaluate(pageReadCoastWatch).catch(() => null);
        if (w && w.settled) break;
        await sleep(120);
      }
    }
    const camRest = (await vs7({ want: ['camera'] })).camera || null;
    let restPixel = null;
    if (camRest && camRest.yaw != null) {
      const ctxR = poseCtx(model, camRest.yaw, camRest.pitch, camRest.distance, Wc, Hcs);
      // Majority over several loose points — a single ±1 px-unanimous point on ~2 px
      // columns can straddle a rasterization edge without the app being wrong.
      const fls = findLoosePoints(ctxR, model, (pack.seed || 'sb7') + ':rest', 5);
      if (fls.length) {
        const sR = await page.evaluate(pageSamplePixels,
          { points: fls.map((f) => ({ cx: f.sx, cy: f.sy })) }).catch(() => null);
        const samples = (sR && sR.samples) || [];
        let okCount = 0;
        for (let i = 0; i < fls.length; i++) {
          const got = samples[i] ? samples[i].got : null;
          const it = model.items[fls[i].frontN];
          const hit=castPixel(ctxR,samples[i]?.rayX,samples[i]?.rayY)[0];
          const exp=hit?surfColor(ctxR,hit):null;
          if (got && exp && got.every((v, k2) => Math.abs(v - exp[k2]) <= V7.tol)) okCount++;
        }
        restPixel = { points: fls.length, okCount,
                      got: samples[0] ? samples[0].got : null,
                      ok: okCount * 2 >= fls.length };
      }
    }
    let flick = { samples: null };
    if (w && Array.isArray(w.samples) && w.samples.length >= 3) {
      const S = w.samples;
      const first = S[0];
      const rest = S[S.length - 1];
      const settleMs = w.settleMs != null ? w.settleMs : rest.t;
      const pick = (frac) => {
        const tw = settleMs * frac;
        let best = S[0];
        for (const s of S) if (Math.abs(s.t - tw) < Math.abs(best.t - tw)) best = s;
        return best;
      };
      const mids = [pick(0.25), pick(0.55)].filter((s) => s.t > 5 && s.t < settleMs - 5);
      const residuals = mids.map((s) => ({
        tMs: s.t, yaw: +s.yaw.toFixed(3), vyaw: +(+s.vyaw || 0).toFixed(3),
        residualDeg: +Math.abs(rest.yaw - (s.yaw + (s.vyaw || 0) * V7.tau)).toFixed(3),
        tolDeg: +Math.max(1.0, 0.15 * Math.abs((s.vyaw || 0) * V7.tau)).toFixed(3),
      }));
      const v0 = Math.abs(first.vyaw || 0);
      flick = {
        v0Reported: +v0.toFixed(2), releaseYaw: +first.yaw.toFixed(3),
        restYaw: +rest.yaw.toFixed(3),
        coastDeg: +Math.abs(rest.yaw - first.yaw).toFixed(3),
        directionOk: rest.yaw < first.yaw + 0.01,        // +x drag ⇒ yaw decreases
        residuals,
        identityOk: residuals.length > 0 && residuals.every((r) => r.residualDeg <= r.tolDeg),
        settleMs: w.settleMs,
        settleBudgetMs: +Math.min(2.5, V7.tau * Math.log(Math.max(v0, 2) / 2) + 0.7).toFixed(3) * 1000,
        movedPixelCount: movedCount, movedPixelTotal: got0.length,
        restPixel,
        samples: (w.samples || []).filter((_, i) => i % 3 === 0).slice(0, 40),
      };
    } else flick = { samples: null, watchMissing: !w, movedPixelCount: movedCount, restPixel };

    // cancel-by-pointerdown (wheel must NOT cancel; pointerdown must)
    await page.mouse.dblclick(rect.left + rect.w / 2, rect.top + rect.h / 2);
    await sleep(350);
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    for (let i = 1; i <= 5; i++) { await page.mouse.move(cx + i * 24, cy, { steps: 1 }); await sleep(30); }
    await page.mouse.up();
    await sleep(150);
    const camCoasting = (await vs7({ want: ['camera'] })).camera || null;
    await page.mouse.down();
    await sleep(100);
    const camHeld = (await vs7({ want: ['camera'] })).camera || null;
    await page.mouse.move(cx + 140, cy, { steps: 2 });    // >5 px so the release is not a click
    await page.mouse.up();
    await sleep(300);
    const cancel = {
      vyawMidCoast: camCoasting ? +(+camCoasting.vyaw || 0).toFixed(2) : null,
      vyawWhileHeld: camHeld ? +(+camHeld.vyaw || 0).toFixed(2) : null,
      canceled: !!camHeld && Math.abs(camHeld.vyaw || 0) < 2,
    };

    // slow release (< 6 px/s ⇒ v0 = 1.8°/s, below the 2°/s stop threshold)
    await page.mouse.dblclick(rect.left + rect.w / 2, rect.top + rect.h / 2);
    await sleep(400);
    await page.evaluate(pageArmCoastWatch).catch(() => {});
    await page.mouse.move(cx, cy);
    await page.mouse.down();
    let sx2 = 0;
    for (let i = 1; i <= 6; i++) { sx2 += 8; await page.mouse.move(cx + sx2, cy, { steps: 1 }); await sleep(25); }
    await sleep(260); sx2 += 1; await page.mouse.move(cx + sx2, cy, { steps: 1 });
    await sleep(260); sx2 += 1; await page.mouse.move(cx + sx2, cy, { steps: 1 });
    await page.mouse.up();
    await sleep(900);
    const wSlow = await page.evaluate(pageReadCoastWatch).catch(() => null);
    let slowRelease = { watched: false };
    if (wSlow && Array.isArray(wSlow.samples) && wSlow.samples.length) {
      const first = wSlow.samples[0], last = wSlow.samples[wSlow.samples.length - 1];
      slowRelease = { watched: true, v0Reported: +Math.abs(first.vyaw || 0).toFixed(2),
                      driftDeg: +Math.abs(last.yaw - first.yaw).toFixed(3),
                      ok: Math.abs(last.yaw - first.yaw) <= 0.5 };
    }

    // R8 cadence fact: default-FBO frame-group cadence during the flick coast window
    let cadence = null;
    if (w && w.tUp != null) {
      const ts = await page.evaluate(pageDrawTs, { tail: 4000 }).catch(() => []);
      const t1 = w.tUp + (w.settleMs != null ? w.settleMs : 1500);
      const starts = frameGroups(ts.filter((t) => t >= w.tUp && t <= t1));
      const gaps = starts.slice(1).map((t, i) => t - starts[i]);
      cadence = { frameCount: starts.length, medianFrameMs: gaps.length ? +median(gaps).toFixed(2) : null,
                  windowMs: +(t1 - w.tUp).toFixed(0) };
    }
    merge({ coast: { flick, cancel, slowRelease, cadence } });
    await page.mouse.dblclick(rect.left + rect.w / 2, rect.top + rect.h / 2);
    await sleep(350);
  } else merge({ coast: { skipped: 'canvas not fully in viewport' } });

  // stream (§3.7): every observed SSE batch replayed against a fresh model — digest deltas,
  // per-batch upload-byte accounting from the wrapper, apply latency, changed-instance pixel,
  // and the D1 brushed-mutation observation. The driver pokes the vendor while this waits.
  {
    // Harness fix: 30 s closed the stream window before the driver's alive-gated 110 s
    // D1 fire could land on a fast app; the wait must outlast the fire (budget-capped).
    const waitCap = Math.max(5000, Math.min(90000, budgetLeft() - 25000));
    const t0 = Date.now();
    let log = await page.evaluate(pageStreamLog).catch(() => ({ entries: [] }));
    while (Date.now() - t0 < waitCap) {
      log = await page.evaluate(pageStreamLog).catch(() => ({ entries: [] }));
      if ((log.entries || []).some((e) => e.size != null && e.size > 0)) break;
      await sleep(700);
    }
    await sleep(3400);      // outlast the instrument's 3 s apply-detection cap, so
    //                        c1/digest1 are recorded even when a batch legally changes
    //                        no digest moment (harness fix: a 1.5 s read left them null)
    log = await page.evaluate(pageStreamLog).catch(() => ({ entries: [] }));
    const entries = log.entries || [];
    const model2 = buildModel(pack);
    let lastUpdate = null;
    const perBatch = [];
    for (const e of entries) {
      let digestOk = null, expDig = null, capped = false;
      if (e.size != null && e.records && e.records.length === e.size) {
        const touched = applyBatchToModel(model2, e.records);
        for (const t of touched) if (t.kind === 'update') lastUpdate = t;
        expDig = expectDigest(model2);
        if (e.digest1 && !e.digest1.__err) digestOk = digestTolOk(e.digest1, expDig);
      } else if (e.size != null && e.size > 0) capped = true;
      const bytes = e.c0 && e.c1
        ? (e.c1.bufDataBytes + e.c1.bufSubBytes) - (e.c0.bufDataBytes + e.c0.bufSubBytes) : null;
      perBatch.push({
        batch: e.batch, size: e.size, wireBytes: e.bytes, applyMs: e.applyMs,
        uploadedBytes: bytes,
        reallocsInWindow: e.c0 && e.c1 ? e.c1.reallocs - e.c0.reallocs : null,
        subDataCalls: e.c0 && e.c1 ? e.c1.bufSubCalls - e.c0.bufSubCalls : null,
        digestOk, capped,
        touchedIds: (e.records || []).map((r) => r.id).slice(0, 8),
      });
    }
    streamApplied = entries.length;                       // model2 replay covers them all
    // fold the replay into the session model (brush and pixel expectations stay brush-aware)
    for (const it of model2.items) {
      const cur = model.byId.get(it.id);
      if (cur == null) {
        applyBatchToModel(model, [{ id: it.id, amount_minor: it.amount_minor, currency: it.cur,
                                    status: it.status, day: it.day }]);
      } else {
        const m = model.items[cur];
        m.amount_minor = it.amount_minor; m.cur = it.cur; m.status = it.status;
        placeItem(model, m);
      }
    }
    let changedPixel = null;
    if (lastUpdate && model.byId.has(lastUpdate.id)) {
      // Harness fix: §3.7 grades the changed instance "at a close-up pose" — search
      // azimuth-biased close poses for a decisive point, apply through setCamera,
      // sample, then restore the defaults.
      const n = model.byId.get(lastUpdate.id);
      const itC = model.items[n];
      const rngC = seedRng(pack.seed || 'sb7', 'changedpx');
      const azC = Math.atan2(itC.x, itC.z) * 180 / Math.PI;
      const radC = Math.hypot(itC.x, itC.z);
      const posesC = [[V7.yaw0, V7.pitch0, V7.dist0]];
      for (let k = 0; k < 36; k++) {
        const pC = 16 + rngC() * 32;
        const dC = (k % 2 === 0)
          ? clamp(36 + rngC() * 130, 16, 340)
          : clamp((radC + 25 + rngC() * 55) / Math.cos(deg(pC)), 16, 340);
        posesC.push([azC + (rngC() - 0.5) * 40, pC, dC]);
      }
      let hitC = null;
      for (const [py, pp, pd] of posesC) {
        const ctxN = poseCtx(model, py, pp, pd, Wc, Hcs);
        const pt = findDecisivePointFor(ctxN, model, n);
        if (pt) { hitC = { pose: [py, pp, pd], pt }; break; }
      }
      if (hitC) {
        await setCam(hitC.pose[0], hitC.pose[1], hitC.pose[2]);
        const s = await page.evaluate(pageSamplePixels,
          { points: [{ cx: hitC.pt.sx, cy: hitC.pt.sy }] }).catch(() => null);
        const got = s && s.samples && s.samples[0] ? s.samples[0].got : null;
        const exp = expectPx(model.items[n], hitC.pt.factor);
        changedPixel = { id: lastUpdate.id, got, expect: exp,
                         ok: !!got && got.every((v, i) => Math.abs(v - exp[i]) <= V7.tol) };
        await setCam(V7.yaw0, V7.pitch0, V7.dist0);
      } else changedPixel = { id: lastUpdate.id, noDecisivePoint: true };
    }
    const dBrush = await vs7({ want: ['brush'] });
    const brushAfter = Array.isArray(dBrush.brush) ? dBrush.brush : null;
    const mutationSeen = d1TargetId != null &&
      entries.some((e) => (e.records || []).some((r) => r.id === d1TargetId));
    const d1Row = d1TargetId != null
      ? await page.evaluate(pageTableRowByld, { id: d1TargetId, click: false })
        .catch(() => ({ found: false }))
      : { found: false };
    // §3.1 pin: the layout basis must not have moved across the applied batches
    const layoutAfterRead = await vs7({ want: ['layout'] });
    const layoutAfterGot = layoutAfterRead.layout && !layoutAfterRead.layout.__err
      ? layoutAfterRead.layout : null;
    merge({ stream: {
      esUrls: log.esUrls || [], unsupported: !!log.unsupported,
      batchesObserved: entries.length, perBatch, changedPixel,
      digestOkCount: perBatch.filter((b) => b.digestOk === true).length,
      digestGradedCount: perBatch.filter((b) => b.digestOk != null).length,
      layoutAfter: { got: layoutAfterGot },
    } });
    merge({ streamApplied: {
      observed: entries.length > 0,
      applied: perBatch.some((b) => b.digestOk === true) || (changedPixel && changedPixel.ok === true),
      batches: entries.length,
    } });
    merge({ d1: {
      targetId: d1TargetId, armedBrushed: d1.brushed, via: d1.via,
      mutationSeen, brushAfter,
      survivedInBrush: !!(d1TargetId != null && brushAfter && brushAfter.includes(d1TargetId)),
      // survived is the D1-corner OBSERVATION: null unless the target was brushed when a
      // mutation for it was actually seen — the only case documented-vs-observed can grade.
      survived: (d1TargetId != null && d1.brushed && mutationSeen && brushAfter)
        ? brushAfter.includes(d1TargetId) : null,
      rowBrushedAfter: d1Row.found ? d1Row.dataBrushed === 'true' : null,
    } });
  }

  // background click clears the brush; dim lifts (pixel-verified back to full hex).
  if (inViewport) {
    await syncModelWithStream();
    await page.evaluate(pageScrollCanvasIntoView).catch(() => {});
    await sleep(150);
    rect = (await page.evaluate(pageCanvasRect).catch(() => null)) || rect;
    const ctxC = poseCtx(model, V7.yaw0, V7.pitch0, V7.dist0, Wc, Hcs);
    const tC = findPickTargets(ctxC, model, (pack.seed || 'sb7') + ':clear');
    if (!tC.fronts.length) {
      const loose = findLoosePoints(ctxC, model, (pack.seed || 'sb7') + ':clearpx', 1);
      if (loose.length) tC.fronts = [{ sx: loose[0].sx, sy: loose[0].sy,
                                       frontN: loose[0].frontN,
                                       frontId: model.items[loose[0].frontN].id }];
    }
    let clear = { available: !!tC.background };
    if (tC.background) {
      const probeF = tC.fronts[0] || null;                // a non-member while brush is non-empty
      let beforePx = null;
      if (probeF && model.brush.size > 0 && !model.brush.has(probeF.frontId)) {
        const s = await page.evaluate(pageSamplePixels,
          { points: [{ cx: probeF.sx, cy: probeF.sy }] }).catch(() => null);
        beforePx = s && s.samples && s.samples[0] ? s.samples[0].got : null;
      }
      await page.mouse.click(rect.left + tC.background.sx, rect.top + tC.background.sy);
      await sleep(450);
      const b = await vs7({ want: ['brush'] });
      const emptied = Array.isArray(b.brush) && b.brush.length === 0;
      if (emptied) model.brush.clear();
      let afterPx = null, fullHexOk = null;
      if (probeF) {
        const s = await page.evaluate(pageSamplePixels,
          { points: [{ cx: probeF.sx, cy: probeF.sy }] }).catch(() => null);
        afterPx = s && s.samples && s.samples[0] ? s.samples[0].got : null;
        const it = model.items[probeF.frontN];
        const hit=castPixel(ctxC,probeF.sx,probeF.sy)[0];
        const expFull = hit ? surfColor(ctxC,hit) : null;
        fullHexOk = !!afterPx && !!expFull && afterPx.every((v,i)=>Math.abs(v-expFull[i])<=V7.tol);
      }
      const cnt = await page.evaluate(pageBrushCount).catch(() => ({}));
      clear = { available: true, emptied, brushAfter: b.brush || null,
                countText: cnt.text || null, beforePx, afterPx, fullHexOk };
    }
    merge({ brushClear: clear });
  }

  // vs7dbgTruth (§3.8): the surface vs reality, assembled from this session's evidence.
  {
    const glFinal = await page.evaluate(pageGlCounters).catch(() => null);
    let frameGroupsInWindow = null;
    const db = result.dragBudget;
    if (db && db.watch && db.watch.c0 && db.watch.c1) {
      const ts = await page.evaluate(pageDrawTs, { tail: 6000 }).catch(() => []);
      frameGroupsInWindow = frameGroups(ts.filter(
        (t) => t >= db.watch.c0.t && t <= db.watch.c1.t)).length;
    }
    merge({ vs7dbgTruth: {
      surfacePresent: !!(result.ready && result.ready.vs7dbg),
      layoutOk: result.layout ? result.layout.ok : null,
      digestOk: result.digest ? result.digest.ok : null,
      digestMaxDelta: result.digest ? result.digest.maxDelta : null,
      cameraDefaultsYawErr: result.cameraMath && result.cameraMath.defaults
        ? result.cameraMath.defaults.yawErrDeg : null,
      framesReportedDelta: db ? db.deltaFrames : null,
      frameGroupsCounted: frameGroupsInWindow,
      framesAgree: db && db.deltaFrames != null && frameGroupsInWindow != null
        ? Math.abs(db.deltaFrames - frameGroupsInWindow) <= Math.max(4, 0.35 * frameGroupsInWindow)
        : null,
      pickTripletAgree: result.picks ? result.picks.agreeAll : null,
      heightVsDigest: result.heightPixels
        ? { ok: result.heightPixels.okCount, total: result.heightPixels.total } : null,
      restPixelOk: result.coast && result.coast.flick && result.coast.flick.restPixel
        ? result.coast.flick.restPixel.ok : null,
    } });
    merge({ gl: glFinal });
  }

  await sb71VisualScenario(page,model,H,pack);
  await saveShot('viz');
  await H.finalizeMedia();
  emit({ consoleErrors: consoleErrors() });
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
}
