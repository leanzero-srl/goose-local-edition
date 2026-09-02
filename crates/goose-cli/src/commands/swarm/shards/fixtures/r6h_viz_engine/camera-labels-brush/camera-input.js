// ============================================================================
// viz.js piece — Camera: input wiring (assembly section: "Camera")
// drag / wheel-consumed / double-click reset / release velocity → coast.
//   drag:  yaw ← yaw − 0.30·Δx (unbounded); pitch ← clamp(pitch + 0.30·Δy, 5, 85)
//   wheel: distance ← clamp(distance·exp(0.0012·deltaY), 15, 340); CONSUMED
//          (preventDefault — no page scroll). Wheel does NOT cancel a coast.
//   dblclick: reset defaults (30/40/260) AND zero all angular velocity.
//   pointerdown: cancels any coast.
//   pointerup: coast from the LAST TWO move events, v = 0.30·Δpx/Δt, drag sign
//          preserved → vyaw = −0.30·Δx/Δt, vpitch = +0.30·Δy/Δt (deg/s).
// ============================================================================

const CAM_DRAG_DEG_PER_PX = 0.30;  // drag factor (deg per CSS px)
const CAM_WHEEL_EXP_K = 0.0012;    // wheel: distance·exp(0.0012·deltaY)

function bindCameraInput(canvas) {
  let dragging = false;
  let lastMove = null;   // {x, y, t} most recent pointermove (CSS px, event ms)
  let prevMove = null;   // the move before it — release velocity uses these two

  canvas.style.touchAction = 'none'; // let pointer events own touch gestures

  canvas.addEventListener('pointerdown', (e) => {
    if (!e.isPrimary || (e.pointerType === 'mouse' && e.button !== 0)) return;
    camCancelCoast(); // pointerdown cancels the coast
    dragging = true;
    prevMove = null;
    lastMove = { x: e.clientX, y: e.clientY, t: e.timeStamp };
    try { canvas.setPointerCapture(e.pointerId); } catch (_) {}
  });

  canvas.addEventListener('pointermove', (e) => {
    if (!dragging || !lastMove) return;
    const dx = e.clientX - lastMove.x;
    const dy = e.clientY - lastMove.y;
    prevMove = lastMove;
    lastMove = { x: e.clientX, y: e.clientY, t: e.timeStamp };
    if (dx === 0 && dy === 0) return;
    camera.yaw -= CAM_DRAG_DEG_PER_PX * dx;                          // unbounded
    camera.pitch = camClampPitch(camera.pitch + CAM_DRAG_DEG_PER_PX * dy);
    camInvalidateScene(); // demand render (coalesced to rAF) + pick refresh
  });

  const endDrag = (e) => {
    if (!dragging) return;
    dragging = false;
    try { canvas.releasePointerCapture(e.pointerId); } catch (_) {}
    // Release velocity = rate implied by the LAST TWO move events:
    // v = 0.30·Δpx/Δt with drag sign preserved (same mapping as the drag).
    let vyaw = 0, vpitch = 0;
    if (prevMove && lastMove && lastMove.t > prevMove.t) {
      const dt = (lastMove.t - prevMove.t) / 1000; // seconds
      vyaw = -CAM_DRAG_DEG_PER_PX * (lastMove.x - prevMove.x) / dt;
      vpitch = CAM_DRAG_DEG_PER_PX * (lastMove.y - prevMove.y) / dt;
    }
    camStartCoast(vyaw, vpitch); // below-threshold releases start no coast
  };

  canvas.addEventListener('pointerup', endDrag);
  canvas.addEventListener('pointercancel', () => { dragging = false; });

  // Wheel: zoom in place, event consumed (no page scroll). Does NOT cancel coast.
  canvas.addEventListener('wheel', (e) => {
    e.preventDefault();
    camera.distance = camClampDist(camera.distance * Math.exp(CAM_WHEEL_EXP_K * e.deltaY));
    camInvalidateScene();
  }, { passive: false });

  // Double-click: reset to defaults AND zero all angular velocity.
  canvas.addEventListener('dblclick', (e) => {
    e.preventDefault();
    camCancelCoast();
    camera.yaw = 30;
    camera.pitch = 40;
    camera.distance = 260;
    camInvalidateScene();
  });
}
