#!/usr/bin/env python3
"""Gates for the MLX engine campaign. Live in CODE, on the acting path, not in prose.

Every mount/bench action calls `gates.py mount ...` first; exit 1 means BLOCKED.
Each gate records the observation that produced it so a future reader can judge
whether it still applies instead of obeying it on faith.

Gates:
  G1 MEMORY-MOUNT   — the model must fit in AVAILABLE memory with headroom, beside the
                      running fleet. Observation: this workhorse runs the fleet's 27B
                      (~16 GB) for live swarm work (2026-08-30); an OOM here kills a
                      benchmark someone else is running. BLOCK: no fit. WARN: thin band.
  G2 PORT-SAFETY    — the sidecar must never target the fleet's ports (1234 LM Studio /
                      11434 Ollama) and never a port something already listens on.
                      Observation: "don't disturb what is happening now seriously"
                      (Mihai 2026-08-30); a mount request to :1234 would load/unload on
                      the LIVE fleet. Always BLOCK.
  G3 FLEET-UNTOUCHED — `lms ps` resident set must be identical to the session-start
                      snapshot. Observation: proving the negative ("we didn't touch the
                      fleet") requires a same-object before/after, not an impression
                      (cf. the 159-ticket incident: a negative that authorises action
                      must be PROVEN). BLOCK on drift; WARN when lms is absent (cannot
                      prove — say so loudly, never silently pass).
"""
import json
import os
import shutil
import socket
import subprocess
import sys

GIB = 1024 ** 3
FORBIDDEN_PORTS = {1234, 11434}
SNAPSHOT_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".fleet-snapshot.json")


def _f(severity, gate, message):
    return {"severity": severity, "gate": gate, "message": message}


# ---- pure resolvers (self-tested in gates_selftest.py) ----

