# SB-8.0 — Gantry: a working 3D cargo crane

Build ONE local web app: a warehouse gantry digital twin. One server, one persistent store,
one page. No authentication, framework mandate, microservices, webhook platform, reporting
suite or documentation essay. Spend your effort on correct geometry, transactional commands
and a usable 3D console. Do not replace 3D with a diagram or static picture.

Boot contract: `python3 app.py --port PORT --db PATH --vendor URL`. It may launch an installed
frontend build, but must bind 127.0.0.1 and serve `/`. Install dependencies before finishing.
PATH is a new directory on first boot and an existing one after restart. Vendor for development:
{BASE_URL}. `GET /scene` there provides the immutable scene JSON. Fetch once on first boot and
persist it; restarting must work even with the vendor unavailable. Vendor also serves a pinned
Three.js ES module at `/three.js`; you may copy it into your app. No CDN/network needed afterwards.
The grader uses fresh seeds, ports and databases; never hardcode the development scene.

## Coordinates and state

Metres, right-handed, Y up. Floor [0,width] × [0,depth], crane bridge moves along Z,
trolley along X; hoist height Y is the BOTTOM of its carried box. Angles in degrees around +Y.
Scene shape:
`{seed,width,depth,height,capacity,initial:{x,z,y,yaw},boxes:[{id,x,z,y,w,d,h,mass,color}],obstacles:[{id,x,z,y,w,d,h}]}`.
Box/obstacle X,Z are centres; Y is bottom. Box footprints are axis-aligned initially. Scene
positions are finite and inside the warehouse. Color is CSS #RRGGBB. Masses and capacity are
integer kg; IDs are opaque, and box order is not meaningful.

`GET /api/scene`: unchanged scene. `GET /api/state`: `{revision,pose:{x,z,y,yaw},held,boxes}`.
Initially revision=0, held=null, pose=scene.initial. State boxes add yaw=0 and retain every
scene field. Every successful command increments revision ONCE. GET never changes state.

`POST /api/commands`: `{id,revision,op,...}`; id is nonempty string, revision integer.
Ops: `move` with x,z,y,yaw (all required finite numbers), `grip` with boxId, `release`.
Return HTTP 200 and the resulting state, or `{error:"CODE"}` with the status below.
Serialize concurrent commands. Persist state AND command receipts atomically before replying.
Compare bodies as parsed JSON values, independent of key order. An identical id/body retry returns the original response without mutation, EVEN after restart
and even when its revision is now stale. Reusing an id with a different body → 409 `id_conflict`.
Check a new command's revision before semantic validation: stale → 409 `stale_revision`.
Malformed JSON/types/unknown op → 400 `invalid_command`; no state change or consumed id.
Semantic errors below → 422, unchanged state/revision and no consumed id.

Move: x,z within warehouse bounds, y in [0,height], yaw in [-180,180]. Otherwise `bounds`.
An empty hook can move anywhere in these bounds. With a held box its centre and bottom follow
pose, its yaw follows pose; its rotated footprint and full height must stay within the warehouse.
Mass > capacity → `overload` at grip. Grip needs no current load (`already_holding`), an existing
box (`unknown_box`), and |x-box.x|,|z-box.z|,|y-(box.y+box.h)| ≤0.05 (`not_aligned`). Grip lifts
without jumping: after grip pose.y becomes box.y; preserve the box yaw and set pose.yaw to that yaw; snap pose.x,z to the box centre.

Collision: positive-volume intersection of the carried oriented box with any other box or
obstacle is forbidden (`collision`); touching is allowed (epsilon 1e-7). Test Y interval and
2D separating axes of BOTH rectangles, not bounding boxes alone. Test the entire move, not
just its endpoint: interpolate linearly along the shortest yaw arc (exact 180 tie takes +180),
with N=max(1,ceil(sqrt(dx²+dy²+dz²)/0.1),ceil(abs(yawDelta)/2)), sample t=0..N inclusive. Bounds apply
at every sample. Commit only if every sample passes. No partial move on failure.

