/**
 * "Free up memory" (docs/memfree/RESEARCH.md §4.7, HANDOFF.md "Coordinator decisions"): the pure
 * part of the desktop half. The engine measures every number (phys_footprint per process, the fit
 * budget, the verdict); this file only turns its answer into what the dialog and the card say, so
 * it can be tested without rendering. Nothing here re-measures or second-guesses the engine — the
 * one fallback (`verdictOf`) mirrors the engine's own rule for an older engine that sends no verdict.
 */

export const GIB = 1024 * 1024 * 1024;

/** Where a holder of memory comes from, in the order the dialog lists them. */
export const CATEGORY_ORDER = [
  'gooseEngine',
  'gooseOrphan',
  'aiServer',
  'job',
  'userApp',
  'system',
] as const;
export type MemfreeCategory = (typeof CATEGORY_ORDER)[number];

export interface MemfreePid {
  pid: number;
  startedAt: number;
}

/** One holder: an app bundle (helpers folded in), an engine, an orphaned goose serve, a model server. */
export interface MemfreeGroup {
  id: string;
  label: string;
  category: MemfreeCategory | string;
  bytes: number;
  pids: MemfreePid[];
  bundleId?: string | null;
  quittable: boolean;
  reclaimableByGoose: boolean;
  why?: string | null;
  /** Set when nothing may free it (launchd job, MCP server, terminal session, …): never offered. */
  protectedWhy?: string | null;
  /** Who uses it: TCP clients of a server's port, the process that started it, its launchd job. */
  usedBy?: string[];
  launchdLabel?: string | null;
  /** Ticked by default in "Free everything except protected" (never when protected or in use,
   * never for a server goose did not start). */
  nuclearDefault?: boolean;
}

export type VerdictKind = 'fitsByGooseOwned' | 'fitsWithApps' | 'cannotFit';

export interface MemfreeVerdict {
  kind: VerdictKind | string;
  remainingBytes: number;
  appsToQuit: string[];
}

export type MemfreeAlternative =
  | { kind: 'smallerVariant'; modelId: string; needBytes: number }
  /** budgetBytes: free memory there from its Link load report; absent when it sent none. */
  | { kind: 'peerMac'; nodeId: string; label: string; budgetBytes?: number };

/** The fields this side reads of the engine's MlxMountFitDto (`verdict` is "allow" | "warn" | "block"). */
export interface MemfreeFit {
  modelId?: string;
  verdict?: string | null;
  needBytes?: number;
  budgetBytes?: number;
  /** Block only: needBytes - budgetBytes. */
  shortBytes?: number | null;
}

/** `_goose/unstable/mlxEngine/memoryHolders` (camelCase), the fields this side reads. */
export interface MemoryHolders {
  availableBytes: number;
  budgetBytes: number;
  totalBytes: number;
  groups: MemfreeGroup[];
  gooseOwnedBytes: number;
  /** What unloading the unprotected model servers / stopping other MLX engines would free. */
  serversBytes?: number;
  appsBytes: number;
  shortBytes?: number | null;
  verdict?: MemfreeVerdict | null;
  alternatives: MemfreeAlternative[] | { kind: string }[];
  quitAppsAllowed: boolean;
  fit?: MemfreeFit | null;
  /** The engine's own reading of GOOSE_MEMFREE_ENABLED (absent on an older engine = on). */
  enabled?: boolean;
}

export interface MemfreeItem {
  id: string;
  label: string;
  ok: boolean;
  freedBytes?: number | null;
  message: string;
  /** An app that did not quit and that goose could stop; only an explicit Force stop does. */
  forceStoppable?: boolean;
}

/** `_goose/unstable/mlxEngine/memoryFree`, the fields this side reads. */
export interface MemoryFreeResult {
  items: MemfreeItem[];
  before: { budgetBytes: number };
  after: { budgetBytes: number };
  fit?: MemfreeFit | null;
}

export const gib = (bytes: number) => bytes / GIB;

/**
 * The engine's verdict rule, mirrored for an engine that sends none: goose's own memory alone
 * covers the shortfall, or goose's own plus the user apps (largest first, only as many as it
 * takes), or not even everything — then how much is still missing.
 */
export function verdictOf(
  shortBytes: number,
  gooseOwnedBytes: number,
  apps: readonly { bundleId?: string | null; bytes: number }[]
): MemfreeVerdict {
  const appsBytes = apps.reduce((sum, a) => sum + a.bytes, 0);
  if (gooseOwnedBytes >= shortBytes) {
    return { kind: 'fitsByGooseOwned', remainingBytes: 0, appsToQuit: [] };
  }
  if (gooseOwnedBytes + appsBytes >= shortBytes) {
    const appsToQuit: string[] = [];
    let freed = gooseOwnedBytes;
    for (const app of [...apps].sort((a, b) => b.bytes - a.bytes)) {
      if (freed >= shortBytes) break;
      if (app.bundleId) appsToQuit.push(app.bundleId);
      freed += app.bytes;
    }
    return { kind: 'fitsWithApps', remainingBytes: 0, appsToQuit };
  }
  return {
    kind: 'cannotFit',
    remainingBytes: shortBytes - gooseOwnedBytes - appsBytes,
    appsToQuit: [],
  };
}

