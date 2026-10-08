import type { Deps } from "./deps";
import { safeText, truncate } from "./http";

// Operator activity record (optional, off unless SITE_EVENTS_SANITY_* is set). The
// LeanZero-run deployment notes each sign-in in the same private Sanity record that
// leanzero.net writes its visitor events to (`_type: "siteEvent"`), and a daily digest
// mails the day's events to the operator. A self-hoster leaves the env unset and nothing
// is sent anywhere.
//
// Mutate API — https://www.sanity.io/docs/http-mutations
//   POST https://<projectId>.api.sanity.io/v2025-01-01/data/mutate/<dataset>
//   Authorization: Bearer <token>; body { mutations: [{ create: { _id, _type, ... } }] }
// The id contains a period (`siteEvent.<uuid>`): Sanity serves dotted ids to token
// holders only, so the record stays private even on a public dataset.

export const SANITY_API_VERSION = "v2025-01-01";

export type LinkEventKind = "link-signup" | "link-signin";

export function sanityMutateUrl(projectId: string, dataset: string): string {
  return `https://${encodeURIComponent(projectId)}.api.sanity.io/${SANITY_API_VERSION}/data/mutate/${encodeURIComponent(dataset)}`;
}

/// Never throws and never rejects: an unreachable record must not fail a sign-in.
export async function recordSiteEvent(
  deps: Deps,
  event: { kind: LinkEventKind; email: string; ids?: Record<string, string> },
): Promise<void> {
  const sink = deps.config.siteEvents;
  if (sink === undefined) {
    return;
  }
  const doc = {
    _id: `siteEvent.${crypto.randomUUID()}`,
    _type: "siteEvent",
    kind: event.kind,
    at: new Date(deps.now()).toISOString(),
    source: "leanzero-link",
    email: event.email,
    name: null,
    product: "leanzero-link",
    page: null,
    country: null,
    summary: null,
    ids: event.ids ?? {},
  };
  try {
    const response = await deps.fetchFn(sanityMutateUrl(sink.projectId, sink.dataset), {
      method: "POST",
      headers: { Authorization: `Bearer ${sink.token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ mutations: [{ create: doc }] }),
    });
    if (!response.ok) {
      deps.log("site_event_failed", { kind: event.kind, status: response.status, detail: truncate(await safeText(response)) });
      return;
    }
    deps.log("site_event_recorded", { kind: event.kind });
  } catch (error) {
    deps.log("site_event_failed", { kind: event.kind, error: String(error) });
  }
}