def resolve_memory_gate(model_bytes, available_bytes, total_bytes):
    floor = max(8 * GIB, total_bytes // 10)
    leftover = available_bytes - model_bytes - floor
    if leftover < 0:
        return _f("BLOCK", "G1-memory-mount",
                  "model %.1f GiB + floor %.1f GiB exceeds available %.1f GiB (short %.1f GiB)"
                  % (model_bytes / GIB, floor / GIB, available_bytes / GIB, -leftover / GIB))
    if leftover < 4 * GIB:
        return _f("WARN", "G1-memory-mount",
                  "fits, but only %.1f GiB above the floor — expect pressure under load" % (leftover / GIB))
    return _f("ALLOW", "G1-memory-mount",
              "model %.1f GiB fits with %.1f GiB above the %.1f GiB floor"
              % (model_bytes / GIB, leftover / GIB, floor / GIB))


def resolve_port_gate(port, something_listening):
    if port in FORBIDDEN_PORTS:
        return _f("BLOCK", "G2-port-safety", "port %d belongs to the running fleet" % port)
    if something_listening:
        return _f("BLOCK", "G2-port-safety", "port %d already has a listener — a mount would hit a foreign server" % port)
    return _f("ALLOW", "G2-port-safety", "port %d is free and not a fleet port" % port)


def resolve_fleet_gate(snapshot_ids, current_ids):
    if snapshot_ids is None:
        return _f("WARN", "G3-fleet-untouched", "no snapshot exists — cannot PROVE the fleet is untouched; run `gates.py snapshot` at session start")
    if current_ids is None:
        return _f("WARN", "G3-fleet-untouched", "lms unavailable — cannot PROVE the fleet is untouched")
    added = sorted(set(current_ids) - set(snapshot_ids))
    removed = sorted(set(snapshot_ids) - set(current_ids))
    if added or removed:
        return _f("BLOCK", "G3-fleet-untouched", "fleet residency DRIFTED since snapshot: added=%s removed=%s" % (added, removed))
    return _f("ALLOW", "G3-fleet-untouched", "fleet residency identical to snapshot (%d models)" % len(snapshot_ids))


# ---- measurements ----

def total_memory_bytes():
    return int(subprocess.check_output(["sysctl", "-n", "hw.memsize"]).strip())


def available_memory_bytes():
    out = subprocess.check_output(["vm_stat"], text=True)
    page_size = 16384
    pages = {}
    for line in out.splitlines():
        if line.startswith("Mach Virtual Memory Statistics"):
            page_size = int(line.split("page size of")[1].split("bytes")[0].strip())
            continue
        if ":" in line:
            key, val = line.split(":", 1)
            pages[key.strip()] = int(val.strip().rstrip("."))
    usable = ("Pages free", "Pages inactive", "Pages speculative", "Pages purgeable")
    return sum(pages.get(k, 0) for k in usable) * page_size


def dir_size_bytes(path):
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            fp = os.path.join(root, name)
            if not os.path.islink(fp):
                total += os.path.getsize(fp)
    return total


def port_has_listener(port):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.settimeout(0.5)
        return s.connect_ex(("127.0.0.1", port)) == 0


def lms_resident_ids():
    lms = os.path.expanduser("~/.lmstudio/bin/lms")
    if not os.path.exists(lms):
        lms = shutil.which("lms")
    if not lms:
        return None
    try:
        out = subprocess.check_output([lms, "ps", "--json"], text=True, timeout=15)
        return sorted(m.get("identifier", m.get("modelKey", "?")) for m in json.loads(out))
    except Exception:
        return None


# ---- CLI ----

def _emit(findings):
    blocked = False
    for f in findings:
        print("%-5s %-18s %s" % (f["severity"], f["gate"], f["message"]))
        blocked = blocked or f["severity"] == "BLOCK"
    return 1 if blocked else 0


def main(argv):
    cmd = argv[1] if len(argv) > 1 else "probe"
    if cmd == "probe":
        total, avail = total_memory_bytes(), available_memory_bytes()
        print("total   %.1f GiB" % (total / GIB))
        print("available %.1f GiB" % (avail / GIB))
        ids = lms_resident_ids()
        print("lms resident: %s" % (ids if ids is not None else "UNKNOWN (lms unavailable)"))
        return 0
    if cmd == "snapshot":
        ids = lms_resident_ids()
        if ids is None:
            print("WARN  G3-fleet-untouched  lms unavailable — snapshot NOT written")
            return 1
        with open(SNAPSHOT_PATH, "w") as fh:
            json.dump({"resident": ids}, fh)
        print("snapshot written: %d resident models" % len(ids))
        return 0
    if cmd == "verify-fleet":
        snap = None
        if os.path.exists(SNAPSHOT_PATH):
            with open(SNAPSHOT_PATH) as fh:
                snap = json.load(fh)["resident"]
        return _emit([resolve_fleet_gate(snap, lms_resident_ids())])
    if cmd == "mount":
        args = dict(zip(argv[2::2], argv[3::2]))
        if "--model-path" in args:
            model_bytes = dir_size_bytes(os.path.expanduser(args["--model-path"]))
        elif "--size-gb" in args:
            model_bytes = int(float(args["--size-gb"]) * GIB)
        else:
            print("mount requires --model-path DIR or --size-gb N (and --port N)")
            return 2
        port = int(args.get("--port", "0"))
        findings = [resolve_memory_gate(model_bytes, available_memory_bytes(), total_memory_bytes())]
        if port:
            findings.append(resolve_port_gate(port, port_has_listener(port)))
        else:
            findings.append(_f("WARN", "G2-port-safety", "no --port given — port gate not evaluated"))
        snap = None
        if os.path.exists(SNAPSHOT_PATH):
            with open(SNAPSHOT_PATH) as fh:
                snap = json.load(fh)["resident"]
        findings.append(resolve_fleet_gate(snap, lms_resident_ids()))
        return _emit(findings)
    print("usage: gates.py [probe|snapshot|verify-fleet|mount --model-path DIR|--size-gb N --port N]")
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
