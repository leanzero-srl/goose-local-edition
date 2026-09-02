import { errorMessage } from '../../utils/conversionUtils';

/**
 * The ACP transport rejects with the SDK's `RequestError extends Error { code, data }`: `message`
 * is the JSON-RPC class ("Invalid params") and `data` is the reason the sidecar wrote
 * ("port 8090 has an unsupervised listener — unmount/reclaim it first"). A person needs the
 * reason; the generic `errorMessage()` takes the `instanceof Error` arm and returns the class
 * alone — measured 2026-09-02 as the banner "Mount failed  Invalid params".
 */
export function mlxErrorMessage(err: unknown, fallback: string): string {
  if (typeof err === 'object' && err !== null && 'data' in err) {
    const data = (err as { data?: unknown }).data;
    if (typeof data === 'string' && data.trim() !== '') return data;
  }
  return errorMessage(err, fallback);
}
