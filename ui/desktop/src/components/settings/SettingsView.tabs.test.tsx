import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SettingsView from './SettingsView';

/**
 * Goose Swarm pass A: the Models tab left the settings navigation (provider management consolidates
 * into the LeanZero Swarm view). This pins the tab LIST — the components stay in code, the nav entry
 * must not creep back, and the default tab must be one that actually exists.
 */

// Section bodies are out of scope here — the test targets the tab list. Each mock renders a marker.
vi.mock('./models/ModelsSection', () => ({ default: () => <div data-testid="models-body" /> }));
vi.mock('./swarm/SwarmSettingsSection', () => ({ default: () => <div data-testid="swarm-body" /> }));
vi.mock('./import/ImportView', () => ({ default: () => <div data-testid="import-body" /> }));
vi.mock('./chat/ChatSettingsSection', () => ({ default: () => <div data-testid="chat-body" /> }));
vi.mock('./app/ExternalBackendSection', () => ({ default: () => <div /> }));
vi.mock('./PromptsSettingsSection', () => ({ default: () => <div /> }));
vi.mock('./keyboard/KeyboardShortcutsSection', () => ({ default: () => <div /> }));
vi.mock('./auth/AuthSettingsSection', () => ({ default: () => <div /> }));
vi.mock('./app/AppSettingsSection', () => ({ default: () => <div /> }));
vi.mock('./config/ConfigSettings', () => ({ default: () => <div /> }));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock('../../utils/analytics', () => ({ trackSettingsTabViewed: vi.fn() }));

const features = { localInference: false, mlxEngine: false, isLoading: false };
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => features,
}));

const editionState = { edition: 'standard', setEdition: vi.fn(), isLocal: false };
vi.mock('../../contexts/EditionContext', () => ({
  useEdition: () => editionState,
}));

const renderSettings = () =>
  render(
    <IntlTestWrapper>
      <SettingsView onClose={() => {}} setView={() => {}} viewOptions={{}} />
    </IntlTestWrapper>
  );

describe('SettingsView tab list (models tab removed)', () => {
  it('standard edition shows chat/sharing/prompts/keyboard/auth/app — no Models tab', () => {
    renderSettings();
    expect(screen.queryByTestId('settings-models-tab')).toBeNull();
    expect(screen.queryByTestId('settings-swarm-tab')).toBeNull();
    expect(screen.queryByTestId('settings-import-tab')).toBeNull();
    for (const tab of ['chat', 'sharing', 'prompts', 'keyboard', 'auth', 'app']) {
      expect(screen.getByTestId(`settings-${tab}-tab`)).toBeTruthy();
    }
    // The default tab is a real one: Chat is active and its body is mounted.
    expect(screen.getByTestId('settings-chat-tab').getAttribute('data-state')).toBe('active');
    expect(screen.getByTestId('chat-body')).toBeTruthy();
  });

  it('local edition (Goose Swarm) defaults to the Swarm tab and still has no Models tab', () => {
    editionState.edition = 'local';
    editionState.isLocal = true;
    try {
      renderSettings();
      expect(screen.queryByTestId('settings-models-tab')).toBeNull();
      expect(screen.getByTestId('settings-swarm-tab').getAttribute('data-state')).toBe('active');
      expect(screen.getByTestId('settings-import-tab')).toBeTruthy();
      expect(screen.getByTestId('swarm-body')).toBeTruthy();
    } finally {
      editionState.edition = 'standard';
      editionState.isLocal = false;
    }
  });
});
