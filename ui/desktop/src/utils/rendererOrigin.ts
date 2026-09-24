/**
 * The Origin goose's renderer requests carry: a window's page loads from `file://`, so every request
 * it makes is re-stamped with the dev-server origin goosed's CORS accepts.
 *
 * ONLY a window's requests. Main's own `net.fetch` rides the same default session, and a stamped
 * Origin turns it into a "browser" request: the LeanZero Link chat relay refuses any Origin-bearing
 * request with 403, so the MLX tile's live read of a model served from another Mac failed every poll
 * (3.0.28, "Rates unavailable over LeanZero Link — engine returned 403"). goosed answers an Origin-less
 * request as it always did — its CORS layer acts only when the header is present.
 */
export const RENDERER_ORIGIN = 'http://localhost:5173';

export function withRendererOrigin(
  requestHeaders: Record<string, string>,
  webContentsId: number | undefined
): Record<string, string> {
  if (webContentsId === undefined) return requestHeaders;
  return { ...requestHeaders, Origin: RENDERER_ORIGIN };
}
