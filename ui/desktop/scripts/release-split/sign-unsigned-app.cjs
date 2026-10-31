// Split release, step 2 (signer Mac only): Developer-ID sign an UNSIGNED app built on another Mac.
// Adapted from docs/ios-release/desktop-controller-candidate/node-management/sign-only.cjs
// (origin/codex/link-controller-plan, reviewed 2026-10-29) for every release version:
//   * paths and version come from the build receipt, not constants;
//   * the metadata validator that lived only in /tmp is now classifyChanges() below;
//   * signing uses the SAME @electron/osx-sign options forge's Developer-ID branch passes
//     (forge.config.ts: keychain, entitlements, entitlements-inherit), so the result matches a
//     `just release-notarized` build; checkEntitlementParity() proves it against a reference app.
// Usage: node sign-unsigned-app.cjs <build-receipt.json> <unsigned.zip> <fresh stage dir> <tested cli sha256> [reference.app]
// Never notarizes, uploads or installs. Writes <stage>/receipt.json.
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto'), assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { compareSignedCli, compareStripped } = require('./signature-equivalence.cjs');

const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const load = p => JSON.parse(fs.readFileSync(p));
const run = (c, a) => execFileSync(c, a, { stdio: 'inherit' });
const capture = (c, a) => execFileSync(c, a, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 64 * 1024 * 1024 });
const write = (p, v) => { fs.writeFileSync(p + '.pending', JSON.stringify(v, null, 2) + '\n', { mode: 0o600 }); fs.renameSync(p + '.pending', p); };
const TEAM = 'ZZ8MTZ6NRZ';
const CLI = 'Contents/Resources/bin/goose', MAIN = 'Contents/MacOS/Goose Swarm';
const HELPER = 'Contents/Frameworks/Goose Swarm Helper.app/Contents/MacOS/Goose Swarm Helper';

function machOMagic(file) {
  const fd = fs.openSync(file, 'r'), b = Buffer.alloc(4);
  try { if (fs.readSync(fd, b, 0, 4, 0) < 4) return null; } finally { fs.closeSync(fd); }
  return b.readUInt32LE(0);
}
const MACHO = new Set([0xfeedfacf, 0xfeedface, 0xcafebabe, 0xbebafeca, 0xcffaedfe]);

function requireSupportedThinMachO(file, relative) {
  const fd = fs.openSync(file, 'r'), header = Buffer.alloc(8);
  try { assert.equal(fs.readSync(fd, header, 0, 8, 0), 8, 'Truncated Mach-O: ' + relative); } finally { fs.closeSync(fd); }
  assert(header.readUInt32LE(0) === 0xfeedfacf && [0x0100000c, 0x01000007].includes(header.readUInt32LE(4)),
    'Unsupported Mach-O architecture for signing equivalence: ' + relative + ' (header ' + header.toString('hex') + '); thin arm64 or x86_64 required');
}

// Signing may only (a) change Mach-O bytes, (b) add _CodeSignature/CodeResources seals and
// (c) write com.apple.cs.* signature attributes on a signed non-Mach-O file (Electron's MainMenu.nib).
// Every other path keeps kind, mode, bytes and every other xattr. Returns what changed, by class.
const SIGNATURE_XATTR = /^com\.apple\.cs\./;
const withoutSignatureXattrs = x => Object.fromEntries(Object.entries(x).filter(([k]) => !SIGNATURE_XATTR.test(k)));
function classifyChanges(before, after, beforeRoot, afterRoot) {
  const machO = [], added = [], resources = [], seals = [];
  const isSeal = p => path.basename(p) === 'CodeResources' && path.basename(path.dirname(p)) === '_CodeSignature';
  for (const [p, item] of Object.entries(before)) {
    const now = after[p];
    assert(now, 'Missing after signing: ' + p);
    assert.equal(now.kind, item.kind, 'Kind changed: ' + p);
    assert.equal(now.mode, item.mode, 'Mode changed: ' + p);
    assert.deepEqual(withoutSignatureXattrs(now.xattrs), withoutSignatureXattrs(item.xattrs), 'Extended attributes changed: ' + p);
    const sigChanged = JSON.stringify(now.xattrs) !== JSON.stringify(item.xattrs);
    if (sigChanged) assert(item.kind === 'file' && now.sha256 === item.sha256 && Object.keys(now.xattrs).some(k => SIGNATURE_XATTR.test(k)), 'Signature attributes changed on a non-resource: ' + p);
    if (sigChanged) resources.push(p);
    if (item.kind === 'symlink') assert.equal(now.target, item.target, 'Symlink target changed: ' + p);
    if (item.kind !== 'file' || now.sha256 === item.sha256) continue;
    // an unsigned package already carries the prebuilt Electron framework's (ad-hoc) seal; signing rewrites it
    if (isSeal(p)) { seals.push(p); continue; }
    const m0 = beforeRoot ? machOMagic(path.join(beforeRoot, p)) : item.machO, m1 = afterRoot ? machOMagic(path.join(afterRoot, p)) : now.machO;
    assert(MACHO.has(m0) && MACHO.has(m1), 'Non-Mach-O bytes changed: ' + p);
    machO.push(p);
  }
  for (const p of Object.keys(after)) {
    if (before[p]) continue;
    const ok = (path.basename(p) === '_CodeSignature' && after[p].kind === 'directory') || (isSeal(p) && after[p].kind === 'file');
    assert(ok, 'Unexpected new path after signing: ' + p);
    added.push(p);
  }
  return { machO: machO.sort(), added: added.sort(), resources: resources.sort(), seals: seals.sort() };
}

