import { useCallback } from 'react';
import { useConfig } from '../ConfigContext';
import { useNavigation } from '../../hooks/useNavigation';
import { startNewSession } from '../../sessions';

/**
 * "Start an AI session about this" from any sidebar row: a new chat in the configured working
 * directory whose FIRST message carries the item (a skill, a memory, an MCP, a desk, a run, a
 * session) so the model reads it before anything else.
 */
export function useStartChatAbout() {
  const setView = useNavigation();
  const { extensionsList } = useConfig();
  return useCallback(
    async (prompt: string) => {
      const configured: unknown = window.electron.getConfig?.().GOOSE_WORKING_DIR;
      const workingDir = typeof configured === 'string' && configured ? configured : '~';
      await startNewSession(prompt, setView, workingDir, { allExtensions: extensionsList });
    },
    [setView, extensionsList]
  );
}
