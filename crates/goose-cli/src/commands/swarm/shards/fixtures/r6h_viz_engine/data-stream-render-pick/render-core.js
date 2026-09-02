/* ============================================================================
 * viz.js piece — Rendering (bounded draw calls, demand rendering)  (shard: data-stream-render-pick)
 * Main-thread WebGL context on #viz3d with DPR-sized backing store; instanced
 * column draws (1 default-FBO draw per frame ≤ 8); demand-only rendering;
 * panel states. Sole writer of: frames. Reads camera/brushSet (sibling state).
 * ==========================================================================*/

let frames = 0;                       // shared state I write: total rendered frames, monotonic

let vizGL = null, vizCanvas = null;   // WebGL context (null when unavailable) + canvas
let vizW = 1, vizH = 1, vizDPR = 1;   // drawing-buffer size in device px + DPR
let vizDivisor = null, vizDrawInst = null;
let mainProg = null, pickProg = null;
let vizMainMVP = null, vizMainBrush = null, vizPickMVP = null;
let geomBuf = null, instBuf = null, idBuf = null, flagBuf = null;
let vizRenderQueued = false;
const VIZ_CAM_DEFAULT = { yaw: 30, pitch: 40, distance: 260, vyaw: 0, vpitch: 0 };

// Unit column: footprint 0.9x0.9 (half 0.45), base y=0, unit height; aTop marks
// the top face. 30 verts x [x,y,z,top] = 120 floats (480 bytes, static).
const VIZ_VERT_COUNT = 30;
const VIZ_VERTS = new Float32Array([
  // top face (y=1), top=1
  -0.45,1,-0.45,1,  0.45,1,-0.45,1,  0.45,1,0.45,1,
  -0.45,1,-0.45,1,  0.45,1,0.45,1,  -0.45,1,0.45,1,
  // front z=+0.45, top=0
  -0.45,0,0.45,0,  0.45,0,0.45,0,  0.45,1,0.45,0,
  -0.45,0,0.45,0,  0.45,1,0.45,0,  -0.45,1,0.45,0,
  // back z=-0.45
   0.45,0,-0.45,0, -0.45,0,-0.45,0, -0.45,1,-0.45,0,
   0.45,0,-0.45,0, -0.45,1,-0.45,0,  0.45,1,-0.45,0,
  // left x=-0.45
  -0.45,0,0.45,0, -0.45,0,-0.45,0, -0.45,1,-0.45,0,
  -0.45,0,0.45,0, -0.45,1,-0.45,0, -0.45,1,0.45,0,
  // right x=+0.45
   0.45,0,-0.45,0, 0.45,0,0.45,0,  0.45,1,0.45,0,
   0.45,0,-0.45,0, 0.45,1,0.45,0,  0.45,1,-0.45,0
]);

// Main pass: flat unlit exact colors. top face = base; side = round(0.55*base);
// brushed-dim base = round(0.30*c) applied BEFORE the side factor.
const VIZ_MAIN_VS = [
  'precision highp float;',
  'attribute vec3 aPos; attribute float aTop;',
  'attribute vec3 aInstXZH; attribute vec3 aInstColor; attribute float aFlag;',
  'uniform mat4 uMVP; uniform float uBrush;',
  'varying vec3 vBase; varying float vTop;',
  'void main() {',
  '  vec3 c255 = aInstColor * 255.0;',
  '  vec3 base = (uBrush > 0.5 && aFlag < 0.5) ? floor(c255 * 0.30 + 0.5) : c255;',
  '  vBase = base; vTop = aTop;',
  '  vec3 p = vec3(aPos.x + aInstXZH.x, aPos.y * aInstXZH.z, aPos.z + aInstXZH.y);',
  '  gl_Position = uMVP * vec4(p, 1.0);',
  '}'
].join('\n');
const VIZ_MAIN_FS = [
  'precision highp float;',
  'varying vec3 vBase; varying float vTop;',
  'void main() {',
  '  vec3 col = (vTop > 0.5) ? vBase / 255.0 : floor(vBase * 0.55 + 0.5) / 255.0;',
  '  gl_FragColor = vec4(col, 1.0);',
  '}'
].join('\n');

