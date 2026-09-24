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
 * A remote single — the single engine on a Link peer, this Mac's chat routed to it: amber while it
 * mounts there, red when it failed, and once it serves exactly the single engine's colours from the
 * peer engine's own live read (grey before the first one lands).
 */
export function remotePhase(state: string, activity: MlxActivity | null): EnginePhase {
  if (state === 'mounting') return 'loading';
  if (state === 'failed') return 'failed';
  if (state === 'off') return 'unloaded';
  return activity ? activityPhase(activity) : 'idle';
}

/**
 * The distributed run. Admission held by the memory watchdog is orange whatever the run state says.
 * While it is up, `activity` — rank 0's own `/v1/status` read through the single engine's
 * `mlxActivity` — decides exactly as it does for the single engine (reading blue, writing green,
 * queued orange). Without a live read, `serving` (the supervisor's "a request is in flight") is the
 * working green: the supervisor's counters cannot tell reading from writing.
 */
export function runPhase(
  state: string,
  admissionOpen: boolean,
  activity: MlxActivity | null = null
): EnginePhase {
  if (state === 'failed') return 'failed';
  const up = state === 'ready' || state === 'serving';
  if (!admissionOpen && up) return 'held';
  if (up && activity) return activityPhase(activity);
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
