import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { GABEE, gb } from './memfree.fixtures';
import { DEFAULT_NEVER_TOUCH, type MemfreeGroup, type MemoryHolders } from './memfree';

const NT = [...DEFAULT_NEVER_TOUCH];

const mockHolders = vi.fn();
const mockFree = vi.fn();
const mockNeverTouch = vi.fn();
vi.mock('../../acp/memfree', () => ({
  memoryHolders: (...a: unknown[]) => mockHolders(...a),
  memoryFree: (...a: unknown[]) => mockFree(...a),
  readNeverTouchExtras: (...a: unknown[]) => mockNeverTouch(...a),
}));

import { FreeMemoryDialog } from './FreeMemoryDialog';

const show = (props: Partial<Parameters<typeof FreeMemoryDialog>[0]> = {}) =>
  render(
    <IntlTestWrapper>
      <FreeMemoryDialog macName="Gabee" fitModelId="qwen-30b" onClose={() => {}} {...props} />
    </IntlTestWrapper>
  );

const WITH_OWN: MemoryHolders = {
  ...GABEE,
  gooseOwnedBytes: gb(12),
  shortBytes: gb(10),
  verdict: { kind: 'fitsByGooseOwned', remainingBytes: 0, appsToQuit: [] },
  groups: [
    ...GABEE.groups,
    {
      id: 'engine-8080',
      label: 'goose MLX engine (idle)',
      category: 'gooseEngine',
      bytes: gb(12),
      pids: [{ pid: 300, startedAt: 2 }],
      quittable: false,
      reclaimableByGoose: true,
    },
  ],
};

beforeEach(() => {
  mockHolders.mockReset();
  mockFree.mockReset();
  mockNeverTouch.mockReset().mockResolvedValue([]);
});
afterEach(cleanup);

