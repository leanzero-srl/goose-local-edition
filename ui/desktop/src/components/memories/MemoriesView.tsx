import { useState, useEffect, useMemo, useCallback, type ReactNode } from 'react';
import { Brain, BookOpen, MessageSquare, Pencil, Sparkles, Trash2 } from 'lucide-react';
import { TreeContextMenu } from '../Layout/tree';
import { useStartChatAbout } from '../Layout/useStartChatAbout';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import MarkdownContent from '../MarkdownContent';
import { errorMessage } from '../../utils/conversionUtils';
import { getInitialWorkingDir } from '../../utils/workingDir';
import { acpGetSessionListItem } from '../../acp/sessions';
import { displaySessionListName } from '../../sessions';
import {
  describeSourceTag,
  isSessionKey,
  splitSourceTags,
  type MemoryOrigin,
} from '../../utils/memoryProvenance';
import { Button, Chip, EmptyState, FOCUS, MOTION, RADIUS, SURFACE, TNUM, TYPE, cx } from '../lz';
import { LibraryGroup, LibraryRow, LibraryShell, shownSelection } from '../library/Library';

/** One stored memory — mirrors the shape returned by the `list-memories` IPC in main.ts. */
export interface MemoryEntry {
  id: string;
  category: string;
  scope: 'global' | 'local';
  tags: string[];
  content: string;
  /** The category FILE's mtime: the store keeps no per-entry time. */
  updatedAt: number;
  filePath?: string;
  /** Present only when the entry is a saved agent proposal. */
  origin?: MemoryOrigin;
}

/** kebab-case category → readable title ("absorb-human-voice" → "Absorb Human Voice"). */
function prettyTitle(category: string): string {
  return category
    .replace(/[-_]+/g, ' ')
    .replace(/\b\w/g, (c) => c.toUpperCase())
    .trim();
}

/** The list preview: the text as one run of prose (markdown emphasis and headings stripped),
 *  bounded before it reaches the DOM — the row clamps it to two lines. */
