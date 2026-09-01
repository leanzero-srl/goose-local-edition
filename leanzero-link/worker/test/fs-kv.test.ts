import { mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFsKvStore, fileNameFor } from "../src/lib/fs-kv";

// Every test operates in a fresh mkdtemp under the OS temp dir — never a real
// LINK_KV_DIR. The dir is removed in afterEach.
let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "link-fs-kv-"));
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

function captureLog(): {
  log: (event: string, fields?: Record<string, unknown>) => void;
  events: Array<{ event: string; fields?: Record<string, unknown> }>;
} {
  const events: Array<{ event: string; fields?: Record<string, unknown> }> = [];
  return { log: (event, fields) => events.push({ event, fields }), events };
}

describe("createFsKvStore", () => {
  it("round-trips put/get and persists across store instances over the same dir", async () => {
    const kv = createFsKvStore(dir);
    await kv.put("otp:user@example.com", "hello");
    expect(await kv.get("otp:user@example.com")).toBe("hello");

    const reopened = createFsKvStore(dir);
    expect(await reopened.get("otp:user@example.com")).toBe("hello");
  });

  it("returns null for a key that was never written", async () => {
    const kv = createFsKvStore(dir);
    expect(await kv.get("missing")).toBeNull();
  });

  it("honors expirationTtl: an expired record returns null and the file is removed", async () => {
    let nowMs = 1_000_000;
    const kv = createFsKvStore(dir, { now: () => nowMs });
    await kv.put("rl:email:x", "1", { expirationTtl: 600 });
    expect(await kv.get("rl:email:x")).toBe("1");
    expect((await readdir(dir)).length).toBe(1);

    nowMs += 600 * 1000; // exactly at expiry — get treats >= expiresAtMs as expired
    expect(await kv.get("rl:email:x")).toBeNull();
    expect((await readdir(dir)).length).toBe(0);
  });

  it("keeps a record with a TTL that has not yet elapsed", async () => {
    let nowMs = 1_000_000;
    const kv = createFsKvStore(dir, { now: () => nowMs });
    await kv.put("k", "v", { expirationTtl: 600 });
    nowMs += 599 * 1000;
    expect(await kv.get("k")).toBe("v");
  });

  it("a record with no TTL never expires", async () => {
    let nowMs = 1_000_000;
    const kv = createFsKvStore(dir, { now: () => nowMs });
    await kv.put("k", "v");
    nowMs += 10 * 365 * 86400 * 1000;
    expect(await kv.get("k")).toBe("v");
  });

  it("delete removes the value", async () => {
    const kv = createFsKvStore(dir);
    await kv.put("k", "v");
    await kv.delete("k");
    expect(await kv.get("k")).toBeNull();
    expect((await readdir(dir)).length).toBe(0);
  });

  it("delete of an absent key is a no-op", async () => {
    const kv = createFsKvStore(dir);
    await expect(kv.delete("nope")).resolves.toBeUndefined();
  });

  it("treats a corrupt file as absent, logs, and cleans it up — never throws", async () => {
    const { log, events } = captureLog();
    const kv = createFsKvStore(dir, { log });
    await kv.put("k", "v");
    const [file] = await readdir(dir);
    expect(file).toBeDefined();
    await writeFile(join(dir, file as string), "}{ not json at all", "utf8");

    expect(await kv.get("k")).toBeNull();
    expect(events.some((e) => e.event === "fs_kv_corrupt")).toBe(true);
    expect((await readdir(dir)).length).toBe(0);
  });

  it("treats a JSON file that is not a { value: string } record as corrupt", async () => {
    const { log } = captureLog();
    const kv = createFsKvStore(dir, { log });
    await kv.put("k", "v");
    const [file] = await readdir(dir);
    await writeFile(join(dir, file as string), JSON.stringify({ value: 42 }), "utf8");
    expect(await kv.get("k")).toBeNull();
  });

  it("treats a record whose expiresAtMs is not a finite number as corrupt", async () => {
    const { log, events } = captureLog();
    const kv = createFsKvStore(dir, { log });
    await writeFile(join(dir, fileNameFor("k")), JSON.stringify({ key: "k", value: "v", expiresAtMs: "soon" }), "utf8");
    expect(await kv.get("k")).toBeNull();
    expect(events.some((e) => e.event === "fs_kv_corrupt")).toBe(true);
    expect((await readdir(dir)).length).toBe(0);
  });

  it("names files sha256(key) hex — fixed 64 chars, hex alphabet, so path traversal is unrepresentable", async () => {
    const kv = createFsKvStore(dir);
    const evil = "../../../../etc/passwd";
    await kv.put(evil, "pwned");

    const files = await readdir(dir);
    expect(files.length).toBe(1);
    const name = files[0] as string;
    expect(name).toMatch(/^[0-9a-f]{64}\.json$/);
    expect(name).toBe(fileNameFor(evil));
    expect(name).not.toContain("/");
    expect(name).not.toContain("..");

    // The value is still retrievable by the original key, and nothing was written
    // outside the dir (no file at the traversal target inside the temp root).
    expect(await kv.get(evil)).toBe("pwned");
    await expect(stat(join(dir, "..", "..", "..", "..", "etc", "passwd-should-not-exist"))).rejects.toThrow();
  });

  // W-M7's measured case: base64url(key).json.<uuid>.tmp exceeded NAME_MAX (255) at a
  // 141-char email → ENAMETOOLONG → 500 on request-code. The name no longer grows with
  // the key at all, so there is no cap to derive.
  it("stores keys of ANY length: a 254-char email, a 2 KB key and a 64 KB key all round-trip", async () => {
    const kv = createFsKvStore(dir);
    const longEmail = `${"a".repeat(64)}@${"b".repeat(63)}.${"c".repeat(63)}.${"d".repeat(57)}.com`;
    expect(longEmail.length).toBe(254);
    const keys = [`otp:${longEmail}`, `rl:email:${longEmail}:496738`, "x".repeat(2048), "y".repeat(65536)];
    for (const key of keys) {
      await kv.put(key, `value-for-${key.length}`, { expirationTtl: 600 });
      expect(await kv.get(key)).toBe(`value-for-${key.length}`);
      expect(fileNameFor(key).length).toBe(64 + ".json".length);
    }
    expect((await readdir(dir)).length).toBe(keys.length);
  });

  it("update is atomic per key: 500 concurrent increments land as exactly 500", async () => {
    const kv = createFsKvStore(dir);
    await Promise.all(
      Array.from({ length: 500 }, () =>
        kv.update("counter", (raw) => ({ value: String((raw === null ? 0 : Number(raw)) + 1) })),
      ),
    );
    expect(await kv.get("counter")).toBe("500");
  });

  it("update sees an expired record as absent and can keep or delete", async () => {
    let nowMs = 1_000_000;
    const kv = createFsKvStore(dir, { now: () => nowMs });
    await kv.put("k", "old", { expirationTtl: 60 });
    nowMs += 61_000;
    const seen: Array<string | null> = [];
    await kv.update("k", (raw) => {
      seen.push(raw);
      return "keep";
    });
    expect(seen).toEqual([null]);
    expect((await readdir(dir)).length).toBe(0);
    await kv.put("k", "v");
    await kv.update("k", () => "delete");
    expect(await kv.get("k")).toBeNull();
  });

  it("serializes put/get/delete with update on the same key, never across keys", async () => {
    const kv = createFsKvStore(dir);
    const order: string[] = [];
    const slow = kv.update("a", (raw) => {
      order.push(`update:${raw}`);
      return { value: "from-update" };
    });
    const later = kv.put("a", "from-put").then(() => order.push("put"));
    const otherKey = kv.put("b", "independent").then(() => order.push("b"));
    await Promise.all([slow, later, otherKey]);
    expect(order.indexOf("update:null")).toBeLessThan(order.indexOf("put"));
    expect(await kv.get("a")).toBe("from-put");
    expect(await kv.get("b")).toBe("independent");
  });

  it("writes the on-disk record as JSON with the plaintext key, the value and an expiry", async () => {
    const kv = createFsKvStore(dir, { now: () => 5_000 });
    await kv.put("k", "v", { expirationTtl: 600 });
    const [file] = await readdir(dir);
    const raw = await readFile(join(dir, file as string), "utf8");
    expect(JSON.parse(raw)).toEqual({ key: "k", value: "v", expiresAtMs: 5_000 + 600 * 1000 });
  });

  it("treats a record whose embedded key is not the looked-up key as corrupt (a misplaced file)", async () => {
    const { log, events } = captureLog();
    const kv = createFsKvStore(dir, { log });
    await writeFile(join(dir, fileNameFor("k")), JSON.stringify({ key: "other", value: "v" }), "utf8");
    expect(await kv.get("k")).toBeNull();
    expect(events.some((e) => e.event === "fs_kv_corrupt")).toBe(true);
    expect((await readdir(dir)).length).toBe(0);
  });
});

