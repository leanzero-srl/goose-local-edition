import type { EnginePhase } from '../lz/tokens';
import type { MlxActivity } from './mlxLiveStats';

/**
 * Which ENGINE PHASE (lz/tokens.ts, the one palette) each backend state is, for every MLX surface —
 * the state tile, the distributed node cards and start strip, the peer's hosting tile and the
 * menu-bar tray. Pure and React-free: main imports it for the tray. A state the backend adds after
 * this build maps to `idle` (a neutral claim) and keeps its own word on screen.
 */

/** The single engine's work while RUNNING, from its requests' own phases. */
export function activityPhase(activity: MlxActivity): EnginePhase {
  switch (activity) {
    case 'generating':
      return 'writing';
    case 'prefill':
      return 'reading';
    case 'queued':
      return 'held';
    case 'idle':
      return 'idle';
    case 'not_loaded':
      return 'unloaded';
  }
}

/**
 * The single engine: `state` null = not read yet (unreachable when the read failed). Running with
 * no live read yet is `idle` — the model is loaded, and nothing measured says it works.
 */
export function singlePhase(
  state: string | null,
  unreachable: boolean,
  activity: MlxActivity | null
): EnginePhase {
  if (state === null) return unreachable ? 'failed' : 'unloaded';
  switch (state) {
    case 'running':
      return activity ? activityPhase(activity) : 'idle';
    case 'mounting':
      return 'loading';
    case 'failed':
      return 'failed';
    case 'stopped':
      return 'unloaded';
    default:
      return 'idle';
  }
}

/**
 * The distributed run. `serving` is the supervisor's "a request is in flight" (rank 0's progress
 * reports inflight > 0) — the run reports no read/write split, so in-flight work is the working
 * green. Admission held by the memory watchdog is orange whatever the run state says.
 */
export function runPhase(state: string, admissionOpen: boolean): EnginePhase {
  if (state === 'failed') return 'failed';
  if (!admissionOpen && (state === 'ready' || state === 'serving')) return 'held';
  switch (state) {
    case 'preflight':
    case 'starting':
    case 'stopping':
      return 'loading';
    case 'ready':
      return 'idle';
    case 'serving':
      return 'writing';
    case 'stopped':
      return 'unloaded';
    default:
      return 'idle';
  }
}

/** One rank of the distributed run (supervisor NodeState). */
export function nodePhase(state: string): EnginePhase {
  switch (state) {
    case 'makingRoom':
    case 'warming':
    case 'preflight':
    case 'loading':
      return 'loading';
    case 'ready':
      return 'idle';
    case 'serving':
      return 'writing';
    case 'failed':
      return 'failed';
    case 'stopped':
      return 'unloaded';
    default:
      return 'idle';
  }
}

/**
 * A rank THIS Mac serves for another Mac (link_host: `loading` = spawned, not yet in the group;
 * `serving` = joined). The hosted rank reports no requests of its own, so a joined rank is the idle
 * grey — the requester's tile carries the work.
 */
export function hostingPhase(state: string): EnginePhase {
  if (state === 'loading') return 'loading';
  if (state === 'failed') return 'failed';
  return 'idle';
}
