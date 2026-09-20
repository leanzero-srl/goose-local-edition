import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import pins from '../scripts/benchmark-runtimes.json';
import { resolveBenchmarkRuntime } from './benchRuntime';
import type { BenchmarkRuntimeProgress, BenchmarkRuntimeStatus } from './benchRuntimeTypes';
export type { BenchmarkRuntimeProgress, BenchmarkRuntimeStatus } from './benchRuntimeTypes';

const execute = promisify(execFile);
const installations = new Map<string, Promise<void>>();
type Pin = {
  url: string;
  integrity: string;
  executable: string;
  stripComponents: number;
  downloadBytes: number;
};

function selectedPins(): Record<string, Pin> | undefined {
  return (pins as Record<string, Record<string, Pin>>)[`${process.platform}-${process.arch}`];
}

export async function inspectBenchmarkRuntime(
  installParent: string
): Promise<BenchmarkRuntimeStatus> {
  const selected = selectedPins();
  if (!selected)
    return {
      state: 'unsupported',
      downloadBytes: 0,
      error: 'SB7.1 currently requires macOS on Apple Silicon.',
    };
  const downloadBytes = Object.values(selected).reduce((sum, pin) => sum + pin.downloadBytes, 0);
  const root = path.join(installParent, 'runtime');
  let raw: string;
  try {
    raw = await fs.readFile(path.join(root, 'manifest.json'), 'utf8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT')
      return { state: 'missing', downloadBytes };
    return {
      state: 'invalid',
      downloadBytes,
      error: 'Benchmark tools cannot be read. Install them again.',
    };
  }
  try {
    const manifest = JSON.parse(raw);
    if (manifest.platform !== process.platform || manifest.arch !== process.arch)
      throw new Error('platform');
    for (const [name, pin] of Object.entries(selected)) {
      if (manifest[name] !== pin.executable || manifest.integrities?.[name] !== pin.integrity)
        throw new Error('version');
      await fs.access(path.join(root, pin.executable), fs.constants.X_OK);
    }
    return { state: 'ready', downloadBytes };
  } catch {
    return {
      state: 'invalid',
      downloadBytes,
      error: 'Benchmark tools are incomplete or outdated. Install them again.',
    };
  }
}

export function installBenchmarkRuntime(
  installParent: string,
  onProgress: (progress: BenchmarkRuntimeProgress) => void
): Promise<void> {
  const existing = installations.get(installParent);
  if (existing) return existing;
  const installation = performInstall(installParent, onProgress).finally(() =>
    installations.delete(installParent)
  );
  installations.set(installParent, installation);
  return installation;
}

async function performInstall(
  installParent: string,
  onProgress: (progress: BenchmarkRuntimeProgress) => void
) {
  const selected = selectedPins();
  if (!selected) throw new Error('SB7.1 currently requires macOS on Apple Silicon.');
  const totalBytes = Object.values(selected).reduce((sum, pin) => sum + pin.downloadBytes, 0);
  let receivedBytes = 0;
  await fs.mkdir(installParent, { recursive: true });
  const staging = await fs.mkdtemp(path.join(installParent, '.runtime-install-'));
  const root = path.join(staging, 'runtime');
  await fs.mkdir(root);
  const manifest: Record<string, unknown> = {
    platform: process.platform,
    arch: process.arch,
    integrities: {},
  };
  try {
    for (const [name, pin] of Object.entries(selected)) {
      onProgress({ phase: 'downloading', receivedBytes, totalBytes });
      const response = await fetch(pin.url);
      if (!response.ok || !response.body)
        throw new Error(`Could not download ${name} (HTTP ${response.status}). Please retry.`);
      const archive = path.join(staging, `${name}.tar.gz`);
      const file = await fs.open(archive, 'wx');
      const [algorithm, expected] = pin.integrity.split(':');
      const hash = createHash(algorithm);
      try {
        for await (const part of response.body as unknown as AsyncIterable<Uint8Array>) {
          hash.update(part);
          await file.writeFile(part);
          receivedBytes += part.length;
          onProgress({ phase: 'downloading', receivedBytes, totalBytes });
        }
      } finally {
        await file.close();
      }
      if (hash.digest(algorithm === 'sha256' ? 'hex' : 'base64') !== expected) {
        throw new Error(`The ${name} download failed its integrity check. Please retry.`);
      }
      onProgress({ phase: 'extracting', receivedBytes, totalBytes });
      const destination = name === 'python' ? root : path.join(root, name);
      await fs.mkdir(destination, { recursive: true });
      await execute('/usr/bin/tar', [
        '-xzf',
        archive,
        '-C',
        destination,
        `--strip-components=${pin.stripComponents}`,
      ]);
      await fs.chmod(path.join(root, pin.executable), 0o755);
      await fs.unlink(archive);
      manifest[name] = pin.executable;
      (manifest.integrities as Record<string, string>)[name] = pin.integrity;
    }
    await fs.writeFile(path.join(root, 'manifest.json'), JSON.stringify(manifest, null, 2));
    await fs.writeFile(path.join(root, 'sources.json'), JSON.stringify(selected, null, 2));
    onProgress({ phase: 'verifying', receivedBytes, totalBytes });
    await resolveBenchmarkRuntime(staging);
    const target = path.join(installParent, 'runtime');
    const previous = path.join(staging, 'previous');
    let hadPrevious = false;
    try {
      await fs.rename(target, previous);
      hadPrevious = true;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
    }
    try {
      await fs.rename(root, target);
    } catch (error) {
      if (hadPrevious) await fs.rename(previous, target);
      throw error;
    }
  } finally {
    await fs.rm(staging, { recursive: true, force: true });
  }
}