// Pick pass: identity idNum color (r,g,b byte split of n+1, a=255).
const VIZ_PICK_VS = [
  'precision highp float;',
  'attribute vec3 aPos; attribute vec3 aInstXZH; attribute float aId;',
  'uniform mat4 uMVP; varying vec4 vId;',
  'void main() {',
  '  vec3 p = vec3(aPos.x + aInstXZH.x, aPos.y * aInstXZH.z, aPos.z + aInstXZH.y);',
  '  gl_Position = uMVP * vec4(p, 1.0);',
  '  float id = aId;',
  '  vId = vec4(mod(id, 256.0) / 255.0, mod(floor(id / 256.0), 256.0) / 255.0,',
  '              mod(floor(id / 65536.0), 256.0) / 255.0, 1.0);',
  '}'
].join('\n');
const VIZ_PICK_FS = [
  'precision highp float;',
  'varying vec4 vId; void main() { gl_FragColor = vId; }'
].join('\n');

function vizCompileProgram(vsSrc, fsSrc, attribs) {
  const gl = vizGL;
  function sh(type, src) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src); gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  }
  const p = gl.createProgram();
  gl.attachShader(p, sh(gl.VERTEX_SHADER, vsSrc));
  gl.attachShader(p, sh(gl.FRAGMENT_SHADER, fsSrc));
  for (const name in attribs) gl.bindAttribLocation(p, attribs[name], name);
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  return p;
}

// Backing store = clientWidth/Height x devicePixelRatio; pick FBO follows.
function vizSizeCanvas() {
  if (!vizCanvas) return;
  vizDPR = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(vizCanvas.clientWidth * vizDPR));
  const h = Math.max(1, Math.round(vizCanvas.clientHeight * vizDPR));
  if (vizCanvas.width !== w) vizCanvas.width = w;
  if (vizCanvas.height !== h) vizCanvas.height = h;
  if (w !== vizW || h !== vizH) {
    vizW = w; vizH = h;
    if (vizGL) { vizCreatePickFBO(); pickDirty = true; }
  }
}

// Allocate/refresh the per-instance GL buffers from current JS state.
// Initial allocation happens at load (bufferData allowed); overflow growth is a
// last resort. Flags are preserved from the sibling's CPU brushFlag when present.
function vizUploadAllInstances() {
  const gl = vizGL; if (!gl) return;
  if (!instBuf) { instBuf = gl.createBuffer(); idBuf = gl.createBuffer(); flagBuf = gl.createBuffer(); }
  gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
  gl.bufferData(gl.ARRAY_BUFFER, instanceGeom, gl.DYNAMIC_DRAW);
  gl.bindBuffer(gl.ARRAY_BUFFER, idBuf);
  gl.bufferData(gl.ARRAY_BUFFER, idNumArr, gl.DYNAMIC_DRAW);
  const fb = new Uint8Array(geomCap);
  if (typeof brushFlag !== 'undefined' && brushFlag) {
    fb.set(brushFlag.subarray(0, Math.min(brushFlag.length, geomCap)));
  }
  gl.bindBuffer(gl.ARRAY_BUFFER, flagBuf);
  gl.bufferData(gl.ARRAY_BUFFER, fb, gl.DYNAMIC_DRAW);
  // fixed attribute indices: 0 aPos, 1 aTop, 2 aInstXZH, 3 aInstColor, 4 aFlag, 5 aId
  gl.bindBuffer(gl.ARRAY_BUFFER, instBuf);
  gl.enableVertexAttribArray(2); gl.vertexAttribPointer(2, 3, gl.FLOAT, false, 24, 0);
  gl.enableVertexAttribArray(3); gl.vertexAttribPointer(3, 3, gl.FLOAT, false, 24, 12);
  gl.bindBuffer(gl.ARRAY_BUFFER, flagBuf);
  gl.enableVertexAttribArray(4); gl.vertexAttribPointer(4, 1, gl.UNSIGNED_BYTE, false, 1, 0);
  gl.bindBuffer(gl.ARRAY_BUFFER, idBuf);
  gl.enableVertexAttribArray(5); gl.vertexAttribPointer(5, 1, gl.FLOAT, false, 4, 0);
  vizDivisor(2, 1); vizDivisor(3, 1); vizDivisor(4, 1); vizDivisor(5, 1);
}