// Each mutation of the real fingerprints must be rejected by classifyChanges.
function classificationNegativeControls(before, after) {
  const files = Object.keys(before).filter(p => before[p].kind === 'file' && before[p].sha256 === after[p].sha256);
  const resource = files.find(p => p.endsWith('.nib') || p.endsWith('.plist') || p.endsWith('.js')) || files[0];
  assert(resource, 'No unchanged resource for negative controls');
  const mutate = f => { const a = structuredClone(after); f(a); return a; };
  const cases = {
    resourceBytes: mutate(a => { a[resource].sha256 = '0'.repeat(64); }),
    extraAttribute: mutate(a => { a[resource].xattrs['com.example.added'] = '0'.repeat(64); }),
    priorAttributeChanged: mutate(a => { a[resource].xattrs['com.apple.provenance'] = '2'.repeat(64); }),
    signatureAttributeOnChangedBytes: mutate(a => { a[resource].sha256 = '3'.repeat(64); a[resource].xattrs['com.apple.cs.CodeSignature'] = '4'.repeat(64); }),
    changedMode: mutate(a => { a[resource].mode ^= 0o100; }),
    missingPath: mutate(a => { delete a[resource]; }),
    sealOutsideCodeSignature: mutate(a => { a['Contents/Resources/CodeResources'] = { kind: 'file', mode: 0o644, xattrs: {}, sha256: '5'.repeat(64), bytes: 1 }; }),
    unexpectedNewFile: mutate(a => { a['Contents/Resources/unexpected.txt'] = { kind: 'file', mode: 0o644, xattrs: {}, sha256: '1'.repeat(64), bytes: 1 }; }),
  };
  const rejected = [];
  for (const [name, mutated] of Object.entries(cases)) {
    // without bundle roots classifyChanges reads Mach-O magic from the entry; a resource has none
    assert.throws(() => classifyChanges(before, mutated, null, null), undefined, 'Negative control accepted: ' + name);
    rejected.push(name);
  }
  return { positiveResource: resource, rejected };
}

function compareAllChangedMachO(classification, baseline, app) {
  const results = {};
  for (const relative of classification.machO) {
    requireSupportedThinMachO(path.join(baseline, relative), relative);
    requireSupportedThinMachO(path.join(app, relative), relative);
    results[relative] = compareSignedCli(path.join(baseline, relative), path.join(app, relative));
  }
  assert.equal(Object.keys(results).length, classification.machO.length);
  return results;
}

// Verbatim logic of sign-only.cjs helperCorruptionControl: flip one real __TEXT,__text byte, must be rejected.
function helperCorruptionControl(baseline, app, relative) {
  requireSupportedThinMachO(path.join(baseline, relative), relative);
  requireSupportedThinMachO(path.join(app, relative), relative);
  const temporary = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'goose-helper-negative-'));
  try {
    const original = path.join(temporary, 'original'), signed = path.join(temporary, 'signed');
    fs.copyFileSync(path.join(baseline, relative), original); fs.copyFileSync(path.join(app, relative), signed);
    execFileSync('codesign', ['--remove-signature', original]); execFileSync('codesign', ['--remove-signature', signed]);
    const a = fs.readFileSync(original), b = fs.readFileSync(signed);
    compareStripped(a, b);
    let cursor = 32, textOffset;
    const end = 32 + b.readUInt32LE(20);
    for (let i = 0; i < b.readUInt32LE(16); i++) {
      const command = b.readUInt32LE(cursor), size = b.readUInt32LE(cursor + 4);
      assert(size >= 8 && cursor + size <= end, 'Invalid helper load commands');
      if (command === 0x19) {
        const sections = b.readUInt32LE(cursor + 64); assert(72 + 80 * sections <= size, 'Invalid helper sections');
        for (let j = 0; j < sections; j++) {
          const section = cursor + 72 + 80 * j, name = b.subarray(section, section + 16).toString().replace(/\0+$/, '');
          const segment = b.subarray(section + 16, section + 32).toString().replace(/\0+$/, '');
          if (name === '__text' && segment === '__TEXT') {
            assert.equal(textOffset, undefined, 'Duplicate text section');
            assert(b.readBigUInt64LE(section + 40) > 0n, 'Empty helper text'); textOffset = b.readUInt32LE(section + 48);
          }
        }
      }
      cursor += size;
    }
    assert(Number.isInteger(textOffset) && textOffset >= end && textOffset < b.length, 'No helper code byte found');
    b[textOffset] ^= 1;
    assert.throws(() => compareStripped(a, b), /differs beyond/, 'Modified helper code accepted');
    return { path: relative, positiveComparisonPassed: true, corruptedTextOffset: textOffset, nonSignatureCodeChangeRejected: true };
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
}

