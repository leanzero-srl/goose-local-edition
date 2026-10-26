import { assertAccountKey, assertRegistryRecord, type DeviceRegistryStore, type RegistryRecord } from "./registry-store";

// All reads, including introspection, hit this strongly consistent object, never Workers KV.
// https://developers.cloudflare.com/durable-objects/api/sqlite-storage-api/
export class DeviceRegistryDurableObject {
  constructor(private readonly state: DurableObjectState) {
    state.storage.transactionSync(() => {
      state.storage.sql.exec("CREATE TABLE IF NOT EXISTS registry_schema (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL)");
      const version = state.storage.sql.exec("SELECT version FROM registry_schema WHERE id=1").toArray()[0]?.version;
      if (version !== undefined && version !== 1) throw new Error("unsupported controller registry schema");
      state.storage.sql.exec("CREATE TABLE IF NOT EXISTS account (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL, value TEXT NOT NULL)");
      state.storage.sql.exec("INSERT OR IGNORE INTO registry_schema(id,version) VALUES(1,1)");
    });
  }

  async fetch(request: Request): Promise<Response> {
    const path = new URL(request.url).pathname;
    if (request.method === "GET" && path === "/record") {
      const row = this.state.storage.sql.exec("SELECT revision,value FROM account WHERE id=1").toArray()[0];
      return Response.json(row === undefined ? null : assertRegistryRecord(row));
    }
    if (request.method !== "POST" || path !== "/compare-and-swap") return new Response(null, { status: 404 });
    const body = await request.json() as { expectedRevision?: unknown; value?: unknown };
    const revision = body.expectedRevision;
    if ((revision !== null && (!Number.isSafeInteger(revision) || (revision as number) < 1)) || typeof body.value !== "string") {
      return new Response(null, { status: 400 });
    }
    const value = body.value;
    const changed = this.state.storage.transactionSync(() => {
      const row = this.state.storage.sql.exec("SELECT revision FROM account WHERE id=1").toArray()[0];
      if ((row?.revision ?? null) !== revision) return false;
      this.state.storage.sql.exec("INSERT INTO account(id,revision,value) VALUES(1,?,?) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,value=excluded.value", (revision as number | null ?? 0) + 1, value);
      return true;
    });
    return Response.json(changed);
  }
}

export class DurableDeviceRegistry implements DeviceRegistryStore {
  constructor(private readonly namespace: DurableObjectNamespace) {}

  private stub(key: string): DurableObjectStub {
    assertAccountKey(key);
    return this.namespace.get(this.namespace.idFromName(key));
  }

  async read(key: string): Promise<RegistryRecord | null> {
    const response = await this.stub(key).fetch("https://registry.internal/record");
    if (!response.ok) throw new Error("controller registry read failed");
    const result: unknown = await response.json();
    return result === null ? null : assertRegistryRecord(result);
  }

  async compareAndSwap(key: string, expectedRevision: number | null, value: string): Promise<boolean> {
    const response = await this.stub(key).fetch("https://registry.internal/compare-and-swap", {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ expectedRevision, value }),
    });
    if (!response.ok) throw new Error("controller registry transaction failed");
    const result: unknown = await response.json();
    if (typeof result !== "boolean") throw new Error("invalid registry transaction result");
    return result;
  }
}
