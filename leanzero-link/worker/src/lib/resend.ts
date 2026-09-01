import type { Deps } from "./deps";
import { safeText, truncate } from "./http";

export const RESEND_API_BASE = "https://api.resend.com";

export type SendMailResult = { ok: true } | { ok: false; status: number; detail: string };

// Send Email API — verified against https://resend.com/docs/api-reference/emails/send-email
//   POST https://api.resend.com/emails
//   Headers: Authorization: Bearer re_xxx, Content-Type: application/json
//   Body: from (required; may be `Name <email@example.com>`), to (string | string[], required),
//         subject (required), html (optional), text (optional)
//   Success response: { "id": "<email id>" }
export async function sendOtpEmail(deps: Deps, email: string, code: string): Promise<SendMailResult> {
  const { resendApiKey, mailFrom } = deps.config;
  if (!resendApiKey || !mailFrom) {
    return { ok: false, status: 0, detail: "mail not configured" };
  }
  const body = {
    from: mailFrom,
    to: email,
    subject: `${code} is your LeanZero Link sign-in code`,
    text:
      `Your LeanZero Link sign-in code is ${code}.\n\n` +
      `It expires in 10 minutes and can be used once.\n` +
      `If you didn't request this code, you can ignore this email.`,
    html:
      `<p>Your LeanZero Link sign-in code is:</p>` +
      `<p style="font-size:28px;font-weight:700;letter-spacing:6px;font-family:monospace">${code}</p>` +
      `<p>It expires in 10 minutes and can be used once.<br>` +
      `If you didn't request this code, you can ignore this email.</p>`,
  };
  try {
    const response = await deps.fetchFn(`${RESEND_API_BASE}/emails`, {
      method: "POST",
      headers: { Authorization: `Bearer ${resendApiKey}`, "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (response.ok) {
      return { ok: true };
    }
    const detail = truncate(await safeText(response));
    deps.log("resend_send_failed", { status: response.status, detail });
    return { ok: false, status: response.status, detail };
  } catch (error) {
    deps.log("resend_send_error", { error: String(error) });
    return { ok: false, status: 0, detail: String(error) };
  }
}

export type AudienceSyncStatus = "synced" | "skipped" | "failed";

// Contacts API — verified against:
//   https://resend.com/docs/api-reference/contacts/create-contact
//     POST https://api.resend.com/contacts
//     Body: email (required), unsubscribed?, first_name?, last_name?, properties?,
//           segments?: [{ id }], topics?
//     Success response: { "object": "contact", "id": "<uuid>" }
//   https://resend.com/docs/api-reference/contacts/add-contact-to-segment
//     POST https://api.resend.com/contacts/{contact}/segments/{segment_id}
//     ({contact} may be a contact id OR an email address; no request body)
//     Success response: { "id": "<id>" }
// DEVIATION FROM THE ORIGINAL SPEC, verified against
// https://resend.com/docs/dashboard/segments/migrating-from-audiences-to-segments:
//   Resend renamed Audiences to Segments and deprecated the audience-scoped
//   endpoint (POST /audiences/{audience_id}/contacts) — "Contacts API endpoints
//   that previously required an audience_id can now be used directly instead."
//   RESEND_AUDIENCE_ID therefore holds the segment id (existing audiences were
//   migrated in place), and this worker calls the current, non-deprecated API.
export async function upsertAudienceContact(deps: Deps, email: string): Promise<AudienceSyncStatus> {
  const { resendApiKey, resendAudienceId } = deps.config;
  if (!resendApiKey || !resendAudienceId) {
    return "skipped";
  }
  try {
    const create = await deps.fetchFn(`${RESEND_API_BASE}/contacts`, {
      method: "POST",
      headers: { Authorization: `Bearer ${resendApiKey}`, "Content-Type": "application/json" },
      body: JSON.stringify({ email, unsubscribed: false, segments: [{ id: resendAudienceId }] }),
    });
    if (create.ok) {
      return "synced";
    }
    if (create.status === 409) {
      const attach = await deps.fetchFn(
        `${RESEND_API_BASE}/contacts/${encodeURIComponent(email)}/segments/${encodeURIComponent(resendAudienceId)}`,
        { method: "POST", headers: { Authorization: `Bearer ${resendApiKey}` } },
      );
      if (attach.ok) {
        return "synced";
      }
      deps.log("resend_segment_attach_failed", { status: attach.status, detail: truncate(await safeText(attach)) });
      return "failed";
    }
    deps.log("resend_contact_create_failed", { status: create.status, detail: truncate(await safeText(create)) });
    return "failed";
  } catch (error) {
    deps.log("resend_contact_error", { error: String(error) });
    return "failed";
  }
}
