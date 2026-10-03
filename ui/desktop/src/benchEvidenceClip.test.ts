import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { afterEach, describe, expect, it } from 'vitest';
import {
  PUBLISH_VIDEO_LIMIT,
  evidenceCaption,
  nextEncodeStep,
  publishableClip,
  type ClipTools,
} from './benchEvidenceClip';
import { readBenchMedia } from './benchMedia';

const TARGET = Math.floor(PUBLISH_VIDEO_LIMIT * 0.9);
// GLM 5.3 FlashX (sb-7.2, 0.699) — the real graded recording's identity.
const GLM_SHA = '67cc29cf15b4e0a753bf17ec0106f8d8b4d038726e183e6a394d365c25861bb4';

describe('the encode ladder, on the measured GLM 5.3 FlashX numbers', () => {
  it('steps the width by the measured overshoot', () => {
    // The scorer's encode at 1280 wide measured 4,559,944 B; two-pass 960 wide measured 3,681,201 B.
    expect(nextEncodeStep({ width: 1280, fps: 25 }, 4_559_944, TARGET)).toEqual({ width: 952, fps: 25 });
    // A near miss steps a little; the floor width is tried before the frame rate drops.
    expect(nextEncodeStep({ width: 960, fps: 25 }, 3_900_000, TARGET)?.width).toBe(836);
    expect(nextEncodeStep({ width: 500, fps: 25 }, 6_000_000, TARGET)).toEqual({ width: 480, fps: 25 });
    expect(nextEncodeStep({ width: 480, fps: 25 }, 6_000_000, TARGET)).toEqual({ width: 480, fps: 14 });
    // Nothing left: the caller reports the sizes, never posts an oversized clip.
    expect(nextEncodeStep({ width: 480, fps: 5 }, 6_000_000, TARGET)).toBeNull();
    expect(nextEncodeStep({ width: 480, fps: null }, 6_000_000, TARGET)).toBeNull();
  });

  it('names the graded original in a caption the site accepts', () => {
    const caption = evidenceCaption({ width: 952, fps: 25 }, 594, 111.72, GLM_SHA);
    expect(caption).toBe(
      `Graded browser recording re-encoded to 952x594 at 25 fps to fit 4 MiB, same 111.7 s; graded original sha256 ${GLM_SHA}`
    );
    expect(caption.length).toBeLessThanOrEqual(200);
  });
});

const dirs: string[] = [];
afterEach(async () => {
  await Promise.all(dirs.splice(0).map((d) => fs.rm(d, { recursive: true, force: true })));
});
const sha = (b: Buffer) => crypto.createHash('sha256').update(b).digest('hex');
const webm = (size: number) =>
  Buffer.concat([Buffer.from([0x1a, 0x45, 0xdf, 0xa3]), Buffer.from('webm'), Buffer.alloc(size - 8, 7)]);

/** Fake ffmpeg/ffprobe: the encode writes bytes proportional to the requested width (≈ the measured
 *  4.56 MB at 1280), and ffprobe answers the GLM recording's duration and geometry. */
async function fakeTools(dir: string, log: string): Promise<ClipTools> {
  const ffmpeg = path.join(dir, 'ffmpeg');
  const ffprobe = path.join(dir, 'ffprobe');
  await fs.writeFile(
    ffmpeg,
    `#!/bin/sh
echo "$@" >> "${log}"
vf=""; prev=""; out=""
for a in "$@"; do [ "$prev" = "-vf" ] && vf="$a"; prev="$a"; out="$a"; done
case "$*" in *"-pass 1"*) exit 0;; esac
w=$(echo "$vf" | sed -E 's/scale=([0-9]+).*/\\1/')
size=$(( w * 3562 ))
printf '\\032\\105\\337\\243webm' > "$out"
head -c $((size - 8)) /dev/zero >> "$out"
`
  );
  await fs.writeFile(
    ffprobe,
    `#!/bin/sh
echo '{"format":{"duration":"111.720000"},"streams":[{"width":1280,"height":800,"avg_frame_rate":"25/1"}]}'
`
  );
  await fs.chmod(ffmpeg, 0o755);
  await fs.chmod(ffprobe, 0o755);
  return { ffmpeg, ffprobe };
}

async function oversizedRun() {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'bench-evidence-test-')));
  dirs.push(root);
  await fs.mkdir(path.join(root, 'bench-media', 'raw'), { recursive: true });
  const raw = webm(PUBLISH_VIDEO_LIMIT + 4096);
  const entry = {
    file: 'bench-media/raw/page.webm',
    caption: 'Original graded browser recording; compressed copy unavailable',
    mimeType: 'video/webm',
    sha256: sha(raw),
    bytes: raw.length,
    sourceFile: 'bench-media/raw/page.webm',
  };
  await fs.writeFile(path.join(root, entry.file), raw);
  await fs.writeFile(
    path.join(root, 'bench-media', 'media-manifest.json'),
    JSON.stringify({ schemaVersion: 1, recording: 'graded-browser', videos: [entry], errors: [] })
  );
  return { root, entry };
}

