import { describe, expect, it, vi } from 'vitest';
import {
  attributeServing,
  fetchMlxServing,
  serveHttpBase,
  servingRowsForEngine,
  type MlxServingRow,
} from './mlxServing';

const MODEL = 'mihai-qwen3.8-27b-atlassian-q8-mlx';

function row(over: Partial<MlxServingRow> & Pick<MlxServingRow, 'id' | 'via'>): MlxServingRow {
  return {
    sessionId: null,
    provider: 'omlx',
    model: MODEL,
    nodeId: null,
    startedAt: '2026-09-23T20:00:00Z',
    sessionName: null,
    sessionType: null,
    sessionError: null,
    ...over,
  };
}

describe('attributeServing — who the engine serves, from goose’s own two doors', () => {
  it('a router lease on a user session is a chat in this app, named by the session', () => {
    const s = attributeServing(
      [
        row({
          id: 1,
          via: 'swarmRouter',
          sessionId: 's1',
          sessionName: 'Memory · verify',
          sessionType: 'user',
          nodeId: 'mihai-mlx',
        }),
      ],
      1,
      [],
      null
    );
    expect(s.clients).toEqual([
      { key: 'chat:s1', kind: 'chat', sessionId: 's1', sessionName: 'Memory · verify', count: 1 },
    ]);
    expect(s.unattributed).toBe(0);
  });

  it('an external /v1 request routed through `swarm` is the external client, not a chat', () => {
    const s = attributeServing(
      [
        row({ id: 1, via: 'openaiApi', sessionId: 'e1', provider: 'swarm', model: 'swarm' }),
        row({ id: 2, via: 'swarmRouter', sessionId: 'e1', sessionType: 'user' }),
      ],
      1,
      [],
      null
    );
    expect(s.clients).toEqual([
      { key: 'external:e1', kind: 'external', model: 'swarm/swarm', count: 1 },
    ]);
    expect(s.unattributed).toBe(0);
  });

  it('an external request straight to omlx is on the engine; one to a cloud provider is not', () => {
    const s = attributeServing(
      [
        row({ id: 1, via: 'openaiApi', sessionId: 'e1' }),
        row({ id: 2, via: 'openaiApi', sessionId: 'e2', provider: 'anthropic', model: 'claude' }),
      ],
      1,
      [],
      null
    );
    expect(s.clients.map((c) => c.key)).toEqual(['external:e1']);
  });

  it('a session title and its turn on one chat are one client counted twice', () => {
    const s = attributeServing(
      [
        row({ id: 1, via: 'swarmRouter', sessionId: 's1', sessionType: 'user' }),
        row({ id: 2, via: 'swarmRouter', sessionId: 's1', sessionType: 'user' }),
      ],
      2,
      [],
      null
    );
    expect(s.clients).toHaveLength(1);
    expect(s.clients[0].count).toBe(2);
  });

  it('other session types are named by their own type; a row with no session keeps its own key', () => {
    const s = attributeServing(
      [
        row({ id: 1, via: 'swarmRouter', sessionId: 'sub', sessionType: 'sub_agent' }),
        row({ id: 7, via: 'swarmRouter' }),
      ],
      2,
      [],
      null
    );
    expect(s.clients.map((c) => [c.kind, c.key])).toEqual([
      ['session', 'session:sub'],
      ['session', 'session:row-7'],
    ]);
  });

  it('what no door explains is COUNTED, with the live swarm runs stated beside it', () => {
    const s = attributeServing(
      [row({ id: 1, via: 'swarmRouter', sessionId: 's1', sessionType: 'user' })],
      3,
      ['bench-r9'],
      null
    );
    expect(s.unattributed).toBe(2);
    expect(s.swarmRuns).toEqual(['bench-r9']);
  });

  it('never a negative count when goose listed a turn the engine has already finished', () => {
    const s = attributeServing(
      [row({ id: 1, via: 'swarmRouter', sessionId: 's1', sessionType: 'user' })],
      0,
      [],
      null
    );
    expect(s.unattributed).toBe(0);
  });
});

