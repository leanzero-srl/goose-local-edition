/**
 * The bundled Forge era's kit readiness as the Benchmark view shows it. The kit (forge/DESIGN.md §10) is
 * the pinned Forge module trees and Atlassian's runtime wrapper, materialised by the era's kit module
 * (`bench/forge2_kit.py ensure`) into a cache — never shipped in the app. Its `status()` reads it without
 * the network.
 */
export interface ForgeKitStatus {
  /** `needs-tools`: the Benchmark tools (Python/Node) are not installed, so the kit cannot be read yet. */
  state: 'ready' | 'missing' | 'needs-tools' | 'error';
  /** The parts `ensure` would still fetch or build (`app-modules`, `wrapper/wrapper.js`, …). */
  missing?: string[];
  kitLockSha256?: string;
  /** The bundled Forge tier's call_budget (isolated_tiers) — the calls its single-model entrant gets. */
  callBudget?: number;
  /** The bundled Forge tier's reasoning_effort (isolated_tiers) — the pinned provider reasoning effort. */
  reasoningEffort?: string | null;
  error?: string;
}
