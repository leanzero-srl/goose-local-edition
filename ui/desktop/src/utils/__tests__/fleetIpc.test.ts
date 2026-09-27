import { afterEach, describe, expect, it, vi } from 'vitest';
import { fleetProbeHandler } from '../fleetIpc';

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
 * main's fleet-probe IPC handler, run exactly as registered (`ipcMain.handle(name, handler)`), under a
 * fake fetch. LM Studio with "require API token" on (this Mac) answers 401 to a bare call; the handler
 * carries LMSTUDIO_API_KEY, and a refusal is the typed `http` error naming the key.
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