describe("createFsKvStore.sweepExpired", () => {
  it("removes exactly the records whose expiresAtMs has passed; live and no-TTL records stay", async () => {
    let nowMs = 1_000_000;
    const clock = (): number => nowMs;
    const kv = createFsKvStore(dir, { now: clock });
    expect(await kv.sweepExpired(clock)).toBe(0);

    await kv.put("rl:ip:203.0.113.9:277", "3", { expirationTtl: 7200 }); // expires 8_200_000
    await kv.put("rl:email:old@example.com:277", "1", { expirationTtl: 7200 }); // expires 8_200_000
    await kv.put("otp:old@example.com", '{"hash":"h","attempts":0}', { expirationTtl: 600 }); // expires 1_600_000
    nowMs = 5_000_000;
    await kv.put("rl:email:recent@example.com:278", "1", { expirationTtl: 7200 }); // expires 12_200_000
    await kv.put("nodesecret:someone@example.com", "secret"); // no TTL: never a candidate
    await writeFile(join(dir, "not-a-record.txt"), "ignored", "utf8");
    await writeFile(join(dir, `${fileNameFor("k")}.00000000-0000-0000-0000-000000000000.tmp`), "{", "utf8");
    const recordFiles = async (): Promise<string[]> => (await readdir(dir)).filter((name) => name.endsWith(".json")).sort();

    nowMs = 1_599_999;
    expect(await kv.sweepExpired(clock)).toBe(0);
    expect((await recordFiles()).length).toBe(5);

    nowMs = 1_600_000; // exactly at the OTP expiry — >= is expired, the same predicate get uses
    expect(await kv.sweepExpired(clock)).toBe(1);
    expect(await recordFiles()).not.toContain(fileNameFor("otp:old@example.com"));

    nowMs = 8_200_000;
    expect(await kv.sweepExpired(clock)).toBe(2);
    expect(await recordFiles()).toEqual(
      [fileNameFor("rl:email:recent@example.com:278"), fileNameFor("nodesecret:someone@example.com")].sort(),
    );
    expect(await kv.sweepExpired(clock)).toBe(0);

    nowMs += 100 * 365 * 86400 * 1000;
    expect(await kv.sweepExpired(clock)).toBe(1);
    expect(await recordFiles()).toEqual([fileNameFor("nodesecret:someone@example.com")]);
    expect(await kv.get("nodesecret:someone@example.com")).toBe("secret");
    expect((await readdir(dir)).length).toBe(3); // the secret, the foreign file, the stray tmp
  });

  it("a concurrent update on an expired key wins: the refreshed record is kept and not counted", async () => {
    let nowMs = 1_000_000;
    const kv = createFsKvStore(dir, { now: () => nowMs });
    const key = "rl:email:racer@example.com:277";
    await kv.put(key, "3", { expirationTtl: 60 });
    await kv.put("nodesecret:racer@example.com", "secret");
    nowMs += 61_000;

    // The sweep consults `now` once per record that can expire, and `key` holds the only
    // one, so the first call IS the scan's verdict on it. The update enqueued from inside
    // that call lands on the key's chain BEFORE the sweep's own removal step, which pins
    // the interleaving under test: the scan saw an expired record, the key was refreshed,
    // then the removal step ran — and must find the refreshed record and keep it.
    const order: string[] = [];
    let refresh: Promise<void> | undefined;
    const clock = (): number => {
      order.push("clock");
      refresh ??= kv.update(key, () => {
        order.push("mutate");
        return { value: "fresh", expirationTtl: 60 };
      });
      return nowMs;
    };

    const removed = await kv.sweepExpired(clock);
    await refresh;
    expect(removed).toBe(0);
    expect(order).toEqual(["clock", "mutate", "clock"]);
    expect(await kv.get(key)).toBe("fresh");
    const raw = JSON.parse(await readFile(join(dir, fileNameFor(key)), "utf8")) as { expiresAtMs: number };
    expect(raw.expiresAtMs).toBe(nowMs + 60_000);
    expect(await kv.get("nodesecret:racer@example.com")).toBe("secret");
  });

  it("a concurrent update that keeps an expired key is not double-counted: the update's own read removed the file", async () => {
    let nowMs = 1_000_000;
    const kv = createFsKvStore(dir, { now: () => nowMs });
    const key = "otp:keeper@example.com";
    await kv.put(key, "code", { expirationTtl: 600 });
    nowMs += 600_000;
    let keep: Promise<void> | undefined;
    const clock = (): number => {
      keep ??= kv.update(key, () => "keep");
      return nowMs;
    };
    expect(await kv.sweepExpired(clock)).toBe(0);
    await keep;
    expect((await readdir(dir)).length).toBe(0);
  });

  it("reports corrupt and misplaced files and leaves them for get to clean up", async () => {
    const { log, events } = captureLog();
    const kv = createFsKvStore(dir, { log });
    await writeFile(join(dir, fileNameFor("junk")), "}{ not json", "utf8");
    await writeFile(join(dir, fileNameFor("k")), JSON.stringify({ key: "other", value: "v", expiresAtMs: 1 }), "utf8");
    await kv.put("rl:ip:198.51.100.7:1", "1", { expirationTtl: 1 });

    expect(await kv.sweepExpired(() => Number.MAX_SAFE_INTEGER)).toBe(1);
    expect(events.filter((e) => e.event === "fs_kv_corrupt").map((e) => e.fields?.file).sort()).toEqual(
      [fileNameFor("junk"), fileNameFor("k")].sort(),
    );
    expect((await readdir(dir)).sort()).toEqual([fileNameFor("junk"), fileNameFor("k")].sort());
  });
});
