import { parseConfig, type WorkerEnvVars } from "../src/lib/config";
import type { Deps, FetchFn, KVStore, KvMutation } from "../src/lib/deps";

export class FakeClock {
  nowMs: number;

  constructor(startMs = 1_756_000_000_000) {
    this.nowMs = startMs;
  }

  now = (): number => this.nowMs;

  advanceSeconds(seconds: number): void {
    this.nowMs += seconds * 1000;
  }
}

export class MemoryKV implements KVStore {
  entries = new Map<string, { value: string; expiresAtMs: number | null }>();

  constructor(private readonly clock: () => number) {}

  async get(key: string): Promise<string | null> {
    const entry = this.entries.get(key);
    if (entry === undefined) {
      return null;
    }
    if (entry.expiresAtMs !== null && this.clock() >= entry.expiresAtMs) {
      this.entries.delete(key);
      return null;
    }
    return entry.value;
  }

  async put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void> {
    if (options?.expirationTtl !== undefined && options.expirationTtl < 60) {
      // Mirrors Cloudflare KV's real constraint so a too-short TTL fails in tests, not in production.
      throw new Error(`KV expirationTtl must be >= 60 seconds, got ${options.expirationTtl}`);
    }
    const expiresAtMs = options?.expirationTtl !== undefined ? this.clock() + options.expirationTtl * 1000 : null;
    this.entries.set(key, { value, expiresAtMs });
  }

  async delete(key: string): Promise<void> {
    this.entries.delete(key);
  }

  // Read, mutate and write inside ONE synchronous step — nothing can interleave, which
  // is the contract the handlers rely on and what fs-kv's per-key chain provides.
  async update(key: string, mutate: (current: string | null) => KvMutation): Promise<void> {
    const entry = this.entries.get(key);
    const current = entry === undefined || (entry.expiresAtMs !== null && this.clock() >= entry.expiresAtMs) ? null : entry.value;
    if (current === null) {
      this.entries.delete(key);
    }
    const decision = mutate(current);
    if (decision === "keep") {
      return;
    }
    if (decision === "delete") {
      this.entries.delete(key);
      return;
    }
    if (decision.expirationTtl !== undefined && decision.expirationTtl < 60) {
      throw new Error(`KV expirationTtl must be >= 60 seconds, got ${decision.expirationTtl}`);
    }
    const expiresAtMs = decision.expirationTtl !== undefined ? this.clock() + decision.expirationTtl * 1000 : null;
    this.entries.set(key, { value: decision.value, expiresAtMs });
  }
}

export interface RecordedCall {
  url: string;
  init: RequestInit | undefined;
}

export function jsonRes(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

export function jsonBody(init: RequestInit | undefined): unknown {
  if (init === undefined || typeof init.body !== "string") {
    throw new Error("expected a string request body");
  }
  return JSON.parse(init.body);
}

export const FULL_ENV: WorkerEnvVars = {
  LINK_JWT_SECRET: "unit-test-jwt-secret-with-at-least-32-bytes",
  RESEND_API_KEY: "re_test_key",
  RESEND_AUDIENCE_ID: "seg_test_audience",
  LEANZERO_MAIL_FROM: "LeanZero Link <link@leanzero.test>",
  TS_API_TOKEN: "tskey-api-test-token",
  TS_TAILNET: "example.ts.net",
};

// Recorded fixture of the documented Tailscale create-key response — shape verified
// against https://tailscale.com/api (POST /api/v2/tailnet/{tailnet}/keys; the same
// response example is published in the tailscale/tailscale repo's api.md, "Tailnet keys"):
// { "id", "key": "tskey-...", "created", "expires", "capabilities": { "devices": { "create":
//   { "reusable", "ephemeral", "preauthorized", "tags" } } } }
export const TAILSCALE_CREATE_KEY_FIXTURE = {
  id: "k123456CNTRL",
  key: "tskey-auth-k123456CNTRL-abcdefghijklmnopqrstuvwxyz",
  created: "2026-09-01T00:00:00Z",
  expires: "2026-09-01T00:10:00Z",
  capabilities: {
    devices: {
      create: {
        reusable: false,
        ephemeral: true,
        preauthorized: true,
        tags: ["tag:leanzero-link"],
      },
    },
  },
} as const;

export interface TestHarness {
  deps: Deps;
  clock: FakeClock;
  kv: MemoryKV;
  calls: RecordedCall[];
  logs: Array<{ event: string; fields: Record<string, unknown> | undefined }>;
}

export type FetchHandler = (url: string, init?: RequestInit) => Response | Promise<Response>;

export function makeHarness(
  options: { env?: WorkerEnvVars; fetchHandler?: FetchHandler; otp?: string[] } = {},
): TestHarness {
  const clock = new FakeClock();
  const kv = new MemoryKV(clock.now);
  const otpQueue = [...(options.otp ?? ["123456"])];
  const logs: TestHarness["logs"] = [];
  const calls: RecordedCall[] = [];
  const handler: FetchHandler =
    options.fetchHandler ??
    ((url: string) => {
      throw new Error(`unexpected fetch: ${url}`);
    });
  const fetchFn: FetchFn = async (url, init) => {
    calls.push({ url, init });
    return handler(url, init);
  };
  const deps: Deps = {
    kv,
    fetchFn,
    now: clock.now,
    randomOtp: () => {
      const code = otpQueue.shift();
      if (code === undefined) {
        throw new Error("otp queue exhausted — pass more codes to makeHarness");
      }
      return code;
    },
    log: (event, fields) => {
      logs.push({ event, fields });
    },
    config: parseConfig(options.env ?? FULL_ENV),
  };
  return { deps, clock, kv, calls, logs };
}

export function postJson(path: string, body: unknown, headers: Record<string, string> = {}): Request {
  return new Request(`https://link.example${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...headers },
    body: JSON.stringify(body),
  });
}

export const RESEND_EMAILS_URL = "https://api.resend.com/emails";
export const RESEND_CONTACTS_URL = "https://api.resend.com/contacts";

export const resendHappyFetch: FetchHandler = (url) => {
  if (url === RESEND_EMAILS_URL) {
    return jsonRes(200, { id: "email_test_1" });
  }
  if (url === RESEND_CONTACTS_URL) {
    return jsonRes(201, { object: "contact", id: "contact_test_1" });
  }
  throw new Error(`unexpected fetch: ${url}`);
};

export async function responseJson(response: Response): Promise<Record<string, unknown>> {
  return (await response.json()) as Record<string, unknown>;
}
