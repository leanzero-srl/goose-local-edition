export interface WorkerEnvVars {
  LINK_JWT_SECRET?: string;
  RESEND_API_KEY?: string;
  RESEND_AUDIENCE_ID?: string;
  LEANZERO_MAIL_FROM?: string;
  TS_API_TOKEN?: string;
  TS_TAILNET?: string;
  TS_NODE_TAG?: string;
  TS_KEY_EXPIRY_SECONDS?: string;
  // Headscale (self-hosted, multi-tenant): the control-plane API this worker mints
  // per-account preauth keys against, and the PUBLIC control URL a node joins with.
  HEADSCALE_API_URL?: string;
  HEADSCALE_API_KEY?: string;
  HEADSCALE_LOGIN_SERVER?: string;
  ALLOWED_ORIGINS?: string;
}

export const OTP_TTL_SECONDS = 600;
export const OTP_MAX_ATTEMPTS = 5;
export const EMAIL_RATE_LIMIT = 3;
export const IP_RATE_LIMIT = 10;
// Verify calls per email per window. A maximally clumsy legitimate hour is 3 codes × 5
// attempts + 3 successes = 18; this bounds guessing at 20/1e6 per hour per email even
// where the attempt counter is only eventually consistent (Cloudflare KV).
export const VERIFY_RATE_LIMIT = 20;
export const RATE_WINDOW_SECONDS = 3600;
export const JWT_TTL_SECONDS = 180 * 86400;
export const DEFAULT_TS_KEY_EXPIRY_SECONDS = 600;
export const DEFAULT_TS_NODE_TAG = "tag:leanzero-link";

/// Which control plane the worker mints mesh join keys against. `headscale` is the
/// self-hosted, per-account-isolated path (preferred when configured); `tailscale` is
/// the hosted-control fallback; `none` means no mesh backend is configured.
export type MeshProvider = "headscale" | "tailscale" | "none";

export interface Config {
  jwtSecret: string | undefined;
  resendApiKey: string | undefined;
  resendAudienceId: string | undefined;
  mailFrom: string | undefined;
  tsApiToken: string | undefined;
  tsTailnet: string | undefined;
  tsNodeTag: string | undefined;
  tsKeyExpirySeconds: number;
  tsKeyExpiryInvalid: boolean;
  hsApiUrl: string | undefined;
  hsApiKey: string | undefined;
  hsLoginServer: string | undefined;
  meshProvider: MeshProvider;
  allowedOrigins: string[];
}

function nonEmpty(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

export function parseConfig(env: WorkerEnvVars): Config {
  const rawExpiry = nonEmpty(env.TS_KEY_EXPIRY_SECONDS);
  let tsKeyExpirySeconds = DEFAULT_TS_KEY_EXPIRY_SECONDS;
  let tsKeyExpiryInvalid = false;
  if (rawExpiry !== undefined) {
    if (/^[1-9][0-9]*$/.test(rawExpiry)) {
      tsKeyExpirySeconds = Number(rawExpiry);
    } else {
      tsKeyExpiryInvalid = true;
    }
  }
  const origins = (nonEmpty(env.ALLOWED_ORIGINS) ?? "*")
    .split(",")
    .map((origin) => origin.trim())
    .filter((origin) => origin.length > 0);
  const hsApiUrl = nonEmpty(env.HEADSCALE_API_URL)?.replace(/\/+$/, "");
  const hsApiKey = nonEmpty(env.HEADSCALE_API_KEY);
  const hsLoginServer = nonEmpty(env.HEADSCALE_LOGIN_SERVER)?.replace(/\/+$/, "");
  const tsApiToken = nonEmpty(env.TS_API_TOKEN);
  const tsTailnet = nonEmpty(env.TS_TAILNET);
  // Headscale wins when fully configured (self-hosted, per-account isolation); the
  // Tailscale hosted-control path is the fallback.
  const meshProvider: MeshProvider =
    hsApiUrl && hsApiKey && hsLoginServer ? "headscale" : tsApiToken && tsTailnet ? "tailscale" : "none";
  return {
    jwtSecret: nonEmpty(env.LINK_JWT_SECRET),
    resendApiKey: nonEmpty(env.RESEND_API_KEY),
    resendAudienceId: nonEmpty(env.RESEND_AUDIENCE_ID),
    mailFrom: nonEmpty(env.LEANZERO_MAIL_FROM),
    tsApiToken,
    tsTailnet,
    // Distinguish "absent" from "explicitly empty": TS_NODE_TAG unset keeps the
    // Cloudflare-Worker default tag; TS_NODE_TAG present but empty means mint UNTAGGED
    // (undefined) — required on a personal tailnet that doesn't own tag:leanzero-link
    // in its ACL, where minting a tagged key is rejected but untagged succeeds.
    tsNodeTag: env.TS_NODE_TAG === undefined ? DEFAULT_TS_NODE_TAG : nonEmpty(env.TS_NODE_TAG),
    tsKeyExpirySeconds,
    tsKeyExpiryInvalid,
    hsApiUrl,
    hsApiKey,
    hsLoginServer,
    meshProvider,
    allowedOrigins: origins.length > 0 ? origins : ["*"],
  };
}

export interface Capabilities {
  mail: boolean;
  audience: boolean;
  mesh: boolean;
}

export function capabilities(config: Config): Capabilities {
  return {
    mail: Boolean(config.resendApiKey && config.mailFrom),
    audience: Boolean(config.resendApiKey && config.resendAudienceId),
    mesh: config.meshProvider !== "none",
  };
}
