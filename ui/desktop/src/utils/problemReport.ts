import type { SystemInfo } from '../types/diagnostics';
import { LEANZERO_CONTACT_API_URL, LEANZERO_SUPPORT_EMAIL, SWARM_DISPLAY_NAME } from '../branding';

/**
 * Report a problem (Q-192): the rules both delivery paths share, kept free of React and Electron so
 * the renderer's form and main's `send-problem-report` handler read ONE copy.
 *
 * Two paths, both to office@leanzero.net, both chosen by the person — never switched silently:
 *  - SEND: main POSTs to leanzero.net's own contact endpoint (the site's recipient is fixed server
 *    side; this app cannot address anyone else). The endpoint requires a reply address.
 *  - MAIL APP: a mailto: link with the report prefilled, sent from the person's own mail client.
 * System details are attached only when the person ticks the box, and what is attached is exactly
 * the text `systemDetailsText` returns — the form previews that same string.
 */

export type FetchLike = (url: string, init?: RequestInit) => Promise<Response>;

/** measured: leanzero.net/api/contact answers 400 "Message must be at least 10 characters long". */
export const REPORT_MIN_CHARS = 10;
/** policy: a report is one readable email; the field stops here rather than the server truncating. */
export const REPORT_MAX_CHARS = 5000;
/** policy: the details are six short lines — anything longer is not what the preview showed. */
export const ATTACHMENT_MAX_CHARS = 2000;
/** Transport bound on the one POST (a dead network, not a slow server, is what outlives it). */
export const SEND_TIMEOUT_MS = 20_000;

export const REPORT_SUBJECT = `${SWARM_DISPLAY_NAME} problem report`;

/** The same shape test the site's route applies, so the form refuses what the server would. */
const EMAIL_SHAPE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function isEmail(value: string): boolean {
  return EMAIL_SHAPE.test(value.trim());
}

export type ReportVia = 'send' | 'mail';
export type ReportProblemKind =
  | 'description-short'
  | 'description-long'
  | 'email-missing'
  | 'email-invalid';

/** What stops this report going out by `via`, or null. Only SEND needs the email (the site does). */
export function validateReport(
  { description, email }: { description: string; email: string },
  via: ReportVia
): ReportProblemKind | null {
  const text = description.trim();
  if (text.length < REPORT_MIN_CHARS) return 'description-short';
  if (text.length > REPORT_MAX_CHARS) return 'description-long';
  const address = email.trim();
  if (address === '') return via === 'send' ? 'email-missing' : null;
  return isEmail(address) ? null : 'email-invalid';
}

/** A model given as a local path carries the person's home folder; the file name is what helps. */
function modelLabel(model: string): string {
  const isPath = model.startsWith('/') || model.startsWith('~') || /^[A-Za-z]:[\\/]/.test(model);
  if (!isPath) return model;
  const parts = model.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? model;
}

/**
 * The system details a person may attach — names and versions only. Never session messages, logs,
 * config values, endpoints or keys: the source is the engine's `summary` diagnostics `system` block,
 * and only these named fields are read from it. An absent field is SAID ("not set"), not dropped.
 */
export function systemDetailsText(info: SystemInfo, desktopVersion: string): string {
  const extensions = info.enabled_extensions.join(', ');
  return [
    `${SWARM_DISPLAY_NAME} desktop: ${desktopVersion || 'version not reported'}`,
    `goose engine: ${info.app_version}`,
    `OS: ${info.os} ${info.os_version} (${info.architecture})`,
    `Provider: ${info.provider || 'not set'}`,
    `Model: ${info.model ? modelLabel(info.model) : 'not set'}`,
    `Extensions: ${extensions || 'none'}`,
  ].join('\n');
}

const DETAILS_RULE = '--- System details (attached by the sender) ---';

/** The message text both paths carry: the person's words, then the details they chose to attach. */
export function reportMessage(description: string, attachment: string | null): string {
  const text = description.trim();
  return attachment ? `${text}\n\n${DETAILS_RULE}\n${attachment}` : text;
}

