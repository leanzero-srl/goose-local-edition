#!/usr/bin/env python3
"""Self-test: every gate proves a BLOCK case AND an ALLOW case before it counts as wired."""
import sys

from gates import (GIB, resolve_fleet_gate, resolve_memory_gate, resolve_port_gate,
                   available_memory_bytes, total_memory_bytes)

FAILURES = []


def check(name, actual, expected_severity):
    if actual["severity"] != expected_severity:
        FAILURES.append("%s: expected %s got %s (%s)" % (name, expected_severity, actual["severity"], actual["message"]))


# G1 memory-mount — 96 GiB machine numbers, mirroring the real workhorse
TOTAL = 96 * GIB
check("G1 BLOCK: 30G model into 20G available", resolve_memory_gate(30 * GIB, 20 * GIB, TOTAL), "BLOCK")
check("G1 BLOCK: fits raw but not above floor", resolve_memory_gate(12 * GIB, 20 * GIB, TOTAL), "BLOCK")
check("G1 WARN: fits with thin band", resolve_memory_gate(8 * GIB, 20 * GIB, TOTAL), "WARN")
check("G1 ALLOW: 6G model into 40G available", resolve_memory_gate(6 * GIB, 40 * GIB, TOTAL), "ALLOW")
check("G1 floor respects 10pct on big boxes", resolve_memory_gate(1 * GIB, 12 * GIB, 512 * GIB), "BLOCK")

# G2 port-safety
check("G2 BLOCK: fleet port 1234", resolve_port_gate(1234, False), "BLOCK")
check("G2 BLOCK: ollama port 11434", resolve_port_gate(11434, False), "BLOCK")
check("G2 BLOCK: occupied port", resolve_port_gate(8090, True), "BLOCK")
check("G2 ALLOW: free non-fleet port", resolve_port_gate(8090, False), "ALLOW")

# G3 fleet-untouched
SNAP = ["workhorse-qwen3.6-27b-mlx"]
check("G3 BLOCK: model unloaded", resolve_fleet_gate(SNAP, []), "BLOCK")
check("G3 BLOCK: model added", resolve_fleet_gate(SNAP, SNAP + ["extra-9b"]), "BLOCK")
check("G3 WARN: no snapshot cannot prove", resolve_fleet_gate(None, SNAP), "WARN")
check("G3 WARN: lms unavailable cannot prove", resolve_fleet_gate(SNAP, None), "WARN")
check("G3 ALLOW: identical residency", resolve_fleet_gate(SNAP, list(SNAP)), "ALLOW")

# probes return sane live numbers on this machine
total, avail = total_memory_bytes(), available_memory_bytes()
if not (total > 8 * GIB and 0 < avail < total):
    FAILURES.append("probes: implausible measurements total=%d avail=%d" % (total, avail))

if FAILURES:
    print("SELFTEST FAIL (%d):" % len(FAILURES))
    for f in FAILURES:
        print("  " + f)
    sys.exit(1)
print("SELFTEST PASS: %d assertions, every gate proved BLOCK and ALLOW" % 15)
