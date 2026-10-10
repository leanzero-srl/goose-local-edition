import { createRequire } from 'node:module';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { afterEach, expect, it } from 'vitest';
import {
  BENCH_RENDER_PROBE,
  BENCH_SPEC_FILE,
  defaultBenchmarkScorer,
  defaultBenchmarkTier,
} from './benchTierPayload';
const copy = createRequire(import.meta.url)('../scripts/copy-bench-release-manifest.cjs');
const { STABLE_RELEASE, FORGE_RELEASE } = copy as {
  STABLE_RELEASE: { dir: string; scorerVersion: string; spec: string; probe: string };
  FORGE_RELEASE: { dir: string; scorerVersion: string; spec: string; probe: string };
};
const roots: string[] = [];
afterEach(() => roots.splice(0).forEach((root) => rmSync(root, { recursive: true, force: true })));
const sha = (text: string) => createHash('sha256').update(text).digest('hex');
const fixture = () => {
  const root = mkdtempSync(join(tmpdir(), 'bench-release-'));
  roots.push(root);
  const src = join(root, 'source'),
    dest = join(root, 'packaged');
  for (const dir of [src, dest]) mkdirSync(join(dir, STABLE_RELEASE.dir), { recursive: true });
  mkdirSync(join(dest, 'bench'), { recursive: true });
  writeFileSync(join(dest, STABLE_RELEASE.spec), 'actual spec');
  writeFileSync(join(dest, STABLE_RELEASE.probe), 'actual probe');
  return { src, dest };
};
const covering = (extra: Record<string, string> = {}) => ({
  scorerVersion: STABLE_RELEASE.scorerVersion,
  files: {
    [STABLE_RELEASE.spec]: sha('actual spec'),
    [STABLE_RELEASE.probe]: sha('actual probe'),
    ...extra,
  },
});
const manifestPath = (root: string) => join(root, STABLE_RELEASE.dir, 'release-manifest.json');

/** The cjs cannot import TypeScript; this is what keeps its stable release equal to the app's default. */
it('verifies the release the app runs by default — the sb7.2 manifest, its spec and its probe', () => {
  expect(STABLE_RELEASE).toEqual({
    dir: 'sb7.2',
    scorerVersion: defaultBenchmarkScorer(),
    spec: BENCH_SPEC_FILE[defaultBenchmarkTier()],
    probe: `bench/${BENCH_RENDER_PROBE[defaultBenchmarkTier()]}`,
  });
});
it('verifies the Forge era the app bundles — the forge2 manifest, its spec and its probe', () => {
  expect(FORGE_RELEASE).toEqual({
    dir: 'forge2',
    scorerVersion: defaultBenchmarkScorer('forge'),
    spec: BENCH_SPEC_FILE[defaultBenchmarkTier('forge')],
    probe: `bench/${BENCH_RENDER_PROBE[defaultBenchmarkTier('forge')]}`,
  });
});
it('refuses packaging without the actual release manifest', () => {
  const { src, dest } = fixture();
  expect(() => copy(src, dest)).toThrow();
});
it('copies exact manifest bytes only when packaged inputs match', () => {
  const { src, dest } = fixture();
  const bytes = JSON.stringify(covering()) + '\n';
  writeFileSync(manifestPath(src), bytes);
  copy(src, dest);
  expect(readFileSync(manifestPath(dest), 'utf8')).toBe(bytes);
  writeFileSync(join(dest, STABLE_RELEASE.spec), 'stale spec');
  expect(() => copy(src, dest)).toThrow('differs from release manifest');
});
it('refuses a manifest from another scorer, or one that does not cover the stable spec and probe', () => {
  const { src, dest } = fixture();
  writeFileSync(manifestPath(src), JSON.stringify({ ...covering(), scorerVersion: 'sb-7.1' }));
  expect(() => copy(src, dest)).toThrow('no frozen file coverage');
  const withoutProbe = covering();
  delete (withoutProbe.files as Record<string, string>)[STABLE_RELEASE.probe];
  writeFileSync(manifestPath(src), JSON.stringify(withoutProbe));
  expect(() => copy(src, dest)).toThrow(`does not cover ${STABLE_RELEASE.probe}`);
  expect(existsSync(manifestPath(dest))).toBe(false);
});
it('refuses a listed file the mirror did not ship, by name', () => {
  const { src, dest } = fixture();
  writeFileSync(
    manifestPath(src),
    JSON.stringify(covering({ [`${STABLE_RELEASE.dir}/starter/STARTER.md`]: sha('starter') }))
  );
  expect(() => copy(src, dest)).toThrow(
    `missing a release manifest file: ${STABLE_RELEASE.dir}/starter/STARTER.md`
  );
});
it('refuses circular manifest hashing', () => {
  const { src, dest } = fixture();
  writeFileSync(
    manifestPath(src),
    JSON.stringify(covering({ [`${STABLE_RELEASE.dir}/release-manifest.json`]: 'a'.repeat(64) }))
  );
  expect(() => copy(src, dest)).toThrow('circular');
});
