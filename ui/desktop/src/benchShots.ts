import fs from 'node:fs/promises';
import path from 'node:path';

export interface BenchShot {
  name: string;
  caption: string;
  b64: string;
}

const SB8_CAPTIONS: Record<string, string> = {
  initial: 'Initial app view',
  front: 'Front camera',
  top: 'Top camera',
  iso: 'Isometric camera',
  kinematics: 'Crane movement',
  lift: 'Cargo lifted',
  rotation: 'Cargo rotated',
  final: 'Final app view',
};

/**
 * The isolated payments probes' named captures. SB7.2's probe shares SB7.1's capture path, so the
 * same scenes are read under either probe's prefix (`sb71-field`, `sb72-field`, …) — a prefix the
 * reader does not know would drop the run's evidence without a word.
 */
const PAYMENTS_SHOT_PREFIXES = ['sb71', 'sb72'] as const;
const PAYMENTS_SCENES: Record<string, string> = {
  field: 'Payment towers overview',
  'inspect-usd': 'USD payment inspection',
  'inspect-jpy': 'JPY payment inspection',
  'inspect-kwd': 'KWD payment inspection',
  'inspect-eur': 'EUR payment inspection',
  'live-update': 'Committed payment update',
  'final-inspector': 'Final payment inspector',
};
const PAYMENTS_CAPTIONS: Record<string, string> = Object.fromEntries(
  PAYMENTS_SHOT_PREFIXES.flatMap((prefix) =>
    Object.entries(PAYMENTS_SCENES).map(([scene, caption]) => [`${prefix}-${scene}`, caption])
  )
);
const PAYMENTS_INSPECT = new RegExp(`^(?:${PAYMENTS_SHOT_PREFIXES.join('|')})-inspect-([a-z]+)$`);

/**
 * forge_probe.mjs's captures (forge/DESIGN.md §6.6): `<surface>-<board|sprint>-<theme>-<w>x<h>.png` in
 * `forge-shots/`, plus the contact sheet of every surface. The card leads with the full-width widget and
 * the sprint action in both themes; the contact sheet closes the set (it is the largest, so the upload
 * limit drops it first). Within a kind the lowest name wins, so the pick is deterministic.
 */
const FORGE_PICKS: Array<{ name: string; caption: string; match: RegExp }> = [
  { name: 'forge-widget-light', caption: 'Dashboard widget · light', match: /^widget-view-\d+-light-1180x\d+\.png$/ },
  { name: 'forge-widget-dark', caption: 'Dashboard widget · dark', match: /^widget-view-\d+-dark-1180x\d+\.png$/ },
  { name: 'forge-sprint-light', caption: 'Sprint action · light', match: /^sprint-action-\d+-light-\d+x\d+\.png$/ },
  { name: 'forge-sprint-dark', caption: 'Sprint action · dark', match: /^sprint-action-\d+-dark-\d+x\d+\.png$/ },
  { name: 'forge-edit', caption: 'Widget edit view', match: /^widget-edit-\d+-light\.png$/ },
  { name: 'forge-widget-narrow', caption: 'Dashboard widget · 380 px', match: /^widget-view-\d+-light-380x\d+\.png$/ },
  { name: 'forge-noconfig', caption: 'Widget before configuration', match: /^widget-view-noconfig\.png$/ },
  { name: 'forge-not-started', caption: 'Sprint not started', match: /^not-started-\d+\.png$/ },
  { name: 'forge-contact-sheet', caption: 'Every captured surface', match: /^contact-sheet\.png$/ },
];

async function pickForgeShots(dir: string, files: string[]): Promise<BenchShot[]> {
  const sorted = files.slice().sort();
  const result: BenchShot[] = [];
  for (const pick of FORGE_PICKS) {
    const file = sorted.find((name) => pick.match.test(name));
    if (!file) continue;
    try {
      const bytes = await fs.readFile(path.join(dir, file));
      result.push({ name: pick.name, caption: pick.caption, b64: bytes.toString('base64') });
    } catch {
      // A capture still being written or removed cannot be displayed yet.
    }
  }
  return result;
}

