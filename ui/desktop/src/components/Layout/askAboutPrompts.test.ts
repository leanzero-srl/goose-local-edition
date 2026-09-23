import { describe, expect, it } from 'vitest';
import { askAboutSkillPrompt } from '../skills/SkillsView';
import { askAboutMemoryPrompt } from '../memories/MemoriesView';
import { askAboutExtensionPrompt } from '../settings/extensions/subcomponents/ExtensionItem';
import { askAboutSessionPrompt } from './ProjectsSection';
import { askAboutAgentPrompt } from './AgentWorkSection';
import { askAboutRunPrompt } from './BenchmarkSection';

/**
 * Every "start an AI session about this" prompt is OPERATIONAL (it names where the item lives, which
 * tool changes it, how to fork it, and asks before writing — Mihai 2026-09-22) and FACTUAL: it states
 * only what the app read for THIS item. UX audit T2 (2026-09-23): the skill prompt said "sibling files
 * in the same folder are its references" about a folder holding only SKILL.md, and a local model
 * answered — with no tool call — that it had read "four sibling reference files", citing two invented
 * ones. Each builder is tested with the item's facts present AND absent.
 */
const skill = (over: Record<string, unknown> = {}) =>
  ({
    type: 'skill',
    name: 'release-checklist',
    description: 'How to run a release',
    content: '# Release',
    path: '/Users/me/.agents/skills/release-checklist',
    global: true,
    ...over,
  }) as never;

describe('the skill prompt states the folder it was given, never an assumed one', () => {
  it('a folder holding only SKILL.md is said to hold nothing else — no "sibling references"', () => {
    const p = askAboutSkillPrompt(skill(), '/Users/me');
    expect(p).toContain('the file /Users/me/.agents/skills/release-checklist/SKILL.md');
    expect(p).not.toContain('the file /Users/me/.agents/skills/release-checklist —');
    expect(p).toContain('found no other files');
    expect(p).toContain('SKILL.md is all there is');
    expect(p).not.toMatch(/sibling|references/i);
    expect(p).toContain('It is a global skill');
    expect(p).toContain('~/.agents/skills/<new-name>/SKILL.md');
    expect(p).toContain('/Users/me/.agents/skills/<new-name>/SKILL.md');
    expect(p).toContain('frontmatter');
    expect(p).toContain('fork');
    expect(p).toContain('Ask me what I want changed before you write anything');
  });

  it('a folder with files names exactly those files, relative to the skill folder', () => {
    const p = askAboutSkillPrompt(
      skill({
        global: false,
        path: '/w/proj/.agents/skills/deploy',
        name: 'deploy',
        supportingFiles: [
          '/w/proj/.agents/skills/deploy/scripts/ship.sh',
          '/w/proj/.agents/skills/deploy/docs/gotchas.md',
        ],
      }),
      '/w/proj'
    );
    expect(p).toContain('found 2 other files: docs/gotchas.md, scripts/ship.sh.');
    expect(p).toContain('Open a file before you say anything about what it holds.');
    expect(p).not.toContain('no other files');
    expect(p).toContain('It is a project skill: goose sees it only when working in /w/proj.');
    expect(p).toContain('/w/proj/.agents/skills/<new-name>/SKILL.md');
  });

  it('a very large folder names a bounded list and counts the rest', () => {
    const files = Array.from({ length: 75 }, (_, i) => `/s/big/f${String(i).padStart(2, '0')}.md`);
    const p = askAboutSkillPrompt(skill({ path: '/s/big', supportingFiles: files }), '/w');
    expect(p).toContain('found 75 other files');
    expect(p).toContain('f59.md, and 15 more not named here');
    expect(p).not.toContain('f60.md');
  });

  it('a built-in skill is said to have no file on disk and to be forked, not edited', () => {
    const p = askAboutSkillPrompt(
      skill({
        type: 'builtinSkill',
        path: 'builtin://skills/goose-doc-guide',
        name: 'goose-doc-guide',
      }),
      '/w'
    );
    expect(p).toContain('there is no file on disk');
    expect(p).toContain('load_skill with the name "goose-doc-guide"');
    expect(p).not.toContain('SKILL.md —');
    expect(p).not.toContain('builtin://skills/goose-doc-guide/SKILL.md');
  });

  it('a persona says which part the swarm rewrites and where a lasting change goes', () => {
    const p = askAboutSkillPrompt(skill({ path: '/c/goose/skills/stack-python-fastapi' }), '/w');
    expect(p).toContain('goose wrote this skill about itself');
    expect(p).toContain('"## Your notes" heading');
  });
});

