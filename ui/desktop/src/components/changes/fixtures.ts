import type {
  Message,
  ToolRequestMessageContent,
  ToolResponseMessageContent,
} from '../../types/message';
import type { BeforeState } from './fileDiff';

/** A unified diff the way the engine's developer/file_diff.rs writes it. */
export function unified(path: string, body: string, created = false): string {
  return `--- ${created ? '/dev/null' : path}\n+++ ${path}\n${body}`;
}

/** The owner's case (Q-189): "Agenda: TBD" became a four-line agenda. */
export const KICKOFF_EDIT = unified(
  '/w/notes/kickoff.md',
  '@@ -1,5 +1,8 @@\n # Kickoff\n \n-Agenda: TBD\n+Agenda:\n+- scope\n+- dates\n+- owners\n \n Owner: me\n'
);

export function editRequest(id: string, path: string, name = 'edit'): ToolRequestMessageContent {
  return {
    type: 'toolRequest',
    id,
    toolCall: { status: 'success', value: { name, arguments: { path } } },
  };
}

export function diffResponse(
  id: string,
  diff: {
    path: string;
    unified: string;
    added: number;
    removed: number;
    before?: BeforeState;
  },
  text = `Edited ${diff.path}`
): ToolResponseMessageContent {
  return {
    type: 'toolResponse',
    id,
    toolResult: {
      status: 'success',
      value: { content: [{ type: 'text', text }], isError: false },
    },
    metadata: { status: 'completed', fileDiff: { before: 'file', ...diff } },
  };
}

export function failedResponse(id: string, error: string): ToolResponseMessageContent {
  return { type: 'toolResponse', id, toolResult: { status: 'error', error } };
}

export function message(
  role: 'user' | 'assistant',
  content: Message['content'],
  created = 1
): Message {
  return {
    role,
    created,
    content,
    metadata: { userVisible: true, agentVisible: true },
  };
}

/** A diff of `count` added lines, as a create writes it. */
export function bigCreate(path: string, count: number): string {
  const lines = Array.from({ length: count }, (_, i) => `+line ${i + 1}`).join('\n');
  return unified(path, `@@ -0,0 +1,${count} @@\n${lines}\n`, true);
}
