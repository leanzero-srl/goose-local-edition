import { acpImportSession } from './acp/sessions';
import { getInitialWorkingDir } from './utils/workingDir';

/**
 * Imports a session from an encrypted Nostr deep link.
 */
export async function importNostrSessionFromDeepLink(url: string): Promise<void> {
  await acpImportSession(url, 'nostr', getInitialWorkingDir());
}
