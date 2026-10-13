import { createHash } from 'node:crypto';

const SHA256 = /^[a-f0-9]{64}$/;

/** The installed release's exact bytes are the task identity registered by leanzero.net. */
export function forgeTaskFingerprint(rawManifest: Uint8Array): string {
  return createHash('sha256').update(rawManifest).digest('hex');
}

export function forgeTaskPublicationProblem(
  stored: { scorerVersion?: unknown; taskFingerprint?: unknown },
  installedFingerprint: string
): string | null {
  if (stored.scorerVersion !== 'forge-2.0') return null;
  if (typeof stored.taskFingerprint !== 'string' || !SHA256.test(stored.taskFingerprint))
    return 'This Forge 2.0 build has no recorded task receipt. Run the current benchmark again to publish.';
  if (!SHA256.test(installedFingerprint) || stored.taskFingerprint !== installedFingerprint)
    return 'This build used a different Forge 2.0 task from the installed benchmark. Run the current benchmark again to publish.';
  return null;
}

export function forgeTaskPublicationFields(stored: {
  scorerVersion?: unknown;
  taskFingerprint?: unknown;
}): Record<string, string> {
  if (stored.scorerVersion !== 'forge-2.0') return {};
  if (typeof stored.taskFingerprint !== 'string' || !SHA256.test(stored.taskFingerprint))
    throw new Error('Forge 2.0 publication has no recorded task receipt.');
  return { taskFingerprint: stored.taskFingerprint };
}
