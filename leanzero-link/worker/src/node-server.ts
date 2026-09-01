import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { capabilities, parseConfig, RATE_WINDOW_SECONDS, type Config } from "./lib/config";
import type { Deps, KVStore } from "./lib/deps";
import { createFsKvStore, type FsKvStore } from "./lib/fs-kv";
import { generateOtp } from "./lib/otp";
import { handleRequest } from "./router";

// Self-hosted Node adapter: the same handleRequest(request, deps) the Cloudflare
// Worker serves, but the deps are built from process.env + a filesystem KV, and the
// Web Request/Response are bridged to Node's http server. Bind loopback only —
// Tailscale Funnel terminates TLS and forwards to 127.0.0.1:<PORT>.

const DEFAULT_PORT = 8791;
const MAX_BODY_BYTES = 1_048_576; // 1 MiB — OTP/verify/join-key bodies are tiny JSON.

function jsonLog(event: string, fields?: Record<string, unknown>): void {
  console.log(JSON.stringify({ event, ...fields }));
}

function defaultKvDir(): string {
  return process.env.LINK_KV_DIR ?? join(homedir(), ".leanzero", "link-kv");
}

export function buildDeps(kv: KVStore): Deps {
  const config = parseConfig(process.env);
  return {
    kv,
    fetchFn: (url, init) => fetch(url, init),
    now: () => Date.now(),
    randomOtp: generateOtp,
    log: jsonLog,
    config,
  };
}

export class BodyTooLargeError extends Error {
  constructor(public readonly limitBytes: number) {
    super(`request body exceeds ${limitBytes} bytes`);
    this.name = "BodyTooLargeError";
  }
}

// Past the limit the body is no longer buffered: the promise rejects at once (the 413 goes
// out immediately) and the rest of the request is DRAINED, not reset — `req.destroy()`
// here tore the socket down before the response was written, so a client saw ECONNRESET
// and never a 413 (measured by the adapter test). A sender that keeps going past the
// drain allowance is cut off.
const DRAIN_ALLOWANCE_BYTES = MAX_BODY_BYTES * 4;

function collectBody(req: IncomingMessage): Promise<Buffer | null> {
  const method = (req.method ?? "GET").toUpperCase();
  if (method === "GET" || method === "HEAD") {
    return Promise.resolve(null);
  }
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let total = 0;
    let overflowed = false;
    req.on("data", (chunk: Buffer) => {
      total += chunk.length;
      if (overflowed) {
        if (total > DRAIN_ALLOWANCE_BYTES) {
          req.destroy();
        }
        return;
      }
      if (total > MAX_BODY_BYTES) {
        overflowed = true;
        chunks.length = 0;
        reject(new BodyTooLargeError(MAX_BODY_BYTES));
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => resolve(chunks.length > 0 ? Buffer.concat(chunks) : Buffer.alloc(0)));
    req.on("error", reject);
  });
}

/// 413 is reserved for the one error that means "too large"; an aborted or malformed
/// request prelude is a 400 — it used to be reported as 413 too.
export function preludeErrorStatus(error: unknown): { status: 413 | 400; error: string } {
  return error instanceof BodyTooLargeError
    ? { status: 413, error: "request body too large" }
    : { status: 400, error: "malformed request" };
}

// Headers a client may send to impersonate a trusted proxy. Nothing in front of this
// server ever sets CF-Connecting-IP (Cloudflare is not on this path), so any inbound value
// is client-supplied and is dropped before the handlers see it; X-Forwarded-For is kept
// because Tailscale Funnel/Serve SETS it (replacing any inbound value) — see lib/clientIp.ts.
const UNTRUSTED_PROXY_HEADERS = new Set(["cf-connecting-ip"]);

export function toWebRequest(req: IncomingMessage, body: Buffer | null, port: number): Request {
  const host = req.headers.host ?? `127.0.0.1:${port}`;
  const url = `http://${host}${req.url ?? "/"}`;
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (value === undefined || UNTRUSTED_PROXY_HEADERS.has(name.toLowerCase())) {
      continue;
    }
    if (Array.isArray(value)) {
      for (const item of value) {
        headers.append(name, item);
      }
    } else {
      headers.set(name, value);
    }
  }
  const method = (req.method ?? "GET").toUpperCase();
  const init: RequestInit = { method, headers };
  if (body !== null && method !== "GET" && method !== "HEAD") {
    init.body = body;
  }
  return new Request(url, init);
}

