import { useState, useSyncExternalStore } from 'react';
import { ChevronDown, ChevronUp, Loader2, RotateCcw, X } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../lz';
import { ToneBanner } from './studio';
import {
  latestRestoreLine,
  publishRestoreLine,
  retryRestore,
  subscribeRestoreLine,
  type RestoreLine,
  type RestoreReason,
  type RestoreWhat,
} from './mlxRestore';

/**
 * The one line that says what a relaunch is bringing back (mlxRestore.ts) — the Engine tab shows it
 * as a banner, the composer as its strip, the tray as its first line. Restoring: amber, with where.
 * Failed: red, goose's reason, Try again and Dismiss. Nothing to restore: nothing shown.
 */

const i18n = defineMessages({
  label: { id: 'mlxRestore.label', defaultMessage: 'Restore' },
  restoring: {
    id: 'mlxRestore.restoring',
    defaultMessage:
      'Restoring {model} {kind, select, single {on this Mac} split {across your Macs} other {on {peer}}}…',
  },
  failed: {
    id: 'mlxRestore.failed',
    defaultMessage:
      'Could not restore {model} {kind, select, single {on this Mac} split {across your Macs} other {on {peer}}}: {reason}',
  },
  unreadable: {
    id: 'mlxRestore.unreadable',
    defaultMessage: 'Could not read what served before the relaunch: {reason}',
  },
  linkDown: {
    id: 'mlxRestore.linkDown',
    defaultMessage: 'LeanZero Link is not connected ({detail})',
  },
  stoppedEarly: { id: 'mlxRestore.stoppedEarly', defaultMessage: 'it stopped before it served' },
  otherMac: { id: 'mlxRestore.otherMac', defaultMessage: 'the other Mac' },
  waitingPrevious: {
    id: 'mlxRestore.waitingPrevious',
    defaultMessage:
      'The previous split is still shutting down on {node} — goose restores it when that finishes',
  },
  tryAgain: { id: 'mlxRestore.tryAgain', defaultMessage: 'Try again' },
  dismiss: { id: 'mlxRestore.dismiss', defaultMessage: 'Dismiss' },
  details: { id: 'mlxRestore.details', defaultMessage: 'Details' },
  hideDetails: { id: 'mlxRestore.hideDetails', defaultMessage: 'Hide details' },
});

export function useRestoreLine(): RestoreLine {
  return useSyncExternalStore(subscribeRestoreLine, latestRestoreLine);
}

function reasonText(intl: IntlShape, reason: RestoreReason): string {
  switch (reason.code) {
    case 'linkDown':
      return intl.formatMessage(i18n.linkDown, { detail: reason.detail });
    case 'stoppedEarly':
      return intl.formatMessage(i18n.stoppedEarly);
    case 'said':
      return reason.text;
  }
}

function whereValues(intl: IntlShape, what: RestoreWhat) {
  return {
    model: what.modelId.split('/').pop() || what.modelId,
    kind: what.kind,
    peer: what.peerName ?? intl.formatMessage(i18n.otherMac),
  };
}

/** The line in words; null when there is nothing to say. */
export function restoreLineText(intl: IntlShape, line: RestoreLine): string | null {
  if (line.phase === 'idle') return null;
  if (line.phase === 'restoring') {
    return line.waitingOn
      ? intl.formatMessage(i18n.waitingPrevious, { node: line.waitingOn })
      : intl.formatMessage(i18n.restoring, whereValues(intl, line.what));
  }
  const reason = reasonText(intl, line.reason);
  return line.what
    ? intl.formatMessage(i18n.failed, { ...whereValues(intl, line.what), reason })
    : intl.formatMessage(i18n.unreadable, { reason });
}

/** What stands behind a failed line's words (pids, command lines) — shown only behind Details. */
export function restoreLineDetail(line: RestoreLine): string | null {
  if (line.phase !== 'failed' || line.reason.code !== 'said') return null;
  return line.reason.detail ?? null;
}

/**
 * Details / Hide details for a failed line that carries a detail, and the detail itself: the
 * toggle sits with the line's actions, the text on its own full-width row below the words.
 */
export function useRestoreDetails(line: RestoreLine) {
  const intl = useIntl();
  const [open, setOpen] = useState(false);
  const detail = restoreLineDetail(line);
  const toggle =
    detail == null ? null : (
      <Button
        size="sm"
        variant="secondary"
        icon={open ? <ChevronUp /> : <ChevronDown />}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        data-testid="mlx-restore-details"
      >
        {intl.formatMessage(open ? i18n.hideDetails : i18n.details)}
      </Button>
    );
  const panel =
    detail != null && open ? (
      <p className="basis-full break-all font-mono text-lz-mono" data-testid="mlx-restore-detail">
        {detail}
      </p>
    ) : null;
  return { toggle, panel };
}

/** Try again / Dismiss under a failed restore. */
export function RestoreActions({ variant = 'secondary' }: { variant?: 'secondary' | 'ghost' }) {
  const intl = useIntl();
  return (
    <span className="flex shrink-0 items-center gap-2">
      <Button
        size="sm"
        variant={variant}
        icon={<RotateCcw />}
        onClick={retryRestore}
        data-testid="mlx-restore-retry"
      >
        {intl.formatMessage(i18n.tryAgain)}
      </Button>
      <Button
        size="sm"
        variant={variant}
        icon={<X />}
        onClick={() => publishRestoreLine({ phase: 'idle' })}
        data-testid="mlx-restore-dismiss"
      >
        {intl.formatMessage(i18n.dismiss)}
      </Button>
    </span>
  );
}

/** The Engine tab's banner. */
export function MlxRestoreBanner() {
  const intl = useIntl();
  const line = useRestoreLine();
  const { toggle, panel } = useRestoreDetails(line);
  const text = restoreLineText(intl, line);
  if (text == null) return null;
  return (
    <div className="flex flex-col gap-2">
      <ToneBanner
        tone={line.phase === 'failed' ? 'err' : 'warn'}
        live={line.phase === 'restoring'}
        label={intl.formatMessage(i18n.label)}
        text={text}
        testId="mlx-restore"
        action={
          line.phase === 'failed' ? (
            <span className="flex shrink-0 items-center gap-2">
              {toggle}
              <RestoreActions />
            </span>
          ) : (
            <Loader2 aria-hidden className="size-4 shrink-0 animate-spin" />
          )
        }
      />
      {panel}
    </div>
  );
}
