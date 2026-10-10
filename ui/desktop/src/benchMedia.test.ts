import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { afterEach, expect, it } from 'vitest';
import { BenchMediaServer, FORGE2_RECORDING_CAPTION, readBenchMedia } from './benchMedia';

const dirs: string[] = [];
const servers: BenchMediaServer[] = [];
const bytes = Buffer.concat([
  Buffer.from([0x1a, 0x45, 0xdf, 0xa3]),
  Buffer.from('webm-test-recording'),
]);
async function fixture(caption = 'Graded browser', scorerVersion?: string) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'benchmark-video-'));
  dirs.push(root);
  await fs.mkdir(path.join(root, 'bench-media'));
  const video = {
    file: 'bench-media/clip.webm',
    caption,
    mimeType: 'video/webm',
    sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    bytes: bytes.length,
  };
  await fs.writeFile(path.join(root, video.file), bytes);
  const manifest = {
    schemaVersion: 1,
    ...(scorerVersion ? { scorerVersion } : {}),
    recording: 'graded-browser',
    videos: [video],
  };
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

/** forge2_probe.mjs assembleRecording's caption for its full recording, verbatim (bench/forge2_probe.mjs). */
const FORGE2_PROBE_CAPTION =
  'Full graded browser recording: dashboard widget (no config, edit + Save, light and dark, 380 and 1180 px, a second ' +
  'instance), sprint action modal (sort, router, select, comment post through the 429 retry, double click, forbidden post, close), ' +
  'the not-started sprint and the Forge LLM cases';

it('shows and publishes a Forge 2.0 recording under the app’s own caption, which names only what the probe records', async () => {
  // The probe's string claims a comment "429 retry" Forge 2.0 never arms and reads as both themes at both
  // widths; the card and the publish both read the clip through readBenchMedia, so one caption serves both.
  const { root } = await fixture(FORGE2_PROBE_CAPTION, 'forge-2.0');
  const media = await readBenchMedia(root);
  expect(media.error).toBeUndefined();
  expect(media.videos[0].caption).toBe(FORGE2_RECORDING_CAPTION);
  expect(FORGE2_RECORDING_CAPTION).toBe(
    'Full graded browser recording: dashboard widget (before configuration, edit and Save, light and dark at 380 px, a second instance at 1180 px), sprint action (light and dark, sort, issue link, select, comment post, double click, forbidden post, explain, close), the not-started sprint and the Forge LLM cases, where the app has each. The UI Kit admin panel is not in the recording.'
  );
  expect(FORGE2_RECORDING_CAPTION).not.toMatch(/429|380 and 1180|modal|router|no config/);
  // It rides as an HTTP header to a route that refuses more than 400 characters, and leanzero.net tells a
  // full recording from an excerpt by how the caption opens.
  expect(FORGE2_RECORDING_CAPTION.length).toBeLessThanOrEqual(400);
  expect(FORGE2_RECORDING_CAPTION).toMatch(/^Full graded browser recording: [\x20-\x7e]+$/);
});

it('leaves every other caption as its manifest wrote it', async () => {
  // Another era's full recording, a Forge 2.0 clip whose publication encoding is still pending, and a
  // manifest that names no scorer: each keeps its own words.
  for (const [caption, scorer] of [
    [FORGE2_PROBE_CAPTION, 'forge-1.0'],
    [FORGE2_PROBE_CAPTION, undefined],
    ['Original graded browser recording; publication encoding pending', 'forge-2.0'],
    ['Full graded browser recording: payment field, structure inspection', 'sb-7.2'],
  ] as const) {
    const { root } = await fixture(caption, scorer);
    expect((await readBenchMedia(root)).videos[0].caption).toBe(caption);
  }
});
