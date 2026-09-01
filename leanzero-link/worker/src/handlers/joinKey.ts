import type { Deps } from "../lib/deps";
import { mintHeadscaleJoinKey } from "../lib/headscale";
import { jsonResponse } from "../lib/http";
import { verifyJwt } from "../lib/jwt";
import { ensureNodeSecret } from "../lib/nodeSecret";
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
  if (deps.config.meshConfigError !== undefined) {
    deps.log("config_error", { error: "mesh_provider_partial_config", detail: deps.config.meshConfigError });
    return jsonResponse(500, { error: deps.config.meshConfigError });
  }
  if (deps.config.meshProvider === "none") {
    return jsonResponse(501, { error: "mesh keys not configured on this deployment" });
  }
  if (deps.config.tsKeyExpiryInvalid) {
    deps.log("config_error", { error: "TS_KEY_EXPIRY_SECONDS is not a positive integer" });
    return jsonResponse(500, { error: "TS_KEY_EXPIRY_SECONDS is not a positive integer" });
  }

  const email = verified.claims.sub;
  if (deps.config.meshProvider === "headscale") {
    const minted = await mintHeadscaleJoinKey(deps, email);
    if (!minted.ok) {
      return jsonResponse(502, { error: "headscale key mint failed", status: minted.status });
    }
    const nodeSecret = await ensureNodeSecret(deps.kv, email, deps.log);
    return jsonResponse(200, {
      authKey: minted.authKey,
      loginServer: minted.loginServer,
      expirySeconds: minted.expirySeconds,
      nodeSecret,
    });
  }

  const minted = await mintJoinKey(deps, email);
  if (!minted.ok) {
    return jsonResponse(502, { error: "tailscale key mint failed", status: minted.status });
  }
  const nodeSecret = await ensureNodeSecret(deps.kv, email, deps.log);
  return jsonResponse(200, { authKey: minted.authKey, expirySeconds: minted.expirySeconds, nodeSecret });
}
