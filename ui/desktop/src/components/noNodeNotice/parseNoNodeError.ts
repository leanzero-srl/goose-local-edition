/**
 * The swarm chat router's refusal (crates/goose/src/providers/swarm_router.rs, the `servable.is_empty()`
 * arm) reaches the chat as ASSISTANT TEXT, wrapped by the agent loop (agents/agent.rs):
 *
 *   Ran into this error: Execution error: swarm chat: no node can serve this turn — <id>: <reason>; <id>: <reason>.
 *
 *   Please retry if you think this is a transient or recoverable error.
 *
 * The marker is the router's own phrase. Reasons are split on `; ` only OUTSIDE parentheses, because
 * the MLX reason carries its own `; ` inside "(this process's manager: stopped; error sending …)".
 * A reason the classifier does not know stays `kind: 'other'` and is rendered verbatim — never dropped.
 */
export const NO_NODE_MARKER = 'no node can serve this turn — ';

export type NodeReason =
  | { kind: 'mlx-down'; base: string }
  | { kind: 'mlx-wrong-model'; served: string; wanted: string }
  | { kind: 'lm-unreachable'; url: string }
  | { kind: 'lm-not-listed'; model: string }
  | { kind: 'busy' }
  | { kind: 'no-devices' }
  | { kind: 'other' };

export interface NoNodeRow {
  /** The swarm device id; null for the router's pool-level reason (no enabled device). */
  nodeId: string | null;
  /** The router's reason, verbatim. */
  raw: string;
  reason: NodeReason;
}

function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === '(') depth++;
    else if (ch === ')') depth = Math.max(0, depth - 1);
    else if (depth === 0 && ch === ';' && text[i + 1] === ' ') {
      parts.push(text.slice(start, i));
      start = i + 2;
      i++;
    }
  }
  parts.push(text.slice(start));
  return parts.map((p) => p.trim()).filter(Boolean);
}

export function classifyReason(raw: string): NodeReason {
  let m = /^MLX engine is not listening on (\S+)/.exec(raw);
  if (m) return { kind: 'mlx-down', base: m[1] };
  m = /^MLX engine serves '([^']*)', the device wants '([^']*)'/.exec(raw);
  if (m) return { kind: 'mlx-wrong-model', served: m[1], wanted: m[2] };
  m = /^model '([^']*)' is not listed by \S+/.exec(raw);
  if (m) return { kind: 'lm-not-listed', model: m[1] };
  m = /^(\S+) unreachable \(/.exec(raw);
  if (m) return { kind: 'lm-unreachable', url: m[1] };
  if (raw === 'refused admission this turn') return { kind: 'busy' };
  if (raw.startsWith('no enabled device is configured')) return { kind: 'no-devices' };
  return { kind: 'other' };
}

/** Rows when `text` is the router's refusal, else null. */
export function parseNoNodeError(text: string): NoNodeRow[] | null {
  const at = text.indexOf(NO_NODE_MARKER);
  if (at < 0) return null;
  let body = text.slice(at + NO_NODE_MARKER.length);
  const paragraph = body.indexOf('\n\n');
  if (paragraph >= 0) body = body.slice(0, paragraph);
  body = body.trim().replace(/\.$/, '');
  return splitTopLevel(body).map((segment) => {
    const m = /^([A-Za-z0-9][\w.-]*): ([\s\S]+)$/.exec(segment);
    const nodeId = m ? m[1] : null;
    const raw = m ? m[2] : segment;
    return { nodeId, raw, reason: classifyReason(raw) };
  });
}
