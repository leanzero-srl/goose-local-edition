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
import type { MlxDistributedDiscovery, MlxDistributedStatus } from '../../acp/mlx-distributed';
// What the backend's Detect returned for the real pair (exported by the goose crate's
// `export_discovery_ui_fixture` from the captured probe answers of both Macs).
import DISCOVERY from './mlxDistributedDiscovery.fixture.json';

const UNCHOSEN = DISCOVERY.unchosen as MlxDistributedDiscovery;
const CHOSEN_27B = DISCOVERY.chosen27b as MlxDistributedDiscovery;
const MODEL_27B = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';

async function openAdvanced() {
  await userEvent.click(screen.getByRole('button', { name: 'Advanced' }));
}

const mockPreflight = vi.fn();
const mockStart = vi.fn();
const mockStop = vi.fn();
const mockConfigUpdate = vi.fn();
const mockCandidates = vi.fn();
const mockDiscover = vi.fn();
const mockProvision = vi.fn();
vi.mock('../../acp/mlx-distributed', () => ({
  mlxDistributedPreflight: (...a: unknown[]) => mockPreflight(...a),
  mlxDistributedStart: (...a: unknown[]) => mockStart(...a),
  mlxDistributedStop: (...a: unknown[]) => mockStop(...a),
  mlxDistributedConfigUpdate: (...a: unknown[]) => mockConfigUpdate(...a),
  mlxDistributedPeerCandidates: (...a: unknown[]) => mockCandidates(...a),
  mlxDistributedDiscover: (...a: unknown[]) => mockDiscover(...a),
  mlxDistributedProvision: (...a: unknown[]) => mockProvision(...a),
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
  mockCandidates.mockReset().mockResolvedValue([
    { alias: 'workhorse', answered: true, detail: 'WorksMacStudio' },
    {
      alias: 'old-box',
      answered: false,
      detail: 'ssh: connect to host old-box: Operation timed out',
    },
  ]);
  mockDiscover.mockReset();
  mockProvision.mockReset();
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
    await openAdvanced();
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
    // The section names the engine it configures, even while the Mac belongs to the single one.
    expect(screen.getByTestId('mlx-dist-mode-text')).toHaveTextContent(
      'Distributed · 2 nodes · JACCL'
    );
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
    await openAdvanced();
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
    await openAdvanced();
    await userEvent.click(screen.getByRole('combobox', { name: 'Backend' }));
    await userEvent.click(screen.getByTestId('mlx-dist-backend-ring'));
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart.mock.calls[0][0]).toMatchObject({ backend: 'ring', modelId: FLASH_MODEL });
  });

  it('a node is edited in a dialog; an empty required field blocks Save and Start and is named', async () => {
    section({ status: STOPPED_WITH_CONFIG });
    await openAdvanced();
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

  it('a saved but stopped config: the headline is the configured engine, not the single one', () => {
    section({ status: STOPPED_WITH_CONFIG });
    expect(screen.getByTestId('mlx-dist-mode-text')).toHaveTextContent(
      'Distributed · 2 nodes · JACCL'
    );
    expect(within(screen.getByTestId('mlx-dist-mode')).getByText('Stopped')).toBeInTheDocument();
    expect(screen.queryByText('Single · this Mac')).toBeNull();
    expect(screen.getByTestId('mlx-dist-config-summary')).toHaveTextContent(
      'rank 1 · workhorse · workhorse · 192.168.0.2'
    );
  });
});

