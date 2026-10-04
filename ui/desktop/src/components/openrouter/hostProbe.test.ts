import { describe, expect, it } from 'vitest';
import type { OpenRouterHost, OpenRouterProbe } from '../../acp/openrouter';
import {
  formatContext,
  hostLabel,
  pinSummary,
  probeError,
  rankProbes,
  recommendedTag,
  splitByTools,
  type ProbeRow,
} from './hostProbe';

const host = (tag: string, providerName: string, extra: Partial<OpenRouterHost> = {}) =>
  ({ tag, providerName, supportsTools: true, ...extra }) as OpenRouterHost;

const done = (tag: string, extra: Partial<OpenRouterProbe>): ProbeRow => ({
  host: host(tag, tag),
  state: {
    kind: 'done',
    result: { tag, seconds: 10, toolCall: false, ...extra } as OpenRouterProbe,
  },
});

// The owner's check of qwen/qwen3.8-27b on 2026-10-04, as the probes answered it.
const tonight: ProbeRow[] = [
  done('phala', { tokensPerSecond: 20.0, toolCall: true, finishReason: 'tool_calls' }),
  done('parasail/fp8', { tokensPerSecond: 89.1, toolCall: false, finishReason: 'length' }),
  done('alibaba', { error: 'Provider returned error', httpStatus: 400 }),
  { host: host('chutes', 'Chutes'), state: { kind: 'failed', error: 'backend disconnected' } },
  done('novita', { tokensPerSecond: 56.7, toolCall: true, finishReason: 'tool_calls' }),
  done('wafer', { tokensPerSecond: 79.4, toolCall: true, finishReason: 'tool_calls' }),
];

describe('rankProbes', () => {
  it('puts the fastest host that called the tool first, a faster host without the tool after every tool host, failures last', () => {
    expect(rankProbes(tonight).map((row) => row.host.tag)).toEqual([
      'wafer',
      'novita',
      'phala',
      'parasail/fp8',
      'alibaba',
      'chutes',
    ]);
  });

  it('keeps a host still running between the answers and the failures, whatever order answers arrive in', () => {
    const rows: ProbeRow[] = [
      { host: host('slow', 'Slow'), state: { kind: 'running' } },
      done('broken', { error: '429 Rate limit exceeded', httpStatus: 429 }),
      done('fast', { tokensPerSecond: 70, toolCall: true }),
    ];
    expect(rankProbes(rows).map((row) => row.host.tag)).toEqual(['fast', 'slow', 'broken']);
    expect(rankProbes([...rows].reverse()).map((row) => row.host.tag)).toEqual([
      'fast',
      'slow',
      'broken',
    ]);
  });
});

describe('recommendedTag', () => {
  it('recommends the fastest host with a tool call, never the faster one that skipped the tool', () => {
    expect(recommendedTag(tonight)).toBe('wafer');
  });

  it('recommends nothing when no host called the tool', () => {
    expect(
      recommendedTag([
        done('parasail/fp8', { tokensPerSecond: 89.1, toolCall: false }),
        done('alibaba', { error: 'Provider returned error' }),
      ])
    ).toBeNull();
    expect(recommendedTag([])).toBeNull();
  });
});

describe('labels', () => {
  it('shows every fact the listing gave and nothing it did not', () => {
    expect(
      hostLabel(
        host('wafer', 'Wafer', { quantization: 'fp8', contextLength: 262144, uptimeLast30m: 99.24 })
      )
    ).toBe('Wafer · fp8 · 262K context · 99.2% uptime');
    expect(hostLabel(host('alibaba', 'Alibaba'))).toBe('Alibaba');
    expect(formatContext(1_000_000)).toBe('1M');
    expect(formatContext(1_048_576)).toBe('1M');
    expect(formatContext(32_768)).toBe('33K');
    expect(formatContext(512)).toBe('512');
    expect(formatContext(null)).toBeNull();
  });

  it('splits the hosts the picker offers from the ones without tool support', () => {
    const { tools, noTools } = splitByTools([
      host('wafer', 'Wafer'),
      host('deepinfra/bf16', 'DeepInfra', { supportsTools: false }),
    ]);
    expect(tools.map((h) => h.tag)).toEqual(['wafer']);
    expect(noTools.map((h) => h.tag)).toEqual(['deepinfra/bf16']);
  });
});

describe('probeError', () => {
  it("carries the host's words verbatim behind the HTTP status, and nothing for an answer", () => {
    const age = 'This model requires 18+ age confirmation.';
    expect(probeError(done('chutes', { error: age, httpStatus: 403 }).state)).toBe(`403: ${age}`);
    expect(probeError(done('x', { error: 'the request did not complete: refused' }).state)).toBe(
      'the request did not complete: refused'
    );
    expect(probeError({ kind: 'failed', error: 'backend disconnected' })).toBe(
      'backend disconnected'
    );
    expect(probeError(tonight[0].state)).toBeNull();
    expect(probeError({ kind: 'running' })).toBeNull();
  });
});

describe('pinSummary', () => {
  const hosts = [host('wafer', 'Wafer'), host('novita', 'Novita')];

  it('names OpenRouter routing when no pin is saved', () => {
    expect(pinSummary({}, hosts, 'qwen/qwen3.8-27b').text).toMatch(/OpenRouter picks the host/);
  });

  it('names the pinned host and that it applies to every OpenRouter model', () => {
    const summary = pinSummary(
      { tag: 'wafer', raw: '{"provider":{"order":["wafer"],"allow_fallbacks":false}}' },
      hosts,
      'qwen/qwen3.8-27b'
    );
    expect(summary.tone).toBe('plain');
    expect(summary.text).toMatch(/Wafer \(wafer\), with no fallback/);
    expect(summary.text).toMatch(/every OpenRouter model/);
  });

  it('warns loudly when the pinned host does not serve the model on screen', () => {
    const summary = pinSummary(
      { tag: 'xiaomi', raw: '{"provider":{"order":["xiaomi"],"allow_fallbacks":false}}' },
      hosts,
      'qwen/qwen3.8-27b'
    );
    expect(summary.tone).toBe('err');
    expect(summary.text).toMatch(/xiaomi, which does not serve qwen\/qwen3\.8-27b/);
  });

  it('shows custom routing verbatim instead of pretending it is a host', () => {
    const raw = '{"provider":{"ignore":["relace"],"order":["deepseek"]}}';
    expect(pinSummary({ raw }, hosts, 'm/x').text).toContain(raw);
  });
});
