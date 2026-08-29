import { useEffect, useRef } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

/**
 * OPEN ON THE RUN THAT IS ALREADY GOING.
 *
 * A benchmark started headlessly -- via the `benchmark-run` IPC, which is how the harness and the loop
 * start one -- never touches the renderer, so the window opens on the chat view with a three-node run in
 * flight behind it. Mihai, on finding exactly that: "when I opened the desktop app the benchmark tab was
 * not even selected. Why?"
 *
 * A run can begin on either side of this component's mount, so there are two arrivals and neither one
 * covers the other:
 *   - already running when the renderer arrives at '/' -> the status probe.
 *   - started while the renderer was already sitting on '/' -> the 'benchmark-started' event, which the
 *     probe structurally cannot see.
 *
 * ONCE PER SESSION, NOT ONCE PER MOUNT. `fired` is what stops this dragging a user who has deliberately
 * walked back to the chat view out of it again for the whole length of a multi-hour run -- and it is
 * sufficient on its own, which matters because an earlier version replaced it with a mount-only probe on
 * the grounds that re-probing "used to" yank repeatedly. It could not have: the guard has always made
 * the redirect fire at most once. What the mount-only version DID lose is the window that boots on a
 * non-default route -- a restored session, a deep link -- where the probe resolves while the user is
 * elsewhere, is correctly discarded, and then never runs again however long they sit on '/' afterwards.
 *
 * So the probe is keyed on arrival at '/' and guarded by `fired`: it can catch a late return to the
 * default route, and it still cannot fire twice.
 */
export default function BenchmarkAutoOpen() {
  const navigate = useNavigate();
  const location = useLocation();
  const fired = useRef(false);
  // The event listener is mounted once; only the probe is keyed on the route. Keeping the callback in a
  // ref means re-keying the probe does not tear down and re-register the subscription.
  const openRef = useRef<() => void>(() => {});

  openRef.current = () => {
    if (fired.current || location.pathname !== '/') return;
    fired.current = true;
    navigate('/benchmark');
  };

  useEffect(() => {
    const onStarted = () => openRef.current();
    window.electron.on?.('benchmark-started', onStarted);
    return () => window.electron.off?.('benchmark-started', onStarted);
  }, []);

  useEffect(() => {
    if (fired.current || location.pathname !== '/') return;
    let alive = true;
    void (async () => {
      try {
        const st = await window.electron.benchmarkStatus();
        // Re-read the route through the ref rather than closing over it: the probe is async, and a user
        // who navigated away while it was in flight must not be yanked back by its result.
        if (alive && st?.running) openRef.current();
      } catch {
        /* no benchmark bridge in this build */
      }
    })();
    return () => {
      alive = false;
    };
  }, [location.pathname]);

  return null;
}
