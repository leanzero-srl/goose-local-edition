import { chmodSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assertAccountKey, assertRegistryRecord, type DeviceRegistryStore, type RegistryRecord } from "./registry-store";

// DatabaseSync is the supported export on Node 24 and the measured Node 26.5 runtime.
// https://nodejs.org/docs/latest-v24.x/api/sqlite.html
export class SqliteDeviceRegistry implements DeviceRegistryStore {
  private readonly db: DatabaseSync;

  constructor(path: string) {
    if (path !== ":memory:") mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
    this.db = new DatabaseSync(path);
    if (path !== ":memory:") chmodSync(path, 0o600);
    this.db.exec("PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;");
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const version = this.db.prepare("PRAGMA user_version").get()?.user_version;
      if (version !== 0 && version !== 1) throw new Error("unsupported controller registry schema");
      this.db.exec("CREATE TABLE IF NOT EXISTS accounts (account_key TEXT PRIMARY KEY, revision INTEGER NOT NULL, value TEXT NOT NULL); PRAGMA user_version=1; COMMIT;");
    } catch (error) {
      this.db.exec("ROLLBACK");
      this.db.close();
      throw error;
    }
  }

  async read(accountKey: string): Promise<RegistryRecord | null> {
    assertAccountKey(accountKey);
    const row = this.db.prepare("SELECT revision, value FROM accounts WHERE account_key=?").get(accountKey);
    return row === undefined ? null : assertRegistryRecord(row);
  }

  async compareAndSwap(accountKey: string, expectedRevision: number | null, value: string): Promise<boolean> {
    assertAccountKey(accountKey);
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const result = expectedRevision === null
        ? this.db.prepare("INSERT OR IGNORE INTO accounts(account_key,revision,value) VALUES (?,1,?)").run(accountKey, value)
        : this.db.prepare("UPDATE accounts SET value=?, revision=revision+1 WHERE account_key=? AND revision=?").run(value, accountKey, expectedRevision);
      this.db.exec("COMMIT");
      return result.changes === 1;
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }

  close(): void { this.db.close(); }
}
