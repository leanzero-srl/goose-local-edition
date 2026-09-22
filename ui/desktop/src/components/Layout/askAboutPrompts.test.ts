import { describe, expect, it } from 'vitest';
import { askAboutSkillPrompt } from '../skills/SkillsView';
import { askAboutMemoryPrompt } from '../memories/MemoriesView';
import { askAboutExtensionPrompt } from '../settings/extensions/subcomponents/ExtensionItem';
import { askAboutSessionPrompt } from './ProjectsSection';
import { askAboutAgentPrompt } from './AgentWorkSection';

/** Every "start an AI session about this" prompt is OPERATIONAL: it names where the item lives,
 *  which tool creates or modifies it, how to fork it, and asks before writing — the chat exists to
 *  change the item, not only to discuss it (Mihai 2026-09-22). */
describe('ask-about prompts are operational', () => {
  it('a skill prompt names the SKILL.md path, the frontmatter, the fork location and asks first', () => {
    const p = askAboutSkillPrompt({
      name: 'deploy',
      description: 'ship it',
      path: '/Users/me/.agents/skills/deploy/SKILL.md',
      origin: 'global',
    } as never);
    expect(p).toContain('/Users/me/.agents/skills/deploy/SKILL.md');
    expect(p).toContain('frontmatter');
    expect(p).toContain('~/.agents/skills/<name>/');
    expect(p).toContain('fork');
    expect(p).toContain('Ask me what I want changed before you write anything');
  });

  it('a memory prompt names the store path for its scope and the memory tools', () => {
    const local = askAboutMemoryPrompt({
      id: 'a',
      category: 'vendor-api',
      scope: 'local',
      tags: ['feedback'],
      content: 'use cursors',
      updatedAt: 0,
    });
    expect(local).toContain('.goose/memory/<category>.txt');
    expect(local).toContain('remember_memory');
    expect(local).toContain('remove_specific_memory');
    expect(local).toContain('use cursors');
    const global = askAboutMemoryPrompt({
      id: 'b',
      category: 'style',
      scope: 'global',
      tags: [],
      content: 'x',
      updatedAt: 0,
    });
    expect(global).toContain('~/.config/goose/memory/<category>.txt');
    expect(global).toContain('tags: none');
  });

  it('an MCP prompt names the config.yaml entry, the launch command and manage_extensions', () => {
    const p = askAboutExtensionPrompt({
      type: 'stdio',
      name: 'jira',
      cmd: 'npx',
      args: ['jira-mcp'],
      enabled: true,
      timeout: 300,
      envs: {},
    } as never);
    expect(p).toContain('"jira" entry under extensions: in ~/.config/goose/config.yaml');
    expect(p).toContain('npx jira-mcp');
    expect(p).toContain('manage_extensions');
    expect(p).toContain('brand-new MCP');
  });

  it('a session prompt and a desk prompt say what can be made from them and ask first', () => {
    const s = askAboutSessionPrompt({
      id: 's1',
      name: 'Fix login',
      workingDir: '/w',
    } as never);
    expect(s).toContain('turn what it learned into a skill or memory');
    expect(s).toContain('Ask me what I want before you write anything');
    const d = askAboutAgentPrompt({ dir: '/desks/ops', name: 'ops' } as never);
    expect(d).toContain('agent.yaml');
    expect(d).toContain('fork the desk');
  });
});
