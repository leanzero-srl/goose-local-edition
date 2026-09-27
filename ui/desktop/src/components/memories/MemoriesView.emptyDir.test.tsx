import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import MemoriesView from './MemoriesView';
import { IntlTestWrapper } from '../../i18n/test-utils';

vi.mock('../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => vi.fn() }));
vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/proj/goose' }));

const withConfigDir = (dir: string | undefined) => {
  Object.defineProperty(window, 'appConfig', {
    configurable: true,
    value:
      dir === undefined
        ? undefined
        : { get: (key: string) => (key === 'GOOSE_CONFIG_DIR' ? dir : undefined) },
  });
};

afterEach(() => withConfigDir(undefined));

const mountEmpty = () => {
  (window as unknown as { electron: Record<string, unknown> }).electron.listMemories = vi.fn(
    async () => []
  );
  return render(
    <IntlTestWrapper>
      <MemoriesView />
    </IntlTestWrapper>
  );
};

describe('the empty memories view names the folder goose writes (Q-198)', () => {
  it('unset root: the text it always showed, ~/.config/goose/memory/', async () => {
    withConfigDir('~/.config/goose');
    mountEmpty();
    expect(
      await screen.findByText(/Goose stores them in ~\/\.config\/goose\/memory\//)
    ).toBeTruthy();
  });

  it("an isolated profile: the profile's memory folder, never the owner's", async () => {
    withConfigDir('/tmp/profile/config');
    mountEmpty();
    expect(
      await screen.findByText(/Goose stores them in \/tmp\/profile\/config\/memory\//)
    ).toBeTruthy();
    expect(screen.queryByText(/~\/\.config\/goose/)).toBeNull();
  });
});
