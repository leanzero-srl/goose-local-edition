import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

// jsdom has no Element.scrollTo; the chat wizard follows its own scroll after every message.
beforeAll(() => {
  Element.prototype.scrollTo = () => {};
});
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import PersonaChooser from './PersonaChooser';
import RecipeWizard from './RecipeWizard';
import RecipeChatWizard from './RecipeChatWizard';
import AgentSetupWizard from './AgentSetupWizard';

/**
 * The four Local Edition wizards moved onto the Studio tokens (no inline palette vars, no CHIP_RADIUS,
 * no hand-written grey, no opacity states, no border-l divider). This renders each one open, refuses the
 * bans on the rendered tree and measures every emitted class against the real pipeline.
 */

vi.mock('./useFleet', () => ({
  useFleet: () => ({
    online: true,
    models: ['mihai-qwen3.6-27b', 'gabee-coder'],
    endpoint: 'http://127.0.0.1:1234',
  }),
}));
vi.mock('../../hooks/useLmStudioFleetVisible', () => ({ useLmStudioFleetVisible: () => true }));
vi.mock('../../acp/schedules', () => ({
  acpListSchedules: async () => [{ id: 'nightly', loopConfig: {} }],
  acpCreateSchedule: async () => ({ id: 'x' }),
  acpRunScheduleNow: async () => undefined,
}));
vi.mock('../../acp/sources', () => ({ listSkillSources: async () => [{}, {}] }));
vi.mock('../loop/LoopModal', () => ({ LoopModal: () => null }));
vi.mock('../../recipe/recipe_management', () => ({ saveRecipe: async () => undefined }));

const utilitiesOf = (classes: string[]) => classes.filter((c) => !c.startsWith('lucide'));

describe('the Local Edition wizards emit only classes that compile, and nothing the Studio bans', () => {
  afterEach(() => cleanup());

  it('PersonaChooser — a divided group, the active option the accent fill, no border-l', async () => {
    const { container, getAllByRole } = render(
      <PersonaChooser value="agent" onChange={() => {}} />
    );
    assertStudioClean(container);
    const [coding, agent] = getAllByRole('button');
    expect(agent.getAttribute('aria-pressed')).toBe('true');
    expect(agent.className).toContain('bg-lz-accent');
    expect(coding.className).toContain('text-lz-ink-3');
    for (const b of [coding, agent]) expect(b.getAttribute('style')).toBeNull();
    expect(await missingUtilities(utilitiesOf(allClasses(container)))).toEqual([]);
  });

  it('RecipeWizard — the Studio field recipe, one primary Save, the err box on the tokens', async () => {
    const { container, getByRole } = render(
      <RecipeWizard isOpen onClose={() => {}} onSaved={() => {}} />
    );
    assertStudioClean(container);
    const save = getByRole('button', { name: /Save recipe/ });
    expect(save.className).toContain('bg-lz-accent');
    expect(save).toBeDisabled();
    for (const el of Array.from(container.querySelectorAll('input, textarea'))) {
      expect(el.className).toContain('border-lz-border-strong');
      expect(el.getAttribute('style')).toBeNull();
    }
    expect(await missingUtilities(utilitiesOf(allClasses(container)))).toEqual([]);
  });

  it('RecipeChatWizard — bubbles, the model chip, the draft card and the footer actions on the tokens', async () => {
    const { container, findByText, getByRole } = render(
      <RecipeChatWizard isOpen onClose={() => {}} onSaved={() => {}} />
    );
    await findByText(/What's the task\?/);
    assertStudioClean(container);
    expect(getByRole('button', { name: /^Send$/ }).className).toContain('bg-lz-accent');
    expect(getByRole('button', { name: /Save recipe/ })).toBeDisabled();
    expect(container.querySelector('[style*="6b7280"]')).toBeNull();
    expect(await missingUtilities(utilitiesOf(allClasses(container)))).toEqual([]);
  });

  it('AgentSetupWizard — zone headers, the one accent action row, solid secondaries', async () => {
    const { container, findByText, getByText } = render(
      <AgentSetupWizard isOpen onClose={() => {}} setView={() => {}} workingDir="/tmp/x" />
    );
    await findByText('Existing loops');
    assertStudioClean(container);
    expect(getByText(/Build a recipe with the fleet/).closest('button')?.className).toContain(
      'bg-lz-accent'
    );
    expect(getByText(/draft one by hand/).closest('button')?.className).toContain(
      'border-lz-border-strong'
    );
    expect(container.querySelectorAll('[style]').length).toBe(0);
    expect(await missingUtilities(utilitiesOf(allClasses(container)))).toEqual([]);
  });
});
