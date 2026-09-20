# Benchmark activity display verification — 2026-09-20

The former panel presented the latest arbitrary stdout line as activity. During the real Gemini run `cloud-20940258-acc3-40be-a65b-69fc56bf66d5`, that became a lone closing brace, while the full temporary directory overflowed the panel.

Commit `0f88c60f6` replaces that presentation with recent recorded tool actions, expandable multiline excerpts, a bounded console tail, and a wrapping run-location disclosure. Main owns the history so remounts recover it; a delayed status response cannot replace newer events. Lifecycle markers remain console-derived. Tool headings do not claim success, and a completed score requires the canonical verdict.

Validation: typecheck and scoped lint passed; the full UI suite passed 2,052 tests across 255 suites, with one skipped. The actual Gemini transcript was replayed through the production component for manual light/dark and 900/360 CSS-pixel checks. Preview timestamps explicitly represent import time. Expanded commands, raw output and full paths stayed inside their containers. Screenshots are in `/tmp/sb71-activity-preview/evidence`.

The clean test worktree at `d6f7b6acb` was packaged, signed locally with the existing Developer ID and installed at `/Applications/Goose.app`. Strict deep signature verification passed. The app was reopened with the existing acceptance profile: optional tools remained ready and the original failed-scoring session remained preserved. The product now says Swarm and describes mixed local/cloud nodes. No new paid model run was launched for this display change. Live activity rendering was verified using the recorded-run preview; the newly installed app was checked at rest.

This is a local test installation, not a notarized release or approval to promote SB7.1. Scorer accuracy review and final release acceptance remain separate outstanding work.
