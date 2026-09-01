import type { KVStore } from "./deps";

export interface RateLimitCheck {
  limited: boolean;
  retryAfterSeconds: number;
}

// Fixed-window counter in KV, bumped through the store's atomic `update` so a concurrent
// burst cannot all read 0 and all pass (the get→put version did exactly that, measured at
// 500 concurrent calls). On the filesystem store the update is serialized per key; on
// Cloudflare KV it is eventually consistent (documented in the README). The TTL only
// garbage-collects stale windows — the window arithmetic is what enforces the limit.
export async function bumpFixedWindow(
  kv: KVStore,
  keyPrefix: string,
  limit: number,
  windowSeconds: number,
  nowMs: number,
): Promise<RateLimitCheck> {
  const nowSeconds = Math.floor(nowMs / 1000);
  const window = Math.floor(nowSeconds / windowSeconds);
  const key = `${keyPrefix}:${window}`;
  let limited = false;
  await kv.update(key, (raw) => {
    const count = raw === null ? 0 : Number.parseInt(raw, 10) || 0;
    if (count >= limit) {
      limited = true;
      return "keep";
    }
    return { value: String(count + 1), expirationTtl: windowSeconds * 2 };
  });
  if (limited) {
    return { limited: true, retryAfterSeconds: (window + 1) * windowSeconds - nowSeconds };
  }
  return { limited: false, retryAfterSeconds: 0 };
}
