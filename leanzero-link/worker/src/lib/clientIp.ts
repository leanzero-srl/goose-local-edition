export type ClientIp =
  | { ok: true; ip: string; source: "x-forwarded-for" | "cf-connecting-ip"; forwardedFor: string | null }
  | { ok: false; forwardedFor: string | null };

// Which address a rate-limit bucket is keyed on. Only a PROXY-SET value is trusted:
//
// - Tailscale Funnel/Serve (the self-hosted Node path): ipn/ipnlocal/serve.go
//   `addProxyForwardedHeaders` does `r.Out.Header.Set("X-Forwarded-For",
//   c.SrcAddr.Addr().String())` — Set, not Add, so a client-supplied value is REPLACED,
//   and `HandleIngressTCPConn` documents srcAddr as "the source AddrPort and not that of
//   the ingressPeer" (tailscale/tailscale @ 295179bf2 = v1.98.5, the daemon that fronts
//   this worker). The header is therefore exactly one per-client address. Measured live
//   2026-09-02 through https://worksmacstudio.tailfc4700.ts.net/leanzero-link: the worker
//   logged forwardedFor:"188.27.154.233" — one value, equal to the caller's public egress
//   address (api.ipify.org agreed), not the Funnel ingress and not a 100.x/127.x address.
// - Cloudflare (https://developers.cloudflare.com/fundamentals/reference/http-headers/):
//   X-Forwarded-For is APPENDED — "Cloudflare will append the IP address of the HTTP proxy
//   connecting to Cloudflare" — so the rightmost value is the one Cloudflare saw, equal to
//   CF-Connecting-IP. CF-Connecting-IP is consulted only when no X-Forwarded-For exists;
//   the Node adapter deletes any inbound CF-Connecting-IP so a client cannot supply one.
//
// Rightmost-value rule: every trusted proxy in front of this worker either Sets the header
// or appends to it, so the last element is always the hop the trusted proxy observed. A
// request that reaches the handler with neither header is `ok:false` — the caller skips
// the ip scope loudly rather than bucketing every stranger together as "unknown".
export function resolveClientIp(request: Request): ClientIp {
  const forwardedFor = request.headers.get("X-Forwarded-For");
  if (forwardedFor !== null) {
    const parts = forwardedFor.split(",").map((part) => part.trim()).filter((part) => part.length > 0);
    const rightmost = parts[parts.length - 1];
    if (rightmost !== undefined) {
      return { ok: true, ip: rightmost, source: "x-forwarded-for", forwardedFor };
    }
  }
  const cf = request.headers.get("CF-Connecting-IP")?.trim();
  if (cf) {
    return { ok: true, ip: cf, source: "cf-connecting-ip", forwardedFor };
  }
  return { ok: false, forwardedFor };
}
