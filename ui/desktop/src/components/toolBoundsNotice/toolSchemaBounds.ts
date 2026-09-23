/**
 * The engine's refusal of a tool set too large to compile into a grammar, and the app's
 * measurement of the session's tools in the ENGINE's own units.
 *
 * rapid-mlx (0.14.3, routes/chat.py `_enforce_tool_grammar_bounds_or_400`, #561) refuses the turn
 * with HTTP 400 when constrained tool-calling is on and the tools list exceeds its bounds; the
 * agent loop (agents/agent.rs, the generic provider-error arm) wraps it as assistant text and, since
 * 3a98d9974, persists it user-only, so the same text comes back on reopen:
 *
 *   Ran into this error: Request failed: Bad request (400): tool schema exceeds grammar-compile
 *   bounds (max 256 tools, 65536 bytes, depth 32); reduce the tool schema or set
 *   RAPID_MLX_CONSTRAIN_TOOLS=0 to fall back to free-form tool calling..
 *
 * The bounds are READ from that text, never assumed: an engine built with other caps states them
 * the same way, and a bound the text does not carry stays null (and is not compared against).
 */
import type { ToolListItem } from '@aaif/goose-sdk';

export const TOOL_BOUNDS_MARKER = 'tool schema exceeds grammar-compile bounds';

export interface ToolBounds {
  maxTools: number | null;
  maxBytes: number | null;
  maxDepth: number | null;
  /** The engine's sentence, verbatim, from the marker to the end of its paragraph. */
  raw: string;
}

function numberAfter(pattern: RegExp, text: string): number | null {
  const m = pattern.exec(text);
  return m ? Number(m[1]) : null;
}

/** The engine's bounds when `text` is its refusal, else null. */
export function parseToolBoundsError(text: string): ToolBounds | null {
  const at = text.indexOf(TOOL_BOUNDS_MARKER);
  if (at < 0) return null;
  let raw = text.slice(at);
  const paragraph = raw.indexOf('\n\n');
  if (paragraph >= 0) raw = raw.slice(0, paragraph);
  raw = raw.trim().replace(/\.+$/, '.');
  const open = raw.indexOf('(');
  const close = open >= 0 ? raw.indexOf(')', open) : -1;
  const stated = open >= 0 && close > open ? raw.slice(open + 1, close) : '';
  return {
    maxTools: numberAfter(/max\s+(\d+)\s+tools/, stated),
    maxBytes: numberAfter(/(\d+)\s+bytes/, stated),
    maxDepth: numberAfter(/depth\s+(\d+)/, stated),
    raw,
  };
}

/** Python `json.dumps(s)` (ensure_ascii) for a string: JSON.stringify, then every non-ASCII UTF-16
 *  unit as `\uXXXX` — the escaped form whose byte length the engine charges. */
function pyJsonString(s: string): string {
  return JSON.stringify(s).replace(
    /[\u0080-￿]/g,
    (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, '0')}`
  );
}

interface Walk {
  bytes: number;
  depth: number;
}

/** Compact-JSON bytes and container depth of `value` exactly as the engine's walker counts them
 *  (`_walk_size_and_depth`: `{}`/`[]` 2 each, `:` per member, `,` between, scalars as json.dumps;
 *  the depth is the deepest CONTAINER's level, the root object being level 0). */
function walk(value: unknown, level: number, out: Walk): void {
  if (Array.isArray(value)) {
    out.depth = Math.max(out.depth, level);
    out.bytes += 2 + Math.max(0, value.length - 1);
    for (const v of value) walk(v, level + 1, out);
  } else if (value !== null && typeof value === 'object') {
    out.depth = Math.max(out.depth, level);
    const entries = Object.entries(value as Record<string, unknown>);
    out.bytes += 2 + entries.length + Math.max(0, entries.length - 1);
    for (const [k, v] of entries) {
      out.bytes += pyJsonString(k).length;
      walk(v, level + 1, out);
    }
  } else if (typeof value === 'string') {
    out.bytes += pyJsonString(value).length;
  } else {
    out.bytes += String(JSON.stringify(value ?? null)).length;
  }
}

/** One tool as the engine charges it: `{"name": …, "parameters": …}` — the description is NOT
 *  compiled into the grammar and is not counted. */
export function engineToolSize(tool: Pick<ToolListItem, 'name' | 'inputSchema'>): Walk {
  const out: Walk = { bytes: 0, depth: 0 };
  walk({ name: tool.name, parameters: tool.inputSchema ?? null }, 0, out);
  return out;
}

export interface ToolGroupSize {
  tools: number;
  /** The tools' own bytes, without the list envelope. */
  bytes: number;
  depth: number;
}

export function measureTools(tools: ReadonlyArray<Pick<ToolListItem, 'name' | 'inputSchema'>>) {
  let bytes = 0;
  let depth = 0;
  for (const tool of tools) {
    const size = engineToolSize(tool);
    bytes += size.bytes;
    depth = Math.max(depth, size.depth);
  }
  return { tools: tools.length, bytes, depth } satisfies ToolGroupSize;
}

/** The whole list as the engine charges it: the tools plus `[]` and the N-1 commas between them. */
export function listEnvelopeBytes(toolCount: number, toolBytes: number): number {
  return toolBytes + 2 + Math.max(0, toolCount - 1);
}

export function withinBounds(bounds: ToolBounds, total: ToolGroupSize): boolean | null {
  const checks: boolean[] = [];
  if (bounds.maxTools != null) checks.push(total.tools <= bounds.maxTools);
  if (bounds.maxBytes != null)
    checks.push(listEnvelopeBytes(total.tools, total.bytes) <= bounds.maxBytes);
  if (bounds.maxDepth != null) checks.push(total.depth <= bounds.maxDepth);
  return checks.length === 0 ? null : checks.every(Boolean);
}
