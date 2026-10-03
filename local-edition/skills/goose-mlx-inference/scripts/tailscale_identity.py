# The personal Tailscale's IDENTITY (traffic counters excluded — a benchmark moves them).
import json, sys
d = json.load(sys.stdin)
s = d.get("Self") or {}
peers = sorted(
    (p.get("ID"), p.get("PublicKey"), p.get("HostName"), tuple(p.get("TailscaleIPs") or []))
    for p in (d.get("Peer") or {}).values()
)
print(json.dumps({
    "BackendState": d.get("BackendState"),
    "Self": [s.get("ID"), s.get("PublicKey"), s.get("HostName"), s.get("DNSName"),
             s.get("TailscaleIPs"), s.get("UserID")],
    "Tailnet": (d.get("CurrentTailnet") or {}).get("Name"),
    "Peers": peers,
}, sort_keys=True))
