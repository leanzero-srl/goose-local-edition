import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';

const run = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock('../components/leanzero-swarm/mlxRestore', () => ({ runRestore: run }));
const features = vi.hoisted(() => ({
  mlxEngine: true,
  mlxDistributed: true,
  leanzeroLink: true,
  isLoading: false,
}));
vi.mock('../contexts/FeaturesContext', () => ({ useFeatures: () => features }));

import { useMlxRestore } from './useMlxRestore';

const bridge = window.electron as unknown as { mlxRestoreClaim?: () => Promise<boolean> };

describe('useMlxRestore — one restore per app launch', () => {
  beforeEach(() => {
    run.mockClear();
    features.isLoading = false;
    features.mlxEngine = true;
  });

  it('the window main hands the claim to restores', async () => {
    bridge.mlxRestoreClaim = vi.fn(async () => true);
    renderHook(() => useMlxRestore());
    await waitFor(() => expect(run).toHaveBeenCalledTimes(1));
  });

  it('every other window (or a reload) is told no and restores nothing', async () => {
    const claim = vi.fn(async () => false);
    bridge.mlxRestoreClaim = claim;
    renderHook(() => useMlxRestore());
    await waitFor(() => expect(claim).toHaveBeenCalled());
    await Promise.resolve();
    expect(run).not.toHaveBeenCalled();
  });

  it('a goose without the MLX engine, or capabilities not read yet, asks for nothing', async () => {
    const claim = vi.fn(async () => true);
    bridge.mlxRestoreClaim = claim;
    features.isLoading = true;
    const { rerender } = renderHook(() => useMlxRestore());
    features.isLoading = false;
    features.mlxEngine = false;
    rerender();
    expect(claim).not.toHaveBeenCalled();
  });
});
