import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import RecipesView from './RecipesView';

const recipes = vi.hoisted(() => ({ list: vi.fn() }));

vi.mock('../../recipe/recipe_management', () => ({
  convertToLocaleDateString: (value: string) => value,
  deleteRecipe: vi.fn(),
  listSavedRecipes: recipes.list,
  recipeToYaml: vi.fn(),
  scheduleRecipe: vi.fn(),
  setRecipeSlashCommand: vi.fn(),
}));
vi.mock('../../hooks/useNavigation', () => ({ useNavigation: () => vi.fn() }));
vi.mock('../../sessions', () => ({ createSession: vi.fn() }));
vi.mock('../../toasts', () => ({ toastSuccess: vi.fn(), toastError: vi.fn() }));

const MANIFEST = {
  id: 'recipe-1',
  file_path: '/Users/someone/.config/goose/recipes/weekly.yaml',
  last_modified: '2026-09-29T10:00:00Z',
  recipe: { title: 'Weekly report', description: 'Summarise the week', instructions: 'go' },
};

/**
 * Q-511: the list settles through two timers of the view's own — the skeleton lifts 300 ms after
 * the list loads, the fade-in flips 50 ms later. While RecipeItem was declared inside RecipesView,
 * the fade-in render made a new component type and remounted every card, so a Delete button a
 * person (or a test) found between the two steps was detached by the time the click landed, and
 * the click opened nothing. The card must survive the settle.
 */
describe('RecipesView settle (Q-511)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    recipes.list.mockReset();
    recipes.list.mockResolvedValue([MANIFEST]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('a Delete button found before the 50 ms fade-in step still opens the ConfirmationModal after it', async () => {
    render(
      <IntlTestWrapper>
        <RecipesView />
      </IntlTestWrapper>
    );
    await act(async () => {
      await Promise.resolve();
    });
    await act(() => vi.advanceTimersByTimeAsync(300));

    const button = screen.getByTitle('Delete recipe');
    expect(button.closest('.opacity-0')).not.toBeNull();

    await act(() => vi.advanceTimersByTimeAsync(50));
    expect(screen.getByTitle('Delete recipe').closest('.opacity-100')).not.toBeNull();

    fireEvent.click(button);

    const dialog = screen.getByRole('dialog');
    expect(
      within(dialog).getByText('Are you sure you want to delete "Weekly report"?')
    ).toBeTruthy();
  });
});
