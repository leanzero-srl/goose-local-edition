import { describe, it, expect, vi } from 'vitest';
import {
  chatCompletionsUrl,
  modelsUrl,
  postFleetChat,
  probeFleetModels,
  type FetchLike,
} from '../fleetProbe';

const LAN = 'http://192.168.8.220:1234';
const LIVE = 'http://localhost:1234';

const jsonResponse = (body: unknown, status = 200): Response =>
  ({
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
  }) as unknown as Response;

const fetchReturning = (res: Response): FetchLike & { mock: ReturnType<typeof vi.fn>['mock'] } =>
  vi.fn(async () => res) as unknown as FetchLike & { mock: ReturnType<typeof vi.fn>['mock'] };

/**
 * Gate 8 refutation of 949d3fa6e (2026-09-02): the renderer's CSP is the intersection of index.html's
 * static meta and main's header, so a LAN swarm host could never be probed from the renderer. The probe
 * now runs in MAIN through these functions; every failure arm is named so "offline" is honest.
 */
describe('probeFleetModels — the models probe as main runs it', () => {
  it('GETs <origin>/api/v0/models on the LAN host verbatim and hands back the data array', async () => {
    const fetchImpl = fetchReturning(
      jsonResponse({ data: [{ id: 'workhorse-qwen3.6-27b', state: 'loaded', arch: 'qwen3' }] })
    );
    const r = await probeFleetModels(LAN, fetchImpl);
    expect(fetchImpl.mock.calls[0][0]).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(r).toEqual({
      ok: true,
      url: 'http://192.168.8.220:1234/api/v0/models',
      data: [{ id: 'workhorse-qwen3.6-27b', state: 'loaded', arch: 'qwen3' }],
    });
  });

  it('probes the live default http://localhost:1234 at 127.0.0.1 — one url function for both processes', async () => {
    const fetchImpl = fetchReturning(jsonResponse({ data: [] }));
    const r = await probeFleetModels(LIVE, fetchImpl);
    expect(fetchImpl.mock.calls[0][0]).toBe('http://127.0.0.1:1234/api/v0/models');
    expect(r).toEqual({ ok: true, url: 'http://127.0.0.1:1234/api/v0/models', data: [] });
  });

  it('names a refused connection `unreachable` with the socket error, never an empty fleet', async () => {
    const err = new TypeError('fetch failed');
    (err as { cause?: unknown }).cause = new Error('connect ECONNREFUSED 192.168.8.220:1234');
    const fetchImpl: FetchLike = vi.fn(async () => {
      throw err;
    });
    const r = await probeFleetModels(LAN, fetchImpl);
    expect(r).toEqual({
      ok: false,
      url: 'http://192.168.8.220:1234/api/v0/models',
      error: 'unreachable',
      detail: 'connect ECONNREFUSED 192.168.8.220:1234',
    });
  });

  it('names a probe that outlives its window `timeout` (the abort signal fires the fetch rejection)', async () => {
    const fetchImpl: FetchLike = vi.fn(
      (_url, init) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => {
            const e = new Error('aborted');
            e.name = 'AbortError';
            reject(e);
          });
        })
    );
    const r = await probeFleetModels(LAN, fetchImpl, 5);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toBe('timeout');
  });

  it('names a non-2xx answer `http` with the status, and a non-JSON 200 `bad-json`', async () => {
    const http = await probeFleetModels(LAN, fetchReturning(jsonResponse({}, 503)));
    expect(http).toMatchObject({ ok: false, error: 'http', status: 503 });
    const badJson = await probeFleetModels(
      LAN,
      fetchReturning({
        ok: true,
        status: 200,
        json: async () => {
          throw new SyntaxError('Unexpected token <');
        },
      } as unknown as Response)
    );
    expect(badJson).toMatchObject({ ok: false, error: 'bad-json', detail: 'Unexpected token <' });
  });

  it('refuses a value that is not an http(s) host base WITHOUT fetching anything', async () => {
    const fetchImpl = fetchReturning(jsonResponse({ data: [] }));
    for (const bad of ['localhost:1234', '', 'ftp://x:1']) {
      const r = await probeFleetModels(bad, fetchImpl);
      expect(r).toMatchObject({ ok: false, error: 'bad-endpoint', url: bad });
    }
    expect(fetchImpl.mock.calls).toHaveLength(0);
  });

  it('reads a 2xx body without a data array as an empty fleet, not a crash', async () => {
    const r = await probeFleetModels(LAN, fetchReturning(jsonResponse({ object: 'list' })));
    expect(r).toMatchObject({ ok: true, data: [] });
  });
});

describe('postFleetChat — the wizard chat POST as main runs it', () => {
  it('POSTs the body as JSON to <origin>/v1/chat/completions and returns the reply body', async () => {
    const fetchImpl = fetchReturning(
      jsonResponse({ choices: [{ message: { content: 'What does the recipe do?' } }] })
    );
    const body = { model: 'm', messages: [], stream: false };
    const r = await postFleetChat(LAN, body, fetchImpl);
    const [url, init] = fetchImpl.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('http://192.168.8.220:1234/v1/chat/completions');
    expect(init.method).toBe('POST');
    expect(init.body).toBe(JSON.stringify(body));
    expect(r).toEqual({
      ok: true,
      url: 'http://192.168.8.220:1234/v1/chat/completions',
      body: { choices: [{ message: { content: 'What does the recipe do?' } }] },
    });
  });

  it('carries the status of a non-2xx answer so the wizard says "fleet returned 500"', async () => {
    const r = await postFleetChat(LIVE, {}, fetchReturning(jsonResponse({}, 500)));
    expect(r).toMatchObject({ ok: false, error: 'http', status: 500, url: 'http://127.0.0.1:1234/v1/chat/completions' });
  });
});

describe('the url helpers are shared verbatim with the renderer', () => {
  it('derive from the origin and loopback-normalise', () => {
    expect(modelsUrl('http://192.168.8.220:1234/v1')).toBe('http://192.168.8.220:1234/api/v0/models');
    expect(chatCompletionsUrl(LIVE)).toBe('http://127.0.0.1:1234/v1/chat/completions');
  });
});
