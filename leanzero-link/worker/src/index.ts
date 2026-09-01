import { parseConfig, type WorkerEnvVars } from "./lib/config";
import type { Deps, KVStore } from "./lib/deps";
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
    return { get: async () => fail(), put: async () => fail(), delete: async () => fail() };
  }
  return {
    get: (key) => namespace.get(key),
    put: (key, value, options) => namespace.put(key, value, options),
    delete: (key) => namespace.delete(key),
  };
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const deps: Deps = {
      kv: kvStore(env),
      fetchFn: (url, init) => fetch(url, init),
      now: () => Date.now(),
      randomOtp: generateOtp,
      log: (event, fields) => console.log(JSON.stringify({ event, ...fields })),
      config: parseConfig(env),
    };
    return handleRequest(request, deps);
  },
} satisfies ExportedHandler<Env>;
