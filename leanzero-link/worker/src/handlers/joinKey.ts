import type { Deps } from "../lib/deps";
import { jsonResponse } from "../lib/http";
import { verifyJwt } from "../lib/jwt";
import { mintJoinKey } from "../lib/tailscale";

export async function handleJoinKey(request: Request, deps: Deps): Promise<Response> {
  const secret = deps.config.jwtSecret;
  if (!secret) {
    deps.log("config_error", { error: "LINK_JWT_SECRET is not configured" });
    return jsonResponse(500, { error: "LINK_JWT_SECRET not configured on this deployment" });
  }
  const authorization = request.headers.get("Authorization");
  const match = authorization === null ? null : /^Bearer\s+(.+)$/.exec(authorization);
  const bearer = match?.[1];
  if (bearer === undefined) {
    return jsonResponse(401, { error: "missing bearer token" });
  }
  const verified = await verifyJwt(secret, bearer, Math.floor(deps.now() / 1000));
  if (!verified.ok) {
    return jsonResponse(401, { error: "invalid token", reason: verified.reason });
  }
  if (!deps.config.tsApiToken || !deps.config.tsTailnet) {
    return jsonResponse(501, { error: "mesh keys not configured on this deployment" });
  }
  if (deps.config.tsKeyExpiryInvalid) {
    deps.log("config_error", { error: "TS_KEY_EXPIRY_SECONDS is not a positive integer" });
    return jsonResponse(500, { error: "TS_KEY_EXPIRY_SECONDS is not a positive integer" });
  }
  const minted = await mintJoinKey(deps, verified.claims.sub);
  if (!minted.ok) {
    return jsonResponse(502, { error: "tailscale key mint failed", status: minted.status });
  }
  return jsonResponse(200, { authKey: minted.authKey, expirySeconds: minted.expirySeconds });
}
