import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ModelFieldDto } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ModelCustomFields } from './ModelCustomFields';
import { parseNumberDraft, withFieldValue } from './modelFieldsLogic';
import type { ModelFieldsListing, ModelFieldValues } from '../../acp/modelFields';

const mockList = vi.fn();
const mockSave = vi.fn();
vi.mock('../../acp/modelFields', () => ({
  acpListModelFields: (...args: unknown[]) => mockList(...args),
  acpSaveModelFields: (...args: unknown[]) => mockSave(...args),
}));

/** What the engine answered for deepseek/deepseek-v4.1-flash from OpenRouter's live listing. */
const EFFORT: ModelFieldDto = {
  id: 'effort',
  label: 'Effort',
  description: 'Sent as reasoning.effort. The levels are the ones OpenRouter lists for this model.',
  kind: { type: 'select', options: ['max', 'high', 'low'] },
  modelDefault: 'high',
};
const DOCUMENTED_EFFORT: ModelFieldDto = {
  ...EFFORT,
  kind: {
    type: 'select',
    options: ['max', 'xhigh', 'high', 'medium', 'low', 'minimal', 'none'],
  },
  modelDefault: undefined,
};
const TOP_K: ModelFieldDto = {
  id: 'top_k',
  label: 'Top K',
  description: 'Sent as top_k.',
  kind: { type: 'number', min: 0, max: null, integer: true },
};

function listing(fields: ModelFieldDto[], values: ModelFieldValues = {}): ModelFieldsListing {
  return { fields, source: fields.length ? 'provider_metadata' : 'none', values };
}

function mount(props: Partial<Parameters<typeof ModelCustomFields>[0]> = {}) {
  return render(
    <IntlTestWrapper>
      <ModelCustomFields
        providerId="openrouter"
        modelId="deepseek/deepseek-v4.1-flash"
        {...props}
      />
    </IntlTestWrapper>
  );
}

beforeEach(() => {
  mockList.mockReset();
  mockSave.mockReset();
});
afterEach(cleanup);

describe('ModelCustomFields', () => {
  it('shows the levels the listing declares and saves the chosen effort for this model', async () => {
    mockList.mockResolvedValue(listing([EFFORT, TOP_K]));
    mockSave.mockImplementation(async (_p: string, _m: string, values: ModelFieldValues) => values);
    const heard = vi.fn();
    mount({ onValuesChange: heard });

    const strip = await screen.findByRole('radiogroup', { name: 'Effort' });
    expect(mockList).toHaveBeenCalledWith('openrouter', 'deepseek/deepseek-v4.1-flash');
    expect(strip.textContent).toContain('Model default (high)');
    expect(screen.getByTestId('model-field-effort-max')).toBeTruthy();
    expect(screen.queryByTestId('model-field-effort-medium')).toBeNull();

    await userEvent.click(screen.getByTestId('model-field-effort-low'));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith('openrouter', 'deepseek/deepseek-v4.1-flash', {
        effort: 'low',
      })
    );
    await waitFor(() => expect(heard).toHaveBeenLastCalledWith({ effort: 'low' }));
  });

  it('puts a long documented level list in a dropdown, never a native select', async () => {
    mockList.mockResolvedValue(listing([DOCUMENTED_EFFORT]));
    const { container } = mount();
    await screen.findByTestId('model-field-effort-trigger');
    expect(container.querySelector('select')).toBeNull();
  });

  it('renders nothing for a model with no fields', async () => {
    mockList.mockResolvedValue(listing([]));
    const { container } = mount();
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    await waitFor(() => expect(container.textContent).toBe(''));
  });

  it('says so when the listing does not carry the typed model id', async () => {
    mockList.mockResolvedValue({ fields: [], source: 'unlisted_model', values: {} });
    mount({ modelId: 'vendor/typo' });
    const notice = await screen.findByTestId('model-custom-fields-unlisted');
    expect(notice.textContent).toContain('vendor/typo');
  });

  it('a pinned field shows the pin in place of its control, never the saved value', async () => {
    mockList.mockResolvedValue(listing([EFFORT], { effort: 'low' }));
    mount({ pinned: { effort: 'Forge runs every model at reasoning effort medium' } });
    const row = await screen.findByTestId('model-field-row-effort');
    expect(row.textContent).toContain(
      'Pinned for this run: Forge runs every model at reasoning effort medium'
    );
    expect(screen.queryByRole('radiogroup', { name: 'Effort' })).toBeNull();
    expect(row.textContent).not.toContain('low');
  });

  it('a refused save is shown and the value goes back to the saved one', async () => {
    mockList.mockResolvedValue(listing([EFFORT], { effort: 'max' }));
    mockSave.mockRejectedValue({ data: "Effort 'low' is not one of: max, high" });
    mount();
    await screen.findByRole('radiogroup', { name: 'Effort' });
    await userEvent.click(screen.getByTestId('model-field-effort-low'));
    const alert = await screen.findByTestId('model-custom-fields-save-error');
    expect(alert.textContent).toContain('is not one of');
    expect(screen.getByTestId('model-field-effort-max').getAttribute('aria-checked')).toBe('true');
  });

  it('a number commits on Enter and an empty box clears it back to the default', async () => {
    mockList.mockResolvedValue(listing([TOP_K], { top_k: 40 }));
    mockSave.mockImplementation(async (_p: string, _m: string, values: ModelFieldValues) => values);
    mount();
    const input = (await screen.findByTestId('model-field-top_k')) as HTMLInputElement;
    expect(input.value).toBe('40');
    await userEvent.clear(input);
    await userEvent.type(input, '{Enter}');
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith('openrouter', 'deepseek/deepseek-v4.1-flash', {})
    );
  });

  it('reads nothing until a provider and a model are chosen', async () => {
    const { container } = mount({ providerId: '' });
    expect(container.textContent).toBe('');
    expect(mockList).not.toHaveBeenCalled();
  });
});

describe('parseNumberDraft', () => {
  it('checks the declared range and integer-ness', () => {
    const temperature: ModelFieldDto = {
      ...TOP_K,
      id: 'temperature',
      kind: { type: 'number', min: 0, max: 2, integer: false },
    };
    expect(parseNumberDraft('', temperature)).toEqual({ kind: 'clear' });
    expect(parseNumberDraft(' 0.7 ', temperature)).toEqual({ kind: 'value', value: 0.7 });
    expect(parseNumberDraft('2.5', temperature)).toEqual({ kind: 'invalid', reason: 'above' });
    expect(parseNumberDraft('-1', temperature)).toEqual({ kind: 'invalid', reason: 'below' });
    expect(parseNumberDraft('abc', temperature)).toEqual({
      kind: 'invalid',
      reason: 'not-a-number',
    });
    expect(parseNumberDraft('4.5', TOP_K)).toEqual({ kind: 'invalid', reason: 'not-whole' });
  });
});

describe('withFieldValue', () => {
  it('sets a value and removes one cleared to the default', () => {
    expect(withFieldValue({ effort: 'low' }, 'top_k', 40)).toEqual({ effort: 'low', top_k: 40 });
    expect(withFieldValue({ effort: 'low', top_k: 40 }, 'effort', null)).toEqual({ top_k: 40 });
  });
});
