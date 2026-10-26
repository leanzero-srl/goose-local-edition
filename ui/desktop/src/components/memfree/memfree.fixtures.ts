import { GIB, type MemfreeGroup, type MemoryHolders } from './memfree';

export const gb = (n: number) => Math.round(n * GIB);

export const app = (id: string, label: string, sizeGb: number): MemfreeGroup => ({
  id,
  label,
  category: 'userApp',
  bytes: gb(sizeGb),
  pids: [{ pid: 100, startedAt: 1 }],
  bundleId: id,
  quittable: true,
  reclaimableByGoose: false,
});

/** Gabee, 2026-10-25 18:08 (RESEARCH.md §3): need 30.6, budget 9.4, short 21.2, apps 15.5. */
export const GABEE: MemoryHolders = {
  availableBytes: gb(15.4),
  budgetBytes: gb(9.4),
  totalBytes: gb(64),
  groups: [
    app('com.google.Chrome', 'Google Chrome', 7.2),
    // Measured as Teams 4.1 GB; Teams is never-touch (the engine leaves it out of appsBytes since
    // 3.0.132), so the fixture uses an ordinary app of the same size to keep the figures.
    app('com.spotify.client', 'Spotify', 4.1),
    app('com.apple.Safari', 'Safari', 4.2),
    {
      id: 'WindowServer',
      label: 'WindowServer',
      category: 'system',
      bytes: gb(1.1),
      pids: [{ pid: 150, startedAt: 1 }],
      quittable: false,
      reclaimableByGoose: false,
    },
  ],
  gooseOwnedBytes: 0,
  appsBytes: gb(15.5),
  shortBytes: gb(21.2),
  alternatives: [
    { kind: 'smallerVariant', modelId: 'mlx-community/Qwen3.8-27B-4bit', needBytes: gb(16.1) },
    { kind: 'peerMac', nodeId: 'studio', label: 'Mac Studio', budgetBytes: gb(98.3) },
  ],
  quitAppsAllowed: true,
};
