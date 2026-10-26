import type { Deps } from "../lib/deps";
import { digest } from "./crypto";
import { ControllerError, Registry } from "./registry";
import { newAccount } from "./service";

/** Serializes the legacy external mint against controller admission in the same
 * authoritative account transaction. In-flight mints and their full credential
 * lifetimes prevent an old binary entering after the compatibility check. */
export async function reserveLegacyMint(deps: Deps, email: string): Promise<(() => Promise<void>) | null> {
  if (!deps.deviceRegistry) return null;
  const registry = new Registry(deps.deviceRegistry); const key = await digest(email);
  await registry.update(key, old => {
    const state = old ?? newAccount(email);
    if (Object.values(state.mesh_enrollments ?? {}).some(e => e.role === "controller" && e.status !== "revoked" && (e.status === "confirmed" || e.expires_at > deps.now()))) throw new ControllerError(409, "update_required", "Controller-enabled accounts require scoped host enrollment");
    const mints = state.legacy_mesh_mints ??= { in_flight: 0, expires_at: 0 };
    mints.in_flight += 1;
    return { state, result: undefined };
  });
  return () => registry.update(key, state => {
    if (!state?.legacy_mesh_mints || state.legacy_mesh_mints.in_flight < 1) throw new Error("missing legacy mint reservation");
    state.legacy_mesh_mints.in_flight -= 1;
    state.legacy_mesh_mints.expires_at = Math.max(state.legacy_mesh_mints.expires_at, deps.now() + deps.config.tsKeyExpirySeconds * 1000);
    return { state, result: undefined };
  });
}
