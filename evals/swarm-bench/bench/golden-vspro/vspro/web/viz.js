/* VendorSync Pro — raw-WebGL 3D bucket bar chart.
 *
 * Implements the frozen sb-6 scene/camera/interaction contract exactly: bar grid from
 * /api/buckets, orbit camera (yaw 35 / pitch 27 / distance 30, target (0,3,0), fovY 50),
 * flat unlit colors (top = status hex, sides = round(0.62 x top), clear #0F172A), depth-tested
 * occlusion, and NOTHING but the bars — no floor, grid, axes, or in-canvas labels.
 * Picking is GPU truth: an offscreen color-id framebuffer read at the cursor, so a partially
 * occluded bar loses to the bar in front exactly as the depth buffer says.
 * Rendering is static between inputs; while a pointer drag is captured the scene renders on
 * requestAnimationFrame so frames() advances at display cadence.
 */
'use strict';

(function () {
  const STATUS_ORDER = ['settled', 'pending', 'refunded', 'failed'];
  const STATUS_RGB = {
    settled: [22, 163, 74],
    pending: [245, 158, 11],
    refunded: [139, 92, 246],
    failed: [220, 38, 38],
  };
  const BG = [15 / 255, 23 / 255, 42 / 255];
  const DEFAULTS = { yaw: 35, pitch: 27, distance: 30 };
  const TARGET = [0, 3, 0];
  const FOVY = 50;
  const NEAR = 0.1;
  const FAR = 200;
  const CELL_PITCH = 1.5;
  const HALF = 0.5;
  const H_PER_COUNT = 0.25;
  const DRAG_DEG_PER_PX = 0.35;
  const WHEEL_K = 0.0012;
  const PITCH_MIN = 5, PITCH_MAX = 85, DIST_MIN = 10, DIST_MAX = 90;

  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  const rad = (d) => (d * Math.PI) / 180;
  const sideChannel = (c) => Math.round(c * 0.62);

  const VS = [
    'attribute vec3 aPos;',
    'attribute vec3 aColor;',
    'uniform mat4 uMvp;',
    'varying vec3 vColor;',
    'void main() {',
    '  vColor = aColor;',
    '  gl_Position = uMvp * vec4(aPos, 1.0);',
    '}',
  ].join('\n');
  const FS = [
    'precision mediump float;',
    'varying vec3 vColor;',
    'void main() { gl_FragColor = vec4(vColor, 1.0); }',
  ].join('\n');

  function cameraEye(yaw, pitch, distance) {
    const t = rad(yaw), p = rad(pitch);
    return [
      TARGET[0] + distance * Math.cos(p) * Math.sin(t),
      TARGET[1] + distance * Math.sin(p),
      TARGET[2] + distance * Math.cos(p) * Math.cos(t),
    ];
  }

  function cameraBasis(eye) {
    const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    const norm = (a) => {
      const l = Math.hypot(a[0], a[1], a[2]);
      return [a[0] / l, a[1] / l, a[2] / l];
    };
    const cross = (a, b) => [
      a[1] * b[2] - a[2] * b[1],
      a[2] * b[0] - a[0] * b[2],
      a[0] * b[1] - a[1] * b[0],
    ];
    const f = norm(sub(TARGET, eye));
    const r = norm(cross(f, [0, 1, 0]));
    const u = cross(r, f);
    return { f, r, u };
  }

  // MVP = P * V, column-major, matching the spec's projection formulas exactly:
  // clip = ((k/aspect)*xc, k*yc, ((far+near)*zc - 2*far*near)/(far-near), zc).
  function mvpMatrix(yaw, pitch, distance, aspect) {
    const eye = cameraEye(yaw, pitch, distance);
    const { f, r, u } = cameraBasis(eye);
    const k = 1 / Math.tan(rad(FOVY) / 2);
    const kx = k / aspect;
    const A = (FAR + NEAR) / (FAR - NEAR);
    const B = (-2 * FAR * NEAR) / (FAR - NEAR);
    const dr = r[0] * eye[0] + r[1] * eye[1] + r[2] * eye[2];
    const du = u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2];
    const df = f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2];
    // Row i of V: basis row with translation -dot(basis, eye); P rows as above.
    // Stored column-major: out[col*4+row].
    return new Float32Array([
      kx * r[0], k * u[0], A * f[0], f[0],
      kx * r[1], k * u[1], A * f[1], f[1],
      kx * r[2], k * u[2], A * f[2], f[2],
      kx * -dr, k * -du, A * -df + B, -df,
    ]);
  }

  function barsFromBuckets(buckets) {
    const days = buckets.days || [];
    const D = days.length;
    const bars = [];
    for (const cell of buckets.cells || []) {
      if (!cell.count) continue;              // zero-count cells draw nothing, pick nothing
      const i = days.indexOf(cell.day);
      const j = STATUS_ORDER.indexOf(cell.status);
      if (i < 0 || j < 0) continue;
      bars.push({
        key: cell.day + '|' + cell.status,
        day: cell.day,
        status: cell.status,
        count: cell.count,
        i, j,
        x: (i - (D - 1) / 2) * CELL_PITCH,
        z: (j - 1.5) * CELL_PITCH,
        h: cell.count * H_PER_COUNT,
      });
    }
    return bars;
  }

  function mount(canvas, opts) {
    const tooltip = opts.tooltipEl;
    const onPickStatus = opts.onPickStatus || function () {};
    const formatDay = opts.formatDay || ((d) => d);

    let yaw = DEFAULTS.yaw, pitch = DEFAULTS.pitch, distance = DEFAULTS.distance;
    let buckets = { days: [], statuses: STATUS_ORDER.slice(), cells: [] };
    let bars = [];
    let frameCount = 0;
    let vertexCount = 0;
    let pickDirty = true;

    const gl = canvas.getContext('webgl2', { antialias: false, alpha: false }) ||
      canvas.getContext('webgl', { antialias: false, alpha: false }) ||
      canvas.getContext('experimental-webgl', { antialias: false, alpha: false });

    let program = null, aPos = -1, aColor = -1, uMvp = null;
    let posBuf = null, colorBuf = null, pickColorBuf = null;
    let pickFb = null, pickTex = null, pickDepth = null;

    function compile(type, src) {
      const sh = gl.createShader(type);
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      return sh;
    }

    if (gl) {
      program = gl.createProgram();
      gl.attachShader(program, compile(gl.VERTEX_SHADER, VS));
      gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FS));
      gl.linkProgram(program);
      gl.useProgram(program);
      aPos = gl.getAttribLocation(program, 'aPos');
      aColor = gl.getAttribLocation(program, 'aColor');
      uMvp = gl.getUniformLocation(program, 'uMvp');
      posBuf = gl.createBuffer();
      colorBuf = gl.createBuffer();
      pickColorBuf = gl.createBuffer();
      gl.enable(gl.DEPTH_TEST);
      gl.disable(gl.DITHER);
      gl.disable(gl.CULL_FACE);
      pickTex = gl.createTexture();
      pickDepth = gl.createRenderbuffer();
      pickFb = gl.createFramebuffer();
    }

    function cssSize() {
      return { w: canvas.clientWidth || 1, h: canvas.clientHeight || 1 };
    }

    function resizeBacking() {
      const dpr = window.devicePixelRatio || 1;
      const { w, h } = cssSize();
      const bw = Math.max(1, Math.round(w * dpr));
      const bh = Math.max(1, Math.round(h * dpr));
      if (canvas.width !== bw || canvas.height !== bh) {
        canvas.width = bw;
        canvas.height = bh;
        if (gl) {
          gl.bindTexture(gl.TEXTURE_2D, pickTex);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, bw, bh, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
          gl.bindRenderbuffer(gl.RENDERBUFFER, pickDepth);
          gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, bw, bh);
          gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
          gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, pickTex, 0);
          gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, pickDepth);
          gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        }
        pickDirty = true;
      }
    }

    function rebuildGeometry() {
      bars = barsFromBuckets(buckets);
      if (!gl) return;
      const pos = [], col = [], pcol = [];
      const pushQuad = (a, b, c, d, rgb, out, pickRgb) => {
        for (const v of [a, b, c, a, c, d]) pos.push(v[0], v[1], v[2]);
        for (let n = 0; n < 6; n++) {
          out.push(rgb[0] / 255, rgb[1] / 255, rgb[2] / 255);
          pcol.push(pickRgb[0] / 255, pickRgb[1] / 255, pickRgb[2] / 255);
        }
      };
      bars.forEach((bar, index) => {
        const id = index + 1;
        const pickRgb = [(id >> 16) & 255, (id >> 8) & 255, id & 255];
        const top = STATUS_RGB[bar.status];
        const side = top.map(sideChannel);
        const x0 = bar.x - HALF, x1 = bar.x + HALF;
        const z0 = bar.z - HALF, z1 = bar.z + HALF;
        const h = bar.h;
        pushQuad([x0, h, z0], [x1, h, z0], [x1, h, z1], [x0, h, z1], top, col, pickRgb);
        pushQuad([x0, 0, z0], [x1, 0, z0], [x1, h, z0], [x0, h, z0], side, col, pickRgb);
        pushQuad([x1, 0, z0], [x1, 0, z1], [x1, h, z1], [x1, h, z0], side, col, pickRgb);
        pushQuad([x1, 0, z1], [x0, 0, z1], [x0, h, z1], [x1, h, z1], side, col, pickRgb);
        pushQuad([x0, 0, z1], [x0, 0, z0], [x0, h, z0], [x0, h, z1], side, col, pickRgb);
      });
      vertexCount = pos.length / 3;
      gl.bindBuffer(gl.ARRAY_BUFFER, posBuf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(pos), gl.STATIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, colorBuf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(col), gl.STATIC_DRAW);
      gl.bindBuffer(gl.ARRAY_BUFFER, pickColorBuf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(pcol), gl.STATIC_DRAW);
      pickDirty = true;
    }

    function drawScene(colors, clearColor) {
      const { w, h } = cssSize();
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.clearColor(clearColor[0], clearColor[1], clearColor[2], 1);
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
      if (!vertexCount) return;
      gl.useProgram(program);
      gl.uniformMatrix4fv(uMvp, false, mvpMatrix(yaw, pitch, distance, w / h));
      gl.bindBuffer(gl.ARRAY_BUFFER, posBuf);
      gl.enableVertexAttribArray(aPos);
      gl.vertexAttribPointer(aPos, 3, gl.FLOAT, false, 0, 0);
      gl.bindBuffer(gl.ARRAY_BUFFER, colors);
      gl.enableVertexAttribArray(aColor);
      gl.vertexAttribPointer(aColor, 3, gl.FLOAT, false, 0, 0);
      gl.drawArrays(gl.TRIANGLES, 0, vertexCount);
    }

    function render() {
      if (!gl) return;
      resizeBacking();
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      drawScene(colorBuf, BG);
      frameCount++;
      pickDirty = true;
    }

    function renderPickBuffer() {
      gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
      drawScene(pickColorBuf, [0, 0, 0]);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      pickDirty = false;
    }

    function project(x, y, z) {
      const eye = cameraEye(yaw, pitch, distance);
      const { f, r, u } = cameraBasis(eye);
      const q = [x - eye[0], y - eye[1], z - eye[2]];
      const xc = q[0] * r[0] + q[1] * r[1] + q[2] * r[2];
      const yc = q[0] * u[0] + q[1] * u[1] + q[2] * u[2];
      const zc = q[0] * f[0] + q[1] * f[1] + q[2] * f[2];
      if (zc <= 0.1) return null;
      const { w, h } = cssSize();
      const k = 1 / Math.tan(rad(FOVY) / 2);
      const ndcx = (k / (w / h)) * (xc / zc);
      const ndcy = k * (yc / zc);
      return [((ndcx + 1) / 2) * w, ((1 - ndcy) / 2) * h];
    }

    // Analytic ray-AABB picking: the no-WebGL fallback for vsdbg.pick, and the cross-check
    // for the GPU path. Nearest entry distance along the eye ray is exactly what the depth
    // buffer resolves.
    function pickAnalytic(sx, sy) {
      const { w, h } = cssSize();
      const eye = cameraEye(yaw, pitch, distance);
      const { f, r, u } = cameraBasis(eye);
      const k = 1 / Math.tan(rad(FOVY) / 2);
      const ndcx = (sx / w) * 2 - 1;
      const ndcy = 1 - (sy / h) * 2;
      const cx = (ndcx / (k / (w / h)));
      const cy = ndcy / k;
      const dir = [
        f[0] + cx * r[0] + cy * u[0],
        f[1] + cx * r[1] + cy * u[1],
        f[2] + cx * r[2] + cy * u[2],
      ];
      let bestKey = null, bestT = Infinity;
      for (const bar of bars) {
        const mn = [bar.x - HALF, 0, bar.z - HALF];
        const mx = [bar.x + HALF, bar.h, bar.z + HALF];
        let t0 = -Infinity, t1 = Infinity, hit = true;
        for (let axis = 0; axis < 3; axis++) {
          if (Math.abs(dir[axis]) < 1e-9) {
            if (eye[axis] < mn[axis] || eye[axis] > mx[axis]) { hit = false; break; }
            continue;
          }
          let a = (mn[axis] - eye[axis]) / dir[axis];
          let b = (mx[axis] - eye[axis]) / dir[axis];
          if (a > b) { const tmp = a; a = b; b = tmp; }
          t0 = Math.max(t0, a);
          t1 = Math.min(t1, b);
        }
        if (hit && t0 <= t1 && t1 > 0 && Math.max(t0, 0) < bestT) {
          bestT = Math.max(t0, 0);
          bestKey = bar.key;
        }
      }
      return bestKey;
    }

    function pick(sx, sy) {
      if (!gl) return pickAnalytic(sx, sy);
      if (!bars.length) return null;
      if (pickDirty) renderPickBuffer();
      const scaleX = canvas.width / (canvas.clientWidth || 1);
      const scaleY = canvas.height / (canvas.clientHeight || 1);
      const bx = Math.round(sx * scaleX);
      const by = canvas.height - 1 - Math.round(sy * scaleY);
      if (bx < 0 || by < 0 || bx >= canvas.width || by >= canvas.height) return null;
      const px = new Uint8Array(4);
      gl.bindFramebuffer(gl.FRAMEBUFFER, pickFb);
      gl.readPixels(bx, by, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      const id = (px[0] << 16) | (px[1] << 8) | px[2];
      if (!id || id > bars.length) return null;
      return bars[id - 1].key;
    }

    // ── interaction ─────────────────────────────────────────────────────────────────────────
    let dragging = false;
    let lastX = 0, lastY = 0, dragDist = 0;

    function rafLoop() {
      if (!dragging) return;
      render();
      requestAnimationFrame(rafLoop);
    }

    function showTooltip(sx, sy) {
      const key = pick(sx, sy);
      if (!key) {
        tooltip.hidden = true;
        return;
      }
      const bar = bars.find((b) => b.key === key);
      tooltip.textContent = bar.count + ' ' + bar.status + ' · ' + formatDay(bar.day);
      tooltip.hidden = false;
      tooltip.style.left = (canvas.offsetLeft + sx + 14) + 'px';
      tooltip.style.top = (canvas.offsetTop + sy - 10) + 'px';
    }

    canvas.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      dragging = true;
      dragDist = 0;
      lastX = e.clientX;
      lastY = e.clientY;
      canvas.classList.add('dragging');
      canvas.setPointerCapture(e.pointerId);
      tooltip.hidden = true;
      requestAnimationFrame(rafLoop);
    });

    canvas.addEventListener('pointermove', (e) => {
      if (dragging) {
        const dx = e.clientX - lastX;
        const dy = e.clientY - lastY;
        lastX = e.clientX;
        lastY = e.clientY;
        dragDist += Math.abs(dx) + Math.abs(dy);
        yaw -= DRAG_DEG_PER_PX * dx;
        pitch = clamp(pitch + DRAG_DEG_PER_PX * dy, PITCH_MIN, PITCH_MAX);
        return;                                // the rAF loop renders this frame
      }
      const rect = canvas.getBoundingClientRect();
      showTooltip(e.clientX - rect.left, e.clientY - rect.top);
    });

    const endDrag = (e) => {
      if (!dragging) return;
      dragging = false;
      canvas.classList.remove('dragging');
      if (canvas.hasPointerCapture && canvas.hasPointerCapture(e.pointerId)) {
        canvas.releasePointerCapture(e.pointerId);
      }
      render();
    };
    canvas.addEventListener('pointerup', endDrag);
    canvas.addEventListener('pointercancel', endDrag);
    canvas.addEventListener('pointerleave', () => { tooltip.hidden = true; });

    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();                       // zooming over the chart must not scroll the page
      distance = clamp(distance * Math.exp(WHEEL_K * e.deltaY), DIST_MIN, DIST_MAX);
      render();
    }, { passive: false });

    canvas.addEventListener('dblclick', () => {
      yaw = DEFAULTS.yaw;
      pitch = DEFAULTS.pitch;
      distance = DEFAULTS.distance;
      render();
    });

    canvas.addEventListener('click', (e) => {
      if (dragDist > 5) return;                 // a drag is not a click
      const rect = canvas.getBoundingClientRect();
      const key = pick(e.clientX - rect.left, e.clientY - rect.top);
      if (key) onPickStatus(key.split('|')[1]);
    });

    if (typeof ResizeObserver !== 'undefined' && gl) {
      new ResizeObserver(() => render()).observe(canvas);
    }

    // ── the graded instrumentation surface ──────────────────────────────────────────────────
    window.vsdbg = {
      version: 3,
      scene() {
        return {
          days: (buckets.days || []).slice(),
          statuses: STATUS_ORDER.slice(),
          bars: bars.map((b) => ({ key: b.key, i: b.i, j: b.j, count: b.count,
                                   x: b.x, z: b.z, h: b.h })),
        };
      },
      camera() {
        return { yaw: yaw, pitch: pitch, distance: distance };
      },
      setCamera(cam) {
        if (cam && typeof cam.yaw === 'number') yaw = cam.yaw;
        if (cam && typeof cam.pitch === 'number') pitch = clamp(cam.pitch, PITCH_MIN, PITCH_MAX);
        if (cam && typeof cam.distance === 'number') distance = clamp(cam.distance, DIST_MIN, DIST_MAX);
        render();
      },
      project: project,
      pick: pick,
      frames() { return frameCount; },
    };

    return {
      supported: !!gl,
      setBuckets(next) {
        buckets = next || { days: [], statuses: STATUS_ORDER.slice(), cells: [] };
        rebuildGeometry();
        render();
      },
      render: render,
      bars: () => bars,
    };
  }

  window.VsproViz = { mount: mount, barsFromBuckets: barsFromBuckets, STATUS_ORDER: STATUS_ORDER };
})();
