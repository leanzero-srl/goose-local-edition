import type { MlxEngineStatus } from './mlx-engine';

/**
 * THIS Mac's engine as goose last answered it, from whichever surface read it (the Engine view's
 * poll, the composer's) — `mlxEngineStatus` records every local read here. For a surface that needs
 * a fact of the status its parent did not hand down: the state tile's measured runs are keyed by
 * the HF model id, and the tile is handed only the state word. null = never read in this window.
 */
let latest: MlxEngineStatus | null = null;
const listeners = new Set<() => void>();

export function latestLocalMlxEngineStatus(): MlxEngineStatus | null {
  return latest;
}

export function rememberLocalMlxEngineStatus(status: MlxEngineStatus | null): void {
  latest = status;
  listeners.forEach((listener) => listener());
}

export function subscribeLocalMlxEngineStatus(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
