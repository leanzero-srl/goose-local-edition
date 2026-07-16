import { useState, useEffect, useCallback } from 'react';
import type { SourceEntry } from '@aaif/goose-sdk';
import { Button } from '../ui/button';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { ScrollArea } from '../ui/scroll-area';
import MarkdownContent from '../MarkdownContent';
import { errorMessage } from '../../utils/conversionUtils';
import { updateSkillSource, deleteSkillSource, readSkillSourceFresh } from '../../acp/sources';
import { isEditable, isPersonaPath, splitPersona, recomposePersona } from './skillKinds';

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
  const persona = isPersonaPath(entry.path);
  const editable = isEditable(entry);

  useEffect(() => {
    setEditing(false);
    setError(null);
  }, [entry.path]);

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
          ) : (
            <div className="prose-sm max-w-none">
              <MarkdownContent content={entry.content} />
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
