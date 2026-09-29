import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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

function renderView() {
  return render(
    <IntlTestWrapper>
      <RecipesView />
    </IntlTestWrapper>
  );
}

async function clickDelete() {
  const button = await screen.findByTitle('Delete recipe');
  fireEvent.click(button);
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
    renderView();
    await clickDelete();

    const dialog = await screen.findByRole('dialog');
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
    renderView();
    await clickDelete();
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(recipes.remove).not.toHaveBeenCalled();
  });
});
