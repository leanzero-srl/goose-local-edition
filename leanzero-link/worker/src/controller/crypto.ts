import { bytesToHex } from "../lib/otp";
import type { AccessClaims } from "./types";

const encoder = new TextEncoder();
export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const ACCOUNT_KEY = /^[0-9a-f]{64}$/;
export const ACCESS_TTL_SECONDS = 15 * 60;
export const REFRESH_TTL_SECONDS = 180 * 86400;
export const AUTH_LEASE_SECONDS = 30;
export const HOST_LEASE_SECONDS = 60;

function encode(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
function decode(value: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error("invalid encoding");
  return Uint8Array.from(atob(value.replace(/-/g, "+").replace(/_/g, "/")), char => char.charCodeAt(0));
}
export async function digest(value: string): Promise<string> {
  return bytesToHex(new Uint8Array(await crypto.subtle.digest("SHA-256", encoder.encode(value))));
}
export function randomSecret(): string { return encode(crypto.getRandomValues(new Uint8Array(32))); }
async function hmac(secret: string, usage: "sign" | "verify"): Promise<CryptoKey> {
  return crypto.subtle.importKey("raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, [usage]);
}
export async function signAccess(secret: string, claims: AccessClaims): Promise<string> {
  const unsigned = `${encode(encoder.encode(JSON.stringify({ alg: "HS256", typ: "JWT" })))}.${encode(encoder.encode(JSON.stringify(claims)))}`;
  return `${unsigned}.${encode(new Uint8Array(await crypto.subtle.sign("HMAC", await hmac(secret, "sign"), encoder.encode(unsigned))))}`;
}
export async function verifyAccess(secret: string, token: string, now: number): Promise<AccessClaims | null> {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const [header, payload, signature] = parts as [string, string, string];
    const h = JSON.parse(new TextDecoder().decode(decode(header))) as Record<string, unknown>;
    if (h.alg !== "HS256" || h.typ !== "JWT") return null;
    if (!await crypto.subtle.verify("HMAC", await hmac(secret, "verify"), decode(signature), encoder.encode(`${header}.${payload}`))) return null;
    const c = JSON.parse(new TextDecoder().decode(decode(payload))) as Partial<AccessClaims>;
    if (c.ver !== 2 || c.token_use !== "controller" || c.iss !== "leanzero-link-auth" || c.aud !== "leanzero-link/controller") return null;
    if (typeof c.sub !== "string" || !UUID.test(c.sub) || typeof c.account_key !== "string" || !ACCOUNT_KEY.test(c.account_key)) return null;
    if (typeof c.device_id !== "string" || !UUID.test(c.device_id) || typeof c.jti !== "string" || !UUID.test(c.jti)) return null;
    if (!Number.isSafeInteger(c.device_revision) || (c.device_revision ?? 0) < 1 || !Number.isSafeInteger(c.iat) || !Number.isSafeInteger(c.exp)) return null;
    if (c.iat! > now || c.exp! <= now || c.exp! <= c.iat! || c.exp! - c.iat! > ACCESS_TTL_SECONDS) return null;
    return c as AccessClaims;
  } catch { return null; }
}

async function receiptKey(secret: string): Promise<CryptoKey> {
  // Domain-separated AEAD key; receipt ciphertext is bound to the account/request with AAD.
  // https://www.w3.org/TR/WebCryptoAPI/#hkdf-operations
  const key = await crypto.subtle.importKey("raw", encoder.encode(secret), "HKDF", false, ["deriveKey"]);
  return crypto.subtle.deriveKey({ name: "HKDF", hash: "SHA-256", salt: encoder.encode("leanzero-link/controller"), info: encoder.encode("receipt/v1") }, key, { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}
export async function seal(secret: string, aad: string, value: unknown): Promise<string> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const cipher = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: encoder.encode(aad) }, await receiptKey(secret), encoder.encode(JSON.stringify(value))));
  const bytes = new Uint8Array(iv.length + cipher.length); bytes.set(iv); bytes.set(cipher, iv.length);
  return encode(bytes);
}
export async function unseal<T>(secret: string, aad: string, value: string): Promise<T> {
  const bytes = decode(value);
  const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: bytes.slice(0, 12), additionalData: encoder.encode(aad) }, await receiptKey(secret), bytes.slice(12));
  return JSON.parse(new TextDecoder().decode(plain)) as T;
}
