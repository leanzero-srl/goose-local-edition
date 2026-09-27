import { fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import RecipeWizard from './RecipeWizard';
import RecipeChatWizard from './RecipeChatWizard';
import AgentSetupWizard from './AgentSetupWizard';

/**
 * Q-21: the Set up flow's overlays were hand-rolled fixed divs — Escape did nothing and focus
 * walked out behind them (round 1 #10, shots 08/12). Each is now the app's dialog primitive.
 */

beforeAll(() => {
  Element.prototype.scrollTo = () => {};
});
vi.mock('./useFleet', () => ({
  useFleet: () => ({ online: false, models: [], endpoint: 'http://127.0.0.1:1234' }),
}));
vi.mock('../../hooks/useLmStudioFleetVisible', () => ({ useLmStudioFleetVisible: () => false }));
vi.mock('../leanzero-swarm/useMlxEngineStatus', () => ({
  useMlxEngineStatusPoll: () => ({ status: null, error: null }),
}));
vi.mock('../../acp/schedules', () => ({
  acpListSchedules: async () => [],
  acpCreateSchedule: async () => ({ id: 'x' }),
  acpRunScheduleNow: async () => undefined,
}));
vi.mock('../../acp/sources', () => ({ listSkillSources: async () => [] }));
vi.mock('../loop/LoopModal', () => ({ LoopModal: () => null }));
vi.mock('../../recipe/recipe_management', () => ({ saveRecipe: async () => undefined }));

const cases: Array<[string, (onClose: () => void) => React.ReactElement]> = [
  [
    'Set up (the recipes and loops hub)',
    (onClose) => <AgentSetupWizard isOpen onClose={onClose} setView={() => {}} workingDir="/tmp" />,
  ],
  ['Draft a recipe', (onClose) => <RecipeWizard isOpen onClose={onClose} onSaved={() => {}} />],
  [
    'Build a recipe with the fleet',
    (onClose) => <RecipeChatWizard isOpen onClose={onClose} onSaved={() => {}} />,
  ],
];

describe('the Set up flow’s dialogs close on Escape and hold focus (Q-21)', () => {
  it.each(cases)('%s', (_name, ui) => {
    const onClose = vi.fn();
    render(ui(onClose));
    const dialog = screen.getByRole('dialog');
    expect(dialog.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