// Create the webgl/webgl2 context {antialias:false, alpha:false} on #viz3d on
// the MAIN thread; sizes the backing store; returns false + visible notice if
// both contexts are null. Never throws.
function initViz() {
  const canvas = document.getElementById('viz3d');
  if (!canvas) { setPanelState('unavailable'); return false; }
  vizCanvas = canvas;
  let gl = null;
  try { gl = canvas.getContext('webgl2', { antialias: false, alpha: false }); } catch (e) {}
  const isGL2 = !!gl;
  if (!gl) { try { gl = canvas.getContext('webgl', { antialias: false, alpha: false }); } catch (e) {} }
  if (!gl) { setPanelState('unavailable'); return false; }
  let dv = null, di = null;
  if (isGL2) { dv = gl.vertexAttribDivisor.bind(gl); di = gl.drawArraysInstanced.bind(gl); }
  else {
    const ext = gl.getExtension('ANGLE_instanced_arrays');
    if (ext) { dv = ext.vertexAttribDivisorANGLE.bind(ext); di = ext.drawArraysInstancedANGLE.bind(ext); }
  }
  if (!dv || !di) { setPanelState('unavailable'); return false; }
  vizGL = gl; vizDivisor = dv; vizDrawInst = di;
  try {
    vizSizeCanvas();
    mainProg = vizCompileProgram(VIZ_MAIN_VS, VIZ_MAIN_FS,
      { aPos: 0, aTop: 1, aInstXZH: 2, aInstColor: 3, aFlag: 4 });
    pickProg = vizCompileProgram(VIZ_PICK_VS, VIZ_PICK_FS, { aPos: 0, aInstXZH: 2, aId: 5 });
    vizMainMVP = gl.getUniformLocation(mainProg, 'uMVP');
    vizMainBrush = gl.getUniformLocation(mainProg, 'uBrush');
    vizPickMVP = gl.getUniformLocation(pickProg, 'uMVP');
    geomBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, geomBuf);
    gl.bufferData(gl.ARRAY_BUFFER, VIZ_VERTS, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 16, 0);
    gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 1, gl.FLOAT, false, 16, 12);
    vizDivisor(0, 0); vizDivisor(1, 0);
    vizCreatePickFBO();
    window.addEventListener('resize', function () { vizSizeCanvas(); requestRender(); });
  } catch (e) {
    vizGL = null; setPanelState('unavailable'); return false;
  }
  return true;
}