function entitlementsAndFlags(file) {
  let ent = '';
  try { ent = capture('codesign', ['-d', '--entitlements', ':-', file]); } catch { ent = ''; }
  const keys = [...ent.matchAll(/<key>([^<]+)<\/key>/g)].map(m => m[1]).sort();
  return keys;
}
function codesignInfo(file) {
  const r = require('node:child_process').spawnSync('codesign', ['-dv', '--verbose=2', file], { encoding: 'utf8' });
  const text = (r.stdout || '') + (r.stderr || '');
  const flags = (text.match(/flags=(0x[0-9a-f]+)/) || [])[1];
  const team = (text.match(/TeamIdentifier=(\S+)/) || [])[1];
  const ident = (text.match(/Identifier=(\S+)/) || [])[1];
  return { flags, team, ident };
}

// Every signed code object present in the reference app (the last notarized release) must carry the
// same entitlement keys, flags and identifier in the new app. Proves osx-sign parity with forge's path.
function checkEntitlementParity(app, reference, codePaths) {
  const out = { compared: 0, missingInReference: [] };
  for (const p of codePaths) {
    const ref = path.join(reference, p);
    if (!fs.existsSync(ref)) { out.missingInReference.push(p); continue; }
    const a = { ents: entitlementsAndFlags(path.join(app, p)), ...codesignInfo(path.join(app, p)) };
    const r = { ents: entitlementsAndFlags(ref), ...codesignInfo(ref) };
    assert.deepEqual(a, r, 'Signature metadata differs from the reference release: ' + p);
    out.compared++;
  }
  return out;
}

