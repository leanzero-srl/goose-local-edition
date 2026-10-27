/**
 * A rank of ANOTHER Mac's distributed engine running on this Mac, read from the process list — the
 * one way this Mac's goose can learn of a rank the requester launched over ssh (the requester's
 * supervisor spawns it through `ssh -tt`, so this Mac's goosed never hears of it; a Link-hosted rank
 * is also this Mac's goosed's own `hosting`). Owner, 2026-10-27: "both machines should show the
 * floating window", with the part this Mac plays ("serving layers 17-47 for workhorse").
 *
 * The rank's command line is goose's own (crates/goose-sidecar/src/distributed/launch.rs
 * `python_args`): `<python> -c <BOOT> <base64 program> <base64 spec JSON> goose-distributed-rank
 * [goose-distributed-owner=<token>]`. The spec carries rank, size, backend, the served model and,
 * for the layer split, the fork's own `--split` (ranks 1..N-1's first layers, plan.rs `split_arg`).
 */

export const RANK_MARKER = 'goose-distributed-rank';

export interface PeerRankProcess {
  pid: number;
  ppid: number;
  rank: number;
  size: number;
  backend: string | null;
  servedId: string;
  modelDir: string;
  /** The layer split's `--split` starts (ranks 1..N-1); null = a tensor split. */
  splitStarts: number[] | null;
}

export interface PeerRankLayers {
  first: number;
  last: number;
}

/** One rank as main shows it; `requester` null = no ssh session (this Mac's goosed hosts it over Link). */
export interface PeerRankSeen {
  pid: number;
  rank: number;
  size: number;
  modelId: string;
  backend: string | null;
  requester: string | null;
  layers: PeerRankLayers | null;
}

function decodeSpec(b64: string): Record<string, unknown> | null {
  try {
    const json = Buffer.from(b64, 'base64').toString('utf8');
    const value: unknown = JSON.parse(json);
    return value != null && typeof value === 'object' ? (value as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

function splitStartsOf(spec: Record<string, unknown>): number[] | null {
  const args = spec.serve_args;
  if (!Array.isArray(args)) return null;
  const at = args.indexOf('--split');
  const value = at >= 0 ? args[at + 1] : undefined;
  if (typeof value !== 'string' || value.trim() === '') return [];
  const starts = value.split(',').map((v) => Number(v.trim()));
  return starts.every((n) => Number.isInteger(n) && n >= 0) ? starts : null;
}

/**
 * Every goose rank on a `ps -axww -o pid=,ppid=,command=` listing, its spec decoded. Only a python
 * interpreter carrying the marker as an argument counts — the ssh carrier on the requester, a shell
 * or a grep that merely names it are not ranks. `own` pids are left out.
 */
export function parseGooseRanks(listing: string, own: readonly number[] = []): PeerRankProcess[] {
  const out: PeerRankProcess[] = [];
  for (const line of listing.split('\n')) {
    const fields = line.trim().split(/\s+/);
    if (fields.length < 4) continue;
    const pid = Number(fields[0]);
    const ppid = Number(fields[1]);
    if (!Number.isInteger(pid) || !Number.isInteger(ppid) || own.includes(pid)) continue;
    const argv = fields.slice(2);
    const program = argv[0].split('/').pop() ?? '';
    if (!program.startsWith('python')) continue;
    const at = argv.indexOf(RANK_MARKER);
    if (at < 2) continue;
    const spec = decodeSpec(argv[at - 1]);
    if (!spec) continue;
    const { rank, size } = spec;
    if (typeof rank !== 'number' || typeof size !== 'number') continue;
    out.push({
      pid,
      ppid,
      rank,
      size,
      backend: typeof spec.backend === 'string' ? spec.backend : null,
      servedId: typeof spec.served_id === 'string' ? spec.served_id : '',
      modelDir: typeof spec.model_dir === 'string' ? spec.model_dir : '',
      splitStarts: splitStartsOf(spec),
    });
  }
  return out;
}

/**
 * The layers a rank of a layer split runs: rank 0 from layer 0, rank r from the r-th start, each to
 * the layer before the next rank's start, the last to the model's last layer — in the fork's own
 * 0-based numbering, so it reads the same as the split's plan.
 */
export function pipelineLayers(
  rank: number,
  starts: readonly number[],
  numLayers: number | null
): PeerRankLayers | null {
  if (rank < 0 || rank > starts.length) return null;
  const first = rank === 0 ? 0 : starts[rank - 1];
  const next = rank < starts.length ? starts[rank] : numLayers;
  if (next == null || next <= first) return null;
  return { first, last: next - 1 };
}

/** `num_hidden_layers` of a model's config.json (top level, else its text_config). */
export function numLayersOf(configJson: string): number | null {
  try {
    const config = JSON.parse(configJson) as Record<string, unknown>;
    const n =
      config.num_hidden_layers ??
      (config.text_config as Record<string, unknown> | undefined)?.num_hidden_layers;
    return typeof n === 'number' && Number.isInteger(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

/** The ssh client address of a process's environment (`ps -E`): `SSH_CONNECTION=<ip> <port> …`. */
export function sshClientOf(envListing: string): string | null {
  const match = /(?:^|\s)SSH_CONNECTION=(\S+)\s/.exec(envListing);
  return match ? match[1] : null;
}

/**
 * The name a reverse lookup gives an address, as people call the Mac: MagicDNS's
 * `works-mac-studio.tail1234.ts.net` and the router's `WorksMacStudio.lan` both read as their first
 * label. null = nothing worth showing (the bare address is shown instead).
 */
export function macNameOfHost(hostname: string | null | undefined): string | null {
  const label = hostname?.trim().replace(/\.$/, '').split('.')[0];
  return label ? label : null;
}

/** A rank 0 is the requester's own: only a worker rank on this Mac serves ANOTHER Mac. */
export function servesAnotherMac(rank: PeerRankProcess): boolean {
  return rank.rank > 0;
}
