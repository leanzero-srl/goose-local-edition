import { createServer, type Server } from "node:http";
import { usernameForEmail } from "../src/lib/headscale";

interface User { id: string; name: string }
interface Key { id: string; key: string; user: User; reusable: boolean; ephemeral: boolean; used: boolean; expiration: string; aclTags: string[] }
export interface MockNode { id: string; nodeKey: string; user: User; preAuthKey: Key | null; ipAddresses: string[]; online: boolean; expiry: string | null; tags: string[] }

/** Wire-level HTTP mock: official v0.29.3 REST spelling and actual HTTP boundaries.
 * A wrong account remains in the full listing as the isolation negative control. */
export class HeadscaleWireMock {
  server!: Server; url = ""; users: User[] = []; keys: Key[] = []; nodes: MockNode[] = [];
  calls: { path: string; method: string; body: Record<string, unknown> }[] = [];
  unavailable = false; paginated = false; badPolicy = false;
  onMint: (() => Promise<void>) | null = null;
  private serial = 0;
  private next(): string { this.serial += 1; return String(this.serial); }
  async start(): Promise<void> {
    this.server = createServer(async (req, res) => {
      const url = new URL(req.url!, "http://localhost"); const method = req.method!;
      const chunks: Buffer[] = []; for await (const chunk of req) chunks.push(Buffer.from(chunk));
      const raw = Buffer.concat(chunks).toString(); const body = raw ? JSON.parse(raw) as Record<string, unknown> : {};
      this.calls.push({ path: url.pathname, method, body });
      const send = (status: number, value: unknown): void => { res.writeHead(status, { "Content-Type": "application/json" }); res.end(JSON.stringify(value)); };
      if (req.headers.authorization !== "Bearer test-headscale-api") return send(401, {});
      if (this.unavailable) return send(503, {});
      if (url.pathname === "/api/v1/policy") return send(200, { policy: JSON.stringify({ acls: [{ action: "accept", src: ["*"], dst: [this.badPolicy ? "*:*" : "autogroup:self:*"] }] }) });
      if (url.pathname === "/api/v1/user") return send(200, { users: this.users.filter(u => u.name === url.searchParams.get("name")) });
      if (url.pathname === "/api/v1/node" && method === "GET") return send(200, { nodes: this.nodes, ...(this.paginated ? { nextPageToken: "more-nodes" } : {}) });
      if (url.pathname === "/api/v1/preauthkey" && method === "GET") return send(200, { preAuthKeys: this.keys });
      if (url.pathname === "/api/v1/preauthkey" && method === "POST") {
        const user = this.users.find(u => u.id === body.user);
        if (!user) return send(400, {});
        const id = this.next();
        const key: Key = { id, key: "hskey-auth-test-" + id, user, reusable: body.reusable as boolean, ephemeral: body.ephemeral as boolean, used: false, expiration: body.expiration as string, aclTags: [] };
        this.keys.push(key);
        if (this.onMint) await this.onMint();
        return send(200, { preAuthKey: key });
      }
      if (url.pathname === "/api/v1/preauthkey/expire") {
        const key = this.keys.find(k => k.id === body.id);
        if (!key) return send(404, {});
        key.expiration = "2000-01-01T00:00:00Z"; return send(200, {});
      }
      if (/^\/api\/v1\/node\/[0-9]+$/.test(url.pathname) && method === "DELETE") {
        const id = url.pathname.split("/").at(-1); this.nodes = this.nodes.filter(n => n.id !== id); return send(200, {});
      }
      send(404, {});
    });
    await new Promise<void>(resolve => this.server.listen(0, "127.0.0.1", resolve));
    const address = this.server.address(); if (typeof address !== "object" || address === null) throw new Error("listener missing");
    this.url = "http://127.0.0.1:" + address.port;
  }
  async stop(): Promise<void> { await new Promise<void>((resolve, reject) => this.server.close(e => e ? reject(e) : resolve())); }
  async account(email: string): Promise<User> {
    const user = { id: this.next(), name: await usernameForEmail(email) }; this.users.push(user); return user;
  }
  join(authKey: string, keyChar: string): MockNode {
    const key = this.keys.find(k => k.key === authKey); if (!key) throw new Error("missing auth key");
    key.used = true;
    const n = { id: this.next(), nodeKey: "nodekey:" + keyChar.repeat(64), user: key.user, preAuthKey: key, ipAddresses: ["100.100.0." + (this.serial % 200 + 1), "fd7a:115c:a1e0::1234"], online: true, expiry: null, tags: [] };
    this.nodes.push(n); return n;
  }
  legacy(user: User, keyChar = "e"): MockNode {
    const n = { id: this.next(), nodeKey: "nodekey:" + keyChar.repeat(64), user, preAuthKey: null, ipAddresses: ["100.100.0.200"], online: true, expiry: null, tags: [] };
    this.nodes.push(n); return n;
  }
}
