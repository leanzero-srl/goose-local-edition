import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { MlxNgramTableFields } from './MlxNgramTableFields';
import { draftsFromProfile, profileFromDrafts, profileHasValues } from './MlxEngineView';
import type { MlxLocalModel } from '../../acp/mlx-engine';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });
afterEach(cleanup);

const model = (over: Partial<MlxLocalModel>): MlxLocalModel => ({
  id: 'lz/test',
  sizeBytes: 1,
  complete: true,
  missingFiles: 0,
  ...over,
});

describe('the n-gram table draft', () => {
  it('round-trips On and Off and leaves auto off the wire', () => {
    for (const on of [true, false]) {
      const drafts = draftsFromProfile({ pleOffload: on });
      expect(drafts.pleOffload).toBe(String(on));
      expect(profileFromDrafts(drafts)).toEqual({ pleOffload: on });
      expect(profileHasValues({ pleOffload: on })).toBe(true);
    }
    expect(profileFromDrafts(draftsFromProfile({}))).toEqual({});
  });
});

describe('MlxNgramTableFields', () => {
  it('is not offered for a model without the table', () => {
    const { container } = render(
      <MlxNgramTableFields model={model({})} drafts={draftsFromProfile({})} setDraft={vi.fn()} />
    );
    expect(container.textContent).toBe('');
  });

  it('shows what auto does on this Mac and drafts the choice', async () => {
    const setDraft = vi.fn();
    render(
      <MlxNgramTableFields
        model={model({ hasNgramTable: true, ngramTableAutoOn: true })}
        drafts={draftsFromProfile({})}
        setDraft={setDraft}
      />
    );
    expect(screen.getByText('auto: on for this Mac')).toBeTruthy();
    await userEvent.click(screen.getByTestId('mlx-ngram-off'));
    expect(setDraft).toHaveBeenCalledWith('pleOffload', 'false');
  });
});
