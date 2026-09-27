import { describe, expect, it } from 'vitest';
import { fileDiffOf, parseUnifiedDiff, sessionChanges, splitPath } from './fileDiff';
import {
  KICKOFF_EDIT,
  bigCreate,
  diffResponse,
  editRequest,
  failedResponse,
  message,
  unified,
} from './fixtures';

describe('parseUnifiedDiff', () => {
  it('numbers every line on its own side of the owner edit', () => {
    const [hunk] = parseUnifiedDiff(KICKOFF_EDIT);
    expect(hunk.oldStart).toBe(1);
    expect(hunk.lines.map((l) => [l.kind, l.text])).toEqual([
      ['ctx', '# Kickoff'],
      ['ctx', ''],
      ['del', 'Agenda: TBD'],
      ['add', 'Agenda:'],
      ['add', '- scope'],
      ['add', '- dates'],
      ['add', '- owners'],
      ['ctx', ''],
      ['ctx', 'Owner: me'],
    ]);
    const removed = hunk.lines[2];
    const firstAdded = hunk.lines[3];
    const lastContext = hunk.lines[8];
    expect(removed.kind === 'del' && removed.oldNo).toBe(3);
    expect(firstAdded.kind === 'add' && firstAdded.newNo).toBe(3);
    expect(lastContext.kind === 'ctx' && [lastContext.oldNo, lastContext.newNo]).toEqual([5, 8]);
  });

  it('keeps two hunks apart and reads the no-newline marker', () => {
    const hunks = parseUnifiedDiff(
      unified(
        '/w/f',
        '@@ -1 +1 @@\n-a\n+A\n@@ -40,2 +40,2 @@\n x\n-y\n\\ No newline at end of file\n+Y\n'
      )
    );
    expect(hunks).toHaveLength(2);
    expect(hunks[1].newStart).toBe(40);
    expect(hunks[1].lines.map((l) => l.kind)).toEqual(['ctx', 'del', 'eof', 'add']);
  });

  it('reads a create from line 1 and a line whose text starts like a header as content', () => {
    const [hunk] = parseUnifiedDiff(unified('/w/n', '@@ -0,0 +1,2 @@\n+--- a\n+@@ x\n', true));
    expect(hunk.lines).toEqual([
      { kind: 'add', text: '--- a', newNo: 1 },
      { kind: 'add', text: '@@ x', newNo: 2 },
    ]);
  });
});

describe('fileDiffOf', () => {
  it('reads the engine diff off the tool response metadata', () => {
    const diff = fileDiffOf(
      diffResponse('c1', {
        path: '/w/notes/kickoff.md',
        unified: KICKOFF_EDIT,
        added: 4,
        removed: 1,
      })
    );
    expect(diff).toMatchObject({
      path: '/w/notes/kickoff.md',
      added: 4,
      removed: 1,
      before: 'file',
    });
    expect(diff?.hunks).toHaveLength(1);
  });

  it('is null for a failed edit, a call with no diff and a malformed marker', () => {
    expect(fileDiffOf(failedResponse('c1', 'No match found'))).toBeNull();
    expect(fileDiffOf(undefined)).toBeNull();
    expect(
      fileDiffOf({
        type: 'toolResponse',
        id: 'c2',
        toolResult: { status: 'success', value: { content: [], isError: false } },
        metadata: { fileDiff: { path: '/w/f', added: 'many' } },
      })
    ).toBeNull();
  });
});

describe('sessionChanges', () => {
  it('is empty when nothing was written', () => {
    expect(sessionChanges([message('user', [{ type: 'text', text: 'hi' }])])).toEqual({
      files: [],
      added: 0,
      removed: 0,
    });
  });

  it('groups edits per file in first-touched order and sums what each call did', () => {
    const messages = [
      message('assistant', [editRequest('c1', '/w/a.md'), editRequest('c2', '/w/b.md')]),
      message('user', [
        diffResponse('c1', { path: '/w/a.md', unified: KICKOFF_EDIT, added: 4, removed: 1 }),
        diffResponse('c2', {
          path: '/w/b.md',
          unified: bigCreate('/w/b.md', 3),
          added: 3,
          removed: 0,
          before: 'none',
        }),
      ]),
      message('assistant', [editRequest('c3', '/w/a.md'), editRequest('c4', '/w/a.md')]),
      message('user', [
        diffResponse('c3', {
          path: '/w/a.md',
          unified: unified('/w/a.md', '@@ -1 +1 @@\n-x\n+y\n'),
          added: 1,
          removed: 1,
        }),
        failedResponse('c4', 'No match found for the specified text.'),
      ]),
    ];
    const changes = sessionChanges(messages);
    expect(changes.files.map((f) => [f.path, f.added, f.removed, f.edits.length])).toEqual([
      ['/w/a.md', 5, 2, 2],
      ['/w/b.md', 3, 0, 1],
    ]);
    expect(changes.files[0].edits.map((e) => e.toolCallId)).toEqual(['c1', 'c3']);
    expect([changes.added, changes.removed]).toEqual([8, 2]);
  });
});

describe('splitPath', () => {
  it('splits posix and windows paths', () => {
    expect(splitPath('/w/notes/kickoff.md')).toEqual({ name: 'kickoff.md', dir: '/w/notes' });
    expect(splitPath('C:\\w\\a.txt')).toEqual({ name: 'a.txt', dir: 'C:\\w' });
    expect(splitPath('bare')).toEqual({ name: 'bare', dir: '' });
  });
});
