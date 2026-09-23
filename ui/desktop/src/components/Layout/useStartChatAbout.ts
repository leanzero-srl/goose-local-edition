import { useCallback } from 'react';
import { useConfig, type FixedExtensionEntry } from '../ConfigContext';
import { useNavigation } from '../../hooks/useNavigation';
import { startNewSession } from '../../sessions';
import type { ExtensionConfig } from '../../types/extensions';

/**
 * The ONLY extensions a "work on this item" chat loads (plus the ones the item itself names). The
 * set is what the job needs to CREATE or MODIFY the item, not only talk about it (Mihai
 * 2026-09-22: "we need to give goose the capability to also modify or create new ones, both mcp,
 * memories, skills"): `developer` writes SKILL.md and config files, `memory` carries
 * remember_memory / remove_specific_memory, `skills` loads a skill by name, and
 * `extensionmanager` enables, disables and discovers MCPs.
 *
 * Why exactly these and not "everything enabled, plus these": every loaded extension's tool
 * schema rides every request, and an engine that compiles tools into a grammar refuses a set
 * past its bounds. MEASURED 2026-09-23: an ask-AI session on the in-house MLX engine (rapid-mlx
 * 0.14.3) failed its FIRST turn with "tool schema exceeds grammar-compile bounds (max 256 tools,
 * 65536 bytes, depth 32)" — the profile's enabled set sent 62 tools / 55,750 bytes before
 * LeanZero Documents pushed it over (playwright alone 25 tools / 19,305 B), none of which an
 * edit-this-skill session uses.
 */
export const WORK_ON_ITEM_EXTENSIONS = [
  'developer',
  'memory',
  'skills',
  'extensionmanager',
] as const;

/** The session's extension set: exactly the `required` names the profile carries, whatever their
 *  enabled flag — the profile's other enabled extensions stay out. A required one the profile does
 *  not configure at all is left out here and named by the prompt, never silently substituted. */
export function extensionsForWorkOnItem(
  all: FixedExtensionEntry[],
  required: readonly string[] = WORK_ON_ITEM_EXTENSIONS
): ExtensionConfig[] {
  return all
    .filter((extension) => required.includes(extension.name))
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
 * session) so the model reads it before anything else, with only the extensions that can change it.
 * The session is NAMED after the item at creation (a user-set name, so the model's auto-title
 * leaves it alone); a prompt no rule recognises gets no name here and the auto-title applies.
 */
export function useStartChatAbout() {
  const setView = useNavigation();
  const { extensionsList } = useConfig();
  return useCallback(
    /** `alsoEnable`: extensions the item itself names — the ones its prompt TELLS the model to use
     *  (a session ask names chatrecall) or the item IS (an MCP ask loads that MCP so it can be
     *  tried) — loaded for this session when the profile has them. */
    async (prompt: string, options?: { alsoEnable?: readonly string[] }) => {
      const configured: unknown = window.electron.getConfig?.().GOOSE_WORKING_DIR;
      const workingDir = typeof configured === 'string' && configured ? configured : '~';
      await startNewSession(prompt, setView, workingDir, {
        extensionConfigs: extensionsForWorkOnItem(extensionsList, [
          ...WORK_ON_ITEM_EXTENSIONS,
          ...(options?.alsoEnable ?? []),
        ]),
        title: chatAboutTitle(prompt) ?? undefined,
      });
    },
    [setView, extensionsList]
  );
}
