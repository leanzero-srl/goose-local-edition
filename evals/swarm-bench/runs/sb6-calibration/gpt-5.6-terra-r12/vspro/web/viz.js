window.VSViz = (() => {
'use strict';
const canvas = document.querySelector('#viz3d');
const stage = document.querySelector('#viz-stage');
const tip = document.querySelector('#viz-tooltip');
const loading = document.querySelector('#viz-loading');
const empty = document.querySelector('#viz-empty');
const error = document.querySelector('#viz-error');
const fallback = document.querySelector('#fallback-wrap');
const toggle = document.querySelector('#viz-toggle');
const COLORS = {settled:'#16A34A', pending:'#F59E0B', refunded:'#8B5CF6', failed:'#DC2626'};
const defaultCamera = {yaw:35, pitch:27, distance:30};
const camera = {...defaultCamera};
let gl = null, program = null, locations = null, positionBuffer = null, colorBuffer = null;
let vertexCount = 0, bars = [], days = [], statuses = [], showFallback = false, contextLost = false, frames = 0, drag = null;
let eventsBound = false, observer = null;

const subtract = (a, b) => a.map((value, index) => value - b[index]);
const dot = (a, b) => a.reduce((total, value, index) => total + value * b[index], 0);
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const normalize = vector => { const length = Math.hypot(...vector) || 1; return vector.map(value => value / length); };
const esc = value => String(value ?? '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
function cameraBasis() {
  const target = [0, 3, 0];
  const yaw = camera.yaw * Math.PI / 180;
  const pitch = camera.pitch * Math.PI / 180;
  const eye = [Math.cos(pitch) * Math.sin(yaw) * camera.distance, 3 + Math.sin(pitch) * camera.distance, Math.cos(pitch) * Math.cos(yaw) * camera.distance];
  const forward = normalize(subtract(target, eye));
  const right = normalize(cross(forward, [0, 1, 0]));
  return {eye, forward, right, up:cross(right, forward)};
}
function projectionMatrix() {
  const {eye, forward, right, up} = cameraBasis();
  const aspect = Math.max(1, canvas.clientWidth) / Math.max(1, canvas.clientHeight);
  const focal = 1 / Math.tan(25 * Math.PI / 180);
  const near = 0.1, far = 200;
  const view = [right[0],up[0],-forward[0],0, right[1],up[1],-forward[1],0, right[2],up[2],-forward[2],0, -dot(right,eye),-dot(up,eye),dot(forward,eye),1];
  const perspective = [focal/aspect,0,0,0, 0,focal,0,0, 0,0,(far+near)/(near-far),-1, 0,0,(2*far*near)/(near-far),0];
  const output = new Float32Array(16);
  for (let column = 0; column < 4; column++) for (let row = 0; row < 4; row++) for (let index = 0; index < 4; index++) output[column * 4 + row] += perspective[index * 4 + row] * view[column * 4 + index];
  return output;
}
function project(x, y, z) {
  const {eye, forward, right, up} = cameraBasis();
  const relative = subtract([x, y, z], eye);
  const depth = dot(relative, forward);
  if (depth <= 0.1) return null;
  const focal = 1 / Math.tan(25 * Math.PI / 180);
  const width = canvas.clientWidth, height = canvas.clientHeight;
  const aspect = width / height;
  return [(focal / aspect * dot(relative, right) / depth + 1) * width / 2, (1 - focal * dot(relative, up) / depth) * height / 2];
}
function compile(type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source); gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) { const message = gl.getShaderInfoLog(shader) || 'Unknown shader compilation error'; gl.deleteShader(shader); throw new Error(message); }
  return shader;
}
function createProgram() {
  const vertex = compile(gl.VERTEX_SHADER, 'attribute vec3 p;attribute vec3 c;uniform mat4 m;varying vec3 v;void main(){v=c;gl_Position=m*vec4(p,1.0);}');
  const fragment = compile(gl.FRAGMENT_SHADER, 'precision mediump float;varying vec3 v;void main(){gl_FragColor=vec4(v,1.0);}');
  const next = gl.createProgram();
  gl.attachShader(next, vertex); gl.attachShader(next, fragment); gl.linkProgram(next);
  gl.deleteShader(vertex); gl.deleteShader(fragment);
  if (!gl.getProgramParameter(next, gl.LINK_STATUS)) { const message = gl.getProgramInfoLog(next) || 'Unknown shader link error'; gl.deleteProgram(next); throw new Error(message); }
  return next;
}
function initGL() {
  try {
    const options = {antialias:false, alpha:false};
    gl = canvas.getContext('webgl', options) || canvas.getContext('webgl2', options) || canvas.getContext('experimental-webgl', options);
    if (!gl) throw new Error('WebGL is unavailable');
    program = createProgram();
    locations = {position:gl.getAttribLocation(program, 'p'), color:gl.getAttribLocation(program, 'c'), matrix:gl.getUniformLocation(program, 'm')};
    if (locations.position < 0 || locations.color < 0 || !locations.matrix) throw new Error('WebGL program is incomplete');
    positionBuffer = gl.createBuffer(); colorBuffer = gl.createBuffer();
    gl.enable(gl.DEPTH_TEST); gl.clearColor(15/255, 23/255, 42/255, 1);
    uploadGeometry();
    return true;
  } catch (_) {
    gl = null; program = null; locations = null;
    toggle.disabled = true; toggle.setAttribute('aria-disabled', 'true'); toggle.textContent = '3D unavailable';
    show(true, '3D is unavailable on this device. Showing the activity table instead.');
    return false;
  }
}
function color(hex, side) { return [1,3,5].map(index => { const channel = parseInt(hex.slice(index, index + 2), 16); return (side ? Math.round(channel * 0.62) : channel) / 255; }); }
function uploadGeometry() {
  if (!gl || !positionBuffer || !colorBuffer) return;
  const positions = [], colors = [];
  for (const bar of bars) {
    const x = bar.x, z = bar.z, height = bar.h;
    const points = [[x-.5,0,z-.5],[x+.5,0,z-.5],[x+.5,0,z+.5],[x-.5,0,z+.5],[x-.5,height,z-.5],[x+.5,height,z-.5],[x+.5,height,z+.5],[x-.5,height,z+.5]];
    const faces = [[4,5,6,7,false],[0,1,5,4,true],[1,2,6,5,true],[2,3,7,6,true],[3,0,4,7,true]];
    const shade = COLORS[bar.status] || '#38BDF8';
    for (const [a,b,c,d,side] of faces) for (const point of [a,b,c,a,c,d]) { positions.push(...points[point]); colors.push(...color(shade, side)); }
  }
  vertexCount = positions.length / 3;
  gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(positions), gl.DYNAMIC_DRAW);
  gl.bindBuffer(gl.ARRAY_BUFFER, colorBuffer); gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(colors), gl.DYNAMIC_DRAW);
}
function render() {
  if (!gl || contextLost || showFallback) return;
  const ratio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(canvas.clientWidth * ratio)), height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
  gl.viewport(0, 0, width, height); gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  if (vertexCount) {
    gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer); gl.enableVertexAttribArray(locations.position); gl.vertexAttribPointer(locations.position, 3, gl.FLOAT, false, 0, 0);
    gl.bindBuffer(gl.ARRAY_BUFFER, colorBuffer); gl.enableVertexAttribArray(locations.color); gl.vertexAttribPointer(locations.color, 3, gl.FLOAT, false, 0, 0);
    gl.uniformMatrix4fv(locations.matrix, false, projectionMatrix()); gl.drawArrays(gl.TRIANGLES, 0, vertexCount);
  }
  frames++;
}
function rayPick(screenX, screenY) {
  if (!bars.length) return null;
  const {eye, forward, right, up} = cameraBasis();
  const focal = 1 / Math.tan(25 * Math.PI / 180), aspect = canvas.clientWidth / canvas.clientHeight;
  const nx = (2 * screenX / canvas.clientWidth - 1) * aspect / focal;
  const ny = (1 - 2 * screenY / canvas.clientHeight) / focal;
  const direction = normalize([forward[0] + right[0] * nx + up[0] * ny, forward[1] + right[1] * nx + up[1] * ny, forward[2] + right[2] * nx + up[2] * ny]);
  let hit = null, nearest = Infinity;
  for (const bar of bars) {
    const low = [bar.x-.5, 0, bar.z-.5], high = [bar.x+.5, bar.h, bar.z+.5];
    let enter = -Infinity, leave = Infinity;
    for (let axis = 0; axis < 3; axis++) {
      if (Math.abs(direction[axis]) < 1e-10) { if (eye[axis] < low[axis] || eye[axis] > high[axis]) { enter = Infinity; break; } continue; }
      const first = (low[axis] - eye[axis]) / direction[axis], second = (high[axis] - eye[axis]) / direction[axis];
      enter = Math.max(enter, Math.min(first, second)); leave = Math.min(leave, Math.max(first, second));
    }
    const distance = enter >= 0 ? enter : leave;
    if (leave >= Math.max(enter, 0) && distance >= 0 && distance < nearest) { nearest = distance; hit = bar; }
  }
  return hit;
}
function berlinDate(day) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!match) return day;
  return new Intl.DateTimeFormat(undefined, {timeZone:'Europe/Berlin', day:'numeric', month:'short', year:'numeric'}).format(new Date(Date.UTC(+match[1], +match[2]-1, +match[3], 12)));
}
function hover(event) {
  if (drag) return;
  const rect = canvas.getBoundingClientRect(); const bar = rayPick(event.clientX - rect.left, event.clientY - rect.top);
  if (!bar) { tip.style.display = 'none'; return; }
  tip.textContent = bar.count + ' ' + bar.status + ' · ' + berlinDate(bar.day); tip.style.display = 'block';
  const width = tip.offsetWidth, height = tip.offsetHeight;
  tip.style.left = Math.max(4, Math.min(rect.width - width - 4, event.clientX - rect.left + 12)) + 'px';
  tip.style.top = Math.max(4, Math.min(rect.height - height - 4, event.clientY - rect.top + 12)) + 'px';
}
function fallbackTable() {
  const lookup = new Map(bars.map(bar => [bar.day + '|' + bar.status, bar.count]));
  return '<table id="viz-fallback"><thead><tr><th>Day</th>' + statuses.map(status => '<th>' + esc(status) + '</th>').join('') + '<th>Total</th></tr></thead><tbody>' + days.map(day => { const values = statuses.map(status => lookup.get(day + '|' + status) || 0); return '<tr><td>' + esc(day) + '</td>' + statuses.map((status, index) => '<td data-day="' + esc(day) + '" data-status="' + esc(status) + '"><button type="button" class="fallback-count" data-status="' + esc(status) + '" aria-label="Filter payments by ' + esc(status) + '">' + values[index] + '</button></td>').join('') + '<td>' + values.reduce((sum, value) => sum + value, 0) + '</td></tr>'; }).join('') + '</tbody></table>';
}
function show(on, message = '') {
  if (!gl) on = true;
  showFallback = on; toggle.setAttribute('aria-pressed', String(on));
  if (!toggle.disabled) toggle.textContent = on ? 'Show 3D' : 'Show table';
  canvas.hidden = on; fallback.hidden = !on;
  if (on) fallback.innerHTML = (message ? '<p id="viz-unavailable">' + esc(message) + '</p>' : '') + fallbackTable(); else render();
}
function bindEvents() {
  if (eventsBound) return; eventsBound = true;
  canvas.addEventListener('webglcontextlost', event => { event.preventDefault(); contextLost = true; tip.style.display = 'none'; show(true, '3D is temporarily unavailable. Showing the activity table instead.'); });
  canvas.addEventListener('webglcontextrestored', () => { contextLost = false; if (initGL()) show(false); });
  canvas.addEventListener('pointerdown', event => { drag = {x:event.clientX, y:event.clientY, moved:false}; canvas.setPointerCapture(event.pointerId); });
  canvas.addEventListener('pointermove', event => { if (!drag) { hover(event); return; } const dx = event.clientX - drag.x, dy = event.clientY - drag.y; drag.moved ||= Math.abs(dx) + Math.abs(dy) > 2; camera.yaw -= dx * .35; camera.pitch = Math.max(5, Math.min(85, camera.pitch + dy * .35)); drag.x = event.clientX; drag.y = event.clientY; render(); });
  canvas.addEventListener('pointerup', event => { const previous = drag; drag = null; if (previous && !previous.moved) { const rect = canvas.getBoundingClientRect(); const bar = rayPick(event.clientX - rect.left, event.clientY - rect.top); if (bar) window.dispatchEvent(new CustomEvent('vspro-status', {detail:bar.status})); } hover(event); });
  canvas.addEventListener('pointercancel', () => { drag = null; });
  canvas.addEventListener('wheel', event => { event.preventDefault(); camera.distance = Math.max(10, Math.min(90, camera.distance * Math.exp(.0012 * event.deltaY))); render(); }, {passive:false});
  canvas.addEventListener('dblclick', () => { Object.assign(camera, defaultCamera); render(); });
  canvas.addEventListener('keydown', event => { if (event.key === 'Escape') { Object.assign(camera, defaultCamera); render(); } });
  toggle.addEventListener('click', () => show(!showFallback));
  fallback.addEventListener('click', event => { const button = event.target.closest('.fallback-count'); if (button) window.dispatchEvent(new CustomEvent('vspro-status', {detail:button.dataset.status})); });
  observer = new ResizeObserver(() => render()); observer.observe(stage);
}
function setLoading() { loading.hidden = false; empty.hidden = true; error.hidden = true; tip.style.display = 'none'; }
function setData(data) {
  days = Array.isArray(data?.days) ? data.days : []; statuses = Array.isArray(data?.statuses) ? data.statuses : [];
  const dayIndex = new Map(days.map((day, index) => [day, index])); const statusIndex = new Map(statuses.map((status, index) => [status, index]));
  bars = (Array.isArray(data?.cells) ? data.cells : []).filter(cell => Number(cell.count) > 0 && dayIndex.has(cell.day) && statusIndex.has(cell.status)).map(cell => { const i = dayIndex.get(cell.day), j = statusIndex.get(cell.status); return {key:cell.day + '|' + cell.status, day:cell.day, status:cell.status, count:Number(cell.count), i, j, x:(i - (days.length - 1) / 2) * 1.5, z:(j - 1.5) * 1.5, h:Number(cell.count) * .25}; });
  loading.hidden = true; error.hidden = true; empty.hidden = bars.length > 0; uploadGeometry(); if (showFallback) show(true); render();
}
function showError() {
  loading.hidden = true; tip.style.display = 'none'; empty.hidden = true;
  bars = []; days = []; statuses = []; vertexCount = 0; fallback.innerHTML = ''; fallback.hidden = true;
  showFallback = false; canvas.hidden = true; error.hidden = false;
}
bindEvents(); initGL();
window.vsdbg = {version:3, scene:() => ({days:[...days], statuses:[...statuses], bars:bars.map(({key,i,j,count,x,z,h}) => ({key,i,j,count,x,z,h}))}), camera:() => ({...camera}), setCamera:value => { if (Number.isFinite(value.yaw)) camera.yaw = value.yaw; if (Number.isFinite(value.pitch)) camera.pitch = Math.max(5, Math.min(85, value.pitch)); if (Number.isFinite(value.distance)) camera.distance = Math.max(10, Math.min(90, value.distance)); render(); }, project, pick:(x,y) => rayPick(x,y)?.key || null, frames:() => frames};
return {setData, setLoading, error:showError, render};
})();