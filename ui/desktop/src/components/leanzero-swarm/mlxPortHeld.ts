/**
 * The engine port a mount's refusal names as taken, or null for any other failure. goose words it
 * as goose-sidecar's `UnsupervisedListenerError` — "port <N> has an unsupervised listener: pid …
 * — nothing was signalled; <the one next step>" — the refusal a mount owes, before its memory
 * check, when a process this goose may not stop holds the port (Q-276). The Engine tab already
 * names that holder and its step (StrayListenerBanner, from the status), so a surface that sees
 * this refusal says it once, in plain words, rather than repeat the holder a second time (Q-277).
 * The opening is pinned on the backend by `a_leftover_not_ours_still_counts_and_the_mount_names_it_
 * before_the_gate` (crates/goose-sidecar/src/engine.rs).
 */
export function portHeldBy(text: string): number | null {
  const match = /^port (\d+) has an unsupervised listener/.exec(text);
  return match ? Number(match[1]) : null;
}