/** Quittable user apps, the ones a verdict may name. */
export function userApps(groups: readonly MemfreeGroup[]): MemfreeGroup[] {
  return groups.filter((g) => g.category === 'userApp' && g.quittable && !!g.bundleId);
}

/** What the headline of the dialog (and the card's hint) says, every number in GB. */
export type VerdictLine =
  | { kind: 'fitsByGooseOwned'; freeGb: number }
  /** `withGoose`: goose's own memory is part of it. False when goose holds nothing to free, so
   * the line names only the apps (it never credits a source that frees 0). */
  | { kind: 'fitsWithApps'; freeGb: number; apps: string[]; withGoose: boolean }
  | {
      kind: 'cannotFit';
      everythingGb: number;
      shortGb: number;
      alternatives: MemfreeAlternative[];
    };

/**
 * The verdict in words. null when nothing is short (the model already fits, or the engine did
 * not size a model). "Free up X GB" names what the one action frees: goose's own memory, plus the
 * named apps when those are needed too.
 */
export function verdictLine(h: MemoryHolders): VerdictLine | null {
  const short = h.shortBytes ?? 0;
  if (!(short > 0)) return null;
  const apps = userApps(h.groups);
  const verdict = h.verdict ?? verdictOf(short, h.gooseOwnedBytes, apps);
  switch (verdict.kind) {
    case 'fitsByGooseOwned':
      return { kind: 'fitsByGooseOwned', freeGb: gib(h.gooseOwnedBytes) };
    case 'fitsWithApps': {
      const named = verdict.appsToQuit
        .map((bid) => apps.find((a) => a.bundleId === bid))
        .filter((a): a is MemfreeGroup => !!a);
      const appBytes = named.reduce((sum, a) => sum + a.bytes, 0);
      return {
        kind: 'fitsWithApps',
        freeGb: gib(h.gooseOwnedBytes + appBytes),
        apps: named.length > 0 ? named.map((a) => a.label) : verdict.appsToQuit,
        withGoose: h.gooseOwnedBytes > 0,
      };
    }
    default:
      return {
        kind: 'cannotFit',
        everythingGb: gib(h.gooseOwnedBytes + h.appsBytes),
        shortGb: gib(verdict.remainingBytes),
        alternatives: knownAlternatives(h.alternatives),
      };
  }
}

function knownAlternatives(list: MemoryHolders['alternatives']): MemfreeAlternative[] {
  return (list as { kind: string }[]).filter(
    (a): a is MemfreeAlternative => a.kind === 'smallerVariant' || a.kind === 'peerMac'
  );
}

/** One category's holders, biggest first, with their total. Empty categories are left out. */
export interface CategoryBlock {
  category: string;
  bytes: number;
  groups: MemfreeGroup[];
}

export function groupByCategory(groups: readonly MemfreeGroup[]): CategoryBlock[] {
  const order = (c: string) => {
    const i = (CATEGORY_ORDER as readonly string[]).indexOf(c);
    return i < 0 ? CATEGORY_ORDER.length : i;
  };
  const byCat = new Map<string, MemfreeGroup[]>();
  for (const g of groups) {
    const list = byCat.get(g.category) ?? [];
    list.push(g);
    byCat.set(g.category, list);
  }
  return [...byCat.entries()]
    .map(([category, list]) => ({
      category,
      bytes: list.reduce((sum, g) => sum + g.bytes, 0),
      groups: [...list].sort((a, b) => b.bytes - a.bytes),
    }))
    .sort((a, b) => order(a.category) - order(b.category));
}

/**
 * Whether a holder gets its own Quit button, and whether it is pressable. Only a quittable user app
 * with a bundle id is ever offered; system holders, goose's own and model servers never are. A peer
 * that does not allow remote app quit shows the button disabled, with the reason.
 */
export function quitOffer(
  group: MemfreeGroup,
  quitAppsAllowed: boolean
): { show: false } | { show: true; disabled: boolean } {
  if (group.category !== 'userApp' || !group.quittable || !group.bundleId) return { show: false };
  return { show: true, disabled: !quitAppsAllowed };
}

/**
 * A model server (LM Studio, Ollama) or another MLX engine (`mlx:<pid>`): freed only when the person
 * names it (`unloadServers`). Mirrors the engine's `is_server`.
 */
