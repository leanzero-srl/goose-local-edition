import { describe, expect, it } from "vitest";
import { handleRequest } from "../src/router";
import { FULL_ENV, makeHarness, postJson, responseJson } from "./helpers";

const ORIGIN_ENV = { ...FULL_ENV, ALLOWED_ORIGINS: "https://app.example, https://desktop.example" };

describe("router + CORS", () => {
  it("answers preflight with 204 and wildcard CORS by default", async () => {
    const h = makeHarness();
    const response = await handleRequest(
      new Request("https://link.example/v1/auth/verify", { method: "OPTIONS", headers: { Origin: "https://anywhere.example" } }),
      h.deps,
    );
    expect(response.status).toBe(204);
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe("*");
    expect(response.headers.get("Access-Control-Allow-Methods")).toBe("GET, POST, OPTIONS");
    expect(response.headers.get("Access-Control-Allow-Headers")).toBe("Content-Type, Authorization");
    expect(response.headers.get("Access-Control-Max-Age")).toBe("86400");
  });

  it("echoes a listed origin and varies on Origin", async () => {
    const h = makeHarness({ env: ORIGIN_ENV });
    const response = await handleRequest(
      new Request("https://link.example/v1/health", { headers: { Origin: "https://app.example" } }),
      h.deps,
    );
    expect(response.status).toBe(200);
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe("https://app.example");
    expect(response.headers.get("Vary")).toBe("Origin");
  });

  it("sends no CORS header to an unlisted origin", async () => {
    const h = makeHarness({ env: ORIGIN_ENV });
    const response = await handleRequest(
      new Request("https://link.example/v1/health", { headers: { Origin: "https://evil.example" } }),
      h.deps,
    );
    expect(response.status).toBe(200);
    expect(response.headers.get("Access-Control-Allow-Origin")).toBeNull();
  });

  it("attaches CORS headers to error responses too", async () => {
    const h = makeHarness();
    const response = await handleRequest(
      new Request("https://link.example/v1/nope", { headers: { Origin: "https://anywhere.example" } }),
      h.deps,
    );
    expect(response.status).toBe(404);
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe("*");
    expect(await responseJson(response)).toEqual({ error: "not found" });
  });

  it("rejects a method mismatch with 405 and an Allow header", async () => {
    const h = makeHarness();
    const response = await handleRequest(new Request("https://link.example/v1/auth/verify"), h.deps);
    expect(response.status).toBe(405);
    expect(response.headers.get("Allow")).toBe("POST, OPTIONS");
  });

  it("converts an unhandled handler crash into a logged 500", async () => {
    const h = makeHarness();
    const unbound = async (): Promise<never> => {
      throw new Error("KV binding LINK_KV is not configured");
    };
    h.deps.kv.get = unbound;
    h.deps.kv.update = unbound;
    const response = await handleRequest(
      postJson("/v1/auth/request-code", { email: "user@example.com" }, { "CF-Connecting-IP": "203.0.113.7" }),
      h.deps,
    );
    expect(response.status).toBe(500);
    expect(await responseJson(response)).toEqual({ error: "internal error" });
    expect(h.logs.some((l) => l.event === "unhandled_error")).toBe(true);
  });
});
