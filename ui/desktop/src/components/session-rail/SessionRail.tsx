import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { X } from 'lucide-react';
import { useIntl } from '../../i18n';
import type { Message } from '../../types/message';
import { FOCUS, MOTION, SURFACE, Segmented, cx } from '../lz';
import { ChangesPanelBody, ChangesPill, changesRailMessages } from '../changes/ChangesRail';
import { sessionChanges } from '../changes/fileDiff';
import { LoopPanel } from '../loops/LoopPanel';
import { LoopPill } from '../loops/LoopPill';
import { loopWords } from '../loops/loopWords';
import { pillView, statusTicks, type PillView } from '../loops/loopView';
import type { SessionLoop } from '../loops/useSessionLoop';
import type { ControlResult } from '../loops/useSessionLoop';
import type { LoopControlAction } from '../loops/model';

export type RailTab = 'loop' | 'changes';

interface RailMemory {
  open: boolean;
  tab: RailTab;
}

const memoryKey = (sessionId: string) => `goose.sessionRail.${sessionId}`;
const endedSeenKey = (loopId: string) => `goose.sessionRail.endedSeen.${loopId}`;

/** Per-session open state and last tab (a per-viewer convenience: any storage failure is no memory). */
export function readRailMemory(sessionId: string): RailMemory | null {
  try {
    const raw = window.localStorage.getItem(memoryKey(sessionId));
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<RailMemory>;
    const tab = parsed.tab === 'loop' || parsed.tab === 'changes' ? parsed.tab : null;
    return tab ? { open: parsed.open === true, tab } : null;
  } catch {
    return null;
  }
}

function writeRailMemory(sessionId: string, memory: RailMemory) {
  try {
    window.localStorage.setItem(memoryKey(sessionId), JSON.stringify(memory));
  } catch {
    // The rail still works; it only forgets across remounts.
  }
}

function readEndedSeen(loopId: string): boolean {
  try {
    return window.localStorage.getItem(endedSeenKey(loopId)) === '1';
  } catch {
    return false;
  }
}

function writeEndedSeen(loopId: string) {
  try {
    window.localStorage.setItem(endedSeenKey(loopId), '1');
  } catch {
    // Forgotten across remounts only.
  }
}

/** A clock for what the person watches tick by (elapsed, "in 8m"): display only, decides nothing. */
function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    if (!ticking) return undefined;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [ticking]);
  return now;
}

/**
 * The chat's right rail (Q-190 + Q-228 L5): Changes and the session loop side by side, "where the
 * file modifications are". Collapsed it is up to two pills in the pane's corner — the loop's status
 * first, when the chat has a loop, then "N files +A −R". Opened it is ONE overlay panel with two
 * tabs, Loop and Changes; it floats over the chat's right side and never narrows the conversation.
 * Escape closes it and gives focus back to the pill that opened it. Open state and tab are
 * remembered per session.
 */
