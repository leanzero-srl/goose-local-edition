import { ENGINE_PHASES, type EnginePhase } from '../components/lz/tokens';

/**
 * What the renderer hands MAIN about the linked Macs after every Link read (main owns no ACP
 * client): ONE line per Mac, in the words and the engine-phase colour My Macs shows for it, and the
 * localized label of the action that opens My Macs. Validated on arrival — IPC is a trust boundary.
 * `null` = not on LeanZero Link: the tray keeps the Link line instead.
 */
export interface MacTrayLine {
  /** The Mac's one name — My Macs's. */
  name: string;
  /** The engine phase; null = nothing an engine is doing is known (offline, switched off). */
  phase: EnginePhase | null;
  /** The whole menu line, as My Macs says it. */
  text: string;
}

export interface MacsTrayReport {
  lines: MacTrayLine[];
  openLabel: string;
}

/** A menu item stays one readable line; the full text lives on My Macs. */
export const MAC_TRAY_CHARS = 120;

export function clipTrayText(text: string): string {
  const one = text.replace(/\s+/g, ' ').trim();
  return one.length > MAC_TRAY_CHARS ? `${one.slice(0, MAC_TRAY_CHARS - 1)}…` : one;
}

function isLine(value: unknown): value is MacTrayLine {
  if (typeof value !== 'object' || value == null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.name === 'string' &&
    typeof r.text === 'string' &&
    (r.phase === null ||
      (typeof r.phase === 'string' && (ENGINE_PHASES as readonly string[]).includes(r.phase)))
  );
}

export function isMacsTrayReport(value: unknown): value is MacsTrayReport | null {
  if (value === null) return true;
  if (typeof value !== 'object' || value == null) return false;
  const r = value as Record<string, unknown>;
  return typeof r.openLabel === 'string' && Array.isArray(r.lines) && r.lines.every(isLine);
}

/**
 * Every window runs its own goosed and each reports; the tray shows the report that names the most
 * Macs — a window whose goose is not on the mesh reports none and never hides the one that is.
 */
export function pickMacsTrayReport(
  reports: Iterable<MacsTrayReport | null>
): MacsTrayReport | null {
  let best: MacsTrayReport | null = null;
  for (const report of reports) {
    if (report && report.lines.length > 0 && (!best || report.lines.length > best.lines.length)) {
      best = report;
    }
  }
  return best;
}
