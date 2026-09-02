// ============================================================================
// viz.js piece — Camera: projection contract  (assembly section: "Camera")
// Owns the exact orbit projection the grader recomputes independently.
//   θ = yaw·π/180, φ = pitch·π/180, T = (0,1,0)
//   eye = T + distance·(cosφ·sinθ, sinφ, cosφ·cosθ)
//   f = normalize(T − eye);  r = normalize(f × (0,1,0));  u = r × f
//   q = p − eye;  xc = q·r;  yc = q·u;  zc = q·f;   zc ≤ 0.5 → does not project
//   fovY = 50°, k = 1/tan(fovY/2), aspect = Wcss/Hcss, near 0.5 / far 1000
//   ndcx = (k/aspect)·xc/zc;  ndcy = k·yc/zc
//   sx = (ndcx+1)/2·Wcss;     sy = (1−ndcy)/2·Hcss      (CSS px, canvas top-left)
// Reads shared state: camera (written by this shard).
// ============================================================================

const CAM_DEG2RAD = Math.PI / 180;
const CAM_FOVY_K = 1 / Math.tan(25 * CAM_DEG2RAD); // k = 1/tan(fovY/2), fovY = 50°
const CAM_NEAR_Z = 0.5;                            // zc ≤ near → does not project

let camCanvasEl = null;
function camGetCanvas() {
  if (!camCanvasEl) camCanvasEl = document.getElementById('viz3d');
  return camCanvasEl;
}

/**
 * project(x, y, z): {sx, sy} | null
 * Exact projection of world point (x,y,z) under the LIVE camera, in CSS pixels
 * relative to the canvas top-left. Returns null when zc ≤ 0.5 (behind/at near).
 */
function project(x, y, z) {
  const cv = camGetCanvas();
  if (!cv) return null;
  const W = cv.clientWidth, H = cv.clientHeight;   // CSS px (Wcss / Hcss)
  if (!(W > 0) || !(H > 0)) return null;

  const th = camera.yaw * CAM_DEG2RAD;
  const ph = camera.pitch * CAM_DEG2RAD;
  const cp = Math.cos(ph), sp = Math.sin(ph);
  const ct = Math.cos(th), st = Math.sin(th);
  const d = camera.distance;

  // eye = T + distance·(cosφ·sinθ, sinφ, cosφ·cosθ), with T = (0,1,0)
  const ex = d * cp * st;
  const ey = 1 + d * sp;
  const ez = d * cp * ct;

  // f = normalize(T − eye)
  let fx = -ex, fy = 1 - ey, fz = -ez;
  const fl = Math.sqrt(fx * fx + fy * fy + fz * fz);
  if (!(fl > 0)) return null;
  fx /= fl; fy /= fl; fz /= fl;

  // r = normalize(f × (0,1,0));  f × up(0,1,0) = (−fz, 0, fx)
  let rx = -fz, rz = fx;
  const rl = Math.sqrt(rx * rx + rz * rz);
  if (!(rl > 0)) return null; // pitch pole — unreachable under the [5,85] clamp
  rx /= rl; rz /= rl;
  const ry = 0;

  // u = r × f
  const ux = ry * fz - rz * fy;
  const uy = rz * fx - rx * fz;
  const uz = rx * fy - ry * fx;

  // q = p − eye → camera-space coordinates
  const qx = x - ex, qy = y - ey, qz = z - ez;
  const xc = qx * rx + qy * ry + qz * rz;
  const yc = qx * ux + qy * uy + qz * uz;
  const zc = qx * fx + qy * fy + qz * fz;
  if (zc <= CAM_NEAR_Z) return null;

  const aspect = W / H;
  const ndcx = (CAM_FOVY_K / aspect) * xc / zc;
  const ndcy = CAM_FOVY_K * yc / zc;
  return { sx: (ndcx + 1) * 0.5 * W, sy: (1 - ndcy) * 0.5 * H };
}