function previewOf(content: string): string {
  return content
    .slice(0, 400)
    .replace(/\*\*/g, '')
    .replace(/^#+\s*/gm, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** The kind tags goose's importer and memory tools write (`# feedback …`): shown as the row's label. */
const KIND_TAGS = new Set(['feedback', 'project', 'user', 'reference']);

function kindOf(memory: MemoryEntry): string | undefined {
  return memory.tags.find((t) => KIND_TAGS.has(t.toLowerCase()));
}

function formatWhen(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** What is asked of the model when a memory is opened as a chat about it: the memory's text,
 *  where it is stored, and the memory tools that rewrite it. */
export function askAboutMemoryPrompt(memory: MemoryEntry): string {
  const store =
    memory.scope === 'global'
      ? '~/.config/goose/memory/<category>.txt (global, every project)'
      : '<working dir>/.goose/memory/<category>.txt (local to this project)';
  const tags = memory.tags.length > 0 ? memory.tags.join(', ') : 'none';
  return [
    `I want to work on one of my goose memories — category "${memory.category}", ${memory.scope} scope, tags: ${tags}. It currently says:`,
    '',
    memory.content,
    '',
    `Memories live in ${store}; you change them with the memory tools: remember_memory (category, data, tags, is_global) adds an entry, remove_specific_memory removes one, remove_memory_category drops the whole category, retrieve_memories reads it back. To modify this one, remove the old entry and remember the new text under the same category and scope; to fork it, remember it under a new category.`,
    'Ask me what I want changed before you write anything, then make the change and read the category back to confirm.',
  ].join('\n');
}

function MemoryListItem({
  memory,
  selected,
  onSelect,
  onEdit,
  onDelete,
  onAsk,
}: {
  memory: MemoryEntry;
  selected: boolean;
  onSelect: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onAsk: () => void;
}) {
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  return (
    <div>
      <LibraryRow
        testId="memory-row"
        title={prettyTitle(memory.category)}
        label={kindOf(memory)}
        preview={previewOf(memory.content)}
        selected={selected}
        onSelect={onSelect}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      />
      {menu && (
        <TreeContextMenu
          x={menu.x}
          y={menu.y}
          testId="memory-context-menu"
          onClose={() => setMenu(null)}
          items={[
            {
              key: 'open',
              label: 'Open',
              icon: <BookOpen />,
              onClick: () => {
                setMenu(null);
                onSelect();
              },
            },
            {
              key: 'edit',
              label: 'Edit',
              icon: <Pencil />,
              onClick: () => {
                setMenu(null);
                onEdit();
              },
            },
            {
              key: 'ask',
              label: 'Start an AI session about this memory',
              icon: <Sparkles />,
              onClick: () => {
                setMenu(null);
                onAsk();
              },
            },
            {
              key: 'delete',
              label: 'Delete',
              icon: <Trash2 />,
              danger: true,
              separator: true,
              onClick: () => {
                setMenu(null);
                onDelete();
              },
            },
          ]}
        />
      )}
    </div>
  );
}

/** The session a saved proposal came from, by name when the session list still knows it. */
function OriginSession({ sessionKey }: { sessionKey: string }) {
  const [name, setName] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);
  useEffect(() => {
    let live = true;
    setName(null);
    setMissing(false);
    acpGetSessionListItem(sessionKey)
      .then((item) => live && setName(displaySessionListName(item.name)))
      .catch(() => live && setMissing(true));
    return () => {
      live = false;
    };
  }, [sessionKey]);
  if (missing) {
    return (
      <span className={TYPE.body}>
        <span className="font-mono text-lz-mono">{sessionKey}</span>
        <span className="text-lz-ink-2"> — no longer in the session list</span>
      </span>
    );
  }
  return (
    <button
      type="button"
      data-testid="memory-origin-session"
      onClick={() => {
        window.location.hash = `#/pair?resumeSessionId=${encodeURIComponent(sessionKey)}`;
      }}
      className={cx(
        'inline-flex items-center gap-1.5 text-left text-lz-body text-lz-accent underline-offset-2 hover:underline [&_svg]:size-3.5',
        RADIUS.control,
        FOCUS
      )}
    >
      <MessageSquare aria-hidden />
      {name ?? sessionKey}
    </button>
  );
}

/**
 * Where the memory came from — only what the store records: its scope and file, the tags the
 * importer wrote, and (for a memory saved from an agent proposal) the session, the date it was
 * proposed and the agent's reason. A field with no data is not drawn.
 */
function MemoryProvenance({ memory, workingDir }: { memory: MemoryEntry; workingDir?: string }) {
  const { sources, rest } = splitSourceTags(memory.tags);
  const origin = memory.origin;
  const rows: Array<{ key: string; label: string; value: ReactNode }> = [];
  rows.push({
    key: 'scope',
    label: 'Scope',
    value:
      memory.scope === 'global'
        ? 'Global — recalled in every project'
        : `This project${workingDir ? ` — ${workingDir.split('/').filter(Boolean).pop()}` : ''}`,
  });
  for (const tag of sources) {
    rows.push({ key: `source-${tag}`, label: 'Source', value: describeSourceTag(tag) });
  }
  if (origin) {
    rows.push({ key: 'source-proposal', label: 'Source', value: 'Saved from an agent proposal' });
    if (isSessionKey(origin.key)) {
      rows.push({
        key: 'session',
        label: 'Session',
        value: <OriginSession sessionKey={origin.key} />,
      });
    }
    if (origin.proposedAt > 0) {
      rows.push({
        key: 'proposed',
        label: 'Proposed',
        value: formatWhen(origin.proposedAt * 1000),
      });
    }
    if (origin.why) {
      rows.push({ key: 'why', label: 'Why it was kept', value: origin.why });
    }
  }
  rows.push({
    key: 'category',
    label: 'Category',
    value: <span className="font-mono text-lz-mono">{memory.category}</span>,
  });
  if (rest.length > 0) {
    rows.push({
      key: 'tags',
      label: 'Tags',
      value: (
        <span className="flex flex-wrap gap-1">
          {rest.map((t) => (
            <Chip key={t}>{t}</Chip>
          ))}
        </span>
      ),
    });
  }
  if (memory.updatedAt > 0) {
    rows.push({
      key: 'changed',
      label: 'File last changed',
      value: <span className={TNUM}>{formatWhen(memory.updatedAt)}</span>,
    });
  }
  if (memory.filePath) {
    rows.push({
      key: 'file',
      label: 'Stored in',
      value: <span className="break-all font-mono text-lz-mono">{memory.filePath}</span>,
    });
  }
  return (
    <dl
      data-testid="memory-provenance"
      aria-label="Where this memory came from"
      className={cx(
        'grid grid-cols-[max-content_minmax(0,1fr)] items-baseline gap-x-6 gap-y-2 border p-4',
        RADIUS.card,
        SURFACE.hairline
      )}
    >
      {rows.map((r) => (
        <div key={r.key} className="contents" data-testid={`memory-provenance-${r.key}`}>
          <dt className={TYPE.meta}>{r.label}</dt>
          <dd className={TYPE.body}>{r.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function MemoryDetail({
  memory,
  workingDir,
  onSaved,
  onDeleted,
  onAsk,
  requestEdit,
  requestDelete,
}: {
  memory: MemoryEntry;
  workingDir?: string;
  onSaved: () => void;
  onDeleted: () => void;
  onAsk: () => void;
  /** Bumped by the list's context menu: enter editing / open the delete confirm from outside. */
  requestEdit?: number;
  requestDelete?: number;
}) {
  const [editing, setEditing] = useState(false);
  const [body, setBody] = useState(memory.content);
  const [deleting, setDeleting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset when a different memory is selected or its content changes on disk (e.g. after a save re-list).
  useEffect(() => {
    setEditing(false);
    setBody(memory.content);
    setError(null);
    setConfirmDelete(false);
  }, [memory.id, memory.content]);
  // The list's context menu may select AND ask to edit/delete in one motion: these run AFTER the
  // reset above so the request wins over it.
  useEffect(() => {
    if (requestEdit) setEditing(true);
  }, [requestEdit]);
  useEffect(() => {
    if (requestDelete) setConfirmDelete(true);
  }, [requestDelete]);

  const dirty = body !== memory.content;

  const save = useCallback(async () => {
    setError(null);
    try {
      await window.electron.editMemory({
        scope: memory.scope,
        category: memory.category,
        oldContent: memory.content,
        newContent: body,
        workingDir,
      });
      setEditing(false);
      onSaved();
    } catch (e) {
      setError(errorMessage(e, 'Failed to save'));
    }
  }, [memory, body, workingDir, onSaved]);

  const doDelete = useCallback(async () => {
    setDeleting(true);
    setError(null);
    try {
      await window.electron.deleteMemory({
        scope: memory.scope,
        category: memory.category,
        content: memory.content,
        workingDir,
      });
      setConfirmDelete(false);
      onDeleted();
    } catch (e) {
      setError(errorMessage(e, 'Failed to delete'));
    } finally {
      setDeleting(false);
    }
  }, [memory, workingDir, onDeleted]);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="memory-detail">
      <div className={cx('flex items-start gap-3 border-b px-lz-page py-4', SURFACE.hairline)}>
        <h2 className={cx(TYPE.h1, 'min-w-0 flex-1 truncate')}>{prettyTitle(memory.category)}</h2>
        {!editing && (
          <Button size="sm" variant="ghost" icon={<Sparkles />} onClick={onAsk}>
            Ask AI about it
          </Button>
        )}
        <Button
          size="sm"
          variant="secondary"
          icon={<Pencil />}
          onClick={() => {
            if (editing) setBody(memory.content);
            setEditing((e) => !e);
          }}
        >
          {editing ? 'Cancel edit' : 'Edit'}
        </Button>
        <Button
          size="sm"
          variant="secondary"
          icon={<Trash2 />}
          onClick={() => setConfirmDelete(true)}
        >
          Delete
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-lz-page py-5">
        <div className="flex max-w-[760px] flex-col gap-5">
          {error && (
            <p role="alert" className="text-lz-body text-lz-err">
              {error}
            </p>
          )}
          {editing ? (
            <div className="flex flex-col gap-3">
              <textarea
                aria-label="Memory text"
                value={body}
                onChange={(e) => setBody(e.target.value)}
                spellCheck={false}
                className={cx(
                  'h-[360px] w-full resize-y bg-lz-surface p-3 font-mono text-lz-mono text-lz-ink',
                  SURFACE.outline,
                  RADIUS.control,
                  FOCUS,
                  MOTION
                )}
              />
              <div className="flex gap-2">
                <Button size="sm" variant="primary" onClick={save} disabled={!dirty}>
                  Save
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setBody(memory.content)}
                  disabled={!dirty}
                >
                  Revert
                </Button>
              </div>
            </div>
          ) : (
            <div data-testid="memory-body" className="break-words text-lz-body text-lz-ink">
              <MarkdownContent content={memory.content} />
            </div>
          )}
          <MemoryProvenance memory={memory} workingDir={workingDir} />
        </div>
      </div>

      <ConfirmationModal
        isOpen={confirmDelete}
        title={`Delete "${prettyTitle(memory.category)}"?`}
        message="This permanently removes this memory — goose will no longer recall it. There is no undo."
        confirmLabel="Delete"
        cancelLabel="Keep it"
        confirmVariant="destructive"
        isSubmitting={deleting}
        onConfirm={doDelete}
        onCancel={() => setConfirmDelete(false)}
      />
    </div>
  );
}

export default function MemoriesView() {
  const [memories, setMemories] = useState<MemoryEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchTerm, setSearchTerm] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editRequest, setEditRequest] = useState(0);
  const [deleteRequest, setDeleteRequest] = useState(0);
  const startChat = useStartChatAbout();

  const loadMemories = useCallback(async () => {
    try {
      setError(null);
      setMemories(await window.electron.listMemories(getInitialWorkingDir()));
    } catch (err) {
      setError(errorMessage(err, 'Failed to load memories'));
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    loadMemories();
  }, [loadMemories]);

  // Goose writes new memories from its own process while a window sits open — re-read on focus so the list on
  // screen matches disk (same reasoning as the Skills view).
  useEffect(() => {
    const onFocus = () => loadMemories();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [loadMemories]);

  const filtered = useMemo(() => {
    const q = searchTerm.trim().toLowerCase();
    if (!q) return memories;
    return memories.filter(
      (m) =>
        m.category.toLowerCase().includes(q) ||
        prettyTitle(m.category).toLowerCase().includes(q) ||
        m.content.toLowerCase().includes(q) ||
        m.tags.some((t) => t.toLowerCase().includes(q))
    );
  }, [memories, searchTerm]);

  const groups = useMemo(() => {
    const order: MemoryEntry['scope'][] = ['global', 'local'];
    const titles: Record<MemoryEntry['scope'], string> = {
      global: 'Global',
      local: 'This project',
    };
    return order
      .map((scope) => ({
        scope,
        title: titles[scope],
        items: filtered.filter((m) => m.scope === scope),
      }))
      .filter((g) => g.items.length > 0);
  }, [filtered]);

  const visible = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const selected = shownSelection(visible, selectedId, (m) => m.id);

  const renderList = () => {
    if (!loaded) {
      return <p className={cx(TYPE.meta, 'px-2 py-2')}>Reading memories…</p>;
    }
    if (error) {
      return (
        <div className="flex flex-col items-start gap-2 px-2 py-2">
          <p role="alert" className="text-lz-body text-lz-err">
            Could not load memories: {error}
          </p>
          <Button size="sm" variant="secondary" onClick={loadMemories}>
            Try again
          </Button>
        </div>
      );
    }
    if (memories.length === 0) {
      return (
        <p className={cx(TYPE.bodyMuted, 'px-2 py-2')}>
          No memories yet. Goose stores them in ~/.config/goose/memory/ — imported from your cloud
          profile and learned as it works.
        </p>
      );
    }
    if (visible.length === 0) {
      return (
        <p className={cx(TYPE.bodyMuted, 'px-2 py-2')}>No memory matches “{searchTerm.trim()}”.</p>
      );
    }
    return groups.map((group) => (
      <LibraryGroup key={group.scope} title={group.title} count={group.items.length}>
        {group.items.map((m) => (
          <MemoryListItem
            key={m.id}
            memory={m}
            selected={m.id === selected?.id}
            onSelect={() => setSelectedId(m.id)}
            onEdit={() => {
              setSelectedId(m.id);
              setEditRequest((n) => n + 1);
            }}
            onDelete={() => {
              setSelectedId(m.id);
              setDeleteRequest((n) => n + 1);
            }}
            onAsk={() => void startChat(askAboutMemoryPrompt(m))}
          />
        ))}
      </LibraryGroup>
    ));
  };

  return (
    <LibraryShell
      testId="memories-view"
      title="Memories"
      subtitle="What goose remembers — imported from your cloud profile and learned as it works."
      search={{
        value: searchTerm,
        onChange: setSearchTerm,
        placeholder: 'Search memories',
        label: 'Search memories by name, text or tag',
      }}
      list={renderList()}
      detail={
        selected ? (
          <MemoryDetail
            memory={selected}
            workingDir={getInitialWorkingDir()}
            requestEdit={editRequest}
            requestDelete={deleteRequest}
            onAsk={() => void startChat(askAboutMemoryPrompt(selected))}
            onSaved={() => loadMemories()}
            onDeleted={() => {
              setSelectedId(null);
              loadMemories();
            }}
          />
        ) : loaded && !error && memories.length === 0 ? (
          <EmptyState
            icon={<Brain />}
            title="No memories yet"
            body="Goose remembers what you tell it to keep, and what you save from its proposals in a chat."
          />
        ) : null
      }
    />
  );
}