describe('FreeMemoryDialog', () => {
  it('tells Gabee the honest cannotFit verdict, with both alternatives, in bold GB', async () => {
    mockHolders.mockResolvedValue(GABEE);
    show();
    const verdict = await screen.findByTestId('free-memory-verdict');
    expect(verdict.dataset.verdict).toBe('cannotFit');
    expect(verdict.textContent).toContain('15.5 GB');
    expect(verdict.textContent).toContain('5.7 GB short');
    expect(within(verdict).getAllByText(/GB$/, { selector: 'strong' }).length).toBeGreaterThan(0);
    const alts = screen.getByTestId('free-memory-alternatives');
    expect(alts.textContent).toContain('mlx-community/Qwen3.8-27B-4bit');
    expect(alts.textContent).toContain('Mac Studio');
    expect(alts.textContent).toContain('98.3 GB');
    expect(mockHolders).toHaveBeenCalledWith({
      nodeId: undefined,
      fitModelId: 'qwen-30b',
      neverTouch: NT,
    });
    // goose holds nothing here: the one-click button says so and is disabled.
    expect(screen.getByTestId('free-memory-own')).toBeDisabled();
    // Categories: apps and system, each with a solid chip and its total.
    expect(screen.getByTestId('free-memory-cat-userApp').textContent).toContain('15.5 GB');
    expect(screen.getByTestId('free-memory-cat-system')).toBeInTheDocument();
    // system holders never get a Quit.
    expect(screen.queryByTestId('free-memory-quit-WindowServer')).toBeNull();
  });

  it('names only the apps when goose holds nothing to free', async () => {
    mockHolders.mockResolvedValue({
      ...GABEE,
      gooseOwnedBytes: 0,
      shortBytes: gb(4.3),
      verdict: { kind: 'fitsWithApps', remainingBytes: 0, appsToQuit: ['com.google.Chrome'] },
    });
    show();
    const verdict = await screen.findByTestId('free-memory-verdict');
    expect(verdict.dataset.verdict).toBe('fitsWithApps');
    expect(verdict.textContent).toContain('Google Chrome');
    expect(verdict.textContent).toContain('7.2 GB');
    expect(verdict.textContent).not.toMatch(/goose/i);
  });

  it('names the other Mac without a GB figure when it sent no Link load reading', async () => {
    mockHolders.mockResolvedValue({
      ...GABEE,
      alternatives: [{ kind: 'peerMac', nodeId: 'studio', label: 'Mac Studio' }],
    });
    show();
    const alts = await screen.findByTestId('free-memory-alternatives');
    expect(alts.textContent).toBe('Run it on Mac Studio instead');
    expect(alts.textContent).not.toContain('GB');
  });

  it('frees goose’s own memory in one click and shows before → after and the new verdict', async () => {
    mockHolders.mockResolvedValueOnce(WITH_OWN).mockResolvedValueOnce({
      ...WITH_OWN,
      gooseOwnedBytes: 0,
      shortBytes: 0,
      budgetBytes: gb(21.4),
      groups: GABEE.groups,
    });
    mockFree.mockResolvedValue({
      items: [
        {
          id: 'engine-8080',
          label: 'goose MLX engine',
          ok: true,
          freedBytes: gb(12),
          message: 'stopped',
        },
      ],
      before: { budgetBytes: gb(9.4) },
      after: { budgetBytes: gb(21.4) },
      fit: { verdict: 'allow', shortBytes: null },
    });
    const user = userEvent.setup();
    show({ target: 'n-gabee' });
    const own = await screen.findByTestId('free-memory-own');
    expect(own.textContent).toContain('12.0 GB');
    await user.click(own);
    await waitFor(() =>
      expect(mockFree).toHaveBeenCalledWith({
        nodeId: 'n-gabee',
        gooseOwned: true,
        quitApps: [],
        unloadServers: [],
        seenUsedBy: {},
        forceStopApps: [],
        nuclear: false,
        neverTouch: NT,
        fitModelId: 'qwen-30b',
      })
    );
    const after = await screen.findByTestId('free-memory-after');
    expect(after.textContent).toContain('9.4 GB');
    expect(after.textContent).toContain('21.4 GB');
    expect(after.textContent).toContain('The model fits now.');
    expect(mockHolders).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(screen.queryByTestId('free-memory-verdict')).toBeNull());
  });

  it('quits an app only after the house confirm dialog', async () => {
    mockHolders.mockResolvedValue(GABEE);
    mockFree.mockResolvedValue({
      items: [
        {
          id: 'com.google.Chrome',
          label: 'Google Chrome',
          ok: true,
          freedBytes: gb(7.2),
          message: 'quit',
        },
      ],
      before: { budgetBytes: gb(9.4) },
      after: { budgetBytes: gb(16.6) },
      fit: { verdict: 'block', shortBytes: gb(14) },
    });
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByTestId('free-memory-quit-com.google.Chrome'));
    await user.click(await screen.findByRole('button', { name: 'Keep it open' }));
    expect(mockFree).not.toHaveBeenCalled();
    await user.click(screen.getByTestId('free-memory-quit-com.google.Chrome'));
    await user.click(await screen.findByRole('button', { name: 'Quit Google Chrome' }));
    await waitFor(() =>
      expect(mockFree).toHaveBeenCalledWith({
        nodeId: undefined,
        gooseOwned: false,
        quitApps: ['com.google.Chrome'],
        unloadServers: [],
        seenUsedBy: {},
        forceStopApps: [],
        nuclear: false,
        neverTouch: NT,
        fitModelId: 'qwen-30b',
      })
    );
    expect((await screen.findByTestId('free-memory-after')).textContent).toContain('14.0 GB');
  });

  it('disables a peer’s Quit with the reason when remote app quit is off', async () => {
    mockHolders.mockResolvedValue({ ...GABEE, quitAppsAllowed: false });
    show({ target: 'n-gabee' });
    expect(await screen.findByTestId('free-memory-quit-com.google.Chrome')).toBeDisabled();
    expect(screen.getByTestId('free-memory-remote-off').textContent).toContain('Gabee');
  });

  it('shows the measurement failure in words', async () => {
    mockHolders.mockRejectedValue(new Error('engine gone'));
    show();
    expect((await screen.findByTestId('free-memory-error')).textContent).toContain('engine gone');
  });

  // Workhorse 2026-10-25 shape: rapid-mlx with an ssh port-forward on it (offered, NOT ticked),
  // an idle LM Studio (ticked), a launchd MCP server (protected), Teams (never-touch), TextEdit.
  const server = (
    id: string,
    label: string,
    sizeGb: number,
    extra: Partial<MemfreeGroup> = {}
  ): MemfreeGroup => ({
    id,
    label,
    category: id.startsWith('mlx:') ? 'gooseEngine' : 'aiServer',
    bytes: gb(sizeGb),
    pids: [{ pid: 97100, startedAt: 1 }],
    quittable: false,
    reclaimableByGoose: true,
    ...extra,
  });
  const WORKHORSE: MemoryHolders = {
    ...GABEE,
    gooseOwnedBytes: gb(1),
    shortBytes: 0,
    verdict: null,
    groups: [
      server('mlx:97100', 'rapid-mlx (port 8093)', 27.4, {
        usedBy: ['sshd-session (pid 97112) is connected to it'],
        nuclearDefault: false,
      }),
      // LM Link peers reach LM Studio through lmlink-connector; an older engine may still say
      // nuclearDefault: the desktop never ticks a server goose did not start.
      server('ai:/Applications/LM Studio.app', 'LM Studio', 0.6, {
        usedBy: ['LM Link: lmlink-connector (pid 14330) listens on 34267'],
        nuclearDefault: true,
      }),
      {
        id: 'job:mcp-web-search-upd',
        label: 'mcp-web-search-upd',
        category: 'job',
        bytes: gb(0.3),
        pids: [{ pid: 14200, startedAt: 1 }],
        quittable: false,
        reclaimableByGoose: false,
        protectedWhy: 'launchd job net.leanzero.mcp-web-search-upd',
        launchdLabel: 'net.leanzero.mcp-web-search-upd',
      },
      {
        id: 'app:com.microsoft.teams2',
        label: 'Microsoft Teams',
        category: 'userApp',
        bytes: gb(2),
        pids: [{ pid: 500, startedAt: 1 }],
        bundleId: 'com.microsoft.teams2',
        quittable: true,
        reclaimableByGoose: false,
        // An older engine that did not apply never-touch: the desktop still refuses to offer it.
        nuclearDefault: true,
      },
      {
        id: 'app:com.apple.TextEdit',
        label: 'TextEdit',
        category: 'userApp',
        bytes: gb(1),
        pids: [{ pid: 600, startedAt: 1 }],
        bundleId: 'com.apple.TextEdit',
        quittable: true,
        reclaimableByGoose: false,
        nuclearDefault: true,
      },
      {
        id: 'goosed:4242',
        label: 'goose serve (left over)',
        category: 'gooseOrphan',
        bytes: gb(1),
        pids: [{ pid: 4242, startedAt: 1 }],
        quittable: false,
        reclaimableByGoose: true,
      },
    ],
  };
  const OK_RESULT = {
    items: [],
    before: { budgetBytes: gb(9) },
    after: { budgetBytes: gb(11) },
    fit: { verdict: 'allow', shortBytes: null },
  };

  it('a never-touch or protected app gets no Quit and no Force stop, and the row says why', async () => {
    mockHolders.mockResolvedValue({
      ...WORKHORSE,
      groups: [
        ...WORKHORSE.groups,
        {
          id: 'app:com.microsoft.Outlook',
          label: 'Microsoft Outlook',
          category: 'userApp',
          bytes: gb(1.5),
          pids: [{ pid: 700, startedAt: 1 }],
          bundleId: 'com.microsoft.Outlook',
          // An engine before the fold: quittable, no protectedWhy. The desktop still refuses.
          quittable: true,
          reclaimableByGoose: false,
        },
      ],
    });
    mockFree.mockResolvedValue({
      ...OK_RESULT,
      items: [
        {
          id: 'app:com.microsoft.Outlook',
          label: 'Microsoft Outlook',
          ok: false,
          message: 'Microsoft Outlook is on the never-touch list (com.microsoft.Outlook)',
          forceStoppable: true,
          refusalCode: 'neverTouch',
        },
        {
          id: 'app:com.apple.TextEdit',
          label: 'TextEdit',
          ok: false,
          message: "TextEdit didn't quit",
          forceStoppable: true,
        },
      ],
    });
    const user = userEvent.setup();
    show();
    const row = await screen.findByTestId('free-memory-holder-app:com.microsoft.Outlook');
    expect(row.textContent).toContain('never-touch list (com.microsoft.Outlook)');
    expect(screen.queryByTestId('free-memory-quit-app:com.microsoft.Outlook')).toBeNull();
    expect(screen.queryByTestId('free-memory-quit-app:com.microsoft.teams2')).toBeNull();
    expect(screen.queryByTestId('free-memory-quit-job:mcp-web-search-upd')).toBeNull();
    await user.click(screen.getByTestId('free-memory-quit-app:com.apple.TextEdit'));
    await user.click(await screen.findByRole('button', { name: 'Quit TextEdit' }));
    expect(await screen.findByTestId('free-memory-force-com.apple.TextEdit')).toBeInTheDocument();
    expect(screen.queryByTestId('free-memory-force-com.microsoft.Outlook')).toBeNull();
  });

  it('the one-click button frees goose’s own only: no servers, no apps, not nuclear', async () => {
    mockHolders.mockResolvedValue(WORKHORSE);
    mockFree.mockResolvedValue(OK_RESULT);
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByTestId('free-memory-own'));
    await waitFor(() => expect(mockFree).toHaveBeenCalledTimes(1));
    const call = mockFree.mock.calls[0][0];
    expect(call.gooseOwned).toBe(true);
    expect(call.quitApps).toEqual([]);
    expect(call.unloadServers).toEqual([]);
    expect(call.nuclear).toBe(false);
  });

  it('unloads one server only after the confirm, which names who uses it', async () => {
    mockHolders.mockResolvedValue(WORKHORSE);
    mockFree.mockResolvedValue(OK_RESULT);
    const user = userEvent.setup();
    show();
    // Protected and never-touch holders never get an Unload or Quit.
    await screen.findByTestId('free-memory-unload-mlx:97100');
    expect(screen.queryByTestId('free-memory-unload-job:mcp-web-search-upd')).toBeNull();
    await user.click(screen.getByTestId('free-memory-unload-mlx:97100'));
    expect((await screen.findByTestId('free-memory-unload-used-by')).textContent).toContain(
      'sshd-session (pid 97112)'
    );
    await user.click(screen.getByRole('button', { name: 'Keep it running' }));
    expect(mockFree).not.toHaveBeenCalled();
    await user.click(screen.getByTestId('free-memory-unload-mlx:97100'));
    await user.click(await screen.findByRole('button', { name: 'Unload rapid-mlx (port 8093)' }));
    await waitFor(() =>
      expect(mockFree).toHaveBeenCalledWith(
        expect.objectContaining({
          gooseOwned: false,
          quitApps: [],
          unloadServers: ['mlx:97100'],
          seenUsedBy: { 'mlx:97100': ['sshd-session (pid 97112) is connected to it'] },
          nuclear: false,
        })
      )
    );
  });

  it('nuclear: ticks only nuclearDefault, never a server, protected is never selectable, sends only ticked + neverTouch', async () => {
    mockNeverTouch.mockResolvedValue(['TextEditHelperX']);
    mockHolders.mockResolvedValue(WORKHORSE);
    mockFree.mockResolvedValue(OK_RESULT);
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByTestId('free-memory-nuclear-open'));
    const view = screen.getByTestId('free-memory-nuclear');
    const tick = (id: string) => within(view).queryByTestId(`free-memory-tick-${id}`);
    // A server goose did not start is offered, never pre-ticked (review C1/C2).
    expect(tick('ai:/Applications/LM Studio.app')?.getAttribute('aria-checked')).toBe('false');
    expect(tick('app:com.apple.TextEdit')?.getAttribute('aria-checked')).toBe('true');
    expect(tick('gooseOwned')?.getAttribute('aria-checked')).toBe('true');
    // In use: offered but not ticked.
    expect(tick('mlx:97100')?.getAttribute('aria-checked')).toBe('false');
    // Protected (launchd) and never-touch (Teams): a chip with the reason, no checkbox at all.
    expect(tick('job:mcp-web-search-upd')).toBeNull();
    expect(tick('app:com.microsoft.teams2')).toBeNull();
    const mcp = within(view).getByTestId('free-memory-kept-job:mcp-web-search-upd');
    expect(mcp.textContent).toContain('Protected');
    expect(mcp.textContent).toContain('net.leanzero.mcp-web-search-upd');
    expect(
      within(view).getByTestId('free-memory-kept-app:com.microsoft.teams2').textContent
    ).toContain('never-touch list (com.microsoft.teams2)');
    // 1 (TextEdit) + 1 (goose's own) = 2.0 GB ticked; tick LM Studio -> 2.6; untick TextEdit -> 1.6.
    expect(screen.getByTestId('free-memory-nuclear-total').textContent).toContain('2.0 GB');
    await user.click(tick('ai:/Applications/LM Studio.app')!);
    expect(screen.getByTestId('free-memory-nuclear-total').textContent).toContain('2.6 GB');
    await user.click(tick('app:com.apple.TextEdit')!);
    expect(screen.getByTestId('free-memory-nuclear-total').textContent).toContain('1.6 GB');
    await user.click(screen.getByTestId('free-memory-nuclear-free'));
    // ONE confirm: apps are asked to quit; one that does not is left running and reported.
    expect(
      await screen.findByText(/does not quit is left running and reported/)
    ).toBeInTheDocument();
    expect(screen.queryByText(/SIGTERM/)).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Free 1.6 GB' }));
    await waitFor(() => expect(mockFree).toHaveBeenCalledTimes(1));
    expect(mockFree.mock.calls[0][0]).toEqual({
      nodeId: undefined,
      gooseOwned: true,
      quitApps: [],
      unloadServers: ['ai:/Applications/LM Studio.app'],
      seenUsedBy: {
        'ai:/Applications/LM Studio.app': [
          'LM Link: lmlink-connector (pid 14330) listens on 34267',
        ],
      },
      forceStopApps: [],
      nuclear: true,
      neverTouch: [...NT, 'TextEditHelperX'],
      fitModelId: 'qwen-30b',
    });
    // Back on the list with before -> after.
    expect((await screen.findByTestId('free-memory-after')).textContent).toContain('11.0 GB');
  });

  it('nuclear: cancelling the confirm frees nothing', async () => {
    mockHolders.mockResolvedValue(WORKHORSE);
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByTestId('free-memory-nuclear-open'));
    await user.click(screen.getByTestId('free-memory-nuclear-free'));
    await user.click(await screen.findByRole('button', { name: 'Keep everything' }));
    expect(mockFree).not.toHaveBeenCalled();
  });

  it('C4: an app that did not quit gets a Force stop, only after a confirm that says unsaved work is lost', async () => {
    mockHolders.mockResolvedValue(WORKHORSE);
    mockFree.mockResolvedValue({
      ...OK_RESULT,
      items: [
        {
          id: 'app:com.apple.TextEdit',
          label: 'TextEdit',
          ok: false,
          message: "TextEdit didn't quit (maybe unsaved work); it was left running",
          forceStoppable: true,
        },
        { id: 'app:x.other', label: 'Other', ok: false, message: 'refused' },
      ],
    });
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByTestId('free-memory-quit-app:com.apple.TextEdit'));
    await user.click(await screen.findByRole('button', { name: 'Quit TextEdit' }));
    const force = await screen.findByTestId('free-memory-force-com.apple.TextEdit');
    expect(screen.queryByTestId('free-memory-force-x.other')).toBeNull();
    mockFree.mockClear();
    await user.click(force);
    expect(await screen.findByText(/Unsaved work in TextEdit is lost/)).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Keep it open' }));
    expect(mockFree).not.toHaveBeenCalled();
    await user.click(screen.getByTestId('free-memory-force-com.apple.TextEdit'));
    await user.click(await screen.findByRole('button', { name: 'Force stop TextEdit' }));
    await waitFor(() =>
      expect(mockFree).toHaveBeenCalledWith({
        nodeId: undefined,
        gooseOwned: false,
        quitApps: [],
        unloadServers: [],
        seenUsedBy: {},
        forceStopApps: ['com.apple.TextEdit'],
        nuclear: true,
        neverTouch: NT,
        fitModelId: 'qwen-30b',
      })
    );
  });
});
