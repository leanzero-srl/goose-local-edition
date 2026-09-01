export interface JwtClaims {
  sub: string;
  iat: number;
  exp: number;
  ver: number;
}

export type JwtVerifyResult =
  | { ok: true; claims: JwtClaims }
  | { ok: false; reason: "malformed" | "bad_signature" | "expired" | "bad_claims" };

const encoder = new TextEncoder();

function base64UrlEncodeBytes(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function base64UrlDecodeBytes(value: string): Uint8Array | null {
  try {
    const base64 = value.replace(/-/g, "+").replace(/_/g, "/");
    const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
    const binary = atob(padded);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  } catch {
    return null;
  }
}

async function hmacKey(secret: string, usage: "sign" | "verify"): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    [usage],
  );
}

export async function signJwt(secret: string, claims: JwtClaims): Promise<string> {
  const header = base64UrlEncodeBytes(encoder.encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const payload = base64UrlEncodeBytes(encoder.encode(JSON.stringify(claims)));
  const signingInput = `${header}.${payload}`;
  const key = await hmacKey(secret, "sign");
  const signature = await crypto.subtle.sign("HMAC", key, encoder.encode(signingInput));
  return `${signingInput}.${base64UrlEncodeBytes(new Uint8Array(signature))}`;
}

// The header's declared alg is never trusted: verification always recomputes
// HMAC-SHA256 with our secret, so alg-confusion ("none", RS256 swap) cannot bypass it.
export async function verifyJwt(secret: string, token: string, nowSeconds: number): Promise<JwtVerifyResult> {
  const parts = token.split(".");
  if (parts.length !== 3) {
    return { ok: false, reason: "malformed" };
  }
  const [header, payload, signature] = parts;
  if (!header || !payload || !signature) {
    return { ok: false, reason: "malformed" };
  }
  const signatureBytes = base64UrlDecodeBytes(signature);
  if (signatureBytes === null) {
    return { ok: false, reason: "malformed" };
  }
  const key = await hmacKey(secret, "verify");
  const valid = await crypto.subtle.verify("HMAC", key, signatureBytes, encoder.encode(`${header}.${payload}`));
  if (!valid) {
    return { ok: false, reason: "bad_signature" };
  }
  const payloadBytes = base64UrlDecodeBytes(payload);
  if (payloadBytes === null) {
    return { ok: false, reason: "malformed" };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(payloadBytes));
  } catch {
    return { ok: false, reason: "malformed" };
  }
  if (typeof parsed !== "object" || parsed === null) {
    return { ok: false, reason: "bad_claims" };
  }
  const claims = parsed as Record<string, unknown>;
  if (
    typeof claims.sub !== "string" ||
    typeof claims.iat !== "number" ||
    typeof claims.exp !== "number" ||
    claims.ver !== 1
  ) {
    return { ok: false, reason: "bad_claims" };
  }
  if (nowSeconds >= claims.exp) {
    return { ok: false, reason: "expired" };
  }
  return { ok: true, claims: { sub: claims.sub, iat: claims.iat, exp: claims.exp, ver: 1 } };
}
