#!/usr/bin/env bash
# Pre-load the swarm models across the LM Link fleet BEFORE running a swarm.
#
# Why: remote JIT-loading a large model on a device that is already busy generating
# (e.g. loading a 38 GB worker on the workhorse while it runs the 27B planner) can fail
# with "Server error: Model is unloaded". Pre-warming every model avoids that entirely.
# Verified: workhorse holds the 27B planner AND the 35B worker simultaneously (~68 GB).
set -euo pipefail
LMS="${LMS:-lms}"

echo "Pre-loading swarm models across the LM Link fleet (TTL 1h)..."
# Planner (smart, dense) — workhorse
"$LMS" load qwen/qwen3.6-27b -y --ttl 3600
# Worker A — mac.lan
"$LMS" load qwen/qwen3.6-35b-a3b -y --ttl 3600
# Worker B — macbook (Local)
"$LMS" load qwen3.6-35b-a3b-mtp-holo3-qwopus-qx86-hi-mlx -y --ttl 3600
# Worker C — workhorse
"$LMS" load qwopus3.6-35b-a3b-v1-mtp -y --ttl 3600

echo
echo "Fleet now loaded:"
"$LMS" ps
