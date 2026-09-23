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
export const WORK_ON_ITEM_EXTENSIONS = [
  'developer',
  'memory',
  'skills',
  'extensionmanager',
] as const;

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
 * The item's kind and name, read from the FIRST LINE of the prompt each "ask about" builder writes
 * (askAboutSkillPrompt, askAboutMemoryPrompt, askAboutExtensionPrompt, askAboutAgentPrompt,
 * askAboutRunPrompt, askAboutSessionPrompt). useStartChatAbout.test.ts feeds every real builder
 * through this table, so a builder whose first line drifts fails the build instead of quietly
 * titling its sessions "I want to work" again (UX audit C3).
 */
const TITLE_RULES: ReadonlyArray<{ kind: string; pattern: RegExp }> = [
  { kind: 'Skill', pattern: /^I want to work on my goose skill "([^"]+)"/ },
  { kind: 'Memory', pattern: /^I want to work on one of my goose memories — category "([^"]+)"/ },
  { kind: 'MCP', pattern: /^I want to work on my goose MCP extension "([^"]+)"/ },
  { kind: 'Desk', pattern: /^I want to work on my Agent Work desk "([^"]+)"/ },
  { kind: 'Session', pattern: /^I want to work from my earlier goose session "([^"]+)"/ },
  { kind: 'Run', pattern: /^I want to look at my benchmark run (\S+) on (\S+)/ },
];

/** "Memory · lms-ps-is-fleet-ground-truth"; null when the prompt is not an ask-about prompt. */
export function chatAboutTitle(prompt: string): string | null {
  const firstLine = prompt.split('\n', 1)[0] ?? '';
  for (const { kind, pattern } of TITLE_RULES) {
    const m = pattern.exec(firstLine);
    if (m) return [kind, ...m.slice(1)].join(' · ');
  }
  return null;
}

/**
 * "Start an AI session about this" from any sidebar row: a new chat in the configured working
 * directory whose FIRST message carries the item (a skill, a memory, an MCP, a desk, a run, a
 * session) so the model reads it before anything else, with the extensions that can change it.
 * The session is NAMED after the item at creation (a user-set name, so the model's auto-title
 * leaves it alone); a prompt no rule recognises gets no name here and the auto-title applies.
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
        title: chatAboutTitle(prompt) ?? undefined,
      });
    },
    [setView, extensionsList]
  );
}