export default function SessionRail({
  sessionId,
  messages,
  loop,
  control,
  workingDir,
  className,
}: {
  sessionId: string;
  messages: readonly Message[];
  loop: SessionLoop;
  control: (action: LoopControlAction) => Promise<ControlResult>;
  workingDir?: string;
  className?: string;
}) {
  const intl = useIntl();
  const changes = useMemo(() => sessionChanges(messages), [messages]);
  const [memory, setMemory] = useState<RailMemory>(
    () => readRailMemory(sessionId) ?? { open: false, tab: 'changes' }
  );
  const [endedSeen, setEndedSeen] = useState(false);
  const panelId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const loopPillRef = useRef<HTMLButtonElement>(null);
  const changesPillRef = useRef<HTMLButtonElement>(null);
  const openedFrom = useRef<RailTab>('changes');

  useEffect(() => {
    setMemory(readRailMemory(sessionId) ?? { open: false, tab: 'changes' });
  }, [sessionId]);

  const loopId = loop.kind === 'loop' ? loop.loop.id : null;
  useEffect(() => {
    setEndedSeen(loopId ? readEndedSeen(loopId) : false);
  }, [loopId]);

  const ticking = loop.kind === 'loop' && statusTicks(loop.status);
  const nowMs = useNow(ticking);

  const update = (next: RailMemory) => {
    setMemory(next);
    writeRailMemory(sessionId, next);
  };

  const ended = loop.kind === 'loop' && loop.status === 'ended';
  const onLoopTab = memory.open && memory.tab === 'loop';
  useEffect(() => {
    if (ended && onLoopTab && loopId && !endedSeen) {
      writeEndedSeen(loopId);
      setEndedSeen(true);
    }
  }, [ended, onLoopTab, loopId, endedSeen]);

  useEffect(() => {
    if (memory.open) panelRef.current?.focus();
  }, [memory.open]);

  const pill: PillView | null =
    loop.kind === 'loop'
      ? ended && endedSeen
        ? null
        : pillView(loop.loop, { status: loop.status, reason: loop.reason }, nowMs)
      : loop.kind === 'unreadable'
        ? { tone: 'err', label: { message: loopWords.pillUnreadable } }
        : null;
  const hasChanges = changes.files.length > 0;
  const hasLoop = loop.kind === 'loop' || loop.kind === 'unreadable';

  if (!pill && !hasChanges && !(memory.open && hasLoop)) return null;

  const open = (tab: RailTab) => {
    openedFrom.current = tab;
    update({ open: true, tab });
  };
  const close = () => {
    update({ ...memory, open: false });
    requestAnimationFrame(() =>
      (openedFrom.current === 'loop' ? loopPillRef : changesPillRef).current?.focus()
    );
  };

  const tabOptions = [
    {
      value: 'loop' as const,
      label: intl.formatMessage(loopWords.tabLoop),
      testId: 'rail-tab-loop',
    },
    {
      value: 'changes' as const,
      label: intl.formatMessage(loopWords.tabChanges, { count: changes.files.length }),
      testId: 'rail-tab-changes',
    },
  ];

  return (
    <div
      data-testid="changes-rail"
      className={cx('pointer-events-none flex flex-col items-end', className)}
    >
      {!memory.open ? (
        <div
          data-testid="session-rail-pills"
          className="flex items-start justify-end gap-2 max-[560px]:flex-col max-[560px]:items-end"
        >
          {pill && (
            <LoopPill
              ref={loopPillRef}
              tone={pill.tone}
              label={pill.label}
              panelId={panelId}
              onOpen={() => open('loop')}
            />
          )}
          {hasChanges && (
            <ChangesPill
              ref={changesPillRef}
              changes={changes}
              panelId={panelId}
              onOpen={() => open('changes')}
            />
          )}
        </div>
      ) : (
        <div
          id={panelId}
          ref={panelRef}
          role="region"
          tabIndex={-1}
          aria-label={
            memory.tab === 'loop'
              ? intl.formatMessage(loopWords.tabLoop)
              : intl.formatMessage(changesRailMessages.title)
          }
          data-testid="session-rail-panel"
          data-tab={memory.tab}
          onKeyDown={(e) => {
            if (e.key !== 'Escape') return;
            if (!panelRef.current?.contains(e.target as Node)) return;
            close();
          }}
          className={cx(
            'pointer-events-auto flex max-h-[65vh] w-[min(32rem,calc(100vw-2rem))] flex-col overflow-hidden outline-none',
            SURFACE.overlay
          )}
        >
          <div
            className={cx('flex shrink-0 items-center gap-2 border-b px-3 py-2', SURFACE.hairline)}
          >
            <Segmented
              as="tabs"
              size="sm"
              aria-label={intl.formatMessage(loopWords.tabsLabel)}
              options={tabOptions}
              value={memory.tab}
              onChange={(tab) => update({ open: true, tab })}
            />
            <button
              type="button"
              data-testid="session-rail-close"
              aria-label={intl.formatMessage(
                memory.tab === 'loop' ? loopWords.closeLoop : changesRailMessages.close
              )}
              onClick={close}
              className={cx(
                'ml-auto inline-flex size-7 items-center justify-center rounded-lz-control text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink',
                FOCUS,
                MOTION
              )}
            >
              <X aria-hidden className="size-4" />
            </button>
          </div>
          <div role="tabpanel" className="flex min-h-0 flex-1 flex-col">
            {memory.tab === 'loop' ? (
              <LoopPanel
                sessionId={sessionId}
                state={loop}
                messages={messages}
                workingDir={workingDir}
                nowMs={nowMs}
                control={control}
              />
            ) : (
              <div data-testid="changes-rail-panel" className="flex min-h-0 flex-1 flex-col">
                <ChangesPanelBody changes={changes} />
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
