const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { notarize } = require('@electron/notarize');
const { assertOnlyExpectedChanges } = require('./stage-incremental-cli.cjs');
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const run = (command, args) => execFileSync(command, args, { stdio: 'inherit' });

async function main() {
  const receiptPath = process.argv[2];
  if (!receiptPath) throw new Error('Usage: node notarize-incremental-cli.cjs RECEIPT');
  const directory = fs.realpathSync(path.dirname(receiptPath));
  if (path.dirname(directory) !== '/private/tmp' || !path.basename(directory).startsWith('goose-controller-incremental-')) throw new Error('Expected private incremental staging directory');
  const receipt = JSON.parse(fs.readFileSync(receiptPath));
  const app = fs.realpathSync(receipt.app);
  if (app !== path.join(directory, 'Goose Swarm.app') || !receipt.cliEquivalence || !receipt.mainEquivalence || receipt.notarized) throw new Error('Expected validated, not-yet-notarized staged candidate');
  const archive = path.join(directory, 'Goose-Swarm-controller-notarized.zip');
  if (fs.existsSync(archive)) throw new Error('Delivery archive already exists');
  if (sha(path.join(app, 'Contents/Resources/bin/goose')) !== receipt.signedCliSha256) throw new Error('Candidate CLI changed after review');
  const before = path.join(directory, 'before-notarization.json');
  const snapshot = file => run('python3', [path.join(__dirname, 'bundle-fingerprint.py'), app, file]);
  snapshot(before);
  assertOnlyExpectedChanges(JSON.parse(fs.readFileSync(path.join(directory, 'after-signing.json'))), JSON.parse(fs.readFileSync(before)), []);
  run('codesign', ['--verify', '--deep', '--strict', app]);
  for (const name of ['APPLE_TEAM_ID', 'APPLE_ID', 'APPLE_ID_PASSWORD'])
    if (!process.env[name]) throw new Error('Missing existing local signing environment: ' + name);
  console.log('Submitting approved private candidate to Apple notarization');
  await notarize({ appPath: app, appleId: process.env.APPLE_ID, appleIdPassword: process.env.APPLE_ID_PASSWORD, teamId: process.env.APPLE_TEAM_ID });
  run('codesign', ['--verify', '--deep', '--strict', app]);
  run('xcrun', ['stapler', 'validate', app]);
  run('/usr/sbin/spctl', ['-a', '-vvv', '-t', 'exec', app]);
  const after = path.join(directory, 'after-notarization.json');
  snapshot(after);
  const changed = assertOnlyExpectedChanges(JSON.parse(fs.readFileSync(before)), JSON.parse(fs.readFileSync(after)), ['Contents/CodeResources']);
  run('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, archive]);
  Object.assign(receipt, { notarized: true, notarizedAt: new Date().toISOString(), notarizationChanged: changed, archive, archiveSha256: sha(archive) });
  fs.writeFileSync(receiptPath, JSON.stringify(receipt, null, 2) + '\n');
  fs.writeFileSync(archive + '.sha256', `${receipt.archiveSha256}  ${archive}\n`);
  console.log(JSON.stringify(receipt, null, 2));
}

main().catch(error => {
  let message = String(error.message || error);
  for (const key of ['APPLE_ID_PASSWORD', 'APPLE_ID'])
    if (process.env[key]) message = message.split(process.env[key]).join('[redacted]');
  console.error(message);
  process.exit(1);
});