export function isServerGroup(group: MemfreeGroup): boolean {
  return (
    group.category === 'aiServer' ||
    (group.category === 'gooseEngine' && group.id.startsWith('mlx:'))
  );
}

/**
 * Whether a server gets its own Unload button, and whether it is pressable. Never for a protected
 * one or one goose cannot free now; a peer without remote app quit shows it disabled.
 */
export function unloadOffer(
  group: MemfreeGroup,
  allowed: boolean
): { show: false } | { show: true; disabled: boolean } {
  if (!isServerGroup(group) || group.protectedWhy || !group.reclaimableByGoose) {
    return { show: false };
  }
  return { show: true, disabled: !allowed };
}

/** The soft clean's own kinds: goose's idle engine and its proven left-over servers. */
export function isGooseOwnGroup(group: MemfreeGroup): boolean {
  return (
    (group.id === 'gooseEngine' && group.reclaimableByGoose) || group.category === 'gooseOrphan'
  );
}

/** Built into the engine (memfree_protect::DEFAULT_NEVER_TOUCH); the setting can only add. */
export const DEFAULT_NEVER_TOUCH: readonly string[] = [
  'com.microsoft.teams2',
  'com.microsoft.teams',
  'com.microsoft.Outlook',
  'com.tinyspeck.slackmacgap',
  'us.zoom.xos',
  'com.apple.Terminal',
  'com.googlecode.iterm2',
  'dev.warp.Warp-Stable',
];

/**
 * The entries the person added (GOOSE_MEMFREE_NEVER_TOUCH): a list, or a JSON / comma-separated
 * string as an env var sets it. Trimmed, de-duplicated case-insensitively, defaults dropped.
 */
export function neverTouchExtrasFrom(value: unknown): string[] {
  let raw: unknown[] = [];
  if (Array.isArray(value)) raw = value;
  else if (typeof value === 'string') {
    const text = value.trim();
    if (text.startsWith('[')) {
      try {
        const parsed: unknown = JSON.parse(text);
        if (Array.isArray(parsed)) raw = parsed;
      } catch {
        raw = [];
      }
    } else raw = text.split(',');
  }
  const out: string[] = [];
  const seen = new Set(DEFAULT_NEVER_TOUCH.map((d) => d.toLowerCase()));
  for (const item of raw) {
    if (typeof item !== 'string') continue;
    const name = item.trim();
    if (!name || seen.has(name.toLowerCase())) continue;
    seen.add(name.toLowerCase());
    out.push(name);
  }
  return out;
}

/** Defaults first, then the person's additions: what a request's `neverTouch` carries. */
export function effectiveNeverTouch(extras: readonly string[]): string[] {
  return [...DEFAULT_NEVER_TOUCH, ...neverTouchExtrasFrom([...extras])];
}

/**
 * The never-touch entry a group matches (bundle id, label, the name in its id), like the engine's
 * `never_touch_hit`: case-insensitive, equal or contained, so "Teams" holds "Microsoft Teams" and
 * "com.microsoft" holds every bundle id under it.
 */
export function neverTouchHit(group: MemfreeGroup, list: readonly string[]): string | null {
  const idName = group.id.includes(':') ? group.id.slice(group.id.indexOf(':') + 1) : '';
  const appName = idName.endsWith('.app')
    ? (idName.split('/').pop() ?? '').replace(/\.app$/, '')
    : null;
  const names = [group.bundleId, group.label.replace(/ \(build\)$/, ''), idName, appName]
    .filter((n): n is string => typeof n === 'string')
    .map((n) => n.trim().toLowerCase())
    .filter((n) => n !== '');
  return (
    list.find((entry) => {
      const e = entry.trim().toLowerCase();
      return e !== '' && names.some((n) => n === e || n.includes(e));
    }) ?? null
  );
}

/** One holder in the "Free everything except protected" view. */
export type NuclearRow =
  | { kind: 'offer'; group: MemfreeGroup; ticked: boolean }
  | { kind: 'protected'; group: MemfreeGroup; reason: string }
  | { kind: 'left'; group: MemfreeGroup; reason: string | null };

/**
 * Sort every holder for the nuclear view. Protected (engine reason, or on the never-touch list)
 * is never offered. Offered = a quittable app or a server goose can free now; ticked by default
 * only when the engine says so (`nuclearDefault`: nobody uses it), and a server never — goose did
 * not start it, so the person ticks it themselves (even if an older engine says otherwise). goose's own kinds are the
 * separate "goose's own" row (`gooseOwned`); everything else is left alone with its reason.
 */
