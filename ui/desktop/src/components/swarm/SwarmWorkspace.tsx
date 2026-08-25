import React, { useEffect, useId, useRef, useState } from 'react';
import { MessageSquare, RadioTower } from 'lucide-react';
import { cn } from '../../utils';

export type SwarmWorkspaceTab = 'conversation' | 'run';

export const SWARM_WORKSPACE_MIN_WIDTH = 1080;

export function shouldSplitSwarmWorkspace({
  isLocal,
  present,
  inProgress,
  finished,
}: {
  isLocal: boolean;
  present: boolean;
  inProgress: boolean;
  finished: boolean;
}): boolean {
  return isLocal && present && inProgress && !finished;
}

export function nextWorkspaceTab(
  current: SwarmWorkspaceTab,
  key: string
): SwarmWorkspaceTab | null {
  if (key === 'Home') return 'conversation';
  if (key === 'End') return 'run';
  if (key === 'ArrowLeft' || key === 'ArrowUp') {
    return current === 'conversation' ? 'run' : 'conversation';
  }
  if (key === 'ArrowRight' || key === 'ArrowDown') {
    return current === 'conversation' ? 'run' : 'conversation';
  }
  return null;
}

export function SwarmWorkspace({
  conversation,
  run,
  initialTab = 'conversation',
  active = true,
}: {
  conversation: React.ReactNode;
  run: React.ReactNode;
  initialTab?: SwarmWorkspaceTab;
  active?: boolean;
}) {
  const [activeTab, setActiveTab] = useState<SwarmWorkspaceTab>(initialTab);
  const [isWide, setIsWide] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const conversationTabRef = useRef<HTMLButtonElement>(null);
  const runTabRef = useRef<HTMLButtonElement>(null);
  const id = useId();

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;

    const update = (width: number) => setIsWide(width >= SWARM_WORKSPACE_MIN_WIDTH);
    update(root.getBoundingClientRect().width);

    const observer = new ResizeObserver((entries) => {
      update(entries[0]?.contentRect.width ?? root.getBoundingClientRect().width);
    });
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!active) setActiveTab('conversation');
  }, [active]);

  const selectTab = (tab: SwarmWorkspaceTab, focus = false) => {
    setActiveTab(tab);
    if (focus) {
      const ref = tab === 'conversation' ? conversationTabRef : runTabRef;
      ref.current?.focus();
    }
  };

  const onTabKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    const next = nextWorkspaceTab(activeTab, event.key);
    if (!next) return;
    event.preventDefault();
    selectTab(next, true);
  };

  const conversationTabId = `${id}-conversation-tab`;
  const runTabId = `${id}-run-tab`;
  const conversationPanelId = `${id}-conversation-panel`;
  const runPanelId = `${id}-run-panel`;

  return (
    <div
      ref={rootRef}
      className="flex flex-1 min-h-0 flex-col"
      data-testid="swarm-workspace"
      data-layout={active ? (isWide ? 'wide' : 'narrow') : 'conversation'}
    >
      {active && !isWide && (
        <div
          role="tablist"
          aria-label="Active swarm workspace"
          className="mx-3 mb-2 grid h-10 shrink-0 grid-cols-2 border border-border-primary bg-background-primary"
          style={{ borderRadius: 4 }}
        >
          <button
            ref={conversationTabRef}
            id={conversationTabId}
            type="button"
            role="tab"
            aria-selected={activeTab === 'conversation'}
            aria-controls={conversationPanelId}
            tabIndex={activeTab === 'conversation' ? 0 : -1}
            onClick={() => selectTab('conversation')}
            onKeyDown={onTabKeyDown}
            className={cn(
              'flex items-center justify-center gap-2 border-r border-border-primary px-3 text-xs font-semibold outline-none',
              activeTab === 'conversation'
                ? 'bg-[#0b5bd3] text-white focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-white'
                : 'bg-background-primary text-text-primary hover:bg-background-secondary focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[#0b5bd3]'
            )}
          >
            <MessageSquare className="h-4 w-4" />
            Conversation
          </button>
          <button
            ref={runTabRef}
            id={runTabId}
            type="button"
            role="tab"
            aria-selected={activeTab === 'run'}
            aria-controls={runPanelId}
            tabIndex={activeTab === 'run' ? 0 : -1}
            onClick={() => selectTab('run')}
            onKeyDown={onTabKeyDown}
            className={cn(
              'flex items-center justify-center gap-2 px-3 text-xs font-semibold outline-none',
              activeTab === 'run'
                ? 'bg-[#0b5bd3] text-white focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-white'
                : 'bg-background-primary text-text-primary hover:bg-background-secondary focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[#0b5bd3]'
            )}
          >
            <RadioTower className="h-4 w-4" />
            Run
          </button>
        </div>
      )}

      <div
        className={cn(
          'flex flex-1 min-h-0',
          active && isWide && 'grid grid-cols-[minmax(360px,0.9fr)_minmax(520px,1.1fr)]'
        )}
      >
        <section
          id={conversationPanelId}
          role={!active || isWide ? 'region' : 'tabpanel'}
          aria-label={!active || isWide ? 'Conversation' : undefined}
          aria-labelledby={!active || isWide ? undefined : conversationTabId}
          hidden={active && !isWide && activeTab !== 'conversation'}
          className={cn(
            'flex min-h-0 min-w-0 flex-1 flex-col',
            active && isWide && 'border-r border-border-primary'
          )}
          data-testid="swarm-workspace-conversation"
        >
          {conversation}
        </section>
        <section
          id={runPanelId}
          role={!active || isWide ? 'region' : 'tabpanel'}
          aria-label={!active || isWide ? 'Run' : undefined}
          aria-labelledby={!active || isWide ? undefined : runTabId}
          aria-hidden={!active ? true : undefined}
          hidden={!active || (!isWide && activeTab !== 'run')}
          className="flex min-h-0 min-w-0 flex-col bg-background-secondary"
          data-testid="swarm-workspace-run"
        >
          {run}
        </section>
      </div>
    </div>
  );
}

export default SwarmWorkspace;
