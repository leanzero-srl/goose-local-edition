/**
 * Where a stored memory came from, read from what goose actually records — nothing inferred.
 *
 * The memory store (crates/goose-memory-store) keeps only category, scope, tags and text per entry.
 * One more record exists: a memory the agent PROPOSED and a person SAVED leaves its proposal on
 * disk under `<config>/proposals/<key>.json` with `state: "saved"`, the text as saved, the agent's
 * one-line reason, and when it was asked. `answer()` stores the edited text on the proposal and
 * `remember()` writes that same text (newline-trimmed) into the category file, so an entry whose
 * category, scope and text equal a saved proposal's is that proposal's memory. An entry edited
 * afterwards no longer matches, and then no origin is claimed.
 */

export interface MemoryOrigin {
  /** The proposal file's key: an ACP session id, or `wd-<hash>` for a working-directory proposal. */
  key: string;
  /** Epoch seconds the agent proposed it. */
  proposedAt: number;
  why: string;
  polarity?: 'positive' | 'negative';
}

interface StoredProposal {
  text?: unknown;
  why?: unknown;
  category?: unknown;
  is_global?: unknown;
  created_at?: unknown;
  state?: unknown;
  polarity?: unknown;
}

const originKey = (category: string, global: boolean, text: string) =>
  `${global ? 'g' : 'l'}\u0000${category}\u0000${text.replace(/^\n+|\n+$/g, '')}`;

/** Index every SAVED proposal in the given files by (scope, category, text). A file that does not
 *  parse as a proposal list is named in `unreadable` so the caller can log it — never read as empty. */
export function indexSavedProposals(files: ReadonlyArray<{ key: string; json: string }>): {
  index: Map<string, MemoryOrigin>;
  unreadable: string[];
} {
  const out = new Map<string, MemoryOrigin>();
  const unreadable: string[] = [];
  for (const { key, json } of files) {
    let rows: unknown;
    try {
      rows = JSON.parse(json);
    } catch {
      unreadable.push(key);
      continue;
    }
    if (!Array.isArray(rows)) {
      unreadable.push(key);
      continue;
    }
    for (const raw of rows as StoredProposal[]) {
      if (raw?.state !== 'saved') continue;
      if (typeof raw.text !== 'string' || typeof raw.category !== 'string') continue;
      const origin: MemoryOrigin = {
        key,
        proposedAt: typeof raw.created_at === 'number' ? raw.created_at : 0,
        why: typeof raw.why === 'string' ? raw.why.trim() : '',
        polarity:
          raw.polarity === 'positive' || raw.polarity === 'negative' ? raw.polarity : undefined,
      };
      // The same lesson saved from two sessions is ONE entry (`remember` answers Unchanged the
      // second time): the session that first proposed it is where it came from.
      const id = originKey(raw.category, raw.is_global === true, raw.text);
      const seen = out.get(id);
      if (!seen || origin.proposedAt < seen.proposedAt) out.set(id, origin);
    }
  }
  return { index: out, unreadable };
}

export function originOf(
  index: Map<string, MemoryOrigin>,
  entry: { category: string; scope: 'global' | 'local'; content: string }
): MemoryOrigin | undefined {
  return index.get(originKey(entry.category, entry.scope === 'global', entry.content));
}

/** A proposal key names a chat session unless it is the working-directory key. */
export function isSessionKey(key: string): boolean {
  return !key.startsWith('wd-');
}

/**
 * The tags that say where an entry came from, written as `<how>:<source>` — the Claude Code
 * importer tags every entry `imported:claude-code`. Returned apart from the entry's other tags.
 */
export function splitSourceTags(tags: readonly string[]): { sources: string[]; rest: string[] } {
  const sources: string[] = [];
  const rest: string[] = [];
  for (const t of tags) (/^imported:/.test(t) ? sources : rest).push(t);
  return { sources, rest };
}

/** "imported:claude-code" → "Imported from Claude Code". */
export function describeSourceTag(tag: string): string {
  const [how, ...from] = tag.split(':');
  const source = from
    .join(':')
    .split(/[-_]+/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ');
  const verb = how.charAt(0).toUpperCase() + how.slice(1);
  return source ? `${verb} from ${source}` : verb;
}