Release: no load → `not_holding`. Bottom y must be within 0.05 of the floor or the top of a
single box/obstacle that completely contains the carried rotated footprint (`unsupported`).
Choose the highest valid support; check collision again after snapping. Commit box at the support height, preserving yaw; held becomes null. Touching support is legal.
A collision/bounds check still applies. Held boxes remain in state.boxes (one box, never duplicated).

## The 3D scene is the primary product

Render a real WebGL perspective/orthographic scene driven by /api/scene and /api/state:
floor, four columns, TWO runway rails, TWO moving bridge beams, trolley with four wheels,
spreader, FOUR vertical cables, and every box and obstacle as dimensionally accurate solid
geometry. Columns at warehouse corners; runway rails at x=0,width and y=height; bridge beams
at z=pose.z±0.25,y=height; trolley centred at pose.x,pose.z; spreader above the hook/load;
cables follow hoist length. Show visible structural bracing and box edges. Box height, location,
yaw, ID and color come from backend; the held box must visibly travel with the crane. Light,
material contrast and depth cues must make it readable. Provide orbit/zoom and camera presets.

Deterministic FRONT camera for grading: orthographic, at (width/2,height/2,depth+20), looking
at (width/2,height/2,depth/2), up +Y, vertical span max(height+4,(width+4)/aspect), horizontal span derived from canvas
aspect. TOP: at (width/2,height+20,depth/2), looking down, up vector (0,0,-1), vertical span
max(depth+4,(width+4)/aspect). ISO is orthographic at target+(20,20,24),
looking at target=(width/2,height/2,depth/2), up +Y, vertical span 1.6*max(width,depth,height). Show both in the UI alongside a normal ISO overview. Boxes use their literal color
with an unlit material in FRONT/TOP for reliable visual measurement; edges/other structure may
be shaded. Structure palette: columns/rails/braces #475569, beams #f59e0b, trolley #06b6d4,
spreader #ef4444, cables #e2e8f0, wheels #111827, obstacles #64748b, floor #0f172a.
Use unlit structure materials in all camera presets so projected geometry is measurable.
Columns/rails width 0.2; bridge beams width 0.15; trolley 1×0.3×0.8 at y=height+0.2;
spreader 1×0.15×0.8 at y=pose.y+(heldBox.h if held else 0)+0.3; cables at
x=pose.x±0.4,z=pose.z±0.3 between spreader and trolley. Wheels radius 0.12, centred at x=pose.x±0.6,z=pose.z±0.5,y=height+0.05, outside the trolley so they remain visible.
All presets render live geometry, never a separately drawn test image. Keep the
canvas at least 600×400 CSS pixels at a 1280×900 viewport, resize correctly. No chart substitute.

DOM contract (accessibility/test labels, not hidden grading hooks): canvas `data-testid=scene`;
buttons `data-testid=camera-front`, `camera-top`, `camera-iso`; number inputs labelled X, Z,
Height, Yaw; buttons named Move, Grip, Release; selectable table rows `data-testid=box-ID`;
selected ID in `data-testid=selection`; status/error message `role=status`/`role=alert`;
revision visible in `data-testid=revision`. Clicking a box in 3D selects the SAME table row.
Selection displays dimensions/mass and highlights the box without erasing its colour. Camera
changes never issue motion commands. Inputs submit through backend with a fresh unique ID and
current revision; pending disables repeat submission, errors remain readable, stale conflicts
refresh state. Refresh from backend at least once a second so external commands appear within
2 seconds. Data errors show Retry, not an empty-success scene. No native alert/confirm/select.

## Scoring and scope

Backend correctness/durability 30%, rendered 3D 45%, working UI 20%, excellence 5%. Independent
rungs award partial credit: boot/state → accurate scene → kinematics → real picking → swept
collision → rotated support → concurrency/replay/restart. A superficial demo cannot score
high: missing 3D earns zero visual points, incorrect persistence loses its own checks and a
critical multiplier; fabricated/cached state cannot satisfy seeded mutations. No points for
source keywords, claimed test counts, self-reported scene graphs or an unverified canvas.
Grader drives HTTP and browser controls, inspects actual pixels, tests fresh seeds, and restarts
the process. Runtime infrastructure failure refuses a score; an app failing to render scores zero
on its visual checks. No hidden feature requirements. Ship the app, not a plan.
