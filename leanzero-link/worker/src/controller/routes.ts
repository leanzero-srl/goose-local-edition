import type { Deps } from "../lib/deps";
import { EMAIL_RATE_LIMIT, IP_RATE_LIMIT, VERIFY_RATE_LIMIT, RATE_WINDOW_SECONDS } from "../lib/config";
import { resolveClientIp } from "../lib/clientIp";
import { jsonResponse, readJsonBody } from "../lib/http";
import { bumpFixedWindow } from "../lib/ratelimit";
import { sendOtpEmail } from "../lib/resend";
import { normalizeCode, normalizeEmail } from "../lib/validate";
import { ACCOUNT_KEY, HOST_LEASE_SECONDS, UUID } from "./crypto";
import { ControllerError, Registry, fail, integer, object, string, uuid } from "./registry";
import { ControllerService } from "./service";
import { HostService } from "./hosts";
import type { Platform } from "./types";

function bearer(request: Request): string {
  const match = /^Bearer ([^\s]+)$/.exec(request.headers.get("Authorization") ?? "");
  return match?.[1] ?? fail(401, "invalid_credential", "Bearer credential required");
}
async function fields(request: Request): Promise<Record<string, unknown>> {
  const body = await readJsonBody(request);
  if (!body.ok) fail(400, "invalid_request", "Invalid JSON body");
  return object(body.value);
}
async function rate(deps: Deps, key: string, limit: number): Promise<void> {
  if ((await bumpFixedWindow(deps.kv, key, limit, RATE_WINDOW_SECONDS, deps.now())).limited) fail(429, "rate_limited", "Too many attempts");
}
function name(value: unknown): string {
  const text = string(value, "name").trim();
  if (!text || text.length > 128 || /[\u0000-\u001f]/.test(text)) fail(400, "invalid_request", "Invalid display name");
  return text;
}
function requireMethod(request: Request, method: string): void {
  if (request.method !== method) fail(405, "method_not_allowed", `Use ${method}`);
}

