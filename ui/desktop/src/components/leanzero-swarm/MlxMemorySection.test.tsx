import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { cleanup, render as rtlRender, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { MlxEngineSettings, MlxEngineStatus, MlxMemoryMachine } from '../../acp/mlx-engine';

const mockMachine = vi.fn();
const mockWiredApply = vi.fn();
const mockWiredRemove = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineMemoryMachine: (...a: unknown[]) => mockMachine(...a),
  mlxEngineWiredLimitApply: (...a: unknown[]) => mockWiredApply(...a),
  mlxEngineWiredLimitRemoveDaemon: (...a: unknown[]) => mockWiredRemove(...a),
}));

import { MlxMemorySection } from './MlxMemorySection';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });
afterEach(cleanup);

const GIB = 1024 ** 3;
const machine = (over: Partial<MlxMemoryMachine> = {}): MlxMemoryMachine => ({
  physicalBytes: 128 * GIB,
  availableBytes: 90e9,
  workingSetBytes: 103e9,
  wiredLimitMb: 0,
  defaultFloorGb: 7,
  tunablesSupported: true,
  launcher: 'uvx rapid-mlx',
  ...over,
});
const settings = (memory = {}): MlxEngineSettings => ({
  modelsDir: '~/.goose/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
  modelId: 'org/m',
  memory,
});
const running = (over: Partial<MlxEngineStatus> = {}): MlxEngineStatus => ({
  state: 'running',
  modelId: 'org/m',
  restartRequired: false,
  availableMemoryGb: 80,
  totalMemoryGb: 128,
  activeRequests: 0,
  memoryBudget: {
    limitBytes: 99e9,
    source: 'auto-context',
    binding: 'engine',
    kvBytesPerToken: 65536,
    weightsBytes: 40e9,
    prefixBudgetBytes: 10e9,
    activeBytes: 50e9,
    peakBytes: 60e9,
    othersBytes: 12e9,
    freeBytes: 20e9,
  },
  ...over,
});

function mount(props: Partial<React.ComponentProps<typeof MlxMemorySection>> = {}) {
  const saveSettings = vi.fn(async (next: MlxEngineSettings) => next);
  const onApply = vi.fn();
  render(
    <MlxMemorySection
      macPicker={null}
      isSelf
      macName="workhorse"
      status={null}
      settings={settings()}
      engineBusy={false}
      saveSettings={saveSettings}
      onApply={onApply}
      {...props}
    />
  );
  return { saveSettings, onApply };
}

beforeEach(() => {
  mockMachine.mockReset().mockResolvedValue(machine());
  mockWiredApply.mockReset().mockResolvedValue({ wiredLimitMb: 122000, command: 'x' });
  mockWiredRemove.mockReset().mockResolvedValue({ wiredLimitMb: 122000, command: 'x' });
});

