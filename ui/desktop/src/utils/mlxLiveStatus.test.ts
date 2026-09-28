import { describe, expect, it, vi } from 'vitest';
import { fetchMlxLiveStatus, mlxLiveStatusUrl } from './mlxLiveStatus';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('mlxLiveStatusUrl — loopback only', () => {
  it('derives /v1/status from the engine base URL; localhost fetches 127.0.0.1', () => {
    expect(mlxLiveStatusUrl('http://127.0.0.1:8090')).toBe('http://127.0.0.1:8090/v1/status');
    expect(mlxLiveStatusUrl('http://127.0.0.1:8090/')).toBe('http://127.0.0.1:8090/v1/status');
    expect(mlxLiveStatusUrl('http://localhost:9600')).toBe('http://127.0.0.1:9600/v1/status');
  });

  it("keeps the base's path: a remote single's engine is read through goosed's loopback relay", () => {
    expect(mlxLiveStatusUrl('http://127.0.0.1:61001/relay/cafe')).toBe(
      'http://127.0.0.1:61001/relay/cafe/v1/status'
    );
    expect(mlxLiveStatusUrl('http://127.0.0.1:61001/relay/cafe/')).toBe(
      'http://127.0.0.1:61001/relay/cafe/v1/status'
    );
  });

  it('refuses a non-loopback host, a non-http scheme and non-URLs', () => {
    expect(() => mlxLiveStatusUrl('http://192.168.8.220:8090')).toThrow(/loopback/);
    expect(() => mlxLiveStatusUrl('https://example.com')).toThrow(/not http/);
    expect(() => mlxLiveStatusUrl('')).toThrow(/not a URL/);
  });
});

describe('fetchMlxLiveStatus — every failure is named', () => {
  it('returns the raw body on 200', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse({ status: 'idle' }));
    const r = await fetchMlxLiveStatus('http://127.0.0.1:8090', fetchImpl);
    expect(r).toEqual({
      ok: true,
      url: 'http://127.0.0.1:8090/v1/status',
      body: { status: 'idle' },
    });
    expect(fetchImpl).toHaveBeenCalledWith(
      'http://127.0.0.1:8090/v1/status',
      expect.objectContaining({ method: 'GET' })
    );
  });

  it('a refused base URL fetches nothing', async () => {
    const fetchImpl = vi.fn();
    const r = await fetchMlxLiveStatus('http://10.0.0.5:8090', fetchImpl);
    expect(r.ok).toBe(false);
    expect(!r.ok && r.error).toBe('bad-base-url');
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it('http, bad-json, unreachable and timeout are distinct', async () => {
    const http = await fetchMlxLiveStatus('http://127.0.0.1:8090', async () =>
      jsonResponse({}, 503)
    );
    expect(!http.ok && [http.error, http.status]).toEqual(['http', 503]);

    const badJson = await fetchMlxLiveStatus(
      'http://127.0.0.1:8090',
      async () => new Response('<html>', { status: 200 })
    );
    expect(!badJson.ok && badJson.error).toBe('bad-json');

    const refused = await fetchMlxLiveStatus('http://127.0.0.1:8090', async () => {
      throw Object.assign(new TypeError('fetch failed'), {
        cause: new Error('connect ECONNREFUSED'),
      });
    });
    expect(!refused.ok && [refused.error, refused.detail]).toEqual([
      'unreachable',
      'connect ECONNREFUSED',
    ]);

    const slow = await fetchMlxLiveStatus(
      'http://127.0.0.1:8090',
      (_url, init) =>
        new Promise((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => {
            const e = new Error('aborted');
            e.name = 'AbortError';
            reject(e);
          });
        }),
      5
    );
    expect(!slow.ok && slow.error).toBe('timeout');
  });
});

describe('fetchMlxLiveStatus — Q-409: a relay base’s capability never leaves the read', () => {
  const CAPABILITY = '9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08';
  const RELAY = `http://127.0.0.1:61001/relay/${CAPABILITY}`;

  it('fetches through the real relay path, and says it back redacted', async () => {
    const fetchImpl = vi.fn(async () => jsonResponse({ status: 'idle' }));
    const r = await fetchMlxLiveStatus(RELAY, fetchImpl);
    expect(fetchImpl).toHaveBeenCalledWith(`${RELAY}/v1/status`, expect.anything());
    expect(r).toEqual({
      ok: true,
      url: 'http://127.0.0.1:61001/relay/…/v1/status',
      body: { status: 'idle' },
    });
  });

  it('a refused relay base is named without its capability', async () => {
    const r = await fetchMlxLiveStatus(`http://10.0.0.2:61001/relay/${CAPABILITY}`, vi.fn());
    expect(r).toEqual({
      ok: false,
      url: 'http://10.0.0.2:61001/relay/…',
      error: 'bad-base-url',
      detail: 'engine base URL is not a loopback host: http://10.0.0.2:61001/relay/…',
    });
  });

  it('a body the JSON parser refuses is quoted at the fault — the capability in it is not', async () => {
    const r = await fetchMlxLiveStatus(
      RELAY,
      async () =>
        new Response(`{"detail": "no such relay", "path": /relay/${CAPABILITY}/v1/status}`)
    );
    expect(!r.ok && r.error).toBe('bad-json');
    expect(!r.ok && r.detail).toContain('/relay/…');
    expect(JSON.stringify(r)).not.toContain(CAPABILITY.slice(0, 3));
  });
});
