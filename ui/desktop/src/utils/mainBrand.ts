import * as fsSync from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { isLocalProviderName } from '../components/settings/models/leanzeroSelectorPolicy';

/**
 * MAIN-PROCESS edition branding (queued fix #10): the tray tooltip, notifications and the
 * main-process dialogs used to hardcode "Goose" while every renderer surface said "Goose Swarm" —
 * main.ts had no edition awareness at all. This is the ONE shared constant pair + resolver both
 * main.ts and autoUpdater.ts read.
 *
 * Resolution mirrors the renderer's EditionContext / the Rust resolver (edition.rs), minus the
 * CLI/env layers the desktop main process does not have: an explicitly persisted `edition`
 * setting wins; with none stored, the edition derives from the active provider in the goose
 * config.yaml (local/swarm provider fragments => the local edition => "Goose Swarm").
 *
 * Electron-free on purpose (paths are passed in / defaulted from the home dir), so the pure
 * pieces are unit-testable without an electron mock.
 */

export const STANDARD_BRAND = 'Goose';
export const SWARM_BRAND = 'Goose Swarm';

/** PURE resolver — explicit persisted edition first, else provider derivation. */
export function resolveBrandName(
  persistedEdition: unknown,
  activeProvider: string | null
): string {
  if (persistedEdition === 'local') return SWARM_BRAND;
  if (persistedEdition === 'standard') return STANDARD_BRAND;
  if (activeProvider && isLocalProviderName(activeProvider)) return SWARM_BRAND;
  // This fork IS Goose Swarm: with nothing persisted, the brand is Goose Swarm regardless of the
  // provider (mirrors EditionContext.resolveEdition); "Goose" only by explicit persisted choice.
  return SWARM_BRAND;
}

/**
 * PURE: pull the active provider out of a config.yaml text (flat top-level keys, optional
 * quotes). CAUGHT LIVE on this machine (2026-08-31): the config carries `active_provider: omlx`
 * and NO `GOOSE_PROVIDER` key at all — the agentic config writes active_provider — so a
 * GOOSE_PROVIDER-only parse derived NO provider and the tray branded plain "Goose". The legacy
 * GOOSE_PROVIDER key still wins when both exist (it is the explicit override the server honors).
 */
export function parseProviderFromConfigYaml(text: string): string | null {
  const legacy = text.match(/^GOOSE_PROVIDER:[ \t]*["']?([^"'\n#]+)/m)?.[1]?.trim();
  if (legacy) return legacy;
  const active = text.match(/^active_provider:[ \t]*["']?([^"'\n#]+)/m)?.[1]?.trim();
  return active ? active : null;
}

export function defaultGooseConfigPath(): string {
  return path.join(os.homedir(), '.config', 'goose', 'config.yaml');
}

function readPersistedEdition(settingsFile: string): unknown {
  try {
    const parsed = JSON.parse(fsSync.readFileSync(settingsFile, 'utf8')) as { edition?: unknown };
    return parsed.edition;
  } catch {
    return undefined;
  }
}

function readActiveProvider(configYamlPath: string): string | null {
  try {
    return parseProviderFromConfigYaml(fsSync.readFileSync(configYamlPath, 'utf8'));
  } catch {
    return null;
  }
}

/**
 * The brand name for main-process surfaces, resolved fresh per call (the files are tiny and the
 * call sites are rare: tray updates, dialogs, notifications).
 */
export function getBrandName(opts: { settingsFile: string; configYamlPath?: string }): string {
  return resolveBrandName(
    readPersistedEdition(opts.settingsFile),
    readActiveProvider(opts.configYamlPath ?? defaultGooseConfigPath())
  );
}
