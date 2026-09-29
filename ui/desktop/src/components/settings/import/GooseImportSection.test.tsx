import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import GooseImportSection from './GooseImportSection';

const parseRecipeFromFile = vi.fn();
const saveRecipe = vi.fn();
const listSavedRecipes = vi.fn();
const acpCreateSchedule = vi.fn();
const toastSuccess = vi.fn();
const toastError = vi.fn();

vi.mock('../../../recipe', () => ({
  parseRecipeFromFile: (...args: unknown[]) => parseRecipeFromFile(...args),
}));
vi.mock('../../../recipe/recipe_management', () => ({
  saveRecipe: (...args: unknown[]) => saveRecipe(...args),
  listSavedRecipes: (...args: unknown[]) => listSavedRecipes(...args),
}));
vi.mock('../../../acp/schedules', () => ({
  acpCreateSchedule: (...args: unknown[]) => acpCreateSchedule(...args),
}));
vi.mock('react-toastify', () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccess(...args),
    error: (...args: unknown[]) => toastError(...args),
  },
}));

const SOURCE = '/Users/someone/.local/share/goose';

type FileResponse = { file: string; filePath: string; error: string | null; found: boolean };

function stubSource(files: Record<string, FileResponse>, recipeFiles: string[]) {
  const electron = window.electron as unknown as Record<string, unknown>;
  electron.directoryChooser = vi.fn(async () => ({ canceled: false, filePaths: [SOURCE] }));
  electron.listFiles = vi.fn(async () => recipeFiles);
  electron.readFile = vi.fn(async (path: string) => {
    const hit = files[path];
    if (hit) return hit;
    return {
      file: '',
      filePath: path,
      error: `cat: ${path}: No such file or directory\n`,
      found: false,
    };
  });
}

function found(path: string, file: string): Record<string, FileResponse> {
  return { [path]: { file, filePath: path, error: null, found: true } };
}

async function scan() {
  render(
    <IntlTestWrapper>
      <GooseImportSection />
    </IntlTestWrapper>
  );
  fireEvent.click(screen.getByText('Choose source folder'));
  await waitFor(() => expect(screen.queryByText(/^Scanning/)).toBeNull());
}

const LEGACY_SCHEDULE = JSON.stringify([
  {
    id: 'nightly-fix',
    source: `${SOURCE}/scheduled_recipes/nightly-fix.yaml`,
    cron: '0 0 14 * * *',
    loop_config: {
      max_iterations: 10,
      stop_check: { type: 'Shell', command: 'npm test' },
      state_artifact: 'NOW.md',
    },
  },
  {
    id: 'plain-report',
    source: `${SOURCE}/scheduled_recipes/plain-report.yaml`,
    cron: '0 0 9 * * *',
  },
]);

