import type { Deps } from "../lib/deps";
import { usernameForEmail } from "../lib/headscale";
import { RESEND_API_BASE } from "../lib/resend";
import { sanityMutateUrl } from "../lib/siteEvent";
import { MeshProvider } from "./mesh-provider";
import { fail, object } from "./registry";

function providerId(value: unknown): string {
  if (typeof value !== "string" || !/^[1-9][0-9]*$/.test(value)) fail(502, "provider_invalid", "Provider identity is missing");
  return value;
}

export async function deleteAccountMesh(deps: Deps, email: string): Promise<void> {
  const provider = new MeshProvider(deps);
  await provider.assertIsolation();
  const name = await usernameForEmail(email);
  const listing = await provider.request("/api/v1/user?name=" + encodeURIComponent(name));
  if (!Array.isArray(listing.users)) fail(502, "provider_invalid", "Provider user list is missing");
  const found = listing.users.map(object).filter(u => u.name === name);
  if (found.length > 1) fail(502, "provider_ambiguous", "Provider account is ambiguous");
  if (found.length === 0) return;
  const userId = providerId(found[0]!.id);
  const keys = await provider.request("/api/v1/preauthkey");
  if (!Array.isArray(keys.preAuthKeys)) fail(502, "provider_invalid", "Provider key list is missing");
  for (const raw of keys.preAuthKeys) {
    const key = object(raw); const user = key.user == null ? null : object(key.user);
    if (user?.id !== userId && user?.name !== name) continue;
    if (user?.id !== userId || user.name !== name) fail(502, "provider_account_mismatch", "Provider key account disagrees");
    await provider.expireKey(providerId(key.id));
  }
  const snapshot = await provider.nodes(email);
  if (snapshot.user.id !== userId) fail(502, "provider_account_mismatch", "Provider account changed");
  for (const node of snapshot.nodes) await provider.deleteNode(node.id);
  // v0.29.3 deletes all the user's preauth keys, then hard-deletes the empty user.
  // https://github.com/juanfont/headscale/blob/v0.29.3/hscontrol/db/users.go#L45-L78
  // https://github.com/juanfont/headscale/blob/v0.29.3/proto/headscale/v1/headscale.proto
  await provider.request("/api/v1/user/" + userId, "DELETE", undefined, true);
  const after = await provider.request("/api/v1/user?name=" + encodeURIComponent(name));
  if (!Array.isArray(after.users) || after.users.some(u => object(u).name === name)) fail(502, "provider_cleanup_incomplete", "Provider account still exists");
}

async function external(deps: Deps, url: string, init: RequestInit, absentOk = false): Promise<Response> {
  const response = await deps.fetchFn(url, { ...init, redirect: "manual" });
  if (!response.ok && !(absentOk && response.status === 404)) throw new Error("account data cleanup failed");
  return response;
}

export async function deleteLinkContact(deps: Deps, email: string): Promise<string[]> {
  const { resendApiKey, resendAudienceId } = deps.config;
  if (!resendAudienceId) return [];
  if (!resendApiKey) throw new Error("contact cleanup credentials missing");
  const lookup = `${RESEND_API_BASE}/contacts/${encodeURIComponent(email)}`;
  const headers = { Authorization: `Bearer ${resendApiKey}` };
  const found = await external(deps, lookup, { headers }, true);
  if (found.status === 404) return [];
  const identity = object(await found.json());
  if (identity.email !== email || typeof identity.id !== "string" || !/^[0-9a-f-]{36}$/i.test(identity.id)) throw new Error("contact identity mismatch");
  // Immutable provider IDs fence a resumed cleanup from a newly-created contact.
  const contact = `${RESEND_API_BASE}/contacts/${identity.id}`;
  // Other services can add a newsletter membership at any time. No second read
  // proves exclusive ownership, so this worker never deletes the shared contact.
  // https://resend.com/docs/api-reference/contacts/delete-contact-segment
  const response = await external(deps, contact + "/segments/" + encodeURIComponent(resendAudienceId), { method: "DELETE", headers }, true);
  if (response.status !== 404 && object(await response.json()).deleted !== true) throw new Error("contact membership removal was not confirmed");
  return ["shared_contact_record"];
}

export async function deleteLinkEvents(deps: Deps, email: string): Promise<void> {
  const sink = deps.config.siteEvents;
  if (!sink) return;
  // Exact account AND product filter; other website activity remains independent.
  // Batches obey Sanity's 10,000-document mutation limit; sync visibility lets a
  // subsequent count prove removal. https://www.sanity.io/docs/http-reference/mutation
  const query = '*[_type == "siteEvent" && source == "leanzero-link" && email == $email]';
  const base = sanityMutateUrl(sink.projectId, sink.dataset);
  const headers = { Authorization: `Bearer ${sink.token}`, "Content-Type": "application/json" };
  let previous: number | null = null;
  for (;;) {
    const url = new URL(base.replace("/data/mutate/", "/data/query/"));
    url.searchParams.set("query", `count(${query})`); url.searchParams.set("$email", JSON.stringify(email));
    const count = object(await (await external(deps, url.toString(), { headers })).json()).result;
    if (typeof count !== "number" || !Number.isSafeInteger(count) || count < 0) throw new Error("event cleanup count invalid");
    if (count === 0) return;
    if (previous !== null && count >= previous) throw new Error("event cleanup made no progress");
    previous = count;
    await external(deps, base + "?visibility=sync", { method: "POST", headers, body: JSON.stringify({ mutations: [{ delete: { query: `${query}[0...10000]`, params: { email } } }] }) });
  }
}
