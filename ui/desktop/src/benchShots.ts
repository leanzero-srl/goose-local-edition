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

const SB71_CAPTIONS: Record<string, string> = {
  'sb71-field': 'Payment towers overview',
  'sb71-inspect-usd': 'USD payment inspection',
  'sb71-inspect-jpy': 'JPY payment inspection',
  'sb71-inspect-kwd': 'KWD payment inspection',
  'sb71-inspect-eur': 'EUR payment inspection',
  'sb71-live-update': 'Committed payment update',
  'sb71-final-inspector': 'Final payment inspector',
};

/** Read probe evidence for local viewing; upload constraints must not hide local captures. */
export async function pickBenchShots(workdir: string): Promise<BenchShot[]> {
  const dir = path.join(workdir, 'bench-shots');
  const files = await fs.readdir(dir).catch(() => [] as string[]);
  type Capture = { file: string; epoch: number; scenario: string; legacy?: boolean };
  const captures: Capture[] = [];
  for (const file of files) {
    const modern = file.match(
      /^(\d+)-(loaded|synced|error|empty|mobile|boot|flow|viz|sb71-(?:field|inspect-[a-z]+|live-update|final-inspector)|sb8-(?:initial|front|top|iso|kinematics|lift|rotation|final))\.png$/
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
    ...SB71_CAPTIONS,
    ...Object.fromEntries(captures.filter((capture) => capture.scenario.startsWith('sb71-inspect-')).map((capture) => [capture.scenario, `${capture.scenario.slice('sb71-inspect-'.length).toUpperCase()} payment inspection`])),
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
