import type { GooseServeLease, GooseServeLeaseRegistry } from './gooseServeLeaseRegistry';
import type { Logger } from './gooseServe';

export interface DesktopLifetime {
  link_host_enabled: boolean;
  active_run_ids: string[];
  detached_runs: boolean;
}
export function parseDesktopLifetime(value: unknown): DesktopLifetime {
  const state = value as Partial<DesktopLifetime> | null;
  if (
    !state ||
    typeof state.link_host_enabled !== 'boolean' ||
    typeof state.detached_runs !== 'boolean' ||
    !Array.isArray(state.active_run_ids) ||
    state.active_run_ids.some((id) => typeof id !== 'string' || !id)
  )
    throw new Error('The backend returned an invalid lifetime state.');
  return state as DesktopLifetime;
}

/** One read loop per owned goosed. No renderer has to stay open to keep the owner alive. */
export class DesktopBackendLifetime {
  private stopped = false;
  private detached = false;
  private latest: DesktopLifetime | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private lastError: string | null = null;

  constructor(
    private readonly lease: GooseServeLease,
    private readonly registry: GooseServeLeaseRegistry,
    private readonly read: () => Promise<DesktopLifetime | 'legacy'>,
    private readonly logger: Logger
  ) {}

  start(): void {
    void this.registry.setApplicationHolders(this.lease, ['lifetime-check']);
    // acquireLocal must reserve its first pending window before an idle legacy result can release.
    this.timer = setTimeout(() => void this.refresh(), 0);
  }
  canDetach(): boolean {
    return this.detached;
  }
  snapshot(): DesktopLifetime | null {
    return this.latest;
  }
  stop(): void {
    this.stopped = true;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
  async refresh(): Promise<void> {
    if (this.stopped || this.lease.cleanedUp || this.lease.exited) return;
    try {
      const next = await this.read();
      if (this.stopped || this.lease.cleanedUp || this.lease.exited) return;
      if (next === 'legacy') {
        // Capability removal after an affirmative response is not proof that detached work ended.
        if (this.detached) throw new Error('The backend lost its detached-run capability.');
        await this.registry.setApplicationHolders(this.lease, []);
        this.stop();
        return;
      }
      this.latest = next;
      this.detached ||= next.detached_runs;
      this.lastError = null;
      const holders = [
        ...(this.detached ? ['controller-runtime'] : []),
        ...(next.link_host_enabled ? ['link-host'] : []),
        ...next.active_run_ids.map((id) => `run:${id}`),
      ];
      await this.registry.setApplicationHolders(this.lease, holders);
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      if (detail !== this.lastError)
        this.logger.error(
          'Cannot verify Goose backend lifetime; retaining the application lease:',
          detail
        );
      this.lastError = detail;
      // Failed reads cannot prove idle. Preserve the previous holder, including the initial probe.
    }
    if (!this.stopped && !this.lease.cleanedUp && !this.lease.exited)
      this.timer = setTimeout(() => void this.refresh(), 1000);
  }
}
