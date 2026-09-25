import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStartResponse, MlxDistributedStatus } from '../../acp/mlx-distributed';
import type {
  MlxRemoteSingleStartResponse,
  MlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import type { LinkState } from '../../acp/leanzero-link';
import type { MlxServingIntent, MlxServingIntentRead } from '../../acp/mlx-serving-intent';
import type { MlxRestoreReport } from '../../utils/mlxRestoreReport';
import { linkStateSettling } from '../../hooks/useLinkTrayReporter';
import { ownsTheMac } from './mlxDistributed';
import { singleLoad } from './mlxLiveStats';

/**
 * RESTORE ON RELAUNCH: every app update or relaunch stops goosed, and with it what served chat. At
 * launch, goose's record of what the owner last started and did not stop (`servingIntent`) is
 * brought back through the SAME start a click makes — Mount (the memory gate and Make room apply),
 * the remote single's start once LeanZero Link has reconnected by itself, the saved split's start
 * (its preflight applies) — and followed to serving or to a named failure. What is happening is one
 * line the Engine tab, the composer and the tray all show; nothing claims more than the status says.
 */

export interface RestoreWhat {
  kind: 'single' | 'remoteSingle' | 'split';
  modelId: string;
  /** remoteSingle only: the Mac by its one name, as goose recorded it at the start. */
  peerName: string | null;
}

/** The facts and starts a restore reads and makes — the renderer's ACP calls, injected. */
export interface RestoreDeps {
  readIntent(): Promise<MlxServingIntentRead>;
  singleStatus(): Promise<MlxEngineStatus>;
  mount(modelId: string): Promise<void>;
  remoteStatus(): Promise<MlxRemoteSingleStatus>;
  remoteStart(peer: string, modelId: string): Promise<MlxRemoteSingleStartResponse>;
  /** null = this goose offers no split. */
  distributedStatus(): Promise<MlxDistributedStatus | null>;
  distributedStart(): Promise<MlxDistributedStartResponse>;
  /** null = this goose offers no LeanZero Link. */
  linkState(): Promise<LinkState | null>;
  /** One status-poll interval. */
  wait(): Promise<void>;
}

/** Why a restore did not bring the thing back — the parts the line puts in words. */
export type RestoreReason =
  | { code: 'linkDown'; detail: string }
  | { code: 'stoppedEarly' }
  | { code: 'said'; text: string };

/**
 * A route that could not reach the peer because LeanZero Link is still coming up: another try
 * waits one poll for the roster, at most this many times. A UI retry budget (a few seconds of a
 * reconnect), never a bound on model work; the last refusal is what the failure says.
 */
export const REMOTE_START_TRIES = 15;
const TRANSIENT_REMOTE = new Set(['linkNotConnected', 'unknownPeer', 'peerUnreachable']);
/** The split's states on its way up; anything else ends the wait. */
const SPLIT_COMING_UP = new Set(['preflight', 'starting', 'loading']);

function whatOf(intent: MlxServingIntent): RestoreWhat | null {
  if (intent.kind !== 'single' && intent.kind !== 'remoteSingle' && intent.kind !== 'split') {
    return null;
  }
  return { kind: intent.kind, modelId: intent.modelId, peerName: intent.peerName ?? null };
}

function sameIntent(a: MlxServingIntent | null, b: MlxServingIntent): boolean {
  return a != null && a.kind === b.kind && a.modelId === b.modelId && a.peer === b.peer;
}

function linkDetail(state: LinkState): string {
  const reconnect = state.reconnect;
  if (reconnect?.state === 'failed' || reconnect?.state === 'skipped') return reconnect.reason;
  return state.lastError ?? state.auth.state;
}

/** null once Link is connected; else why it is not (after its own reconnect settled). */
async function linkConnected(deps: RestoreDeps): Promise<string | null> {
  for (;;) {
    const state = await deps.linkState();
    if (!state) return 'this goose does not offer LeanZero Link';
    if (state.auth.state === 'connected') return null;
    if (!linkStateSettling(state)) return linkDetail(state);
    await deps.wait();
  }
}

type Outcome = { served: true } | { served: false; reason: RestoreReason };
const said = (text: string): Outcome => ({ served: false, reason: { code: 'said', text } });

async function restoreSingle(deps: RestoreDeps, modelId: string): Promise<Outcome> {
  await deps.mount(modelId);
  for (;;) {
    const status = await deps.singleStatus();
    if (status.state === 'running' && status.modelId === modelId) return { served: true };
    if (status.state === 'failed') {
      return said(status.lastError ?? status.gateMessage ?? status.probeError ?? 'failed');
    }
    // A start the sidecar is measuring (Make room runs before the state flips) is still the mount.
    if (status.state !== 'mounting' && singleLoad(status) == null) {
      return { served: false, reason: { code: 'stoppedEarly' } };
    }
    await deps.wait();
  }
}

async function restoreRemote(deps: RestoreDeps, peer: string, modelId: string): Promise<Outcome> {
  const down = await linkConnected(deps);
  if (down) return { served: false, reason: { code: 'linkDown', detail: down } };
  let response = await deps.remoteStart(peer, modelId);
  for (let tries = 1; !response.started && tries < REMOTE_START_TRIES; tries++) {
    if (!TRANSIENT_REMOTE.has(response.refusal?.code ?? '')) break;
    await deps.wait();
    response = await deps.remoteStart(peer, modelId);
  }
  if (!response.started) return said(response.refusal?.message ?? 'refused');
  for (;;) {
    const status = await deps.remoteStatus();
    if (status.state === 'ready') return { served: true };
    if (status.state === 'failed') return said(status.lastError ?? 'failed');
    // `reconnecting`: the route stands and its Mac is not answering yet (a relaunch settling) —
    // still coming up, never "it stopped before it served".
    if (status.state !== 'mounting' && status.state !== 'reconnecting') {
      return { served: false, reason: { code: 'stoppedEarly' } };
    }
    await deps.wait();
  }
}

async function restoreSplit(deps: RestoreDeps, modelId: string): Promise<Outcome> {
  // The split's other Macs are reached over LeanZero Link when it is set up that way; a start
  // before the launch reconnect settles would be refused for no reason of its own.
  const link = await deps.linkState();
  if (link && link.auth.state !== 'connected' && linkStateSettling(link)) {
    const down = await linkConnected(deps);
    if (down) return { served: false, reason: { code: 'linkDown', detail: down } };
  }
  const response = await deps.distributedStart();
  if (!response.started) return said(response.refusal?.message ?? 'refused');
  for (;;) {
    const status = await deps.distributedStatus();
    if (!status) return { served: false, reason: { code: 'stoppedEarly' } };
    if (ownsTheMac(status) && (status.state === 'ready' || status.state === 'serving')) {
      return status.modelId == null || status.modelId === modelId
        ? { served: true }
        : said(`the saved split serves ${status.modelId}`);
    }
    if (status.state === 'failed') return said(status.lastError ?? 'failed');
    if (!SPLIT_COMING_UP.has(status.state)) {
      return status.lastError
        ? said(status.lastError)
        : { served: false, reason: { code: 'stoppedEarly' } };
    }
    await deps.wait();
  }
}

/** Already serving what the record names (another window, or goosed kept it): nothing to do. */
async function alreadyServing(deps: RestoreDeps, intent: MlxServingIntent): Promise<boolean> {
  if (intent.kind === 'single') {
    const status = await deps.singleStatus();
    return (
      (status.state === 'running' || status.state === 'mounting') &&
      status.modelId === intent.modelId
    );
  }
  if (intent.kind === 'remoteSingle') {
    const status = await deps.remoteStatus();
    return (
      status.state !== 'off' && status.peer === intent.peer && status.modelId === intent.modelId
    );
  }
  const status = await deps.distributedStatus();
  return status != null && ownsTheMac(status) && status.modelId === intent.modelId;
}

export type RestoreResult =
  | { phase: 'idle' }
  | { phase: 'failed'; what: RestoreWhat | null; reason: RestoreReason };

/**
 * Bring back what served before the relaunch. `onRestoring` fires once the restore actually starts
 * (not for nothing to do, nor for something already serving). A start the owner stopped while it
 * came up (the record is gone) ends quietly: that stop was a choice, not a failure.
 */
export async function restoreServing(
  deps: RestoreDeps,
  onRestoring: (what: RestoreWhat) => void
): Promise<RestoreResult> {
  const read = await deps.readIntent();
  if (read.error)
    return { phase: 'failed', what: null, reason: { code: 'said', text: read.error } };
  const intent = read.intent;
  if (!intent) return { phase: 'idle' };
  const what = whatOf(intent);
  if (!what) {
    return {
      phase: 'failed',
      what: null,
      reason: {
        code: 'said',
        text: `goose recorded a kind this build does not know: ${intent.kind}`,
      },
    };
  }
  if (await alreadyServing(deps, intent)) return { phase: 'idle' };
  onRestoring(what);
  let outcome: Outcome;
  try {
    outcome =
      what.kind === 'single'
        ? await restoreSingle(deps, what.modelId)
        : what.kind === 'remoteSingle'
          ? await restoreRemote(deps, intent.peer ?? '', what.modelId)
          : await restoreSplit(deps, what.modelId);
  } catch (error) {
    outcome = said(error instanceof Error ? error.message : String(error));
  }
  if (outcome.served) return { phase: 'idle' };
  const now = await deps.readIntent().catch(() => null);
  if (now && !now.error && !sameIntent(now.intent, intent)) return { phase: 'idle' };
  return { phase: 'failed', what, reason: outcome.reason };
}

// ---------------------------------------------------------------------------------------------
// The one line every surface shows, and what MAIN is told.
// ---------------------------------------------------------------------------------------------

export type RestoreLine =
  | { phase: 'idle' }
  | { phase: 'restoring'; what: RestoreWhat }
  /** `what` null = the record itself could not be read. */
  | { phase: 'failed'; what: RestoreWhat | null; reason: RestoreReason };

let current: RestoreLine = { phase: 'idle' };
const listeners = new Set<() => void>();

function reportToMain(line: RestoreLine): void {
  const report = (
    window as unknown as { electron?: { mlxRestoreReport?: (r: MlxRestoreReport | null) => void } }
  ).electron?.mlxRestoreReport;
  report?.(toRestoreReport(line));
}

/** English for main's tray: the reason's own words, the codes spelled out. */
function reasonText(reason: RestoreReason): string {
  switch (reason.code) {
    case 'linkDown':
      return `LeanZero Link is not connected (${reason.detail})`;
    case 'stoppedEarly':
      return 'it stopped before it served';
    case 'said':
      return reason.text;
  }
}

export function toRestoreReport(line: RestoreLine): MlxRestoreReport | null {
  if (line.phase === 'idle') return null;
  return {
    phase: line.phase,
    kind: line.what?.kind ?? null,
    modelId: line.what?.modelId ?? null,
    peerName: line.what?.peerName ?? null,
    reason: line.phase === 'failed' ? reasonText(line.reason) : null,
  };
}

export function latestRestoreLine(): RestoreLine {
  return current;
}

export function subscribeRestoreLine(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function publishRestoreLine(line: RestoreLine): void {
  current = line;
  reportToMain(line);
  for (const listener of listeners) listener();
}

let running: Promise<void> | null = null;
let lastDeps: RestoreDeps | null = null;

/**
 * A failed line whose engine serves now says something false — measured on 3.0.31: "Could not
 * restore … on Work's Mac Studio" stayed up beside the Studio's engine serving this Mac's chat. The
 * Engine view hands its reads here on every change; a line whose model serves the way it names is
 * cleared, never left as a claim the tile below contradicts.
 */
export function settleRestoreLine(serving: {
  single: MlxEngineStatus | null;
  remote: MlxRemoteSingleStatus | null;
  distributed: MlxDistributedStatus | null;
}): void {
  const line = current;
  if (line.phase !== 'failed' || !line.what) return;
  const { kind, modelId } = line.what;
  const served =
    kind === 'single'
      ? serving.single?.state === 'running' && serving.single.modelId === modelId
      : kind === 'remoteSingle'
        ? serving.remote?.state === 'ready' && serving.remote.modelId === modelId
        : serving.distributed != null &&
          ownsTheMac(serving.distributed) &&
          (serving.distributed.state === 'ready' || serving.distributed.state === 'serving') &&
          serving.distributed.modelId === modelId;
  if (served) publishRestoreLine({ phase: 'idle' });
}

/** Run one restore and keep the line current; a second call while one runs joins it. */
export function runRestore(deps: RestoreDeps): Promise<void> {
  if (running) return running;
  lastDeps = deps;
  running = restoreServing(deps, (what) => publishRestoreLine({ phase: 'restoring', what }))
    .then((result) => publishRestoreLine(result))
    .catch((error: unknown) =>
      publishRestoreLine({
        phase: 'failed',
        what: current.phase === 'restoring' ? current.what : null,
        reason: { code: 'said', text: error instanceof Error ? error.message : String(error) },
      })
    )
    .finally(() => {
      running = null;
    });
  return running;
}

/** The line's Try again: the same restore, with the calls the launch used. */
export function retryRestore(): void {
  if (lastDeps) void runRestore(lastDeps);
}
