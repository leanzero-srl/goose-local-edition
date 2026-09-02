import { afterEach, describe, expect, it, vi } from 'vitest';
import { fleetChatHandler, fleetProbeHandler } from '../fleetIpc';

const LIVE = 'http://127.0.0.1:1234';
const jsonResponse = (body: unknown, status = 200): Response =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
const fetchReturning = (res: Response) => vi.fn(async () => res);
const headersOf = (fetchImpl: ReturnType<typeof vi.fn>): Record<string, string> =>
  (fetchImpl.mock.calls[0] as unknown as [string, RequestInit])[1].headers as Record<
    string,
    string
  >;

/**
 * main's two fleet IPC handlers, run exactly as registered (`ipcMain.handle(name, handler)`), under a
 * fake fetch. LM Studio with "require API token" on (this Mac) answers 401 to a bare call; both handlers
 * carry LMSTUDIO_API_KEY from the same source, and a refusal is the typed `http` error naming the key.
 */
describe('fleet-probe — the models probe as main registers it', () => {
  afterEach(() => vi.unstubAllEnvs());

  it("reads LMSTUDIO_API_KEY from main's environment by default and sends it as a bearer", async () => {
    vi.stubEnv('LMSTUDIO_API_KEY', 'lm-token-1');
    const fetchImpl = fetchReturning(jsonResponse({ data: [{ id: 'workhorse-qwen3.8-27b' }] }));
    const r = await fleetProbeHandler(fetchImpl)({}, LIVE);
    expect(headersOf(fetchImpl)['Authorization']).toBe('Bearer lm-token-1');
    expect(r).toEqual({
      ok: true,
      url: `${LIVE}/api/v0/models`,
      data: [{ id: 'workhorse-qwen3.8-27b' }],
    });
  });

  it('sends NO Authorization header when the environment has no key', async () => {
    vi.stubEnv('LMSTUDIO_API_KEY', '');
    const fetchImpl = fetchReturning(jsonResponse({ data: [] }));
    await fleetProbeHandler(fetchImpl)({}, LIVE);
    expect(headersOf(fetchImpl)).toEqual({});
  });

  it('a 401 without a key is the typed `http` error naming the key — never unreachable', async () => {
    const r = await fleetProbeHandler(fetchReturning(jsonResponse({}, 401)), () => null)({}, LIVE);
    expect(r).toMatchObject({
      ok: false,
      error: 'http',
      status: 401,
      detail: 'fleet returned 401 — LM Studio wants an API token (set LMSTUDIO_API_KEY)',
    });
  });

  it('a non-string endpoint from the renderer is the typed bad-endpoint error', async () => {
    const r = await fleetProbeHandler(fetchReturning(jsonResponse({ data: [] })), () => null)(
      {},
      42
    );
    expect(r).toMatchObject({ ok: false, error: 'bad-endpoint' });
  });
});

describe('fleet-chat — the wizard POST as main registers it', () => {
  afterEach(() => vi.unstubAllEnvs());

  it('carries the same bearer as the probe beside its Content-Type and returns the reply body', async () => {
    vi.stubEnv('LMSTUDIO_API_KEY', 'lm-token-1');
    const body = { choices: [{ message: { content: 'hi' } }] };
    const fetchImpl = fetchReturning(jsonResponse(body));
    const r = await fleetChatHandler(fetchImpl)({}, LIVE, { model: 'm', messages: [] });
    expect(headersOf(fetchImpl)).toEqual({
      'Content-Type': 'application/json',
      Authorization: 'Bearer lm-token-1',
    });
    expect((fetchImpl.mock.calls[0] as unknown as [string, RequestInit])[0]).toBe(
      `${LIVE}/v1/chat/completions`
    );
    expect(r).toEqual({ ok: true, url: `${LIVE}/v1/chat/completions`, body });
  });

  it('sends NO Authorization header without a key — the old bare POST, unchanged', async () => {
    vi.stubEnv('LMSTUDIO_API_KEY', '');
    const fetchImpl = fetchReturning(jsonResponse({ choices: [] }));
    await fleetChatHandler(fetchImpl)({}, LIVE, {});
    expect(headersOf(fetchImpl)).toEqual({ 'Content-Type': 'application/json' });
  });

  it('a rejected key is the typed `http` 401 naming the key, with the value never echoed', async () => {
    const r = await fleetChatHandler(fetchReturning(jsonResponse({}, 401)), () => 'wrong-token')(
      {},
      LIVE,
      {}
    );
    expect(r).toMatchObject({
      ok: false,
      error: 'http',
      status: 401,
      detail: 'fleet returned 401 — the LMSTUDIO_API_KEY it carried was rejected',
    });
    expect(JSON.stringify(r)).not.toContain('wrong-token');
  });

  it('the token source is consulted per call, so a key that appears later is picked up without a restart', async () => {
    let current: string | null = null;
    const fetchImpl = vi.fn(async () => jsonResponse({ choices: [] }));
    const handler = fleetChatHandler(fetchImpl, () => current);
    await handler({}, LIVE, {});
    current = 'lm-token-2';
    await handler({}, LIVE, {});
    const second = (fetchImpl.mock.calls[1] as unknown as [string, RequestInit])[1]
      .headers as Record<string, string>;
    expect(headersOf(fetchImpl)['Authorization']).toBeUndefined();
    expect(second['Authorization']).toBe('Bearer lm-token-2');
  });
});
