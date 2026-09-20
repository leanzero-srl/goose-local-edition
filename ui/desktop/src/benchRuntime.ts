import fs from 'node:fs/promises';
import { constants } from 'node:fs';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execute = promisify(execFile);

export async function resolveBenchmarkRuntime(
  installParent: string
): Promise<{ python: string; node: string; env: Record<string, string> }> {
  if (process.platform !== 'darwin' || process.arch !== 'arm64')
    throw new Error('SB7.1 isolation currently requires macOS on Apple Silicon.');
  const root = path.join(installParent, 'runtime');
  let manifest: Record<string, unknown>;
  try {
    manifest = JSON.parse(await fs.readFile(path.join(root, 'manifest.json'), 'utf8'));
  } catch {
    throw new Error(
      'Benchmark runtime manifest is missing or invalid. Install or retry Benchmark tools in the Benchmark view.'
    );
  }
  if (!manifest || manifest.platform !== process.platform || manifest.arch !== process.arch)
    throw new Error(
      'Benchmark runtime does not match this platform. Install Benchmark tools for this platform.'
    );
  const binaries: Record<string, string> = {};
  for (const name of ['python', 'node', 'ffmpeg', 'ffprobe']) {
    const relative = manifest[name];
    if (typeof relative !== 'string' || !relative || path.isAbsolute(relative))
      throw new Error(`Benchmark runtime has an invalid ${name} path.`);
    const file = path.resolve(root, relative);
    if (!file.startsWith(root + path.sep))
      throw new Error(`Benchmark runtime ${name} escapes its installation.`);
    try {
      await fs.access(file, constants.X_OK);
      const [realRoot, realFile] = await Promise.all([fs.realpath(root), fs.realpath(file)]);
      if (!realFile.startsWith(realRoot + path.sep)) throw new Error('Executable escapes installation');
    } catch {
      throw new Error(
        `Benchmark runtime ${name} is missing or not executable. Install or retry Benchmark tools in the Benchmark view.`
      );
    }
    binaries[name] = file;
  }
  const env = {
    BENCH_FFMPEG: binaries.ffmpeg,
    BENCH_FFPROBE: binaries.ffprobe,
    PATH: [path.dirname(binaries.python), path.dirname(binaries.node), process.env.PATH ?? ''].join(path.delimiter),
  };
  try {
    await execute(
      binaries.python,
      [
        '-B',
        '-c',
        'import sys,sqlite3,ssl,zoneinfo; assert sys.version_info >= (3,11), "Python 3.11 or newer required"',
      ],
      { env: { ...process.env, ...env, PYTHONDONTWRITEBYTECODE: '1' }, timeout: 15000 }
    );
    await execute(binaries.node, ['-e', 'if (Number(process.versions.node.split(".")[0]) < 22) process.exit(1)'], { env: { ...process.env, ...env }, timeout: 15000 });
    await execute(binaries.ffmpeg, ['-version'], {
      env: { ...process.env, ...env },
      timeout: 15000,
    });
    await execute(binaries.ffprobe, ['-version'], {
      env: { ...process.env, ...env },
      timeout: 15000,
    });
  } catch {
    throw new Error(
      'Installed benchmark runtime preflight failed (Python 3.11+, sqlite3, SSL, timezone support, Node 22+ or video tools). Install or retry Benchmark tools in the Benchmark view.'
    );
  }
  return { python: binaries.python, node: binaries.node, env };
}
