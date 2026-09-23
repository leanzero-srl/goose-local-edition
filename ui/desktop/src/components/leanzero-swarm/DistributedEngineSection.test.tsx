import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render as rtlRender, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  DistributedEngineSection,
  type DistributedEngineSectionProps,
} from './DistributedEngineSection';
import {
  FLASH_CONFIG,
  FLASH_MODEL,
  FLASH_PREFLIGHT_OK,
  FLASH_PREFLIGHT_REFUSED,
  FLASH_READY,
  STOPPED_WITH_CONFIG,
} from './mlxDistributed.fixtures';
import type { MlxEngineStatus } from '../../acp/mlx-engine';

const mockPreflight = vi.fn();
const mockStart = vi.fn();
const mockStop = vi.fn();
const mockConfigUpdate = vi.fn();
vi.mock('../../acp/mlx-distributed', () => ({
  mlxDistributedPreflight: (...a: unknown[]) => mockPreflight(...a),
  mlxDistributedStart: (...a: unknown[]) => mockStart(...a),
  mlxDistributedStop: (...a: unknown[]) => mockStop(...a),
  mlxDistributedConfigUpdate: (...a: unknown[]) => mockConfigUpdate(...a),
}));
const mockUnmount = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: (...a: unknown[]) => mockUnmount(...a),
}));

const SINGLE_RUNNING: MlxEngineStatus = {
  state: 'running',
  modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  servedModelId: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  restartRequired: false,
  availableMemoryGb: 70,
  totalMemoryGb: 128,
};

const onRefresh = vi.fn().mockResolvedValue(undefined);
const onSingleChanged = vi.fn();

function section(overrides: Partial<DistributedEngineSectionProps> = {}) {
  const props: DistributedEngineSectionProps = {
    capability: true,
    peerHostname: null,
    status: FLASH_READY,
    statusError: null,
    onRefresh,
    models: [
      { id: FLASH_MODEL, sizeBytes: 98e9, complete: true, missingFiles: 0 },
      { id: 'org/other-model', sizeBytes: 20e9, complete: true, missingFiles: 0 },
    ],
    singleStatus: null,
    onSingleChanged,
    ...overrides,
  };
  return rtlRender(<DistributedEngineSection {...props} />, { wrapper: IntlTestWrapper });
}

async function expectDesigned(container: HTMLElement) {
  assertStudioClean(container);
  const utilities = allClasses(container).filter((c) => !c.startsWith('lucide'));
  expect(await missingUtilities(utilities)).toEqual([]);
}

beforeEach(() => {
  mockPreflight.mockReset();
  mockStart.mockReset();
  mockStop.mockReset();
  mockConfigUpdate.mockReset();
  mockUnmount.mockReset().mockResolvedValue(undefined);
  onRefresh.mockClear();
  onSingleChanged.mockClear();
});

