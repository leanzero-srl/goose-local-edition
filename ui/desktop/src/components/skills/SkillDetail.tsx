import { useState, useEffect, useCallback, useMemo } from 'react';
import type { SourceEntry } from '@aaif/goose-sdk';
import { ChevronRight, ChevronDown, FileText, Folder, FolderOpen } from 'lucide-react';
import { Button } from '../ui/button';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { ScrollArea } from '../ui/scroll-area';
import MarkdownContent from '../MarkdownContent';
import { errorMessage } from '../../utils/conversionUtils';
import { updateSkillSource, deleteSkillSource, readSkillSourceFresh } from '../../acp/sources';
import { isEditable, isPersonaPath, splitPersona, recomposePersona } from './skillKinds';
import { buildSkillTree, defaultExpanded, type TreeNode } from './skillTree';

/** Solid, saturated origin colors — one hue per root, no tints. */
const ORIGIN_STYLE: Record<string, { label: string; className: string }> = {
  persona: { label: 'WRITTEN BY GOOSE', className: 'bg-[#7c3aed] text-white' },
  builtin: { label: 'BUILT IN', className: 'bg-[#0f766e] text-white' },
  global: { label: 'GLOBAL', className: 'bg-[#1d4ed8] text-white' },
  project: { label: 'PROJECT', className: 'bg-[#b45309] text-white' },
};

function OriginBadge({ origin }: { origin: string }) {
  const s = ORIGIN_STYLE[origin] ?? ORIGIN_STYLE.global;
  return (
    <span className={`px-2 py-0.5 text-[10px] font-bold tracking-wider ${s.className}`}>
      {s.label}
    </span>
  );
}

/**
 * The editor for the ONE section of a persona the engine keeps.
 *
 * Everything above `## Your notes` is regenerated wholesale by the next successful build of that stack, so
 * this deliberately does NOT offer to edit it. That is not a simplification — an editor over the engine's
 * region would accept a correction, report a successful save, and then have it silently deleted by the next
 * build, which is exactly the self-poisoning the persona feature is only defensible for preventing.
 */
function PersonaEditor({
  entry,
  projectDir,
  onSaved,
}: {
  entry: SourceEntry;
  projectDir: string;
  onSaved: (updated: SourceEntry) => void;
}) {
  const zones = splitPersona(entry.content);
  const [notes, setNotes] = useState(zones.notes ?? '');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<SourceEntry | null>(null);

  useEffect(() => {
    setNotes(splitPersona(entry.content).notes ?? '');
    setError(null);
  }, [entry.path, entry.content]);

  const dirty = notes.trim() !== (zones.notes ?? '').trim();

  const save = useCallback(
    async (force: boolean) => {
      setSaving(true);
      setError(null);
      try {
        // CONFLICT CHECK. The swarm rewrites this file from another process with a truncating write and no
        // locking, and the update API has no etag/mtime to do this server-side — so compare on-disk bytes
        // against the ones this editor was seeded with. Without it, saving would resurrect a stale lesson
        // over the one the engine just PROVED, and the re-list afterwards would render the stale text back
        // as current truth, concealing it.
        if (!force) {
          const fresh = await readSkillSourceFresh(projectDir, entry.path);
          if (fresh && fresh.content !== entry.content) {
            setConflict(fresh);
            setSaving(false);
            return;
          }
        }
        const base = force && conflict ? splitPersona(conflict.content).engine : zones.engine;
        const updated = await updateSkillSource({
          path: entry.path,
          name: entry.name,
          description: entry.description,
          content: recomposePersona(base, notes),
        });
        setConflict(null);
        onSaved(updated);
      } catch (e) {
        setError(errorMessage(e, 'Failed to save'));
      } finally {
        setSaving(false);
      }
    },
    [entry, notes, zones.engine, conflict, projectDir, onSaved]
  );

  return (
    <div className="flex flex-col gap-3">
      <div className="border border-borderSubtle p-4">
        <div className="flex items-center justify-between mb-2">
          <h3 className="text-sm font-bold">Your notes</h3>
          {dirty && <span className="text-[11px] font-bold text-[#b45309]">UNSAVED</span>}
        </div>
        <p className="text-xs text-text-secondary mb-3">
          Goose keeps this section word for word every time it rewrites the rest of this skill. A correction
          here is permanent; one written above is regenerated away on the next successful build.
        </p>
        <textarea
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          spellCheck={false}
          placeholder="e.g. goose keeps getting this wrong: we use SQLModel here, not raw SQLAlchemy."
          className="w-full h-40 p-3 font-mono text-xs bg-background-default border border-borderSubtle text-text-default focus:outline-none focus:border-borderStandard resize-y"
        />
        {error && <p className="mt-2 text-xs font-bold text-[#dc2626]">{error}</p>}
        <div className="flex gap-2 mt-3">
          <Button size="sm" onClick={() => save(false)} disabled={!dirty || saving}>
            {saving ? 'Saving…' : 'Save notes'}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => setNotes(zones.notes ?? '')}
            disabled={!dirty || saving}
          >
            Revert
          </Button>
        </div>
      </div>

      <ConfirmationModal
        isOpen={conflict !== null}
        title="Goose rewrote this skill while you were editing"
        message="It finished another successful build of this stack and replaced the lesson above. Your notes are safe either way — choose which lesson to keep."
        confirmLabel="Keep goose's new lesson"
        cancelLabel="Cancel my save"
        onConfirm={() => save(true)}
        onCancel={() => setConflict(null)}
      />
    </div>
  );
}