export async function handleController(request: Request, deps: Deps): Promise<Response> {
  try {
    if (request.headers.has("Origin")) fail(403, "origin_denied", "Native controller requests cannot carry Origin");
    if (!deps.deviceRegistry) fail(503, "registry_unconfigured", "Controller registry is not configured");
    if (!deps.config.jwtSecret) fail(503, "authority_unconfigured", "Controller signing key is not configured");
    const service = new ControllerService(new Registry(deps.deviceRegistry), deps.config.jwtSecret, deps.now);
    const hosts = new HostService(service);
    const path = new URL(request.url).pathname.slice("/v1/controller".length);
    let result: unknown;
    switch (path) {
      case "/auth/request-code": {
        requireMethod(request, "POST");
        const f = await fields(request); const email = normalizeEmail(f.email);
        if (!email) fail(400, "invalid_request", "Invalid email");
        const enrollment = uuid(f.enrollment_id, "enrollment_id");
        if (!deps.config.resendApiKey || !deps.config.mailFrom) fail(503, "mail_unconfigured", "Mail is not configured");
        const ip = resolveClientIp(request);
        if (ip.ok) await rate(deps, `rl:ip:${ip.ip}`, IP_RATE_LIMIT);
        await rate(deps, `rl:email:${email}`, EMAIL_RATE_LIMIT);
        const code = deps.randomOtp();
        const challenge = await service.prepareChallenge(email, enrollment, code);
        if (challenge.send) {
          const sent = await sendOtpEmail(deps, email, code);
          await service.challengeSent(challenge.challenge_id, enrollment, sent.ok);
          if (!sent.ok) fail(502, "mail_failed", "Could not deliver the code");
        }
        result = { challenge_id: challenge.challenge_id, expires_at: challenge.expires_at }; break;
      }
      case "/auth/verify": {
        requireMethod(request, "POST");
        const f = await fields(request); const challenge = string(f.challenge_id, "challenge_id");
        const [key, challengeUuid, extra] = challenge.split(".");
        if (!ACCOUNT_KEY.test(key ?? "") || !UUID.test(challengeUuid ?? "") || extra !== undefined) fail(400, "invalid_request", "Invalid challenge handle");
        const state = await service.registry.read(key!);
        if (state) await rate(deps, `rl:verify:${state.email}`, VERIFY_RATE_LIMIT);
        const code = normalizeCode(f.code);
        if (!code || !["ios", "ipados", "desktop"].includes(f.platform as string) || f.role !== undefined) fail(400, "invalid_request", "Invalid enrollment fields");
        result = await service.verify(challenge, uuid(f.enrollment_id, "enrollment_id"), code, name(f.device_name), f.platform as Platform); break;
      }
      case "/auth/refresh": {
        requireMethod(request, "POST"); const f = await fields(request);
        result = await service.refresh(uuid(f.device_id, "device_id"), string(f.refresh_token, "refresh_token"), uuid(f.request_id, "request_id")); break;
      }
      case "/devices": {
        requireMethod(request, "GET");
        const auth = await service.authenticate(bearer(request), "devices.read");
        result = { revision: auth.revision, devices: Object.values(auth.state.devices).map(d => d.record) }; break;
      }
      case "/hosts/register": {
        requireMethod(request, "POST"); const f = await fields(request);
        if (f.protocol_version !== 1) fail(400, "invalid_request", "Unsupported host protocol");
        const pin = f.tls_spki_sha256 === undefined || f.tls_spki_sha256 === null ? null : string(f.tls_spki_sha256, "tls_spki_sha256");
        if (pin !== null && !ACCOUNT_KEY.test(pin)) fail(400, "invalid_request", "TLS SPKI pin must be SHA-256 hex");
        result = await hosts.register(bearer(request), request.headers.get("X-Link-Host-Credential"), { owner_node_id: uuid(f.owner_node_id, "owner_node_id"), registration_id: uuid(f.registration_id, "registration_id"), host_name: name(f.host_name), protocol_version: 1, tls_spki_sha256: pin }); break;
      }
      case "/hosts/lease": {
        requireMethod(request, "POST"); const f = await fields(request);
        result = await hosts.lease(bearer(request), { owner_node_id: uuid(f.owner_node_id, "owner_node_id"), runtime_id: uuid(f.runtime_id, "runtime_id"), store_generation: uuid(f.store_generation, "store_generation"), catalog_revision: integer(f.catalog_revision, "catalog_revision"), mesh_node_key: string(f.mesh_node_key, "mesh_node_key") }); break;
      }
      case "/introspect": {
        requireMethod(request, "POST"); const f = await fields(request);
        result = await hosts.introspect(bearer(request), string(f.token, "token"), uuid(f.owner_node_id, "owner_node_id"), string(f.required_scope, "required_scope"), uuid(f.runtime_id, "runtime_id"), uuid(f.host_lease_id, "host_lease_id")); break;
      }
      case "/hosts": {
        requireMethod(request, "GET"); const auth = await service.authenticate(bearer(request), "catalog.read");
        result = { revision: auth.revision, hosts: Object.values(auth.state.hosts).map(h => ({ owner_node_id: h.owner_node_id, name: h.host_name, device_role: "host", protocol_version: h.protocol_version, availability: h.lease && h.lease.expires_at > deps.now() ? "update_required" : "offline", registration_state: "awaiting_mesh_binding", last_seen_at: h.lease ? new Date(h.lease.expires_at - HOST_LEASE_SECONDS * 1000).toISOString() : null, tls_spki_sha256: h.tls_spki_sha256, mesh_node_key: h.lease?.mesh_node_key ?? null, endpoints: [], capabilities: [] })) }; break;
      }
      default: {
        const match = /^\/devices\/([^/]+)\/revoke$/.exec(path);
        if (!match) fail(404, "not_found", "Unknown controller route");
        requireMethod(request, "POST"); const f = await fields(request);
        result = await service.revoke(bearer(request), uuid(match[1], "device_id"), uuid(f.request_id, "request_id"), integer(f.expected_revision, "expected_revision"));
      }
    }
    return jsonResponse(200, result, { "Cache-Control": "no-store" });
  } catch (error) {
    if (error instanceof ControllerError) return jsonResponse(error.status, { error: { code: error.code, message: error.message, retryable: error.status === 429 || error.status === 503 } }, { "Cache-Control": "no-store" });
    // No tokens, payload, driver SQL or secret-bearing exception text in diagnostics.
    deps.log("controller_registry_error", { path: new URL(request.url).pathname });
    return jsonResponse(503, { error: { code: "registry_unavailable", message: "Controller authority is unavailable", retryable: true } }, { "Cache-Control": "no-store" });
  }
}
