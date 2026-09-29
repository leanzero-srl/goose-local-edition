import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SecurityToggle } from './SecurityToggle';
import { IntlTestWrapper } from '../../../i18n/test-utils';

const upsert = vi.fn().mockResolvedValue(undefined);

vi.mock('../../ConfigContext', () => ({
  useConfig: () => ({
    config: {
      SECURITY_PROMPT_ENABLED: true,
      SECURITY_PROMPT_CLASSIFIER_ENABLED: true,
      SECURITY_PROMPT_CLASSIFIER_MODEL: 'deberta-small',
    },
    upsert,
  }),
}));

vi.mock('../../../utils/analytics', () => ({
  trackSettingToggled: vi.fn(),
}));

const MAPPING = JSON.stringify({
  'deberta-small': { model_type: 'prompt' },
  'deberta-large': { model_type: 'prompt' },
  'shell-guard': { model_type: 'command' },
});

describe('SecurityToggle detection model', () => {
  const originalAppConfig = window.appConfig;

  beforeEach(() => {
    upsert.mockClear();
    Object.defineProperty(window, 'appConfig', {
      configurable: true,
      value: {
        get: (key: string) => (key === 'SECURITY_ML_MODEL_MAPPING' ? MAPPING : undefined),
      },
    });
  });

  afterEach(() => {
    Object.defineProperty(window, 'appConfig', { configurable: true, value: originalAppConfig });
  });

  it('renders the model choice as the Studio listbox, never a native <select>', () => {
    const { container } = render(<SecurityToggle />, { wrapper: IntlTestWrapper });

    expect(container.querySelector('select')).toBeNull();
    expect(screen.getByRole('combobox', { name: 'Detection Model' })).toHaveTextContent(
      'deberta-small'
    );
  });

  it('picking a model upserts it', async () => {
    const user = userEvent.setup();
    render(<SecurityToggle />, { wrapper: IntlTestWrapper });

    await user.click(screen.getByRole('combobox', { name: 'Detection Model' }));
    expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'deberta-small',
      'deberta-large',
    ]);
    await user.click(screen.getByRole('option', { name: 'deberta-large' }));

    expect(upsert).toHaveBeenCalledWith('SECURITY_PROMPT_CLASSIFIER_MODEL', 'deberta-large', false);
  });
});
