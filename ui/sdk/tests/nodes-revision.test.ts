import assert from "node:assert/strict";
import { test } from "node:test";
import {
  zNodesConfig,
  zNodesRefusalCode,
} from "../src/generated/zod.gen.ts";

test("node config validators preserve the owner revision across desktop read/edit/write", () => {
  const read = zNodesConfig.parse({ version: 1, revision: 7 });
  const edited = { ...read, declined: ["fixture-unused-device"] };
  const written = zNodesConfig.parse(JSON.parse(JSON.stringify(edited)));
  assert.equal(written.revision, 7);
  assert.deepEqual(written.declined, ["fixture-unused-device"]);
  assert.equal(zNodesConfig.parse({ version: 1 }).revision, 0);
});

test("node refusal validators accept stale revisions without admitting unknown codes", () => {
  assert.equal(zNodesRefusalCode.parse("staleRevision"), "staleRevision");
  assert.equal(zNodesRefusalCode.safeParse("fixtureUnknownRefusal").success, false);
});
