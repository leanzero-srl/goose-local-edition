import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmWorkspace, { SWARM_WORKSPACE_MIN_WIDTH, nextWorkspaceTab } from './SwarmWorkspace';
import { shouldSplitSwarmWorkspace } from './swarmRunLiveness';

type ResizeEntry = { contentRect: { width: number } };
let resizeCallback: (entries: ResizeEntry[]) => void;

class ResizeObserverMock {
  constructor(callback: (entries: ResizeEntry[]) => void) {
    resizeCallback = callback;
  }

  observe() {}
  unobserve() {}
  disconnect() {}
}

function resizeTo(width: number) {
  act(() => {
    resizeCallback([{ contentRect: { width } }]);
  });
}

const NOW = 2_000_000_000_000;
const liveRun = {
  present: true,
  inProgress: true,
  finished: false,
  heartbeat: NOW,
  heartbeatExited: false,
};

describe('SwarmWorkspace', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', ResizeObserverMock);
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      width: SWARM_WORKSPACE_MIN_WIDTH - 1,
      height: 700,
      top: 0,
      right: 0,
      bottom: 700,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('keeps both pane subtrees mounted while switching the narrow workspace', () => {
    render(
      <SwarmWorkspace
        conversation={<input aria-label="Conversation draft" defaultValue="still here" />}
        run={<input aria-label="Run note" defaultValue="also here" />}
      />
    );

    const conversationPanel = screen.getByTestId('swarm-workspace-conversation');
    const runPanel = screen.getByTestId('swarm-workspace-run');
    expect(conversationPanel).not.toHaveAttribute('hidden');
    expect(runPanel).toHaveAttribute('hidden');
    expect(screen.getByLabelText('Conversation draft')).toBeInTheDocument();
    expect(screen.getByLabelText('Run note')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('tab', { name: 'Run' }));
    expect(conversationPanel).toHaveAttribute('hidden');
    expect(runPanel).not.toHaveAttribute('hidden');
    expect(screen.getByLabelText('Run note')).toHaveValue('also here');
  });

  it('provides roving tab focus and arrow, Home, and End navigation', () => {
    render(<SwarmWorkspace conversation={<div>Chat</div>} run={<div>Build</div>} />);
    const runTab = screen.getByRole('tab', { name: 'Run' });
    const conversationTab = screen.getByRole('tab', { name: 'Conversation' });

    expect(conversationTab).toHaveAttribute('aria-selected', 'true');
    expect(conversationTab).toHaveAttribute('tabindex', '0');
    expect(runTab).toHaveAttribute('tabindex', '-1');

    fireEvent.keyDown(conversationTab, { key: 'ArrowRight' });
    expect(runTab).toHaveAttribute('aria-selected', 'true');
    expect(runTab).toHaveFocus();

    fireEvent.keyDown(runTab, { key: 'Home' });
    expect(conversationTab).toHaveAttribute('aria-selected', 'true');
    expect(conversationTab).toHaveFocus();

    fireEvent.keyDown(conversationTab, { key: 'End' });
    expect(runTab).toHaveAttribute('aria-selected', 'true');
    expect(runTab).toHaveFocus();
  });

  it('shows both independently labelled regions when the workspace is wide', () => {
    render(<SwarmWorkspace conversation={<div>Chat</div>} run={<div>Build</div>} />);
    resizeTo(SWARM_WORKSPACE_MIN_WIDTH);

    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Conversation' })).not.toHaveAttribute('hidden');
    expect(screen.getByRole('region', { name: 'Run' })).not.toHaveAttribute('hidden');
  });

  it('preserves the conversation DOM, draft, and scroll across inactive, active, and finished runs', () => {
    const conversation = () => (
      <div data-testid="conversation-scroll">
        <input aria-label="Conversation draft" defaultValue="" />
      </div>
    );
    const { rerender } = render(
      <SwarmWorkspace active={false} conversation={conversation()} run={<div>Run history</div>} />
    );

    const conversationPanel = screen.getByTestId('swarm-workspace-conversation');
    const draft = screen.getByLabelText('Conversation draft');
    fireEvent.change(draft, { target: { value: 'do not lose this' } });
    conversationPanel.scrollTop = 173;
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
    expect(screen.getByTestId('swarm-workspace-run')).toHaveAttribute('hidden');

    rerender(<SwarmWorkspace active conversation={conversation()} run={<div>Active run</div>} />);
    expect(screen.getByLabelText('Conversation draft')).toBe(draft);
    expect(draft).toHaveValue('do not lose this');
    expect(screen.getByTestId('swarm-workspace-conversation')).toBe(conversationPanel);
    expect(conversationPanel.scrollTop).toBe(173);
    expect(screen.getByRole('tablist', { name: 'Active swarm workspace' })).toBeInTheDocument();

    rerender(
      <SwarmWorkspace active={false} conversation={conversation()} run={<div>Finished run</div>} />
    );
    expect(screen.getByLabelText('Conversation draft')).toBe(draft);
    expect(draft).toHaveValue('do not lose this');
    expect(conversationPanel).not.toHaveAttribute('hidden');
    expect(screen.getByTestId('swarm-workspace-run')).toHaveAttribute('hidden');
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
  });

  // THE REGRESSION THIS PINS: staleness used to feed workspace visibility, so a local model that stopped
  // writing for 45s collapsed the split and took the run pane with it. Only a real end may do that now.
  it('keeps the split while the engine heartbeat is silent, and folds it only when the run ends', () => {
    const conversation = () => <input aria-label="Crash-safe draft" defaultValue="" />;
    const { rerender } = render(
      <SwarmWorkspace
        active={shouldSplitSwarmWorkspace({ isLocal: true, run: liveRun })}
        conversation={conversation()}
        run={<div>Live run</div>}
      />
    );
    const draft = screen.getByLabelText('Crash-safe draft');
    fireEvent.change(draft, { target: { value: 'survives a quiet model' } });
    expect(screen.getByRole('tablist', { name: 'Active swarm workspace' })).toBeInTheDocument();

    const quietRun = { ...liveRun, heartbeat: NOW - 600_000 };
    rerender(
      <SwarmWorkspace
        active={shouldSplitSwarmWorkspace({ isLocal: true, run: quietRun })}
        conversation={conversation()}
        run={<div>Still running</div>}
      />
    );
    expect(screen.getByRole('tablist', { name: 'Active swarm workspace' })).toBeInTheDocument();

    rerender(
      <SwarmWorkspace
        active={shouldSplitSwarmWorkspace({
          isLocal: true,
          run: { ...quietRun, inProgress: false, finished: true },
        })}
        conversation={conversation()}
        run={<div>Finished run</div>}
      />
    );
    expect(screen.getByLabelText('Crash-safe draft')).toBe(draft);
    expect(draft).toHaveValue('survives a quiet model');
    expect(screen.queryByRole('tablist')).not.toBeInTheDocument();
    expect(screen.getByTestId('swarm-workspace-run')).toHaveAttribute('hidden');
  });
});

describe('nextWorkspaceTab', () => {
  it('maps the supported tab keys without consuming unrelated keys', () => {
    expect(nextWorkspaceTab('run', 'ArrowLeft')).toBe('conversation');
    expect(nextWorkspaceTab('conversation', 'ArrowRight')).toBe('run');
    expect(nextWorkspaceTab('run', 'Home')).toBe('conversation');
    expect(nextWorkspaceTab('conversation', 'End')).toBe('run');
    expect(nextWorkspaceTab('run', 'Enter')).toBeNull();
  });
});
