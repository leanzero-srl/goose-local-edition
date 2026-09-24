#!/usr/bin/env python3
"""Self-test: every gate proves a BLOCK case AND an ALLOW case before it counts as wired."""
import sys

from gates import (GIB, resolve_fleet_gate, resolve_memory_gate, resolve_port_gate,
                   available_memory_bytes, total_memory_bytes)

FAILURES = []


def check(name, actual, expected_severity):
    if actual["severity"] != expected_severity:
        FAILURES.append("%s: expected %s got %s (%s)" % (name, expected_severity, actual["severity"], actual["message"]))


# G1 memory-mount — the app's one fit rule (fit.rs), 96 GiB workhorse + its 77.8 GiB Metal ceiling
TOTAL = 96 * GIB
CEIL = int(77.76 * GIB)
check("G1 BLOCK: 30G model into 20G available", resolve_memory_gate(30 * GIB, 20 * GIB, TOTAL, CEIL), "BLOCK")
check("G1 BLOCK: fits raw but not past the 9.3% margin", resolve_memory_gate(12 * GIB, 20 * GIB, TOTAL, CEIL), "BLOCK")
check("G1 WARN: fits inside the 2% band", resolve_memory_gate(10 * GIB, 20 * GIB, TOTAL, CEIL), "WARN")
check("G1 ALLOW: 6G model into 40G available", resolve_memory_gate(6 * GIB, 40 * GIB, TOTAL, CEIL), "ALLOW")
check("G1 the GPU ceiling binds on an idle Mac", resolve_memory_gate(80 * GIB, 95 * GIB, TOTAL, CEIL), "BLOCK")
# The recorded case: Flash 97.5 GiB on the M4 Max with 93.0 available, ceiling 107.5 -> short 16.4
check("G1 Flash on the M4 Max is refused", resolve_memory_gate(int(97.5 * GIB), 93 * GIB, 128 * GIB, int(107.5 * GIB)), "BLOCK")

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
