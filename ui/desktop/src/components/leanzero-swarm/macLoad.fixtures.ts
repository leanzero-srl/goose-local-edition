import type { JobBrief, NodeLoad } from '../../acp/leanzero-link';

/** A brief in the given state; the rest fixed so tests read the state only. */
export function brief(
  id: string,
  state: JobBrief['state'],
  over: Partial<JobBrief> = {}
): JobBrief {
  return {
    job_id: id,
    origin_node: 'n-studio',
    kind: 'shell',
    started_at: '2026-10-25T10:00:00Z',
    state,
    ...over,
  };
}

/** Own, inference and jobs all non-zero: cpu 0.35/core, 62% memory used, gpu 48%, user idle 7m. */
export function busyLoad(over: Partial<NodeLoad> = {}): NodeLoad {
  return {
    own: {
      user_idle_s: 420,
      user_active: false,
      cpu_load_per_core: 0.35,
      ncpu: 16,
      mem_free_pct: 38,
      mem_total_gb: 128,
      gpu_util_pct: 48,
      lid_closed_sleeps: false,
      heavy_slot_held: true,
    },
    inference: { engine_active: 2, router_slots_used: 1 },
    jobs: [
      brief('j-run', 'running', { heavy: true }),
      brief('j-wait', 'queued', { kind: 'prompt', origin_node: 'n-gabee' }),
      brief('j-done', 'done'),
    ],
    sampled_at: '2026-10-25T10:07:00Z',
    ...over,
  };
}
