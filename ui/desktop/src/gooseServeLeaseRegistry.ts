import type { GooseServeExitSignal, GooseServeResult, GooseServeStop, Logger } from './gooseServe';

export const GOOSE_SERVE_EXITED_USER_MESSAGE =
  "This window's Goose backend stopped. Close this window and open a new chat to start a new backend. If this keeps happening, restart Goose Desktop.";

export interface GooseServeLease {
  /** 'local' = the goosed this app spawned — one per app, shared by every window (Q-257). */
  kind: 'local' | 'external';
  /** The local goosed's pid; null for an external backend (or a spawn that never got one). */
  pid: number | null;
  acpUrl: string;
  secretKey: string;
  cleanup: () => Promise<GooseServeStop | void>;
  windowIds: Set<number>;
  /** Windows handed this lease by `acquireLocal` that have not attached (or given it back) yet. */
  pendingWindows: number;
  cleanedUp: boolean;
  exited: boolean;
  exitCode: number | null;
  exitSignal: GooseServeExitSignal;
}

export class GooseServeLeaseRegistry {
  private leasesByWindowId = new Map<number, GooseServeLease>();
  // Stops already under way. A window's `closed` releases its lease and starts the stop without
  // waiting (nothing there can), which removes the lease from the map — so the app's quit must
  // still find and wait on it here, or it exits while goosed is mid-teardown (Q-223).
  private stopping = new Set<Promise<GooseServeStop>>();
  // The app's ONE local goosed (Q-257). Split discovery needs the goosed that holds the LeanZero
  // Link mesh; a second goosed on this Mac is refused the mesh while the first one's daemon lives,
  // and never asks again after it is gone — so every window shares this one instead of spawning
  // its own. A new one is started only once this one is stopping or has exited.
  private local: GooseServeLease | null = null;
  private localStart: Promise<GooseServeLease | null> | null = null;

  constructor(private readonly logger: Logger) {}

  create(result: GooseServeResult, secretKey: string): GooseServeLease {
    const lease: GooseServeLease = {
      kind: 'local',
      pid: result.process.pid ?? null,
      acpUrl: result.acpUrl,
      secretKey,
      cleanup: result.cleanup,
      windowIds: new Set<number>(),
      pendingWindows: 0,
      cleanedUp: false,
      exited: false,
      exitCode: null,
      exitSignal: null,
    };

    const markExited = ({
      code,
      signal,
      logUnexpected,
    }: {
      code?: number | null;
      signal?: GooseServeExitSignal;
      logUnexpected: boolean;
    }) => {
      const firstExit = !lease.exited;
      lease.exited = true;
      if (code !== undefined) {
        lease.exitCode = code;
      }
      if (signal !== undefined) {
        lease.exitSignal = signal;
      }

      if (logUnexpected && firstExit && !lease.cleanedUp) {
        this.logger.error('Goose ACP server exited unexpectedly', {
          code: lease.exitCode,
          signal: lease.exitSignal,
          windowIds: [...lease.windowIds],
        });
      }
    };

    this.local = lease;

    result.process.once('exit', (code, signal) => {
      markExited({ code, signal, logUnexpected: true });
    });

    if (result.hasExited()) {
      const exitDetails = result.getExitDetails();
      markExited({ code: exitDetails.code, signal: exitDetails.signal, logUnexpected: false });
    }

    return lease;
  }

  createExternal(
    acpUrl: string,
    secretKey: string,
    cleanup: () => Promise<void> = async () => undefined
  ): GooseServeLease {
    return {
      kind: 'external',
      pid: null,
      acpUrl,
      secretKey,
      cleanup,
      windowIds: new Set<number>(),
      pendingWindows: 0,
      cleanedUp: false,
      exited: false,
      exitCode: null,
      exitSignal: null,
    };
  }

  /** The app's local goosed while it still serves — neither stopping nor exited — else null. */
  liveLocal(): GooseServeLease | null {
    const lease = this.local;
    return lease && !lease.cleanedUp && !lease.exited ? lease : null;
  }

  /**
   * The local goosed a new window attaches to: the live one, else ONE start shared by every window
   * asking meanwhile. The start first waits for every stop under way: a goosed still tearing down
   * holds the mesh daemon, and a goosed started beside it is refused the mesh for good (Q-257,
   * Q-223). `start` resolves null when it failed (it has told the user); the next window tries anew.
   * The caller then either attaches a window (`attachWindow`) or gives the lease back
   * (`releaseUnattached`): until one of those, the lease counts as held, so a sibling window's failed
   * creation cannot stop the goosed this one is about to use.
   */
  async acquireLocal(
    start: () => Promise<GooseServeLease | null>
  ): Promise<GooseServeLease | null> {
    const lease =
      this.liveLocal() ??
      (await (this.localStart ??= (async () => {
        try {
          if (this.stopping.size > 0) {
            this.logger.info(
              `A window needs a goose serve backend: waiting for ${this.stopping.size} stopping backend(s) to exit first, so the new one can hold the LeanZero Link mesh`
            );
            await Promise.all([...this.stopping]);
          }
          return await start();
        } finally {
          this.localStart = null;
        }
      })()));
    if (lease) lease.pendingWindows += 1;
    return lease;
  }

