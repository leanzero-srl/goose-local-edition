import { chatNodeSetOf, type NodesConfig } from '../nodes/model';
import type { NoNodeRow } from './parseNoNodeError';

/**
 * "Answer on {next} for now" (Q-381, DESIGN-Q359-CHAT-NODES.md "Failures": "lead can't run, switch
 * off → turn ends offering [Answer on {next} for now]"): the node a refused turn of a chat on its own
 * nodes can run on THIS turn, the set unchanged — or null when there is none to offer.
 *
 * Offered only when the refusal is the lead's (its row names the set's first node) and the failover
 * switch is off (on, the router already went down the set). The node is the set's first after the
 * lead that the refusal does not name: an asked turn that also failed on that node names it too, so
 * the same dead end is never offered twice. Read against the set as it is NOW — what goosed will
 * honour when the prompt arrives.
 */
export function answerOnNextOf(
  config: NodesConfig | null | undefined,
  sessionId: string | null | undefined,
  rows: readonly NoNodeRow[]
): { next: string; lead: string } | null {
  const set = chatNodeSetOf(config, sessionId);
  if (!set || set.answerOnNext || set.nodes.length < 2) return null;
  const refused = new Set(rows.map((row) => row.nodeId).filter((id): id is string => id != null));
  const lead = set.nodes[0];
  if (!refused.has(lead)) return null;
  const next = set.nodes.slice(1).find((node) => !refused.has(node));
  return next != null ? { next, lead } : null;
}
