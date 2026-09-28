import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import SkillsView from './SkillsView';
import { IntlTestWrapper } from '../../i18n/test-utils';

vi.mock('../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => vi.fn() }));
vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/proj/goose' }));

const skill = {
  type: 'skill',
  name: 'panel-surgeon',
  description: 'Desktop swarm UI edits.',
  path: '/proj/goose/.goose/skills/panel-surgeon',
  content: '# panel-surgeon\nEdit the panel.',
  global: false,
};

const sources = vi.hoisted(() => ({
  deleteSkillSource: vi.fn(async () => undefined),
}));
vi.mock('../../acp/sources', () => ({
  listSkillSources: vi.fn(async () => [skill]),
  readSkillSourceFresh: vi.fn(async () => skill),
  updateSkillSource: vi.fn(),
  deleteSkillSource: sources.deleteSkillSource,
}));

// Q-221: the delete goes out with the projectDir the list was loaded for — the backend refuses a
// project skill it cannot see in that listing.
describe('SkillsView deletes a skill under the projectDir it listed', () => {
  it('right-click → Delete → confirm sends the listed projectDir', async () => {
    render(
      <IntlTestWrapper>
        <SkillsView />
      </IntlTestWrapper>
    );
    const row = (await screen.findByText('panel-surgeon', { selector: 'button *' })).closest(
      'button'
    ) as HTMLElement;
    fireEvent.contextMenu(row);
    fireEvent.click(within(await screen.findByTestId('skill-context-menu')).getByText('Delete'));
    fireEvent.click(await screen.findByRole('button', { name: /^Delete$/ }));
    await waitFor(() =>
      expect(sources.deleteSkillSource).toHaveBeenCalledWith(skill.path, '/proj/goose')
    );
  });
});
