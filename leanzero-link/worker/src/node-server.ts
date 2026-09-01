import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { homedir } from "node:os";
import { join } from "node:path";
import { capabilities, parseConfig } from "./lib/config";
import type { Deps } from "./lib/deps";
import { createFsKvStore } from "./lib/fs-kv";
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

export function buildDeps(): Deps {
  const config = parseConfig(process.env);
  return {
    kv: createFsKvStore(defaultKvDir(), { log: jsonLog }),
    fetchFn: (url, init) => fetch(url, init),
    now: () => Date.now(),
    randomOtp: generateOtp,
    log: jsonLog,
    config,
  };
}

function collectBody(req: IncomingMessage): Promise<Buffer | null> {
  const method = (req.method ?? "GET").toUpperCase();
  if (method === "GET" || method === "HEAD") {
    return Promise.resolve(null);
  }
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let total = 0;
    req.on("data", (chunk: Buffer) => {
      total += chunk.length;
      if (total > MAX_BODY_BYTES) {
        reject(new Error("request body too large"));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => resolve(chunks.length > 0 ? Buffer.concat(chunks) : Buffer.alloc(0)));
    req.on("error", reject);
  });
}

function toWebRequest(req: IncomingMessage, body: Buffer | null, port: number): Request {
  const host = req.headers.host ?? `127.0.0.1:${port}`;
  const url = `http://${host}${req.url ?? "/"}`;
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (value === undefined) {
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
        res.writeHead(413, { "Content-Type": "application/json; charset=utf-8" });
        res.end(JSON.stringify({ error: "request body too large" }));
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

function main(): void {
  const port = Number(process.env.PORT ?? DEFAULT_PORT);
  const deps = buildDeps();
  const server = createNodeServer(deps, port);
  server.listen(port, "127.0.0.1", () => {
    jsonLog("node_server_listening", {
      addr: `127.0.0.1:${port}`,
      kvDir: defaultKvDir(),
      capabilities: capabilities(deps.config),
      ok: Boolean(deps.config.jwtSecret),
    });
  });
}

// Only auto-start when run as the entrypoint (tsx src/node-server.ts), not when
// imported by the smoke test, which drives createNodeServer on an ephemeral port.
if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
