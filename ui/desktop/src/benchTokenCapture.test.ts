import { describe, expect, it, vi } from 'vitest';
import fixture from './benchTokenUsage.fixture.json';
import { captureTokenUsage, type CachedCapture } from './benchTokenCapture';

const sonnet = fixture.sonnet;
const RUN =
  '/Users/example/Library/Application Support/Goose/benchmark/runs/build/openrouter-cloud-1d88ea1b-07c6-4370-b1c5-1d1d6f3b9d7c-r0';
const KEY = 'test-key-not-a-secret';
const stored = { workdir: RUN, candidateWorkdir: RUN };
const verdict = { agent: sonnet.agent } as Record<string, unknown>;
const readFile = vi.fn(async (file: string) => {
  if (file === `${RUN}/.swarm/telemetry.jsonl`) return sonnet.telemetry;
  throw new Error(`ENOENT ${file}`);
});

/** OpenRouter's generation endpoint, answering from the run's real records. */
function openRouter(missing = new Set<string>()) {
  return vi.fn(async (url: string | URL, init?: RequestInit) => {
    const u = new URL(String(url));
    const id = u.searchParams.get('id')!;
    const record = (sonnet.records as Record<string, unknown>)[id];
    if (!record || missing.has(id)) return new Response('{}', { status: 404 });
    expect(u.origin + u.pathname).toBe('https://openrouter.ai/api/v1/generation');
    expect((init?.headers as Record<string, string>).Authorization).toBe(`Bearer ${KEY}`);
    return new Response(JSON.stringify({ data: record }), { status: 200 });
  });
}

describe('captureTokenUsage: the publish-time token record', () => {
  it('an OpenRouter run re-reads every generation record: exact, by model, cached for the next publish', async () => {
    const fetchImpl = openRouter();
    const saved: Record<string, CachedCapture> = {};
    const result = await captureTokenUsage(
      { stored, verdict, verdictSha256: 'c'.repeat(64), route: 'cloud' },
      {
        readFile,
        fetchImpl: fetchImpl as unknown as typeof fetch,
        openRouterCredentials: async () => ({ key: KEY, host: 'https://openrouter.ai' }),
        cache: {
          get: async (sha) => saved[sha] ?? null,
          set: async (sha, u) => void (saved[sha] = u),
        },
        sleep: async () => {},
      }
    );
    expect(result.problem).toBeNull();
    expect(result.usage).toMatchObject({
      promptTokens: 2_610_671,
      completionTokens: 90_481,
      cachedTokens: 2_509_317,
      reasoningTokens: 63_297,
      calls: 51,
      exact: true,
      source: 'provider-records',
    });
    expect(fetchImpl).toHaveBeenCalledTimes(51);
    // The exact read is also the complete bill the run-end read missed (43 + 8 records then): OpenRouter's own USD.
    expect(result.recordsBill).toEqual({
      billedUsd: 1.6556498,
      requestCount: 51,
      missingAtRunEnd: 8,
    });
    expect(saved['c'.repeat(64)]).toEqual({ usage: result.usage, recordsBill: result.recordsBill });
    // A republish reads the cache, not OpenRouter.
    const again = await captureTokenUsage(
      { stored, verdict, verdictSha256: 'c'.repeat(64), route: 'cloud' },
      {
        readFile,
        fetchImpl: fetchImpl as unknown as typeof fetch,
        openRouterCredentials: async () => null,
        cache: { get: async (sha) => saved[sha] ?? null, set: async () => {} },
      }
    );
    expect(again.usage).toEqual(result.usage);
    expect(fetchImpl).toHaveBeenCalledTimes(51);
  });

  it('the key never goes to a host other than the one that billed the run', async () => {
    const fetchImpl = vi.fn();
    const result = await captureTokenUsage(
      { stored, verdict, verdictSha256: null, route: 'cloud' },
      {
        readFile,
        fetchImpl: fetchImpl as unknown as typeof fetch,
        openRouterCredentials: async () => ({ key: KEY, host: 'https://proxy.example' }),
      }
    );
    expect(fetchImpl).not.toHaveBeenCalled();
    // No usable bill split (incomplete, two models): goose's per-call usage, labelled, never exact.
    expect(result.usage).toMatchObject({ source: 'session-counters', exact: false, calls: 51 });
    expect(result.usage!.note).toMatch(
      /OpenRouter's records could not be read \(goose's OpenRouter host is not the one that billed this run/
    );
  });

  it('records still missing at publish keep the call counted and the record a lower bound', async () => {
    const lost = new Set(
      (sonnet.agent.billed_cost.missing as Array<{ id: string }>).slice(0, 2).map((m) => m.id)
    );
    const result = await captureTokenUsage(
      { stored, verdict, verdictSha256: null, route: 'cloud' },
      {
        readFile,
        fetchImpl: openRouter(lost) as unknown as typeof fetch,
        openRouterCredentials: async () => ({ key: KEY, host: 'https://openrouter.ai' }),
        sleep: async () => {},
      }
    );
    expect(result.usage).toMatchObject({
      exact: false,
      missingCalls: 2,
      calls: 51,
      source: 'provider-records',
    });
  });

  it('a local engine run counts the engine usage per call', async () => {
    const lines = [
      '{"model":"local/qwen","prompt_tokens":10,"completion_tokens":2,"usage":true,"response_id":"chatcmpl-a","ended":"completed"}',
    ];
    const result = await captureTokenUsage(
      {
        stored,
        verdict: { agent: { billed_cost: { status: 'unavailable', provider: 'openai' } } },
        verdictSha256: null,
        route: 'local',
      },
      { readFile: async () => lines.join('\n'), openRouterCredentials: vi.fn() }
    );
    expect(result.usage).toMatchObject({
      source: 'engine-telemetry',
      promptTokens: 10,
      completionTokens: 2,
      calls: 1,
      exact: true,
    });
  });

  it('no per-call record at all: says why and sends nothing', async () => {
    const result = await captureTokenUsage(
      { stored, verdict: { agent: {} }, verdictSha256: null, route: 'cloud' },
      {
        readFile: async () => {
          throw new Error('ENOENT');
        },
        openRouterCredentials: vi.fn(),
      }
    );
    expect(result.usage).toBeNull();
    expect(result.problem).toMatch(/telemetry\.jsonl/);
  });
});

