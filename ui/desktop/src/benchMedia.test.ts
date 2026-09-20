import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { afterEach, expect, it } from 'vitest';
import { BenchMediaServer, readBenchMedia } from './benchMedia';

const dirs: string[] = [];
const servers: BenchMediaServer[] = [];
const bytes = Buffer.concat([
  Buffer.from([0x1a, 0x45, 0xdf, 0xa3]),
  Buffer.from('webm-test-recording'),
]);
async function fixture() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'benchmark-video-'));
  dirs.push(root);
  await fs.mkdir(path.join(root, 'bench-media'));
  const video = {
    file: 'bench-media/clip.webm',
    caption: 'Graded browser',
    mimeType: 'video/webm',
    sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    bytes: bytes.length,
  };
  await fs.writeFile(path.join(root, video.file), bytes);
  const manifest = { schemaVersion: 1, recording: 'graded-browser', videos: [video] };
  await fs.writeFile(path.join(root, 'bench-media/media-manifest.json'), JSON.stringify(manifest));
  return { root, manifest };
}
afterEach(async () => {
  servers.splice(0).forEach((s) => s.close());
  await Promise.all(dirs.splice(0).map((d) => fs.rm(d, { recursive: true, force: true })));
});

it('streams only validated evidence and supports actual byte-range seek', async () => {
  const { root } = await fixture();
  const media = await readBenchMedia(root);
  expect(media.error).toBeUndefined();
  const server = new BenchMediaServer();
  servers.push(server);
  const url = await server.expose(media.videos[0]);
  const response = await fetch(url, { headers: { Range: 'bytes=4-7' } });
  expect(response.status).toBe(206);
  expect(await response.text()).toBe('webm');
  expect(response.headers.get('content-range')).toBe(`bytes 4-7/${bytes.length}`);
  expect((await fetch(url.replace(/[^/]+$/, 'unknown'))).status).toBe(404);
  expect((await fetch(url, { headers: { Range: 'bytes=999-1000' } })).status).toBe(416);
});

it('rejects changed bytes and traversal rather than displaying another file', async () => {
  const { root, manifest } = await fixture();
  await fs.appendFile(path.join(root, manifest.videos[0].file), 'tampered');
  expect((await readBenchMedia(root)).error).toMatch(/hash or size/);
  manifest.videos[0].file = '../outside.webm';
  await fs.writeFile(path.join(root, 'bench-media/media-manifest.json'), JSON.stringify(manifest));
  expect((await readBenchMedia(root)).videos).toHaveLength(0);
});
