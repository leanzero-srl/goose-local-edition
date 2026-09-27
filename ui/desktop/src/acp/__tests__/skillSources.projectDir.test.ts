import { beforeEach, describe, expect, it, vi } from 'vitest';

const goose = vi.hoisted(() => ({
  sourcesUpdate_unstable: vi.fn(async () => ({ source: { name: 'x' } })),
  sourcesDelete_unstable: vi.fn(async () => undefined),
}));
vi.mock('../acpConnection', () => ({ getAcpClient: async () => ({ goose }) }));

import { deleteSkillSource, updateSkillSource } from '../sources';

beforeEach(() => {
  goose.sourcesUpdate_unstable.mockClear();
  goose.sourcesDelete_unstable.mockClear();
});

// Q-221: the backend touches only a skill folder its listing offers for the request's projectDir, so
// the editor's save and delete must send the projectDir the skill was listed under.
describe('skill update/delete carry the projectDir they were listed under (Q-221)', () => {
  it('update sends it', async () => {
    await updateSkillSource({
      path: '/proj/.agents/skills/x',
      name: 'x',
      description: 'd',
      content: 'c',
      projectDir: '/proj',
    });
    expect(goose.sourcesUpdate_unstable).toHaveBeenCalledWith({
      type: 'skill',
      path: '/proj/.agents/skills/x',
      name: 'x',
      description: 'd',
      content: 'c',
      projectDir: '/proj',
    });
  });

  it('delete sends it', async () => {
    await deleteSkillSource('/proj/.agents/skills/x', '/proj');
    expect(goose.sourcesDelete_unstable).toHaveBeenCalledWith({
      type: 'skill',
      path: '/proj/.agents/skills/x',
      projectDir: '/proj',
    });
  });
});