describe('a graded clip over the publication limit', () => {
  it('keeps the recording, makes a smaller clip of it once, records both sha256s, and reuses it', async () => {
    const { root, entry } = await oversizedRun();
    const log = path.join(root, 'ffmpeg.log');
    const tools = await fakeTools(root, log);
    const media = await readBenchMedia(root);
    expect(media.error).toBeUndefined();

    const first = await publishableClip(root, media.videos[0], async () => tools);
    // 1280 wide → 4,559,360 B (over); the measured step lands at 952 wide.
    const calls = (await fs.readFile(log, 'utf8')).trim().split('\n');
    expect(calls.filter((c) => c.includes('-pass 2')).map((c) => /scale=(\d+)/.exec(c)?.[1])).toEqual([
      '1280',
      '952',
    ]);
    expect(first.video.bytes).toBeLessThanOrEqual(TARGET);
    expect(first.video.bytes).toBe(952 * 3562);
    expect(first.video.caption).toContain(`graded original sha256 ${entry.sha256}`);

    // The graded recording is untouched and still verifies; the manifest names both clips.
    const after = await readBenchMedia(root);
    expect(after.videos[0].sha256).toBe(entry.sha256);
    const manifest = JSON.parse(
      await fs.readFile(path.join(root, 'bench-media', 'media-manifest.json'), 'utf8')
    );
    const record = manifest.videos[0].evidenceClip;
    expect(record).toMatchObject({
      sha256: first.video.sha256,
      bytes: first.video.bytes,
      sourceSha256: entry.sha256,
      sourceBytes: entry.bytes,
      sourceFile: entry.file,
      durationSeconds: 111.72,
      fps: 25,
    });
    expect(sha(await fs.readFile(path.join(root, record.file)))).toBe(record.sha256);
    // Only the landed rung is kept.
    expect(await fs.readdir(path.join(root, 'bench-media', 'evidence'))).toEqual([path.basename(record.file)]);

    // A second publish reuses it — no encode.
    await fs.writeFile(log, '');
    const again = await publishableClip(root, media.videos[0], async () => tools);
    expect(again.video.sha256).toBe(first.video.sha256);
    expect(await fs.readFile(log, 'utf8')).toBe('');

    // Tampered evidence is never reused: it is made again from the graded recording.
    await fs.appendFile(path.join(root, record.file), 'x');
    const remade = await publishableClip(root, media.videos[0], async () => tools);
    expect(remade.video.sha256).toBe(first.video.sha256);
    expect(await fs.readFile(log, 'utf8')).toContain('-pass 2');
  });

  it('leaves a clip that fits alone — no tools resolved, no manifest write', async () => {
    const { root } = await oversizedRun();
    const small = webm(1024);
    const video = {
      file: path.join(root, 'small.webm'),
      caption: 'c',
      mimeType: 'video/webm' as const,
      sha256: sha(small),
      bytes: small.length,
    };
    const result = await publishableClip(root, video, async () => {
      throw new Error('tools must not be resolved');
    });
    expect(result).toEqual({ video, evidence: null });
  });
});

// The REAL GLM 5.3 FlashX tree, on a machine that has it (BENCH_REAL_GLM_TREE=<a COPY of the run dir>):
// the runtime's own ffmpeg, the real 9,969,210-byte graded recording.
const realTree = process.env.BENCH_REAL_GLM_TREE;
const realRuntime = process.env.BENCH_REAL_RUNTIME;
describe.skipIf(!realTree || !realRuntime)('the real GLM 5.3 FlashX recording', () => {
  it('publishes as an evidence clip under the limit with the same duration', async () => {
    const media = await readBenchMedia(realTree!);
    expect(media.videos[0].sha256).toBe(GLM_SHA);
    const tools = {
      ffmpeg: path.join(realRuntime!, 'ffmpeg', 'ffmpeg'),
      ffprobe: path.join(realRuntime!, 'ffprobe', 'ffprobe'),
    };
    const result = await publishableClip(realTree!, media.videos[0], async () => tools);
    console.log(JSON.stringify(result.evidence, null, 1));
    expect(result.video.bytes).toBeLessThanOrEqual(TARGET);
    expect(Math.abs(result.evidence!.encodedDurationSeconds - 111.72)).toBeLessThanOrEqual(0.25);
  }, 900_000);
});
