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
 * A run can begin on either side of this component's mount, so there are two arrivals and they are not
 * interchangeable:
 *   - already running when the renderer mounted -> the one status probe below. It runs ONCE per mount:
 *     re-probing on every arrival at '/' is what used to drag a user who had deliberately walked back to
 *     the chat view out of it again, over and over, for the whole length of a multi-hour run.
 *   - started while the renderer was already sitting on '/' -> the 'benchmark-started' event. The mount
 *     probe structurally cannot see this one, and it is the case the redirect was written for.
 *
 * Both arrivals redirect only FROM the default route, read at the moment the redirect would happen, so a
 * deliberate navigation is never overridden -- not even by a probe that resolves after the user has moved.
 */
export default function BenchmarkAutoOpen() {
  const navigate = useNavigate();
  const location = useLocation();
  // Held in a ref, not in the deps: an effect keyed on either one re-runs on every navigation, which is
  // exactly how the mount-only probe turned into a poll that fought the user.
  const latest = useRef({ pathname: location.pathname, navigate });
  latest.current = { pathname: location.pathname, navigate };

  useEffect(() => {
    let alive = true;
    const openBenchmark = () => {
      if (!alive || latest.current.pathname !== '/') return;
      latest.current.navigate('/benchmark');
    };

    void (async () => {
      try {
        const st = await window.electron.benchmarkStatus();
        if (st?.running) openBenchmark();
      } catch {
        /* no benchmark bridge in this build */
      }
    })();

    const onStarted = () => openBenchmark();
    window.electron.on?.('benchmark-started', onStarted);
    return () => {
      alive = false;
      window.electron.off?.('benchmark-started', onStarted);
    };
  }, []);

  return null;
}
