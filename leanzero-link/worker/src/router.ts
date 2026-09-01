import { handleHealth } from "./handlers/health";
import { handleJoinKey } from "./handlers/joinKey";
import { handleRequestCode } from "./handlers/requestCode";
import { handleVerify } from "./handlers/verify";
import { preflightResponse, withCorsHeaders } from "./lib/cors";
import type { Deps } from "./lib/deps";
import { jsonResponse } from "./lib/http";

interface Route {
  method: string;
  handler: (request: Request, deps: Deps) => Promise<Response> | Response;
}

const ROUTES: Record<string, Route> = {
  "/v1/auth/request-code": { method: "POST", handler: handleRequestCode },
  "/v1/auth/verify": { method: "POST", handler: handleVerify },
  "/v1/mesh/join-key": { method: "POST", handler: handleJoinKey },
  "/v1/health": { method: "GET", handler: (_request, deps) => handleHealth(deps) },
};

export async function handleRequest(request: Request, deps: Deps): Promise<Response> {
  const origin = request.headers.get("Origin");
  const allowed = deps.config.allowedOrigins;
  if (request.method === "OPTIONS") {
    return preflightResponse(origin, allowed);
  }
  const url = new URL(request.url);
  const route = ROUTES[url.pathname];
  let response: Response;
  if (route === undefined) {
    response = jsonResponse(404, { error: "not found" });
  } else if (request.method !== route.method) {
    response = jsonResponse(405, { error: "method not allowed" }, { Allow: `${route.method}, OPTIONS` });
  } else {
    try {
      response = await route.handler(request, deps);
    } catch (error) {
      deps.log("unhandled_error", {
        path: url.pathname,
        error: error instanceof Error ? `${error.message}\n${error.stack ?? ""}` : String(error),
      });
      response = jsonResponse(500, { error: "internal error" });
    }
  }
  return withCorsHeaders(response, origin, allowed);
}
