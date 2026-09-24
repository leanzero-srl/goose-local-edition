import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { PlacementBadge, PlacementCard } from './PlacementCard';
import { NODES, PLAN_27B, PLAN_FLASH } from './placement.fixtures';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { PlacementPlan } from '../../acp/mlx-placement';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';

const mockPlan = vi.fn();
const mockMeasure = vi.fn();
const mockRemoteStart = vi.fn();
const mockRemoteStatus = vi.fn();
const mockDistributedStart = vi.fn();
let remoteLatest: unknown = null;

vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: (...a: unknown[]) => mockPlan(...a),
  mlxMeasureSpeed: (...a: unknown[]) => mockMeasure(...a),
}));
vi.mock('../../acp/mlx-remote-single', () => ({
  mlxRemoteSingleStart: (...a: unknown[]) => mockRemoteStart(...a),
  mlxRemoteSingleStatus: (...a: unknown[]) => mockRemoteStatus(...a),
  latestMlxRemoteSingleStatus: () => remoteLatest,
  subscribeMlxRemoteSingleStatus: () => () => undefined,
}));
vi.mock('../../acp/mlx-distributed', () => ({
  mlxDistributedStart: (...a: unknown[]) => mockDistributedStart(...a),
}));

const MODEL = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';

function answer(plan: PlacementPlan) {
  return { plans: [plan], nodes: NODES, storeErrors: [], probeMs: 5404 };
}

function renderCard(single: MlxEngineStatus | null = null, onMountHere = vi.fn()) {
  render(
    <IntlTestWrapper>
      <PlacementCard
        modelId={MODEL}
        single={single}
        distributed={null}
        onMountHere={onMountHere}
        mountBusy={false}
      />
    </IntlTestWrapper>
  );
  return onMountHere;
}

beforeEach(() => {
  remoteLatest = null;
  mockRemoteStatus.mockResolvedValue({ state: 'off' });
  mockPlan.mockResolvedValue(answer(PLAN_27B));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('PlacementCard on the real 27B plan', () => {
  it('recommends the M3 Ultra alone with its estimate, range and context, and names the rest', async () => {
    renderCard();
    const best = await screen.findByTestId('placement-best');
    expect(within(best).getByText('Work’s Mac Studio alone')).toBeInTheDocument();
    expect(within(best).getByText('~21.9 tok/s writing')).toBeInTheDocument();
    expect(within(best).getByText('20.8–23.0')).toBeInTheDocument();
    expect(within(best).getByText('estimated')).toBeInTheDocument();
    expect(within(best).getByText('262,144 context')).toBeInTheDocument();
    expect(mockPlan).toHaveBeenCalledWith('chat', MODEL);

    // The fastest one needs a piece that this capture's goose had not landed; the best a person
    // could start then is named apart, with its own button.
    const now = screen.getByTestId('placement-best-now');
    expect(within(now).getByText('2 Macs · tensor split · JACCL')).toBeInTheDocument();
    expect(within(now).getByText('fits only at 72,704 context')).toBeInTheDocument();

    await userEvent.click(screen.getByText('2 other ways'));
    const local = screen.getByTestId('placement-other-single:local');
    expect(
      within(local).getByText(/Does not fit: short 10\.8 GB on Mihai Macbook/)
    ).toBeInTheDocument();
    const pipeline = screen.getByTestId('placement-other-pipeline:jaccl:local+workhorse');
    expect(
      within(pipeline).getByText(/^not supported yet: goose splits qwen3_5 tensor-parallel only$/)
    ).toBeInTheDocument();
    expect(screen.getByTestId('placement-nodes').textContent).toContain(
      'Work’s Mac Studio: Apple M3 Ultra · 60-core GPU · 819 GB/s'
    );
  });

  it('switches the goal and plans again for it', async () => {
    renderCard();
    await screen.findByTestId('placement-best');
    await userEvent.click(screen.getByRole('radio', { name: 'Long documents' }));
    await waitFor(() => expect(mockPlan).toHaveBeenLastCalledWith('longDocuments', MODEL));
  });

  it('offers Measure speed on the placement that is running and records it', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      best: 'single:local',
      bestAvailable: 'single:local',
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.id === 'single:local'
          ? { ...c, outcome: { code: 'best' }, fit: { ...c.fit, status: 'fits', context: 262144 } }
          : c
      ),
    };
    mockPlan.mockResolvedValue(answer(plan));
    mockMeasure.mockResolvedValue({
      records: [{ workload: 'chat', decodeTps: 21.5, prefillTps: 238 }],
    });
    renderCard({ state: 'running', modelId: MODEL } as MlxEngineStatus);
    const measure = await screen.findByTestId('placement-measure-single:local');
    expect(screen.getByText(/Running and not measured yet/)).toBeInTheDocument();
    await userEvent.click(measure);
    await waitFor(() => expect(mockMeasure).toHaveBeenCalledWith(MODEL, 'single:local', false));
    expect(
      await screen.findByText('Measured: 21.5 tok/s writing, 238 tok/s reading')
    ).toBeInTheDocument();
  });

  it('starts a Link peer’s engine for [Use this] and shows a refusal verbatim', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.id === 'single:workhorse'
          ? {
              ...c,
              id: 'single:link:wh',
              key: { ...c.key, nodes: ['link:wh'] },
              action: { kind: 'remoteSingle' },
            }
          : c
      ),
      best: 'single:link:wh',
      bestAvailable: 'single:link:wh',
    };
    mockPlan.mockResolvedValue(answer(plan));
    mockRemoteStart.mockResolvedValue({
      started: false,
      refusal: { code: 'chatServingDisabled', message: 'the peer does not serve chat' },
      status: { state: 'off' },
    });
    renderCard();
    await userEvent.click(await screen.findByTestId('placement-use-single:link:wh'));
    expect(mockRemoteStart).toHaveBeenCalledWith('wh', MODEL);
    expect(await screen.findByText('the peer does not serve chat')).toBeInTheDocument();
  });

  it('mounts here through the view for a local [Use this]', async () => {
    const plan: PlacementPlan = {
      ...PLAN_27B,
      best: 'single:local',
      bestAvailable: 'single:local',
      candidates: (PLAN_27B.candidates ?? []).map((c) =>
        c.id === 'single:local'
          ? { ...c, outcome: { code: 'best' }, fit: { ...c.fit, status: 'fits' } }
          : c
      ),
    };
    mockPlan.mockResolvedValue(answer(plan));
    const onMount = renderCard();
    await userEvent.click(await screen.findByTestId('placement-use-single:local'));
    expect(onMount).toHaveBeenCalledTimes(1);
  });

  it('says nothing fits when goose found no placement, and a failed plan is named', async () => {
    mockPlan.mockResolvedValueOnce(answer(PLAN_FLASH));
    renderCard();
    expect(await screen.findByText('Nothing fits right now')).toBeInTheDocument();
    cleanup();
    mockPlan.mockRejectedValueOnce({ data: 'mlx_engine config is unreadable' });
    renderCard();
    expect(await screen.findByText('mlx_engine config is unreadable')).toBeInTheDocument();
  });

  it('renders on Studio tokens only', async () => {
    renderCard();
    const card = await screen.findByTestId('placement-card');
    await userEvent.click(screen.getByText('2 other ways'));
    assertStudioClean(card);
    expect(allClasses(card).filter((c) => c === 'border-l' || /^border-l-\d/.test(c))).toEqual([]);
  });
});

