import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const platform = process.env.ELECTRON_PLATFORM || process.platform;
const arch = process.env.ELECTRON_ARCH || process.arch;
const pins = JSON.parse(
  fs.readFileSync(path.join(root, 'scripts/benchmark-runtimes.json'), 'utf8')
);
const selected = pins[`${platform}-${arch}`];
const output = path.join(root, 'benchmark-runtime');
fs.mkdirSync(output, { recursive: true });
if (!selected) {
  fs.writeFileSync(
    path.join(output, 'manifest.json'),
    JSON.stringify({ platform, arch, supported: false })
  );
  console.log(`SB7.1 runtime is not supported on ${platform}-${arch}`);
  process.exit(0);
}
const manifest = { platform, arch };
for (const [name, pin] of Object.entries(selected)) {
  const marker = path.join(output, `.${name}-integrity`);
  const executable = path.join(output, pin.executable);
  if (
    !fs.existsSync(marker) ||
    fs.readFileSync(marker, 'utf8') !== pin.integrity ||
    !fs.existsSync(executable)
  ) {
    const response = await fetch(pin.url);
    if (!response.ok) throw new Error(`Cannot download ${name}: HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    const [algorithm, expected] = pin.integrity.split(':');
    const actual = createHash(algorithm)
      .update(bytes)
      .digest(algorithm === 'sha256' ? 'hex' : 'base64');
    if (actual !== expected) throw new Error(`Integrity mismatch for ${name}`);
    const archive = path.join(output, `${name}.tar.gz`);
    fs.writeFileSync(archive, bytes);
    const destination = name === 'python' ? output : path.join(output, name);
    fs.rmSync(path.join(output, name), { recursive: true, force: true });
    fs.mkdirSync(destination, { recursive: true });
    execFileSync('tar', [
      '-xzf',
      archive,
      '-C',
      destination,
      `--strip-components=${pin.stripComponents}`,
    ]);
    fs.unlinkSync(archive);
    fs.chmodSync(executable, 0o755);
    fs.writeFileSync(marker, pin.integrity);
  }
  const args =
    name === 'python'
      ? ['-B', '-c', 'import sqlite3,ssl,zoneinfo; print("Python runtime ready")']
      : name === 'node' ? ['--version'] : ['-version'];
  const proof = execFileSync(executable, args, {
    env: { PATH: '/usr/bin:/bin', PYTHONDONTWRITEBYTECODE: '1' },
    encoding: 'utf8',
  });
  console.log(`${name}: ${proof.split('\n')[0]}`);
  manifest[name] = pin.executable;
}
fs.writeFileSync(path.join(output, 'manifest.json'), JSON.stringify(manifest, null, 2));
fs.copyFileSync(
  path.join(root, 'scripts/benchmark-runtimes.json'),
  path.join(output, 'sources.json')
);
