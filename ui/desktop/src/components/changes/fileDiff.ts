/**
 * THE diff model (Q-189/Q-190): what a developer `write`/`edit` changed, as the engine measured it.
 *
 * The engine diffs the file it actually wrote (developer/file_diff.rs) and forwards the result on the
 * tool-call update as `_meta.goose.fileDiff`; the ACP adapter lands it on the tool response's
 * `metadata.fileDiff`. The tool card and the session's Changes rail both read it through
 * `fileDiffOf` — one parse, never a second derivation from the tool's arguments. The model never
 * sees any of it: the diff lives outside the result's content.
 */
import type { Message, ToolResponseMessageContent } from '../../types/message';

export type BeforeState = 'file' | 'none' | 'unreadable';

export type DiffLine =
  | { kind: 'add'; text: string; newNo: number }
  | { kind: 'del'; text: string; oldNo: number }
  | { kind: 'ctx'; text: string; oldNo: number; newNo: number }
  /** `\ No newline at end of file` — belongs to the line above it. */
  | { kind: 'eof'; text: string };

export interface DiffHunk {
  oldStart: number;
  newStart: number;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  before: BeforeState;
  added: number;
  removed: number;
  hunks: DiffHunk[];
}

const HUNK_HEADER = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

export function parseUnifiedDiff(unified: string): DiffHunk[] {
  const hunks: DiffHunk[] = [];
  let hunk: DiffHunk | null = null;
  let oldNo = 0;
  let newNo = 0;
  const lines = unified.split('\n');
  if (lines[lines.length - 1] === '') lines.pop();
  for (const line of lines) {
    const header = HUNK_HEADER.exec(line);
    if (header) {
      oldNo = Number(header[1]);
      newNo = Number(header[2]);
      hunk = { oldStart: oldNo, newStart: newNo, lines: [] };
      hunks.push(hunk);
      continue;
    }
    if (!hunk) continue;
    const text = line.slice(1);
    switch (line[0]) {
      case '+':
        hunk.lines.push({ kind: 'add', text, newNo: newNo++ });
        break;
      case '-':
        hunk.lines.push({ kind: 'del', text, oldNo: oldNo++ });
        break;
      case ' ':
        hunk.lines.push({ kind: 'ctx', text, oldNo: oldNo++, newNo: newNo++ });
        break;
      case '\\':
        hunk.lines.push({ kind: 'eof', text: line.slice(2) });
        break;
    }
  }
  return hunks;
}

const BEFORE_STATES: readonly BeforeState[] = ['file', 'none', 'unreadable'];

const parsed = new WeakMap<object, FileDiff | null>();

/** The diff a tool response carries, or null (not a write/edit, a failed call, an older session). */
export function fileDiffOf(toolResponse: ToolResponseMessageContent | undefined): FileDiff | null {
  const raw = toolResponse?.metadata?.fileDiff;
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  if (parsed.has(raw)) return parsed.get(raw) ?? null;
  const record = raw as Record<string, unknown>;
  const { path, before, added, removed, unified } = record;
  const diff =
    typeof path === 'string' &&
    typeof unified === 'string' &&
    typeof added === 'number' &&
    typeof removed === 'number' &&
    BEFORE_STATES.includes(before as BeforeState)
      ? {
          path,
          before: before as BeforeState,
          added,
          removed,
          hunks: parseUnifiedDiff(unified),
        }
      : null;
  parsed.set(raw, diff);
  return diff;
}

/** One write/edit in the session, pointing back at the tool call that made it. */
export interface SessionEdit {
  toolCallId: string;
  diff: FileDiff;
}

export interface ChangedFile {
  path: string;
  added: number;
  removed: number;
  /** In the order they happened. */
  edits: SessionEdit[];
}

export interface SessionChanges {
  files: ChangedFile[];
  added: number;
  removed: number;
}

/**
 * Every file the session wrote, in first-touched order. Counts are the sum of each call's own
 * diff — what the calls did, one after another — not a re-diff against a snapshot the engine never
 * took.
 */
export function sessionChanges(messages: readonly Message[]): SessionChanges {
  const byPath = new Map<string, ChangedFile>();
  let added = 0;
  let removed = 0;
  for (const message of messages) {
    for (const content of message.content) {
      if (content.type !== 'toolResponse') continue;
      const diff = fileDiffOf(content);
      if (!diff) continue;
      let file = byPath.get(diff.path);
      if (!file) {
        file = { path: diff.path, added: 0, removed: 0, edits: [] };
        byPath.set(diff.path, file);
      }
      file.added += diff.added;
      file.removed += diff.removed;
      file.edits.push({ toolCallId: content.id, diff });
      added += diff.added;
      removed += diff.removed;
    }
  }
  return { files: [...byPath.values()], added, removed };
}

/** The DOM id of a tool call's card — the rail scrolls to it. */
export function toolCallDomId(toolCallId: string): string {
  return `tool-call-${toolCallId}`;
}

export const REVEAL_TOOL_CALL_EVENT = 'goose:reveal-tool-call';

/** Scroll the chat to a tool call's card and open it. */
export function revealToolCall(toolCallId: string): void {
  const card = document.getElementById(toolCallDomId(toolCallId));
  card?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  window.dispatchEvent(new CustomEvent(REVEAL_TOOL_CALL_EVENT, { detail: { toolCallId } }));
}

export function splitPath(path: string): { name: string; dir: string } {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut === -1
    ? { name: path, dir: '' }
    : { name: path.slice(cut + 1), dir: path.slice(0, cut) };
}