describe('MlxMemorySection', () => {
  it('shows every tunable with its unit, range, flag and default', async () => {
    mount();
    expect(await screen.findByText('goose default 7 GB (5% of RAM)')).toBeTruthy();
    for (const flag of [
      '--metal-limit-gb',
      '--memory-floor-gb',
      '--cache-memory-mb',
      '--prefill-step-size',
      '--max-model-len',
      '--memory-guard-margin-gb',
      '--max-concurrent-requests',
      '--auto-utilization-floor',
      '--auto-utilization-ceiling',
      '--auto-headroom-fraction',
      '--system-reserve-fraction',
    ]) {
      expect(screen.getByText(flag)).toBeTruthy();
    }
    const cap = screen.getByTestId('mlx-memory-field-metalLimitGb');
    expect(within(cap).getByText('GB')).toBeTruthy();
    expect(within(cap).getByText(/valid: above 0, up to 137\.4/)).toBeTruthy();
  });

  it('a preset only fills the fields; Save sends them', async () => {
    const { saveSettings } = mount();
    await screen.findByText('goose default 7 GB (5% of RAM)');
    await userEvent.click(screen.getByRole('button', { name: 'Inference box' }));
    expect((screen.getByLabelText('Engine Metal cap') as HTMLInputElement).value).toBe('112');
    expect((screen.getByLabelText('Free-memory floor') as HTMLInputElement).value).toBe('4');
    expect(saveSettings).not.toHaveBeenCalled();
    expect(screen.getByTestId('mlx-memory-cap-above-ws').textContent).toContain('122,000 MB');
    await userEvent.click(screen.getByTestId('mlx-memory-save'));
    expect(saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ memory: { metalLimitGb: 112, memoryFloorGb: 4 } })
    );
  });

  it('refuses an impossible combination with a clear message and no Save', async () => {
    const { saveSettings } = mount();
    await screen.findByText('goose default 7 GB (5% of RAM)');
    await userEvent.type(screen.getByLabelText('Engine Metal cap'), '130');
    await userEvent.type(screen.getByLabelText('Free-memory floor'), '11');
    expect(screen.getByTestId('mlx-memory-problem-capPlusFloor').textContent).toContain(
      'Cap 130 GB + floor 11 GB is more than this Mac’s 137.4 GB of RAM.'
    );
    expect((screen.getByTestId('mlx-memory-save') as HTMLButtonElement).disabled).toBe(true);
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it('shows the live consequence and the KV-over-cap refusal from the running engine', async () => {
    mount({ status: running() });
    await screen.findByText('goose default 7 GB (5% of RAM)');
    await userEvent.type(screen.getByLabelText('Engine Metal cap'), '80');
    const room = 80e9 - 40e9 - 10e9 - 0.02 * 80e9;
    expect(screen.getByTestId('mlx-memory-consequences').textContent).toContain(
      Math.floor(room / 65536).toLocaleString('en-US')
    );
    await userEvent.type(screen.getByLabelText('Max context (admitted window)'), '2000000');
    expect(screen.getByTestId('mlx-memory-problem-kvOverCap')).toBeTruthy();
  });

  it('a running engine restarts only on Apply, and never while it serves requests', async () => {
    const { onApply } = mount({ status: running({ restartRequired: true }) });
    await screen.findByTestId('mlx-memory-restart-needed');
    const apply = screen.getByTestId('mlx-memory-apply') as HTMLButtonElement;
    expect(apply.disabled).toBe(false);
    expect(onApply).not.toHaveBeenCalled();
    await userEvent.click(apply);
    expect(onApply).toHaveBeenCalledTimes(1);
    cleanup();
    const busy = mount({ status: running({ restartRequired: true, activeRequests: 2 }) });
    expect((await screen.findByTestId('mlx-memory-busy')).textContent).toContain('2 requests');
    expect((screen.getByTestId('mlx-memory-apply') as HTMLButtonElement).disabled).toBe(true);
    expect(busy.onApply).not.toHaveBeenCalled();
  });

  it('the wired-limit buttons build the right command and pass persist apart', async () => {
    mount();
    const panel = await screen.findByTestId('mlx-memory-wired');
    expect(within(panel).getByTestId('mlx-memory-wired-command').textContent).toBe(
      '/usr/sbin/sysctl iogpu.wired_limit_mb=122000'
    );
    await userEvent.click(within(panel).getByRole('button', { name: 'Set until reboot' }));
    expect(mockWiredApply).toHaveBeenCalledWith(122000, false);
    await userEvent.click(within(panel).getByRole('button', { name: 'Set and keep after reboot' }));
    expect(mockWiredApply).toHaveBeenLastCalledWith(122000, true);
    const input = within(panel).getByLabelText('New limit');
    await userEvent.clear(input);
    await userEvent.type(input, '4000');
    expect(within(panel).getByRole('alert').textContent).toContain('8192 to 131,072');
    expect(
      (within(panel).getByRole('button', { name: 'Set until reboot' }) as HTMLButtonElement)
        .disabled
    ).toBe(true);
  });

  it('offers removal of an installed boot setting', async () => {
    mockMachine.mockResolvedValue(machine({ wiredLimitMb: 122000, daemonWiredLimitMb: 122000 }));
    mount();
    await userEvent.click(await screen.findByRole('button', { name: 'Remove boot setting' }));
    expect(mockWiredRemove).toHaveBeenCalledTimes(1);
  });

  it('marks the lz.20 fields when this Mac launches an older engine', async () => {
    mockMachine.mockResolvedValue(
      machine({ tunablesSupported: false, launcher: 'uvx …@v0.14.3-lz.18' })
    );
    mount();
    expect((await screen.findByTestId('mlx-memory-needs-engine')).textContent).toContain('lz.18');
    const prefill = screen.getByTestId('mlx-memory-field-prefillStepSize');
    expect(within(prefill).queryByText('lz.20')).toBeNull();
    expect(
      within(screen.getByTestId('mlx-memory-field-metalLimitGb')).getByText('lz.20')
    ).toBeTruthy();
  });

  it('another Mac shows its tunables but sets the wired limit only at that Mac', async () => {
    mount({ isSelf: false, macName: 'workmule', status: running() });
    expect(await screen.findByText(/open goose on workmule/)).toBeTruthy();
    expect(mockMachine).not.toHaveBeenCalled();
    expect(screen.queryByTestId('mlx-memory-wired')).toBeNull();
  });
});