describe('the memory prompt names the file it was read from', () => {
  const base = {
    id: 'a',
    category: 'vendor-api',
    scope: 'local' as const,
    tags: ['feedback'],
    content: 'use cursors',
    updatedAt: 0,
  };

  it('with the file the list parsed it from', () => {
    const p = askAboutMemoryPrompt({ ...base, filePath: '/w/proj/.goose/memory/vendor-api.txt' });
    expect(p).toContain('It is one entry in the file /w/proj/.goose/memory/vendor-api.txt.');
    expect(p).toContain('local scope');
    expect(p).toContain('remember_memory');
    expect(p).toContain('remove_specific_memory');
    expect(p).toContain('use cursors');
  });

  it('without one it says so instead of guessing a path', () => {
    const p = askAboutMemoryPrompt({ ...base, scope: 'global', tags: [] });
    expect(p).toContain('The app did not report which file holds it.');
    expect(p).not.toContain('<working dir>');
    expect(p).not.toContain('<category>.txt');
    expect(p).toContain('tags: none');
  });
});

describe('the MCP prompt names the real config key', () => {
  it('a display name with spaces is stored under its derived key, not the name', () => {
    const p = askAboutExtensionPrompt({
      type: 'stdio',
      name: 'LeanZero Documents',
      cmd: '/Applications/Goose Swarm.app/Contents/Resources/bin/node',
      args: ['index.js'],
      enabled: true,
      timeout: 300,
      envs: {},
    } as never);
    expect(p).toContain('"leanzerodocuments" key under extensions: in ~/.config/goose/config.yaml');
    expect(p).toContain('bin/node index.js');
    expect(p).toContain('manage_extensions');
    expect(p).toContain('brand-new MCP');
  });

  it('the key the config was read from wins over the derivation', () => {
    const p = askAboutExtensionPrompt({
      type: 'stdio',
      name: 'jira',
      configKey: 'jira-cloud',
      cmd: 'npx',
      args: ['jira-mcp'],
      enabled: true,
      timeout: 300,
      envs: {},
    } as never);
    expect(p).toContain('"jira-cloud" key under extensions:');
  });
});

describe('the session prompt names the one way the chat can read the session', () => {
  const session = {
    id: 's1',
    name: 'Fix login',
    workingDir: '/w',
    messageCount: 14,
    createdAt: '2026-09-20T10:00:00Z',
    updatedAt: '2026-09-21T11:00:00Z',
  } as never;

  it('with chatrecall in the profile it names the tool call', () => {
    const p = askAboutSessionPrompt(session, { chatRecall: true });
    expect(p).toContain('14 messages');
    expect(p).toContain('chatrecall tool with session_id "s1"');
    expect(p).toContain('turn what it learned into a skill or memory');
    expect(p).toContain('Ask me what I want before you write anything');
  });

  it('without it the prompt says nothing is attached rather than "use the session tools if you have them"', () => {
    const p = askAboutSessionPrompt(session, { chatRecall: false });
    expect(p).toContain('None of its messages are attached here');
    expect(p).not.toContain('chatrecall tool with');
    expect(p).not.toContain('if you have them');
  });
});

describe('the desk prompt states what the roster read', () => {
  it('a readable agent.yaml and run state are quoted from the read', () => {
    const p = askAboutAgentPrompt({
      dir: '/desks/ops',
      manifest: {
        name: 'ops',
        cadence: '15m',
        timezone: 'Europe/Bucharest',
        surgeons: [{ name: 'fixer' }],
      },
      state: { status: 'idle', tick: 7, phase: 'sleep' },
    } as never);
    expect(p).toContain('/desks/ops/agent.yaml, which the app read: cadence 15m');
    expect(p).toContain('surgeons fixer');
    expect(p).toContain('/desks/ops/.swarm/agent/state.json: status idle, tick 7');
    expect(p).toContain('fork the desk');
  });

  it('an unreadable agent.yaml and absent state are said to be so', () => {
    const p = askAboutAgentPrompt({ dir: '/desks/ops', manifest: null, state: null } as never);
    expect(p).toContain('could not read /desks/ops/agent.yaml');
    expect(p).toContain('no readable run state');
    expect(p).not.toContain('cadence');
  });
});

describe('the run prompt points at the run folder only when main found it', () => {
  const era = { scorerVersion: 'sb-7.1', title: 'Vendor sync' } as never;

  it('with a folder, tiers and a scoring error', () => {
    const p = askAboutRunPrompt(era, {
      runId: 'r0',
      startedAt: '2026-09-21T10:00:00Z',
      outcome: 'finished',
      score: 0.4616,
      tiers: { t1: 0.5 },
      scoringError: 'port 8850 busy',
      dataDir: '/data/benchmark/sessions/r0',
    } as never);
    expect(p).toContain('46.2%');
    expect(p).toContain('Tier scores: t1 50.0%.');
    expect(p).toContain('Scoring failed: port 8850 busy');
    expect(p).toContain('Its files are in /data/benchmark/sessions/r0');
    expect(p).not.toContain('benchmark/runs folder');
  });

  it('without one it says there are no files', () => {
    const p = askAboutRunPrompt(era, {
      runId: null,
      startedAt: '2026-09-21T10:00:00Z',
      outcome: 'did_not_start',
    } as never);
    expect(p).toContain('The app has no folder on disk for this run');
    expect(p).toContain('no score');
  });
});
