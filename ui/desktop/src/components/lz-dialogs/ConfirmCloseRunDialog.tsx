import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Panel, StatusDot, SURFACE, TYPE, cx } from '../lz';
import type { LiveRunRef, TurnInFlight } from '../../utils/closeGuard';

/**
 * The question main asks when a window holding a LIVE swarm run — or a chat whose prompt is in flight
 * on this window's connection (Q-490) — is closed (closeGuard.ts): the close ends the window's
 * connection and the run or the answer dies with it.
 * With a run it speaks of the run (naming any turns too); with only turns it speaks of the answer. Never window.confirm — a Studio overlay Panel with a warn StatusDot, the safe
 * action ("Keep running") focused and on Escape, the destructive one in the err tone.
 *
 * Modal for real: aria-modal on the dialog, Tab/Shift+Tab cycle inside it, Escape anywhere in the
 * window keeps the run, focus returns to where it was when the dialog goes away.
 */
const i18n = defineMessages({
  title: {
    id: 'confirmCloseRun.title',
    defaultMessage: 'A swarm run is live in this window',
  },
  body: {
    id: 'confirmCloseRun.body',
    defaultMessage:
      "Closing this window stops the run: the window's backend is released and the swarm engine under it is signalled to exit.",
  },
  runLine: {
    id: 'confirmCloseRun.runLine',
    defaultMessage: 'Run {runId} in {runDir}',
  },
  keepRunning: {
    id: 'confirmCloseRun.keepRunning',
    defaultMessage: 'Keep running',
  },
  stopAndClose: {
    id: 'confirmCloseRun.stopAndClose',
    defaultMessage: 'Stop run and close',
  },
  stopping: {
    id: 'confirmCloseRun.stopping',
    defaultMessage: 'Stopping the run…',
  },
  warning: {
    id: 'confirmCloseRun.warning',
    defaultMessage: 'Warning',
  },
  turnTitle: {
    id: 'confirmCloseRun.turnTitle',
    defaultMessage: 'A chat in this window is still answering',
  },
  turnBody: {
    id: 'confirmCloseRun.turnBody',
    defaultMessage:
      'Closing this window stops the answer: the chat was sent from this window, and its reply ends where it is.',
  },
  turnLine: {
    id: 'confirmCloseRun.turnLine',
    defaultMessage: 'Chat: {name}',
  },
  keepAnswering: {
    id: 'confirmCloseRun.keepAnswering',
    defaultMessage: 'Keep answering',
  },
  stopAnswerAndClose: {
    id: 'confirmCloseRun.stopAnswerAndClose',
    defaultMessage: 'Stop the answer and close',
  },
  stoppingAnswer: {
    id: 'confirmCloseRun.stoppingAnswer',
    defaultMessage: 'Stopping the answer…',
  },
});

export interface ConfirmCloseRunDialogProps {
  /** The live runs this window's renderer is watching — what the body names. */
  runs: LiveRunRef[];
  /** The chats with a prompt in flight on this window's connection. */
  turns: TurnInFlight[];
  /** Escape, or the secondary action: nothing happens, the window stays. */
  onKeepRunning: () => void;
  /** The err-tone action: main will close the window for real. */
  onStopAndClose: () => void;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function ConfirmCloseRunDialog({
  runs,
  turns,
  onKeepRunning,
  onStopAndClose,
}: ConfirmCloseRunDialogProps) {
  const intl = useIntl();
  const titleId = useId();
  const bodyId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const [stopping, setStopping] = useState(false);
  // A swarm run is the bigger thing to lose, so it leads the words whenever there is one.
  const words =
    runs.length > 0
      ? {
          title: i18n.title,
          body: i18n.body,
          keep: i18n.keepRunning,
          stop: i18n.stopAndClose,
          stopping: i18n.stopping,
        }
      : {
          title: i18n.turnTitle,
          body: i18n.turnBody,
          keep: i18n.keepAnswering,
          stop: i18n.stopAnswerAndClose,
          stopping: i18n.stoppingAnswer,
        };
  // Whoever had focus before the dialog, read at RENDER: autoFocus moves focus during commit, before
  // any effect runs, so an effect would only ever see the dialog's own button.
  const [previousFocus] = useState(() => document.activeElement);

  // …and they get it back when the dialog goes away (the window stayed, or is closing).
  useEffect(() => {
    return () => {
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
    };
  }, [previousFocus]);

  // Escape is "Keep running" wherever focus sits — captured at the window so a click on the scrim
  // that moved focus out of the dialog cannot strand the key.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || stopping) return;
      e.preventDefault();
      e.stopPropagation();
      onKeepRunning();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onKeepRunning, stopping]);

  const trapTab = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'Tab') return;
    const root = dialogRef.current;
    if (!root) return;
    const focusable = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE));
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    const outside = !(active instanceof Node) || !root.contains(active);
    if (e.shiftKey ? active === first || outside : active === last || outside) {
      e.preventDefault();
      (e.shiftKey ? last : first).focus();
    }
  };

  const stop = () => {
    if (stopping) return;
    setStopping(true);
    onStopAndClose();
  };

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 p-4"
      data-testid="confirm-close-run-scrim"
      // A click on the scrim must not pull focus out of the dialog; the buttons inside keep theirs.
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) e.preventDefault();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        data-testid="confirm-close-run-dialog"
        onKeyDown={trapTab}
        className="w-[460px] max-w-full"
      >
        <Panel className={SURFACE.overlay}>
          <div className="flex items-start gap-3">
            <StatusDot
              tone="warn"
              label={intl.formatMessage(i18n.warning)}
              size={10}
              className="mt-2"
            />
            <div className="flex min-w-0 flex-1 flex-col gap-2">
              <h2 id={titleId} className={TYPE.h1}>
                {intl.formatMessage(words.title)}
              </h2>
              <div id={bodyId} className="flex flex-col gap-2">
                <p className={TYPE.bodyMuted}>{intl.formatMessage(words.body)}</p>
                <ul className="flex flex-col gap-1" data-testid="confirm-close-run-runs">
                  {runs.map((run) => (
                    <li key={run.runDir} className={cx(TYPE.mono, 'break-all')}>
                      {intl.formatMessage(i18n.runLine, { runId: run.runId, runDir: run.runDir })}
                    </li>
                  ))}
                  {turns.map((turn) => (
                    <li key={turn.sessionId} className={cx(TYPE.body, 'break-all')}>
                      {intl.formatMessage(i18n.turnLine, {
                        name: turn.sessionName ?? turn.sessionId,
                      })}
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          </div>
          <div className="mt-5 flex justify-end gap-2">
            <Button variant="secondary" autoFocus disabled={stopping} onClick={onKeepRunning}>
              {intl.formatMessage(words.keep)}
            </Button>
            <Button
              variant="destructive"
              data-testid="confirm-close-run-stop"
              disabled={stopping}
              onClick={stop}
            >
              {intl.formatMessage(stopping ? words.stopping : words.stop)}
            </Button>
          </div>
        </Panel>
      </div>
    </div>
  );
}
