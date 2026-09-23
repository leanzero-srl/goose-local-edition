import { describe, expect, it } from 'vitest';
import {
  WORK_ON_ITEM_EXTENSIONS,
  chatAboutTitle,
  extensionsForWorkOnItem,
} from './useStartChatAbout';
import type { FixedExtensionEntry } from '../ConfigContext';
import { askAboutSkillPrompt } from '../skills/SkillsView';
import { askAboutMemoryPrompt } from '../memories/MemoriesView';
import { askAboutExtensionPrompt } from '../settings/extensions/subcomponents/ExtensionItem';
import { askAboutSessionPrompt } from './ProjectsSection';
import { askAboutAgentPrompt } from './AgentWorkSection';
import { askAboutRunPrompt } from './BenchmarkSection';

/** A "work on this item" chat loads EXACTLY what can change the item — memory, skills, developer
 *  and extension-manager, even when the profile has them off — plus what the item names, and
 *  nothing else: the profile's other enabled extensions pushed an MLX session past the engine's
 *  grammar-compile bounds on its first turn (62 tools / 55,750 bytes, 2026-09-23). */
const entry = (name: string, enabled: boolean): FixedExtensionEntry =>
  ({ type: 'builtin', name, enabled, timeout: 300 }) as FixedExtensionEntry;

describe('extensionsForWorkOnItem', () => {
  it('loads the required set whatever the profile says, and leaves every other enabled extension out', () => {
    const all = [
      entry('developer', true),
      entry('memory', false),
      entry('skills', false),
      entry('extensionmanager', false),
      entry('todo', false),
      entry('analyze', true),
      entry('playwright', true),
    ];
    const names = extensionsForWorkOnItem(all).map((c) => c.name);
    expect(names).toEqual(['developer', 'memory', 'skills', 'extensionmanager']);
    expect(names).not.toContain('analyze');
    expect(names).not.toContain('playwright');
    expect(WORK_ON_ITEM_EXTENSIONS).toEqual(['developer', 'memory', 'skills', 'extensionmanager']);
  });

  it('adds what the item names (the MCP being asked about, chatrecall) even when it is off', () => {
    const all = [
      entry('developer', true),
      entry('memory', true),
      entry('jira', false),
      entry('chatrecall', false),
      entry('playwright', true),
    ];
    const names = extensionsForWorkOnItem(all, [...WORK_ON_ITEM_EXTENSIONS, 'jira']).map(
      (c) => c.name
    );
    expect(names).toEqual(['developer', 'memory', 'jira']);
  });

  it('never invents an extension the profile does not carry, and strips the enabled flag', () => {
    const configs = extensionsForWorkOnItem([entry('developer', true)]);
    expect(configs.map((c) => c.name)).toEqual(['developer']);
    expect('enabled' in configs[0]).toBe(false);
  });
});

/** UX audit C3: three ask-AI sessions in the sidebar all read "I want to work". Each session is
 *  named after its item at creation; every REAL builder goes through the title table here, so a
 *  builder whose first line drifts fails this test instead of silently losing its name. */
describe('chatAboutTitle', () => {
  it('names a session after the item each real ask-about builder carries', () => {
    expect(
      chatAboutTitle(
        askAboutMemoryPrompt({
          id: 'm',
          category: 'lms-ps-is-fleet-ground-truth',
          scope: 'global',
          tags: ['reference'],
          content: 'Fleet diagnosis: lms ps is truth for BUSY-vs-IDLE.',
          updatedAt: 0,
        })
      )
    ).toBe('Memory · lms-ps-is-fleet-ground-truth');
    expect(
      chatAboutTitle(
        askAboutSkillPrompt(
          {
            name: 'goose-feature-dev',
            description: 'build features in goose',
            path: '/Users/me/.agents/skills/goose-feature-dev',
            type: 'skill',
            global: true,
          } as never,
          '/Users/me'
        )
      )
    ).toBe('Skill · goose-feature-dev');
    expect(
      chatAboutTitle(
        askAboutExtensionPrompt({
          type: 'stdio',
          name: 'jira',
          display_name: 'Jira',
          cmd: 'npx',
          args: ['jira-mcp'],
          enabled: true,
          timeout: 300,
          envs: {},
        } as never)
      )
    ).toMatch(/^MCP · /);
    expect(
      chatAboutTitle(
        askAboutAgentPrompt({
          dir: '/desks/public-web-research',
          manifest: { title: 'Public web research' },
        } as never)
      )
    ).toBe('Desk · Public web research');
    expect(
      chatAboutTitle(
        askAboutSessionPrompt({ id: 's1', name: 'Fix login', workingDir: '/w' } as never, {
          chatRecall: true,
        })
      )
    ).toBe('Session · Fix login');
    expect(
      chatAboutTitle(
        askAboutRunPrompt(
          { scorerVersion: 'sb-7.1', title: 'Vendor sync' } as never,
          {
            runId: 'r0-qwen',
            startedAt: '2026-09-21T10:00:00Z',
            outcome: 'scored',
            score: 0.46,
          } as never
        )
      )
    ).toBe('Run · r0-qwen · sb-7.1');
  });

  it('a prompt no rule recognises gets no name (the auto-title applies), never a generic one', () => {
    expect(chatAboutTitle('I want to work on something else entirely')).toBeNull();
    expect(chatAboutTitle('')).toBeNull();
  });
});
