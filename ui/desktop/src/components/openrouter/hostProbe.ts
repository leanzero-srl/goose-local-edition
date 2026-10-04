import type { OpenRouterHost, OpenRouterPin, OpenRouterProbe } from '../../acp/openrouter';

/** One host's probe as the table shows it: still running, answered (possibly with the host's own
 *  error inside), or the request to the engine itself failed. */
export type ProbeState =
  | { kind: 'running' }
  | { kind: 'done'; result: OpenRouterProbe }
  | { kind: 'failed'; error: string };

export interface ProbeRow {
  host: OpenRouterHost;
  state: ProbeState;
}

export const ANY_HOST = '__any__';

/** "262K" / "1M" / "512" — the context as OpenRouter's own pages print it (decimal thousands). */
export function formatContext(tokens: number | null | undefined): string | null {
  if (tokens == null) return null;
  if (tokens >= 1_000_000) return `${Number((tokens / 1_000_000).toFixed(1))}M`;
  if (tokens >= 1000) return `${Math.round(tokens / 1000)}K`;
  return tokens.toLocaleString('en-US');
}

/** "Wafer · fp8 · 262K context · 99.2% uptime" — every fact the listing gave, nothing invented. */
export function hostLabel(host: OpenRouterHost): string {
  const context = formatContext(host.contextLength);
  return [
    host.providerName,
    host.quantization ?? null,
    context ? `${context} context` : null,
    host.uptimeLast30m != null ? `${host.uptimeLast30m.toFixed(1)}% uptime` : null,
  ]
    .filter((part): part is string => part != null && part !== '')
    .join(' · ');
}

/** The hosts the picker offers (the run needs tool calls) and the ones it leaves out. */
export function splitByTools(hosts: readonly OpenRouterHost[]): {
  tools: OpenRouterHost[];
  noTools: OpenRouterHost[];
} {
  return {
    tools: hosts.filter((host) => host.supportsTools),
    noTools: hosts.filter((host) => !host.supportsTools),
  };
}

/** An answered probe with no error. */
function answered(state: ProbeState): OpenRouterProbe | null {
  return state.kind === 'done' && !state.result.error ? state.result : null;
}

/** The sort bucket: tool call answered first, then answered without the tool, then still running,
 *  then failures. */
function bucket(state: ProbeState): number {
  const ok = answered(state);
  if (ok) return ok.toolCall ? 0 : 1;
  return state.kind === 'running' ? 2 : 3;
}

function speed(state: ProbeState): number {
  return answered(state)?.tokensPerSecond ?? -1;
}

/** Fastest-with-tool-call first; a host's place never depends on the order the answers arrived. */
export function rankProbes(rows: readonly ProbeRow[]): ProbeRow[] {
  return rows
    .map((row, index) => ({ row, index }))
    .sort(
      (a, b) =>
        bucket(a.row.state) - bucket(b.row.state) ||
        speed(b.row.state) - speed(a.row.state) ||
        a.index - b.index
    )
    .map(({ row }) => row);
}

/** The host to recommend: the fastest that answered with the tool call. None when no host did —
 *  a fast host that never calls the tool is not a recommendation for a tool-driven run. */
export function recommendedTag(rows: readonly ProbeRow[]): string | null {
  const best = rankProbes(rows)[0];
  if (!best) return null;
  const ok = answered(best.state);
  return ok?.toolCall ? best.host.tag : null;
}

/** What the pin means, in words, for the model on screen. */
export function pinSummary(
  pin: OpenRouterPin,
  hosts: readonly OpenRouterHost[] | null,
  model: string
): { tone: 'plain' | 'err'; text: string } {
  if (pin.raw == null) {
    return { tone: 'plain', text: 'OpenRouter picks the host for every request.' };
  }
  if (pin.tag == null) {
    return {
      tone: 'plain',
      text: `Custom routing is saved in OPENROUTER_PARAMETERS: ${pin.raw}. Choosing a host replaces it.`,
    };
  }
  const host = hosts?.find((h) => h.tag === pin.tag);
  if (hosts != null && !host) {
    return {
      tone: 'err',
      text: `The pin names ${pin.tag}, which does not serve ${model}: every request for this model will fail with "No endpoints found". Choose a host or Any host.`,
    };
  }
  return {
    tone: 'plain',
    text: `Every OpenRouter request runs on ${host?.providerName ?? pin.tag} (${pin.tag}), with no fallback to another host. This applies to every OpenRouter model, not only this one.`,
  };
}

/** The host's or OpenRouter's own words for a failed probe, verbatim, behind its HTTP status. */
export function probeError(state: ProbeState): string | null {
  if (state.kind === 'failed') return state.error;
  if (state.kind !== 'done' || !state.result.error) return null;
  return state.result.httpStatus != null
    ? `${state.result.httpStatus}: ${state.result.error}`
    : state.result.error;
}

/** The probe's figures as a person reads them. */
export function formatSpeed(probe: OpenRouterProbe): string | null {
  return probe.tokensPerSecond == null ? null : `${probe.tokensPerSecond.toFixed(1)}`;
}

export function formatSeconds(seconds: number): string {
  return `${seconds.toFixed(1)} s`;
}
