import { describe, expect, it } from 'vitest';
import { foldSupervision } from './useSwarmRun';

/**
 * THE OMNI-JUDGE'S MID-STREAM PROBE HAD NO REPRESENTATION UNTIL IT CAME BACK.
 *
 * The engine emits `judge_look_dispatched` when it sends a supervisor to read a still-running call, and
 * `judge_look` when that supervisor returns. Only the CLOSING event was folded, so the whole open span — a
 * real generation, on a real node, for as long as it lasts — was invisible, and a look that never returned
 * was invisible forever. That is the exact failure this pair exists to surface: measured once as a
 * supervisor dying mid-look, followed by 2h56m of engine silence with two of three nodes idle.
 */
const T0 = '2026-08-29T10:00:00.000Z';

describe('foldSupervision — a dispatched look is open work', () => {
  it('opens a span on judge_look_dispatched', () => {
    expect(
      foldSupervision([{ event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 }])
    ).toStrictEqual([
      {
        kind: 'look',
        taskId: 'ledgerd-core',
        label: 'Reading · ledgerd-core',
        sinceMs: Date.parse(T0),
      },
    ]);
  });

  it('closes it when the look returns', () => {
    expect(
      foldSupervision([
        { event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 },
        { event: 'judge_look', task_id: 'ledgerd-core', verdict: 'ok' },
      ])
    ).toStrictEqual([]);
  });

  // ABANDONED IS NOT HUNG. The engine says so explicitly when the supervised call finishes first; without
  // folding it, every normal race would leave a span open and the strip would report phantom supervision.
  it('closes it when the engine abandons the look', () => {
    expect(
      foldSupervision([
        { event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 },
        { event: 'judge_look_abandoned', task_id: 'ledgerd-core' },
      ])
    ).toStrictEqual([]);
  });

  it('leaves the span open when the look never returns', () => {
    const open = foldSupervision([
      { event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 },
      { event: 'task_dispatched', task_id: 'ledgerd-cli' },
    ]);
    expect(open.map((s) => s.kind)).toStrictEqual(['look']);
  });

  it('closes it when the supervised task completes', () => {
    expect(
      foldSupervision([
        { event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 },
        { event: 'task_completed', task_id: 'ledgerd-core' },
      ])
    ).toStrictEqual([]);
  });

  // A task is probed mid-stream and judged afterwards. Sharing one map key would let the probe's close
  // retire the verdict's span, or the verdict's close retire the probe's.
  it('keeps the mid-stream look and the post-task judge apart', () => {
    const both = foldSupervision([
      { event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 },
      { event: 'judge_observed', task_id: 'ledgerd-core', ts: T0 },
    ]);
    expect(both.map((s) => s.kind).sort()).toStrictEqual(['judge', 'look']);
    expect(
      foldSupervision([
        { event: 'judge_look_dispatched', task_id: 'ledgerd-core', ts: T0 },
        { event: 'judge_observed', task_id: 'ledgerd-core', ts: T0 },
        { event: 'judge_verdict', task_id: 'ledgerd-core', verdict: 'ok' },
      ]).map((s) => s.kind)
    ).toStrictEqual(['look']);
  });
});
