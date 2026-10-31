import type { Deps } from "../lib/deps";
import { digest } from "./crypto";
import { MeshProvider } from "./mesh-provider";
import { ControllerError, Registry, assertAccountActive } from "./registry";
import { newAccount } from "./service";
import type { AccountState, MeshEnrollment } from "./types";

const LEGACY_RESERVATION_MS = 60_000;
export type LegacyMintReservation = (() => Promise<void>) & { beginExternal: () => Promise<void>; deps: Deps };

/** A process can die after reserving but before calling the provider. Only that
 * known-no-effect phase is reclaimable by elapsed transport lease; a provider
 * CREATE with no response remains an explicit reconciliation requirement. */
export function reapUnstartedLegacyMints(state: AccountState, now: number): void {
  const mints = state.legacy_mesh_mints;
  if (!mints?.reservations) return;
  for (const [id, reservation] of Object.entries(mints.reservations)) {
    if (!reservation.external_started && reservation.expires_at <= now) {
      delete mints.reservations[id]; mints.in_flight -= 1;
    }
  }
}

/** WR-7. Legacy (account-JWT) mesh keys and controller enrollments are mutually
 * exclusive: a phone is never on the mesh beside an untracked legacy node, and
 * legacy credentials are never outstanding while a controller can join. The old
 * gate enforced that by refusing legacy mints whenever ANY controller enrollment
 * was confirmed, even after its phone node had left the provider. That locked
 * current desktop hosts out of the mesh for good once a phone was ever paired.
 *
 * A confirmed controller is DORMANT when the provider proves it has no foothold:
 * no node (online, offline or expired) carries its binding or key, and its key
 * can no longer admit a node. A dormant phone can only come back through
 * MeshService.join with a fresh enrollment, and that path's compatible() check
 * refuses while legacy nodes or legacy key lifetimes exist (same registry CAS),
 * so the exclusion still holds. Pending/issued controller enrollments with live
 * keys still block. Any provider doubt (unreadable, other provider) keeps the
 * strict rule. Returns "enrollment_id:key_id" tokens. */
async function dormantControllers(deps: Deps, state: AccountState | null): Promise<Set<string>> {
  const dormant = new Set<string>();
  const confirmed = Object.values(state?.mesh_enrollments ?? {}).filter(e => e.role === "controller" && e.status === "confirmed");
  if (!state || confirmed.length === 0 || deps.config.meshProvider !== "headscale") return dormant;
  try {
    const provider = new MeshProvider(deps);
    const { user, nodes } = await provider.nodes(state.email);
    const admitting = await provider.admittingKeys(user.id);
    for (const e of confirmed) {
      const b = e.binding;
      if (!b || e.key_id === null || b.key_id !== e.key_id || e.user_id !== user.id || admitting.has(e.key_id)) continue;
      if (nodes.some(n => n.id === b.node_id || n.node_key === b.node_key || n.key_id === e.key_id)) continue;
      dormant.add(e.enrollment_id + ":" + e.key_id);
    }
  } catch { return new Set(); }
  return dormant;
}

function blocksLegacyMint(e: MeshEnrollment, now: number, dormant: Set<string>): boolean {
  if (e.role !== "controller" || e.status === "revoked") return false;
  if (e.status === "confirmed") return !dormant.has(e.enrollment_id + ":" + e.key_id);
  return e.expires_at > now;
}

export async function reserveLegacyMint(deps: Deps, email: string, issuedAt?: number): Promise<LegacyMintReservation | null> {
  if (!deps.deviceRegistry) return null;
  const registry = new Registry(deps.deviceRegistry); const key = await digest(email); const id = crypto.randomUUID();
  const dormant = await dormantControllers(deps, await registry.read(key));
  const accountId = await registry.update(key, old => {
    const state = old ?? newAccount(email);
    if (state.legacy_revoked_before !== undefined && (issuedAt === undefined || issuedAt <= state.legacy_revoked_before)) throw new ControllerError(401, "account_credential_revoked", "Account credential was revoked by deletion");
    reapUnstartedLegacyMints(state, deps.now());
    if (Object.values(state.mesh_enrollments ?? {}).some(e => blocksLegacyMint(e, deps.now(), dormant))) throw new ControllerError(409, "update_required", "Controller-enabled accounts require scoped host enrollment");
    const mints = state.legacy_mesh_mints ??= { in_flight: 0, expires_at: 0 };
    (mints.reservations ??= {})[id] = { expires_at: deps.now() + LEGACY_RESERVATION_MS, external_started: false };
    mints.in_flight += 1;
    return { state, result: state.account_id };
  });
  const release = async (): Promise<void> => registry.update(key, state => {
    if (!state) throw new Error("missing legacy mint account");
    const reservation = state.legacy_mesh_mints?.reservations?.[id];
    if (state.account_id !== accountId || !reservation || reservation.uncertain) return { state, result: undefined };
    delete state.legacy_mesh_mints!.reservations![id];
    state.legacy_mesh_mints!.in_flight -= 1;
    state.legacy_mesh_mints!.expires_at = Math.max(state.legacy_mesh_mints!.expires_at, deps.now() + deps.config.tsKeyExpirySeconds * 1000);
    return { state, result: undefined };
  }, true);
  const guarded: Deps = { ...deps, fetchFn: async (url, init) => {
    const path = new URL(url).pathname;
    const createsAccountData = init?.method === "POST" && ["/api/v1/user", "/api/v1/preauthkey"].includes(path);
    await registry.update(key, state => {
      assertAccountActive(state);
      const r = state?.legacy_mesh_mints?.reservations?.[id];
      if (!state || state.account_id !== accountId || !r || r.expires_at <= deps.now()) throw new ControllerError(409, "legacy_mint_expired", "Mesh request authority expired");
      r.expires_at = deps.now() + LEGACY_RESERVATION_MS;
      if (createsAccountData) r.uncertain = (r.uncertain ?? 0) + 1;
      return { state, result: undefined };
    });
    const response = await deps.fetchFn(url, { ...init, signal: AbortSignal.timeout(30_000) });
    // A network error, 5xx, or malformed success can hide a committed CREATE.
    // Keep its intent durable; finally must not erase an unknown outcome.
    let knownOutcome = !createsAccountData || (response.status >= 400 && response.status < 500);
    if (createsAccountData && response.ok) {
      const body: unknown = await response.clone().json().catch(() => null);
      if (typeof body === "object" && body !== null) {
        const item = (body as Record<string, unknown>)[path.endsWith("/user") ? "user" : "preAuthKey"];
        if (typeof item === "object" && item !== null) {
          const value = item as Record<string, unknown>;
          knownOutcome = typeof value.id === "string" && /^[1-9][0-9]*$/.test(value.id);
        }
      }
    }
    if (knownOutcome) await registry.update(key, state => {
      const r = state?.legacy_mesh_mints?.reservations?.[id];
      if (!state || state.account_id !== accountId || !r) throw new ControllerError(409, "legacy_mint_expired", "Mesh request authority expired");
      if (createsAccountData && r.uncertain) r.uncertain -= 1;
      return { state, result: undefined };
    }, true);
    return response;
  } };
  return Object.assign(release, { deps: guarded, beginExternal: () => registry.update(key, state => {
    assertAccountActive(state);
    const reservation = state?.legacy_mesh_mints?.reservations?.[id];
    if (!state || state.account_id !== accountId || !reservation || reservation.expires_at <= deps.now()) throw new ControllerError(409, "legacy_mint_expired", "Retry the interrupted mesh request");
    reservation.external_started = true;
    return { state, result: undefined };
  }) });
}
