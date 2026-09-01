import { describe, expect, it } from "vitest";
import { handleHealth } from "../src/handlers/health";
import { handleRequest } from "../src/router";
import { FULL_ENV, makeHarness, responseJson } from "./helpers";

describe("GET /v1/health", () => {
  it("reports every capability true on a fully configured deployment", async () => {
    const h = makeHarness();
    const body = await responseJson(handleHealth(h.deps));
    expect(body).toEqual({
      ok: true,
      version: "0.1.0",
      capabilities: { mail: true, audience: true, mesh: true },
    });
  });

  it("reports ok false and every capability false on an empty deployment", async () => {
    const h = makeHarness({ env: {} });
    const body = await responseJson(handleHealth(h.deps));
    expect(body).toEqual({
      ok: false,
      version: "0.1.0",
      capabilities: { mail: false, audience: false, mesh: false },
    });
  });

  it("requires both the Resend key and the From address for mail", async () => {
    const h = makeHarness({ env: { LINK_JWT_SECRET: "s", RESEND_API_KEY: "re_x" } });
    const body = await responseJson(handleHealth(h.deps));
    expect(body.capabilities).toEqual({ mail: false, audience: false, mesh: false });
  });

  it("reports audience independently of mail", async () => {
    const h = makeHarness({ env: { LINK_JWT_SECRET: "s", RESEND_API_KEY: "re_x", RESEND_AUDIENCE_ID: "seg_1" } });
    const body = await responseJson(handleHealth(h.deps));
    expect(body.capabilities).toEqual({ mail: false, audience: true, mesh: false });
  });

  it("requires both the Tailscale token and tailnet for mesh", async () => {
    const h = makeHarness({ env: { ...FULL_ENV, TS_TAILNET: undefined } });
    const body = await responseJson(handleHealth(h.deps));
    expect(body.capabilities).toEqual({ mail: true, audience: true, mesh: false });
  });

  it("is served on GET /v1/health through the router", async () => {
    const h = makeHarness();
    const response = await handleRequest(new Request("https://link.example/v1/health"), h.deps);
    expect(response.status).toBe(200);
    expect((await responseJson(response)).ok).toBe(true);
  });
});