describe('DistributedEngineSection — READY, 2 nodes over JACCL (the recorded Flash run)', () => {
  it('says the mode and state, and each rank its layers and peak against its budget', async () => {
    const { container } = section();
    const mode = screen.getByTestId('mlx-dist-mode');
    expect(mode).toHaveAttribute('data-mode', 'distributed');
    expect(screen.getByTestId('mlx-dist-mode-text')).toHaveTextContent(
      'Distributed · 2 nodes · JACCL'
    );
    expect(within(mode).getByText('Ready')).toBeInTheDocument();

    const [macbook, workhorse] = screen.getAllByTestId('mlx-dist-node');
    expect(macbook).toHaveAttribute('data-node', 'MacBook Pro');
    expect(within(macbook).getByTestId('mlx-dist-node-layers')).toHaveTextContent(
      'Layers 0–19 · 20 layers'
    );
    expect(within(macbook).getByTestId('mlx-dist-node-peak')).toHaveTextContent('61.0');
    expect(within(macbook).getByTestId('mlx-dist-node-budget')).toHaveTextContent(
      'GiB peak of 83.4 GiB budget'
    );
    expect(within(macbook).getByText('coordinator · rank 0')).toBeInTheDocument();
    expect(within(macbook).getByTestId('mlx-dist-node-link')).toHaveTextContent(
      'JACCL · 192.168.0.1 · en3 · 80 Gb/s'
    );
    expect(
      within(macbook).getByText('Caps: memory 96.0 · wired 76.8 · cache 8.0 GiB')
    ).toBeInTheDocument();

    expect(within(workhorse).getByTestId('mlx-dist-node-layers')).toHaveTextContent(
      'Layers 20–47 · 28 layers'
    );
    expect(within(workhorse).getByTestId('mlx-dist-node-peak')).toHaveTextContent('42.5');
    // 61.6 GiB available × 0.90 = 55.44 GiB — the budget the planner printed, to one decimal.
    expect(within(workhorse).getByTestId('mlx-dist-node-budget')).toHaveTextContent(
      'GiB peak of 55.4 GiB budget'
    );
    const bars = within(workhorse).getAllByRole('progressbar', {
      name: 'Peak memory against the budget',
    });
    expect(bars[0]).toHaveAttribute('aria-valuenow', '77');

    await expectDesigned(container);
  });

  it('admission, in flight and liveness are the supervisor’s own numbers', () => {
    section();
    expect(screen.getByTestId('mlx-dist-admission')).toHaveAttribute('data-open', 'true');
    expect(screen.getByText('Admitting requests')).toBeInTheDocument();
    expect(screen.getByTestId('mlx-dist-inflight')).toHaveTextContent('0');
    expect(screen.getByTestId('mlx-dist-liveness')).toHaveTextContent(
      '12 samples · median 410 ms · hang bound 4s · silent 800 ms'
    );
    expect(screen.getByTestId('mlx-dist-restarts')).toHaveTextContent('1');
  });

  it('a closed admission is a solid warn block that says why', () => {
    section({ status: { ...FLASH_READY, admissionOpen: false } });
    const block = screen.getByTestId('mlx-dist-admission');
    expect(block).toHaveAttribute('data-open', 'false');
    expect(block.className).toContain('bg-lz-warn-solid');
    expect(block).toHaveTextContent("A node's memory is low");
  });

  it('events: every one, newest first, the hang red and the restart and TB repair called out', () => {
    section();
    const events = screen.getAllByTestId('mlx-dist-event');
    expect(events).toHaveLength(FLASH_READY.events.length);
    expect(events.map((e) => e.getAttribute('data-kind'))).toEqual([
      'ready',
      'restart',
      'hang',
      'ready',
      'launched',
      'linkRepaired',
      'preflight',
    ]);
    const hang = events[2];
    expect(within(hang).getByText('Hang detected')).toHaveAttribute('data-tone', 'err');
    expect(within(hang).getByText('workhorse')).toBeInTheDocument();
    expect(hang).toHaveTextContent('no progress for 41.0 s (bound 10 × median 4.1 s)');
    expect(within(events[1]).getByText('Restart')).toHaveAttribute('data-tone', 'warn');
    expect(within(events[5]).getByText('Link repaired')).toHaveAttribute('data-tone', 'warn');
    // The header counts what the body shows.
    expect(screen.getByText('Supervisor events').parentElement).toHaveTextContent(
      String(FLASH_READY.events.length)
    );
  });

  it('the preflight shows every check with its numbers and the plan per rank', () => {
    section();
    const report = screen.getByTestId('mlx-dist-preflight');
    expect(report).toHaveAttribute('data-ok', 'true');
    const plans = within(report).getAllByTestId('mlx-dist-plan');
    expect(within(plans[0]).getByTestId('mlx-dist-plan-planned')).toHaveTextContent('63.6');
    expect(plans[0]).toHaveTextContent('Layers 0–19 · 20 layers');
    expect(plans[0]).toHaveTextContent('GiB planned with overhead, of 83.4 GiB budget');
    expect(plans[1]).toHaveTextContent('GiB planned with overhead, of 55.4 GiB budget');
    expect(plans[1]).toHaveTextContent(
      'weights 38.4 · state 0.2 · workspace 0.4 · prompt cache 0.0 GiB'
    );
    expect(
      within(report).getByText('92.70 GiB available of 128.00 GiB, pressure normal')
    ).toBeInTheDocument();
    expect(report).toHaveTextContent('context 8,192 (requested) · largest that fits 262,144');
  });

  it('while the run owns the Mac: Stop (confirmed), no Start, the config locked', async () => {
    mockStop.mockResolvedValue({
      stop: {
        steps: [
          'SIGTERM rank 0 pid 81234 → gone',
          'SIGTERM rank 1 pid 5521 → gone (verified over ssh)',
        ],
        verified: true,
      },
      status: STOPPED_WITH_CONFIG,
    });
    section();
    expect(screen.queryByRole('button', { name: 'Start' })).toBeNull();
    expect(screen.getByRole('combobox', { name: 'Backend' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(mockStop).not.toHaveBeenCalled();
    expect(screen.getByText('Stop the distributed engine?')).toBeInTheDocument();
    expect(screen.getByText(/Every rank on MacBook Pro, workhorse is stopped/)).toBeInTheDocument();
    const dialog = screen.getByRole('dialog');
    await userEvent.click(within(dialog).getByRole('button', { name: 'Stop' }));
    await waitFor(() => expect(mockStop).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Stopped, verified')).toBeInTheDocument();
  });

  it('an unverified stop is red and names the pid that was left', async () => {
    mockStop.mockResolvedValue({
      stop: { steps: ['SIGTERM rank 1 pid 5521 → STILL ALIVE'], verified: false },
      status: FLASH_READY,
    });
    section();
    await userEvent.click(screen.getByRole('button', { name: 'Stop' }));
    await userEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Stop' }));
    expect(await screen.findByText('Stop not verified')).toBeInTheDocument();
    expect(screen.getByText('SIGTERM rank 1 pid 5521 → STILL ALIVE')).toBeInTheDocument();
  });
});

describe('DistributedEngineSection — starting', () => {
  it('preflight refused: the refusal verbatim, the failing checks with their numbers, the plan that does not fit', async () => {
    mockStart.mockResolvedValue({
      started: false,
      refusal: {
        code: 'preflightFailed',
        message: 'preflight refused the start: workhorse memory: 39.00 GiB available of 96.00 GiB',
      },
      preflight: FLASH_PREFLIGHT_REFUSED,
    });
    const { container } = section({ status: STOPPED_WITH_CONFIG });
    expect(screen.getByTestId('mlx-dist-mode-text')).toHaveTextContent('Single · this Mac');
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));
    expect(mockStart).toHaveBeenCalledWith(null);
    expect(await screen.findByTestId('mlx-dist-refusal')).toHaveTextContent(
      'preflight refused the start: workhorse memory: 39.00 GiB available of 96.00 GiB'
    );
    const failing = screen.getByTestId('mlx-dist-failing');
    const rows = within(failing).getAllByTestId('mlx-dist-check');
    expect(rows.map((r) => r.getAttribute('data-check'))).toEqual(['plan', 'memory']);
    expect(rows[1]).toHaveTextContent('workhorse · memory');
    expect(rows[1]).toHaveTextContent('39.00 GiB available of 96.00 GiB, pressure warn');
    expect(screen.getByTestId('mlx-dist-preflight')).toHaveAttribute('data-ok', 'false');
    expect(screen.getByText('does not fit')).toHaveAttribute('data-tone', 'err');
    expect(mockUnmount).not.toHaveBeenCalled();
    await expectDesigned(container);
  });

  it('single engine mounted: the dialog offers "Unmount and continue"; confirmed → unmount, then start again', async () => {
    mockStart
      .mockResolvedValueOnce({
        started: false,
        refusal: {
          code: 'singleEngineMounted',
          message: 'the single MLX engine is mounted on this Mac; unmount it first',
        },
      })
      .mockResolvedValueOnce({ started: true, preflight: FLASH_PREFLIGHT_OK });
    section({ status: STOPPED_WITH_CONFIG, singleStatus: SINGLE_RUNNING });
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));

    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Unmount the single engine?')).toBeInTheDocument();
    expect(dialog).toHaveTextContent('mihai-qwen3.8-27b-atlassian-q8-mlx');
    // Nothing was unmounted by the refusal itself.
    expect(mockUnmount).not.toHaveBeenCalled();
    expect(mockStart).toHaveBeenCalledTimes(1);

    await userEvent.click(within(dialog).getByRole('button', { name: 'Unmount and continue' }));
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(2));
    expect(mockUnmount).toHaveBeenCalledTimes(1);
    const [firstStart, secondStart] = mockStart.mock.invocationCallOrder;
    const unmount = mockUnmount.mock.invocationCallOrder[0];
    expect(firstStart).toBeLessThan(unmount);
    expect(unmount).toBeLessThan(secondStart);
    expect(onSingleChanged).toHaveBeenCalled();
    expect(screen.queryByTestId('mlx-dist-refusal')).toBeNull();
  });

  it('single engine mounted: "Keep the single engine" unmounts nothing and starts nothing more', async () => {
    mockStart.mockResolvedValue({
      started: false,
      refusal: { code: 'singleEngineMounted', message: 'mounted' },
    });
    section({ status: STOPPED_WITH_CONFIG, singleStatus: SINGLE_RUNNING });
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));
    const dialog = await screen.findByRole('dialog');
    await userEvent.click(within(dialog).getByRole('button', { name: 'Keep the single engine' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(mockUnmount).not.toHaveBeenCalled();
    expect(mockStart).toHaveBeenCalledTimes(1);
  });

  it('a start that throws shows the backend reason verbatim', async () => {
    mockStart.mockRejectedValue(
      Object.assign(new Error('Invalid params'), {
        data: "port 8090 is the single MLX engine's port; the distributed engine serves on its own",
      })
    );
    section({ status: STOPPED_WITH_CONFIG });
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));
    expect(await screen.findByTestId('mlx-dist-action-error')).toHaveTextContent(
      "port 8090 is the single MLX engine's port"
    );
  });

  it('Preflight is a dry run of the persisted config; the repair switch is sent when on', async () => {
    mockPreflight.mockResolvedValue(FLASH_PREFLIGHT_OK);
    section({ status: STOPPED_WITH_CONFIG });
    expect(screen.getByText(/No preflight has run yet/)).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole('switch', { name: 'Repair the Thunderbolt link if a JACCL check fails' })
    );
    await userEvent.click(screen.getByRole('button', { name: 'Preflight (dry run)' }));
    expect(mockPreflight).toHaveBeenCalledWith(null, true);
    expect(await screen.findByTestId('mlx-dist-preflight')).toHaveAttribute('data-ok', 'true');
  });
});

