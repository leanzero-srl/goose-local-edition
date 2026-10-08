import { parseConfig, type WorkerEnvVars } from "./lib/config";
import type { Deps, KVStore, KvMutation } from "./lib/deps";
import { generateOtp } from "./lib/otp";
import { handleRequest } from "./router";

export interface Env extends WorkerEnvVars {
  LINK_KV: KVNamespace;
}

function kvStore(env: Env): KVStore {
  const namespace: KVNamespace | undefined = env.LINK_KV;
  if (namespace === undefined) {
    const fail = (): never => {
      throw new Error("KV binding LINK_KV is not configured; add the kv_namespaces entry to wrangler.toml");
    };
    return { get: async () => fail(), put: async () => fail(), delete: async () => fail(), update: async () => fail() };
  }
  // Workers KV has no compare-and-swap, so `update` here is a plain read-then-write and is
  // NOT atomic across the edge (documented in the README: the per-email verify rate limit
  // is the bound that holds regardless; a Durable Object is the upgrade path).
  const update = async (key: string, mutate: (current: string | null) => KvMutation): Promise<void> => {
    const decision = mutate(await namespace.get(key));
    if (decision === "keep") {
      return;
    }
    if (decision === "delete") {
      await namespace.delete(key);
      return;
    }
    await namespace.put(key, decision.value, decision.expirationTtl === undefined ? undefined : { expirationTtl: decision.expirationTtl });
  };
  return {
    get: (key) => namespace.get(key),
    put: (key, value, options) => namespace.put(key, value, options),
    delete: (key) => namespace.delete(key),
    update,
  };
}

// The Node adapter logs config warnings at boot; a Worker has no boot, so log them once per isolate.
let warnedConfig = false;

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const deps: Deps = {
      kv: kvStore(env),
      fetchFn: (url, init) => fetch(url, init),
      now: () => Date.now(),
      randomOtp: generateOtp,
      log: (event, fields) => console.log(JSON.stringify({ event, ...fields })),
      config: parseConfig(env),
      waitUntil: (work) => ctx.waitUntil(work),
    };
    if (!warnedConfig) {
      warnedConfig = true;
      for (const warning of deps.config.warnings) deps.log("config_error", { ...warning });
    }
    return handleRequest(request, deps);
  },
} satisfies ExportedHandler<Env>;