/**
 * The mail-app path. encodeURIComponent, not URLSearchParams: the latter writes spaces as `+`, which
 * mail clients show literally. The reply address, when given, is written into the body — the mail
 * client sends from the person's own account, so it is a note, not a header.
 */
export function mailtoUrl(description: string, email: string, attachment: string | null): string {
  const address = email.trim();
  const body = address
    ? `${reportMessage(description, attachment)}\n\nReply to: ${address}`
    : reportMessage(description, attachment);
  return `mailto:${LEANZERO_SUPPORT_EMAIL}?subject=${encodeURIComponent(REPORT_SUBJECT)}&body=${encodeURIComponent(body)}`;
}

export interface ProblemReportInput {
  description: string;
  email: string;
  attachment: string | null;
  desktopVersion: string;
}

export type ProblemReportResult =
  | { ok: true }
  | { ok: false; reason: 'invalid' | 'http' | 'network'; status?: number; message: string };

/**
 * The body leanzero.net/api/contact takes. Its `name`/`company`/`service` fields name the SOURCE so
 * the office inbox can tell an app report from a sales enquiry ("New Contact Form Submission from
 * Goose Swarm user", service "Problem report"); the recipient is not a field — the site fixes it.
 */
export function contactPayload(input: ProblemReportInput): Record<string, string> {
  return {
    name: `${SWARM_DISPLAY_NAME} user`,
    email: input.email.trim(),
    company: `${SWARM_DISPLAY_NAME} ${input.desktopVersion || '(version not reported)'}`,
    service: 'Problem report',
    message: reportMessage(input.description, input.attachment),
  };
}

function parseInput(raw: unknown): ProblemReportInput | null {
  if (raw == null || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.description !== 'string' || typeof r.email !== 'string') return null;
  if (r.attachment !== null && typeof r.attachment !== 'string') return null;
  if (typeof r.desktopVersion !== 'string') return null;
  return {
    description: r.description,
    email: r.email,
    attachment: r.attachment,
    desktopVersion: r.desktopVersion.slice(0, 64),
  };
}

/**
 * MAIN's `send-problem-report` body (the renderer CSP blocks leanzero.net). The renderer's object is
 * re-validated and rebuilt key by key — nothing it adds rides along. Every failure is NAMED with the
 * server's own words where it gave any, so the form can say why and offer the mail app instead.
 * The site acknowledges with 200 once the report is accepted; it then hands the mail to Resend.
 */
export async function sendProblemReport(
  raw: unknown,
  fetchImpl: FetchLike
): Promise<ProblemReportResult> {
  const input = parseInput(raw);
  if (input == null) return { ok: false, reason: 'invalid', message: 'malformed report' };
  const problem = validateReport(input, 'send');
  if (problem != null) return { ok: false, reason: 'invalid', message: problem };
  if (input.attachment != null && input.attachment.length > ATTACHMENT_MAX_CHARS) {
    return { ok: false, reason: 'invalid', message: 'attachment-too-long' };
  }
  let res: Response;
  try {
    res = await fetchImpl(LEANZERO_CONTACT_API_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(contactPayload(input)),
      signal: AbortSignal.timeout(SEND_TIMEOUT_MS),
    });
  } catch (err) {
    return {
      ok: false,
      reason: 'network',
      message: err instanceof Error ? err.message : String(err),
    };
  }
  if (res.ok) return { ok: true };
  const text = await res.text().catch((err: unknown) => `(unreadable body: ${String(err)})`);
  let message = text.slice(0, 300);
  try {
    const parsed = JSON.parse(text) as { error?: unknown };
    if (typeof parsed.error === 'string') message = parsed.error;
  } catch {
    // not JSON — the raw text (capped above) is the server's words
  }
  return {
    ok: false,
    reason: 'http',
    status: res.status,
    message: message || `HTTP ${res.status}`,
  };
}
