// While a benchmark run is live, an accidental key chord must not spawn a window, close the
// window that shows the run, quit the app, or navigate that window away — Cmd+Shift+N once opened a
// second window on a live run. Only the ACCELERATOR is refused: a mouse click on the same menu item
// is the user really meaning it, which is why every refusal message ends in "use the menu".

export type GuardedShortcutAction = 'spawn' | 'close' | 'quit' | 'navigate' | 'reload';

// 'spawn' and 'quit' touch the run from any window (a second backend, an orphaned run); 'close',
// 'navigate' and 'reload' only matter on the window that shows the benchmark view.
const benchmarkWindowOnly: ReadonlySet<GuardedShortcutAction> = new Set([
  'close',
  'navigate',
  'reload',
]);

export type ShortcutGuardInput = {
  action: GuardedShortcutAction;
  benchmarkRunning: boolean;
  triggeredByAccelerator: boolean;
  onBenchmarkView: boolean;
};

export function shouldRefuseShortcut({
  action,
  benchmarkRunning,
  triggeredByAccelerator,
  onBenchmarkView,
}: ShortcutGuardInput): boolean {
  if (!benchmarkRunning || !triggeredByAccelerator) return false;
  return !benchmarkWindowOnly.has(action) || onBenchmarkView;
}

export const BENCHMARK_ROUTE_HASH = '#/benchmark';

export function isBenchmarkViewUrl(url: string): boolean {
  const hash = url.slice(url.indexOf('#'));
  return (
    url.includes('#') &&
    (hash === BENCHMARK_ROUTE_HASH ||
      hash.startsWith(`${BENCHMARK_ROUTE_HASH}?`) ||
      hash.startsWith(`${BENCHMARK_ROUTE_HASH}/`))
  );
}
