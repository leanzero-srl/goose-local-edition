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
}

export type VerdictKind = 'fitsByGooseOwned' | 'fitsWithApps' | 'cannotFit';

export interface MemfreeVerdict {
  kind: VerdictKind | string;
  remainingBytes: number;
  appsToQuit: string[];
}

export type MemfreeAlternative =
  | { kind: 'smallerVariant'; modelId: string; needBytes: number }
  | { kind: 'peerMac'; nodeId: string; label: string; budgetBytes: number };

/** `_goose/unstable/mlxEngine/memoryHolders` (camelCase), the fields this side reads. */
export interface MemoryHolders {
  availableBytes: number;
  budgetBytes: number;
  totalBytes: number;
  groups: MemfreeGroup[];
  gooseOwnedBytes: number;
  appsBytes: number;
  shortBytes?: number | null;
  verdict?: MemfreeVerdict | null;
  alternatives: MemfreeAlternative[] | { kind: string }[];
  quitAppsAllowed: boolean;
  /** The engine's own reading of GOOSE_MEMFREE_ENABLED (absent on an older engine = on). */
  enabled?: boolean;
}

export interface MemfreeItem {
  id: string;
  label: string;
  ok: boolean;
  freedBytes?: number | null;
  message: string;
}

/** `_goose/unstable/mlxEngine/memoryFree`, the fields this side reads. */
export interface MemoryFreeResult {
  items: MemfreeItem[];
  before: { budgetBytes: number };
  after: { budgetBytes: number };
  fit?: { status?: string | null; shortBytes?: number | null; budgetBytes?: number } | null;
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
  | { kind: 'fitsWithApps'; freeGb: number; apps: string[] }
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

/** Before → after of one free, in GB, from the engine's own re-reading. */
export function afterLine(r: MemoryFreeResult): {
  beforeGb: number;
  afterGb: number;
  freedGb: number;
  fits: boolean | null;
  shortGb: number | null;
} {
  const status = r.fit?.status ?? null;
  const short = r.fit?.shortBytes ?? null;
  return {
    beforeGb: gib(r.before.budgetBytes),
    afterGb: gib(r.after.budgetBytes),
    freedGb: gib(Math.max(0, r.after.budgetBytes - r.before.budgetBytes)),
    fits: status == null ? null : status !== 'short',
    shortGb: status === 'short' && short != null ? gib(short) : null,
  };
}

/** goose config keys (Settings › App); the engine reads the same keys. */
export const MEMFREE_ENABLED_KEY = 'GOOSE_MEMFREE_ENABLED';
export const MEMFREE_SHOW_APPS_MIN_MB_KEY = 'GOOSE_MEMFREE_SHOW_APPS_MIN_MB';
export const LINK_ALLOW_REMOTE_APP_QUIT_KEY = 'GOOSE_LINK_ALLOW_REMOTE_APP_QUIT';
export const MEMFREE_SHOW_APPS_MIN_MB_DEFAULT = 200;

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
