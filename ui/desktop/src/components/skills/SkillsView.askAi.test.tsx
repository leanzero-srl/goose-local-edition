import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import SkillsView, { askAboutSkillPrompt } from './SkillsView';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * UX audit 2026-09-23: the Memories detail had "Ask AI about it" beside Edit/Delete and the Skills
 * detail had nothing — the only way in was the list's right-click menu. The detail button starts the
 * same session as the menu, with the prompt built from the SELECTED entry's own facts.
 */

const startChat = vi.hoisted(() => vi.fn());
vi.mock('../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => startChat }));
vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/Users/me' }));

const skills = [
  {
    type: 'skill',
    name: 'release-checklist',
    description: 'How to run a release.',
    path: '/Users/me/.agents/skills/release-checklist',
    content: '# Release Checklist',
    global: true,
  },
  {
    type: 'builtinSkill',
    name: 'goose-doc-guide',
    description: 'Reference goose documentation.',
    path: 'builtin://skills/goose-doc-guide',
    content: '# Docs',
    global: true,
  },
];

vi.mock('../../acp/sources', () => ({
  listSkillSources: vi.fn(async () => skills),
  readSkillSourceFresh: vi.fn(),
  updateSkillSource: vi.fn(),
  deleteSkillSource: vi.fn(),
}));

const mount = () =>
  render(
    <IntlTestWrapper>
      <SkillsView />
    </IntlTestWrapper>
  );

const rowOf = async (name: string) =>
  (await screen.findByText(name, { selector: 'button *' }, { timeout: 3000 })).closest(
    'button'
  ) as HTMLElement;

describe('Skills detail — Ask AI about it', () => {
  it('starts the session about the selected skill with its factual prompt', async () => {
    mount();
    fireEvent.click(await rowOf('release-checklist'));
    fireEvent.click(await screen.findByRole('button', { name: /Ask AI about it/ }));
    expect(startChat).toHaveBeenCalledTimes(1);
    const prompt = startChat.mock.calls[0][0] as string;
    expect(prompt).toBe(askAboutSkillPrompt(skills[0] as never, '/Users/me'));
    expect(prompt).toContain('/Users/me/.agents/skills/release-checklist/SKILL.md');
    expect(prompt).toContain('found no other files');
  });

  it('a built-in skill has the button too (it can be forked), and the right-click menu agrees', async () => {
    startChat.mockClear();
    mount();
    fireEvent.click(await rowOf('goose-doc-guide'));
    fireEvent.click(await screen.findByRole('button', { name: /Ask AI about it/ }));
    const fromDetail = startChat.mock.calls[0][0] as string;
    expect(fromDetail).toContain('there is no file on disk');

    fireEvent.contextMenu(await rowOf('goose-doc-guide'));
    fireEvent.click(await screen.findByText('Start an AI session about this skill'));
    expect(startChat.mock.calls[1][0]).toBe(fromDetail);
  });
});
