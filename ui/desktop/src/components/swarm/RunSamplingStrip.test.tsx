import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import RunSamplingStrip from './RunSamplingStrip';

vi.mock('./useSamplingDefaults', () => ({ useSaveSamplingDefaults: () => vi.fn() }));

/**
 * The strip writes <workingDir>/.swarm/run-sampling.json, which ONLY `goose swarm run` (the `swarm-build`
 * model) reads at spawn (run_sampling_env). A routed chat turn (the `swarm` model) never reads it, so
 * "the next run uses these values" was a false claim on every chat session.
 */
describe('RunSamplingStrip renders only for a swarm-build session', () => {
  beforeEach(() => {
    localStorage.clear();
    (window as unknown as { electron: Record<string, unknown> }).electron = {
      swarmGetSampling: vi.fn(async () => ({})),
      swarmSetSampling: vi.fn(async () => true),
    };
  });
  afterEach(() => cleanup());

  it('a swarm-build session shows the strip', async () => {
    render(<RunSamplingStrip workingDir="/tmp/build" active={false} sessionModel="swarm-build" />);
    await waitFor(() => expect(screen.getByTestId('sampling-knobs')).toBeTruthy());
    expect(screen.getByText('the next run uses these values')).toBeTruthy();
  });

  it('a chat session (model `swarm`) renders nothing and never touches the sampling file', async () => {
    const { container } = render(<RunSamplingStrip workingDir="/tmp/build" active={false} sessionModel="swarm" />);
    await new Promise((r) => setTimeout(r, 20));
    expect(container.innerHTML).toBe('');
    const e = (window as unknown as { electron: { swarmGetSampling: ReturnType<typeof vi.fn> } }).electron;
    expect(e.swarmGetSampling).not.toHaveBeenCalled();
  });

  it('an unknown model (null session) renders nothing', () => {
    const { container } = render(<RunSamplingStrip workingDir="/tmp/build" active={false} sessionModel={null} />);
    expect(container.innerHTML).toBe('');
  });
});
