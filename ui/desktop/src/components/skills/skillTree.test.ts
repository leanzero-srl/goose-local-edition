import { describe, it, expect } from 'vitest';
import { buildSkillTree, defaultExpanded } from './skillTree';

const DIR = '/Users/x/.agents/skills/atlassian-community-leanzero';

/** The real shape: the backend hands back absolute paths, flat, unsorted. */
const FILES = [
  `${DIR}/scripts/browser/reply.mjs`,
  `${DIR}/references/strategy.md`,
  `${DIR}/scripts/monitor.py`,
  `${DIR}/.gitignore`,
  `${DIR}/scripts/browser/auth.mjs`,
  `${DIR}/references/lexicon.md`,
  `${DIR}/state/drafts/arek-reply.txt`,
];

describe('buildSkillTree', () => {
  it('rebuilds the folder structure from flat absolute paths', () => {
    const t = buildSkillTree(DIR, FILES);
    expect(t.fileCount).toBe(7);
    const names = t.children!.map((c) => c.name);
    // directories first, then files — each alphabetical
    expect(names).toEqual(['references', 'scripts', 'state', '.gitignore']);

    const scripts = t.children!.find((c) => c.name === 'scripts')!;
    expect(scripts.fileCount).toBe(3);
    expect(scripts.children!.map((c) => c.name)).toEqual(['browser', 'monitor.py']);

    const browser = scripts.children!.find((c) => c.name === 'browser')!;
    expect(browser.children!.map((c) => c.name)).toEqual(['auth.mjs', 'reply.mjs']);
    expect(browser.fileCount).toBe(2);
  });

  it('keeps the absolute path on files and the relative path on nodes', () => {
    const t = buildSkillTree(DIR, FILES);
    const scripts = t.children!.find((c) => c.name === 'scripts')!;
    const monitor = scripts.children!.find((c) => c.name === 'monitor.py')!;
    expect(monitor.path).toBe('scripts/monitor.py');
    expect(monitor.abs).toBe(`${DIR}/scripts/monitor.py`);
    expect(scripts.abs).toBeUndefined();
  });

  it('never drops a file that sits outside the skill dir', () => {
    // A symlinked or oddly-rooted supporting file must still appear: this view's entire job is "show me
    // every file", so silently losing one is the exact failure it exists to prevent.
    const t = buildSkillTree(DIR, [...FILES, '/somewhere/else/odd.md']);
    expect(t.fileCount).toBe(8);
    expect(JSON.stringify(t)).toContain('odd.md');
  });

  it('handles a skill with no supporting files', () => {
    const t = buildSkillTree(DIR, []);
    expect(t.fileCount).toBe(0);
    expect(t.children).toEqual([]);
  });

  it('does not confuse a file and a folder that share a name', () => {
    const t = buildSkillTree(DIR, [`${DIR}/refs`, `${DIR}/refs/inner.md`]);
    expect(t.fileCount).toBe(2);
    const kids = t.children!.filter((c) => c.name === 'refs');
    expect(kids).toHaveLength(2); // one file node, one dir node — neither swallows the other
    expect(kids.some((k) => k.children)).toBe(true);
    expect(kids.some((k) => k.abs)).toBe(true);
  });
});

describe('defaultExpanded', () => {
  it('opens a small tree', () => {
    const t = buildSkillTree(DIR, FILES);
    const open = defaultExpanded(t);
    expect(open.has('scripts')).toBe(true);
    expect(open.has('scripts/browser')).toBe(true);
  });

  it('leaves a huge skill collapsed — expanding 950 files IS the wall of text', () => {
    const many = Array.from({ length: 950 }, (_, i) => `${DIR}/state/drafts/d${i}.txt`);
    const t = buildSkillTree(DIR, many);
    expect(t.fileCount).toBe(950);
    expect(defaultExpanded(t).size).toBe(0);
  });
});
