import { describe, it, expect } from 'vitest';
import { buildPhaseTodo } from './useSwarmRun';

/**
 * A FAILED ROW MUST SAY WHY.
 *
 * `fail_descendants` now emits `error: "dependency 'x' failed"` on the cascade, but the `task_completed`
 * handler built the task state WITHOUT the error, so the detail line had nothing to render: a task
 * killed by a dead dependency looked identical to one that tried and failed on its own.
 */
type Ev = Record<string, unknown>;
const ev = (o: Ev) => o;

const base: Ev[] = [
  ev({ event: 'run_started', ts: '2026-08-29T09:00:00Z', pool: [{ id: 'n1', model_id: 'm/x' }] }),
  ev({
    event: 'plan_loaded',
    ts: '2026-08-29T09:00:01Z',
    tasks: [
      { id: 'parent', description: 'build the store', files: ['store.py'] },
      { id: 'child', description: 'use the store', files: ['app.py'], depends_on: ['parent'] },
    ],
  }),
  ev({ event: 'task_dispatched', ts: '2026-08-29T09:00:02Z', task_id: 'child', device: 'n1' }),
];

const rowFor = (events: Ev[], id: string) => {
  const todo = buildPhaseTodo(events as never, {}, { clarifyPending: false });
  const flat = (todo as unknown as Array<{ items?: Array<Record<string, unknown>> }>)
    .flatMap((p) => p.items ?? []);
  // Rows are phase-prefixed (`b-child` in the Build phase), not the bare task id.
  return flat.find((r) => String(r['id']).endsWith(id)) as Record<string, unknown> | undefined;
};

describe('a failed task keeps its reason', () => {
  it('renders the engine\'s cascade reason as the row detail', () => {
    const row = rowFor(
      [
        ...base,
        ev({
          event: 'task_completed',
          ts: '2026-08-29T09:01:00Z',
          task_id: 'child',
          device: 'n1',
          status: 'failed',
          error: "dependency 'parent' failed",
        }),
      ],
      'child'
    );
    expect(row).toBeDefined();
    expect(String(row?.['detail'] ?? '')).toContain("dependency 'parent' failed");
  });

  it('does not erase a recorded reason with a later reasonless completion', () => {
    const row = rowFor(
      [
        ...base,
        ev({
          event: 'task_completed',
          ts: '2026-08-29T09:01:00Z',
          task_id: 'child',
          device: 'n1',
          status: 'failed',
          error: "dependency 'parent' failed",
        }),
        ev({
          event: 'task_completed',
          ts: '2026-08-29T09:02:00Z',
          task_id: 'child',
          device: 'n1',
          status: 'failed',
        }),
      ],
      'child'
    );
    expect(String(row?.['detail'] ?? '')).toContain("dependency 'parent' failed");
  });
});
