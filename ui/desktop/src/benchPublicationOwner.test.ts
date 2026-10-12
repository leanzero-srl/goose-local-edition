import { describe, expect, it, vi } from 'vitest';
import { BenchPublicationOwner } from './benchPublicationOwner';

describe('main publication ownership across competing renderer requests', () => {
  it('reserves before the first delayed receipt read, makes one POST and admits the next independent build', async () => {
    const owner = new BenchPublicationOwner();
    let finishRead!: () => void;
    const read = new Promise<void>((resolve) => {
      finishRead = resolve;
    });
    const post = vi.fn(async () => ({ ok: true }));
    const first = owner.run(async () => {
      await read;
      return post();
    });
    const duplicate = await owner.run(async () => {
      await read;
      return post();
    });
    expect(duplicate).toEqual({
      ok: false,
      error: 'A benchmark publication is already in progress.',
    });
    expect(post).not.toHaveBeenCalled();
    finishRead();
    expect(await first).toEqual({ ok: true });
    expect(post).toHaveBeenCalledTimes(1);
    expect(await owner.run(post)).toEqual({ ok: true });
    expect(post).toHaveBeenCalledTimes(2);
  });
  it('releases ownership after an explicit failure and preserves its error for a deliberate retry', async () => {
    const owner = new BenchPublicationOwner();
    await expect(
      owner.run(async () => {
        throw new Error('receipt write failed');
      })
    ).rejects.toThrow('receipt write failed');
    expect(await owner.run(async () => ({ ok: true }))).toEqual({ ok: true });
  });
});
