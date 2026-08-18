# Deterministic 3D scoring for the swarm-bench product probe — design + validated prototype

Scope: how `product_probe.mjs` scores a spec-v3 "3D Overview panel" with finesse — deterministically, hermetically (raw WebGL, no CDN), under the existing node+playwright harness. Everything below marked **VERIFIED** was run on this machine (playwright Chromium **143.0.7499.4**, node v22.22.0, global playwright install at `~/.nvm/.../lib/node_modules/playwright`). Validation artifacts: `/private/tmp/claude-501/-Users-mihaiperdum-Projects-goose/f456d2b2-5676-41d6-ba79-ab5c53708d75/scratchpad/{webgl_env_test.mjs,webgl_env_test2.mjs,webgl_env_test3.mjs,bars.html,viz_mini_probe.mjs}`. No repo file was modified.

---

## 0. The design in one paragraph

The spec pins a complete **scene contract** — canvas identity and size, the world-space layout formula mapping payments to 3D bars, exact flat (unlit) status colors, the exact default camera, drag sensitivity, and two DOM readouts. Because every geometric fact is pinned, the probe **computes every expected pixel analytically**: Node-side model-view-projection math projects each bar's top-center to screen coordinates, an exact ray-AABB slab test decides which bars are legitimately occluded (excluded from the denominator, deterministically), and the page-side sampler reads the real framebuffer and compares. **No golden images, no image hashes, no LLM judge** — expectations are derived from the contract, so the instrument is robust to Chromium/SwiftShader version drift and to legitimate app variation, while staying bit-deterministic run to run (verified: identical results across launches, including the same single antialiased-edge miss).

---

## 1. VERIFIED environment facts (the research the design rests on)

