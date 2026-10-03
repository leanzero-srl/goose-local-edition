/**
 * A scorer check's `parts` map, reduced to words a person can read on the run card.
 *
 * MEASURED 2026-10-03: sb-7.2's m_committed_event_replay carries 156 KB of nested parts (corroboration,
 * live, replay, semantics, clockEvidence, motionLegs) and the card rendered "corroboration [object
 * Object]". A nested object becomes a GROUP — its verdict (`ok`) as the tone, its fields one level down
 * as key: value items; anything deeper is COUNTED, never stringified. Text is bounded before it is built
 * (a 120-id list is counted, not joined), so a large part costs the same as a small one.
 */

export type PartLeaf =
  | { key: string; kind: 'flag'; ok: boolean }
  | { key: string; kind: 'text'; text: string; tone?: 'ok' | 'err' };

export type PartView =
  | PartLeaf
  | { key: string; kind: 'group'; ok: boolean | null; items: PartLeaf[]; more: number };

const TEXT_MAX = 48;
const JOIN_MAX_ITEMS = 6;
const GROUP_MAX_ITEMS = 8;

const clip = (text: string) => (text.length > TEXT_MAX ? `${text.slice(0, TEXT_MAX - 1)}…` : text);

const isRecord = (v: unknown): v is Record<string, unknown> =>
  v != null && typeof v === 'object' && !Array.isArray(v);

const isScalar = (v: unknown) =>
  v == null || typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean';

function scalarText(v: unknown): string {
  if (v == null) return 'none';
  if (typeof v === 'number') return Number.isInteger(v) ? String(v) : v.toFixed(2);
  return String(v);
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

function arrayLeaf(key: string, list: unknown[]): PartLeaf {
  if (list.length === 0) return { key, kind: 'text', text: 'none' };
  const verdicts = list.map((item) => (isRecord(item) ? item.ok : undefined));
  if (verdicts.every((ok) => typeof ok === 'boolean')) {
    const passed = verdicts.filter(Boolean).length;
    return {
      key,
      kind: 'text',
      text: `${passed}/${list.length} ok`,
      tone: passed === list.length ? 'ok' : 'err',
    };
  }
  if (list.length <= JOIN_MAX_ITEMS && list.every(isScalar))
    return { key, kind: 'text', text: clip(list.map(scalarText).join(', ')) };
  return { key, kind: 'text', text: plural(list.length, 'item') };
}

/** One value as a single chip: a flag, a number, a clipped string, a counted list, or an object's verdict. */
export function partLeaf(key: string, value: unknown): PartLeaf {
  if (typeof value === 'boolean') return { key, kind: 'flag', ok: value };
  if (Array.isArray(value)) return arrayLeaf(key, value);
  if (isRecord(value)) {
    if (typeof value.ok === 'boolean')
      return { key, kind: 'text', text: value.ok ? 'passed' : 'failed', tone: value.ok ? 'ok' : 'err' };
    const entries = Object.entries(value);
    if (entries.length === 0) return { key, kind: 'text', text: 'none' };
    if (entries.every(([, v]) => isScalar(v)))
      return {
        key,
        kind: 'text',
        text: clip(entries.map(([k, v]) => `${k} ${scalarText(v)}`).join(' · ')),
      };
    return { key, kind: 'text', text: plural(entries.length, 'field') };
  }
  return { key, kind: 'text', text: clip(scalarText(value)) };
}

const isVerdict = (leaf: PartLeaf) => leaf.kind === 'flag' || leaf.tone != null;

/** The whole parts map: scalars and lists as chips, each nested object as a group of its own fields. */
export function summarizeParts(parts: Record<string, unknown>): PartView[] {
  return Object.entries(parts).map(([key, value]): PartView => {
    if (!isRecord(value)) return partLeaf(key, value);
    const fields = Object.entries(value).filter(([k]) => k !== 'ok');
    if (fields.length === 0) return partLeaf(key, value);
    // A field carrying its own verdict outranks a descriptive one, so the cap never hides a failure
    // behind "+N more" (live.visible failed was the ninth field of the real replay check).
    const leaves = fields.map(([k, v]) => partLeaf(k, v));
    const verdicts = leaves.filter(isVerdict);
    const ordered = [...verdicts, ...leaves.filter((leaf) => !isVerdict(leaf))];
    return {
      key,
      kind: 'group',
      ok: typeof value.ok === 'boolean' ? value.ok : null,
      items: ordered.slice(0, Math.max(GROUP_MAX_ITEMS, verdicts.length)),
      more: Math.max(0, ordered.length - Math.max(GROUP_MAX_ITEMS, verdicts.length)),
    };
  });
}