/** The editor for an ordinary, hand-authored skill: the whole body is the user's. */
function BodyEditor({
  entry,
  projectDir,
  onSaved,
}: {
  entry: SourceEntry;
  projectDir: string;
  onSaved: (updated: SourceEntry) => void;
}) {
  const [body, setBody] = useState(entry.content);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<SourceEntry | null>(null);

  useEffect(() => {
    setBody(entry.content);
    setError(null);
  }, [entry.path, entry.content]);

  const dirty = body !== entry.content;

  const save = useCallback(
    async (force: boolean) => {
      setSaving(true);
      setError(null);
      try {
        if (!force) {
          const fresh = await readSkillSourceFresh(projectDir, entry.path);
          if (fresh && fresh.content !== entry.content) {
            setConflict(fresh);
            setSaving(false);
            return;
          }
        }
        const updated = await updateSkillSource({
          path: entry.path,
          name: entry.name,
          description: entry.description,
          content: body,
        });
        setConflict(null);
        onSaved(updated);
      } catch (e) {
        setError(errorMessage(e, 'Failed to save'));
      } finally {
        setSaving(false);
      }
    },
    [entry, body, projectDir, onSaved]
  );

  return (
    <div className="flex flex-col gap-3">
      <textarea
        value={body}
        onChange={(e) => setBody(e.target.value)}
        spellCheck={false}
        className="w-full h-[420px] p-3 font-mono text-xs bg-background-default border border-borderSubtle text-text-default focus:outline-none focus:border-borderStandard resize-y"
      />
      {error && <p className="text-xs font-bold text-[#dc2626]">{error}</p>}
      <div className="flex gap-2">
        <Button size="sm" onClick={() => save(false)} disabled={!dirty || saving}>
          {saving ? 'Saving…' : 'Save'}
        </Button>
        <Button size="sm" variant="outline" onClick={() => setBody(entry.content)} disabled={!dirty}>
          Revert
        </Button>
      </div>
      <ConfirmationModal
        isOpen={conflict !== null}
        title="This skill changed on disk while you were editing"
        message="Something else rewrote this file after you opened it. Saving now would overwrite that with your older copy."
        confirmLabel="Overwrite anyway"
        cancelLabel="Cancel my save"
        onConfirm={() => save(true)}
        onCancel={() => setConflict(null)}
      />
    </div>
  );
}

