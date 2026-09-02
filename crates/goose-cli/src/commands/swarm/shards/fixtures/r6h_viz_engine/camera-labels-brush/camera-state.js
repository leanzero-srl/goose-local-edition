// ============================================================================
// viz.js piece — Camera: shared state, clamps, closed-form τ=0.4 s coast
// (assembly section: "Camera")
// WRITES shared state: camera {yaw deg, pitch deg, distance world units,
//   vyaw deg/s, vpitch deg/s} — defaults yaw 30 / pitch 40 / distance 260.
// Coast law (closed form against REAL elapsed time, not per-frame decay):
//   v(t) = v0·e^(−t/τ);  yaw(t) = yaw0 + v0·τ·(1 − e^(−t/τ));  τ = 0.4 s
//   → remaining-coast identity holds exactly: yaw_rest − yaw(t) = v(t)·τ
//   stop when |vyaw| < 2 AND |vpitch| < 2 deg/s → zero velocities, no draws
//   pitch clamps [5,85] apply continuously; hitting one zeroes vpitch
//   pointerdown / double-click cancel; wheel does NOT (see camera-input piece)
// ============================================================================

const camera = { yaw: 30, pitch: 40, distance: 260, vyaw: 0, vpitch: 0 };

const CAM_PITCH_MIN = 5, CAM_PITCH_MAX = 85;   // pitch clamp (deg)
const CAM_DIST_MIN = 15, CAM_DIST_MAX = 340;   // distance clamp (world units)
const COAST_TAU_S = 0.4;                       // inertia time constant (s)
const COAST_STOP_DEG_S = 2;                    // stop threshold (deg/s)

function camClampPitch(p) { return p < CAM_PITCH_MIN ? CAM_PITCH_MIN : p > CAM_PITCH_MAX ? CAM_PITCH_MAX : p; }
function camClampDist(d) { return d < CAM_DIST_MIN ? CAM_DIST_MIN : d > CAM_DIST_MAX ? CAM_DIST_MAX : d; }

/** getCamera(): live camera state (degrees, deg/s). Backs vs7dbg.camera. */
function getCamera() {
  return { yaw: camera.yaw, pitch: camera.pitch, distance: camera.distance, vyaw: camera.vyaw, vpitch: camera.vpitch };
}

// --- scene invalidation ------------------------------------------------------
// Every camera change is a scene invalidation: demand one render AND mark the
// pick buffer stale so the next pickCore/pickPixelCore performs ≥1 offscreen
// draw + ≥1 readPixels (real-pass accounting, owned by the render-pick shard).
// requestRender() is the declared render-pick entry; invalidatePick() is the
// pick-dirty hook — typeof-guarded: if the sibling inlines dirty-marking into
// requestRender instead, this guard is a harmless no-op. See README ASSUMES.
function camInvalidateScene() {
  requestRender();
  if (typeof invalidatePick === 'function') invalidatePick();
}

/**
 * setCameraCore(yaw, pitch, distance): apply clamps (pitch [5,85], distance
 * [15,340], yaw unbounded), zero/cancel any coast, invalidate the scene
 * (requestRender + pick refresh). vs7dbg.setCamera layers a synchronous
 * renderFrame + updateLabels on top of this (debug-api shard).
 */
function setCameraCore(yaw, pitch, distance) {
  camCancelCoast();
  camera.yaw = yaw;                      // unbounded — compared modulo 360
  camera.pitch = camClampPitch(pitch);
  camera.distance = camClampDist(distance);
  camInvalidateScene();
}

// --- closed-form coast --------------------------------------------------------
let camCoast = null; // {t0: ms, yaw0, pitch0, vyaw0, vpitch0} while coasting

function camCancelCoast() {
  if (camCoast) camCoast = null;
  camera.vyaw = 0;
  camera.vpitch = 0;
}

/**
 * Start the coast from release velocity (deg/s). A release whose rate is below
 * the stop threshold on both axes starts NO visible coast
 * (|yaw_rest − yaw_release| = 0 ≤ 0.5°). v0 is readable via getCamera() at
 * release, as the settle budget requires.
 */
function camStartCoast(vyaw0, vpitch0) {
  camera.vyaw = vyaw0;
  camera.vpitch = vpitch0;
  if (Math.abs(vyaw0) < COAST_STOP_DEG_S && Math.abs(vpitch0) < COAST_STOP_DEG_S) {
    camCancelCoast();
    return;
  }
  camCoast = { t0: performance.now(), yaw0: camera.yaw, pitch0: camera.pitch, vyaw0: vyaw0, vpitch0: vpitch0 };
  requestAnimationFrame(camCoastTick);
}

function camCoastTick() {
  if (!camCoast) return;
  const t = (performance.now() - camCoast.t0) / 1000; // real elapsed seconds
  const e = Math.exp(-t / COAST_TAU_S);
  // Closed form — exact remaining-coast identity at every sampled instant.
  camera.yaw = camCoast.yaw0 + camCoast.vyaw0 * COAST_TAU_S * (1 - e);
  camera.vyaw = camCoast.vyaw0 * e;
  // Pitch: same closed form, clamps applied continuously; hitting one zeroes vpitch.
  let p = camCoast.pitch0 + camCoast.vpitch0 * COAST_TAU_S * (1 - e);
  if (p >= CAM_PITCH_MAX) { p = CAM_PITCH_MAX; camera.vpitch = 0; }
  else if (p <= CAM_PITCH_MIN) { p = CAM_PITCH_MIN; camera.vpitch = 0; }
  else camera.vpitch = camCoast.vpitch0 * e;
  camera.pitch = p;
  camInvalidateScene(); // one demand render + pick refresh for this coast frame
  if (Math.abs(camera.vyaw) < COAST_STOP_DEG_S && Math.abs(camera.vpitch) < COAST_STOP_DEG_S) {
    // Settled: zero velocities, stop scheduling — demand rendering resumes,
    // 0 default-FBO draws over any 500 ms window at rest. Final pose is the
    // frame just demanded above.
    camCoast = null;
    camera.vyaw = 0;
    camera.vpitch = 0;
  } else {
    requestAnimationFrame(camCoastTick);
  }
}
