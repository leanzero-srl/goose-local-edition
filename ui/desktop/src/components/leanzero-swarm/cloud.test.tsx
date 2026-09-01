import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CLOUD_PROVIDERS, CloudPane, cloudCliErr } from './cloud';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

// The cloud pane runs the WHOLE contract through the engine CLI over IPC; here that IPC is the
// one mock, and the assertions are about what the pane shows and which CLI call each control makes.
const mockSwarmCloud = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  (window as unknown as { electron: unknown }).electron = { swarmCloud: mockSwarmCloud };
});

const BEDROCK = CLOUD_PROVIDERS[0];
const inPool = [
  { id: 'd1', model_id: 'anthropic.claude', provider: 'bedrock' },
] as unknown as SwarmDeviceRow[];

function readyRoster() {
  mockSwarmCloud.mockImplementation(async (_p: string, args: string[]) => {
    if (args[0] === 'models')
      return {
        ok: true,
        stdout: JSON.stringify({ region: 'eu-west-1', models: ['anthropic.claude', 'meta.llama'] }),
        stderr: '',
        error: null,
      };
    return { ok: true, stdout: '', stderr: '', error: null };
  });
}

describe('CloudPane — Studio register', () => {
  it('a stored key: KeyValue status, the pool and the roster as tables, one Add per model not in the pool', async () => {
    readyRoster();
    const onChanged = vi.fn(async () => {});
    const onAdded = vi.fn(async () => {});
    const { container } = render(
      <CloudPane
        def={BEDROCK}
        devices={inPool}
        onChanged={onChanged}
        onAdded={onAdded}
        addWeight={3}
      />
    );
    await waitFor(() => {
      expect(screen.getByText('key valid')).toBeInTheDocument();
    });
    const status = screen.getByLabelText('Amazon Bedrock key status');
    expect(status).toHaveTextContent('eu-west-1');
    expect(status).toHaveTextContent('2 models');
    // The pool table names the provider quietly and the model in mono; Remove is a ghost action.
    const pool = screen.getByRole('table', { name: 'Amazon Bedrock nodes in the pool' });
    expect(pool).toHaveTextContent('Bedrock');
    expect(pool).toHaveTextContent('anthropic.claude');
    expect(screen.getByRole('button', { name: 'Remove' })).toBeInTheDocument();
    // The roster: the pooled model is a tone chip, the other offers + Add with the dialog's weight.
    expect(screen.getByText('in pool').getAttribute('data-tone')).toBe('ok');
    await userEvent.click(screen.getByRole('button', { name: '+ Add' }));
    await waitFor(() => {
      expect(mockSwarmCloud).toHaveBeenCalledWith('bedrock', [
        'add',
        'meta.llama',
        '--weight',
        '3',
      ]);
    });
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(onAdded).toHaveBeenCalledWith('meta.llama');
    // The filter narrows the roster and the header counts what the table shows.
    await userEvent.type(screen.getByLabelText('Filter Amazon Bedrock models'), 'llama');
    expect(screen.getAllByTestId('lz-section-count').map((c) => c.textContent)).toContain('1');
    await userEvent.type(screen.getByLabelText('Filter Amazon Bedrock models'), 'zzz');
    expect(screen.getByText('no model matches the filter')).toBeInTheDocument();
    assertStudioClean(container);
    const utilities = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(utilities)).toEqual([]);
  });

  it('Replace key opens the Studio inputs; Validate & save sends the key and region; a refusal is a loud banner', async () => {
    readyRoster();
    render(<CloudPane def={BEDROCK} devices={[]} onChanged={async () => {}} />);
    await waitFor(() => {
      expect(screen.getByText('key valid')).toBeInTheDocument();
    });
    await userEvent.click(screen.getByRole('button', { name: 'Replace key' }));
    const key = screen.getByLabelText('Amazon Bedrock API key');
    expect(key).toHaveAttribute('type', 'password');
    expect(screen.getByRole('button', { name: 'Validate & save' })).toBeDisabled();
    mockSwarmCloud.mockResolvedValueOnce({
      ok: false,
      stdout: '',
      stderr: 'Error: Bedrock rejected the key',
      error: null,
    });
    await userEvent.type(key, 'ABSK-test');
    await userEvent.click(screen.getByRole('button', { name: 'Validate & save' }));
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Bedrock rejected the key');
    });
    expect(mockSwarmCloud).toHaveBeenLastCalledWith('bedrock', [
      'key',
      'ABSK-test',
      '--json',
      '--region',
      'eu-west-1',
    ]);
  });

  it('no stored key: the entry form, and the CLI error line reads the Error: tail', async () => {
    mockSwarmCloud.mockResolvedValue({
      ok: false,
      stdout: '',
      stderr: 'no bedrock API key stored',
      error: null,
    });
    render(<CloudPane def={BEDROCK} devices={[]} onChanged={async () => {}} />);
    await waitFor(() => {
      expect(screen.getByLabelText('Amazon Bedrock API key')).toBeInTheDocument();
    });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(cloudCliErr({ stdout: '', stderr: 'x\nError: boom here', error: null })).toBe(
      'boom here'
    );
  });
});
