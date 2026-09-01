import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import ProjectLanding from './ProjectLanding';
import { AppEvents } from '../constants/events';
import { TYPE } from './lz';
import { allClasses, assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';

/**
 * Pass D (owner): sessions start from projects only. The home route carries NO chat input —
 * it states the rule, hints at the sidebar's "+ New session here" when projects exist, and
 * with none offers the SAME add-project picker the sidebar "+" uses (one flow, one broadcast).
 */

interface ElectronProjectMocks {
  listProjects: ReturnType<typeof vi.fn>;
  addProject: ReturnType<typeof vi.fn>;
  removeProject: ReturnType<typeof vi.fn>;
  directoryChooser: ReturnType<typeof vi.fn>;
}

function electronMocks(): ElectronProjectMocks {
  const mocks: ElectronProjectMocks = {
    listProjects: vi.fn().mockResolvedValue([]),
    addProject: vi.fn().mockResolvedValue([]),
    removeProject: vi.fn().mockResolvedValue([]),
    directoryChooser: vi.fn().mockResolvedValue({ canceled: true, filePaths: [] }),
  };
  Object.assign(window.electron, mocks);
  return mocks;
}

const renderLanding = () =>
  render(
    <IntlProvider locale="en" messages={{}}>
      <ProjectLanding />
    </IntlProvider>
  );

describe('ProjectLanding', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('states the rule and carries NO chat input', async () => {
    electronMocks();
    renderLanding();
    expect(await screen.findByText('Start from a project')).toBeInTheDocument();
    expect(document.querySelector('textarea')).toBeNull();
    expect(document.querySelector('input')).toBeNull();
  });

  it('with projects registered it points at the sidebar "+ New session here" and offers no button', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    renderLanding();
    expect(
      await screen.findByText(/Pick a project in the sidebar and use its “\+ New session here”\./)
    ).toBeInTheDocument();
    expect(screen.queryByText('Add a project')).not.toBeInTheDocument();
  });

  it('with NO projects the Add-project button drives the same picker flow and broadcasts the change', async () => {
    const mocks = electronMocks();
    mocks.directoryChooser.mockResolvedValue({ canceled: false, filePaths: ['/picked/app'] });
    mocks.addProject.mockResolvedValue([{ path: '/picked/app', addedAt: 3 }]);

    const changed = vi.fn();
    window.addEventListener(AppEvents.PROJECTS_CHANGED, changed);
    try {
      renderLanding();
      fireEvent.click(await screen.findByText('Add a project'));

      await waitFor(() => expect(mocks.addProject).toHaveBeenCalledWith('/picked/app'));
      expect(mocks.directoryChooser).toHaveBeenCalled();
      await waitFor(() => expect(changed).toHaveBeenCalledTimes(1));

      // The landing itself follows the broadcast: the empty-state button gives way to the hint.
      expect(
        await screen.findByText(/Pick a project in the sidebar and use its “\+ New session here”\./)
      ).toBeInTheDocument();
      expect(screen.queryByText('Add a project')).not.toBeInTheDocument();
    } finally {
      window.removeEventListener(AppEvents.PROJECTS_CHANGED, changed);
    }
  });

  it('is a composed EmptyState — the LeanZero mark, a display title, ONE body line — over a 3-row KeyValue, centered at 560, with no icon-in-a-square badge', async () => {
    const mocks = electronMocks();
    mocks.listProjects.mockResolvedValue([{ path: '/proj/goose', addedAt: 1 }]);
    const { container } = renderLanding();
    await screen.findByText('Start from a project');

    expect(screen.getByTestId('project-landing').className).toContain('max-w-[560px]');
    const empty = screen.getByTestId('lz-empty-state');
    expect(within(empty).getByTestId('leanzero-glyph')).toBeInTheDocument();
    expect(screen.getByTestId('lz-empty-state-icon').className).toContain('bg-lz-accent');
    const title = screen.getByRole('heading', { name: 'Start from a project' });
    for (const c of TYPE.display.split(' ')) expect(title.className).toContain(c);
    expect(empty.querySelectorAll('p')).toHaveLength(1);
    expect(screen.queryByText('Projects')).toBeNull();

    expect(screen.getByText('What a project gives you')).toBeInTheDocument();
    expect(screen.getAllByTestId('lz-key-value-row')).toHaveLength(3);
    expect(screen.getByText('Working directory')).toBeInTheDocument();
    expect(screen.getByText('Kept in Unfiled')).toBeInTheDocument();
    expect(container.querySelector('[data-variant="primary"]')).toBeNull();
    expect(container.querySelector('[style]')).toBeNull();

    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(classes.length).toBeGreaterThan(20);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('with NO projects the add-project action is the ONE primary Button of the view', async () => {
    electronMocks();
    renderLanding();
    const button = (await screen.findByText('Add a project')).closest('button') as HTMLElement;
    expect(button.dataset.variant).toBe('primary');
    expect(document.querySelectorAll('[data-variant="primary"]')).toHaveLength(1);
    expect(button.getAttribute('style')).toBeNull();
  });

  it('a cancelled picker changes nothing', async () => {
    const mocks = electronMocks();
    renderLanding();
    fireEvent.click(await screen.findByText('Add a project'));
    await waitFor(() => expect(mocks.directoryChooser).toHaveBeenCalled());
    expect(mocks.addProject).not.toHaveBeenCalled();
    expect(await screen.findByText('Add a project')).toBeInTheDocument();
  });
});
