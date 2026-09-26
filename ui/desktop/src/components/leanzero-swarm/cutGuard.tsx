import { useCallback, useEffect, useRef, useState, type ReactElement } from 'react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import {
  MLX_ENGINE_SNAPSHOT_CHANNEL,
  isMlxEngineSnapshot,
  type MlxEngineSnapshot,
} from '../../utils/mlxEngineMonitor';
import {
  clientName,
  inFlightWork,
  workCutBy,
  type InFlightWork,
  type MlxEngineKind,
} from '../../utils/mlxInFlight';
import { formatElapsed } from './mlxLiveStats';

/**
 * Every door that stops or replaces the engine serving chat asks FIRST while that engine holds
 * work, naming what it cuts — whose answer, how long it has run, how many tokens it has written
 * (Q-148). The read is main's, taken at the click (utils/mlxInFlight.ts), so the dialog speaks for
 * the moment the person acted, not for a poll ago. Nothing in flight = no dialog: the door acts.
 */

const i18n = defineMessages({
  title: { id: 'cutGuard.title', defaultMessage: 'Cut the answer being written?' },
  titleMany: {
    id: 'cutGuard.titleMany',
    defaultMessage: 'Cut {count} requests in flight?',
  },
  one: {
    id: 'cutGuard.one',
    defaultMessage: '{action} cuts the answer being written in “{name}” — {figures}.',
  },
  oneUnnamed: {
    id: 'cutGuard.oneUnnamed',
    defaultMessage: '{action} cuts a request goose could not name — {figures}.',
  },
  many: {
    id: 'cutGuard.many',
    defaultMessage: '{action} cuts {count} requests in flight ({names}) — the longest {figures}.',
  },
  manyUnnamed: {
    id: 'cutGuard.manyUnnamed',
    defaultMessage: '{action} cuts {count} requests in flight — the longest {figures}.',
  },
  written: {
    id: 'cutGuard.written',
    defaultMessage: '{elapsed} in, {tokens} tokens written',
  },
  writtenNoClock: { id: 'cutGuard.writtenNoClock', defaultMessage: '{tokens} tokens written' },
  reading: {
    id: 'cutGuard.reading',
    defaultMessage: '{elapsed} in, still reading its {tokens}-token prompt',
  },
  readingNoFigure: {
    id: 'cutGuard.readingNoFigure',
    defaultMessage: 'still reading its prompt',
  },
  keep: { id: 'cutGuard.keep', defaultMessage: 'Keep it writing' },
});

/** The work's figures: elapsed and tokens written (or the prompt it is still reading). */
export function workFiguresText(intl: IntlShape, work: InFlightWork): string {
  const elapsed = work.elapsedS != null ? formatElapsed(work.elapsedS) : null;
  if (work.reading) {
    return work.promptTokens != null && elapsed
      ? intl.formatMessage(i18n.reading, {
          elapsed,
          tokens: intl.formatNumber(work.promptTokens),
        })
      : intl.formatMessage(i18n.readingNoFigure);
  }
  const tokens = intl.formatNumber(work.tokens);
  return elapsed
    ? intl.formatMessage(i18n.written, { elapsed, tokens })
    : intl.formatMessage(i18n.writtenNoClock, { tokens });
}

/** "Stop the split cuts the answer being written in “Jira…” — 39m 15s in, 24,228 tokens written." */
export function cutMessage(intl: IntlShape, action: string, work: InFlightWork): string {
  const figures = workFiguresText(intl, work);
  const names = work.clients.map(clientName);
  if (work.requests <= 1) {
    return names[0]
      ? intl.formatMessage(i18n.one, { action, name: names[0], figures })
      : intl.formatMessage(i18n.oneUnnamed, { action, figures });
  }
  return names.length > 0
    ? intl.formatMessage(i18n.many, {
        action,
        count: work.requests,
        names: intl.formatList(
          names.map((n) => `“${n}”`),
          { type: 'conjunction' }
        ),
        figures,
      })
    : intl.formatMessage(i18n.manyUnnamed, { action, count: work.requests, figures });
}

