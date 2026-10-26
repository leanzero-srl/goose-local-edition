export interface RegistryRecord { revision: number; value: string }

/** CAS is authoritative, including across processes/isolates. KV is not an implementation. */
export interface DeviceRegistryStore {
  read(accountKey: string): Promise<RegistryRecord | null>;
  compareAndSwap(accountKey: string, expectedRevision: number | null, value: string): Promise<boolean>;
}

export function assertAccountKey(key: string): void {
  if (!/^[0-9a-f]{64}$/.test(key)) throw new Error("invalid registry account key");
}

export function assertRegistryRecord(value: unknown): RegistryRecord {
  if (typeof value !== "object" || value === null) throw new Error("invalid registry record");
  const row = value as Record<string, unknown>;
  if (!Number.isSafeInteger(row.revision) || (row.revision as number) < 1 || typeof row.value !== "string") {
    throw new Error("corrupt registry record");
  }
  return { revision: row.revision as number, value: row.value };
}
