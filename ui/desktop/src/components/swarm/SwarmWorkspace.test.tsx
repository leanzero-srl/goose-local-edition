import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmWorkspace, { SWARM_WORKSPACE_MIN_WIDTH, nextWorkspaceTab } from './SwarmWorkspace';

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
