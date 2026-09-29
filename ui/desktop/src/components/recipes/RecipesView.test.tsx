import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import RecipesView from './RecipesView';

const recipes = vi.hoisted(() => ({
  list: vi.fn(),
  remove: vi.fn(),
}));

vi.mock('../../recipe/recipe_management', () => ({
  convertToLocaleDateString: (value: string) => value,
  deleteRecipe: recipes.remove,
  listSavedRecipes: recipes.list,
  recipeToYaml: vi.fn(),
  scheduleRecipe: vi.fn(),
  setRecipeSlashCommand: vi.fn(),
}));
vi.mock('../../hooks/useNavigation', () => ({ useNavigation: () => vi.fn() }));
vi.mock('../../sessions', () => ({ createSession: vi.fn() }));
vi.mock('../../toasts', () => ({ toastSuccess: vi.fn(), toastError: vi.fn() }));

const nativeBox = vi.fn(async () => ({ response: 1 }));

const MANIFEST = {
  id: 'recipe-1',
  file_path: '/Users/someone/.config/goose/recipes/weekly.yaml',
  last_modified: '2026-09-29T10:00:00Z',
  recipe: { title: 'Weekly report', description: 'Summarise the week', instructions: 'go' },
};

/**
 * The view settles through two timers of its own: the skeleton lifts 300 ms after the list loads
 * (the cards, and their Delete buttons, render only then) and the fade-in flips 50 ms later. Those
 * timers run on the fake clock until the view has settled — its list shown and no timer of its own
 * left to fire — so every test clicks a settled view and no real 350 ms is spent waiting for it. A
 * pass flushes what the previous one scheduled: the loaded list's render schedules the skeleton
 * timer only once act flushes it. (Q-466 added this when the fade-in flip remounted every card and
 * detached a button found before it; Q-511 hoisted RecipeItem out of the view, so a card now
 * survives the flip — RecipesView.settle.test.tsx pins that.)
 */
async function renderView() {
  vi.useFakeTimers();
  try {
    render(
      <IntlTestWrapper>
        <RecipesView />
      </IntlTestWrapper>
    );
    for (let pass = 0; pass < 10; pass++) {
      if (vi.getTimerCount() === 0 && screen.queryByTitle('Delete recipe')) break;
      await act(() => vi.runAllTimersAsync());
    }
  } finally {
    vi.useRealTimers();
  }
}

function clickDelete() {
  fireEvent.click(screen.getByTitle('Delete recipe'));
}

describe('RecipesView delete (Q-472)', () => {
  beforeEach(() => {
    recipes.list.mockReset();
    recipes.list.mockResolvedValue([MANIFEST]);
    recipes.remove.mockReset();
    recipes.remove.mockResolvedValue(undefined);
    // The bridge no longer offers a message box (Q-472); a spy proves nothing reaches for one.
    Object.assign(window.electron, { showMessageBox: nativeBox });
    nativeBox.mockClear();
  });

  it('asks in the app ConfirmationModal, never a native message box, and deletes on confirm', async () => {
    await renderView();
    clickDelete();

    const dialog = screen.getByRole('dialog');
    expect(
      within(dialog).getByText('Are you sure you want to delete "Weekly report"?')
    ).toBeTruthy();
    expect(nativeBox).not.toHaveBeenCalled();
    expect(recipes.remove).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(recipes.remove).toHaveBeenCalledWith('recipe-1'));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('cancel closes the modal and deletes nothing', async () => {
    await renderView();
    clickDelete();
    const dialog = screen.getByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(recipes.remove).not.toHaveBeenCalled();
  });
});
