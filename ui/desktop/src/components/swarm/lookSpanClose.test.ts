import { describe, it, expect } from 'vitest';
import { foldSupervision } from './useSwarmRun';

/**
 * A LOOK ENDS THREE WAYS, and the fold knew two.
 *
 * `judge_look` (it returned), `judge_look_abandoned` (the call finished underneath it) and
 * `judge_look_failed` (the judge's OWN model call failed — swarm.rs:16454). The third was never folded,
 * so its span stayed open forever and the fleet strip kept labelling a node "Reading · <task>" for the
 * rest of the run — attached to whatever it picked up next.
 */
type Ev = Record<string, unknown>;
const dispatched: Ev = {
  event: 'judge_look_dispatched',
  ts: '2026-08-29T10:00:02Z',
  task_id: 'alpha',
};

describe('a judge look always closes', () => {
  it('stays OPEN while the probe is genuinely still running — the control', () => {
    expect(foldSupervision([dispatched] as never)).toHaveLength(1);
  });

  for (const terminal of ['judge_look', 'judge_look_abandoned', 'judge_look_failed']) {
    it(`closes on ${terminal}`, () => {
      const spans = foldSupervision([
        dispatched,
        { event: terminal, ts: '2026-08-29T10:00:20Z', task_id: 'alpha' },
      ] as never);
      expect(spans, `${terminal} must close the look`).toHaveLength(0);
    });
  }

  it('closes only the look it names, not another task\'s', () => {
    const spans = foldSupervision([
      dispatched,
      { event: 'judge_look_dispatched', ts: '2026-08-29T10:00:03Z', task_id: 'beta' },
      { event: 'judge_look_failed', ts: '2026-08-29T10:00:20Z', task_id: 'alpha' },
    ] as never);
    expect(spans).toHaveLength(1);
  });
});