export function nuclearRows(
  groups: readonly MemfreeGroup[],
  neverTouch: readonly string[]
): NuclearRow[] {
  const rows: NuclearRow[] = [];
  for (const group of [...groups].sort((a, b) => b.bytes - a.bytes)) {
    if (isGooseOwnGroup(group)) continue;
    if (group.protectedWhy) {
      rows.push({ kind: 'protected', group, reason: group.protectedWhy });
      continue;
    }
    const hit = neverTouchHit(group, neverTouch);
    if (hit) {
      rows.push({ kind: 'protected', group, reason: hit });
      continue;
    }
    const offered =
      (group.category === 'userApp' && group.quittable && !!group.bundleId) ||
      (isServerGroup(group) && group.reclaimableByGoose);
    rows.push(
      offered
        ? { kind: 'offer', group, ticked: !isServerGroup(group) && group.nuclearDefault === true }
        : { kind: 'left', group, reason: group.why ?? null }
    );
  }
  return rows;
}

export interface NuclearFreeRequest {
  gooseOwned: boolean;
  quitApps: string[];
  unloadServers: string[];
  /** Each picked server's "used by" as the person saw it; the engine refuses one that gained a user. */
  seenUsedBy: Record<string, string[]>;
  /** Never filled by the nuclear view: a force stop is only ever its own per-app request. */
  forceStopApps: string[];
  nuclear: true;
  neverTouch: string[];
}

/**
 * The one `memoryFree` call the nuclear view sends: only what is ticked. A ticked id that is not
 * an offered row (protected, never-touch, left alone) is dropped here too — never selectable.
 */
export function nuclearRequest(
  rows: readonly NuclearRow[],
  ticked: ReadonlySet<string>,
  gooseOwn: boolean,
  neverTouch: readonly string[]
): NuclearFreeRequest {
  const quitApps: string[] = [];
  const unloadServers: string[] = [];
  const seenUsedBy: Record<string, string[]> = {};
  for (const row of rows) {
    if (row.kind !== 'offer' || !ticked.has(row.group.id)) continue;
    if (isServerGroup(row.group)) {
      unloadServers.push(row.group.id);
      seenUsedBy[row.group.id] = [...(row.group.usedBy ?? [])];
    } else if (row.group.bundleId) quitApps.push(row.group.bundleId);
  }
  return {
    gooseOwned: gooseOwn,
    quitApps,
    unloadServers,
    seenUsedBy,
    forceStopApps: [],
    nuclear: true,
    neverTouch: [...neverTouch],
  };
}

/** The bundle id of a result item that gets a Force stop button: an app the engine marked so. */
export function forceStopBundle(item: MemfreeItem): string | null {
  if (!item.forceStoppable || !item.id.startsWith('app:')) return null;
  return item.id.slice('app:'.length) || null;
}

/** Before → after of one free, in GB, from the engine's own re-reading. */
export function afterLine(r: MemoryFreeResult): {
  beforeGb: number;
  afterGb: number;
  freedGb: number;
  fits: boolean | null;
  shortGb: number | null;
} {
  const verdict = r.fit?.verdict ?? null;
  const short = r.fit?.shortBytes ?? null;
  return {
    beforeGb: gib(r.before.budgetBytes),
    afterGb: gib(r.after.budgetBytes),
    freedGb: gib(Math.max(0, r.after.budgetBytes - r.before.budgetBytes)),
    fits: verdict == null ? null : verdict !== 'block',
    shortGb: verdict === 'block' && short != null ? gib(short) : null,
  };
}

/** goose config keys (Settings › App); the engine reads the same keys. */
export const MEMFREE_ENABLED_KEY = 'GOOSE_MEMFREE_ENABLED';
export const MEMFREE_SHOW_APPS_MIN_MB_KEY = 'GOOSE_MEMFREE_SHOW_APPS_MIN_MB';
export const LINK_ALLOW_REMOTE_APP_QUIT_KEY = 'GOOSE_LINK_ALLOW_REMOTE_APP_QUIT';
export const MEMFREE_SHOW_APPS_MIN_MB_DEFAULT = 200;
export const MEMFREE_NEVER_TOUCH_KEY = 'GOOSE_MEMFREE_NEVER_TOUCH';

/** On unless the owner turned it off: only an explicit false / "false" is off. */
export function memfreeEnabledFrom(value: unknown): boolean {
  if (value === false) return false;
  if (typeof value === 'string' && value.trim().toLowerCase() === 'false') return false;
  return true;
}

/** Off unless explicitly on, read the way goose reads a boolean. */
export function boolOnFrom(value: unknown): boolean {
  return value === true || (typeof value === 'string' && value.trim().toLowerCase() === 'true');
}

/** A whole number of MB ≥ 0, else the default. */
export function minMbFrom(value: unknown): number {
  const n = typeof value === 'number' ? value : typeof value === 'string' ? Number(value) : NaN;
  return Number.isFinite(n) && n >= 0 ? Math.round(n) : MEMFREE_SHOW_APPS_MIN_MB_DEFAULT;
}
