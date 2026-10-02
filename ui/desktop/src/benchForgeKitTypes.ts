/**
 * The Forge kit's readiness as the Benchmark view shows it. The kit (forge/DESIGN.md §10) is the pinned
 * Forge module trees and Atlassian's runtime wrapper, materialised by `bench/forge_kit.py ensure` into a
 * cache — never shipped in the app. `forge_kit.status()` reads it without the network.
 */
export interface ForgeKitStatus {
  /** `needs-tools`: the Benchmark tools (Python/Node) are not installed, so the kit cannot be read yet. */
  state: 'ready' | 'missing' | 'needs-tools' | 'error';
  /** The parts `ensure` would still fetch or build (`app-modules`, `wrapper/wrapper.js`, …). */
  missing?: string[];
  kitLockSha256?: string;
  /** bench_budget.CALL_BUDGET — the calls every single-model entrant gets. */
  callBudget?: number;
  /** isolated_tiers.FORGE10.wallet_usd — the spend limit a Forge run arms when none is set. */
  walletDefaultUsd?: string | null;
  /** isolated_tiers.FORGE10.reasoning_effort — the pinned provider reasoning effort. */
  reasoningEffort?: string | null;
  error?: string;
}
