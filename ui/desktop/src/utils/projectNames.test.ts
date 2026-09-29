import { describe, expect, it } from 'vitest';
import { distinctProjectNames, projectLabel, projectNameIn } from './projectNames';

const Q = '/Users/mihaiperdum/goose-builds/quality';
const labels = (paths: string[]) => {
  const names = distinctProjectNames(paths);
  return paths.map((p) => projectLabel(projectNameIn(names, p)));
};

describe('distinctProjectNames — Q-485: two folders named "work" are told apart', () => {
  it("the owner's two E2E folders read by the part of their parent that differs", () => {
    expect(
      labels([
        `${Q}/RU-2026-09-28-3v-split-tensor-cafe/work`,
        `${Q}/RU-2026-09-29-3w-split-tensor-cafe/work`,
        '/Users/mihaiperdum/Projects/goose',
      ])
    ).toEqual(['work — RU-…-28-3v-…', 'work — RU-…-29-3w-…', 'goose']);
  });

  it('a unique folder name is the name alone, and trailing slashes are one project', () => {
    const names = distinctProjectNames(['/a/goose/', '/a/goose', '/b/forge']);
    expect(projectNameIn(names, '/a/goose')).toEqual({ name: 'goose' });
    expect(projectNameIn(names, '/b/forge/')).toEqual({ name: 'forge' });
  });

  it('the nearest parent that is shared does not tell them apart; a higher one does', () => {
    expect(labels(['/x/app/work', '/y/app/work', '/x/lib/work'])).toEqual([
      'work — x/…',
      'work — y/…',
      'work — lib',
    ]);
  });

  it('a cut is made only at a word boundary, and a short parent stays whole', () => {
    expect(labels(['/p/projA/work', '/p/projB/work'])).toEqual(['work — projA', 'work — projB']);
    expect(labels(['/p/a/work', '/p/b/work'])).toEqual(['work — a', 'work — b']);
  });

  it('the path that runs out of parents first reads as the root', () => {
    expect(labels(['/work', '/home/work'])).toEqual(['work — /', 'work — home']);
  });

  it('a path outside the set is still named, by its folder', () => {
    expect(projectNameIn(distinctProjectNames([]), '/q/r/work')).toEqual({ name: 'work' });
  });
});