  /** A lease `acquireLocal` handed out whose window was never made: stop it only if nobody holds it. */
  async releaseUnattached(lease: GooseServeLease): Promise<void> {
    if (lease.pendingWindows > 0) lease.pendingWindows -= 1;
    if (lease.windowIds.size === 0 && lease.pendingWindows === 0) await this.cleanupLease(lease);
  }

  get(windowId: number): GooseServeLease | null {
    return this.leasesByWindowId.get(windowId) ?? null;
  }

  getAcpUrl(windowId: number): string | null {
    const lease = this.get(windowId);
    if (!lease) {
      return null;
    }
    if (lease.exited) {
      throw new Error(GOOSE_SERVE_EXITED_USER_MESSAGE);
    }
    return lease.acpUrl;
  }

  getSecretKey(windowId: number): string | null {
    const lease = this.get(windowId);
    if (!lease) {
      return null;
    }
    if (lease.exited) {
      throw new Error(GOOSE_SERVE_EXITED_USER_MESSAGE);
    }
    return lease.secretKey;
  }

  attachWindow(windowId: number, lease: GooseServeLease) {
    if (lease.pendingWindows > 0) lease.pendingWindows -= 1;
    lease.windowIds.add(windowId);
    this.leasesByWindowId.set(windowId, lease);
  }

  async releaseWindow(windowId: number) {
    const lease = this.leasesByWindowId.get(windowId);
    this.leasesByWindowId.delete(windowId);

    if (!lease) {
      return;
    }

    lease.windowIds.delete(windowId);
    // A window `acquireLocal` handed this lease to and that has not attached yet holds it too (the
    // same rule `releaseUnattached` keeps): the last ATTACHED window closing must not stop the goosed
    // a window being made is about to use.
    if (lease.windowIds.size > 0 || lease.pendingWindows > 0) return;
    this.logger.info(
      `Window ${windowId} was the last one using goose serve (pid ${lease.pid ?? '?'}); stopping it`
    );
    await this.cleanupLease(lease);
  }

  async cleanupLease(lease: GooseServeLease): Promise<GooseServeStop> {
    if (lease.cleanedUp) {
      return 'exited';
    }

    lease.cleanedUp = true;
    for (const windowId of lease.windowIds) {
      this.leasesByWindowId.delete(windowId);
    }
    lease.windowIds.clear();

    const stop: Promise<GooseServeStop> = lease.cleanup().then(
      (ended) => ended ?? 'exited',
      (error) => {
        this.logger.error('Failed to cleanup goose serve backend:', error);
        return 'abandoned';
      }
    );
    this.stopping.add(stop);
    try {
      return await stop;
    } finally {
      this.stopping.delete(stop);
    }
  }

  /** A backend is still attached to a window, or one is still stopping. */
  hasBackendsToStop(): boolean {
    return this.uniqueLeases().length > 0 || this.stopping.size > 0;
  }

  /**
   * Stop every attached backend and wait for EVERY stop — these and any already under way. Returns
   * how many stops ended without their goosed exiting (gave up, or failed), so the quit's log says
   * what happened instead of claiming every backend exited.
   */
  async stopAllAndWait(): Promise<{ abandoned: number }> {
    const underWay = Promise.all([...this.stopping]);
    const [attached, earlier] = await Promise.all([this.cleanupAll(), underWay]);
    return { abandoned: [...attached, ...earlier].filter((e) => e === 'abandoned').length };
  }

  /** Every backend still serving a window — attached, not cleaned up, not exited. */
  liveLeases(): GooseServeLease[] {
    return this.uniqueLeases().filter((lease) => !lease.cleanedUp && !lease.exited);
  }

  activeLeaseCount(): number {
    return this.uniqueLeases().length;
  }

  cleanupAll(): Promise<GooseServeStop[]> {
    return Promise.all(this.uniqueLeases().map((lease) => this.cleanupLease(lease)));
  }

  /** Every lease a window holds, plus the local goosed while it is not stopped — even between its
   *  start and its first window attaching, so a quit then still stops it (Q-257). */
  private uniqueLeases(): GooseServeLease[] {
    const leases = new Set(this.leasesByWindowId.values());
    if (this.local && !this.local.cleanedUp) leases.add(this.local);
    return [...leases];
  }
}
