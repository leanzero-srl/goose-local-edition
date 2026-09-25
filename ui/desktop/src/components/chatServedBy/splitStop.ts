import {
  foreignOwner,
  type MlxDistributedEvent,
  type MlxDistributedStatus,
} from '../../acp/mlx-distributed';
import { ownsTheMac } from '../leanzero-swarm/mlxDistributed';

/**
 * THE SPLIT THAT SERVED CHAT STOPPED — read from the supervisor's own event stream, never from a
 * state word alone (Q-81). E2E #2: a rank on Work's Mac Studio ran out of memory mid-run; the
 * supervisor wrote `watchdogWarn` ("Work’s Mac Studio: … available 3.9 GiB of 96.0 GiB"),
 * `rankDied` (node "Work’s Mac Studio", "… the pair cannot serve"), `streamWithoutDone` ("4
 * in-flight request(s) cut by the rankDied") and `stopped`, and left `state: failed` with the Mac
 * handed back to the single engine. Every chat surface then said "No model is mounted" and offered
 * to mount the SINGLE engine — nothing named the split, or why it stopped.
 *
 * The claim holds only between the event that ended the split and the next launch: a split that
 * never became `ready` served nothing, a stop the user asked for is not a failure, and a run that
 * owns the Mac again (a restart, "Start the split again") ends the claim.
 */

/** What stopped it, in the terms a person can act on. */
export type SplitStopCause = 'memory' | 'frozen' | 'ended';

export interface SplitStop {
  /** The Mac whose part stopped, by the name the split carries; null = the supervisor named none. */
  mac: string | null;
  cause: SplitStopCause;
  /** That Mac's memory as the watchdog last sampled it (GiB) — only for the split as it is now. */
  availableGb: number | null;
  totalGb: number | null;
  /** When the event that ended it was written (ms since the epoch). */
  atMs: number;
  /** The supervisor's own words — Details only, never the headline. */
  raw: string;
  /** Every Mac the split ran on, by name. */
  macs: string[];
  /** The HF model it served. */
  modelId: string | null;
}

/**
 * The events that END a run which had been serving (supervisor.rs `monitor` → `RunOutcome::Failed`
 * and the watchdog's `Critical`). `startFailed` and `breakerOpen` are not here: they end a RESTART,
 * after the event that stopped the split — which is what gets named.
 */
const ENDING_KINDS = new Set([
  'rankDied',
  'rankFrozen',
  'hang',
  'localNetworkBlocked',
  'watchdogCritical',
]);
const MEMORY_KINDS = new Set(['watchdogWarn', 'watchdogCritical']);

function splitMacs(status: MlxDistributedStatus): string[] {
  const running = status.nodes.map((n) => n.name);
  return running.length > 0 ? running : (status.config?.nodes ?? []).map((n) => n.name);
}

/**
 * The watchdog writes its event with no `node` and the Mac's name first ("<node>: kernel pressure
 * …", supervisor.rs `monitor`): the Mac is the known name its message starts with — matched
 * against the split's own names, never guessed from free text.
 */
function namedMac(event: MlxDistributedEvent, macs: string[]): string | null {
  if (event.node) return event.node;
  return macs.find((name) => event.message.startsWith(`${name}: `)) ?? null;
}

function causeOf(death: MlxDistributedEvent, memoryWarned: boolean): SplitStopCause {
  if (death.kind === 'watchdogCritical' || memoryWarned) return 'memory';
  if (death.kind === 'rankFrozen' || death.kind === 'hang') return 'frozen';
  return 'ended';
}

/**
 * The split that served and then stopped. `atMs` is the moment asked about (a turn's time): the
 * split must have been `ready` at or before it; the event that stopped it must come at or before
 * `deathByMs` — a refusal written after the stop passes its own time for both, an answer that was
 * CUT passes its start and no bound (it began while the split served). `atMs` null = as it is NOW,
 * which also requires that no run owns the Mac. null = no such stop is known: the split never
 * served, the user stopped it, it runs again, another window owns it, or the bounded event list no
 * longer reaches back that far.
 */
export function splitStopAt(
  status: MlxDistributedStatus | null,
  atMs: number | null,
  deathByMs: number | null = atMs
): SplitStop | null {
  if (!status || foreignOwner(status)) return null;
  if (atMs == null && ownsTheMac(status)) return null;
  const events = status.events;
  const readyBy = atMs ?? Number.POSITIVE_INFINITY;
  const deathBy = deathByMs ?? Number.POSITIVE_INFINITY;
  let ready = -1;
  for (let i = 0; i < events.length && events[i].atMs <= readyBy; i++) {
    if (events[i].kind === 'ready') ready = i;
  }
  if (ready < 0) return null;
  let deathAt = -1;
  for (let i = ready + 1; i < events.length && events[i].atMs <= deathBy; i++) {
    const kind = events[i].kind;
    if (kind === 'stopRequested') return null;
    if (ENDING_KINDS.has(kind)) {
      deathAt = i;
      break;
    }
  }
  if (deathAt < 0) return null;
  const death = events[deathAt];
  const macs = splitMacs(status);
  let mac = namedMac(death, macs);
  let memoryWarned = false;
  for (let i = ready + 1; i < deathAt; i++) {
    const event = events[i];
    if (!MEMORY_KINDS.has(event.kind)) continue;
    const warned = namedMac(event, macs);
    if (warned == null) continue;
    if (mac == null) mac = warned;
    if (warned === mac) memoryWarned = true;
  }
  const node = atMs == null && mac != null ? status.nodes.find((n) => n.name === mac) : undefined;
  return {
    mac,
    cause: causeOf(death, memoryWarned),
    availableGb: node?.availableMemoryGb ?? null,
    totalGb: node?.totalMemoryGb ?? null,
    atMs: death.atMs,
    raw: (atMs == null ? status.lastError : null) ?? death.message,
    macs,
    modelId: status.modelId ?? null,
  };
}
