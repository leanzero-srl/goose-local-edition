import type { SourceEntry } from '@aaif/goose-sdk';

/**
 * The heading the swarm engine preserves verbatim across its own rewrites.
 *
 * MUST stay byte-identical to `PERSONA_USER_MARKER` in crates/goose-cli/src/commands/swarm.rs. The engine
 * lifts everything after this heading out of the old file and re-emits it when it regenerates a persona; if
 * these two strings ever drift, this editor writes a correction into a section the engine no longer reads and
 * the next successful build silently deletes it. There is no runtime link between the two — only this comment
 * and the tests either side of it.
 */
export const PERSONA_USER_MARKER = '## Your notes';

/**
 * Is this skill one goose wrote about itself, rather than one a human wrote?
 *
 * The predicate is the PATH, because that is what the engine's is: `persona_dir(stack_key)` resolves to
 * `<config>/skills/stack-<key>` as a pure function of the stack key, and the engine writes there regardless
 * of the file's contents. Matching on the marker instead would be a strictly narrower test than the engine's
 * own, so a file the engine WILL rewrite could render here as an ordinary, safely-editable skill.
 */
export function isPersonaPath(path: string): boolean {
  return /(?:^|\/)skills\/stack-[^/]+\/?$/.test(path);
}

/**
 * Can this entry actually be written back?
 *
 * NOT `entry.writable` — that field is hardcoded `true` in skills/mod.rs:357 and `builtin_skill_entry` never
 * clears it, so every built-in claims to be writable and the backend then refuses the update in
 * `require_mutable_type`. The wire lies; the source type is the truth.
 */
export function isEditable(entry: Pick<SourceEntry, 'type'>): boolean {
  return entry.type === 'skill';
}

export interface PersonaZones {
  /** Regenerated wholesale by the next successful build of this stack. Editing it here is writing in sand. */
  engine: string;
  /** Preserved verbatim by the engine. `null` when the file has no notes heading at all. */
  notes: string | null;
}

/**
 * Split a persona into the region goose owns and the region the user owns.
 *
 * Matches the marker only as a heading at the start of a line: the engine's own banner NAMES `## Your notes`
 * inline while telling the user where to write, and splitting on the first textual hit would classify goose's
 * entire lesson as the user's notes — which the editor would then write back into the preserved section,
 * where it would compound on every rewrite. This mirrors the same fix in the Rust `persona_user_notes`.
 */
export function splitPersona(content: string): PersonaZones {
  const heading = `\n${PERSONA_USER_MARKER}`;
  const at = content.indexOf(heading);
  if (at === -1) {
    return { engine: content, notes: null };
  }
  return {
    engine: content.slice(0, at),
    // `.trim()` mirrors the Rust `persona_user_notes` exactly. If this side kept surrounding whitespace the
    // engine's copy would not, so the two would disagree about what the user's note IS — and a save here
    // would look like an edit to the engine on every round-trip even when nothing changed.
    notes: content.slice(at + heading.length).trim(),
  };
}

/**
 * Put a persona back together after the user has edited ONLY their own section.
 *
 * The engine region is passed through untouched. That is deliberate: the round-trip must not be the thing
 * that reformats goose's text, or a save would show up as a spurious edit the next time anything diffs it.
 */
export function recomposePersona(engine: string, notes: string): string {
  return `${engine.replace(/\s+$/, '')}\n\n${PERSONA_USER_MARKER}\n\n${notes.trim()}\n`;
}

/** Where a skill came from, for the reader who has to tell three roots apart at a glance. */
export type SkillOrigin = 'persona' | 'builtin' | 'global' | 'project';

export function skillOrigin(entry: Pick<SourceEntry, 'type' | 'path' | 'global'>): SkillOrigin {
  if (entry.type === 'builtinSkill') return 'builtin';
  if (isPersonaPath(entry.path)) return 'persona';
  return entry.global ? 'global' : 'project';
}
