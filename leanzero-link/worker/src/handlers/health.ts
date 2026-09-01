import { capabilities } from "../lib/config";
import type { Deps } from "../lib/deps";
import { jsonResponse } from "../lib/http";
import { VERSION } from "../version";

export function handleHealth(deps: Deps): Response {
  return jsonResponse(200, {
    ok: Boolean(deps.config.jwtSecret),
    version: VERSION,
    capabilities: capabilities(deps.config),
  });
}