async function main() {
  const [receiptPath, input, stage, testedCli, reference] = process.argv.slice(2);
  assert(receiptPath && input && stage && /^[0-9a-f]{64}$/.test(testedCli || ''), 'usage: <build-receipt> <unsigned.zip> <fresh stage> <tested cli sha256> [reference.app]');
  assert(!fs.existsSync(stage), 'Fresh stage required, never re-sign an existing artifact');
  const b = load(receiptPath);
  assert.equal(b.gate_fail, 0, 'build receipt: gate did not pass');
  assert.equal(sha(input), b.unsigned_archive_sha256, 'unsigned archive sha != build receipt');
  assert.equal(b.cli_sha256, testedCli, 'build receipt CLI sha != tested CLI sha');
  const repo = path.resolve(__dirname, '../../../..');
  assert.equal(capture('git', ['-C', repo, 'rev-parse', b.source_commit + '^{tree}']).trim(), b.source_tree, 'source tree mismatch');
  fs.mkdirSync(stage, { mode: 0o700 }); fs.mkdirSync(path.join(stage, 'baseline'));
  run('ditto', ['-x', '-k', input, path.join(stage, 'baseline')]);
  const baseline = path.join(stage, 'baseline/Goose Swarm.app'), app = path.join(stage, 'Goose Swarm.app');
  const snapshot = (root, name) => { const p = path.join(stage, name); run('python3', ['-I', path.join(__dirname, 'bundle-fingerprint.py'), root, p]); return load(p); };
  // tested CLI sha, asserted on the EMBEDDED CLI before signing
  assert.equal(sha(path.join(baseline, CLI)), testedCli, 'embedded CLI != tested CLI');
  assert.equal(capture('/usr/libexec/PlistBuddy', ['-c', 'Print CFBundleShortVersionString', path.join(baseline, 'Contents/Info.plist')]).trim(), b.app_version);
  const before = snapshot(baseline, 'before-signing.json'); run('ditto', [baseline, app]); assert.deepEqual(snapshot(app, 'copied-before-signing.json'), before);
  for (const p of Object.keys(before)) if (before[p].kind === 'file') {
    const candidate = fs.lstatSync(path.join(app, p)), installed = path.join('/Applications/Goose Swarm.app', p);
    if (fs.existsSync(installed)) { const real = fs.lstatSync(installed); assert(candidate.dev !== real.dev || candidate.ino !== real.ino, 'Shared installed inode: ' + p); }
  }
  // Same options as forge.config.ts's APPLE_TEAM_ID branch -> @electron/packager createSignOpts, except
  // continueOnError=false (a signing error must stop the release, not be logged and skipped).
  // node_modules may come from another checkout (GOOSE_NODE_DESKTOP); the osx-sign version must be the source commit's lock pin
  const desktop = process.env.GOOSE_NODE_DESKTOP || path.join(repo, 'ui/desktop');
  const osxSignVersion = load(require.resolve('@electron/osx-sign/package.json', { paths: [desktop] })).version;
  assert(capture('git', ['-C', repo, 'show', b.source_commit + ':ui/pnpm-lock.yaml']).includes(`'@electron/osx-sign@${osxSignVersion}'`), 'osx-sign ' + osxSignVersion + ' is not the source commit lock pin');
  const electronVersion = capture('/usr/libexec/PlistBuddy', ['-c', 'Print CFBundleVersion', path.join(baseline, 'Contents/Frameworks/Electron Framework.framework/Resources/Info.plist')]).trim();
  const { signApp } = require(require.resolve('@electron/osx-sign', { paths: [desktop] }));
  const entitlements = path.join(stage, 'entitlements.plist');
  fs.writeFileSync(entitlements, capture('git', ['-C', repo, 'show', b.source_commit + ':ui/desktop/entitlements.plist']));
  await signApp({ app, platform: 'darwin', version: electronVersion, keychain: process.env.KEYCHAIN_PATH || undefined,
    entitlements, 'entitlements-inherit': entitlements, continueOnError: false });
  const signed = snapshot(app, 'after-signing.json');
  const classification = classifyChanges(before, signed, baseline, app);
  const negativeControls = classificationNegativeControls(before, signed);
  for (const p of [...classification.machO, ...classification.resources]) run('codesign', ['--verify', '--strict', path.join(app, p)]);
  for (const p of classification.resources) assert.equal(codesignInfo(path.join(app, p)).team, TEAM, 'team id: ' + p);
  for (const p of classification.machO) assert.equal(codesignInfo(path.join(app, p)).team, TEAM, 'team id: ' + p);
  run('codesign', ['--verify', '--deep', '--strict', '--test-requirement', `=anchor apple generic and certificate leaf[subject.OU] = "${TEAM}"`, app]);
  const machOEquivalence = compareAllChangedMachO(classification, baseline, app);
  assert(machOEquivalence[CLI], 'CLI was not re-signed'); assert(machOEquivalence[MAIN], 'main executable was not re-signed');
  assert(classification.machO.includes(HELPER), 'Expected actual changed helper positive control');
  const machONegativeControl = helperCorruptionControl(baseline, app, HELPER);
  const bundles = Object.keys(signed).filter(p => p.endsWith('.app') || p.endsWith('.framework')).concat(['.']).map(p => p === '.' ? '' : p);
  const entitlementParity = reference ? checkEntitlementParity(app, reference, [...classification.machO, ...bundles.filter(Boolean)]) : null;
  const result = { sourceCommit: b.source_commit, sourceTree: b.source_tree, buildReceiptSha256: sha(receiptPath), inputArchiveSha256: sha(input), inputCliSha256: testedCli,
    signedCliSha256: sha(path.join(app, CLI)), appVersion: b.app_version, app, team: TEAM, electronVersion, osxSignVersion, classification, negativeControls,
    machOEquivalence, machONegativeControl, entitlementParity, reference: reference || null,
    signedFingerprintSha256: sha(path.join(stage, 'after-signing.json')), signingScriptSha256: sha(__filename),
    machOComparatorSha256: sha(path.join(__dirname, 'signature-equivalence.cjs')), signedAt: new Date().toISOString(), notarized: false, uploaded: false, installed: false };
  write(path.join(stage, 'receipt.json'), result);
  console.log(JSON.stringify({ receipt: path.join(stage, 'receipt.json'), changedMachO: classification.machO.length, added: classification.added.length,
    negativeControls: negativeControls.rejected, helperControl: machONegativeControl.nonSignatureCodeChangeRejected, entitlementParity, signedCliSha256: result.signedCliSha256 }));
}
module.exports = { requireSupportedThinMachO, classifyChanges, classificationNegativeControls, compareAllChangedMachO, helperCorruptionControl };
if (require.main === module) main().catch(e => { console.error(String(e && e.stack || e)); process.exit(1); });
