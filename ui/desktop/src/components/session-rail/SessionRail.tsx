import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
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
import { markEndedSeen, onOpenLoopRailRequest, useEndedSeen } from '../loops/loopRailRequest';
import { useNow } from '../loops/useNow';
import { ContextPanel, contextRailWords } from '../contextRail/ContextPanel';
import { onOpenContextRailRequest } from '../contextRail/contextRailRequest';

export type RailTab = 'loop' | 'changes' | 'context';

interface RailMemory {
  open: boolean;
  tab: RailTab;
}

const memoryKey = (sessionId: string) => `goose.sessionRail.${sessionId}`;

/** Per-session open state and last tab (a per-viewer convenience: any storage failure is no memory). */
export function readRailMemory(sessionId: string): RailMemory | null {
  try {
    const raw = window.localStorage.getItem(memoryKey(sessionId));
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<RailMemory>;
    const tab =
      parsed.tab === 'loop' || parsed.tab === 'changes' || parsed.tab === 'context'
        ? parsed.tab
        : null;
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
  onPillsHeight,
  onSend,
  className,
}: {
  sessionId: string;
  messages: readonly Message[];
  loop: SessionLoop;
  control: (action: LoopControlAction) => Promise<ControlResult>;
  workingDir?: string;
  /**
   * The collapsed pills' height (0 while none show, or the panel is open): the conversation starts
   * below them, so they never sit on its first message (Q-315).
   */
  onPillsHeight: (px: number) => void;
  /** Sends a message in this chat (the Context tab's Compact now). */
  onSend?: (text: string) => void;
  className?: string;
}) {
  const intl = useIntl();
  const changes = useMemo(() => sessionChanges(messages), [messages]);
  const [memory, setMemory] = useState<RailMemory>(
    () => readRailMemory(sessionId) ?? { open: false, tab: 'changes' }
  );
  const panelId = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const loopPillRef = useRef<HTMLButtonElement>(null);
  const changesPillRef = useRef<HTMLButtonElement>(null);
  const openedFrom = useRef<RailTab>('changes');

  useEffect(() => {
    setMemory(readRailMemory(sessionId) ?? { open: false, tab: 'changes' });
  }, [sessionId]);

  const loopId = loop.kind === 'loop' ? loop.loop.id : null;
  const endedSeen = useEndedSeen(loopId);

  const ticking = loop.kind === 'loop' && statusTicks(loop.status);
  const nowMs = useNow(ticking);

  const update = (next: RailMemory) => {
    setMemory(next);
    writeRailMemory(sessionId, next);
  };

  const ended = loop.kind === 'loop' && loop.status === 'ended';
  const onLoopTab = memory.open && memory.tab === 'loop';
  useEffect(() => {
    if (ended && onLoopTab && loopId && !endedSeen) markEndedSeen(loopId);
  }, [ended, onLoopTab, loopId, endedSeen]);

  // The composer's loop chip opens the rail on Loop (§8.1): one loop surface, two doors into it.
  useEffect(
    () =>
      onOpenLoopRailRequest((requested) => {
        if (requested !== sessionId) return false;
        openedFrom.current = 'loop';
        const next: RailMemory = { open: true, tab: 'loop' };
        setMemory(next);
        writeRailMemory(sessionId, next);
        return true;
      }),
    [sessionId]
  );

  // Q-357: the meter menu's "See what it keeps" and the compaction card's "What was kept" open
  // the rail on Context.
  useEffect(
    () =>
      onOpenContextRailRequest((requested) => {
        if (requested !== sessionId) return false;
        const next: RailMemory = { open: true, tab: 'context' };
        setMemory(next);
        writeRailMemory(sessionId, next);
        return true;
      }),
    [sessionId]
  );

  const refocus = useRef(false);
  useEffect(() => {
    if (memory.open) {
      panelRef.current?.focus();
    } else if (refocus.current) {
      refocus.current = false;
      (openedFrom.current === 'loop' ? loopPillRef : changesPillRef).current?.focus();
    }
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

  const [pillsRow, setPillsRow] = useState<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    if (!pillsRow) {
      onPillsHeight(0);
      return;
    }
    const report = () => onPillsHeight(pillsRow.getBoundingClientRect().height);
    report();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(report);
    observer.observe(pillsRow);
    return () => observer.disconnect();
  }, [pillsRow, onPillsHeight]);

  if (!pill && !hasChanges && !(memory.open && (hasLoop || memory.tab === 'context'))) return null;

  const open = (tab: RailTab) => {
    openedFrom.current = tab;
    update({ open: true, tab });
  };
  const close = () => {
    refocus.current = true;
    update({ ...memory, open: false });
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
    {
      value: 'context' as const,
      label: intl.formatMessage(contextRailWords.tab),
      testId: 'rail-tab-context',
    },
  ];

  return (
    <div
      data-testid="changes-rail"
      className={cx('pointer-events-none flex flex-col items-end', className)}
    >
      {!memory.open ? (
        <div
          ref={setPillsRow}
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
              : memory.tab === 'context'
                ? intl.formatMessage(contextRailWords.tab)
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
              aria-label={intl.formatMessage(contextRailWords.tabsLabel)}
              options={tabOptions}
              value={memory.tab}
              onChange={(tab) => update({ open: true, tab })}
            />
            <button
              type="button"
              data-testid="session-rail-close"
              aria-label={intl.formatMessage(
                memory.tab === 'loop'
                  ? loopWords.closeLoop
                  : memory.tab === 'context'
                    ? contextRailWords.close
                    : changesRailMessages.close
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
            ) : memory.tab === 'context' ? (
              <ContextPanel
                sessionId={sessionId}
                refreshKey={String(messages.length)}
                onSend={onSend}
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
