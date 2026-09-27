import type { GooseServeExitSignal, GooseServeResult, Logger } from './gooseServe';

export const GOOSE_SERVE_EXITED_USER_MESSAGE =
  "This window's Goose backend stopped. Close this window and open a new chat to start a new backend. If this keeps happening, restart Goose Desktop.";

export interface GooseServeLease {
  acpUrl: string;
  secretKey: string;
  cleanup: () => Promise<void>;
  windowIds: Set<number>;
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
  private stopping = new Set<Promise<void>>();

  constructor(private readonly logger: Logger) {}

  create(result: GooseServeResult, secretKey: string): GooseServeLease {
    const lease: GooseServeLease = {
      acpUrl: result.acpUrl,
      secretKey,
      cleanup: result.cleanup,
      windowIds: new Set<number>(),
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
      acpUrl,
      secretKey,
      cleanup,
      windowIds: new Set<number>(),
      cleanedUp: false,
      exited: false,
      exitCode: null,
      exitSignal: null,
    };
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
    if (lease.windowIds.size === 0) {
      await this.cleanupLease(lease);
    }
  }

  async cleanupLease(lease: GooseServeLease) {
    if (lease.cleanedUp) {
      return;
    }

    lease.cleanedUp = true;
    for (const windowId of lease.windowIds) {
      this.leasesByWindowId.delete(windowId);
    }
    lease.windowIds.clear();

    const stop = lease.cleanup().catch((error) => {
      this.logger.error('Failed to cleanup goose serve backend:', error);
    });
    this.stopping.add(stop);
    try {
      await stop;
    } finally {
      this.stopping.delete(stop);
    }
  }

  /** A backend is still attached to a window, or one is still stopping. */
  hasBackendsToStop(): boolean {
    return this.uniqueLeases().length > 0 || this.stopping.size > 0;
  }

  /** Stop every attached backend and wait for EVERY stop — these and any already under way. */
  async stopAllAndWait() {
    await this.cleanupAll();
    await Promise.all([...this.stopping]);
  }

  /** Every backend still serving a window — attached, not cleaned up, not exited. */
  liveLeases(): GooseServeLease[] {
    return this.uniqueLeases().filter((lease) => !lease.cleanedUp && !lease.exited);
  }

  activeLeaseCount(): number {
    return this.uniqueLeases().length;
  }

  async cleanupAll() {
    await Promise.all(this.uniqueLeases().map((lease) => this.cleanupLease(lease)));
  }

  private uniqueLeases(): GooseServeLease[] {
    return [...new Set(this.leasesByWindowId.values())];
  }
}
