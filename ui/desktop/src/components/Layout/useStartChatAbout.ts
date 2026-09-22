import { useCallback } from 'react';
import { useConfig, type FixedExtensionEntry } from '../ConfigContext';
import { useNavigation } from '../../hooks/useNavigation';
import { startNewSession } from '../../sessions';
import type { ExtensionConfig } from '../../types/extensions';

/**
 * The extensions a "work on this item" chat needs to CREATE or MODIFY the item, not only talk
 * about it (Mihai 2026-09-22: "we need to give goose the capability to also modify or create new
 * ones, both mcp, memories, skills"): `developer` writes SKILL.md and config files, `memory`
 * carries remember_memory / remove_specific_memory, `skills` loads a skill by name, and
 * `extensionmanager` enables, disables and discovers MCPs.
 */
export const WORK_ON_ITEM_EXTENSIONS = ['developer', 'memory', 'skills', 'extensionmanager'] as const;

/** The session's extension set: everything enabled, plus the required ones the profile has turned
 *  off. Only extensions present in the profile can be added — a required one that is not
 *  configured at all is left out here and named by the prompt, never silently substituted. */
export function extensionsForWorkOnItem(
  all: FixedExtensionEntry[],
  required: readonly string[] = WORK_ON_ITEM_EXTENSIONS
): ExtensionConfig[] {
  return all
    .filter((extension) => extension.enabled || required.includes(extension.name))
    .map((extension) => {
      const { enabled: _enabled, ...config } = extension;
      return config as ExtensionConfig;
    });
}

/**
 * "Start an AI session about this" from any sidebar row: a new chat in the configured working
 * directory whose FIRST message carries the item (a skill, a memory, an MCP, a desk, a run, a
 * session) so the model reads it before anything else, with the extensions that can change it.
 */
export function useStartChatAbout() {
  const setView = useNavigation();
  const { extensionsList } = useConfig();
  return useCallback(
    async (prompt: string) => {
      const configured: unknown = window.electron.getConfig?.().GOOSE_WORKING_DIR;
      const workingDir = typeof configured === 'string' && configured ? configured : '~';
      await startNewSession(prompt, setView, workingDir, {
        extensionConfigs: extensionsForWorkOnItem(extensionsList),
      });
    },
    [setView, extensionsList]
  );
}
