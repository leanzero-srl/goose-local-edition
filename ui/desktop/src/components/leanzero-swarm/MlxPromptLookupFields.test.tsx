import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { MlxPromptLookupFields, promptLookupLane } from './MlxPromptLookupFields';
import {
  draftsEqual,
  draftsFromProfile,
  profileFromDrafts,
  profileHasValues,
} from './MlxEngineView';
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
const QWEN_MTP = model({ hasMtpHead: true, hybrid: true });
const QWEN_FLASH_NO_HEAD = model({ hasMtpHead: false, hybrid: true });
const MELLUM = model({ hasMtpHead: false, hybrid: false });

describe('promptLookupLane mirrors prompt_lookup::lane()', () => {
  it('rides inside MTP whenever the head is served', () => {
    expect(promptLookupLane(QWEN_MTP, '')).toEqual({ kind: 'mtp' });
    expect(promptLookupLane(QWEN_MTP, 'mtp')).toEqual({ kind: 'mtp' });
  });
  it('a hybrid without MTP has no lane, a pure-attention model takes suffix decoding', () => {
    expect(promptLookupLane(QWEN_MTP, 'off')).toEqual({ kind: 'unavailable', reason: 'hybrid' });
    expect(promptLookupLane(QWEN_FLASH_NO_HEAD, '')).toEqual({
      kind: 'unavailable',
      reason: 'hybrid',
    });
    expect(promptLookupLane(MELLUM, '')).toEqual({ kind: 'suffix' });
  });
  it('an unreadable config is unavailable with the read error', () => {
    expect(promptLookupLane(model({ hybridError: 'config.json missing' }), '')).toEqual({
      kind: 'unavailable',
      reason: 'config.json missing',
    });
  });
});

describe('prompt-lookup drafts', () => {
  it('round-trip the profile and leave auto off the payload', () => {
    const profile = { promptLookup: { mode: 'on' as const, minNgram: 4, maxDraft: 63 } };
    const drafts = draftsFromProfile(profile);
    expect(drafts.promptLookupMode).toBe('on');
    expect(drafts.promptLookupMaxNgram).toBe('');
    expect(profileFromDrafts(drafts)).toEqual(profile);
    expect(profileHasValues(profileFromDrafts(drafts))).toBe(true);
    expect(profileFromDrafts(draftsFromProfile(undefined)).promptLookup).toBeUndefined();
  });
  it('drop junk and non-positive numbers instead of sending them', () => {
    const drafts = {
      ...draftsFromProfile(undefined),
      promptLookupMinNgram: '0',
      promptLookupMaxDraft: '2.5',
    };
    expect(profileFromDrafts(drafts).promptLookup).toBeUndefined();
  });
  it('count as an edit for the dirty check', () => {
    const base = draftsFromProfile(undefined);
    expect(draftsEqual(base, { ...base, promptLookupMode: 'off' })).toBe(false);
    expect(draftsEqual(base, { ...base, promptLookupMaxDraft: '95' })).toBe(false);
  });
});

describe('MlxPromptLookupFields', () => {
  it('inside MTP shows all three numbers and writes the mode draft', async () => {
    const setDraft = vi.fn();
    render(
      <MlxPromptLookupFields
        model={QWEN_MTP}
        drafts={draftsFromProfile(undefined)}
        setDraft={setDraft}
      />
    );
    expect(screen.getByText('inside MTP')).toBeTruthy();
    expect(screen.getByLabelText('Shortest n-gram')).toBeTruthy();
    expect(screen.getByLabelText('Longest draft (tokens)')).toBeTruthy();
    await userEvent.click(screen.getByTestId('mlx-pl-on'));
    expect(setDraft).toHaveBeenCalledWith('promptLookupMode', 'on');
  });

  it('on the suffix lane hides the MTP-only shortest n-gram', () => {
    render(
      <MlxPromptLookupFields
        model={MELLUM}
        drafts={draftsFromProfile(undefined)}
        setDraft={vi.fn()}
      />
    );
    expect(screen.getByText('suffix decoding')).toBeTruthy();
    expect(screen.queryByLabelText('Shortest n-gram')).toBeNull();
    expect(screen.getByLabelText('Longest n-gram')).toBeTruthy();
  });

  it('locks a hybrid served without MTP, says why, and keeps Auto reachable', () => {
    render(
      <MlxPromptLookupFields
        model={QWEN_FLASH_NO_HEAD}
        drafts={{ ...draftsFromProfile(undefined), promptLookupMode: 'on' }}
        setDraft={vi.fn()}
      />
    );
    expect(screen.getByText(/Unavailable for this model: it mixes linear-attention/)).toBeTruthy();
    expect((screen.getByTestId('mlx-pl-off') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('mlx-pl-on') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByTestId('mlx-pl-auto') as HTMLButtonElement).disabled).toBe(false);
    expect(screen.queryByLabelText('Longest n-gram')).toBeNull();
  });
});
