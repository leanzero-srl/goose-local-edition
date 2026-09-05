import { act, render, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  EditionProvider,
  providerIsLocal,
  resolveEdition,
  useEdition,
} from './EditionContext';

const setSetting = vi.fn().mockResolvedValue(undefined);
const getSetting = vi.fn();
const readConfig = vi.fn();

// EditionContext derives the edition from the active provider via acp/config when no explicit
// setting is stored (dynamic import inside the effect).
vi.mock('../acp/config', () => ({
  acpReadConfig: (key: string, isSecret: boolean) => readConfig(key, isSecret),
}));

beforeEach(() => {
  setSetting.mockClear();
  getSetting.mockReset();
  readConfig.mockReset();
  // Absent setting + no provider unless a test says otherwise.
  getSetting.mockResolvedValue(undefined);
  readConfig.mockResolvedValue(null);
  // Minimal electron bridge for the context.
  (window as unknown as { electron: unknown }).electron = { setSetting, getSetting };
  document.documentElement.className = '';
  document.title = 'Goose';
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

const renderProvider = () =>
  render(
    <EditionProvider>
      <Probe />
    </EditionProvider>
  );

const settle = async () => {
  // Let the mount-time async resolution (getSetting, then possibly acpReadConfig) drain.
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
};

// Pure resolver — mirrors the tests in crates/goose-cli/src/edition.rs.
describe('resolveEdition (Rust-resolver parity)', () => {
  it('derives local from every local provider fragment, including omlx and mlx-sidecar', () => {
    for (const p of [
      'lmstudio',
      'LMStudio',
      'ollama',
      'swarm',
      'my-llama-host',
      'localai',
      'omlx',
      'mlx-sidecar',
    ]) {
      expect(resolveEdition(undefined, p), `provider ${p} should derive local`).toBe('local');
      expect(providerIsLocal(p)).toBe(true);
    }
  });

  it('persisted value beats derivation in both directions', () => {
    expect(resolveEdition('standard', 'lmstudio')).toBe('standard');
    expect(resolveEdition('local', 'openai')).toBe('local');
  });

  it('defaults to standard for a cloud provider or none', () => {
    expect(resolveEdition(undefined, 'anthropic')).toBe('standard');
    expect(resolveEdition(undefined, null)).toBe('standard');
    expect(resolveEdition(undefined, undefined)).toBe('standard');
  });

  it('an unrecognized persisted value falls through to derivation', () => {
    expect(resolveEdition('weird', 'ollama')).toBe('local');
    expect(resolveEdition('weird', 'openai')).toBe('standard');
  });
});

describe('EditionContext', () => {
  it('with no stored setting and no provider it stays standard with no .local-edition class', async () => {
    const { getByTestId } = renderProvider();
    await settle();
    expect(getByTestId('edition').textContent).toBe('standard');
    expect(document.documentElement.classList.contains('local-edition')).toBe(false);
    expect(document.title).toBe('Goose');
  });

  it('derives local from the active provider (omlx) when no setting is stored', async () => {
    readConfig.mockResolvedValue('omlx');
    const { getByTestId } = renderProvider();
    await waitFor(() => expect(getByTestId('edition').textContent).toBe('local'));
    expect(readConfig).toHaveBeenCalledWith('GOOSE_PROVIDER', false);
    expect(document.documentElement.classList.contains('local-edition')).toBe(true);
    expect(getByTestId('isLocal').textContent).toBe('true');
    // Derivation caches for the pre-paint stamp but never writes the explicit setting.
    expect(localStorage.getItem('edition')).toBe('local');
    expect(setSetting).not.toHaveBeenCalled();
    expect(document.title).toBe('Goose Swarm');
  });

  it('an explicit "standard" setting beats a local provider, without even reading it', async () => {
    getSetting.mockResolvedValue('standard');
    readConfig.mockResolvedValue('omlx');
    // A stale derived cache must lose to the explicit setting.
    localStorage.setItem('edition', 'local');
    const { getByTestId } = renderProvider();
    await settle();
    expect(getByTestId('edition').textContent).toBe('standard');
    expect(document.documentElement.classList.contains('local-edition')).toBe(false);
    expect(readConfig).not.toHaveBeenCalled();
    expect(localStorage.getItem('edition')).toBe('standard');
  });

  it('setEdition("local") stamps the class, persists, and mirrors to localStorage', async () => {
    getSetting.mockResolvedValue('standard');
    const { getByTestId } = renderProvider();
    // Let the mount-time settings load settle before toggling (so it can't race the click).
    await settle();

    await act(async () => {
      getByTestId('to-local').click();
    });
    expect(document.documentElement.classList.contains('local-edition')).toBe(true);
    expect(getByTestId('isLocal').textContent).toBe('true');
    expect(setSetting).toHaveBeenCalledWith('edition', 'local');
    expect(localStorage.getItem('edition')).toBe('local');
    expect(document.title).toBe('Goose Swarm');

    await act(async () => {
      getByTestId('to-standard').click();
    });
    expect(document.documentElement.classList.contains('local-edition')).toBe(false);
    expect(setSetting).toHaveBeenCalledWith('edition', 'standard');
    expect(document.title).toBe('Goose');
  });

  it('never clobbers a custom window title', async () => {
    document.title = 'My Standalone App';
    readConfig.mockResolvedValue('omlx');
    const { getByTestId } = renderProvider();
    await waitFor(() => expect(getByTestId('edition').textContent).toBe('local'));
    expect(document.title).toBe('My Standalone App');
  });
});