async function writeWebResponse(res: ServerResponse, response: Response): Promise<void> {
  const headers: Record<string, string> = {};
  response.headers.forEach((value, name) => {
    headers[name] = value;
  });
  const buffer = Buffer.from(await response.arrayBuffer());
  res.writeHead(response.status, headers);
  res.end(buffer);
}

export function createNodeServer(deps: Deps, port: number): Server {
  return createServer((req: IncomingMessage, res: ServerResponse) => {
    void (async () => {
      let request: Request;
      try {
        const body = await collectBody(req);
        request = toWebRequest(req, body, port);
      } catch (error) {
        deps.log("node_request_error", { error: error instanceof Error ? error.message : String(error) });
        const failure = preludeErrorStatus(error);
        res.writeHead(failure.status, { "Content-Type": "application/json; charset=utf-8", Connection: "close" });
        res.end(JSON.stringify({ error: failure.error }));
        return;
      }
      try {
        const response = await handleRequest(request, deps);
        await writeWebResponse(res, response);
      } catch (error) {
        deps.log("node_unhandled_error", { error: error instanceof Error ? `${error.message}\n${error.stack ?? ""}` : String(error) });
        if (!res.headersSent) {
          res.writeHead(500, { "Content-Type": "application/json; charset=utf-8" });
        }
        res.end(JSON.stringify({ error: "internal error" }));
      }
    })();
  });
}

/// Every parseConfig warning becomes a `config_error` line at boot — the operator reads
/// the log once, not a 500 body later.
export function logConfigWarnings(config: Config, log: (event: string, fields?: Record<string, unknown>) => void): void {
  for (const warning of config.warnings) {
    log("config_error", { ...warning });
  }
}

/// W-L9: the filesystem KV only drops an expired `rl:*` / `otp:*` file when that key is
/// read again, so files for never-repeated emails and addresses accumulate. Sweep once at
/// boot and then once per rate window — the window is what gives those records their
/// lifetime (`bumpFixedWindow` writes `expirationTtl: windowSeconds * 2`), so nothing
/// expired ever outlives two sweeps. Storage housekeeping only; no model work rides on it.
function startExpirySweeps(kv: FsKvStore, now: () => number, log: (event: string, fields?: Record<string, unknown>) => void): void {
  const sweep = async (): Promise<void> => {
    try {
      const removed = await kv.sweepExpired(now);
      log("fs_kv_swept", { removed });
    } catch (error) {
      log("fs_kv_sweep_error", { error: error instanceof Error ? error.message : String(error) });
    }
  };
  void sweep();
  setInterval(() => void sweep(), RATE_WINDOW_SECONDS * 1000).unref();
}

function main(): void {
  const port = Number(process.env.PORT ?? DEFAULT_PORT);
  const kv = createFsKvStore(defaultKvDir(), { log: jsonLog });
  const deps = buildDeps(kv);
  logConfigWarnings(deps.config, jsonLog);
  startExpirySweeps(kv, deps.now, jsonLog);
  const server = createNodeServer(deps, port);
  server.listen(port, "127.0.0.1", () => {
    jsonLog("node_server_listening", {
      addr: `127.0.0.1:${port}`,
      kvDir: defaultKvDir(),
      capabilities: capabilities(deps.config),
      meshProvider: deps.config.meshProvider,
      ok: Boolean(deps.config.jwtSecret),
    });
  });
}

// Only auto-start when run as the entrypoint (tsx src/node-server.ts), not when
// imported by the smoke test, which drives createNodeServer on an ephemeral port.
// pathToFileURL percent-encodes and resolves argv[1] the way the loader did for
// import.meta.url — the string form `file://${argv[1]}` was false for any checkout
// path containing a space, and main() silently never ran.
export function isEntrypoint(importMetaUrl: string, argv1: string | undefined): boolean {
  return argv1 !== undefined && importMetaUrl === pathToFileURL(argv1).href;
}

if (isEntrypoint(import.meta.url, process.argv[1])) {
  main();
}
