// A packaged Electron app ships Electron's prebuilt executables unchanged, so its main executable
// carries the stock Electron LC_UUID — measured 2026-09-24: Goose Swarm 3.0.19's
// `MacOS/Goose Swarm` and node_modules' `Electron.app/Contents/MacOS/Electron` 41.0.0 both report
// 4C4C4450-5555-3144-A175-A5A5EB513DF3. Apple TN3179 (local network privacy): "Local network
// privacy uses your main executable UUID as part of its implementation. If your main executable
// has no UUID, or shares a UUID with other programs, local network privacy may behave weirdly."
// This rewrites the 16-byte LC_UUID payload of every executable in the bundle to a name-based
// (RFC 4122 v5 shape) UUID of bundle id + executable path + arch, so it is unique to this app and
// stable across Electron bumps. It runs in packageAfterCopy, before any signing.

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const MH_MAGIC_64 = 0xfeedfacf;
const FAT_MAGIC = 0xcafebabe;
const FAT_MAGIC_64 = 0xcafebabf;
const LC_UUID = 0x1b;
const CPU_TYPE_ARM64 = 0x0100000c;
const CPU_TYPE_X86_64 = 0x01000007;

function nameUuid(name) {
  const bytes = crypto.createHash('sha1').update(name).digest().subarray(0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  return Buffer.from(bytes);
}

function formatUuid(bytes) {
  const hex = Buffer.from(bytes).toString('hex').toUpperCase();
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function archName(cpuType) {
  if (cpuType === CPU_TYPE_ARM64) return 'arm64';
  if (cpuType === CPU_TYPE_X86_64) return 'x86_64';
  return `cpu${cpuType}`;
}

// The offset of the LC_UUID payload inside one thin 64-bit Mach-O that starts at `base`.
function uuidOffset(buf, base) {
  if (buf.readUInt32LE(base) !== MH_MAGIC_64) {
    throw new Error(`not a 64-bit Mach-O at offset ${base}`);
  }
  const cpuType = buf.readUInt32LE(base + 4);
  const ncmds = buf.readUInt32LE(base + 16);
  let cursor = base + 32;
  for (let i = 0; i < ncmds; i++) {
    const cmd = buf.readUInt32LE(cursor);
    const size = buf.readUInt32LE(cursor + 4);
    if (cmd === LC_UUID) return { offset: cursor + 8, arch: archName(cpuType) };
    cursor += size;
  }
  throw new Error(`no LC_UUID load command in the Mach-O at offset ${base}`);
}

function slices(buf) {
  const magic = buf.readUInt32BE(0);
  if (magic !== FAT_MAGIC && magic !== FAT_MAGIC_64) return [0];
  const count = buf.readUInt32BE(4);
  const out = [];
  for (let i = 0; i < count; i++) {
    out.push(
      magic === FAT_MAGIC
        ? buf.readUInt32BE(8 + i * 20 + 8)
        : Number(buf.readBigUInt64BE(8 + i * 32 + 8))
    );
  }
  return out;
}

/** Rewrites every slice's LC_UUID in `file`; returns [{arch, before, after}]. */
function rewriteMachoUuid(file, seed) {
  const buf = fs.readFileSync(file);
  const changes = slices(buf).map((base) => {
    const { offset, arch } = uuidOffset(buf, base);
    const before = formatUuid(buf.subarray(offset, offset + 16));
    const next = nameUuid(`${seed}:${arch}`);
    next.copy(buf, offset);
    return { arch, before, after: formatUuid(next) };
  });
  fs.writeFileSync(file, buf);
  return changes;
}

/** The main executable and every helper executable under `<bundle>/Contents`. */
function bundleExecutables(contentsDir) {
  const found = [];
  const macos = path.join(contentsDir, 'MacOS');
  for (const name of fs.readdirSync(macos)) found.push(path.join(macos, name));
  const frameworks = path.join(contentsDir, 'Frameworks');
  if (fs.existsSync(frameworks)) {
    for (const name of fs.readdirSync(frameworks)) {
      if (!name.endsWith('.app')) continue;
      const helperMacos = path.join(frameworks, name, 'Contents', 'MacOS');
      for (const exe of fs.readdirSync(helperMacos)) found.push(path.join(helperMacos, exe));
    }
  }
  return found;
}

/**
 * `resourcesPath` is forge's packageAfterCopy argument (`<app>/Contents/Resources/app`); the
 * executables are still named after Electron at that point, so the seed uses the path relative to
 * Contents, which is the same on every build of this app.
 */
function uniquifyBundle(resourcesPath, bundleId) {
  const contentsDir = path.resolve(resourcesPath, '..', '..');
  return bundleExecutables(contentsDir).map((file) => ({
    file: path.relative(contentsDir, file),
    changes: rewriteMachoUuid(file, `${bundleId}:${path.relative(contentsDir, file)}`),
  }));
}

module.exports = { nameUuid, formatUuid, rewriteMachoUuid, uniquifyBundle };
