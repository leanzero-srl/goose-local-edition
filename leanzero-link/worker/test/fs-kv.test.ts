import { mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createFsKvStore } from "../src/lib/fs-kv";

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

  it("encodes keys to a safe base64url filename so path traversal is unrepresentable", async () => {
    const kv = createFsKvStore(dir);
    const evil = "../../../../etc/passwd";
    await kv.put(evil, "pwned");

    const files = await readdir(dir);
    expect(files.length).toBe(1);
    const name = files[0] as string;
    // base64url alphabet only: A-Z a-z 0-9 - _  (plus the .json suffix). No slashes,
    // no dots-as-parent, so nothing can escape the dir.
    expect(name).toMatch(/^[A-Za-z0-9_-]+\.json$/);
    expect(name).not.toContain("/");
    expect(name).not.toContain("..");

    // The value is still retrievable by the original key, and nothing was written
    // outside the dir (no file at the traversal target inside the temp root).
    expect(await kv.get(evil)).toBe("pwned");
    await expect(stat(join(dir, "..", "..", "..", "..", "etc", "passwd-should-not-exist"))).rejects.toThrow();
  });

  it("writes the on-disk record as JSON with the value and an expiry", async () => {
    const kv = createFsKvStore(dir, { now: () => 5_000 });
    await kv.put("k", "v", { expirationTtl: 600 });
    const [file] = await readdir(dir);
    const raw = await readFile(join(dir, file as string), "utf8");
    expect(JSON.parse(raw)).toEqual({ value: "v", expiresAtMs: 5_000 + 600 * 1000 });
  });
});
