import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { cleanup, render as rtlRender, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { MlxDistributedNodePreflight } from '../../acp/mlx-distributed';

const mockApply = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineWiredLimitApply: (...a: unknown[]) => mockApply(...a),
}));

import { DistributedWiredLimits, wiredRoute } from './DistributedWiredLimits';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });
afterEach(cleanup);
beforeEach(() => {
  mockApply.mockReset().mockResolvedValue({ wiredLimitMb: 114688, command: 'x' });
});

const node = (over: Partial<MlxDistributedNodePreflight>): MlxDistributedNodePreflight => ({
  name: 'workhorse',
  rank: 0,
  checks: [],
  ceilingBytes: 115_448_725_504,
  wiredLimitMb: 0,
  ...over,
});

const mule = (host: string | undefined) =>
  node({
    name: 'workmule',
    rank: 1,
    host,
    wiredWhatIf: {
      mb: 114688,
      keepBytes: 16 * 1024 ** 3,
      beyondSuggestion: true,
      maxContextNow: 262_144,
      maxContextThen: 323_584,
      layersNow: 21,
      layersThen: 22,
      budgetNowBytes: 115_448_725_504,
      budgetThenBytes: 120_259_084_288,
      fallbackCommand: 'sudo /usr/sbin/sysctl iogpu.wired_limit_mb=114688',
    },
  });

describe('DistributedWiredLimits', () => {
  it('routes a node to this Mac, a Link peer or ssh', () => {
    expect(wiredRoute(undefined)).toEqual({ kind: 'self' });
    expect(wiredRoute('link:abc')).toEqual({ kind: 'link', nodeId: 'abc' });
    expect(wiredRoute('link:')).toEqual({ kind: 'ssh' });
    expect(wiredRoute('workmule.lan')).toEqual({ kind: 'ssh' });
  });

  it('shows each limit and what the planner says a raise buys, and asks the peer’s app', async () => {
    render(<DistributedWiredLimits nodes={[node({}), mule('link:mule-1')]} />);
    const rows = screen.getAllByTestId('mlx-dist-wired-node');
    expect(rows[0].textContent).toContain('wired limit 0 (macOS default)');
    expect(within(rows[0]).queryByTestId('mlx-dist-wired-raise')).toBeNull();
    const row = rows[1];
    expect(within(row).getByTestId('mlx-dist-wired-buys').textContent).toBe(
      'Raise to 114,688 MB on workmule: +1 layer moves to workmule → max context 262k → ~324k'
    );
    expect(row.textContent).toContain('keep 16 GB');
    expect(row.textContent).toContain('the most the reserve allows');
    await userEvent.click(within(row).getByRole('switch', { name: 'Keep after restart' }));
    await userEvent.click(within(row).getByTestId('mlx-dist-wired-raise'));
    expect(mockApply).toHaveBeenCalledWith(114688, true, 'mule-1');
    expect((await within(row).findByTestId('mlx-dist-wired-result')).textContent).toContain(
      'workmule reads 114,688 MB now'
    );
  });

  it('says plainly when nobody approved and shows the exact command', async () => {
    mockApply.mockRejectedValue(new Error('the administrator prompt was cancelled; nothing changed'));
    render(<DistributedWiredLimits nodes={[mule('link:mule-1')]} />);
    await userEvent.click(screen.getByTestId('mlx-dist-wired-raise'));
    expect((await screen.findByTestId('mlx-dist-wired-result')).textContent).toContain(
      'cancelled'
    );
    expect(screen.getByTestId('mlx-dist-wired-fallback').textContent).toContain(
      'sudo /usr/sbin/sysctl iogpu.wired_limit_mb=114688'
    );
  });

  it('an ssh-only node offers the command, never a prompt it cannot reach', () => {
    render(<DistributedWiredLimits nodes={[mule('workmule.lan')]} />);
    expect(screen.queryByTestId('mlx-dist-wired-raise')).toBeNull();
    expect(screen.getByTestId('mlx-dist-wired-fallback').textContent).toContain(
      'reached over ssh'
    );
    expect(mockApply).not.toHaveBeenCalled();
  });
});
