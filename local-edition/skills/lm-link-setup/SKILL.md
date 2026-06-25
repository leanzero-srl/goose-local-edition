---
name: lm-link-setup
description: Set up, verify, and operate LM Studio LM Link across the local fleet so the swarm agent can route subtasks to specific devices by model-id. Use when LM Link is down, a device is missing, models aren't routing, or when onboarding a new node.
---

# LM Link setup & operations

LM Link makes several LM Studio instances reachable through ONE local OpenAI endpoint
(`http://localhost:1234/v1`). A request for a model-id is routed (over a Tailscale `tsnet`/WireGuard
P2P tunnel) to whichever linked device holds that model. This is the swarm's transport: the agent
addresses a device by choosing its model-id; no per-device endpoints.

## Fleet (this deployment)
- MacBook `Mihai-Macbook-2` — M4 Max / 128GB — control node (runs the agent + the `:1234` front).
- `WorksMacStudio.lan` (workhorse) — M3 Ultra — holds `qwen/qwen3.6-27b` (planner/verifier).
- `Mac.lan` 192.168.8.222 — M3 Max / 64GB — holds `qwen/qwen3.6-35b-a3b` (fast worker).

## Enable LM Link on a node
On EACH machine that should join (GUI: sidebar → LM Link → enable). Headless:
```bash
curl -fsSL https://lmstudio.ai/install.sh | bash   # if lms not present
lms login            # one-time, ties the device to your LM Studio Hub account
lms link enable      # join the encrypted link
lms link set-device-name "<friendly-name>"
```

## Verify the link + routing
```bash
lms link status      # this device + connected peers + their loaded model instances
lms ls               # all models across the fleet, with their DEVICE column
lms ps               # currently LOADED models, STATUS (IDLE/GENERATING), and DEVICE
curl -s http://localhost:1234/v1/models | python3 -m json.tool   # ids the agent can address
```
Expected: `lms link status` shows the peers "connected"; `curl /v1/models` lists every fleet model id;
`lms ps` shows which device is GENERATING when a request is in flight (use this to confirm routing).

## Place a model on a specific device
A model is served by the device that holds it. To control placement:
```bash
lms load <model-id>           # interactive picker can target a specific connected device
```
For deterministic per-device targeting when the SAME model exists on multiple devices, give each a
distinct alias/id (LM Link's "Preferred Device" otherwise picks one). 3-way fan-out of one model
requires loading it on each target device.

## Wire the agent
The agent uses the single endpoint and selects a device by model-id (Goose provider `lmstudio`):
```bash
export LMSTUDIO_HOST=http://localhost:1234
export LMSTUDIO_API_KEY=lm-studio        # dummy; ignored
```
Per-subtask routing is config: a recipe/sub-recipe `settings.goose_model` or a `delegate(model=...)`
call selects which device serves that subtask.

## Troubleshooting
- A model id 404s on `/v1/models`: it isn't downloaded on any linked device → `lms get` / download it there.
- Peer shows disconnected: re-run `lms link enable` on that node; confirm both ran `lms login` (same account).
- Tool-calling intermittently returns plain text on Qwen3.6 hybrids: this was tested and does NOT occur under
  LM Studio's hybrid-aware cache — if it ever appears, suspect a non-LM-Studio engine in the path.
- `workhorse` SSH alias may be stale (DHCP); LM Link routing is independent of SSH. Re-discover SSH IP via
  `arp -a | grep -i worksmacstudio`.
