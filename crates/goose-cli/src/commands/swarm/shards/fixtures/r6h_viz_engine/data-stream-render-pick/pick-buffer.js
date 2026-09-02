/* ============================================================================
 * viz.js piece — The pick buffer  (shard: data-stream-render-pick)
 * Offscreen RGBA8 color + depth FBO sized exactly to the drawing buffer, no
 * MSAA; idNum = n+1 encoded r/g/b byte split, a=255; real-pass accounting
 * (first pick after invalidation does ≥1 offscreen draw + ≥1 readPixels, ≤4
 * offscreen draws per refresh, 0 default-FBO draws on pick); click-to-brush.
 * ==========================================================================*/

let pickFBO = null, pickTex = null, pickDepth = null;
let pickCache = null;       // CPU cache of the last full readback (Uint8Array)
let pickDirty = true;       // set by renderFrame/applyBatch; cleared after refresh
let vizClickDown = null;    // {x, y, t} of the in-progress pointerdown

// Offscreen RGBA8 + depth framebuffer, exactly drawing-buffer sized, no MSAA.
function vizCreatePickFBO() {
  const gl = vizGL; if (!gl) return;
  if (pickFBO) { gl.deleteFramebuffer(pickFBO); gl.deleteTexture(pickTex); gl.deleteRenderbuffer(pickDepth); }
  pickFBO = gl.createFramebuffer();
  pickTex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, pickTex);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, vizW, vizH, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  pickDepth = gl.createRenderbuffer();
  gl.bindRenderbuffer(gl.RENDERBUFFER, pickDepth);
  gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, vizW, vizH);
  gl.bindFramebuffer(gl.FRAMEBUFFER, pickFBO);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, pickTex, 0);
  gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, pickDepth);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
}

// One offscreen pass: clear (0,0,0,a), depth ON, single instanced draw of idNum
// colors, full readPixels into the CPU cache. Exactly 1 draw + 1 read per
// refresh (≤ 4 draws allowed). 0 default-FBO draws.
function vizEnsurePickFresh() {
  if (!pickDirty) return;
  const gl = vizGL;
  const cam = (typeof camera !== 'undefined' && camera) ? camera : VIZ_CAM_DEFAULT;
  const mvp = vizBuildMVP(cam);
  gl.bindFramebuffer(gl.FRAMEBUFFER, pickFBO);
  gl.viewport(0, 0, vizW, vizH);
  gl.enable(gl.DEPTH_TEST);
  gl.depthFunc(gl.LEQUAL);
  gl.clearColor(0, 0, 0, 1);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  if (records.count > 0 && layoutBasis) {
    gl.useProgram(pickProg);
    gl.uniformMatrix4fv(vizPickMVP, false, mvp);
    vizDrawInst(gl.TRIANGLES, 0, VIZ_VERT_COUNT, records.count);
  }
  const need = vizW * vizH * 4;
  if (!pickCache || pickCache.length !== need) pickCache = new Uint8Array(need);
  gl.readPixels(0, 0, vizW, vizH, gl.RGBA, gl.UNSIGNED_BYTE, pickCache);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  pickDirty = false;
}

// Internal pick: device pixel (round(sx*DPR), Hdev-1-round(sy*DPR)); decode
// r + 256g + 65536b -> idNum; 0 = background. {id, index:n} or null.
function pickCore(sx, sy) {
  if (!vizGL || !pickFBO || records.count === 0) return null;
  const px = Math.round(sx * vizDPR);
  const py = vizH - 1 - Math.round(sy * vizDPR);
  if (px < 0 || px >= vizW || py < 0 || py >= vizH) return null;
  vizEnsurePickFresh();
  const i = (py * vizW + px) * 4;
  const idNum = pickCache[i] + 256 * pickCache[i + 1] + 65536 * pickCache[i + 2];
  if (idNum === 0) return null;
  const n = idNum - 1;
  if (n < 0 || n >= records.count) return null;
  return { id: records.id[n], index: n };
}

// Raw [r,g,b,a] bytes from the pick FBO at the same device-pixel mapping.
function pickPixelCore(sx, sy) {
  if (!vizGL || !pickFBO) return [0, 0, 0, 255];
  const px = Math.round(sx * vizDPR);
  const py = vizH - 1 - Math.round(sy * vizDPR);
  if (px < 0 || px >= vizW || py < 0 || py >= vizH) return [0, 0, 0, 255];
  vizEnsurePickFresh();
  const i = (py * vizW + px) * 4;
  return [pickCache[i], pickCache[i + 1], pickCache[i + 2], pickCache[i + 3]];
}

// Wired at load: pointerup within 5 px AND 300 ms of its pointerdown is a
// click. Instance click toggles the brush; background click clears it.
function bindClickInput(canvas) {
  canvas.addEventListener('pointerdown', function (e) {
    vizClickDown = { x: e.clientX, y: e.clientY, t: performance.now() };
  });
  canvas.addEventListener('pointerup', function (e) {
    if (!vizClickDown) return;
    const dx = e.clientX - vizClickDown.x, dy = e.clientY - vizClickDown.y;
    const dt = performance.now() - vizClickDown.t;
    vizClickDown = null;
    if (Math.hypot(dx, dy) > 5 || dt > 300) return;   // drag, not a click
    const rect = canvas.getBoundingClientRect();
    const hit = pickCore(e.clientX - rect.left, e.clientY - rect.top);
    if (window.viz3d) {
      if (hit) window.viz3d.toggleBrush(hit.id);
      else window.viz3d.clearBrush();
    }
  });
}
