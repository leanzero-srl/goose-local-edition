import { describe, it, expect } from 'vitest';
import { shouldRefuseShortcut, isBenchmarkViewUrl } from '../shortcutGuard';
import type { GuardedShortcutAction } from '../shortcutGuard';

const actions: GuardedShortcutAction[] = ['spawn', 'close', 'quit', 'navigate', 'reload'];

describe('shouldRefuseShortcut', () => {
  it('refuses nothing when no benchmark is running', () => {
    for (const action of actions) {
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: false,
          triggeredByAccelerator: true,
          onBenchmarkView: true,
        })
      ).toBe(false);
    }
  });

  it('never refuses a mouse click on the menu item, even mid-run on the benchmark window', () => {
    for (const action of actions) {
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: true,
          triggeredByAccelerator: false,
          onBenchmarkView: true,
        })
      ).toBe(false);
    }
  });

  it('refuses spawn and quit accelerators from any window while a run is live', () => {
    for (const action of ['spawn', 'quit'] as const) {
      for (const onBenchmarkView of [true, false]) {
        expect(
          shouldRefuseShortcut({
            action,
            benchmarkRunning: true,
            triggeredByAccelerator: true,
            onBenchmarkView,
          })
        ).toBe(true);
      }
    }
  });

  it('refuses close, navigate and reload accelerators only on the window showing the run', () => {
    for (const action of ['close', 'navigate', 'reload'] as const) {
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: true,
          triggeredByAccelerator: true,
          onBenchmarkView: true,
        })
      ).toBe(true);
      expect(
        shouldRefuseShortcut({
          action,
          benchmarkRunning: true,
          triggeredByAccelerator: true,
          onBenchmarkView: false,
        })
      ).toBe(false);
    }
  });
});

describe('isBenchmarkViewUrl', () => {
  it('recognises the hash-router benchmark route with or without a query or subpath', () => {
    expect(isBenchmarkViewUrl('file:///Applications/Goose.app/index.html#/benchmark')).toBe(true);
    expect(isBenchmarkViewUrl('http://localhost:5173/#/benchmark?tier=sb-7')).toBe(true);
    expect(isBenchmarkViewUrl('file:///x/index.html#/benchmark/live')).toBe(true);
  });

  it('rejects every other route, including ones that merely contain the word', () => {
    expect(isBenchmarkViewUrl('file:///x/index.html#/')).toBe(false);
    expect(isBenchmarkViewUrl('file:///x/index.html#/settings')).toBe(false);
    expect(isBenchmarkViewUrl('file:///x/index.html#/benchmarks')).toBe(false);
    expect(isBenchmarkViewUrl('file:///x/benchmark/index.html')).toBe(false);
    expect(isBenchmarkViewUrl('')).toBe(false);
  });
});
