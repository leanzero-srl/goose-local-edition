import type { KVStore } from "./deps";
import { bytesToHex } from "./otp";

// The per-ACCOUNT node secret (R-H2). Every device of an account derives its node token
// from this one value, so it must be identical across devices and re-logins: minted
// ONCE per email, stored without TTL under `nodesecret:<email>`, and returned by every
// /verify and /mesh/join-key success. It replaces the derivable HMAC(key=email, msg=
// public constant), which was an account TAG anyone who knew the email could compute,
// not a credential. Minted through the store's atomic `update`, so two devices racing
// the first sign-in converge on one value instead of each storing its own.

export const NODE_SECRET_BYTES = 32;
const NODE_SECRET_HEX = /^[0-9a-f]{64}$/;

export function nodeSecretKey(email: string): string {
  return `nodesecret:${email}`;
}

export async function ensureNodeSecret(
  kv: KVStore,
  email: string,
  log: (event: string, fields?: Record<string, unknown>) => void,
): Promise<string> {
  let secret: string | undefined;
  await kv.update(nodeSecretKey(email), (raw) => {
    if (raw !== null && NODE_SECRET_HEX.test(raw)) {
      secret = raw;
      return "keep";
    }
    if (raw !== null) {
      log("node_secret_corrupt", { email, detail: "stored value is not 64 hex chars; re-minting" });
    }
    const bytes = new Uint8Array(NODE_SECRET_BYTES);
    crypto.getRandomValues(bytes);
    secret = bytesToHex(bytes);
    log("node_secret_minted", { email });
    return { value: secret };
  });
  if (secret === undefined) {
    throw new Error("kv.update did not run the node-secret mutator");
  }
  return secret;
}
