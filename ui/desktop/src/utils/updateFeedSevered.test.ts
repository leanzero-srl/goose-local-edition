import * as fs from 'fs';
import * as path from 'path';
import { describe, expect, it } from 'vitest';

/**
 * Goose Swarm owns its update line (owner decision, pass A): the app must NEVER query the parent
 * goose repository for updates again.
 *
 * The defect this pins: vite.main.config.mts `define`s process.env.GITHUB_OWNER/GITHUB_REPO into
 * the built main bundle, so its defaults — not the source-file defaults in autoUpdater.ts /
 * githubUpdater.ts — are what a packaged app actually queries. They pointed at the parent
 * (aaif-goose/goose) while the source files read as leanzero, and every packaged build phoned the
 * parent feed. Every file in the update path must default to OUR repository.
 */

const ROOT = path.resolve(__dirname, '..', '..');

const UPDATE_PATH_FILES = [
  'vite.main.config.mts',
  'forge.config.ts',
  'src/utils/autoUpdater.ts',
  'src/utils/githubUpdater.ts',
];

const PARENT_MARKERS = [/aaif-goose/, /block\/goose/, /github\.com\/block/];

const read = (rel: string) => fs.readFileSync(path.join(ROOT, rel), 'utf8');

describe('the update feed is severed from the parent goose', () => {
  for (const rel of UPDATE_PATH_FILES) {
    it(`${rel} names no parent repo and defaults to the leanzero release line`, () => {
      const content = read(rel);
      for (const marker of PARENT_MARKERS) {
        expect(content).not.toMatch(marker);
      }
      expect(content).toContain('leanzero-srl');
      expect(content).toContain('goose-local-edition');
    });
  }

  it('the app version is Goose Swarm own 2.x line, above every parent 1.x release', () => {
    const pkg = JSON.parse(read('package.json')) as { version: string };
    const major = Number(pkg.version.split('.')[0]);
    expect(major).toBeGreaterThanOrEqual(2);
  });
});
