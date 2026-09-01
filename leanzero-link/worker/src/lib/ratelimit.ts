import type { KVStore } from "./deps";

export interface RateLimitCheck {
  limited: boolean;
  retryAfterSeconds: number;
}

// Fixed-window counter in KV. KV is eventually consistent, so a burst racing a
// single window edge can slightly overshoot the limit; for an OTP mailer that
// slack is acceptable and documented in the README. The TTL only garbage-collects
// stale windows — the window arithmetic is what enforces the limit.
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
  const raw = await kv.get(key);
  const count = raw === null ? 0 : Number.parseInt(raw, 10) || 0;
  if (count >= limit) {
    return { limited: true, retryAfterSeconds: (window + 1) * windowSeconds - nowSeconds };
  }
  await kv.put(key, String(count + 1), { expirationTtl: windowSeconds * 2 });
  return { limited: false, retryAfterSeconds: 0 };
}
