import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import {
  chatCompletionsUrl,
  deviceFromModelId,
  fetchSwarmContextLimit,
  modelsUrl,
  useFleet,
} from './useFleet';
import type { FleetProbeResult } from '../../utils/fleetProbe';

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;
const LAN = 'http://192.168.8.220:1234';
const LOADED = [
  { id: 'workhorse-qwen3.6-27b', state: 'loaded', arch: 'qwen3', loaded_context_length: 32768 },
  { id: 'mihai-qwen3.6-27b', state: 'loaded', arch: 'qwen3', max_context_length: 65536 },
  { id: 'nomic-embed', state: 'loaded', type: 'embeddings', loaded_context_length: 512 },
  { id: 'gabee-qwen3.6-27b', state: 'not-loaded', arch: 'qwen3' },
];

/**
 * Gate 8 refutation of 949d3fa6e (2026-09-02): the probe runs in MAIN (`window.electron.fleetProbe`),
 * so a LAN `swarm.endpoint` is reachable and the renderer's CSP is not in the path. The hook's shapes
 * are unchanged; what the fixture pins is that the state is DRIVEN by main's typed answer.
 */
describe('useFleet — the state follows main\'s probe result', () => {
  let probe: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    probe = vi.fn();
    electron().fleetProbe = probe;
  });
  afterEach(() => {
    delete electron().fleetProbe;
  });

  it('a LAN endpoint that answers is ONLINE, lanes from the loaded non-embedding models, endpoint verbatim', async () => {
    probe.mockResolvedValue({ ok: true, url: `${LAN}/api/v0/models`, data: LOADED } as FleetProbeResult);
    const { result } = renderHook(() => useFleet(10_000_000, LAN));
    await waitFor(() => expect(result.current.online).toBe(true));
    expect(probe).toHaveBeenCalledWith(LAN);
    expect(result.current.models).toEqual(['workhorse-qwen3.6-27b', 'mihai-qwen3.6-27b']);
    expect(result.current.lanes.map((l) => l.device)).toEqual(['workhorse', 'mihai']);
    expect(result.current.endpoint).toBe(LAN);
    expect(result.current.loading).toBe(false);
  });

  it('a NAMED failure from main is the honest offline state, still naming the configured host', async () => {
    probe.mockResolvedValue({
      ok: false,
      url: `${LAN}/api/v0/models`,
      error: 'unreachable',
      detail: 'connect ECONNREFUSED',
    } as FleetProbeResult);
    const { result } = renderHook(() => useFleet(10_000_000, LAN));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.online).toBe(false);
    expect(result.current.lanes).toEqual([]);
    expect(result.current.endpoint).toBe(LAN);
  });

  it('with discovery disabled nothing is probed', async () => {
    const { result } = renderHook(() => useFleet(10_000_000, LAN, false));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(probe).not.toHaveBeenCalled();
    expect(result.current.online).toBe(false);
  });

  it('fetchSwarmContextLimit takes the MIN loaded context across non-embedding models, null when offline', async () => {
    probe.mockResolvedValue({ ok: true, url: `${LAN}/api/v0/models`, data: LOADED } as FleetProbeResult);
    expect(await fetchSwarmContextLimit(LAN)).toBe(32768);
    probe.mockResolvedValue({ ok: false, url: '', error: 'timeout', detail: '' } as FleetProbeResult);
    expect(await fetchSwarmContextLimit(LAN)).toBeNull();
  });
});

/** U-M3: every probe URL is derived from the configured host base — never a pinned 127.0.0.1 — and
 *  (gate 8 refutation of 949d3fa6e) a LOOPBACK base fetches 127.0.0.1, the one loopback origin the static
 *  meta CSP in index.html allows; the display text (`FleetState.endpoint`) stays the configured base. */
describe('probe URLs derive from the configured swarm endpoint (a host base)', () => {
  it('appends the LM Studio routes to the endpoint ORIGIN, whatever path or slash it carries', () => {
    expect(modelsUrl('http://192.168.8.220:1234/')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(modelsUrl('http://192.168.8.220:1234/v1')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(chatCompletionsUrl('http://192.168.8.220:1234')).toBe(
      'http://192.168.8.220:1234/v1/chat/completions'
    );
  });

  it('probes the live default `http://localhost:1234` at 127.0.0.1 — the URL the meta CSP allows', () => {
    expect(modelsUrl('http://localhost:1234')).toBe('http://127.0.0.1:1234/api/v0/models');
    expect(chatCompletionsUrl('http://localhost:1234')).toBe('http://127.0.0.1:1234/v1/chat/completions');
    expect(modelsUrl('http://127.0.0.1:1234')).toBe('http://127.0.0.1:1234/api/v0/models');
  });

  it('refuses to guess a host for an unparseable endpoint', () => {
    expect(() => modelsUrl('localhost:1234')).toThrow();
    expect(() => modelsUrl('')).toThrow();
  });
});

describe('deviceFromModelId', () => {
  it('derives the node name from an LM Link prefixed model id', () => {
    expect(deviceFromModelId('mihai-qwopus3.6-27b-coder-mlx')).toBe('mihai');
    expect(deviceFromModelId('workhorse-qwopus3.6-27b-coder-mlx')).toBe('workhorse');
    expect(deviceFromModelId('gabee-qwopus3.6-27b-coder-mlx')).toBe('gabee');
  });

  it('strips a publisher/ prefix before deriving the node', () => {
    expect(deviceFromModelId('qwen/qwen3.6-27b')).toBe('qwen3.6');
  });

  it('returns the id unchanged when there is no dash', () => {
    expect(deviceFromModelId('llama3')).toBe('llama3');
  });
});