// Projection contract: T=(0,1,0), fovY 50, near 0.5/far 1000, CSS-px mapping.
function vizBuildMVP(cam) {
  const th = cam.yaw * Math.PI / 180, ph = cam.pitch * Math.PI / 180;
  const cp = Math.cos(ph), sp = Math.sin(ph);
  const ex = cp * Math.sin(th) * cam.distance, ey = 1 + sp * cam.distance, ez = cp * Math.cos(th) * cam.distance;
  let fx = -ex, fy = 1 - ey, fz = -ez;
  const fl = Math.hypot(fx, fy, fz) || 1; fx /= fl; fy /= fl; fz /= fl;
  let rx = -fz, ry = 0, rz = fx;                       // r = normalize(f x (0,1,0))
  const rl = Math.hypot(rx, ry, rz) || 1; rx /= rl; rz /= rl;
  const ux = ry * fz - rz * fy, uy = rz * fx - rx * fz, uz = rx * fy - ry * fx;   // u = r x f
  const V = new Float32Array([
    rx, ux, fx, 0,  ry, uy, fy, 0,  rz, uz, fz, 0,
    -(rx * ex + ry * ey + rz * ez), -(ux * ex + uy * ey + uz * ez), -(fx * ex + fy * ey + fz * ez), 1
  ]);
  const near = 0.5, far = 1000;
  const A = (far + near) / (far - near), B = (-2 * far * near) / (far - near);
  const k = 1 / Math.tan(25 * Math.PI / 180), aspect = vizW / vizH;
  const P = new Float32Array([k / aspect, 0, 0, 0,  0, k, 0, 0,  0, 0, A, 1,  0, 0, B, 0]);
  const out = new Float32Array(16);
  for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) {
    out[c * 4 + r] = P[r] * V[c * 4] + P[4 + r] * V[c * 4 + 1] + P[8 + r] * V[c * 4 + 2] + P[12 + r] * V[c * 4 + 3];
  }
  return out;
}

// Render one frame: clear to #101828, depth ON, ONE instanced draw of all N
// columns (≤ 8 default-FBO draws). Increments frames. Marks pick stale.
function renderFrame() {
  const gl = vizGL; if (!gl) return;
  const cam = (typeof camera !== 'undefined' && camera) ? camera : VIZ_CAM_DEFAULT;
  const mvp = vizBuildMVP(cam);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  gl.viewport(0, 0, vizW, vizH);
  gl.enable(gl.DEPTH_TEST);
  gl.depthFunc(gl.LEQUAL);
  gl.clearColor(DS_BG[0], DS_BG[1], DS_BG[2], 1.0);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  if (records.count > 0 && layoutBasis) {
    gl.useProgram(mainProg);
    gl.uniformMatrix4fv(vizMainMVP, false, mvp);
    gl.uniform1f(vizMainBrush, (typeof brushSet !== 'undefined' && brushSet && brushSet.size > 0) ? 1.0 : 0.0);
    vizDrawInst(gl.TRIANGLES, 0, VIZ_VERT_COUNT, records.count);
  }
  frames++;
  pickDirty = true;   // visible scene may have moved -> next pick re-draws offscreen
  if (typeof updateLabels === 'function') { try { updateLabels(); } catch (e) {} }
}

// Coalesce a demand render to the next rAF — only on load/input/coast/data
// change. No idle loop: 0 default-FBO draws over any 500 ms window at rest.
function requestRender() {
  if (vizRenderQueued || !vizGL) return;
  vizRenderQueued = true;
  requestAnimationFrame(function () { vizRenderQueued = false; renderFrame(); });
}

// Panel states: #viz-empty (no records), #viz-error (fetch failed), visible
// 3D-unavailable notice, or ready. Never a blank panel.
function setPanelState(state) {
  const show = function (id, on) {
    const el = document.getElementById(id);
    if (el) el.style.display = on ? '' : 'none';
  };
  let unav = document.getElementById('viz-unavailable');
  if (state === 'unavailable' && !unav) {
    const host = vizCanvas ? vizCanvas.parentElement : document.body;
    unav = document.createElement('div');
    unav.id = 'viz-unavailable';
    unav.className = 'viz-unavailable';
    unav.setAttribute('role', 'status');
    unav.textContent = '3D unavailable — this browser does not support WebGL.';
    host.appendChild(unav);
  }
  if (unav) unav.style.display = state === 'unavailable' ? '' : 'none';
  show('viz-empty', state === 'empty');
  show('viz-error', state === 'error');
  if (vizCanvas) vizCanvas.style.display = state === 'unavailable' ? 'none' : '';
}
