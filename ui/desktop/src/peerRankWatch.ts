import {
  macNameOfHost,
  numLayersOf,
  parseGooseRanks,
  pipelineLayers,
  servesAnotherMac,
  sshClientOf,
  type PeerRankLayers,
  type PeerRankSeen,
} from './utils/peerRank';

export type { PeerRankSeen };

/**
 * main's read of the goose ranks running on this Mac for ANOTHER Mac (utils/peerRank.ts): the
 * process list on a slow cadence, each new rank's requester from its ssh session's environment and
 * its layers from the model's config.json — the facts the floating window needs to say this Mac's
 * part ("Layers 17–47 of workhorse's split"). Electron-free: main hands it the process reads.
 */

export interface PeerRankWatchDeps {
  /** `ps -axww -o pid=,ppid=,command=`. */
  listProcesses(): Promise<string>;
  /** `ps -E -ww -o command= -p <pid>`: the command with its environment. */
  processEnvironment(pid: number): Promise<string>;
  /** A reverse lookup of an address; null when nothing answers. */
  reverseLookup(address: string): Promise<string | null>;
  readFile(path: string): Promise<string>;
  ownPid: number;
  onChanged(ranks: PeerRankSeen[]): void;
  warn(message: string): void;
}

/** How often the process list is read: a split's start takes minutes, the card follows in seconds. */
export const PEER_RANK_SCAN_MS = 10_000;

export class PeerRankWatch {
  private known = new Map<number, PeerRankSeen>();
  private lastKey = '';
  private reading = false;

  constructor(private readonly deps: PeerRankWatchDeps) {}

  current(): PeerRankSeen[] {
    return [...this.known.values()];
  }

  /** One read of the process list; a rank seen before keeps what was learned about it. */
  async scan(): Promise<void> {
    if (this.reading) return;
    this.reading = true;
    try {
      const listing = await this.deps.listProcesses();
      const ranks = parseGooseRanks(listing, [this.deps.ownPid]).filter(servesAnotherMac);
      const next = new Map<number, PeerRankSeen>();
      for (const rank of ranks) {
        const seen = this.known.get(rank.pid) ?? (await this.learn(rank));
        next.set(rank.pid, seen);
      }
      this.known = next;
      const key = JSON.stringify(this.current());
      if (key !== this.lastKey) {
        this.lastKey = key;
        this.deps.onChanged(this.current());
      }
    } catch (error) {
      this.deps.warn(`peer rank scan failed: ${String(error)}`);
    } finally {
      this.reading = false;
    }
  }

  private async learn(rank: ReturnType<typeof parseGooseRanks>[number]): Promise<PeerRankSeen> {
    let requester: string | null = null;
    try {
      const address = sshClientOf(await this.deps.processEnvironment(rank.pid));
      if (address) {
        const host = await this.deps.reverseLookup(address).catch(() => null);
        requester = macNameOfHost(host) ?? address;
      }
    } catch (error) {
      this.deps.warn(`peer rank ${rank.pid}: environment unreadable: ${String(error)}`);
    }
    let layers: PeerRankLayers | null = null;
    if (rank.splitStarts && rank.modelDir) {
      const config = await this.deps
        .readFile(`${rank.modelDir.replace(/\/$/, '')}/config.json`)
        .catch(() => null);
      layers = pipelineLayers(rank.rank, rank.splitStarts, config ? numLayersOf(config) : null);
    }
    return {
      pid: rank.pid,
      rank: rank.rank,
      size: rank.size,
      modelId: rank.servedId,
      backend: rank.backend,
      requester,
      layers,
    };
  }
}