describe('DistributedEngineSection — configuration', () => {
  it('a model change moves each node’s folder to the new id and Save keeps fields this build does not know', async () => {
    mockConfigUpdate.mockImplementation(async (c: unknown) => c);
    const persisted = { ...FLASH_CONFIG, hangRatioOnly: true, watchdogWarnRatio: 0.05 };
    section({ status: { ...STOPPED_WITH_CONFIG, config: persisted } });
    await userEvent.click(screen.getByRole('combobox', { name: 'Model' }));
    await userEvent.click(screen.getByTestId('mlx-dist-model-org/other-model'));
    await userEvent.click(screen.getByRole('button', { name: 'Save configuration' }));
    await waitFor(() => expect(mockConfigUpdate).toHaveBeenCalledTimes(1));
    const saved = mockConfigUpdate.mock.calls[0][0];
    expect(saved.modelId).toBe('org/other-model');
    expect(saved.nodes[0].modelDir).toBe('/Users/mihaiperdum/.goose/models/org/other-model');
    expect(saved.nodes[1].modelDir).toBe('/Users/workhorse/.goose/models/org/other-model');
    expect(saved.nodes[0].ssh).toBeUndefined();
    expect(saved.hangRatioOnly).toBe(true);
    expect(saved.watchdogWarnRatio).toBe(0.05);
  });

  it('an edited draft is what Start sends', async () => {
    mockStart.mockResolvedValue({ started: true, preflight: FLASH_PREFLIGHT_OK });
    section({ status: STOPPED_WITH_CONFIG });
    await userEvent.click(screen.getByRole('combobox', { name: 'Backend' }));
    await userEvent.click(screen.getByTestId('mlx-dist-backend-ring'));
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart.mock.calls[0][0]).toMatchObject({ backend: 'ring', modelId: FLASH_MODEL });
  });

  it('a node is edited in a dialog; an empty required field blocks Save and Start and is named', async () => {
    section({ status: STOPPED_WITH_CONFIG });
    await userEvent.click(screen.getByRole('button', { name: 'Edit workhorse' }));
    const dialog = await screen.findByRole('dialog');
    const ssh = within(dialog).getByRole('textbox', { name: 'ssh alias' });
    await userEvent.clear(ssh);
    await userEvent.click(within(dialog).getByRole('button', { name: 'Apply' }));
    expect(screen.getByTestId('mlx-dist-missing')).toHaveTextContent(
      'Still empty: workhorse: ssh alias'
    );
    expect(screen.getByRole('button', { name: 'Save configuration' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Start' })).toBeDisabled();
  });

  it('no saved config: Set up drafts two empty nodes and names every empty field', async () => {
    section({ status: { ...STOPPED_WITH_CONFIG, config: null } });
    expect(screen.getByText('No distributed configuration is saved yet.')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Set up' }));
    expect(screen.getAllByTestId('mlx-dist-config-node')).toHaveLength(2);
    expect(screen.getByTestId('mlx-dist-missing')).toHaveTextContent('Model, Backend, API port');
  });
});

describe('DistributedEngineSection — loud absence', () => {
  it('capability missing: the section explains why it is unavailable', async () => {
    const { container } = section({ capability: false, status: null });
    expect(screen.getByText('Distributed inference is unavailable')).toBeInTheDocument();
    expect(screen.getByText(/the mlxDistributed capability is missing/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Start' })).toBeNull();
    await expectDesigned(container);
  });

  it('a linked device selected: it says the engine is supervised from this Mac', () => {
    section({ peerHostname: 'workhorse', status: null });
    expect(
      screen.getByText('The distributed engine is supervised from this Mac')
    ).toBeInTheDocument();
    expect(screen.getByText(/You are managing workhorse/)).toBeInTheDocument();
  });

  it('an unreadable status claims nothing: no mode, no nodes, the reason and a retry', async () => {
    section({ status: null, statusError: 'connection refused' });
    expect(screen.getByText('connection refused')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-dist-mode')).toBeNull();
    expect(screen.queryByTestId('mlx-dist-node')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRefresh).toHaveBeenCalled();
  });

  it('a failed run shows its last error and offers Stop to sweep what it left', () => {
    section({
      status: {
        ...STOPPED_WITH_CONFIG,
        state: 'failed',
        lastError: 'rank 1 died: exit status 137 (watchdog CRITICAL on workhorse)',
      },
    });
    expect(
      screen.getByText('rank 1 died: exit status 137 (watchdog CRITICAL on workhorse)')
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Start' })).toBeEnabled();
  });
});
