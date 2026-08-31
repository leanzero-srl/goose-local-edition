import fs from 'fs';
import os from 'os';
import path from 'path';
import { beforeEach, describe, expect, it, vi } from 'vitest';

let userDataDir = '';

vi.mock('electron', () => ({
  app: {
    getPath: vi.fn(() => userDataDir),
  },
}));

import { addProject, loadProjects, removeProject } from '../projectDirs';

function makeDir(name: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `${name}-`));
  return dir;
}

describe('projects registry (projects.json)', () => {
  beforeEach(() => {
    userDataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'proj-userdata-'));
  });

  it('round-trips add -> load with canonical path and addedAt', () => {
    const dir = makeDir('alpha');
    const added = addProject(dir);
    expect(added).toHaveLength(1);
    expect(added[0].path).toBe(path.resolve(dir));
    expect(typeof added[0].addedAt).toBe('number');

    const loaded = loadProjects();
    expect(loaded).toEqual(added);

    const raw = JSON.parse(fs.readFileSync(path.join(userDataDir, 'projects.json'), 'utf8')) as {
      projects: unknown[];
    };
    expect(raw.projects).toHaveLength(1);
  });

  it('dedupes by canonical path (trailing slash is the same project)', () => {
    const dir = makeDir('beta');
    addProject(`${dir}/`);
    const after = addProject(dir);
    expect(after).toHaveLength(1);
    expect(after[0].path).toBe(path.resolve(dir));
  });

  it('newest project first', () => {
    const first = makeDir('first');
    const second = makeDir('second');
    addProject(first);
    const after = addProject(second);
    expect(after.map((p) => p.path)).toEqual([path.resolve(second), path.resolve(first)]);
  });

  it('rejects symlinks, nonexistent paths, files, and relative paths', () => {
    const target = makeDir('target');
    const link = path.join(os.tmpdir(), `link-${Date.now()}`);
    fs.symlinkSync(target, link);
    expect(addProject(link)).toHaveLength(0);
    fs.unlinkSync(link);

    expect(addProject(path.join(os.tmpdir(), 'does-not-exist-xyz'))).toHaveLength(0);

    const file = path.join(makeDir('holder'), 'a-file.txt');
    fs.writeFileSync(file, 'x');
    expect(addProject(file)).toHaveLength(0);

    expect(addProject('relative/path')).toHaveLength(0);
  });

  it('remove edits the registry ONLY — the directory and its contents survive', () => {
    const dir = makeDir('keepme');
    const marker = path.join(dir, 'marker.txt');
    fs.writeFileSync(marker, 'still here');
    addProject(dir);

    const after = removeProject(dir);
    expect(after).toHaveLength(0);
    expect(loadProjects()).toHaveLength(0);

    expect(fs.existsSync(dir)).toBe(true);
    expect(fs.readFileSync(marker, 'utf8')).toBe('still here');
  });

  it('prunes entries whose directory vanished and rewrites the file', () => {
    const stays = makeDir('stays');
    const goes = makeDir('goes');
    addProject(stays);
    addProject(goes);
    fs.rmdirSync(goes);

    const loaded = loadProjects();
    expect(loaded.map((p) => p.path)).toEqual([path.resolve(stays)]);

    const raw = JSON.parse(fs.readFileSync(path.join(userDataDir, 'projects.json'), 'utf8')) as {
      projects: Array<{ path: string }>;
    };
    expect(raw.projects.map((p) => p.path)).toEqual([path.resolve(stays)]);
  });
});
