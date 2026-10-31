// node --test ui/desktop/scripts/release-split/ (macOS: needs lipo + codesign, no signing identity)
const test = require('node:test'), assert = require('node:assert/strict');
const fs = require('node:fs'), os = require('node:os'), path = require('node:path');
const { execFileSync } = require('node:child_process');
const { compareSignedCli } = require('./signature-equivalence.cjs');
const { requireSupportedThinMachO, classifyChanges, classificationNegativeControls, helperCorruptionControl } = require('./sign-unsigned-app.cjs');

const HELPER = 'Contents/Frameworks/Goose Swarm Helper.app/Contents/MacOS/Goose Swarm Helper';
function pair() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'release-split-test-'));
  const base = path.join(dir, 'baseline'), app = path.join(dir, 'app');
  for (const root of [base, app]) fs.mkdirSync(path.dirname(path.join(root, HELPER)), { recursive: true });
  const thin = path.join(dir, 'thin');
  execFileSync('lipo', ['-thin', 'arm64e', '/usr/bin/true', '-output', thin]);
  fs.copyFileSync(thin, path.join(base, HELPER)); fs.copyFileSync(thin, path.join(app, HELPER));
  execFileSync('codesign', ['-f', '-s', '-', '-i', 'baseline.id', path.join(base, HELPER)]);
  execFileSync('codesign', ['-f', '-s', '-', '-i', 'a.much.longer.signed.identifier.than.before', '-o', 'runtime', path.join(app, HELPER)]);
  return { dir, base, app };
}

test('a re-signed thin Mach-O compares equal and a flipped __text byte is rejected', () => {
  const { dir, base, app } = pair();
  try {
    assert.ok(compareSignedCli(path.join(base, HELPER), path.join(app, HELPER)).equivalentSha256);
    const control = helperCorruptionControl(base, app, HELPER);
    assert.equal(control.nonSignatureCodeChangeRejected, true);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('a universal binary is refused', () => {
  assert.throws(() => requireSupportedThinMachO('/usr/bin/true', 'true'), /thin arm64 or x86_64 required/);
});

test('classification admits only Mach-O changes and CodeResources seals', () => {
  const f = (sha, extra = {}) => ({ kind: 'file', mode: 0o644, xattrs: {}, sha256: sha, bytes: 1, ...extra });
  const before = { '.': { kind: 'directory', mode: 0o755, xattrs: {} }, 'Contents/MacOS/x': f('a', { machO: 0xfeedfacf }), 'Contents/Info.plist': f('p', { machO: 0 }) };
  const after = structuredClone(before);
  after['Contents/MacOS/x'].sha256 = 'b';
  after['Contents/_CodeSignature'] = { kind: 'directory', mode: 0o755, xattrs: {} };
  after['Contents/_CodeSignature/CodeResources'] = f('c');
  const c = classifyChanges(before, after, null, null);
  assert.deepEqual(c.machO, ['Contents/MacOS/x']);
  assert.equal(c.added.length, 2);
  const controls = classificationNegativeControls(before, after);
  assert.deepEqual(controls.rejected, ['resourceBytes', 'extraAttribute', 'changedMode', 'missingPath', 'unexpectedNewFile']);
});
