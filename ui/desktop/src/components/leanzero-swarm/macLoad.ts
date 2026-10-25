import type { JobBrief, JobState, NodeLoad } from '../../acp/leanzero-link';

/**
 * A Mac's load as three bars, ONE derivation for every card that shows them (My Macs, the Nodes
 * page): OWN (what the Mac's owner and its hardware are doing), AI INFERENCE (what its engine
 * serves), JOBS (mesh jobs it runs). Pure and React-free so the numbers are tested without a DOM.
 *
 * OWN is the WORST of cpu, memory used and gpu, because the busiest resource is what decides
 * whether the Mac can take more work (the same reading admission uses: a Mac with free cpu but no
 * memory is full). The two count bars fill against COUNT_SCALE: four concurrent requests or jobs
 * read as a full bar, which is the most one engine or one heavy slot carries well in practice;
 * the exact count is always printed beside the bar, so the scale only decides the fill.
 */

export const COUNT_SCALE = 4;

/** Three solid, distinct hues (no tints): indigo own, pink inference, teal jobs. */
export const BAR_FILL = {
  own: 'bg-[#4F46E5]',
  inference: 'bg-[#DB2777]',
  jobs: 'bg-[#0D9488]',
} as const;

export type BarId = keyof typeof BAR_FILL;

export interface LoadBar {
  id: BarId;
  /** 0..100, the fill. */
  pct: number;
}

export interface OwnParts {
  cpuPct: number | null;
  memUsedPct: number | null;
  gpuPct: number | null;
  userActive: boolean;
  idleS: number | null;
}

function clampPct(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.max(0, Math.min(100, Math.round(v)));
}

export function ownParts(load: NodeLoad): OwnParts {
  const o = load.own;
  return {
    cpuPct: o.cpu_load_per_core == null ? null : clampPct(o.cpu_load_per_core * 100),
    memUsedPct: o.mem_free_pct == null ? null : clampPct(100 - o.mem_free_pct),
    gpuPct: o.gpu_util_pct == null ? null : clampPct(o.gpu_util_pct),
    userActive: o.user_active,
    idleS: o.user_idle_s == null ? null : Math.round(o.user_idle_s),
  };
}

export function inferenceCount(load: NodeLoad): number {
  return load.inference.engine_active + load.inference.router_slots_used;
}

/** Jobs still holding the Mac (queued or running); ended ones are listed but do not fill the bar. */
export function liveJobs(jobs: readonly JobBrief[]): JobBrief[] {
  return jobs.filter((j) => j.state === 'queued' || j.state === 'running');
}

export function loadBars(load: NodeLoad): LoadBar[] {
  const p = ownParts(load);
  const own = Math.max(p.cpuPct ?? 0, p.memUsedPct ?? 0, p.gpuPct ?? 0);
  return [
    { id: 'own', pct: own },
    { id: 'inference', pct: clampPct((inferenceCount(load) / COUNT_SCALE) * 100) },
    { id: 'jobs', pct: clampPct((liveJobs(load.jobs).length / COUNT_SCALE) * 100) },
  ];
}

export function isTerminal(state: JobState): boolean {
  return state !== 'queued' && state !== 'running';
}

/** The values the jobs tunables take; mirrors leanzero-link `UserActivePolicy::parse`. */
export type UserActivePolicy = 'allow' | 'last_resort' | 'refuse';

export const USER_ACTIVE_POLICIES: readonly UserActivePolicy[] = ['allow', 'last_resort', 'refuse'];

/** goose config keys the jobs side reads (link.rs `jobs_config`, leanzero-link jobs/load). */
export const JOBS_KEYS = {
  allow: 'LEANZERO_LINK_ALLOW_REMOTE_JOBS',
  policy: 'LEANZERO_LINK_JOBS_USER_ACTIVE',
  idleS: 'LEANZERO_LINK_JOBS_USER_IDLE_S',
  minFree: 'LEANZERO_LINK_JOBS_MIN_FREE',
} as const;

/** The defaults the backend falls back to when a key is unset. */
export const JOBS_DEFAULTS = {
  allow: false,
  policy: 'last_resort' as UserActivePolicy,
  idleS: 300,
  minFree: 20,
};

export function parsePolicy(v: unknown): UserActivePolicy {
  const s = typeof v === 'string' ? v.trim().toLowerCase().replace('-', '_') : '';
  return (USER_ACTIVE_POLICIES as readonly string[]).includes(s)
    ? (s as UserActivePolicy)
    : JOBS_DEFAULTS.policy;
}

/** A config value as a number, or null when unset/unparseable (the backend's `config_f64`). */
export function parseNumber(v: unknown): number | null {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string' && v.trim() !== '') {
    const n = Number(v);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

export function parseBool(v: unknown): boolean {
  if (typeof v === 'boolean') return v;
  if (typeof v === 'string') return v.trim().toLowerCase() === 'true';
  return false;
}
