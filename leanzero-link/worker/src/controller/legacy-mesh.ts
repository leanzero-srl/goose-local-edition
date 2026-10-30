import type { Deps } from "../lib/deps";
import { digest } from "./crypto";
import { ControllerError, Registry, assertAccountActive } from "./registry";
import { newAccount } from "./service";
import type { AccountState } from "./types";

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

export async function reserveLegacyMint(deps: Deps, email: string, issuedAt?: number): Promise<LegacyMintReservation | null> {
  if (!deps.deviceRegistry) return null;
  const registry = new Registry(deps.deviceRegistry); const key = await digest(email); const id = crypto.randomUUID();
  const accountId = await registry.update(key, old => {
    const state = old ?? newAccount(email);
    if (state.legacy_revoked_before !== undefined && (issuedAt === undefined || issuedAt <= state.legacy_revoked_before)) throw new ControllerError(401, "account_credential_revoked", "Account credential was revoked by deletion");
    reapUnstartedLegacyMints(state, deps.now());
    if (Object.values(state.mesh_enrollments ?? {}).some(e => e.role === "controller" && e.status !== "revoked" && (e.status === "confirmed" || e.expires_at > deps.now()))) throw new ControllerError(409, "update_required", "Controller-enabled accounts require scoped host enrollment");
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
