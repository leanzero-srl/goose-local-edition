import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import ProjectLanding from './ProjectLanding';
import { AppEvents } from '../constants/events';

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
    expect(await screen.findByText('Sessions start from a project')).toBeInTheDocument();
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

  it('a cancelled picker changes nothing', async () => {
    const mocks = electronMocks();
    renderLanding();
    fireEvent.click(await screen.findByText('Add a project'));
    await waitFor(() => expect(mocks.directoryChooser).toHaveBeenCalled());
    expect(mocks.addProject).not.toHaveBeenCalled();
    expect(await screen.findByText('Add a project')).toBeInTheDocument();
  });
});
