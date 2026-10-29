import type { Deps, KVStore } from "../lib/deps";
import { digest } from "./crypto";
import { Registry, fail } from "./registry";
import { newAccount, enrollmentAccount } from "./service";

const OPERATION_LEASE_MS = 60_000;
const OPERATION_FETCH_MS = 30_000;

/** Legacy auth writes use the deletion row's lease. A suspended old process must
 * revalidate its lease before each write, including inside atomic KV mutations.
 * An abandoned external create is explicit unknown, never cleared by a timer. */
export async function reserveAccountOperation(deps: Deps, email: string, startEnrollment = false): Promise<{ deps: Deps; release: () => Promise<void> }> {
  if (!deps.deviceRegistry) return { deps, release: async () => {} };
  const registry = new Registry(deps.deviceRegistry); const key = await digest(email); const id = crypto.randomUUID();
  let expiresAt = deps.now() + OPERATION_LEASE_MS;
  const accountId = await registry.update(key, old => {
    const state = startEnrollment ? enrollmentAccount(old, email) : old ?? newAccount(email);
    (state.legacy_operations ??= {})[id] = { expires_at: expiresAt, external_pending: 0 };
    return { state, result: state.account_id };
  }, startEnrollment);
  const renew = async (externalDelta = 0): Promise<void> => registry.update(key, state => {
    const operation = state?.legacy_operations?.[id];
    if (!state || state.account_id !== accountId || !operation || operation.expires_at <= deps.now()) fail(409, "account_operation_expired", "Retry the interrupted sign-in request");
    expiresAt = deps.now() + OPERATION_LEASE_MS;
    operation.expires_at = expiresAt;
    operation.external_pending += externalDelta;
    return { state, result: undefined };
  }, true);
  const ensureLease = (): void => { if (deps.now() >= expiresAt) fail(409, "account_operation_expired", "Account write lease expired"); };
  const kv: KVStore = {
    get: async key => { await renew(); return deps.kv.get(key); },
    update: async (key, mutate) => { await renew(); await deps.kv.update(key, value => { ensureLease(); return mutate(value); }); },
    put: async (key, value, options) => { await renew(); await deps.kv.update(key, () => { ensureLease(); return { value, ...(options?.expirationTtl === undefined ? {} : { expirationTtl: options.expirationTtl }) }; }); },
    delete: async key => { await renew(); await deps.kv.update(key, () => { ensureLease(); return "delete"; }); },
  };
  const guarded: Deps = { ...deps, kv, fetchFn: async (url, init) => {
    // Mail delivery does not create an account record. Contacts and activity do.
    const createsRecord = init?.method === "POST" && (new URL(url).pathname.startsWith("/contacts") || new URL(url).pathname.includes("/data/mutate/"));
    await renew(createsRecord ? 1 : 0);
    const response = await deps.fetchFn(url, { ...init, signal: AbortSignal.timeout(OPERATION_FETCH_MS) });
    // A returned 5xx or malformed success can hide a committed write, just like
    // a lost response. Only a recognized terminal response retires its intent.
    const settled = !createsRecord || (response.status >= 400 && response.status < 500)
      || (response.ok && await confirmedCreate(url, init, response));
    await renew(createsRecord && settled ? -1 : 0);
    return response;
  } };
  return { deps: guarded, release: () => registry.update(key, state => {
    if (!state) fail(503, "account_authority_missing", "Account authority disappeared during sign-in");
    if (state.account_id !== accountId) return { state, result: undefined };
    const operation = state.legacy_operations?.[id];
    if (operation && !operation.external_pending) delete state.legacy_operations![id];
    return { state, result: undefined };
  }, true) };
}

async function confirmedCreate(url: string, init: RequestInit | undefined, response: Response): Promise<boolean> {
  const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
  const body: unknown = await response.clone().json().catch(() => null);
  if (!record(body)) return false;
  const path = new URL(url).pathname;
  if (path.startsWith("/contacts")) {
    // https://resend.com/docs/api-reference/contacts/create-contact
    // https://resend.com/docs/api-reference/contacts/add-contact-to-segment
    if (typeof body.id !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(body.id)) return false;
    return path === "/contacts" ? body.object === "contact" : path.endsWith("/segments/" + body.id);
  }
  // The current HTTP reference calls the result field documentId, while the
  // official client types call it id. Both must identify this exact create.
  // https://www.sanity.io/docs/http-reference/mutation
  // https://github.com/sanity-io/client/blob/main/src/types.ts (SingleMutationResult)
  if (typeof body.transactionId !== "string" || !body.transactionId || !Array.isArray(body.results) || body.results.length !== 1) return false;
  let request: unknown;
  try { request = JSON.parse(typeof init?.body === "string" ? init.body : ""); } catch { return false; }
  if (!record(request) || !Array.isArray(request.mutations) || request.mutations.length !== 1) return false;
  const mutation: unknown = request.mutations[0], result: unknown = body.results[0];
  if (!record(mutation) || !record(mutation.create) || typeof mutation.create._id !== "string" || !mutation.create._id || !record(result) || result.operation !== "create") return false;
  return (result.id === mutation.create._id || result.documentId === mutation.create._id)
    && (result.id === undefined || result.id === mutation.create._id)
    && (result.documentId === undefined || result.documentId === mutation.create._id);
}