export function cutTitle(intl: IntlShape, work: InFlightWork): string {
  return work.requests > 1
    ? intl.formatMessage(i18n.titleMany, { count: work.requests })
    : intl.formatMessage(i18n.title);
}

type SnapshotBridge = {
  mlxEngineActivity?: () => Promise<MlxEngineSnapshot>;
  on?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
  off?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
};

function bridge(): SnapshotBridge | undefined {
  return (window as unknown as { electron?: SnapshotBridge }).electron;
}

/** main's read NOW; null when there is no bridge or the read failed (then nothing is claimed). */
export async function readMainSnapshot(): Promise<MlxEngineSnapshot | null> {
  const read = bridge()?.mlxEngineActivity;
  if (!read) return null;
  try {
    return await read();
  } catch {
    return null;
  }
}

/** The work a door that stops `engines` would cut, read at the click. */
export async function readWorkCutBy(
  engines: readonly MlxEngineKind[]
): Promise<InFlightWork | null> {
  return workCutBy(await readMainSnapshot(), engines);
}

/** The work in flight as main pushes each read — for what a card SAYS before any click. */
export function useInFlightWork(): InFlightWork | null {
  const [work, setWork] = useState<InFlightWork | null>(null);
  useEffect(() => {
    let alive = true;
    void readMainSnapshot().then((s) => alive && setWork(inFlightWork(s)));
    const onPush = (_event: unknown, ...args: unknown[]) => {
      if (alive && isMlxEngineSnapshot(args[0])) setWork(inFlightWork(args[0]));
    };
    bridge()?.on?.(MLX_ENGINE_SNAPSHOT_CHANNEL, onPush);
    return () => {
      alive = false;
      bridge()?.off?.(MLX_ENGINE_SNAPSHOT_CHANNEL, onPush);
    };
  }, []);
  return work;
}

interface PendingCut {
  title: string;
  message: string;
  action: string;
  cancel: string;
  run: () => void;
}

/** A door that asks even with nothing in flight (the split's Stop): its own title and words. */
export interface PlainAsk {
  title: string;
  message: string;
  cancel: string;
}

export type CutGuard = (
  engines: readonly MlxEngineKind[],
  action: string,
  run: () => void,
  plain?: PlainAsk
) => Promise<void>;

/**
 * `guard(engines, action, run, plain?)`: runs `run` at once when the engines it stops hold no work
 * (or asks `plain`'s question, for a door that always asks), else asks first with the cut named.
 * `dialog` is the one ConfirmationModal the caller renders; `action` is the door's own words
 * ("Stop the split"), the dialog's confirm label too.
 */
export function useCutGuard(): { guard: CutGuard; dialog: ReactElement } {
  const intl = useIntl();
  const [pending, setPending] = useState<PendingCut | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const guard = useCallback<CutGuard>(
    async (engines, action, run, plain) => {
      const work = await readWorkCutBy(engines);
      if (!alive.current) return;
      if (work) {
        setPending({
          title: cutTitle(intl, work),
          message: cutMessage(intl, action, work),
          action,
          cancel: intl.formatMessage(i18n.keep),
          run,
        });
        return;
      }
      if (plain) {
        setPending({ ...plain, action, run });
        return;
      }
      run();
    },
    [intl]
  );
  const dialog = (
    <ConfirmationModal
      isOpen={pending != null}
      title={pending?.title ?? ''}
      message={pending?.message ?? ''}
      confirmLabel={pending?.action}
      cancelLabel={pending?.cancel}
      confirmVariant="destructive"
      onConfirm={() => {
        const run = pending?.run;
        setPending(null);
        run?.();
      }}
      onCancel={() => setPending(null)}
    />
  );
  return { guard, dialog };
}
