#!/usr/bin/env node
// SB7.2 browser probe: product_probe_sb71.mjs under the SB7.2 profile — the published framing
// default camera (yaw 70, pitch 50, distance 190), the wheel step that still reaches the distance
// clamp from it, and the overview legibility check (q_overview_legibility). One probe, one
// implementation: the profile is a global read once at module start, never an environment variable
// an SB7.1 grading process could inherit. Same CLI, same single-JSON output as the SB7.1 probe.
globalThis.__BENCH_PROBE_TIER = 'sb-7.2';
await import('./product_probe_sb71.mjs');
