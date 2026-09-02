import React from 'react';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SettingsView from './SettingsView';

/**
 * Goose Flock pass A: the Models tab left the settings navigation (provider management consolidates
 * into the Goose Flock view). Owner (2026-08-31): the LeanZero Flock tab is gone too — the
 * LeanZero Flock view's nodes tab is the only swarm surface. This pins the tab LIST — the
 * components stay in code, the nav entries must not creep back, and the default tab must be one
 * that actually exists.
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

describe('SettingsView tab list (models and swarm tabs removed)', () => {
  it('standard edition shows chat/sharing/prompts/keyboard/auth/app — no Models, no Swarm tab', () => {
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

  it('local edition (Goose Flock) has NO Swarm tab either and defaults to Chat', () => {
    editionState.edition = 'local';
    editionState.isLocal = true;
    try {
      renderSettings();
      expect(screen.queryByTestId('settings-models-tab')).toBeNull();
      expect(screen.queryByTestId('settings-swarm-tab')).toBeNull();
      expect(screen.queryByTestId('swarm-body')).toBeNull();
      expect(screen.getByTestId('settings-import-tab')).toBeTruthy();
      expect(screen.getByTestId('settings-chat-tab').getAttribute('data-state')).toBe('active');
      expect(screen.getByTestId('chat-body')).toBeTruthy();
    } finally {
      editionState.edition = 'standard';
      editionState.isLocal = false;
    }
  });

  it('even with localInference advertised the Swarm tab stays gone (owner removal, not a capability gate)', () => {
    features.localInference = true;
    try {
      renderSettings();
      expect(screen.queryByTestId('settings-swarm-tab')).toBeNull();
      expect(screen.getByTestId('settings-chat-tab').getAttribute('data-state')).toBe('active');
    } finally {
      features.localInference = false;
    }
  });
});