describe('PlacementBadge', () => {
  it('says where a model fits, in solid tones', () => {
    render(
      <IntlTestWrapper>
        <PlacementBadge badge={{ kind: 'fitsPeer', name: 'Work’s Mac Studio' }} />
        <PlacementBadge badge={{ kind: 'tooBig', shortBytes: 14715588048 }} />
        <PlacementBadge badge={{ kind: 'fitsThisMac' }} />
        <PlacementBadge badge={{ kind: 'needsBothMacs' }} />
      </IntlTestWrapper>
    );
    expect(screen.getByText('Fits Work’s Mac Studio').closest('[data-tone]')).toHaveAttribute(
      'data-tone',
      'accent'
    );
    expect(screen.getByText('Too big, short 13.7 GB').closest('[data-tone]')).toHaveAttribute(
      'data-tone',
      'err'
    );
    expect(screen.getByText('Fits this Mac')).toBeInTheDocument();
    expect(screen.getByText('Needs both Macs')).toBeInTheDocument();
  });
});

describe('PlacementCard follows the engine it recommends, in the engine-phase palette', () => {
  it('the split while it starts is amber, then green once it serves; the single mount amber while mounting', async () => {
    const starting = {
      mode: 'distributed',
      state: 'starting',
      modelId: MODEL,
      admissionOpen: true,
      nodes: [],
      events: [],
      restarts: 0,
    } as MlxDistributedStatus;
    const { rerender } = render(
      <IntlTestWrapper>
        <PlacementCard
          modelId={MODEL}
          single={null}
          distributed={starting}
          onMountHere={vi.fn()}
          mountBusy={false}
        />
      </IntlTestWrapper>
    );
    const now = await screen.findByTestId('placement-best-now');
    const live = within(now).getByTestId('placement-live');
    expect(live).toHaveAttribute('data-phase', 'loading');
    expect(live).toHaveTextContent('Starting');
    rerender(
      <IntlTestWrapper>
        <PlacementCard
          modelId={MODEL}
          single={null}
          distributed={{ ...starting, state: 'serving', inflight: 1 }}
          onMountHere={vi.fn()}
          mountBusy={false}
        />
      </IntlTestWrapper>
    );
    expect(
      within(screen.getByTestId('placement-best-now')).getByTestId('placement-live')
    ).toHaveAttribute('data-phase', 'writing');
    // The best (Work’s Mac Studio alone) is not the engine running: no live chip on it.
    expect(within(screen.getByTestId('placement-best')).queryByTestId('placement-live')).toBeNull();
  });
});
