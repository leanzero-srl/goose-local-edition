import { describe, expect, it } from 'vitest';
import { buildActivity } from './useSwarmRun';

/**
 * The plan_repaired FEED ROW, pinned against the engine's real event shape (972d81f53):
 * before/after carry tasks_owning_nothing / shared_files / module_package_collisions /
 * unassigned_endpoints — each either a list or a count — and actions is the list of fixes
 * the deterministic pass applied. Until this file existed the handler had zero fixtures,
 * so a key rename in the engine would have blanked the row silently (campaign review,
 * 2026-08-30).
 */
const BP1_SEQUENCE = [
  { event: 'phase', phase: 'open' },
  { event: 'slices_opened', count: 3, weights: [3, 2, 2], slices: ['core', 'web', 'store'] },
  { event: 'phase', phase: 'synthesis' },
  {
    event: 'plan_repaired',
    actions: ['drop owns-nothing task doc-pass', 'fold web.py into web/'],
    before: {
      tasks_owning_nothing: ['doc-pass'],
      shared_files: 2,
      module_package_collisions: ['web'],
      unassigned_endpoints: 3,
    },
    after: {
      tasks_owning_nothing: [],
      shared_files: 0,
      module_package_collisions: [],
      unassigned_endpoints: 0,
    },
  },
  { event: 'plan_loaded', tasks: [] },
];

describe('plan_repaired renders as a feed row from the engine event, never from a label', () => {
  it('a repair with actions is a good-tone row carrying every before→after count', () => {
    const { activity, verbose } = buildActivity(BP1_SEQUENCE);
    const row = activity.find((r) => r.text.startsWith('Plan repaired'));
    expect(row).toBeDefined();
    expect(row?.text).toBe('Plan repaired — 2 deterministic fixes');
    expect(row?.tone).toBe('good');
    expect(row?.sub).toContain('owning nothing 1→0');
    expect(row?.sub).toContain('shared files 2→0');
    expect(row?.sub).toContain('module/package collisions 1→0');
    expect(row?.sub).toContain('unassigned endpoints 3→0');
    // The verbose feed carries the same row — one source event, both timelines.
    expect(verbose.some((r) => r.text === 'Plan repaired — 2 deterministic fixes')).toBe(true);
  });

  it('zero actions reads as "no repair needed", info tone — a clean plan is not a green event', () => {
    const clean = BP1_SEQUENCE.map((e) =>
      e.event === 'plan_repaired' ? { ...e, actions: [] } : e
    );
    const { activity } = buildActivity(clean);
    const row = activity.find((r) => r.text === 'Plan needed no repair');
    expect(row).toBeDefined();
    expect(row?.tone).toBe('info');
  });

  it('an archived r2-shaped stream (no plan_repaired event) renders NO repair row at all', () => {
    const r2Shaped = [
      { event: 'phase', phase: 'open' },
      { event: 'phase', phase: 'synthesis' },
      { event: 'plan_loaded', tasks: [] },
      { event: 'task_dispatched', task_id: 'ledgerd-core' },
    ];
    const { activity, verbose } = buildActivity(r2Shaped);
    const repairRows = [...activity, ...verbose].filter(
      (r) => r.text.startsWith('Plan repaired') || r.text === 'Plan needed no repair'
    );
    expect(repairRows).toEqual([]);
  });
});
