import { useEffect, useState } from 'react';
import { Check, Copy } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import type { MlxEngineStatus, MlxStrayListenerHolder } from '../../acp/mlx-engine';
import { defineMessages, useIntl } from '../../i18n';
import { Button, RADIUS, SURFACE, StatusDot, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';

const i18n = defineMessages({
  label: { id: 'strayListener.label', defaultMessage: 'Port {port} is taken' },
  holder: { id: 'strayListener.holder', defaultMessage: 'pid {pid} · {command}' },
  unreadableCommand: {
    id: 'strayListener.unreadableCommand',
    defaultMessage: 'command line unreadable',
  },
  ours: {
    id: 'strayListener.ours',
    defaultMessage: "This goose's own engine, left from an earlier run — nothing runs it now",
  },
  liveStarter: {
    id: 'strayListener.liveStarter',
    defaultMessage:
      "Not this goose's — pid {pid} ({command}) started it and still runs it: another goose on this Mac, or a terminal",
  },
  noMarker: {
    id: 'strayListener.noMarker',
    defaultMessage:
      "Not this goose's — it carries no goose engine mark (an older goose, or another program, started it)",
  },
  otherEngine: {
    id: 'strayListener.otherEngine',
    defaultMessage: "Not this goose's — a goose engine started for another engine or port",
  },
  otherUser: {
    id: 'strayListener.otherUser',
    defaultMessage: "Not this goose's — it runs as another user on this Mac",
  },
  initOrSelf: {
    id: 'strayListener.initOrSelf',
    defaultMessage: "This goose's own app listens on the engine port",
  },
  unreadable: {
    id: 'strayListener.unreadable',
    defaultMessage: "Not proven this goose's — who started it could not be read",
  },
  otherRule: { id: 'strayListener.otherRule', defaultMessage: "Not this goose's — {reason}" },
  stepMount: {
    id: 'strayListener.stepMount',
    defaultMessage: 'Next: start it again in Run it — the start stops this leftover first.',
  },
  stepQuit: {
    id: 'strayListener.stepQuit',
    defaultMessage: 'Next: quit what started it (pid {pid}), then start again in Run it.',
  },
  stepOtherPort: {
    id: 'strayListener.stepOtherPort',
    defaultMessage: 'Next: give the engine another port in its settings.',
  },
  stepKill: {
    id: 'strayListener.stepKill',
    defaultMessage: 'Next: stop it with this command, then start again in Run it.',
  },
  noneNamed: {
    id: 'strayListener.noneNamed',
    defaultMessage:
      'Something answers on port {port}, but no process listens there now — it may have just exited.',
  },
  unread: {
    id: 'strayListener.unread',
    defaultMessage:
      'Who holds port {port} could not be read ({error}). Unmount stops whatever listens on it.',
  },
  unnamed: {
    id: 'strayListener.unnamed',
    defaultMessage: 'A process this goose does not run listens on port {port} — Unmount stops it.',
  },
  copy: { id: 'strayListener.copy', defaultMessage: 'Copy' },
  copied: { id: 'strayListener.copied', defaultMessage: 'Copied' },
  copyLabel: { id: 'strayListener.copyLabel', defaultMessage: 'Copy the command {command}' },
});

const COMMAND_CHARS = 72;

/**
 * A command line short enough to read in a line: each path argument by its last segment (the uv
 * cache's `…/archive-v0/U_t/bin/python` is `python`), cut at a word budget. The whole line rides
 * the row's title.
 */
export function shortCommand(argv: readonly string[]): string {
  const words = argv.map((arg) => (arg.startsWith('/') ? (arg.split('/').pop() ?? arg) : arg));
  const line = words.join(' ');
  return line.length > COMMAND_CHARS ? `${line.slice(0, COMMAND_CHARS - 1).trimEnd()}…` : line;
}

/** The one next step for a port's holders — the same order Q-240's refused Mount names it in. */
export type StrayStep =
  | { kind: 'mount' }
  | { kind: 'quit'; pid: number }
  | { kind: 'otherPort' }
  | { kind: 'kill'; command: string };

export function strayStep(holders: readonly MlxStrayListenerHolder[]): StrayStep | null {
  if (holders.length === 0) return null;
  if (holders.every((h) => h.ours)) return { kind: 'mount' };
  const starter = holders.find((h) => !h.ours && h.liveStarterPid != null)?.liveStarterPid;
  if (starter != null) return { kind: 'quit', pid: starter };
  if (holders.some((h) => h.notOursRule === 'initOrSelf')) return { kind: 'otherPort' };
  // What a Mount would stop itself (ours) is left to it; the rest is named for the person.
  const pids = holders.filter((h) => !h.ours).map((h) => h.pid);
  return { kind: 'kill', command: `kill ${pids.join(' ')}` };
}

/** Whether the holder is this goose's, in words; its full finding rides the title. */
export function whoseWords(intl: IntlShape, holder: MlxStrayListenerHolder): string {
  if (holder.ours) return intl.formatMessage(i18n.ours);
  switch (holder.notOursRule) {
    case 'liveStarter':
      if (holder.liveStarterPid != null) {
        return intl.formatMessage(i18n.liveStarter, {
          pid: holder.liveStarterPid,
          command:
            holder.liveStarterArgv && holder.liveStarterArgv.length > 0
              ? shortCommand(holder.liveStarterArgv)
              : intl.formatMessage(i18n.unreadableCommand),
        });
      }
      break;
    case 'noMarker':
      return intl.formatMessage(i18n.noMarker);
    case 'otherEngine':
      return intl.formatMessage(i18n.otherEngine);
    case 'otherUser':
      return intl.formatMessage(i18n.otherUser);
    case 'initOrSelf':
      return intl.formatMessage(i18n.initOrSelf);
    case 'unreadable':
      return intl.formatMessage(i18n.unreadable);
  }
  return intl.formatMessage(i18n.otherRule, { reason: holder.notOursReason ?? '' });
}

function stepWords(intl: IntlShape, step: StrayStep): string {
  switch (step.kind) {
    case 'mount':
      return intl.formatMessage(i18n.stepMount);
    case 'quit':
      return intl.formatMessage(i18n.stepQuit, { pid: step.pid });
    case 'otherPort':
      return intl.formatMessage(i18n.stepOtherPort);
    case 'kill':
      return intl.formatMessage(i18n.stepKill);
  }
}

function CopyCommand({ command }: { command: string }) {
  const intl = useIntl();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = async () => {
    await navigator.clipboard.writeText(command);
    setCopied(true);
  };
  return (
    <div className="flex min-w-0 items-center gap-2">
      <code
        data-testid="stray-listener-command"
        className={cx('min-w-0 break-all px-2 py-1', SURFACE.inset, RADIUS.control, TYPE.mono)}
      >
        {command}
      </code>
      <Button
        size="sm"
        variant="secondary"
        icon={copied ? <Check /> : <Copy />}
        onClick={copy}
        aria-label={intl.formatMessage(i18n.copyLabel, { command })}
        data-testid="stray-listener-copy"
      >
        {intl.formatMessage(copied ? i18n.copied : i18n.copy)}
      </Button>
    </div>
  );
}

/**
 * The Engine panel's word on a port something this goose does not supervise listens on (Q-249):
 * who holds it — pid and a short command line —, whether it is this goose's own leftover, and the
 * one next step, from the same holders a refused Mount names (goose-sidecar `port_holder`).
 */
export function StrayListenerBanner({
  port,
  status,
}: {
  port: number;
  status: Pick<MlxEngineStatus, 'strayListenerHolders' | 'strayListenerHoldersError'>;
}) {
  const intl = useIntl();
  const holders = status.strayListenerHolders;
  const step = holders ? strayStep(holders) : null;
  const message = holders
    ? holders.length === 0
      ? intl.formatMessage(i18n.noneNamed, { port })
      : null
    : status.strayListenerHoldersError
      ? intl.formatMessage(i18n.unread, { port, error: status.strayListenerHoldersError })
      : intl.formatMessage(i18n.unnamed, { port });
  const label = intl.formatMessage(i18n.label, { port });
  return (
    <div
      role="status"
      data-testid="stray-listener"
      data-step={step?.kind}
      className={cx('flex flex-col gap-2 px-4 py-3', SURFACE.card)}
    >
      <div className="flex items-center gap-3">
        <StatusDot tone="warn" label={label} size={10} />
        <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}>{label}</span>
      </div>
      {message && <p className={cx('break-words', TYPE.body)}>{message}</p>}
      {holders && holders.length > 0 && (
        <ul className="flex flex-col gap-1.5">
          {holders.map((holder) => (
            <li
              key={holder.pid}
              data-testid="stray-listener-holder"
              data-ours={holder.ours}
              className="flex min-w-0 flex-col gap-0.5"
            >
              <span
                title={holder.argv.join(' ') || undefined}
                className={cx('min-w-0 break-all', TYPE.mono)}
              >
                {intl.formatMessage(i18n.holder, {
                  pid: holder.pid,
                  command:
                    holder.argv.length > 0
                      ? shortCommand(holder.argv)
                      : intl.formatMessage(i18n.unreadableCommand),
                })}
              </span>
              <span
                title={holder.notOursReason ?? undefined}
                className={cx(
                  'break-words text-lz-body',
                  holder.ours ? TONE_TEXT.ok : TONE_TEXT.warn
                )}
              >
                {whoseWords(intl, holder)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {step && (
        <p data-testid="stray-listener-step" className={cx(WEIGHT.semibold, TYPE.body)}>
          {stepWords(intl, step)}
        </p>
      )}
      {step?.kind === 'kill' && <CopyCommand command={step.command} />}
    </div>
  );
}
