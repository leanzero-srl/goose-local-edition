import { act, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { EditionProvider, useEdition } from './EditionContext';

const setSetting = vi.fn().mockResolvedValue(undefined);
const getSetting = vi.fn().mockResolvedValue('standard');

beforeEach(() => {
  setSetting.mockClear();
  getSetting.mockClear();
  // Minimal electron bridge for the context.
  (window as unknown as { electron: unknown }).electron = { setSetting, getSetting };
  document.documentElement.className = '';
  localStorage.clear();
});

function Probe() {
  const { edition, setEdition, isLocal } = useEdition();
  return (
    <div>
      <span data-testid="edition">{edition}</span>
      <span data-testid="isLocal">{String(isLocal)}</span>
      <button data-testid="to-local" onClick={() => setEdition('local')}>
        local
      </button>
      <button data-testid="to-standard" onClick={() => setEdition('standard')}>
        standard
      </button>
    </div>
  );
}

describe('EditionContext', () => {
  it('defaults to standard with no .local-edition class', () => {
    const { getByTestId } = render(
      <EditionProvider>
        <Probe />
      </EditionProvider>
    );
    expect(getByTestId('edition').textContent).toBe('standard');
    expect(document.documentElement.classList.contains('local-edition')).toBe(false);
  });

  it('setEdition("local") stamps the class, persists, and mirrors to localStorage', async () => {
    const { getByTestId } = render(
      <EditionProvider>
        <Probe />
      </EditionProvider>
    );
    // Let the mount-time settings load settle before toggling (so it can't race the click).
    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      getByTestId('to-local').click();
    });
    expect(document.documentElement.classList.contains('local-edition')).toBe(true);
    expect(getByTestId('isLocal').textContent).toBe('true');
    expect(setSetting).toHaveBeenCalledWith('edition', 'local');
    expect(localStorage.getItem('edition')).toBe('local');

    await act(async () => {
      getByTestId('to-standard').click();
    });
    expect(document.documentElement.classList.contains('local-edition')).toBe(false);
    expect(setSetting).toHaveBeenCalledWith('edition', 'standard');
  });
});