| # | Fact | Evidence |
|---|------|----------|
| 1 | **Headless Chromium under playwright renders WebGL via SwiftShader by default** — renderer string `ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (LLVM 10.0.0)), SwiftShader driver)`, WebGL 2 available. | 5 launches with 5 different flag sets (none, `--use-angle=swiftshader`, `+--enable-unsafe-swiftshader`, `--use-gl=angle` combo, `--disable-gpu`) all report the same renderer. |
| 2 | **Rendering is byte-deterministic**: a depth-tested perspective cube scene gives full-framebuffer hash `1012323911` and screenshot-PNG hash `-1830945150` identical across separate browser launches; the flat-triangle scene hash `1130579036` identical across all five flag sets. | webgl_env_test.mjs, webgl_env_test2.mjs |
| 3 | **The preserveDrawingBuffer trap is real**: `readPixels` after the frame is composited returns `[0,0,0,0]` when the app created its context without `preserveDrawingBuffer`. Draw-then-read in the same task works. | webgl_env_test2.mjs (`stale: [0,0,0,0]`, `fresh.center: [255,0,0]`) |
| 4 | **The fix works**: an `addInitScript` patch wrapping `HTMLCanvasElement.prototype.getContext` (and `OffscreenCanvas.prototype`) to force `preserveDrawingBuffer: true`, record acquisitions `{type, canvasId}`, count `drawArrays/drawElements[Instanced]` calls, and listen for `webglcontextlost` — after it, readPixels 400 ms after compositing returns the true pixel `[255,0,0,255]`, and the instrumentation reports `contexts:[{type:"webgl",canvasId:"scene"}], drawCalls:1`. | webgl_env_test3.mjs, viz_mini_probe.mjs |
| 5 | **rAF runs at real ~60 Hz in headless**: frame-delta p50 16.70 ms, p95 17.1–17.3 ms over 60 frames. Cadence measurement is viable. | webgl_env_test2.mjs |
| 6 | **WebGL can be disabled two ways**: `--disable-webgl` (getContext→null) and — the one the probe uses — an init-script patch returning null for webgl types, which also covers OffscreenCanvas and **leaves canvas-2D alive** (`twod:true`), exactly what the fallback check needs, with launch args unchanged across scenarios. | webgl_env_test2.mjs |
| 7 | `reducedMotion:'reduce'` context option works (`matchMedia('(prefers-reduced-motion: reduce)').matches === true`), and `--js-flags=--random-seed=N` pins `Math.random` to an identical sequence across launches. | webgl_env_test3.mjs |
| 8 | **Coordinate mapping**: readPixels is bottom-left origin; CSS→backing is `bx = round(cx·W/rect.width)`, `by = H−1−round(cy·H/rect.height)`. Validated by exact center-pixel matches. | webgl_env_test2/3, viz_mini_probe |
| 9 | **End-to-end pipeline against a reference scene** (25-bar grid, pinned camera): tops **16/16** matched at analytically projected positions (9 occluded tops correctly excluded by the ray test), above-top background **17/17**, corners **4/4**, picks **4/4**, background click clears, **depth pair correct** (clicking an occluded bar's projected top picks the occluding bar — the app's independent CPU raycast agrees with the probe's analytic expectation), drag +120 px → readout `az 83°` and **11/11** tops matching the recomputed camera, reverse drag restores **16/16**. **Bit-identical on rerun**, including the one antialiased-edge side-sample miss. | viz_mini_probe.mjs + bars.html |
| 10 | **The probe discriminates**: before the reference app's camera was corrected, it had an azimuth **sign flip** vs the spec formula — precisely the bug class a model ships — and scored tops 1/13, picks 0/5, while drag-readout still passed. The check separates "renders something 3D-ish" from "implements the pinned contract". | first viz_mini_probe run |

---

## 2. Exact launch args and per-scenario context config

```js
// Scoped to the viz* scenarios so existing load/sync/error/empty measurements are untouched.
const VIZ_LAUNCH_ARGS = [
  '--use-angle=swiftshader',        // ANGLE→SwiftShader(Vulkan): software rendering, cross-machine deterministic.
                                    // (Default on this machine already — pinned so a Linux/GPU CI box can't drift.)
  '--enable-unsafe-swiftshader',    // Chromium ≥139 gates software WebGL behind this; harmless where default.
  '--force-color-profile=srgb',     // pin color management
  '--force-device-scale-factor=1',  // DPR=1: CSS px == backing px == readPixels px
  '--js-flags=--random-seed=1357',  // pins Math.random for apps that jitter layout with it (VERIFIED)
];

// Per-scenario context:
//   viz           reducedMotion:'reduce'         + addInitScript(glInstrument)
//   viz-motion    reducedMotion:'no-preference'  + addInitScript(glInstrument)
//   viz-fallback  reducedMotion:'reduce'         + addInitScript(glKill)
const context = await browser.newContext({
  viewport: { width: 1280, height: 800 },
  reducedMotion: scenario === 'viz-motion' ? 'no-preference' : 'reduce',
});
```

Reduced-motion is the determinism lever **and** a real product requirement: the spec requires a static scene under `prefers-reduced-motion: reduce`, so all pixel sampling runs in that mode; the idle animation is measured only in `viz-motion`. Optional hardening for the time-based-animation check: a CDP `Emulation.setCPUThrottlingRate {rate: 4}` window makes frame-count-driven animation drift measurably from the pinned °/s rate.

---

## 3. What spec v3 must pin (the scene contract) — each pin exists to make one check computable

Ready-to-adapt spec section (numbers must match the probe's `VIZ` constants verbatim):

> ### 7. The 3D Overview panel
> Above the table, a **3D overview** renders the payments of the current table page as a grid of 3D bars, drawn with **raw WebGL** (WebGL 1 or 2 — no libraries, no CDN) on `<canvas id="scene" width="800" height="480">`, displayed at exactly 800×480 CSS px on the desktop viewport.
>
> **Layout.** The page's payments in chronological order, index `i` (0-based): bar `i` sits at world `x = (i mod 5 − 2) · 2.2`, `z = (⌊i/5⌋ − 2) · 2.2`, footprint 1.4×1.4, from `y = 0` up to `y = h`, with `h = 0.4 + 3.6 · amount_minor / max_amount_on_page`.
>
> **Color.** Flat, unlit. Top face exactly: settled `#22C55E` (34,197,94), pending `#F59E0B` (245,158,11), refunded `#EF4444` (239,68,68). Side faces exactly 60% of the top color, rounded per channel. Clear color `#0B1220` (11,18,32). No lighting, gradients, or textures on the bars.
>
> **Camera.** Perspective, fovY 45°, near 0.1, far 100, aspect = canvas aspect. Orbit camera: `eye = (R·cos(el)·sin(az), R·sin(el), R·cos(el)·cos(az))` with `R = 13`, default `az = 35°`, `el = 40°`, looking at the origin, up `+Y`.
>
> **Interaction.** Left-drag orbits: dragging **right increases azimuth by 0.4° per CSS pixel**; vertical drag changes elevation at the same rate, clamped to [10°, 80°]. An element `#camera-readout` always shows the camera as `az N° · el M°` (integers, rounded). Clicking a bar writes that payment's **id** into `#picked-payment`; clicking empty space clears it.
>
> **Motion.** Under `prefers-reduced-motion: reduce` the scene is **static**. Otherwise an idle orbit advances azimuth at **6°/second** (time-based, not per-frame), pausing during drag.
>
> **Fallback.** When `canvas.getContext('webgl')` returns null the panel must not throw: it shows a 2D rendering of the same data (canvas-2D, SVG, or DOM) plus a visible notice that 3D is unavailable. The table must remain fully functional.

Why each pin: canvas id/size → sampling coordinates; layout+height formulas → the probe can compute every bar's AABB; exact flat colors → tolerance-±8 pixel equality; exact camera → the probe computes the MVP itself; drag sensitivity+sign → post-drag camera is predictable (and sign errors are separately diagnosable); readouts → picking and camera state are machine-readable without trusting app internals; reduced-motion → freezes time for deterministic sampling; 6°/s time-based → punishes per-frame animation; fallback contract → degradation is testable. Bars are **axis-aligned boxes**, so occlusion is exact via slab tests — that choice is what makes "score 3D with finesse, deterministically" tractable at all.

---

## 4. Probe architecture

- **Expectation input**: the scorer passes the expected first-page payments via env `BENCH_VIZ_EXPECT` = `{"payments":[{"id","amount_minor","status"},…×25]}` (chronological; built from `fixtures.py`). Env is precedented (`BENCH_SHOTS_DIR`).
- **Node side is the source of truth**: projection, occlusion, sample building, pick-target selection all happen in the probe process; the page only reads pixels and DOM. (Residual risk: an in-page app could theoretically monkey-patch `readPixels`; these are model-built apps, not adversaries — and a screenshot-clip cross-check via node `zlib` PNG decode is listed as hardening, since screenshots were verified byte-deterministic too.)
- **Probe emits facts, the scorer judges**: counts, fractions, raw colors, readout strings go into the JSON; all ladders live in `score_build.py` (matches the existing probe/scorer separation and keeps `SCORER_VERSION` the only judgment version).
- **Integration**: three new scenarios `viz | viz-motion | viz-fallback` extend the argv whitelist and the `main()` if/else chain; `VIZ_LAUNCH_ARGS` are added only for them; `saveShot('viz')` etc. keep the quality-screenshot contract.

---

## 5. The code

### 5a. Constants + Node-side math — VERIFIED verbatim in viz_mini_probe.mjs

```js
// ── pinned scene contract (must mirror spec v3 §7 exactly) ──
const VIZ = {
  cssW: 800, cssH: 480, fovYDeg: 45, near: 0.1, far: 100,
  az0: 35, el0: 40, dist: 13, dragDegPerPx: 0.4, idleDegPerSec: 6,
  grid: { cols: 5, pitch: 2.2, half: 0.7 }, hMin: 0.4, hSpan: 3.6,
  bg: [11, 18, 32],
  status: { settled: [34, 197, 94], pending: [245, 158, 11], refunded: [239, 68, 68] },
  sideFactor: 0.6, tol: 8,
};
const sideColor = (c) => c.map((v) => Math.round(v * VIZ.sideFactor));

const deg = (d) => (d * Math.PI) / 180;
const sub = (a, b) => [a[0]-b[0], a[1]-b[1], a[2]-b[2]];
const dot = (a, b) => a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
const cross = (a, b) => [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
const norm = (a) => { const l = Math.hypot(...a); return [a[0]/l, a[1]/l, a[2]/l]; };

function lookAt(eye, target, up) {           // column-major, WebGL convention
  const f = norm(sub(target, eye)), s = norm(cross(f, up)), u = cross(s, f);
  return [s[0], u[0], -f[0], 0, s[1], u[1], -f[1], 0, s[2], u[2], -f[2], 0,
          -dot(s, eye), -dot(u, eye), dot(f, eye), 1];
}
function perspective(fovY, aspect, n, f) {
  const t = 1 / Math.tan(fovY / 2);
  return [t/aspect, 0, 0, 0, 0, t, 0, 0, 0, 0, (f+n)/(n-f), -1, 0, 0, (2*f*n)/(n-f), 0];
}
function mat4mul(a, b) {
  const o = new Array(16).fill(0);
  for (let i = 0; i < 4; i++) for (let j = 0; j < 4; j++) for (let k = 0; k < 4; k++)
    o[j*4+i] += a[k*4+i] * b[j*4+k];
  return o;
}
function cameraEye(azDeg, elDeg, dist) {
  const a = deg(azDeg), e = deg(elDeg);
  return [dist*Math.cos(e)*Math.sin(a), dist*Math.sin(e), dist*Math.cos(e)*Math.cos(a)];
}
function vizMvp(azDeg, elDeg) {
  return mat4mul(perspective(deg(VIZ.fovYDeg), VIZ.cssW / VIZ.cssH, VIZ.near, VIZ.far),
                 lookAt(cameraEye(azDeg, elDeg, VIZ.dist), [0, 0, 0], [0, 1, 0]));
}
function project(m, w) {                      // world → CSS px inside the canvas, or null behind camera
  const c = [0, 1, 2, 3].map((i) => m[i]*w[0] + m[4+i]*w[1] + m[8+i]*w[2] + m[12+i]);
  if (c[3] <= 0) return null;
  return { x: ((c[0]/c[3] + 1) / 2) * VIZ.cssW, y: ((1 - c[1]/c[3]) / 2) * VIZ.cssH };
}
function rayAABB(o, d, mn, mx) {              // slab test: entry distance, or null
  let t0 = -Infinity, t1 = Infinity;
  for (let k = 0; k < 3; k++) {
    if (Math.abs(d[k]) < 1e-9) { if (o[k] < mn[k] || o[k] > mx[k]) return null; continue; }
    let a = (mn[k]-o[k]) / d[k], b = (mx[k]-o[k]) / d[k];
    if (a > b) [a, b] = [b, a];
    t0 = Math.max(t0, a); t1 = Math.min(t1, b);
  }
  return t0 <= t1 && t1 > 0 ? Math.max(t0, 0) : null;
}
function barGeom(payments) {
  const maxAmount = Math.max(...payments.map((p) => p.amount_minor));
  return payments.map((p, i) => {
    const x = (i % VIZ.grid.cols - 2) * VIZ.grid.pitch;
    const z = (Math.floor(i / VIZ.grid.cols) - 2) * VIZ.grid.pitch;
    const h = VIZ.hMin + VIZ.hSpan * p.amount_minor / maxAmount;
    return { id: p.id, status: p.status, x, z, h,
             mn: [x - VIZ.grid.half, 0, z - VIZ.grid.half],
             mx: [x + VIZ.grid.half, h, z + VIZ.grid.half] };
  });
}
function firstHit(eye, P, bars) {             // first box on the full forward ray eye→P→∞
  const d = norm(sub(P, eye));
  let best = null;
  bars.forEach((b, i) => {
    const t = rayAABB(eye, d, b.mn, b.mx);
    if (t != null && (best === null || t < best.t)) best = { index: i, t };
  });
  return best;
}

// Build every sample with its analytic expectation. Occlusion-filtered, deterministic.
function buildVizSamples(payments, azDeg, elDeg) {
  const bars = barGeom(payments);
  const eye = cameraEye(azDeg, elDeg, VIZ.dist);
  const mvp = vizMvp(azDeg, elDeg);
  const tops = [], above = [], sides = [], occludedTops = [];
  bars.forEach((b, i) => {
    const T = [b.x, b.h, b.z], pT = project(mvp, T);
    if (pT) {
      const hit = firstHit(eye, T, bars), dT = Math.hypot(...sub(T, eye));
      // ray from above enters box i exactly at its own top face (t == dT); nearer hit ⇒ occluded
      if (hit && hit.index !== i && hit.t < dT - 1e-6)
        occludedTops.push({ i, id: bars[hit.index].id, css: pT, occluder: hit.index });
      else
        tops.push({ i, id: b.id, cx: pT.x, cy: pT.y, expect: VIZ.status[b.status], kind: 'top' });
    }
    const Q = [b.x, b.h + 0.6, b.z], pQ = project(mvp, Q);   // just above the top: must be sky
    if (pQ && !firstHit(eye, Q, bars))                        // FULL ray clear (a box behind Q fills the pixel too)
      above.push({ i, cx: pQ.x, cy: pQ.y, expect: VIZ.bg, kind: 'above' });
    const S = [b.x, 0.6 * b.h, b.z], pS = project(mvp, S);    // inside the bar column: its own surface
    const hS = pS && firstHit(eye, S, bars);
    if (pS && hS && hS.index === i)
      sides.push({ i, cx: pS.x, cy: pS.y,
                   expectAny: [sideColor(VIZ.status[b.status]), VIZ.status[b.status]], kind: 'side' });
  });
  const sky = [];                                             // background points over the grid gaps
  for (const [gx, gz] of [[-1.1,-1.1],[1.1,-1.1],[-1.1,1.1],[1.1,1.1],[0,0]]) {
    const G = [gx * VIZ.grid.pitch, VIZ.hMin + VIZ.hSpan + 1.2, gz * VIZ.grid.pitch];
    const p = project(mvp, G);
    if (p && !firstHit(eye, G, bars)) sky.push({ cx: p.x, cy: p.y, expect: VIZ.bg, kind: 'sky' });
  }
  const corners = [[2,2],[VIZ.cssW-3,2],[2,VIZ.cssH-3],[VIZ.cssW-3,VIZ.cssH-3]]
    .map(([cx, cy]) => ({ cx, cy, expect: VIZ.bg, kind: 'corner' }));
  const grid = [];                                            // blind coverage grid, no per-point expectation
  for (let ix = 0; ix < 6; ix++) for (let iy = 0; iy < 4; iy++)
    grid.push({ cx: (ix + 0.5) * VIZ.cssW / 6, cy: (iy + 0.5) * VIZ.cssH / 4, kind: 'grid' });
  return { bars, eye, tops, above, sides, sky, corners, grid, occludedTops };
}
```

### 5b. Page-side init scripts — VERIFIED

```js
// Installed via context.addInitScript for viz + viz-motion. Forces preserveDrawingBuffer so the
// probe can readPixels AFTER compositing (fact #3/#4), records acquisition, counts draw calls.
function glInstrument() {
  window.__probeViz = { contexts: [], drawCalls: 0, contextLost: 0 };
  const wrap = (proto, offscreen) => {
    const orig = proto.getContext;
    proto.getContext = function (type, attrs) {
      if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
        attrs = Object.assign({}, attrs || {}, { preserveDrawingBuffer: true });
        const gl = orig.call(this, type, attrs);
        if (gl && !gl.__probeSeen) {
          gl.__probeSeen = true;
          window.__probeViz.contexts.push({ type, offscreen, canvasId: this.id || null });
          for (const fn of ['drawArrays','drawElements','drawArraysInstanced','drawElementsInstanced'])
            if (typeof gl[fn] === 'function') {
              const d = gl[fn].bind(gl);
              gl[fn] = (...a) => { window.__probeViz.drawCalls++; return d(...a); };
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

// Installed for viz-fallback only: WebGL unavailable, canvas-2D alive (fact #6).
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
```

### 5c. Page-side sampler — VERIFIED

```js
// One readPixels of the whole framebuffer, then indexed lookups. CSS→backing mapping and
// bottom-left y-flip validated by exact center-pixel matches (fact #8).
function pageSampleScene(arg) {
  const canvas = document.getElementById(arg.canvasId) || document.querySelector('canvas');
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
  const near = (a, b, t) => Math.abs(a[0]-b[0]) <= t && Math.abs(a[1]-b[1]) <= t && Math.abs(a[2]-b[2]) <= t;
  const out = [];
  for (const s of arg.samples) {
    const got = at(s.cx, s.cy);
    let ok = null;
    if (s.expect) ok = near(got, s.expect, arg.tol);
    else if (s.expectAny) ok = s.expectAny.some((e) => near(got, e, arg.tol));
    out.push({ kind: s.kind, i: s.i, id: s.id, got, ok });
  }
  return { found: true, glReadable: true,
           rect: { left: rect.left, top: rect.top, w: rect.width, h: rect.height },
           backing: { w: W, h: H }, instrument: window.__probeViz || null, samples: out };
}
```

### 5d. Small page-side helpers (validated patterns, same self-contained style)

```js
function pageVizReady() {                     // poll target for scene readiness
  const v = window.__probeViz || { drawCalls: 0 };
  const c = document.getElementById('scene') || document.querySelector('canvas');
  return { canvas: !!c, drawCalls: v.drawCalls, contexts: (v.contexts || []).length };
}
function pageCameraReadout() {
  const el = document.getElementById('camera-readout');
  if (!el || !el.getClientRects().length) return null;
  const m = /az\s*(-?\d+)\s*°?[^-\d]*el\s*(-?\d+)/i.exec(el.textContent || '');
  return m ? { az: parseInt(m[1], 10), el: parseInt(m[2], 10), raw: el.textContent.trim().slice(0, 60) } : { raw: el.textContent.trim().slice(0, 60) };
}
function pagePickedPayment() {
  const el = document.getElementById('picked-payment');
  return el ? (el.textContent || '').trim().slice(0, 80) : null;
}
function pageRafCadence(n) {                  // n frame timestamps via requestAnimationFrame
  return new Promise((res) => {
    const ts = [];
    function tick(t) { ts.push(t); if (ts.length < n) requestAnimationFrame(tick); else res(ts); }
    requestAnimationFrame(tick);
  });
}
function pageVizFallback() {                  // viz-fallback analysis: what replaced the 3D view?
  const c = document.getElementById('scene') || document.querySelector('canvas');
  let canvas2dPainted = false;
  if (c) {
    const ctx = c.getContext('2d');
    if (ctx) {
      try {
        const d = ctx.getImageData(0, 0, c.width, c.height).data;
        const first = [d[0], d[1], d[2]];
        for (let i = 4; i < d.length; i += 397 * 4)
          if (Math.abs(d[i]-first[0]) > 10 || Math.abs(d[i+1]-first[1]) > 10 || Math.abs(d[i+2]-first[2]) > 10) {
            canvas2dPainted = true; break;
          }
      } catch (e) {}
    }
  }
  const vis = (el) => !!(el.getClientRects && el.getClientRects().length) &&
                      getComputedStyle(el).visibility !== 'hidden';
  const panel = (c && c.parentElement) || document.body;
  const domFallback = Array.from(panel.querySelectorAll('svg, [class*="bar" i], [class*="chart" i]')).filter(vis).length;
  let notice = null;
  const re = /2d|fallback|unavailable|not\s+supported|webgl/i;
  for (const el of document.querySelectorAll('body *')) {
    if (el.children.length > 0 || el.closest('script,style,td,th')) continue;
    if (!vis(el)) continue;
    const t = (el.innerText || '').trim();
    if (t && t.length < 200 && re.test(t)) { notice = t.slice(0, 120); break; }
  }
  return { canvasPresent: !!c, canvas2dPainted, domFallbackElements: domFallback, notice };
}
```

### 5e. Scenario blocks (drop into `product_probe.mjs`'s `main()` dispatch)

The building blocks are all verified; this glue composes them in the file's existing style (`err`, `emit`, `safeGoto`, `waitIdle`, `sleep`, `consoleErrors`, `saveShot` as already defined there). Expected-payments come from `JSON.parse(process.env.BENCH_VIZ_EXPECT).payments`.

```js
  } else if (scenario === 'viz') {
    const expect = JSON.parse(process.env.BENCH_VIZ_EXPECT || '{"payments":[]}').payments;
    const navigationError = await safeGoto(20000);
    if (navigationError) { emit({ navigationError, consoleErrors: consoleErrors() }); return; }
    await waitIdle(10000);
    // scene readiness: canvas exists AND at least one draw call landed (10s cap, 300ms settle)
    let ready = { canvas: false, drawCalls: 0 };
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      ready = await page.evaluate(pageVizReady).catch(() => ready);
      if (ready.canvas && ready.drawCalls > 0) break;
      await sleep(150);
    }
    await sleep(300);

    const S = buildVizSamples(expect, VIZ.az0, VIZ.el0);
    const all = [...S.tops, ...S.above, ...S.sides, ...S.sky, ...S.corners, ...S.grid];
    const r1 = await page.evaluate(pageSampleScene, { canvasId: 'scene', samples: all, tol: VIZ.tol });
    // reduced-motion honored: identical grid colors 700ms apart
    await sleep(700);
    const r1b = await page.evaluate(pageSampleScene,
      { canvasId: 'scene', samples: S.grid, tol: VIZ.tol }).catch(() => null);
    const staticUnderReduce = !!(r1.samples && r1b && r1b.samples) &&
      JSON.stringify(r1.samples.filter((s) => s.kind === 'grid').map((s) => s.got)) ===
      JSON.stringify(r1b.samples.map((s) => s.got));

    const cnt = (k) => {
      const a = (r1.samples || []).filter((s) => s.kind === k);
      return { ok: a.filter((s) => s.ok).length, total: a.length };
    };
    const gridGot = (r1.samples || []).filter((s) => s.kind === 'grid').map((s) => s.got);
    const isBg = (c) => Math.abs(c[0]-VIZ.bg[0]) <= VIZ.tol && Math.abs(c[1]-VIZ.bg[1]) <= VIZ.tol && Math.abs(c[2]-VIZ.bg[2]) <= VIZ.tol;
    const nonBg = gridGot.filter((c) => !isBg(c));
    const topGot = (r1.samples || []).filter((s) => s.kind === 'top' && s.ok).map((s) => s.got.join(','));

    // ── picking (default camera; clicks do not move the camera) ──
    const rect = r1.rect || { left: 0, top: 0 };
    const stride = Math.max(1, Math.ceil(S.tops.length / 5));
    const targets = S.tops.filter((_, k) => k % stride === 0).slice(0, 5);
    const picks = [];
    for (const t of targets) {
      await page.mouse.click(rect.left + t.cx, rect.top + t.cy);
      await sleep(80);
      const got = await page.evaluate(pagePickedPayment).catch(() => null);
      picks.push({ want: t.id, got, ok: got === t.id });
    }
    let bgClear = null;
    const bgPt = S.sky[0] || S.corners[0];
    if (bgPt) {
      await page.mouse.click(rect.left + bgPt.cx, rect.top + bgPt.cy);
      await sleep(80);
      const got = await page.evaluate(pagePickedPayment).catch(() => null);
      bgClear = got === '' || got === null;
    }
    let depthPair = { available: S.occludedTops.length > 0, ok: null };
    if (depthPair.available) {
      const o = S.occludedTops[0];
      await page.mouse.click(rect.left + o.css.x, rect.top + o.css.y);
      await sleep(80);
      const got = await page.evaluate(pagePickedPayment).catch(() => null);
      depthPair = { available: true, want: S.bars[o.occluder].id, got, ok: got === S.bars[o.occluder].id };
    }

    // ── drag LAST (it moves the camera): +120px ⇒ az 35+48=83°, then reverse ──
    const cx0 = rect.left + VIZ.cssW / 2, cy0 = rect.top + VIZ.cssH / 2;
    const drag = async (dx) => {
      await page.mouse.move(cx0, cy0); await page.mouse.down();
      await page.mouse.move(cx0 + dx, cy0, { steps: 8 }); await page.mouse.up();
      await sleep(200);
    };
    const baselineTopColors = (r1.samples || []).filter((s) => s.kind === 'top').map((s) => s.got.join(','));
    await drag(120);
    const azT = VIZ.az0 + 120 * VIZ.dragDegPerPx;
    const S2 = buildVizSamples(expect, azT, VIZ.el0);
    const S2f = buildVizSamples(expect, VIZ.az0 - 120 * VIZ.dragDegPerPx, VIZ.el0);  // sign-flip diagnostic
    const r2 = await page.evaluate(pageSampleScene,
      { canvasId: 'scene', samples: [...S2.tops, ...S2f.tops.map((s) => ({ ...s, kind: 'topflip' }))], tol: VIZ.tol })
      .catch(() => ({ samples: [] }));
    const readout = await page.evaluate(pageCameraReadout).catch(() => null);
    await drag(-120);
    const r3 = await page.evaluate(pageSampleScene, { canvasId: 'scene', samples: S.tops, tol: VIZ.tol })
      .catch(() => ({ samples: [] }));
    const f = (arr) => { const a = arr.filter((s) => s.ok).length; return { ok: a, total: arr.length }; };
    const changed = (r2.samples || []).some((s, i) =>
      s.kind === 'top' && baselineTopColors[i] !== undefined && s.got.join(',') !== baselineTopColors[i]);

    await saveShot('viz');
    emit({
      ready, canvasRect: r1.rect || null, backing: r1.backing || null,
      gl: r1.instrument || null, glReadable: !!r1.glReadable,
      staticUnderReduce,
      contextCheck: { gridNonBg: nonBg.length, gridTotal: gridGot.length,
                      gridDistinct: new Set(nonBg.map((c) => c.join(','))).size,
                      corners: cnt('corner') },
      binding: { tops: cnt('top'), above: cnt('above'), sides: cnt('side'), sky: cnt('sky'),
                 occludedTops: S.occludedTops.length,
                 statusesExpected: new Set(expect.map((p) => p.status)).size,
                 distinctTopColors: new Set(topGot).size },
      picking: { picks, correct: picks.filter((p) => p.ok).length, total: picks.length,
                 bgClear, depthPair },
      drag: { pixelsChanged: changed,
              proj: f((r2.samples || []).filter((s) => s.kind === 'top')),
              projFlipped: f((r2.samples || []).filter((s) => s.kind === 'topflip')),
              readout, expectedAz: Math.round(azT),
              reverse: f(r3.samples || []) },
      consoleErrors: consoleErrors(),
    });

  } else if (scenario === 'viz-motion') {
    const navigationError = await safeGoto(20000);
    if (navigationError) { emit({ navigationError, consoleErrors: consoleErrors() }); return; }
    await waitIdle(10000);
    await sleep(1000);
    const grid8 = Array.from({ length: 8 }, (_, k) =>
      ({ cx: (k % 4 + 0.5) * VIZ.cssW / 4, cy: (Math.floor(k / 4) + 0.5) * VIZ.cssH / 2, kind: 'grid' }));
    const a = await page.evaluate(pageSampleScene, { canvasId: 'scene', samples: grid8, tol: VIZ.tol });
    const az1 = await page.evaluate(pageCameraReadout).catch(() => null);
    const t1 = Date.now();
    await sleep(700);
    const b = await page.evaluate(pageSampleScene, { canvasId: 'scene', samples: grid8, tol: VIZ.tol });
    const animating = !!(a.samples && b.samples) &&
      JSON.stringify(a.samples.map((s) => s.got)) !== JSON.stringify(b.samples.map((s) => s.got));
    // time-based rate over ~2s (optional: wrap in Emulation.setCPUThrottlingRate 4 to catch per-frame animators)
    await sleep(2000 - (Date.now() - t1) > 0 ? 2000 - (Date.now() - t1) : 0);
    const az2 = await page.evaluate(pageCameraReadout).catch(() => null);
    const wallS = (Date.now() - t1) / 1000;
    const azDelta = az1 && az2 && az1.az != null && az2.az != null
      ? ((az2.az - az1.az) % 360 + 360) % 360 : null;
    const ts = await page.evaluate(pageRafCadence, 121).catch(() => []);
    const deltas = ts.slice(1).map((t, i) => t - ts[i]).sort((x, y) => x - y);
    const q = (p) => deltas.length ? +deltas[Math.floor(p * (deltas.length - 1))].toFixed(2) : null;
    await saveShot('viz-motion');
    emit({ animating, azStart: az1, azEnd: az2, azDeltaDeg: azDelta, windowS: +wallS.toFixed(2),
           expectedDeltaDeg: +(VIZ.idleDegPerSec * wallS).toFixed(1),
           raf: { frames: ts.length, p50: q(0.5), p95: q(0.95), p99: q(0.99) },
           consoleErrors: consoleErrors() });

  } else if (scenario === 'viz-fallback') {
    const navigationError = await safeGoto(20000);
    if (!navigationError) { await waitIdle(8000); await sleep(800); }
    const fb = await evalRetry(pageVizFallback, { canvasPresent: false });
    const snap = await evalRetry(pageViewSnapshot, null);
    await saveShot('viz-fallback');
    emit({ navigationError, ...fb,
           renderedRowCount: snap ? snap.rowCount : 0,
           consoleErrors: consoleErrors() });   // pageerror listener already feeds consoleErrors
  }
```

---

## 6. The five check families — ladders, cheats, counter-assertions

Probe emits facts; these ladders live in `score_build.py` as `product_check` functions in a new tier **G**. All fractions below are deterministic because the sample sets, occlusion filtering, and SwiftShader pixels are (facts #2, #9).

### (1) `g_webgl_context` — a real context that really drew

```python
@product_check("g_webgl_context", "G")
def _(c):
    p = c.probe_viz
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    gl = p.get("gl") or {}
    ctxs = gl.get("contexts") or []
    on_scene = any(x.get("canvasId") == "scene" for x in ctxs)
    ck = p.get("contextCheck") or {}
    corners = ck.get("corners") or {}
    parts = {
        "canvas_800x480": (p.get("backing") or {}).get("w") == 800 and (p.get("backing") or {}).get("h") == 480,
        "webgl_on_scene": on_scene,
        "draw_calls": (gl.get("drawCalls") or 0) >= 1,
        "coverage": (ck.get("gridNonBg") or 0) >= 3 and (ck.get("gridDistinct") or 0) >= 2,
        "corners_bg": corners.get("ok") == corners.get("total") and (corners.get("total") or 0) > 0,
    }
    return g(0.2*parts["canvas_800x480"] + 0.2*parts["webgl_on_scene"] + 0.15*parts["draw_calls"]
             + 0.25*parts["coverage"] + 0.2*parts["corners_bg"],
             f"ctx={len(ctxs)} draws={gl.get('drawCalls')} nonBg={ck.get('gridNonBg')}/{ck.get('gridTotal')} "
             f"distinct={ck.get('gridDistinct')} corners={corners.get('ok')}/{corners.get('total')}",
             "no real 3D surface — the panel is decoration or absent", parts=parts)
```

Cheats and counters: **acquire-and-clear** (no geometry) → `drawCalls` counts only draw*, clear scores 0 there, and coverage fails. **Canvas-2D fake** → no webgl context recorded on #scene. **Hidden 1×1 webgl canvas + big 2D fake** → `canvasId` recording + pinned 800×480 backing check. **Full-screen wash** → corners must equal the pinned clear color exactly (±8).

### (2) `g_data_binding` — K payments drawn as K distinct, correctly-placed, correctly-colored marks

```python
@product_check("g_data_binding", "G")
def _(c):
    p = c.probe_viz
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    b = p.get("binding") or {}
    tops, above, sides, sky = (b.get(k) or {} for k in ("tops", "above", "sides", "sky"))
    fr = lambda d: (d.get("ok", 0) / d["total"]) if d.get("total") else 0.0
    score = 0.5 * fr(tops) + 0.25 * fr(above) + 0.15 * min(fr(sides) / 0.75, 1.0) + 0.10 * fr(sky)
    # mono-wash guard: >=2 statuses expected but tops show <2 distinct colors ⇒ cap hard
    if (b.get("statusesExpected") or 0) >= 2 and (b.get("distinctTopColors") or 0) < 2:
        score = min(score, 0.25)
    return g(score,
             f"tops {tops.get('ok')}/{tops.get('total')} (occl {b.get('occludedTops')}), "
             f"above {above.get('ok')}/{above.get('total')}, sides {sides.get('ok')}/{sides.get('total')}, "
             f"sky {sky.get('ok')}/{sky.get('total')}, colors {b.get('distinctTopColors')}",
             "the scene does not encode the data — bars misplaced, mis-colored, or mis-heighted")
```

Semantics: each visible bar's **top-center pixel** must show its status color at the analytically projected position (position depends on `h`, so **wrong heights fail here directly** — an equal-heights cheat collapses `tops`); the pixel **just above** each top must be background (brackets the height from above; full forward ray must be clear — a box behind the point fills the pixel too, which the builder accounts for); the mid-column **side** sample must be the bar's own surface (side- or top-color; scored against a 0.75 threshold because a silhouette-adjacent sample can land on an antialiased edge — measured 11/12 on the reference, deterministic across runs); **sky** points over the grid gaps must be background. Cheats: **mono-wash** (paint everything green) → distinct-color guard caps at 0.25, and pending/refunded top expectations plus above/sky/corners all fail. **Billboard sprites at computed positions** → to pass they must recompute correct perspective projections — at which point the app has implemented real 3D projection, which is the thing measured. **Occlusion freeloading** → occluded tops are excluded analytically, not by generosity; the count is emitted (`occludedTops`) and pinned by the fixture.

### (3a) `g_camera_drag` — drag → new camera → reprojection truth → reversibility

```python
@product_check("g_camera_drag", "G")
def _(c):
    p = c.probe_viz
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    d = p.get("drag") or {}
    fr = lambda x: ((x or {}).get("ok", 0) / x["total"]) if (x or {}).get("total") else 0.0
    proj, projf, rev = fr(d.get("proj")), fr(d.get("projFlipped")), fr(d.get("reverse"))
    ro = d.get("readout") or {}
    readout_ok = ro.get("az") is not None and abs(ro["az"] - (d.get("expectedAz") or 0)) <= 1
    # a sign-flipped drag (matches az0-48 instead of az0+48) earns half the projection credit
    proj_credit = proj if proj >= projf else 0.5 * projf
    score = 0.15 * bool(d.get("pixelsChanged")) + 0.35 * proj_credit + 0.15 * readout_ok + 0.35 * rev
    return g(score,
             f"changed={d.get('pixelsChanged')} proj {proj:.2f} (flip {projf:.2f}) "
             f"readout az={ro.get('az')} want {d.get('expectedAz')} reverse {rev:.2f}",
             "the orbit control does not implement the documented camera")
```

Cheats: **readout-only update** → pixel sub-checks fail (0.65 of the weight). **Jitter/redraw-noise on drag** → passes `pixelsChanged`, fails reprojection and reversibility. **CSS-transform the canvas** → a 2D transform cannot reproduce true perspective reprojection of 25 points (parallax); `proj` fails. **Sign confusion** (the bug the validation actually caught) → diagnosed separately via `projFlipped` and half-credited, which is finesse rather than a cliff. Reversibility (drag back ⇒ baseline samples restored, verified 16/16) is the honesty anchor: noise cannot pass it.

### (3b) `g_picking` — click → the right payment id

```python
@product_check("g_picking", "G")
def _(c):
    p = c.probe_viz
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    k = p.get("picking") or {}
    frac = (k.get("correct", 0) / k["total"]) if k.get("total") else 0.0
    dp = k.get("depthPair") or {}
    depth = (1.0 if dp.get("ok") else 0.0) if dp.get("available") else frac  # fixture guarantees availability
    return g(0.6 * frac + 0.15 * bool(k.get("bgClear")) + 0.25 * depth,
             f"{k.get('correct')}/{k.get('total')} picks, bgClear={k.get('bgClear')}, "
             f"depthPair={'ok' if dp.get('ok') else dp.get('got')}",
             "clicking a bar does not identify the payment — the 3D view is not wired to the data")
```

Cheats: **always-same-id** → fails 4 of 5 targets and the background-clear. **2D-nearest picking without depth** → passes targets, fails the depth pair (probe clicks the projected top of an analytically occluded bar; the correct answer is the occluder — validated: app's independent CPU raycast agreed with the probe's expectation). The pinned fixture guarantees occluded tops exist (9 in validation), so `available` is deterministic.

### (4) `g_motion_perf` — animation truth + frame cadence

```python
@product_check("g_motion_perf", "G")
def _(c):
    p, pv = c.probe_viz_motion, c.probe_viz
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    az_ok = p.get("azDeltaDeg") is not None and p.get("expectedDeltaDeg") is not None \
        and abs(p["azDeltaDeg"] - p["expectedDeltaDeg"]) <= 4
    raf = p.get("raf") or {}
    cadence = _ladder(raf.get("p95"), [(1.0, 20), (0.75, 36), (0.5, 70), (0.25, 110)])
    reduce_ok = bool((pv or {}).get("staticUnderReduce"))
    score = 0.2 * bool(p.get("animating")) + 0.2 * az_ok + 0.2 * reduce_ok + 0.4 * cadence
    return g(score,
             f"animating={p.get('animating')} azΔ={p.get('azDeltaDeg')}° (want {p.get('expectedDeltaDeg')}±4) "
             f"raf p95={raf.get('p95')}ms reduced-motion-static={reduce_ok}",
             "animation is absent, frame-locked, or ignores prefers-reduced-motion")
```

`animating` requires pixel change over 700 ms (verified rAF runs ~60 Hz headless, fact #5). `az_ok` catches **per-frame animators** (`az += 0.1` per frame instead of dt-scaled) — optionally hardened with CDP CPU throttling ×4 during the window so frame-locked animation visibly drifts from 6°/s. `reduce_ok` comes from the `viz` scenario (static grid samples 700 ms apart under emulated reduce) — a genuinely hard finesse point models rarely ship. The cadence p95 ladder follows the existing quantized-quarters `_ladder` philosophy: wall-clock like the P tier, budgets **validated on the reference implementation during Bedrock calibration**, not invented. Cheat: an empty rAF loop that renders nothing → `animating` fails on pixels, not on rAF activity.

### (5) `g_fallback` — graceful degradation with WebGL removed

```python
@product_check("g_fallback", "G")
def _(c):
    p = c.probe_viz_fallback
    if _pe(p): return g(0, f"PROBE UNAVAILABLE: {_pe(p)}", "harness failure, not app evidence")
    errs = (p.get("consoleErrors") or {}).get("count", 99)
    content = bool(p.get("canvas2dPainted")) or (p.get("domFallbackElements") or 0) >= 3
    parts = {"no_errors": errs == 0, "fallback_content": content,
             "notice": bool(p.get("notice")), "table_alive": (p.get("renderedRowCount") or 0) > 0}
    return g(0.3*parts["no_errors"] + 0.4*parts["fallback_content"]
             + 0.2*parts["notice"] + 0.1*parts["table_alive"],
             f"errors={errs} painted2d={p.get('canvas2dPainted')} dom={p.get('domFallbackElements')} "
             f"notice={str(p.get('notice'))[:40]} rows={p.get('renderedRowCount')}",
             "a machine without WebGL gets a crash or a blank panel", parts=parts)
```

The `glKill` init script (verified: WebGL null, canvas-2D alive, OffscreenCanvas covered) simulates the no-WebGL machine with launch args unchanged. Cheats: **never-uses-WebGL-anywhere app** → passes here but zeroes `g_webgl_context` in `viz`, so no vacuous win; **blank canvas + notice** → loses the 0.4 content weight (`canvas2dPainted` requires non-uniform pixels via `getImageData`, sampled sparsely); **crash on null context** → `pageerror` feeds consoleErrors, 0.3 gone, and content is usually absent too.

---

## 7. Scorer wiring, controls, calibration

- **Gather**: after the existing product probes in `gather()` (app alive, post-sync so the scene has data):
  ```python
  os.environ.setdefault  # scorer builds BENCH_VIZ_EXPECT from fixtures: first 25 payments, chronological
  env_expect = json.dumps({"payments": fixtures.first_page_payments(25)})
  c.probe_viz          = _product_probe("viz", base)           # env BENCH_VIZ_EXPECT passed via _product_probe env
  c.probe_viz_motion   = _product_probe("viz-motion", base)
  c.probe_viz_fallback = _product_probe("viz-fallback", base)
  ```
  (`_product_probe` gains an `env=` passthrough for `BENCH_VIZ_EXPECT`.)
- **Weights**: new tier `G` in `PRODUCT_WEIGHT` — proposal `{"J": 0.10, "V": 0.07, "P": 0.04, "G": 0.09}` with `PRODUCT_CORE = 0.60` unchanged (0.60 + 0.30 + 0.10 hard = 1.00). Six G checks; per the tier philosophy G is where a great build separates: `g_motion_perf`'s reduce-honored point, `g_camera_drag`'s reversibility, and `g_picking`'s depth pair are deliberately hard. `SCORER_VERSION` bumps (the product gate precedent: a spec change IS a version change) — sb-6.
- **Controls (gates, not memory)**: the reference implementation (bars.html grown to the full contract) must score G ≈ high in the grader-controls HIGH gate; two seeded low-controls must score G low: a **mono-wash** app (one color, no binding) and a **readout-only drag** app (updates `#camera-readout`, never re-renders). The validation already demonstrated the discriminating case: the sign-flipped camera scored tops 1/13 and picks 0/5 while readout passed.
- **Calibration**: run the v3 spec through the existing `bench/calibrate.py` Bedrock entrants; set the cadence-ladder budgets and any fraction thresholds from the reference + cloud-model distribution, per the established "spec-documented budgets, never invented constants" rule. Expectation: cloud models will land the table half easily and bleed heavily on G (camera math, picking, reduce-motion, fallback) — which is precisely the decompression of the 0.95+ ceiling Mihai asked for.

## 8. Confidence statement

**High confidence (empirically verified, deterministic on this machine)**: SwiftShader determinism and launch args; the preserveDrawingBuffer trap and its instrumentation fix; CSS↔backing↔readPixels mapping; the full Node-math → occlusion-filter → sampler → pick → drag pipeline (facts #9–#10, bit-identical reruns); WebGL-kill fallback mechanics; reducedMotion emulation; rAF cadence viability.
**Lower confidence, flagged**: (1) cadence budgets under SwiftShader on other machines — must be calibrated, not asserted; (2) side-face samples can land on antialiased silhouette edges (measured 1/12, deterministic — the 0.75-threshold ladder absorbs it, but the reference-controls run must confirm the achievable fraction before thresholds freeze); (3) in-page sampling is theoretically tamperable by an app overriding `readPixels` — screenshot-clip cross-check (PNG via node zlib, screenshots verified deterministic) is the hardening if it ever matters; (4) the scenario glue in §5e composes verified pieces but has not run against a full spec-v3 app, because none exists yet — building the reference implementation and wiring the two low-controls is the necessary next step before any threshold is trusted.