/** Read probe evidence for local viewing; upload constraints must not hide local captures. */
export async function pickBenchShots(workdir: string): Promise<BenchShot[]> {
  const forgeDir = path.join(workdir, 'forge-shots');
  const forgeFiles = await fs.readdir(forgeDir).catch((error) => {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return null;
    throw error;
  });
  if (forgeFiles) return pickForgeShots(forgeDir, forgeFiles);
  let dir = path.join(workdir, 'bench-shots');
  let files: string[];
  try {
    files = await fs.readdir(dir);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    dir = path.join(workdir, 'sb7-shots');
    files = await fs.readdir(dir).catch((error) => {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return [] as string[];
      throw error;
    });
  }
  type Capture = { file: string; epoch: number; scenario: string; legacy?: boolean };
  const captures: Capture[] = [];
  for (const file of files) {
    const modern = file.match(
      /^(\d+)-(loaded|synced|error|empty|mobile|boot|flow|viz|sb7[12]-(?:field|inspect-[a-z]+|live-update|final-inspector)|sb8-(?:initial|front|top|iso|kinematics|lift|rotation|final))\.png$/
    );
    const oldCamera = file.match(/^sb8-(front|top|iso)\.png$/);
    const oldGate = file.match(/^sb8-gate-(\d+)\.png$/);
    if (modern) captures.push({ file, epoch: Number(modern[1]), scenario: modern[2] });
    else if (oldCamera)
      captures.push({ file, epoch: 0, scenario: `sb8-${oldCamera[1]}`, legacy: true });
    else if (oldGate)
      captures.push({ file, epoch: Number(oldGate[1]), scenario: 'sb8-gate', legacy: true });
  }
  const matching = (scenario: string) =>
    captures.filter((c) => c.scenario === scenario).sort((a, b) => a.epoch - b.epoch);
  const picks: Array<Capture & { name: string; caption: string }> = [];
  const loaded = matching('loaded');
  if (loaded.length > 1)
    picks.push({ ...loaded[0], name: 'loaded-before', caption: 'First captured render' });
  if (loaded.length)
    picks.push({ ...loaded[loaded.length - 1], name: 'loaded', caption: 'Latest captured render' });
  for (const [scenario, caption] of Object.entries({
    synced: 'After sync',
    error: 'Error state',
    empty: 'Empty state',
    mobile: 'Mobile · 375px',
    boot: 'Initial app view',
    flow: 'Payment workflow',
    viz: '3D visualization',
    ...PAYMENTS_CAPTIONS,
    ...Object.fromEntries(
      captures.flatMap((capture) => {
        const currency = PAYMENTS_INSPECT.exec(capture.scenario)?.[1];
        return currency ? [[capture.scenario, `${currency.toUpperCase()} payment inspection`]] : [];
      })
    ),
    ...Object.fromEntries(
      Object.entries(SB8_CAPTIONS).map(([key, label]) => [`sb8-${key}`, label])
    ),
    'sb8-gate': 'Scene canvas · gate capture',
  })) {
    const available = matching(scenario);
    const shot = available[available.length - 1];
    if (shot)
      picks.push({
        ...shot,
        name: scenario,
        caption: `${caption}${shot.legacy ? ' · legacy capture' : ''}`,
      });
  }
  const result: BenchShot[] = [];
  for (const shot of picks) {
    try {
      const bytes = await fs.readFile(path.join(dir, shot.file));
      result.push({ name: shot.name, caption: shot.caption, b64: bytes.toString('base64') });
    } catch {
      // A capture still being written or removed cannot be displayed yet.
    }
  }
  return result;
}

/** The site's existing upload contract, applied only when constructing its publish payload. */
export function limitBenchShotsForPublish(shots: BenchShot[]): BenchShot[] {
  const result: BenchShot[] = [];
  let total = 0;
  for (const shot of shots) {
    const bytes = Buffer.byteLength(shot.b64, 'base64');
    if (bytes > Math.floor(1.4 * 1024 * 1024) || total + bytes > Math.floor(3.5 * 1024 * 1024))
      continue;
    result.push(shot);
    total += bytes;
    if (result.length === 5) break;
  }
  return result;
}
