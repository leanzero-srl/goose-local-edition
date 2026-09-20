import { afterEach, expect, it, vi } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { uploadBenchmarkVideo } from './benchVideoUpload';
const dirs: string[] = [];
afterEach(async () => {
  vi.unstubAllGlobals();
  await Promise.all(dirs.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true })));
});
it('uploads exact verified bytes and rejects mutation before making a request', async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-upload-'));
  dirs.push(dir);
  const file = path.join(dir, 'clip.webm');
  const bytes = Buffer.from('verified clip');
  await fs.writeFile(file, bytes);
  const video = {
    file,
    bytes: bytes.length,
    sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    caption: 'Graded browser',
    mimeType: 'video/webm' as const,
  };
  const fetchMock = vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify({ receipt: { sha256: video.sha256 } })));
  vi.stubGlobal('fetch', fetchMock);
  await uploadBenchmarkVideo(
    video,
    'https://example.com/api/benchmark-runs',
    'install',
    'sb-7.1-rc'
  );
  expect(fetchMock.mock.calls[0][0].href).toBe('https://example.com/api/benchmark-media');
  expect(fetchMock.mock.calls[0][1].body).toEqual(bytes);
  await fs.writeFile(file, 'changed');
  await expect(
    uploadBenchmarkVideo(video, 'https://example.com', 'install', 'sb-7.1-rc')
  ).rejects.toThrow('changed since validation');
  expect(fetchMock).toHaveBeenCalledTimes(1);
});