describe('review fix: a reused run folder never lends its telemetry to another run', () => {
  it('reads the run’s own (re-pointed) folder first and skips a slot that is not named after the run', async () => {
    const slot = '/Users/example/runs/build/swarm-3node-r0';
    const archived = '/Users/example/sessions/run-abc';
    const files: Record<string, string> = {
      [`${slot}/.swarm/telemetry.jsonl`]:
        '{"model":"next/run","prompt_tokens":999,"completion_tokens":9,"usage":true,"response_id":"chatcmpl-n","ended":"completed"}',
      [`${archived}/.swarm/telemetry.jsonl`]:
        '{"model":"this/run","prompt_tokens":5,"completion_tokens":1,"usage":true,"response_id":"chatcmpl-t","ended":"completed"}',
    };
    const read = async (f: string) => {
      if (f in files) return files[f];
      throw new Error('ENOENT');
    };
    const own = await captureTokenUsage(
      {
        stored: { runId: 'run-abc', workdir: archived, candidateWorkdir: slot },
        verdict: { agent: {} },
        verdictSha256: null,
        route: 'local',
      },
      { readFile: read, openRouterCredentials: vi.fn() }
    );
    expect(own.usage?.byModel.map((m) => m.model)).toEqual(['this/run']);
    delete files[`${archived}/.swarm/telemetry.jsonl`];
    const none = await captureTokenUsage(
      {
        stored: { runId: 'run-abc', workdir: archived, candidateWorkdir: slot },
        verdict: { agent: {} },
        verdictSha256: null,
        route: 'local',
      },
      { readFile: read, openRouterCredentials: vi.fn() }
    );
    expect(none.usage).toBeNull();
  });
});

describe('second review: a hung OpenRouter falls back to the harness bill within the budget', () => {
  it('Pareto (complete bill): the publish gets the bill’s split, labelled, after one budget', async () => {
    const pareto = fixture.pareto;
    const RUN2 =
      '/Users/example/Library/Application Support/Goose/benchmark/runs/build/openrouter-cloud-48107c98-71d8-4f7b-95de-6b5c0a50717a-r0';
    const hang = vi.fn(
      (_url: string | URL, init?: RequestInit) =>
        new Promise<Response>((_r, reject) =>
          init?.signal?.addEventListener('abort', () =>
            reject(new DOMException('aborted', 'AbortError'))
          )
        )
    );
    const result = await captureTokenUsage(
      {
        stored: { workdir: RUN2 },
        verdict: { agent: { billed_cost: pareto.modelCost } },
        verdictSha256: null,
        route: 'cloud',
      },
      {
        readFile: async (f) => {
          if (f === `${RUN2}/.swarm/telemetry.jsonl`) return pareto.telemetry;
          throw new Error('ENOENT');
        },
        fetchImpl: hang as unknown as typeof fetch,
        openRouterCredentials: async () => ({ key: KEY, host: 'https://openrouter.ai' }),
        readDeadlineMs: 150,
        sleep: async () => {},
      }
    );
    expect(result.usage).toMatchObject({
      source: 'provider-records',
      promptTokens: 10_932_017,
      calls: 140,
    });
    expect(result.usage!.note).toMatch(
      /could not be read again at publish \(OpenRouter did not answer within the read budget\)/
    );
  }, 5_000);
});

describe('third review: the bill is replaced only when the run-end bill missed records, nothing else', () => {
  it('DeepSeek 1017 build 2: records above goose’s counters are not exact and the incomplete bill stays as it was', async () => {
    const ds = fixture.deepseek;
    const RUN3 =
      '/Users/example/Library/Application Support/Goose/benchmark/runs/build/openrouter-cloud-3ca1d7f0-fa26-4c2d-a034-38f2d4b61aeb-r0';
    const served = vi.fn(async (url: string | URL) => {
      const id = new URL(String(url)).searchParams.get('id')!;
      const record = (ds.records as Record<string, unknown>)[id];
      return record
        ? new Response(JSON.stringify({ data: record }), { status: 200 })
        : new Response('{}', { status: 404 });
    });
    const result = await captureTokenUsage(
      {
        stored: { workdir: RUN3 },
        verdict: { agent: ds.agent },
        verdictSha256: null,
        route: 'cloud',
      },
      {
        readFile: async (f) => {
          if (f === `${RUN3}/.swarm/telemetry.jsonl`) return ds.telemetry;
          throw new Error('ENOENT');
        },
        fetchImpl: served as unknown as typeof fetch,
        openRouterCredentials: async () => ({ key: KEY, host: 'https://openrouter.ai' }),
        sleep: async () => {},
      }
    );
    expect(served).toHaveBeenCalledTimes(271);
    expect(result.usage).toMatchObject({
      promptTokens: 54_638_753,
      completionTokens: 223_659,
      calls: 271,
      missingCalls: 0,
      exact: false,
    });
    expect(result.usage!.note).toMatch(
      /differ from goose's session counters \(\+1,346 in, \+1,152 cached, \+7 out\)/
    );
    expect(result.recordsBill).toBeUndefined();
  });
});
