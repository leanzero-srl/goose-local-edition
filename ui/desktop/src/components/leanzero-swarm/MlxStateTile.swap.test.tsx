import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { MlxStateTile, type MlxStateTileProps } from './MlxStateTile';
import { EngineGlanceCard } from '../engineGlance/EngineGlanceCard';
import { glancePush } from '../../utils/engineGlance.fixtures';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';
import { nodeSwapOf } from '../../utils/nodeSwap';
import {
  J3_EXIT_143,
  J3_MODEL,
  J3_OWN_LOAD,
  J3_READ,
  J3_SWAP_TO_SPLIT,
  REAL_FAILURE,
} from '../../utils/nodeSwap.fixtures';
import { assertStudioClean } from '../lz/assertStudioClean';

/**
 * Q-254 on the Engine tile and the glance card: live J3 on 3.0.65 (19-j3-delegate-swap.png) showed
 * "Failed · Single · this Mac · the engine process (pid 1709) exited: exit status: 143" while the
 * loader swapped the 27B single out for the split. The negative controls are real failures.
 */

const toSplit = nodeSwapOf(J3_READ, J3_SWAP_TO_SPLIT);
const ownLoad = nodeSwapOf(J3_READ, J3_OWN_LOAD);

function tile(overrides: Partial<MlxStateTileProps>) {
  const props: MlxStateTileProps = {
    state: 'failed',
    unreachable: false,
    live: null,
    history: [],
    serving: null,
    mount: null,
    cost: null,
    failedError: J3_EXIT_143,
    action: null,
    modeLabel: 'Single · this Mac',
    distributed: null,
    modelId: J3_MODEL,
    ...overrides,
  };
  return render(
    <IntlTestWrapper>
      <MlxStateTile {...props} />
    </IntlTestWrapper>
  );
}

describe('the Engine tile during a swap (Q-254)', () => {
  it('J3: the single the swap stopped reads "Swapping to <node>", amber, no exit-143 excerpt', () => {
    const { container } = tile({ swap: toSplit });
    const badge = screen.getByTestId('mlx-state-badge');
    expect(badge).toHaveAttribute('data-state', 'swapping');
    expect(badge).toHaveAttribute('data-phase', 'loading');
    expect(screen.getByRole('status').textContent).toBe(
      'Swapping to Qwen3.8-27B-Atlassian-Q8-mlx · both Macs'
    );
    expect(screen.queryByTestId('mlx-failed-excerpt')).toBeNull();
    expect(container.textContent).not.toContain('143');
    assertStudioClean(container);
  });

  it('negative control: no swap — Failed, with the engine’s own words', () => {
    tile({ failedError: REAL_FAILURE, swap: null });
    expect(screen.getByTestId('mlx-state-badge')).toHaveAttribute('data-state', 'failed');
    expect(screen.getByRole('status').textContent).toBe('Failed');
    expect(screen.getByTestId('mlx-failed-excerpt').textContent).toBe(REAL_FAILURE);
  });

  it('negative control: the node being loaded failing on its own way stays Failed', () => {
    tile({ failedError: REAL_FAILURE, swap: ownLoad });
    expect(screen.getByTestId('mlx-state-badge')).toHaveAttribute('data-state', 'failed');
    expect(screen.getByTestId('mlx-failed-excerpt').textContent).toBe(REAL_FAILURE);
  });
});

describe('the glance card during a swap (Q-254)', () => {
  const card = (push: ReturnType<typeof glancePush>) =>
    render(
      <IntlTestWrapper>
        <EngineGlanceCard
          push={push}
          variant="dock"
          collapsed={false}
          expanded={false}
          onOpenEngine={vi.fn()}
          onOpenSession={vi.fn()}
          onToggleExpanded={vi.fn()}
          onCollapsedChange={vi.fn()}
        />
      </IntlTestWrapper>
    );
  const stoppedBySwap = { ...INITIAL_SNAPSHOT, mode: 'failed' as const, modelId: J3_MODEL };

  it('J3: "Swapping to <node>" headline, the stopped engine’s mode line and words gone', () => {
    const { container } = card(
      glancePush({ ...stoppedBySwap, failedError: J3_EXIT_143 }, { swap: toSplit })
    );
    expect(screen.getByTestId('engine-glance-stage').textContent).toBe(
      'Swapping to Qwen3.8-27B-Atlassian-Q8-mlx · both Macs'
    );
    expect(screen.getByTestId('engine-glance')).toHaveAttribute('data-phase', 'loading');
    expect(screen.queryByTestId('engine-glance-mode')).toBeNull();
    expect(container.textContent).not.toContain('143');
    expect(container.textContent).not.toContain('Failed');
    assertStudioClean(container);
  });

  it('negative control: a real failure reads Failed with its words', () => {
    const { container } = card(
      glancePush({ ...stoppedBySwap, failedError: REAL_FAILURE }, { swap: ownLoad })
    );
    expect(screen.getByTestId('engine-glance-stage').textContent).toBe('Failed');
    expect(container.textContent).toContain('exit status: 1');
  });
});
