import { useEffect, useState } from 'react';

/**
 * A clock for what the person watches tick by (elapsed, "in 8m"): display only, it decides
 * nothing. It ticks once a second only while `ticking`; otherwise it holds the time of the last
 * change.
 */
export function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    if (!ticking) return undefined;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [ticking]);
  return now;
}
