import type { Logger } from './gooseServe';
import type { GooseServeLeaseRegistry } from './gooseServeLeaseRegistry';

/** The two Electron quit events the hold answers; both carry a cancellable event. */
export type QuitDoor = 'before-quit' | 'will-quit';

export interface QuitEvent {
  preventDefault(): void;
}

export interface QuitHoldDeps {
  backends: Pick<
    GooseServeLeaseRegistry,
    'hasBackendsToStop' | 'activeLeaseCount' | 'stopAllAndWait'
  >;
  logger: Logger;
  /** `app.quit()` — called once every goosed has exited, to finish the quit that was held. */
  quit: () => void;
  /**
   * A window whose close guard would ASK before closing (it holds a live run, closeGuard.ts). Its
   * question must come before any goosed is stopped, so `before-quit` does not hold while one would
   * ask; the windows close first, the guard asks, and `will-quit` holds as before.
   */
  closeWouldAsk: () => boolean;
}

/**
 * Holds the app's quit until every goosed has EXITED (Q-223, Q-241).
 *
 * The hold starts at `before-quit`, the first event of every quit (Cmd+Q, osascript's quit Apple
 * Event, the tray's Quit, `app.quit()`, SIGTERM), BEFORE Electron closes a single window. Measured on
 * 3.0.65 (2026-09-28 00:58:22Z and 01:02:03Z): the quit Apple Event reached the first window's close
 * and the process exited voluntarily 4 ms later (unified log: "windowShouldClose: prevented close" →
 * CoreAnalytics "Entering exit handler"), so neither the window's `closed` nor `will-quit` ever ran —
 * main.log holds no quit line, goosed saw only its stdin close, and it tore down as an orphan with
 * the app already gone. A hold that sits at `will-quit` is reached only after every window closed;
 * one at `before-quit` stops goosed while the app is still whole, then lets the quit go on.
 *
 * `will-quit` holds too: a stop a window close started (the close guard's confirmed close, a quit the
 * guard asked about) is still waited for there.
 */
export class QuitHold {
  private stopping: Promise<void> | null = null;
  /** Set when the held quit is let go; a refused quit (the close guard) clears it. */
  private released = false;

  constructor(private readonly deps: QuitHoldDeps) {}

  /** Answer one quit event: hold it (preventDefault) while a goosed still has to exit. */
  onQuitEvent(door: QuitDoor, event: QuitEvent): 'held' | 'passed' {
    if (this.stopping) {
      // A second quit during the hold (Cmd+Q again, SIGTERM) waits on the same stop.
      event.preventDefault();
      return 'held';
    }
    if (this.released || !this.deps.backends.hasBackendsToStop()) return 'passed';
    if (door === 'before-quit' && this.deps.closeWouldAsk()) return 'passed';

    event.preventDefault();
    const { backends, logger } = this.deps;
    logger.info(
      `App quitting (${door}): waiting for ${backends.activeLeaseCount()} attached backend(s) and any stop already under way to exit`
    );
    this.stopping = backends.stopAllAndWait().then(
      ({ abandoned }) => this.release(abandoned),
      (error: unknown) => {
        logger.error('App quitting: stopping the goose serve backends failed:', error);
        this.release(null);
      }
    );
    return 'held';
  }

  /** The quit was refused after it was let go (a live run's close guard): the next quit holds again. */
  quitRefused(): void {
    if (this.stopping) return;
    this.released = false;
  }

  private release(abandoned: number | null): void {
    if (abandoned === 0) {
      this.deps.logger.info('App quitting: every goose serve backend has exited');
    } else if (abandoned !== null) {
      this.deps.logger.error(
        `App quitting: ${abandoned} goose serve backend(s) did not exit (logged above); quitting without them — their own stdin watch ends them once this process is gone`
      );
    }
    this.stopping = null;
    this.released = true;
    this.deps.quit();
  }
}