/**
 * The skill's OTHER files.
 *
 * A skill is a directory, not a document: atlassian-community-leanzero ships 950 files — the scripts its own
 * SKILL.md tells the agent to run, the references it tells it to read. Rendering only SKILL.md showed the
 * instructions and hid everything they point at, which is also how a stale import went unnoticed for twelve
 * days: the missing files were never on screen to be missed.
 */
function FileTree({
  root,
  onOpen,
  openPath,
}: {
  root: TreeNode;
  onOpen: (n: TreeNode) => void;
  openPath: string | null;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(() => defaultExpanded(root));
  useEffect(() => setExpanded(defaultExpanded(root)), [root]);

  const toggle = (path: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const row = (n: TreeNode, depth: number): React.ReactNode => {
    const isDir = Boolean(n.children);
    const isOpen = expanded.has(n.path);
    const selected = !isDir && n.path === openPath;
    return (
      <div key={`${n.path}:${isDir ? 'd' : 'f'}`}>
        <button
          onClick={() => (isDir ? toggle(n.path) : onOpen(n))}
          className={`w-full flex items-center gap-1.5 py-1 px-2 text-left text-xs transition-colors ${
            selected ? 'bg-background-accent text-white' : 'hover:bg-background-secondary'
          }`}
          style={{ paddingLeft: 8 + depth * 14 }}
        >
          {isDir ? (
            <>
              {isOpen ? (
                <ChevronDown className="h-3 w-3 shrink-0" />
              ) : (
                <ChevronRight className="h-3 w-3 shrink-0" />
              )}
              {isOpen ? (
                <FolderOpen className="h-3.5 w-3.5 shrink-0 text-[#2e8bff]" />
              ) : (
                <Folder className="h-3.5 w-3.5 shrink-0 text-[#2e8bff]" />
              )}
              <span className="truncate font-medium">{n.name}</span>
              <span
                className={`ml-auto shrink-0 text-[10px] ${selected ? 'text-white/70' : 'text-text-tertiary'}`}
              >
                {n.fileCount}
              </span>
            </>
          ) : (
            <>
              <span className="w-3 shrink-0" />
              <FileText className="h-3.5 w-3.5 shrink-0 text-text-tertiary" />
              <span className="truncate">{n.name}</span>
            </>
          )}
        </button>
        {isDir && isOpen && n.children!.map((c) => row(c, depth + 1))}
      </div>
    );
  };

  return <div className="border border-borderSubtle">{root.children!.map((c) => row(c, 0))}</div>;
}

export function SkillDetail({
  entry,
  origin,
  projectDir,
  onSaved,
  onDeleted,
}: {
  entry: SourceEntry;
  origin: string;
  projectDir: string;
  onSaved: (updated: SourceEntry) => void;
  onDeleted: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openFile, setOpenFile] = useState<{ path: string; abs: string; body: string } | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const persona = isPersonaPath(entry.path);
  const editable = isEditable(entry);
  const tree = useMemo(
    () => buildSkillTree(entry.path, entry.supportingFiles ?? []),
    [entry.path, entry.supportingFiles]
  );

  useEffect(() => {
    setEditing(false);
    setError(null);
    setOpenFile(null);
    setFileError(null);
  }, [entry.path]);

  // Read a supporting file straight off disk. There is no ACP verb for "read an arbitrary file inside a
  // skill" — sources/export returns the SKILL.md body only — so this goes through the same main-process
  // reader the import scanner already uses.
  const openSupporting = useCallback(async (n: TreeNode) => {
    if (!n.abs) return;
    setFileError(null);
    try {
      const res = await window.electron.readFile(n.abs);
      if (!res.found || res.file === undefined) throw new Error(res.error || 'could not read the file');
      setOpenFile({ path: n.path, abs: n.abs, body: res.file });
    } catch (e) {
      setOpenFile(null);
      setFileError(errorMessage(e, `Could not open ${n.path}`));
    }
  }, []);

  const doDelete = async () => {
    setDeleting(true);
    try {
      await deleteSkillSource(entry.path);
      setConfirmDelete(false);
      onDeleted();
    } catch (e) {
      setError(errorMessage(e, 'Failed to delete'));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="pb-4 border-b border-borderSubtle">
        <div className="flex items-center gap-2 mb-2">
          <OriginBadge origin={origin} />
          <h2 className="text-xl truncate">{entry.name}</h2>
        </div>
        <p className="text-sm text-text-secondary mb-2">{entry.description}</p>
        <p className="text-[11px] font-mono text-text-tertiary break-all">{entry.path}</p>
        <div className="flex gap-2 mt-3">
          {editable && (
            <Button size="sm" variant={editing ? 'default' : 'outline'} onClick={() => setEditing(!editing)}>
              {editing ? 'Done editing' : persona ? 'Correct this' : 'Edit'}
            </Button>
          )}
          {editable && (
            <Button size="sm" variant="outline" onClick={() => setConfirmDelete(true)}>
              Delete
            </Button>
          )}
          {!editable && (
            <span className="text-xs text-text-tertiary self-center">
              Built-in skills ship with goose and cannot be edited or deleted.
            </span>
          )}
        </div>
        {error && <p className="mt-2 text-xs font-bold text-[#dc2626]">{error}</p>}
      </div>

      <ScrollArea className="flex-1 min-h-0">
        <div className="py-4">
          {persona && (
            <div className="mb-4 p-3 bg-[#7c3aed] text-white">
              <p className="text-xs font-bold mb-1">Goose wrote this about itself.</p>
              <p className="text-xs">
                It was written after a build of this stack that the engine proved compiled and passed its
                checks — but the lesson was phrased by a local model and can still be wrong. Everything except
                your own notes is rewritten after the next successful build of this stack.
              </p>
            </div>
          )}

          {editing && editable ? (
            persona ? (
              <PersonaEditor entry={entry} projectDir={projectDir} onSaved={onSaved} />
            ) : (
              <BodyEditor entry={entry} projectDir={projectDir} onSaved={onSaved} />
            )
          ) : openFile ? (
            <div>
              <div className="flex items-center justify-between mb-2">
                <span className="font-mono text-xs text-text-secondary truncate">{openFile.path}</span>
                <Button size="sm" variant="outline" onClick={() => setOpenFile(null)}>
                  Back to SKILL.md
                </Button>
              </div>
              {/* Verbatim, in a mono block. A supporting file is a script or a data file — rendering it as
                  markdown would silently eat its indentation, which for a Python script is its meaning. */}
              <pre className="p-3 bg-background-default border border-borderSubtle text-xs font-mono overflow-x-auto whitespace-pre">
                {openFile.body}
              </pre>
            </div>
          ) : (
            <div className="prose-sm max-w-none">
              <MarkdownContent content={entry.content} />
            </div>
          )}

          {/* The skill's other files. A skill is a DIRECTORY: the SKILL.md above tells the agent to run
              scripts/monitor.py and read references/strategy.md, and until now this view showed the
              instructions while hiding every file they point at. */}
          {tree.fileCount > 0 && !editing && (
            <div className="mt-6">
              <h3 className="text-[10px] font-bold tracking-wider text-text-tertiary mb-2">
                FILES · {tree.fileCount}
              </h3>
              {fileError && <p className="mb-2 text-xs font-bold text-[#dc2626]">{fileError}</p>}
              <FileTree root={tree} onOpen={openSupporting} openPath={openFile?.path ?? null} />
            </div>
          )}
        </div>
      </ScrollArea>

      <ConfirmationModal
        isOpen={confirmDelete}
        title={persona ? `Make goose forget ${entry.name}?` : `Delete ${entry.name}?`}
        message={
          persona
            ? 'This deletes the lesson and resets what goose has learned about this stack, including your notes. It will start from scratch on the next build.'
            : 'This permanently deletes the skill and its folder. There is no undo.'
        }
        detail={<span className="font-mono">{entry.path}</span>}
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
