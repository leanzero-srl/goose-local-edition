import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import SkillsView from './SkillsView';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { readError } from './skillKinds';

/**
 * Q-80: goose dropped SKILL.md files it could not parse with a log line that named no file, 40 times a
 * session. A file that truly cannot be read now arrives in the list once, by name, with
 * `properties.readError` and the description "couldn't read <file>: <why>" (sources.rs
 * `unreadable_skill_entry`) — and the page says so instead of hiding it.
 */

vi.mock('../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => vi.fn() }));
vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/proj/goose' }));

const broken = {
  type: 'skill',
  name: 'broken-skill',
  description:
    "couldn't read /home/.agents/skills/broken-skill/SKILL.md: its frontmatter is not valid YAML (did not find expected ',' or ']' at line 4 column 3)",
  path: '/home/.agents/skills/broken-skill',
  content: '',
  global: true,
  writable: false,
  properties: { readError: "its frontmatter is not valid YAML (did not find expected ',' or ']')" },
};

const skills = [
  {
    type: 'skill',
    name: 'campaign',
    description: 'Run a benchmark campaign end to end.',
    path: '/home/.agents/skills/campaign',
    content: '# campaign\nRun it.',
    global: true,
  },
  broken,
];

vi.mock('../../acp/sources', () => ({
  listSkillSources: vi.fn(async () => skills),
  readSkillSourceFresh: vi.fn(),
  updateSkillSource: vi.fn(),
  deleteSkillSource: vi.fn(),
}));

describe('SkillsView — a SKILL.md goose cannot read', () => {
  it('is listed once, by name, under "Couldn\'t read", with the file and the reason', async () => {
    render(
      <IntlTestWrapper>
        <SkillsView />
      </IntlTestWrapper>
    );
    const row = await screen.findByTestId('skill-unreadable-row', {}, { timeout: 3000 });
    expect(row.textContent).toContain('broken-skill');
    expect(row.textContent).toContain(
      "couldn't read /home/.agents/skills/broken-skill/SKILL.md: its frontmatter is not valid YAML"
    );
    expect(screen.getAllByTestId('skill-unreadable-row')).toHaveLength(1);
    expect(screen.getByText("Couldn't read")).toBeInTheDocument();
    // not among the skills goose can use
    expect(screen.getAllByTestId('skill-row').map((r) => r.textContent)).toEqual([
      expect.stringContaining('campaign'),
    ]);
    const detail = within(screen.getByTestId('library-detail'));
    expect(detail.getByTestId('skill-unreadable-detail').textContent).toContain(
      '/home/.agents/skills/broken-skill/SKILL.md'
    );
  });

  it('readError is the reason for an unreadable entry and null for a skill', () => {
    expect(readError(broken)).toContain('not valid YAML');
    expect(readError({ properties: {} })).toBeNull();
    expect(readError({})).toBeNull();
  });
});
