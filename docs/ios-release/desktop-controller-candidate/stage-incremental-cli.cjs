// Private staging only: requires a separately gated CLI and never installs or launches the app.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { compareSignedCli } = require('./cli-signature-equivalence.cjs');

const BASE_ARCHIVE_SHA = '520c8596fc790e8e4431e378aacef412458d905a19c7701209829a87ec9e94e0';
const DESKTOP_SOURCE = 'b018f72f51f0746c4219f6f08015da6951fdbd42';
const TEAM = 'ZZ8MTZ6NRZ';
const IDENTITY = `Developer ID Application: Mihai Perdum (${TEAM})`;
const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const run = (command, args) => execFileSync(command, args, { stdio: 'inherit' });

function assertOnlyExpectedChanges(before, after, allowed) {
  const all = new Set([...Object.keys(before), ...Object.keys(after)]);
  const changes = [...all].filter(key => JSON.stringify(before[key]) !== JSON.stringify(after[key]));
  const unexpected = changes.filter(key => !allowed.includes(key));
  if (unexpected.length) throw new Error(`Unexpected bundle changes: ${unexpected.join(', ')}`);
  return changes;
}

function stage({ repository, archive, cli, cliSha, sourceSha, destination }) {
  if (!/^[a-f0-9]{64}$/.test(cliSha) || !/^[a-f0-9]{40}$/.test(sourceSha)) throw new Error('Exact SHA inputs required');
  if (fs.existsSync(destination)) throw new Error('Staging destination already exists');
  if (path.dirname(path.resolve(destination)) !== '/tmp' || !path.basename(destination).startsWith('goose-controller-incremental-')) throw new Error('Destination must be a fresh direct child of /tmp');
  if (sha(archive) !== BASE_ARCHIVE_SHA) throw new Error('Immutable baseline archive hash mismatch');
  if (sha(cli) !== cliSha) throw new Error('Gated CLI hash mismatch');
  run('git', ['-C', repository, 'diff', '--exit-code', DESKTOP_SOURCE, sourceSha, '--', 'ui']);
  fs.mkdirSync(destination);
  const app = path.join(destination, 'Goose Swarm.app');
  run('ditto', ['-x', '-k', archive, destination]);
  run('codesign', ['--verify', '--deep', '--strict', app]);
  run('xcrun', ['stapler', 'validate', app]);
  const baseline = path.join(destination, 'baseline.json');
  const snapshot = file => run('python3', [path.join(__dirname, 'bundle-fingerprint.py'), app, file]);
  snapshot(baseline);
  const plist = path.join(app, 'Contents/Info.plist');
  const executable = execFileSync('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleExecutable', plist], { encoding: 'utf8' }).trim();
  if (!executable || path.basename(executable) !== executable) throw new Error('Invalid bundle executable');
  const mainRelative = `Contents/MacOS/${executable}`;
  const main = path.join(app, mainRelative);
  const mainBefore = path.join(destination, 'main-before-signing');
  fs.copyFileSync(main, mainBefore);
  const embeddedRelative = 'Contents/Resources/bin/goose';
  const embedded = path.join(app, embeddedRelative);
  if (!fs.lstatSync(embedded).isFile()) throw new Error('Embedded CLI must be a regular file');
  const entitlements = path.join(destination, 'entitlements.plist');
  fs.writeFileSync(entitlements, execFileSync('git', ['-C', repository, 'show', `${sourceSha}:ui/desktop/entitlements.plist`]));
  fs.copyFileSync(cli, embedded);
  fs.chmodSync(embedded, 0o755);
  if (sha(embedded) !== cliSha) throw new Error('Staged CLI differs from gated input');
  const keychain = path.join(process.env.HOME, 'Library/Keychains/goose-signing.keychain-db');
  // Only these two paths are signed. --deep is used only by verification below.
  run('codesign', ['--force', '--sign', IDENTITY, '--keychain', keychain, '--timestamp', '--options', 'runtime', '--entitlements', entitlements, embedded]);
  run('codesign', ['--force', '--sign', IDENTITY, '--keychain', keychain, '--timestamp', '--preserve-metadata=identifier,entitlements,requirements,flags,runtime,launch-constraints,library-constraints', app]);
  run('codesign', ['--verify', '--deep', '--strict', app]);
  run('codesign', ['--verify', '--strict', embedded]);
  const cliEquivalence = compareSignedCli(cli, embedded);
  const mainEquivalence = compareSignedCli(mainBefore, main);
  const after = path.join(destination, 'after-signing.json');
  snapshot(after);
  const changed = assertOnlyExpectedChanges(JSON.parse(fs.readFileSync(baseline)), JSON.parse(fs.readFileSync(after)), [embeddedRelative, mainRelative, 'Contents/_CodeSignature/CodeResources']);
  const receipt = {
    baselineArchiveSha256: BASE_ARCHIVE_SHA,
    desktopSourceSha: DESKTOP_SOURCE,
    cliSourceSha: sourceSha,
    inputCliSha256: cliSha,
    signedCliSha256: sha(embedded),
    cliEquivalence, mainEquivalence,
    changed, app,
    notarized: false, installed: false, launched: false,
  };
  fs.writeFileSync(path.join(destination, 'receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
  console.log(JSON.stringify(receipt, null, 2));
}

module.exports = { assertOnlyExpectedChanges };
if (require.main === module) {
  const [repository, archive, cli, cliSha, sourceSha, destination] = process.argv.slice(2);
  if (![repository, archive, cli, cliSha, sourceSha, destination].every(Boolean)) throw new Error('Usage: node stage-incremental-cli.cjs REPO BASE_ZIP GATED_CLI CLI_SHA256 SOURCE_SHA NEW_STAGE');
  stage({ repository, archive, cli, cliSha, sourceSha, destination });
}
