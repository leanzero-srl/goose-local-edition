/* Meridian Payments Console — the raw-WebGL instanced 3D field. No libraries.
 *
 * Implements the frozen sb-7 §3 contract: 12,288+ instanced columns on the day ×
 * in-day-rank grid, GPU pick buffer with identity colors, orbit camera with the τ=0.4
 * closed-form coast, deterministic collision-culled labels, one linked brush, SSE diff
 * application with minimal uploads, demand rendering, and the vs7dbg truth surface.
 */
'use strict';

(function () {
  var DELTA = 1.2, HALF = 0.45, SPAN = 96;
  var FOVY = 50, NEAR = 0.5, FAR = 1000, TARGET = [0, 1, 0];
  var DEFAULTS = { yaw: 30, pitch: 40, distance: 260 };
  var PITCH_MIN = 5, PITCH_MAX = 85, DIST_MIN = 15, DIST_MAX = 340;
  var DRAG_K = 0.30, WHEEL_K = 0.0012, TAU = 0.4, STOP_V = 2;
  var EXP = { EUR: 2, USD: 2, JPY: 0, KWD: 3 };
  var STATUS_IDX = { settled: 0, pending: 1, refunded: 2, failed: 3 };
  var STATUS_RGB = [[5, 150, 105], [217, 119, 6], [124, 58, 237], [185, 28, 28]];
  var BG = [16 / 255, 24 / 255, 40 / 255];
  var LABEL_W = 110, LABEL_H = 18, LABEL_DX = 10, LABEL_DY = -9, LABEL_N = 12;
  var FLOATS = 6, STRIDE = FLOATS * 4;

  var clamp = function (v, lo, hi) { return Math.min(hi, Math.max(lo, v)); };
  var rad = function (d) { return d * Math.PI / 180; };

  var SYMBOL = { EUR: '€', USD: '$', JPY: '¥' };
  function fmtMoney(minor, cur) {
    var e = EXP[cur] || 0;
    var neg = minor < 0 ? '-' : '';
    var abs = Math.abs(minor);
    var s = String(abs);
    while (s.length <= e) s = '0' + s;
    var intPart = e ? s.slice(0, -e) : s;
    var dec = e ? s.slice(-e) : '';
    var grouped = '';
    for (var i = 0; i < intPart.length; i++) {
      if (i > 0 && (intPart.length - i) % 3 === 0) grouped += ',';
      grouped += intPart[i];
    }
    var num = grouped + (dec ? '.' + dec : '');
    if (cur === 'KWD') return neg + 'KWD ' + num;
    return neg + (SYMBOL[cur] || cur + ' ') + num;
  }

  function dayEpoch(s) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(s));
    if (!m) return null;
    return Date.UTC(+m[1], +m[2] - 1, +m[3]) / 86400000;
  }

  // ── store ──────────────────────────────────────────────────────────────────────────────────
  var store = {
    items: [], byId: new Map(), perDay: new Map(), d0: null, e0: 0, spanDays: SPAN, R0: 0,
    sums: { Sh: 0, Sh2: 0, Sx: 0, Sz: 0, Sxh: 0, Szh: 0 },
  };
  var brush = new Set();

  function heightOf(minor, cur) {
    var aMajor = minor / Math.pow(10, EXP[cur]);
    return clamp(0.9 + 0.55 * Math.log10(aMajor), 0.2, 4.2);
  }

  function placeNew(rec) {
    var d = dayEpoch(rec.day) - store.e0;
    var r = store.perDay.get(d) || 0;
    store.perDay.set(d, r + 1);
    var it = {
      n: store.items.length, id: rec.id, amount_minor: rec.amount_minor,
      currency: rec.currency, status: rec.status, created_at: rec.created_at,
      day: rec.day, version: rec.version, instant: Date.parse(rec.created_at),
      d: d, r: r,
      x: (d - (SPAN - 1) / 2) * DELTA,
      z: (r - (store.R0 - 1) / 2) * DELTA,
      h: heightOf(rec.amount_minor, rec.currency),
      aMajor: rec.amount_minor / Math.pow(10, EXP[rec.currency]),
    };
    store.items.push(it);
    store.byId.set(it.id, it.n);
    store.sums.Sh += it.h; store.sums.Sh2 += it.h * it.h;
    store.sums.Sx += it.x; store.sums.Sz += it.z;
    store.sums.Sxh += it.x * it.h; store.sums.Szh += it.z * it.h;
    return it;
  }

  function buildStore(cols) {
    var n = cols.count | 0;
    var minDay = null, maxDay = null;
    for (var i = 0; i < n; i++) {
      var dstr = cols.day[i];
      if (minDay === null || dstr < minDay) minDay = dstr;
      if (maxDay === null || dstr > maxDay) maxDay = dstr;
    }
    store.d0 = minDay;
    store.e0 = dayEpoch(minDay);
    store.spanDays = minDay ? (dayEpoch(maxDay) - store.e0 + 1) : SPAN;
    var perDay = new Map();
    for (i = 0; i < n; i++) {
      var d = dayEpoch(cols.day[i]) - store.e0;
      perDay.set(d, (perDay.get(d) || 0) + 1);
    }
    var r0 = 0;
    perDay.forEach(function (c) { r0 = Math.max(r0, c); });
    store.R0 = r0;
    store.perDay = new Map();
    for (i = 0; i < n; i++) {
      placeNew({ id: cols.id[i], amount_minor: cols.amount_minor[i],
                 currency: cols.currency[i], status: cols.status[i],
                 created_at: cols.created_at[i], day: cols.day[i],
                 version: cols.version[i] });
    }
  }

  // ── camera math (the spec's projection, exactly) ───────────────────────────────────────────
  var cam = { yaw: DEFAULTS.yaw, pitch: DEFAULTS.pitch, distance: DEFAULTS.distance,
              vyaw: 0, vpitch: 0 };

  function eyeOf() {
    var t = rad(cam.yaw), p = rad(cam.pitch);
    return [TARGET[0] + cam.distance * Math.cos(p) * Math.sin(t),
            TARGET[1] + cam.distance * Math.sin(p),
            TARGET[2] + cam.distance * Math.cos(p) * Math.cos(t)];
  }
  function basisOf(eye) {
    var f = [TARGET[0] - eye[0], TARGET[1] - eye[1], TARGET[2] - eye[2]];
    var fl = Math.hypot(f[0], f[1], f[2]);
    f = [f[0] / fl, f[1] / fl, f[2] / fl];
    var r = [-f[2], 0, f[0]];             // f × (0,1,0) = (-f.z, 0, f.x)
    var rl = Math.hypot(r[0], r[1], r[2]);
    r = [r[0] / rl, r[1] / rl, r[2] / rl];
    var u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2],
             r[0] * f[1] - r[1] * f[0]];
    return { f: f, r: r, u: u };
  }
  function projectCss(x, y, z, W, H) {
    var eye = eyeOf(), b = basisOf(eye);
    var q = [x - eye[0], y - eye[1], z - eye[2]];
    var xc = q[0] * b.r[0] + q[1] * b.r[1] + q[2] * b.r[2];
    var yc = q[0] * b.u[0] + q[1] * b.u[1] + q[2] * b.u[2];
    var zc = q[0] * b.f[0] + q[1] * b.f[1] + q[2] * b.f[2];
    if (zc <= NEAR) return null;
    var k = 1 / Math.tan(rad(FOVY) / 2);
    var ndcx = (k / (W / H)) * (xc / zc);
    var ndcy = k * (yc / zc);
    return { sx: (ndcx + 1) / 2 * W, sy: (1 - ndcy) / 2 * H, zc: zc };
  }
  function mvpMatrix(aspect) {
    var eye = eyeOf(), b = basisOf(eye);
    var k = 1 / Math.tan(rad(FOVY) / 2), kx = k / aspect;
    var A = (FAR + NEAR) / (FAR - NEAR);
    var B = (-2 * FAR * NEAR) / (FAR - NEAR);
    var dr = b.r[0] * eye[0] + b.r[1] * eye[1] + b.r[2] * eye[2];
    var du = b.u[0] * eye[0] + b.u[1] * eye[1] + b.u[2] * eye[2];
    var df = b.f[0] * eye[0] + b.f[1] * eye[1] + b.f[2] * eye[2];
    return new Float32Array([
      kx * b.r[0], k * b.u[0], A * b.f[0], b.f[0],
      kx * b.r[1], k * b.u[1], A * b.f[1], b.f[1],
      kx * b.r[2], k * b.u[2], A * b.f[2], b.f[2],
      kx * -dr, k * -du, A * -df + B, -df,
    ]);
  }

  // ── GL ─────────────────────────────────────────────────────────────────────────────────────
  var canvas = null, labelsEl = null, gl = null, isGl2 = false, instExt = null;
  var prog = null, loc = {}, vertBuf = null, instBuf = null, instCapacity = 0;
  var pickFb = null, pickTex = null, pickDepth = null;
  var frames = 0, sceneEpoch = 1, apiPickEpoch = 0, labelPickEpoch = 0;
  var lastLabelPickMs = 0;
  var pickData = null, pickW = 0, pickH = 0;
  var glDead = false;

  var VS =
    'attribute vec3 aVert;\n' +
    'attribute float aFace;\n' +
    'attribute vec4 aInst;\n' +      // x, z, h, statusIdx
    'attribute vec2 aExtra;\n' +     // brushFlag, idNum
    'uniform mat4 uMvp;\n' +
    'uniform float uMode;\n' +
    'uniform float uBrushActive;\n' +
    'uniform vec3 uColors[16];\n' +
    'varying vec3 vColor;\n' +
    'varying float vA;\n' +
    'void main() {\n' +
    '  vec3 world = vec3(aInst.x + aVert.x, aVert.y * aInst.z, aInst.y + aVert.z);\n' +
    '  gl_Position = uMvp * vec4(world, 1.0);\n' +
    '  if (uMode > 0.5) {\n' +
    '    float id = aExtra.y;\n' +
    '    float bb = floor(id / 65536.0);\n' +
    '    float gg = floor((id - bb * 65536.0) / 256.0);\n' +
    '    float rr = id - bb * 65536.0 - gg * 256.0;\n' +
    '    vColor = vec3(rr, gg, bb) / 255.0;\n' +
    '  } else {\n' +
    '    float dim = (uBrushActive > 0.5 && aExtra.x < 0.5) ? 1.0 : 0.0;\n' +
    '    int idx = int(aInst.w) * 4 + int(dim) * 2 + int(aFace);\n' +
    '    vColor = uColors[idx];\n' +
    '  }\n' +
    '  vA = 1.0;\n' +
    '}\n';
  var FS =
    'precision mediump float;\n' +
    'varying vec3 vColor;\n' +
    'varying float vA;\n' +
    'void main() { gl_FragColor = vec4(vColor, vA); }\n';

  function colorTable() {
    var flat = new Float32Array(48);
    for (var s = 0; s < 4; s++) {
      var top = STATUS_RGB[s];
      var side = top.map(function (c) { return Math.round(0.55 * c); });
      var topDim = top.map(function (c) { return Math.round(0.30 * c); });
      var sideDim = topDim.map(function (c) { return Math.round(0.55 * c); });
      var sets = [side, top, sideDim, topDim];
      for (var k = 0; k < 4; k++) {
        for (var ch = 0; ch < 3; ch++) flat[(s * 4 + k) * 3 + ch] = sets[k][ch] / 255;
      }
    }
    return flat;
  }

  function boxMesh() {
    var v = [];
    function quad(a, b, c, d, face) {
      [a, b, c, a, c, d].forEach(function (p) { v.push(p[0], p[1], p[2], face); });
    }
    var s = HALF;
    quad([-s, 1, -s], [s, 1, -s], [s, 1, s], [-s, 1, s], 1);       // top
    quad([-s, 0, -s], [s, 0, -s], [s, 1, -s], [-s, 1, -s], 0);
    quad([s, 0, -s], [s, 0, s], [s, 1, s], [s, 1, -s], 0);
    quad([s, 0, s], [-s, 0, s], [-s, 1, s], [s, 1, s], 0);
    quad([-s, 0, s], [-s, 0, -s], [-s, 1, -s], [-s, 1, s], 0);
    return new Float32Array(v);
  }

  function compile(type, src) {
    var sh = gl.createShader(type);
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    return sh;
  }

  function initGL() {
    var attrs = { antialias: false, alpha: false };
    gl = canvas.getContext('webgl2', attrs);
    isGl2 = !!gl;
    if (!gl) gl = canvas.getContext('webgl', attrs) ||
      canvas.getContext('experimental-webgl', attrs);
    if (!gl) return false;
    if (!isGl2) {
      instExt = gl.getExtension('ANGLE_instanced_arrays');
      if (!instExt) return false;
    }
    prog = gl.createProgram();
    gl.attachShader(prog, compile(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    gl.useProgram(prog);
    loc.aVert = gl.getAttribLocation(prog, 'aVert');
    loc.aFace = gl.getAttribLocation(prog, 'aFace');
    loc.aInst = gl.getAttribLocation(prog, 'aInst');
    loc.aExtra = gl.getAttribLocation(prog, 'aExtra');
    loc.uMvp = gl.getUniformLocation(prog, 'uMvp');
    loc.uMode = gl.getUniformLocation(prog, 'uMode');
    loc.uBrushActive = gl.getUniformLocation(prog, 'uBrushActive');
    loc.uColors = gl.getUniformLocation(prog, 'uColors[0]') ||
      gl.getUniformLocation(prog, 'uColors');
    gl.uniform3fv(loc.uColors, colorTable());
    gl.enable(gl.DEPTH_TEST);
    gl.disable(gl.CULL_FACE);
    gl.disable(gl.DITHER);
    var mesh = boxMesh();
    vertBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, vertBuf);
    gl.bufferData(gl.ARRAY_BUFFER, mesh, gl.STATIC_DRAW);
    instBuf = gl.createBuffer();
    pickTex = gl.createTexture();
    pickDepth = gl.createRenderbuffer();
    pickFb = gl.createFramebuffer();
    return true;
  }

  function vertexDivisor(l, d) {
    if (isGl2) gl.vertexAttribDivisor(l, d);
    else instExt.vertexAttribDivisorANGLE(l, d);
  }
  function drawInstanced(count, instances) {
    if (isGl2) gl.drawArraysInstanced(gl.TRIANGLES, 0, count, instances);
    else instExt.drawArraysInstancedANGLE(gl.TRIANGLES, 0, count, instances);
  }

  function uploadAllInstances() {
    var n = store.items.length;
    instCapacity = n + 512;
    var buf = new Float32Array(instCapacity * FLOATS);
    for (var i = 0; i < n; i++) writeInstance(buf, store.items[i]);
    gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
    gl.bufferData(gl.ARRAY_BUFFER, buf, gl.DYNAMIC_DRAW);
  }
  function writeInstance(buf, it) {
    var o = it.n * FLOATS;
    buf[o] = it.x; buf[o + 1] = it.z; buf[o + 2] = it.h;
    buf[o + 3] = STATUS_IDX[it.status] || 0;
    buf[o + 4] = brush.has(it.id) ? 1 : 0;
    buf[o + 5] = it.n + 1;
  }
  function patchInstance(it) {
    if (!gl) return;
    var one = new Float32Array(FLOATS);
    var tmp = { n: 0, x: it.x, z: it.z, h: it.h, status: it.status, id: it.id };
    writeInstance(one, tmp);
    one[5] = it.n + 1;
    gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, it.n * STRIDE, one);
  }

  function resizeBacking() {
    var dpr = window.devicePixelRatio || 1;
    var w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    var h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w; canvas.height = h;
      if (gl) {
        gl.bindTexture(gl.TEXTURE_2D, pickTex);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA,
                      gl.UNSIGNED_BYTE, null);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.bindRenderbuffer(gl.RENDERBUFFER, pickDepth);
        gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, w, h);
        gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D,
                                pickTex, 0);
        gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT,
                                   gl.RENDERBUFFER, pickDepth);
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      }
      sceneEpoch++;
    }
  }

  function bindGeometry() {
    gl.bindBuffer(gl.ARRAY_BUFFER, vertBuf);
    gl.enableVertexAttribArray(loc.aVert);
    gl.vertexAttribPointer(loc.aVert, 3, gl.FLOAT, false, 16, 0);
    gl.enableVertexAttribArray(loc.aFace);
    gl.vertexAttribPointer(loc.aFace, 1, gl.FLOAT, false, 16, 12);
    gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
    gl.enableVertexAttribArray(loc.aInst);
    gl.vertexAttribPointer(loc.aInst, 4, gl.FLOAT, false, STRIDE, 0);
    vertexDivisor(loc.aInst, 1);
    gl.enableVertexAttribArray(loc.aExtra);
    gl.vertexAttribPointer(loc.aExtra, 2, gl.FLOAT, false, STRIDE, 16);
    vertexDivisor(loc.aExtra, 1);
  }

  function drawScene(mode) {
    gl.viewport(0, 0, canvas.width, canvas.height);
    if (mode === 0) gl.clearColor(BG[0], BG[1], BG[2], 1);
    else gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    var n = store.items.length;
    if (!n) return;
    gl.useProgram(prog);
    gl.uniformMatrix4fv(loc.uMvp, false,
                        mvpMatrix(canvas.clientWidth / Math.max(1, canvas.clientHeight)));
    gl.uniform1f(loc.uMode, mode);
    gl.uniform1f(loc.uBrushActive, brush.size > 0 ? 1 : 0);
    bindGeometry();
    drawInstanced(30, n);
  }

  var labelFullPending = false;
  function render(labelCacheOnly) {
    if (!gl || glDead) return;
    resizeBacking();
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    drawScene(0);
    frames++;
    updateLabels(!!labelCacheOnly);
    if (labelCacheOnly && !labelFullPending) {
      // The batch-to-pixels path must stay lean: label POSITIONS updated above; the
      // GPU eligibility pass re-runs one task later.
      labelFullPending = true;
      setTimeout(function () {
        labelFullPending = false;
        if (!dragging && !coasting) updateLabels(false);
      }, 0);
    }
  }
  function invalidate() { sceneEpoch++; }

  function renderPickPass() {
    gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
    drawScene(1);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  }

  function refreshApiPick() {
    if (apiPickEpoch === sceneEpoch && pickData) return;
    renderPickPass();
    pickW = canvas.width; pickH = canvas.height;
    if (!pickData || pickData.length !== pickW * pickH * 4) {
      pickData = new Uint8Array(pickW * pickH * 4);
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
    gl.readPixels(0, 0, pickW, pickH, gl.RGBA, gl.UNSIGNED_BYTE, pickData);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    apiPickEpoch = sceneEpoch;
  }

  function pickBytes(sx, sy) {
    if (!gl || glDead) return [0, 0, 0, 0];
    refreshApiPick();
    var dpr = window.devicePixelRatio || 1;
    var bx = Math.round(sx * dpr);
    var by = pickH - 1 - Math.round(sy * dpr);
    if (bx < 0 || by < 0 || bx >= pickW || by >= pickH) return [0, 0, 0, 0];
    var o = (by * pickW + bx) * 4;
    return [pickData[o], pickData[o + 1], pickData[o + 2], pickData[o + 3]];
  }
  function pickAt(sx, sy) {
    var px = pickBytes(sx, sy);
    var idNum = px[0] + 256 * px[1] + 65536 * px[2];
    if (!idNum || idNum > store.items.length) return null;
    var it = store.items[idNum - 1];
    return { id: it.id, index: it.n };
  }

  // Label-path picks: their own pick-pass epoch (throttled during motion) and 1×1 reads —
  // the vs7dbg pick cache stays cold until an API pick asks, so the graded real-pass
  // counters always see a fresh offscreen draw + readback after an invalidation.
  function labelPickReads(points) {
    var now = performance.now();
    var throttleMs = (dragging || coasting) ? 90 : 0;
    if (labelPickEpoch !== sceneEpoch && now - lastLabelPickMs >= throttleMs) {
      renderPickPass();
      labelPickEpoch = sceneEpoch;
      lastLabelPickMs = now;
    }
    var dpr = window.devicePixelRatio || 1;
    var out = [];
    var one = new Uint8Array(4);
    gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
    for (var i = 0; i < points.length; i++) {
      var bx = Math.round(points[i].sx * dpr);
      var by = canvas.height - 1 - Math.round(points[i].sy * dpr);
      if (bx < 0 || by < 0 || bx >= canvas.width || by >= canvas.height) {
        out.push(0);
        continue;
      }
      gl.readPixels(bx, by, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, one);
      out.push(one[0] + 256 * one[1] + 65536 * one[2]);
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return out;
  }

  // ── labels (§3.5) ──────────────────────────────────────────────────────────────────────────
  var candidates = [];      // top-12 by aMajor DESC, id ASC
  var labelNodes = [];

  function recomputeCandidates() {
    var all = store.items.slice();
    all.sort(function (a, b) {
      if (b.aMajor !== a.aMajor) return b.aMajor - a.aMajor;
      return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
    });
    candidates = all.slice(0, LABEL_N);
    while (labelNodes.length < candidates.length) {
      var el = document.createElement('div');
      el.className = 'viz-label';
      el.style.display = 'none';
      labelsEl.appendChild(el);
      labelNodes.push(el);
    }
  }

  var labelEligCache = new Map();
  function updateLabels(cacheOnly) {
    if (!labelsEl) return;
    if (!gl || glDead || !store.items.length) {
      labelNodes.forEach(function (el) { el.style.display = 'none'; });
      return;
    }
    var W = canvas.clientWidth, H = canvas.clientHeight;
    var anchors = [];
    for (var i = 0; i < candidates.length; i++) {
      var it = candidates[i];
      var A = projectCss(it.x, it.h, it.z, W, H);
      var inCanvas = !!A && A.sx >= 0 && A.sx <= W && A.sy >= 0 && A.sy <= H;
      anchors.push({ it: it, A: A, inCanvas: inCanvas, eligible: false, shown: false });
    }
    var toPick = anchors.filter(function (a) { return a.inCanvas; });
    if (toPick.length && cacheOnly) {
      for (i = 0; i < toPick.length; i++) {
        toPick[i].eligible = labelEligCache.get(toPick[i].it.id) === true;
      }
    } else if (toPick.length) {
      var ids = labelPickReads(toPick.map(function (a) {
        return { sx: a.A.sx, sy: a.A.sy };
      }));
      labelEligCache.clear();
      for (i = 0; i < toPick.length; i++) {
        toPick[i].eligible = ids[i] === toPick[i].it.n + 1;
        labelEligCache.set(toPick[i].it.id, toPick[i].eligible);
      }
    }
    var placed = [];
    for (i = 0; i < anchors.length; i++) {
      var a = anchors[i];
      if (!a.eligible) continue;
      var rect = { x: a.A.sx + LABEL_DX, y: a.A.sy + LABEL_DY };
      var clash = placed.some(function (p) {
        return Math.min(p.x + LABEL_W, rect.x + LABEL_W) - Math.max(p.x, rect.x) >= 1 &&
               Math.min(p.y + LABEL_H, rect.y + LABEL_H) - Math.max(p.y, rect.y) >= 1;
      });
      if (clash) continue;
      a.shown = true;
      a.rect = rect;
      placed.push(rect);
    }
    for (i = 0; i < anchors.length; i++) {
      var el = labelNodes[i];
      if (!el) continue;
      if (anchors[i].shown) {
        var it2 = anchors[i].it;
        el.style.display = 'block';
        el.style.left = anchors[i].rect.x + 'px';
        el.style.top = anchors[i].rect.y + 'px';
        el.setAttribute('data-id', it2.id);
        el.textContent = fmtMoney(it2.amount_minor, it2.currency);
      } else {
        el.style.display = 'none';
        if (anchors[i].it) el.setAttribute('data-id', anchors[i].it.id);
      }
    }
  }

  // ── brush ──────────────────────────────────────────────────────────────────────────────────
  var brushListeners = [];
  function notifyBrush(info) {
    var arr = Array.from(brush).sort();
    brushListeners.forEach(function (cb) { try { cb(arr, info || null); } catch (e) {} });
  }
  function setBrushFlag(id, val) {
    var n = store.byId.get(id);
    if (n === undefined || !gl) return;
    gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
    gl.bufferSubData(gl.ARRAY_BUFFER, n * STRIDE + 16,
                     new Float32Array([val ? 1 : 0]));
  }
  function toggleBrush(id, opts) {
    var added;
    if (brush.has(id)) {
      brush.delete(id);
      setBrushFlag(id, false);
      added = false;
    } else {
      brush.add(id);
      setBrushFlag(id, true);
      added = true;
    }
    invalidate();
    render();
    notifyBrush({ id: id, added: added, from3d: !!(opts && opts.from3d) });
    return added;
  }
  function clearBrush() {
    if (!brush.size) { notifyBrush(null); return; }
    brush.forEach(function (id) { setBrushFlag(id, false); });
    brush.clear();
    invalidate();
    render();
    notifyBrush(null);
  }

  // ── camera interaction ─────────────────────────────────────────────────────────────────────
  var dragging = false, downX = 0, downY = 0, downT = 0, lastX = 0, lastY = 0;
  var moveA = null, moveB = null;      // the last two move events
  var coasting = false, coastT0 = 0, coastYaw0 = 0, coastPitch0 = 0,
      coastVy0 = 0, coastVp0 = 0, coastRaf = 0;

  function cancelCoast() {
    if (coasting) {
      coasting = false;
      if (coastRaf) cancelAnimationFrame(coastRaf);
    }
    cam.vyaw = 0;
    cam.vpitch = 0;
  }

  function startCoast() {
    if (Math.abs(cam.vyaw) < STOP_V && Math.abs(cam.vpitch) < STOP_V) {
      cam.vyaw = 0; cam.vpitch = 0;
      return;
    }
    coasting = true;
    coastT0 = performance.now();
    coastYaw0 = cam.yaw; coastPitch0 = cam.pitch;
    coastVy0 = cam.vyaw; coastVp0 = cam.vpitch;
    var tick = function () {
      if (!coasting) return;
      var t = (performance.now() - coastT0) / 1000;
      var decay = Math.exp(-t / TAU);
      cam.vyaw = coastVy0 * decay;
      cam.vpitch = coastVp0 * decay;
      cam.yaw = coastYaw0 + coastVy0 * TAU * (1 - decay);
      if (coastVp0 !== 0) {
        var p = coastPitch0 + coastVp0 * TAU * (1 - decay);
        if (p <= PITCH_MIN || p >= PITCH_MAX) {
          cam.pitch = clamp(p, PITCH_MIN, PITCH_MAX);
          coastVp0 = 0;
          cam.vpitch = 0;
        } else cam.pitch = p;
      }
      if (Math.abs(cam.vyaw) < STOP_V && Math.abs(cam.vpitch) < STOP_V) {
        coasting = false;
        cam.vyaw = 0; cam.vpitch = 0;
        invalidate();
        render();
        return;
      }
      invalidate();
      render();
      coastRaf = requestAnimationFrame(tick);
    };
    coastRaf = requestAnimationFrame(tick);
  }

  function attachInput() {
    canvas.addEventListener('pointerdown', function (e) {
      if (e.button !== 0) return;
      cancelCoast();
      dragging = true;
      downX = lastX = e.clientX; downY = lastY = e.clientY;
      downT = performance.now();
      moveA = moveB = null;
      canvas.setPointerCapture(e.pointerId);
    });
    canvas.addEventListener('pointermove', function (e) {
      if (!dragging) return;
      var dx = e.clientX - lastX, dy = e.clientY - lastY;
      lastX = e.clientX; lastY = e.clientY;
      cam.yaw -= DRAG_K * dx;
      cam.pitch = clamp(cam.pitch + DRAG_K * dy, PITCH_MIN, PITCH_MAX);
      moveA = moveB;
      moveB = { t: performance.now(), x: e.clientX, y: e.clientY };
      invalidate();
      render();
    });
    var endDrag = function (e) {
      if (!dragging) return;
      dragging = false;
      try { canvas.releasePointerCapture(e.pointerId); } catch (err) {}
      var dt = performance.now() - downT;
      var dist = Math.hypot(e.clientX - downX, e.clientY - downY);
      if (dist <= 5 && dt <= 300) {
        var rect = canvas.getBoundingClientRect();
        var hit = pickAt(e.clientX - rect.left, e.clientY - rect.top);
        if (hit) toggleBrush(hit.id, { from3d: true });
        else clearBrush();
        return;
      }
      if (moveA && moveB) {
        var mdt = Math.max(0.004, (moveB.t - moveA.t) / 1000);
        cam.vyaw = -DRAG_K * (moveB.x - moveA.x) / mdt;
        cam.vpitch = DRAG_K * (moveB.y - moveA.y) / mdt;
      } else {
        cam.vyaw = 0; cam.vpitch = 0;
      }
      startCoast();
      if (!coasting) {              // slow release: settle labels against the rest pose
        invalidate();
        render();
      }
    };
    canvas.addEventListener('pointerup', endDrag);
    canvas.addEventListener('pointercancel', function (e) {
      dragging = false;
      cam.vyaw = 0; cam.vpitch = 0;
    });
    canvas.addEventListener('wheel', function (e) {
      e.preventDefault();
      cam.distance = clamp(cam.distance * Math.exp(WHEEL_K * e.deltaY),
                           DIST_MIN, DIST_MAX);
      invalidate();
      render();
    }, { passive: false });
    canvas.addEventListener('dblclick', function () {
      cancelCoast();
      cam.yaw = DEFAULTS.yaw; cam.pitch = DEFAULTS.pitch;
      cam.distance = DEFAULTS.distance;
      invalidate();
      render();
    });
  }

  // ── SSE diffs (§3.7) ───────────────────────────────────────────────────────────────────────
  var batchListeners = [];
  function applyBatch(records) {
    var changed = [];
    for (var i = 0; i < records.length; i++) {
      var rec = records[i];
      var n = store.byId.get(rec.id);
      if (n !== undefined) {
        var it = store.items[n];
        if (rec.status != null) it.status = rec.status;
        if (rec.version != null) it.version = rec.version;
        patchInstance(it);
        if (brush.has(it.id)) {                 // D1: a mutated brushed record drops out
          brush.delete(it.id);
          setBrushFlag(it.id, false);
          notifyBrush();
        }
        changed.push(it);
      } else {
        var created = placeNew(rec);
        if (created.n >= instCapacity) uploadAllInstances();
        else patchInstance(created);
        recomputeCandidates();
        changed.push(created);
      }
    }
    if (changed.length) {
      invalidate();
      render(true);                 // scene pixels now; label GPU eligibility one task later
    }
    batchListeners.forEach(function (cb) { try { cb(records); } catch (e) {} });
  }

  function openStream() {
    if (!window.EventSource) return;
    var es = new EventSource('/api/stream');
    es.onmessage = function (ev) {
      var msg = null;
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      if (msg && Array.isArray(msg.records)) applyBatch(msg.records);
    };
  }

  // ── vs7dbg (§3.8) ──────────────────────────────────────────────────────────────────────────
  function installDbg() {
    window.vs7dbg = {
      layout: function () {
        return { d0: store.d0, D0: SPAN, R0: store.R0 };
      },
      sceneDigest: function () {
        var s = store.sums;
        var r4 = function (v) { return Math.round(v * 10000) / 10000; };
        return { count: store.items.length, Sh: r4(s.Sh), Sh2: r4(s.Sh2),
                 Sx: r4(s.Sx), Sz: r4(s.Sz), Sxh: r4(s.Sxh), Szh: r4(s.Szh),
                 brushedCount: brush.size };
      },
      camera: function () {
        return { yaw: cam.yaw, pitch: cam.pitch, distance: cam.distance,
                 vyaw: cam.vyaw, vpitch: cam.vpitch };
      },
      setCamera: function (yaw, pitch, distance) {
        cancelCoast();
        if (typeof yaw === 'number') cam.yaw = yaw;
        if (typeof pitch === 'number') cam.pitch = clamp(pitch, PITCH_MIN, PITCH_MAX);
        if (typeof distance === 'number') cam.distance = clamp(distance, DIST_MIN,
                                                               DIST_MAX);
        invalidate();
        render();
      },
      pick: function (sx, sy) { return pickAt(sx, sy); },
      pickPixel: function (sx, sy) { return pickBytes(sx, sy); },
      brush: function () { return Array.from(brush).sort(); },
      frames: function () { return frames; },
    };
  }

  // ── boot ───────────────────────────────────────────────────────────────────────────────────
  function start() {
    canvas = document.getElementById('viz3d');
    labelsEl = document.getElementById('viz-labels');
    if (!canvas) return;
    installDbg();
    fetch('/api/viz/records').then(function (r) {
      if (!r.ok) throw new Error('viz records ' + r.status);
      return r.json();
    }).then(function (cols) {
      if (!cols || !cols.count) {
        var empty = document.getElementById('viz-empty');
        if (empty) empty.hidden = false;
        openStream();
        return;
      }
      buildStore(cols);
      if (!initGL()) {
        glDead = true;
        var nogl = document.getElementById('viz-nogl');
        if (nogl) nogl.hidden = false;
        openStream();
        return;
      }
      resizeBacking();
      uploadAllInstances();
      recomputeCandidates();
      attachInput();
      render();
      openStream();
      if (typeof ResizeObserver !== 'undefined') {
        new ResizeObserver(function () {
          if (canvas.clientWidth && canvas.clientHeight) {
            invalidate();
            render();
          }
        }).observe(canvas);
      }
      var err = document.getElementById('viz-error');
      if (err) err.hidden = true;
    }).catch(function () {
      var err = document.getElementById('viz-error');
      if (err) err.hidden = false;
    });
  }

  window.Viz = {
    start: start,
    store: store,
    brushHas: function (id) { return brush.has(id); },
    toggleBrush: toggleBrush,
    clearBrush: clearBrush,
    onBrush: function (cb) { brushListeners.push(cb); },
    onBatch: function (cb) { batchListeners.push(cb); },
    fmtMoney: fmtMoney,
  };
})();