describe('serveHttpBase', () => {
  it('turns the ACP websocket URL into the backend http origin, dropping the token', () => {
    expect(serveHttpBase('ws://127.0.0.1:53211/acp?token=secret')).toBe('http://127.0.0.1:53211');
    expect(serveHttpBase('wss://node.example:443/acp')).toBe('https://node.example');
    expect(serveHttpBase('not a url')).toBeNull();
    expect(serveHttpBase('file:///tmp/x')).toBeNull();
  });
});

describe('fetchMlxServing — one backend, its own secret, every failure named', () => {
  const ok = (body: unknown, status = 200) =>
    vi.fn(async () => new Response(JSON.stringify(body), { status }));

  it('sends the secret as X-Secret-Key and returns the rows', async () => {
    const fetchImpl = ok({ serving: [row({ id: 1, via: 'openaiApi', sessionId: 'e1' })] });
    const read = await fetchMlxServing('http://127.0.0.1:1', 'k', fetchImpl, 1000);
    expect(read.ok && read.rows.map((r) => r.id)).toEqual([1]);
    const [url, init] = fetchImpl.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('http://127.0.0.1:1/mlx-engine/serving');
    expect((init.headers as Record<string, string>)['X-Secret-Key']).toBe('k');
  });

  it('an older backend (404), a refusal and a malformed row are named, not empty lists', async () => {
    expect(await fetchMlxServing('http://h', 'k', ok({}, 404), 1000)).toEqual({
      ok: false,
      detail: 'this goose backend predates /mlx-engine/serving',
    });
    expect(await fetchMlxServing('http://h', 'k', ok({}, 401), 1000)).toEqual({
      ok: false,
      detail: 'goose backend returned 401',
    });
    expect(await fetchMlxServing('http://h', 'k', ok({ serving: [{ id: 'x' }] }), 1000)).toEqual({
      ok: false,
      detail: 'serving row 0 is not a serving row',
    });
  });
});

describe('servingRowsForEngine — a row counts against the engine that runs it', () => {
  const rows = [
    row({
      id: 1,
      via: 'swarmRouter',
      sessionId: 'local',
      sessionType: 'user',
      nodeId: 'mihai-mlx',
    }),
    row({
      id: 2,
      via: 'swarmRouter',
      sessionId: 'studio',
      sessionType: 'user',
      nodeId: 'remote-WorksMacStudio.lan',
      peer: "Work's Mac Studio",
    }),
    row({ id: 3, via: 'openaiApi', sessionId: 'ext-studio', provider: 'swarm', model: 'swarm' }),
    row({
      id: 4,
      via: 'swarmRouter',
      sessionId: 'ext-studio',
      sessionType: 'user',
      nodeId: 'remote-WorksMacStudio.lan',
      peer: "Work's Mac Studio",
    }),
    row({ id: 5, via: 'openaiApi', sessionId: 'ext-direct', provider: 'omlx' }),
  ];

  it("reading the linked Mac's engine keeps this app's leases there and their external rows", () => {
    expect(servingRowsForEngine(rows, true).map((r) => r.id)).toEqual([2, 3, 4]);
    const s = attributeServing(servingRowsForEngine(rows, true), 2, [], null);
    expect(s.clients.map((c) => c.key)).toEqual(['chat:studio', 'external:ext-studio']);
    expect(s.unattributed).toBe(0);
  });

  it("reading this Mac's engine drops the leases that went to the linked Mac", () => {
    expect(servingRowsForEngine(rows, false).map((r) => r.id)).toEqual([1, 5]);
  });

  it('a row from a backend that predates `peer` is this Mac’s', () => {
    const old = row({ id: 6, via: 'swarmRouter', sessionId: 'o', sessionType: 'user' });
    expect(servingRowsForEngine([old], false)).toEqual([old]);
    expect(servingRowsForEngine([old], true)).toEqual([]);
  });
});
