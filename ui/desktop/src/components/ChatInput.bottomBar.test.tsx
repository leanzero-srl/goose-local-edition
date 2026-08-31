import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeAll } from 'vitest';
import ChatInput from './ChatInput';
import { ChatState } from '../types/chatState';
import { IntlTestWrapper } from '../i18n/test-utils';

/**
 * Pass E — the session input bar's contents:
 *  - the extensions affordance (puzzle icon + count) is GONE from the bar (hidden, not deleted;
 *    enabled extensions keep working);
 *  - the cost readout never renders for the LeanZero MLX provider (omlx) — local inference has no
 *    price — while cloud providers keep it exactly as before.
 */

vi.mock('./settings/models/bottom_bar/ModelsBottomBar', () => ({
  default: () => <div data-testid="models-bottom-bar" />,
}));
vi.mock('./bottom_menu/DirSwitcher', () => ({
  DirSwitcher: () => <div data-testid="dir-switcher" />,
}));
vi.mock('./bottom_menu/BottomMenuExtensionSelection', () => ({
  BottomMenuExtensionSelection: () => <div data-testid="extensions-selector" />,
}));
vi.mock('./bottom_menu/CostTracker', () => ({
  CostTracker: () => <div data-testid="cost-tracker" />,
}));
vi.mock('./bottom_menu/ContextWindowIndicator', () => ({
  ContextWindowIndicator: () => <div data-testid="context-indicator" />,
}));
vi.mock('./MentionPopover', () => ({
  default: React.forwardRef(() => null),
}));
vi.mock('../hooks/useAudioRecorder', () => ({
  useAudioRecorder: () => ({
    isEnabled: false,
    dictationProvider: null,
    isRecording: false,
    isTranscribing: false,
    startRecording: vi.fn(),
    stopRecording: vi.fn(),
  }),
}));
vi.mock('./ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    getCurrentModelAndProvider: async () => ({ model: 'test-model', provider: 'anthropic' }),
    currentModel: 'test-model',
    currentProvider: 'anthropic',
  }),
}));
vi.mock('./swarm/usePersona', () => ({
  usePersona: () => ({ persona: 'build', setPersona: vi.fn() }),
}));
vi.mock('./swarm/PersonaChooser', () => ({ PersonaChooser: () => null }));
vi.mock('./swarm/AgentSetupWizard', () => ({ default: () => null }));
vi.mock('./alerts', () => ({
  useAlerts: () => ({ alerts: [], addAlert: vi.fn(), clearAlerts: vi.fn() }),
  AlertType: { Error: 'error', Warning: 'warning', Info: 'info' },
}));
vi.mock('../acp/providers', () => ({ acpListProviderDetails: async () => [] }));
vi.mock('../utils/canonical', () => ({ fetchCanonicalModelInfo: async () => null }));
vi.mock('../acp/mlx-engine', () => ({
  mlxEngineStatus: async () => ({ state: 'stopped', restartRequired: false, availableMemoryGb: 0 }),
}));
vi.mock('./swarm/useFleet', () => ({ fetchSwarmContextLimit: async () => null }));
vi.mock('./ui/Diagnostics', () => ({ DiagnosticsModal: () => null }));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
});

const mount = (provider: string) =>
  render(
    <IntlTestWrapper>
      <ChatInput
        sessionId="sess-1"
        handleSubmit={vi.fn()}
        chatState={ChatState.Idle}
        setView={vi.fn()}
        sessionModel="test-model"
        sessionProvider={provider}
        sessionLoaded
        workingDir="/tmp"
      />
    </IntlTestWrapper>
  );

describe('ChatInput bottom bar (pass E)', () => {
  it('never renders the extensions affordance', async () => {
    mount('anthropic');
    await waitFor(() => expect(screen.getByTestId('models-bottom-bar')).toBeInTheDocument());
    expect(screen.queryByTestId('extensions-selector')).toBeNull();
  });

  it('keeps the cost readout for a cloud provider', async () => {
    mount('anthropic');
    await waitFor(() => expect(screen.getByTestId('cost-tracker')).toBeInTheDocument());
  });

  it('hides the cost readout for the LeanZero MLX provider (omlx)', async () => {
    mount('omlx');
    await waitFor(() => expect(screen.getByTestId('models-bottom-bar')).toBeInTheDocument());
    expect(screen.queryByTestId('cost-tracker')).toBeNull();
  });
});
