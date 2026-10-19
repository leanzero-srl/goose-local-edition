import { describe, expect, it, vi } from 'vitest';
import fixture from './benchTokenUsage.fixture.json';
import { captureTokenUsage } from './benchTokenCapture';
import type { TokenUsage } from './benchTokenUsage';

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
    const saved: Record<string, TokenUsage> = {};
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
    expect(saved['c'.repeat(64)]).toEqual(result.usage);
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