describe('DistributedEngineSection — Set up detects everything from one peer name', () => {
  const NONE: MlxDistributedStatus = { ...STOPPED_WITH_CONFIG, config: null };

  it('nothing configured: the headline says so, and Set up asks for ONE field', async () => {
    const { container } = section({ status: NONE });
    expect(screen.getByTestId('mlx-dist-mode-text')).toHaveTextContent('Not configured');
    expect(screen.queryByText('Single · this Mac')).toBeNull();
    expect(screen.queryByText('Stopped')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Set up' }));
    const setup = screen.getByTestId('mlx-dist-setup');
    expect(within(setup).getAllByRole('textbox')).toHaveLength(1);
    // Only the aliases that answered are offered.
    const offered = await within(setup).findAllByTestId('mlx-dist-setup-candidate');
    expect(offered.map((b) => b.textContent)).toEqual(['workhorse· WorksMacStudio']);
    expect(within(setup).getByRole('button', { name: 'Detect' })).toBeDisabled();
    await expectDesigned(container);
  });

  it('Detect fills every field with its evidence and names the choice it could not make', async () => {
    mockDiscover.mockResolvedValue(UNCHOSEN);
    section({ status: NONE });
    await userEvent.click(screen.getByRole('button', { name: 'Set up' }));
    await userEvent.click(await screen.findByTestId('mlx-dist-setup-candidate'));
    await userEvent.click(screen.getByRole('button', { name: 'Detect' }));
    expect(mockDiscover).toHaveBeenCalledWith(['workhorse'], null);
    const result = await screen.findByTestId('mlx-dist-setup-result');

    const field = (node: string, name: string) =>
      result.querySelector(`[data-field="${name}"][data-node="${node}"]`) as HTMLElement;
    expect(field('cluster', 'backend')).toHaveTextContent('JACCL');
    expect(field('cluster', 'backend')).toHaveTextContent(
      'every node has an active RDMA device on the shared Thunderbolt link'
    );
    expect(field('1', 'tbIp')).toHaveTextContent('192.168.0.2');
    expect(field('1', 'tbIp')).toHaveTextContent('ifconfig en3: inet 192.168.0.2/30');
    expect(field('1', 'tbService')).toHaveTextContent('EXO Thunderbolt 2');
    expect(field('1', 'rdmaDevice')).toHaveTextContent('GID[1] = ::ffff:192.168.0.2');
    expect(field('1', 'python')).toHaveTextContent(
      '/Users/workhorse/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12/bin/python'
    );
    expect(within(field('1', 'python')).getByText('built at Save')).toBeInTheDocument();
    expect(field('cluster', 'port')).toHaveTextContent('8091');
    expect(field('cluster', 'port')).toHaveTextContent("the single engine's 8090");
    // Two models are on both Macs: the choice is a named gap, and Save waits for it.
    expect(screen.getByTestId('mlx-dist-setup-gaps')).toHaveTextContent('pick one');
    expect(field('cluster', 'modelId')).toHaveAttribute('data-found', 'false');
    expect(screen.getByRole('button', { name: 'Save and provision' })).toBeDisabled();
    // The raw fields are there, collapsed.
    expect(screen.getByTestId('mlx-dist-setup-advanced')).toHaveAttribute('data-state', 'closed');
  });

  it('picking the 27B re-detects for it; Save persists the config and provisions every node', async () => {
    mockDiscover.mockResolvedValueOnce(UNCHOSEN).mockResolvedValueOnce(CHOSEN_27B);
    mockConfigUpdate.mockImplementation(async (c: unknown) => c);
    mockProvision.mockResolvedValue({ state: 'running', startedMs: 1, nodes: [] });
    section({ status: NONE });
    await userEvent.click(screen.getByRole('button', { name: 'Set up' }));
    await userEvent.type(screen.getByTestId('mlx-dist-setup-peer'), 'workhorse');
    await userEvent.click(screen.getByRole('button', { name: 'Detect' }));
    await screen.findByTestId('mlx-dist-setup-result');
    await userEvent.click(screen.getByRole('combobox', { name: 'Model' }));
    const option = screen.getByTestId(`mlx-dist-setup-model-${MODEL_27B}`);
    expect(option).toHaveTextContent('on every node');
    await userEvent.click(option);
    await waitFor(() => expect(mockDiscover).toHaveBeenLastCalledWith(['workhorse'], MODEL_27B));
    await waitFor(() =>
      expect(
        screen
          .getByTestId('mlx-dist-setup-result')
          .querySelector('[data-field="modelDir"][data-node="1"]')
      ).toHaveTextContent('/Users/workhorse/jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx')
    );
    expect(screen.queryByTestId('mlx-dist-setup-gaps')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Save and provision' }));
    await waitFor(() => expect(mockProvision).toHaveBeenCalledTimes(1));
    const saved = mockConfigUpdate.mock.calls[0][0];
    expect(saved).toMatchObject({ modelId: MODEL_27B, backend: 'jaccl', port: 8091 });
    expect(saved.nodes[0].ssh).toBeUndefined();
    expect(saved.nodes[1]).toMatchObject({
      ssh: 'workhorse',
      tbIp: '192.168.0.2',
      rdmaDevice: 'rdma_en3',
      modelDir: '/Users/workhorse/jaccl-smoke/models/Qwen3.8-27B-Atlassian-Q8-mlx',
    });
    expect(mockProvision.mock.calls[0][0]).toEqual(saved);
    expect(onRefresh).toHaveBeenCalled();
  });

  it('a peer that does not answer is a red gap, never a filled node', async () => {
    mockDiscover.mockResolvedValue({
      ...UNCHOSEN,
      config: { ...UNCHOSEN.config, backend: '', coordinatorPort: 0 },
      gaps: [{ node: 1, field: 'reachable', reason: 'nas: ssh failed: Connection refused' }],
      evidence: [],
      nodes: [UNCHOSEN.nodes[0], { rank: 1, name: 'nas', host: 'nas', reachable: false }],
      models: [],
    });
    section({ status: NONE });
    await userEvent.click(screen.getByRole('button', { name: 'Set up' }));
    await userEvent.type(screen.getByTestId('mlx-dist-setup-peer'), 'nas');
    await userEvent.click(screen.getByRole('button', { name: 'Detect' }));
    expect(await screen.findByTestId('mlx-dist-setup-gaps')).toHaveTextContent(
      'nas: ssh failed: Connection refused'
    );
    expect(screen.getByText('not reachable')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save and provision' })).toBeDisabled();
  });

  it('provisioning progress: per node, the step, the last line, a failure in red', () => {
    section({
      status: {
        ...STOPPED_WITH_CONFIG,
        provision: {
          state: 'failed',
          startedMs: 1000,
          finishedMs: 8100,
          nodes: [
            {
              rank: 0,
              name: 'MacBook Pro',
              python: '/Users/me/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12/bin/python',
              state: 'done',
              step: 'done',
              detail: 'already /Users/me/.goose/distributed/… 0.32.2 0.31.3',
              lines: [],
              startedMs: 1000,
              finishedMs: 2500,
            },
            {
              rank: 1,
              name: 'workhorse',
              host: 'workhorse',
              python: '/Users/workhorse/.goose/distributed/mlx0.32.2-mlxlm0.31.3-py3.12/bin/python',
              state: 'failed',
              step: 'fail',
              detail: 'uv not found on this node (looked: …)',
              lines: [],
              startedMs: 1000,
              finishedMs: 1300,
            },
          ],
        },
      },
    });
    const rows = screen.getAllByTestId('mlx-dist-provision-node');
    expect(rows.map((r) => r.getAttribute('data-state'))).toEqual(['done', 'failed']);
    expect(within(rows[0]).getByText('Ready')).toHaveAttribute('data-tone', 'ok');
    expect(within(rows[0]).getByText('1.5 s')).toBeInTheDocument();
    expect(within(rows[1]).getByText('Failed')).toHaveAttribute('data-tone', 'err');
    expect(within(rows[1]).getByText('uv not found on this node (looked: …)').className).toContain(
      'text-lz-err'
    );
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

// The real messages of the 2026-09-24 measurement: the app opened from Finder was refused
// 192.168.0.2 while the Studio (probed over ssh) reached 192.168.0.1.
const REFUSED_PING =
  'no ping answer over the TB link from 192.168.0.2 (ping: sendto: No route to host)';
const REFUSED_EVIDENCE =
  'macOS is blocking Goose Swarm from the local network — allow it in System Settings › Privacy & Security › Local Network (this Mac → 192.168.0.2: ping: sendto: No route to host, while workhorse answered over ssh and reached 192.168.0.1 from its side)';
const LOCAL_NETWORK_REFUSED = {
  ...FLASH_PREFLIGHT_OK,
  ok: false,
  nodes: [
    {
      ...FLASH_PREFLIGHT_OK.nodes[0],
      checks: [
        { id: 'reachable', verdict: 'pass' as const, message: 'this Mac' },
        { id: 'ping', verdict: 'fail' as const, message: REFUSED_PING },
        { id: 'localNetworkPermission', verdict: 'fail' as const, message: REFUSED_EVIDENCE },
      ],
    },
    FLASH_PREFLIGHT_OK.nodes[1],
  ],
};

describe('DistributedEngineSection — macOS local network privacy', () => {
  it('Preflight first brings up the system alert from main, then names the refusal with the one click that fixes it', async () => {
    const calls: string[] = [];
    vi.mocked(window.electron.touchLocalNetwork).mockImplementation(async () => {
      calls.push('touch');
      return [];
    });
    mockPreflight.mockImplementation(async () => {
      calls.push('preflight');
      return LOCAL_NETWORK_REFUSED;
    });
    const { container } = section({ status: STOPPED_WITH_CONFIG });
    await userEvent.click(screen.getByRole('button', { name: 'Preflight (dry run)' }));
    const notice = await screen.findByTestId('local-network-blocked');
    expect(calls).toEqual(['touch', 'preflight']);
    expect(notice).toHaveTextContent(
      'macOS is blocking Goose Swarm from the local network — allow it in System Settings › Privacy & Security › Local Network'
    );
    const rows = within(screen.getByTestId('mlx-dist-failing')).getAllByTestId('mlx-dist-check');
    expect(rows.map((r) => r.getAttribute('data-check'))).toEqual([
      'ping',
      'localNetworkPermission',
    ]);
    expect(rows[1]).toHaveTextContent('workhorse answered over ssh and reached 192.168.0.1');
    await userEvent.click(within(notice).getByRole('button', { name: 'Open Privacy & Security' }));
    expect(window.electron.openLocalNetworkSettings).toHaveBeenCalledTimes(1);
    await expectDesigned(container);
  });

  it('Start brings up the alert before it asks the backend', async () => {
    const calls: string[] = [];
    vi.mocked(window.electron.touchLocalNetwork).mockImplementation(async () => {
      calls.push('touch');
      return [];
    });
    mockStart.mockImplementation(async () => {
      calls.push('start');
      return {
        started: false,
        refusal: { code: 'preflightFailed', message: 'preflight refused the start' },
        preflight: LOCAL_NETWORK_REFUSED,
      };
    });
    section({ status: STOPPED_WITH_CONFIG });
    await userEvent.click(screen.getByRole('button', { name: 'Start' }));
    expect(await screen.findByTestId('local-network-blocked')).toBeInTheDocument();
    expect(calls).toEqual(['touch', 'start']);
  });

  it('a rank on this Mac that died of it is named in the events and offers the same click', () => {
    section({
      status: {
        ...STOPPED_WITH_CONFIG,
        state: 'failed',
        lastError: 'rank 0 exited during startup (exit status: 1).',
        events: [
          {
            atMs: 1_000,
            kind: 'localNetworkBlocked',
            node: 'macbook',
            message:
              'macOS is blocking Goose Swarm from the local network — allow it in System Settings › Privacy & Security › Local Network: rank 0 exited during startup (exit status: 1). Last output:\n[ring] Couldn’t connect (error: 65)',
          },
        ],
      },
    });
    expect(screen.getByTestId('local-network-blocked')).toBeInTheDocument();
    const event = screen.getByTestId('mlx-dist-event');
    expect(event).toHaveAttribute('data-kind', 'localNetworkBlocked');
    expect(within(event).getByText('Local network blocked')).toHaveAttribute('data-tone', 'err');
  });

  it('nothing is claimed when the preflight passes', async () => {
    mockPreflight.mockResolvedValue(FLASH_PREFLIGHT_OK);
    section({ status: STOPPED_WITH_CONFIG });
    await userEvent.click(screen.getByRole('button', { name: 'Preflight (dry run)' }));
    expect(await screen.findByTestId('mlx-dist-preflight')).toHaveAttribute('data-ok', 'true');
    expect(screen.queryByTestId('local-network-blocked')).toBeNull();
  });
});
