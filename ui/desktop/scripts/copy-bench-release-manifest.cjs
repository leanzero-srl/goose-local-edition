const fs = require('node:fs');
const path = require('node:path');
const { createHash } = require('node:crypto');
module.exports = function copyBenchReleaseManifest(sourceRoot, destinationRoot) {
  const relative = 'sb7.1/release-manifest.json';
  const bytes = fs.readFileSync(path.join(sourceRoot, relative));
  const manifest = JSON.parse(bytes);
  if (manifest.scorerVersion !== 'sb-7.1' || !manifest.files || !Object.keys(manifest.files).length) {
    throw new Error('SB7.1 release manifest has no frozen file coverage');
  }
  const root = path.resolve(destinationRoot);
  for (const [name, expected] of Object.entries(manifest.files)) {
    const file = path.resolve(root, name);
    if (name === relative || !file.startsWith(root + path.sep)) throw new Error('Invalid circular or escaping release manifest entry');
    const actual = createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    if (actual !== expected) throw new Error(`Packaged benchmark differs from release manifest: ${name}`);
  }
  fs.copyFileSync(path.join(sourceRoot, relative), path.join(destinationRoot, relative));
};
