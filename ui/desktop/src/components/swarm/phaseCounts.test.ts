import { describe, it, expect } from 'vitest';
import { isFinishedWork } from './useSwarmRun';
import type { TodoState } from './useSwarmRun';

describe('the progress counter measures FINISHED work, not VERIFIED work', () => {
  // THE BUG THIS PINS, reconstructed from loop-ab-baseline's own event stream:
  //   plan_loaded 16:54:02 -> panel shows "Build 0/7"
  //   db 16:56:37 · entry-point 16:59:01 · ledger 17:00:46 · models 17:02:37 · frontend 17:07:14 · api 17:08:52
  //   ...six tasks done, panel still "Build 0/7" — for 15 minutes.
  //   replanned 17:09:46 -> panel finally reads "1/10".
  // The counter used `state === 'done'`, but a finished build task is deliberately 'unverified' (the app was
  // never run). The ONLY Build row born 'done' is `Re-planned +N tasks`. So the numerator counted REPLANS:
  // the one time it moved, it moved because the job got BIGGER.
  it('counts a built-but-unverified task as finished', () => {
    expect(isFinishedWork('unverified')).toBe(true);
    expect(isFinishedWork('done')).toBe(true);
  });

  it('does NOT count work that has not finished', () => {
    const notFinished: TodoState[] = ['pending', 'running', 'failed', 'judge_failed', 'blocked'];
    for (const s of notFinished) expect(isFinishedWork(s)).toBe(false);
  });

  it('reproduces the real run: 6 of 7 finished must read 6, not 0', () => {
    // the six that completed, plus one still running
    const states: TodoState[] = [
      'unverified', 'unverified', 'unverified', 'unverified', 'unverified', 'unverified', 'running',
    ];
    expect(states.filter(isFinishedWork).length).toBe(6);
  });

  it('a failed task is not progress — it still needs doing', () => {
    const states: TodoState[] = ['unverified', 'unverified', 'failed'];
    expect(states.filter(isFinishedWork).length).toBe(2);
  });
});
