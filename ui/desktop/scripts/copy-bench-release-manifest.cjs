const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');

// The STABLE benchmark the packaged app runs (benchTierPayload.ts DEFAULT_BENCHMARK_TIER). Its release
// manifest is the frozen inventory of the shipped payload, so it is the one verified here. An older
// tier's manifest (sb7.1/) describes bytes that a later release legitimately changed (run_build.py gains
// the new regime), so it stays in the repository as that release's receipt and is not shipped as a claim
// about this one. benchReleaseManifest.test.ts pins these names to benchTierPayload.ts.
const STABLE_RELEASE = {
  dir: 'sb7.2',
  scorerVersion: 'sb-7.2',
  spec: 'spec-build-sb72.md',
  probe: 'bench/product_probe_sb72.mjs',
};

// The Forge family's bundled era (benchTierPayload.ts BENCH_FAMILY_DEFAULT.forge). Its manifest pins the
// forge payload (forge/public, starter, kit sources without module trees, site, and the bench closure
// run_build --forge imports — release_manifest.py's `forge` family), verified the same way as SB's.
const FORGE_RELEASE = {
  dir: 'forge',
  scorerVersion: 'forge-1.0',
  spec: 'forge/public/spec-build-forge.md',
  probe: 'bench/forge_probe.mjs',
};

function copyBenchReleaseManifest(sourceRoot, destinationRoot, release = STABLE_RELEASE) {
  const relative = `${release.dir}/release-manifest.json`;
  const bytes = fs.readFileSync(path.join(sourceRoot, relative));
  const manifest = JSON.parse(bytes);
  if (
    manifest.scorerVersion !== release.scorerVersion ||
    !manifest.files ||
    !Object.keys(manifest.files).length
  ) {
    throw new Error(`${release.scorerVersion} release manifest has no frozen file coverage`);
  }
  for (const required of [release.spec, release.probe]) {
    if (!(required in manifest.files)) {
      throw new Error(`${release.scorerVersion} release manifest does not cover ${required}`);
    }
  }
  const root = path.resolve(destinationRoot);
  for (const [name, expected] of Object.entries(manifest.files)) {
    const file = path.resolve(root, name);
    if (name === relative || !file.startsWith(root + path.sep))
      throw new Error('Invalid circular or escaping release manifest entry');
    if (!fs.existsSync(file))
      throw new Error(`Packaged benchmark is missing a release manifest file: ${name}`);
    const actual = createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    if (actual !== expected)
      throw new Error(`Packaged benchmark differs from release manifest: ${name}`);
  }
  fs.mkdirSync(path.join(destinationRoot, release.dir), { recursive: true });
  fs.copyFileSync(path.join(sourceRoot, relative), path.join(destinationRoot, relative));
}

module.exports = copyBenchReleaseManifest;
module.exports.STABLE_RELEASE = STABLE_RELEASE;
module.exports.FORGE_RELEASE = FORGE_RELEASE;