describe('GooseImportSection (Q-227/Q-228 L8: the recipe loop is retired)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listSavedRecipes.mockResolvedValue([]);
  });

  it('names every job that carries loop_config as not imported, and offers no loop import', async () => {
    stubSource(found(`${SOURCE}/schedule.json`, LEGACY_SCHEDULE), []);
    await scan();

    const rows = screen.getAllByTestId('goose-import-retired-loop');
    expect(rows.map((r) => r.textContent)).toEqual([
      'Loops are no longer imported — nightly-fix (0 0 14 * * *). Recipes still import.',
    ]);
    expect(screen.getByText('Loops (1)')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /loop/i })).toBeNull();
    expect(screen.queryByRole('switch')).toBeNull();
    expect(acpCreateSchedule).not.toHaveBeenCalled();
    expect(screen.queryByText(/No recipes found/)).toBeNull();
  });

  it('keeps the recipe half: a recipe beside a retired loop still imports', async () => {
    stubSource(
      {
        ...found(`${SOURCE}/schedule.json`, LEGACY_SCHEDULE),
        ...found(`${SOURCE}/recipes/triage.yaml`, 'title: Triage\nprompt: go\n'),
      },
      ['triage.yaml', 'notes.txt']
    );
    parseRecipeFromFile.mockResolvedValue({ title: 'Triage', prompt: 'go' });
    saveRecipe.mockResolvedValue('triage');
    await scan();

    expect(screen.getByText('Recipes (1)')).toBeInTheDocument();
    expect(screen.getByTestId('goose-import-retired-loop')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Import 1 recipe' }));

    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Imported 1 recipe'));
    expect(saveRecipe).toHaveBeenCalledWith({ title: 'Triage', prompt: 'go' }, null);
    expect(acpCreateSchedule).not.toHaveBeenCalled();
  });

  it('names an unparseable schedule.json with its error instead of reading it as "no loops"', async () => {
    stubSource(found(`${SOURCE}/schedule.json`, '[{"id": "half'), []);
    await scan();

    const problem = screen.getByTestId('goose-import-schedule-problem');
    expect(problem.textContent).toMatch(/^schedule\.json could not be read: \S/);
    expect(screen.queryByText(/No recipes found/)).toBeNull();
  });

  it('names a schedule.json that is not a list', async () => {
    stubSource(found(`${SOURCE}/schedule.json`, '{"jobs": []}'), []);
    await scan();

    expect(screen.getByTestId('goose-import-schedule-problem').textContent).toBe(
      'schedule.json could not be read: it is not a list of schedules'
    );
  });

  it('names a schedule.json that exists but cannot be opened', async () => {
    const path = `${SOURCE}/schedule.json`;
    stubSource(
      {
        [path]: {
          file: '',
          filePath: path,
          error: `cat: ${path}: Permission denied\n`,
          found: false,
        },
      },
      []
    );
    await scan();

    expect(screen.getByTestId('goose-import-schedule-problem').textContent).toBe(
      `schedule.json could not be read: cat: ${path}: Permission denied`
    );
  });

  it('an absent schedule.json is not a problem: nothing found is said plainly', async () => {
    stubSource({}, []);
    await scan();

    expect(screen.getByText(`No recipes found under ${SOURCE}.`)).toBeInTheDocument();
    expect(screen.queryByTestId('goose-import-schedule-problem')).toBeNull();
    expect(screen.queryByTestId('goose-import-retired-loops')).toBeNull();
  });

  it('a schedule.json of plain schedules only shows no loop section', async () => {
    stubSource(
      found(
        `${SOURCE}/schedule.json`,
        JSON.stringify([{ id: 'plain-report', source: 'x.yaml', cron: '0 0 9 * * *' }])
      ),
      []
    );
    await scan();

    expect(screen.queryByTestId('goose-import-retired-loops')).toBeNull();
    expect(screen.getByText(`No recipes found under ${SOURCE}.`)).toBeInTheDocument();
  });
});

/**
 * Q-512: Row was declared inside GooseImportSection, so every render of the section made a new
 * component type and remounted every row. Toggling one switch re-renders the section, so the switch
 * a person had just clicked was replaced by a new element: a second click on the one they held
 * reached a detached node and changed nothing.
 */
describe('GooseImportSection rows survive a re-render (Q-512)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listSavedRecipes.mockResolvedValue([]);
  });

  it('the switch clicked stays the switch on screen, and a second click on it still toggles', async () => {
    stubSource({}, ['triage.yaml', 'weekly.yaml']);
    await scan();

    const [held] = screen.getAllByRole('switch');
    expect(held.getAttribute('aria-checked')).toBe('true');
    expect(screen.getByRole('button', { name: 'Import 2 recipes' })).toBeInTheDocument();

    fireEvent.click(held);
    expect(screen.getByRole('button', { name: 'Import 1 recipe' })).toBeInTheDocument();
    expect(screen.getAllByRole('switch')[0]).toBe(held);
    expect(held.getAttribute('aria-checked')).toBe('false');

    fireEvent.click(held);
    expect(screen.getByRole('button', { name: 'Import 2 recipes' })).toBeInTheDocument();
    expect(held.getAttribute('aria-checked')).toBe('true');
  });
});
