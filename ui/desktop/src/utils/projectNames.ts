import { useMemo, useSyncExternalStore } from 'react';

/**
 * Q-485: a project is shown by its folder's name, and two folders named "work" are not two
 * projects the person can tell apart. Every surface that names a project (the Projects list, the
 * Active now subtitle, the landing, the composer's folder chip, the top bar's menu) reads its name
 * from HERE, so a collision is told apart the same way everywhere: VS Code's "work — RU-…-3w-…",
 * the nearest parent segment that differs, with the parts every colliding path shares elided.
 */

const SEPARATOR = /[\\/]/;
const DELIMITER = /[-_. ]/;

/** Trailing-slash-insensitive, so "/a/work/" and "/a/work" are one project. */
export function normalizeProjectPath(dir: string): string {
  const trimmed = dir.replace(/[\\/]+$/, '');
  return trimmed.length > 0 ? trimmed : '/';
}

/** Last path segment of a directory — the display name of a project. */
export function folderName(dir: string): string {
  const trimmed = dir.replace(/[\\/]+$/, '');
  const seg = trimmed.split(SEPARATOR).filter(Boolean).pop();
  return seg ?? trimmed;
}

export interface ProjectName {
  /** The folder's own name. */
  name: string;
  /** What tells it apart from a same-named project; absent when the name is unique. */
  hint?: string;
}

export function projectLabel({ name, hint }: ProjectName): string {
  return hint ? `${name} — ${hint}` : name;
}

function commonPrefix(values: readonly string[]): number {
  const first = values[0];
  let n = 0;
  while (n < first.length && values.every((v) => v[n] === first[n])) n += 1;
  return n;
}

function commonSuffix(values: readonly string[]): number {
  const reversed = values.map((v) => [...v].reverse().join(''));
  return commonPrefix(reversed);
}

/**
 * The part of each segment that differs, with the shared head and tail elided at a word boundary:
 * "RU-2026-09-28-3v-split-tensor-cafe" beside "RU-2026-09-29-3w-split-tensor-cafe" reads
 * "RU-…-28-3v-…" and "RU-…-29-3w-…", which still fits a sidebar row. A cut is only made at a
 * delimiter, so "projA" beside "projB" stays whole.
 */
function elide(segments: readonly string[]): string[] {
  const distinct = [...new Set(segments)];
  if (distinct.length < 2) return [...segments];
  let prefix = commonPrefix(distinct);
  while (prefix > 0 && !DELIMITER.test(distinct[0][prefix - 1])) prefix -= 1;
  let suffix = commonSuffix(distinct);
  const shortest = Math.min(...distinct.map((s) => s.length));
  suffix = Math.min(suffix, shortest - prefix);
  while (suffix > 0 && !DELIMITER.test(distinct[0][distinct[0].length - suffix])) suffix -= 1;
  return segments.map((segment) => {
    const middle = segment.slice(prefix, segment.length - suffix);
    if (middle.length === 0) return segment;
    const firstDelimiter = segment.search(DELIMITER);
    const head = firstDelimiter >= 0 ? segment.slice(0, firstDelimiter + 1) : '';
    const left = prefix === 0 ? '' : prefix > head.length ? `${head}…${segment[prefix - 1]}` : head;
    const right = suffix === 0 ? '' : `${segment[segment.length - suffix]}…`;
    const out = `${left}${middle}${right}`;
    return out.length < segment.length ? out : segment;
  });
}

/**
 * One name per distinct project path. A folder name held by one path is the name alone; paths
 * sharing a name each get the nearest parent segment that no other path in the group shares at
 * that depth ("x/…" when the nearest ones match and a higher one differs; "/" for the path that
 * runs out of parents first).
 */
export function distinctProjectNames(paths: Iterable<string>): Map<string, ProjectName> {
  const unique = [...new Set([...paths].map(normalizeProjectPath))];
  const byName = new Map<string, string[]>();
  for (const path of unique) {
    const name = folderName(path);
    byName.set(name, [...(byName.get(name) ?? []), path]);
  }
  const out = new Map<string, ProjectName>();
  for (const [name, group] of byName) {
    if (group.length === 1) {
      out.set(group[0], { name });
      continue;
    }
    const parentsOf = (path: string) =>
      path.split(SEPARATOR).filter(Boolean).slice(0, -1).reverse();
    const chosen = group.map((path) => {
      const parents = parentsOf(path);
      const others = group.filter((p) => p !== path).map(parentsOf);
      let depth = 1;
      const key = (list: string[], d: number) => list.slice(0, d).join('/');
      while (depth <= parents.length && others.some((o) => key(o, depth) === key(parents, depth))) {
        depth += 1;
      }
      return { path, depth, segment: parents[depth - 1] };
    });
    const elided = elide(chosen.map((c) => c.segment ?? ''));
    const hints = chosen.map((c, i) =>
      c.segment === undefined ? '/' : c.depth > 1 ? `${elided[i]}/…` : elided[i]
    );
    const collide = new Set(hints).size < hints.length;
    chosen.forEach((c, i) => {
      const hint = collide
        ? c.segment === undefined
          ? '/'
          : `${c.segment}${c.depth > 1 ? '/…' : ''}`
        : hints[i];
      out.set(c.path, { name, hint });
    });
  }
  return out;
}

/** The name of `path` among `known`; `path` itself always takes part, so the answer is never empty. */
export function projectNameIn(names: ReadonlyMap<string, ProjectName>, path: string): ProjectName {
  return names.get(normalizeProjectPath(path)) ?? { name: folderName(path) };
}

/*
 * The projects the window knows (the sidebar's derived list), published so a surface that sees
 * only one or two folders (Active now, the composer chip) tells a collision apart against the SAME
 * set the Projects list does.
 */
let knownPaths: readonly string[] = [];
const listeners = new Set<() => void>();

export function publishProjectPaths(paths: readonly string[]): void {
  const next = [...new Set(paths.map(normalizeProjectPath))].sort();
  if (next.length === knownPaths.length && next.every((p, i) => p === knownPaths[i])) return;
  knownPaths = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Test seam: the published set is module state. */
export function resetPublishedProjectPaths(): void {
  publishProjectPaths([]);
}

/**
 * Names for `paths`, told apart against every project the window knows. The paths asked about
 * take part too, so two same-named folders both in Active now are told apart even before the
 * Projects list has published.
 */
export function useProjectNames(paths: readonly string[]): (path: string) => ProjectName {
  const known = useSyncExternalStore(subscribe, () => knownPaths);
  const askedKey = paths.join('\n');
  return useMemo(() => {
    const names = distinctProjectNames([...known, ...askedKey.split('\n').filter(Boolean)]);
    return (path: string) => projectNameIn(names, path);
  }, [known, askedKey]);
}
