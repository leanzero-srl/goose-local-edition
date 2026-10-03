import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import type { BenchVideo } from './benchMedia';

/**
 * A graded clip larger than the site's upload limit publishes as a smaller EVIDENCE clip of the same
 * recording: same duration, same frames in the same order, fewer pixels. The graded recording stays in
 * the tree untouched; the manifest entry gains `evidenceClip` naming both sha256s, and the caption the
 * site signs into its receipt names the graded original's sha256 — the public binding from the posted
 * clip back to what was graded (the site's receipt binds only the bytes it received).
 *
 * MEASURED 2026-10-03 on GLM 5.3 FlashX (sb-7.2, 0.699; raw VP8 1280x800 25 fps, 111.72 s, 9,969,210 B):
 * the scorer's own VP9 encode at its computed 223 kbps came out 4,559,944 B — over the 4,194,304 limit —
 * because VP9 at 1280 wide sits on its quality floor: 200 kbps two-pass still gave 4,310,283 B, CBR gave
 * 5.9 MB, one-pass 9.3 MB. Width is the lever that works (two-pass 960 wide: 3,681,201 B), so the ladder
 * MEASURES each encode and scales the width by the measured overshoot.
 */

/** The site's MAX_BENCHMARK_VIDEO_BYTES (website src/lib/benchmark-media.ts). */
export const PUBLISH_VIDEO_LIMIT = 4 * 1024 * 1024;
// ratio: a clip is accepted at <= 90% of the limit, so a re-mux or a header never tips it over.
const TARGET_FRACTION = 0.9;
// ratio: the bitrate aims at 80% — VP9 two-pass overshoots its bitrate ~5% once the width is below its
// quality floor (960 wide at 250 kb/s measured 3,681,201 B where 250 kb/s x 111.7 s is 3.49 MB). Aiming at
// the 90% line itself made the 918-wide rung miss by a hair and cost a third encode (GLM: 1280, 918, 688).
const BITRATE_FRACTION = 0.8;
// ratio: each step aims a further 10% under the measured requirement, so one step usually lands.
const STEP_MARGIN = 0.9;
const MIN_WIDTH = 480;
const MIN_FPS = 5;
const MAX_CAPTION = 200;

export interface EvidenceClipRecord {
  file: string;
  sha256: string;
  bytes: number;
  sourceFile: string;
  sourceSha256: string;
  sourceBytes: number;
  durationSeconds: number;
  encodedDurationSeconds: number;
  width: number;
  fps: number | null;
  encoder: string;
  caption: string;
  reason: string;
}

export interface EncodeStep {
  width: number;
  /** null — the source reports no frame rate, so frames are kept as recorded (no resampling). */
  fps: number | null;
}

/**
 * The next rung after an encode measured `bytes` against `target`: the width scales by the measured
 * ratio (VP9's size tracks width roughly linearly on this content); below MIN_WIDTH the frame rate drops
 * instead. Null when no rung is left — the caller reports the measured sizes, it never posts an
 * oversized clip.
 */
export function nextEncodeStep(step: EncodeStep, bytes: number, target: number): EncodeStep | null {
  const ratio = (target / bytes) * STEP_MARGIN;
  const width = Math.floor((step.width * ratio) / 2) * 2;
  if (width >= MIN_WIDTH) return { width, fps: step.fps };
  if (step.width > MIN_WIDTH) return { width: MIN_WIDTH, fps: step.fps };
  if (step.fps == null) return null;
  const fps = Math.floor(step.fps * ratio);
  return fps >= MIN_FPS && fps < step.fps ? { width: step.width, fps } : null;
}

export function evidenceCaption(step: EncodeStep, height: number, duration: number, sourceSha: string) {
  const caption =
    `Graded browser recording re-encoded to ${step.width}x${height}${step.fps != null ? ` at ${step.fps} fps` : ''} to fit 4 MiB, ` +
    `same ${duration.toFixed(1)} s; graded original sha256 ${sourceSha}`;
  if (caption.length > MAX_CAPTION) throw new Error('Evidence clip caption exceeds 200 characters');
  return caption;
}

export interface ClipTools {
  ffmpeg: string;
  ffprobe: string;
}

const execute = promisify(execFile);

async function probe(ffprobe: string, file: string) {
  const { stdout } = await execute(
    ffprobe,
    [
      '-v',
      'error',
      '-select_streams',
      'v:0',
      '-show_entries',
      'format=duration:stream=width,height,avg_frame_rate',
      '-of',
      'json',
      file,
    ],
    { timeout: 30000 }
  );
  const data = JSON.parse(stdout) as {
    format?: { duration?: string };
    streams?: Array<{ width?: number; height?: number; avg_frame_rate?: string }>;
  };
  const duration = Number(data.format?.duration);
  const stream = data.streams?.[0];
  const [num, den] = String(stream?.avg_frame_rate ?? '').split('/').map(Number);
  const fps = den ? num / den : num;
  if (!Number.isFinite(duration) || duration <= 0) throw new Error('Recording duration unavailable');
  if (!stream?.width || !stream?.height) throw new Error('Recording dimensions unavailable');
  return {
    duration,
    width: stream.width,
    height: stream.height,
    fps: Number.isFinite(fps) && fps > 0 ? fps : null,
  };
}

async function encodeTwoPass(
  tools: ClipTools,
  source: string,
  output: string,
  step: EncodeStep,
  bitrate: number,
  duration: number
) {
  const work = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-evidence-'));
  // ratio: the scorer's own encode allowance (media_sb71.mjs) — five seconds of wall per second of video.
  const timeout = Math.max(60000, Math.ceil(duration * 5000));
  const shared = [
    '-hide_banner',
    '-loglevel',
    'error',
    '-i',
    source,
    '-an',
    '-vf',
    `scale=${step.width}:-2${step.fps != null ? `,fps=${step.fps}` : ''}`,
    '-c:v',
    'libvpx-vp9',
    '-b:v',
    String(bitrate),
    '-row-mt',
    '1',
    '-cpu-used',
    '5',
    '-deadline',
    'good',
    '-passlogfile',
    path.join(work, 'pass'),
    '-y',
  ];
  try {
    await execute(tools.ffmpeg, [...shared, '-pass', '1', '-f', 'null', '/dev/null'], { timeout });
    await execute(tools.ffmpeg, [...shared, '-pass', '2', output], { timeout });
  } finally {
    await fs.rm(work, { recursive: true, force: true });
  }
}

const sha256Of = (bytes: Buffer) => crypto.createHash('sha256').update(bytes).digest('hex');

/** A recorded evidence clip still on disk, still the bytes the manifest names, still of THIS source. */
async function reusable(root: string, record: unknown, video: BenchVideo): Promise<EvidenceClipRecord | null> {
  const r = record as Partial<EvidenceClipRecord> | null;
  if (!r || typeof r.file !== 'string' || r.sourceSha256 !== video.sha256) return null;
  try {
    const file = await fs.realpath(path.join(root, r.file));
    if (!file.startsWith(root + path.sep)) return null;
    const bytes = await fs.readFile(file);
    return bytes.length === r.bytes && bytes.length <= PUBLISH_VIDEO_LIMIT && sha256Of(bytes) === r.sha256
      ? (r as EvidenceClipRecord)
      : null;
  } catch {
    return null;
  }
}

/**
 * The clip to upload for a verified graded clip: the graded clip itself when it fits, else the evidence
 * clip (made once, recorded in the manifest, reused while its bytes and its source still verify).
 */
export async function publishableClip(
  workdir: string,
  video: BenchVideo,
  tools: () => Promise<ClipTools>
): Promise<{ video: BenchVideo; evidence: EvidenceClipRecord | null }> {
  if (video.bytes <= PUBLISH_VIDEO_LIMIT) return { video, evidence: null };
  const root = await fs.realpath(workdir);
  const manifestPath = path.join(root, 'bench-media', 'media-manifest.json');
  const manifest = JSON.parse(await fs.readFile(manifestPath, 'utf8')) as {
    videos: Array<Record<string, unknown>>;
  };
  const entry = manifest.videos.find((v) => v.sha256 === video.sha256);
  if (!entry) throw new Error('The graded clip is not in its media manifest');
  const asVideo = (record: EvidenceClipRecord): BenchVideo => ({
    file: path.join(root, record.file),
    caption: record.caption,
    mimeType: 'video/webm',
    sha256: record.sha256,
    bytes: record.bytes,
  });
  const existing = await reusable(root, entry.evidenceClip, video);
  if (existing) return { video: asVideo(existing), evidence: existing };

  const { ffmpeg, ffprobe } = await tools();
  const source = await probe(ffprobe, video.file);
  const target = Math.floor(PUBLISH_VIDEO_LIMIT * TARGET_FRACTION);
  const bitrate = Math.floor((PUBLISH_VIDEO_LIMIT * BITRATE_FRACTION * 8) / (source.duration + 1));
  const outDir = path.join(root, 'bench-media', 'evidence');
  await fs.mkdir(outDir, { recursive: true });
  const measured: string[] = [];
  let step: EncodeStep | null = {
    width: Math.min(source.width, 1280) - (Math.min(source.width, 1280) % 2),
    fps: source.fps != null ? Math.round(source.fps) : null,
  };
  while (step) {
    const output = path.join(
      outDir,
      `graded-${video.sha256.slice(0, 16)}-${step.width}w-${step.fps ?? 'src'}fps.webm`
    );
    await encodeTwoPass({ ffmpeg, ffprobe }, video.file, output, step, bitrate, source.duration);
    const bytes = await fs.readFile(output);
    measured.push(`${step.width}w ${step.fps ?? 'source '}fps ${bytes.length} B`);
    if (bytes.length <= target) {
      const encoded = await probe(ffprobe, output);
      // The scorer's own tolerance (media_sb71.mjs): an encode that lost source time is not the recording.
      if (Math.abs(encoded.duration - source.duration) > 0.25)
        throw new Error(
          `The evidence clip lost source time (${encoded.duration.toFixed(2)} s of ${source.duration.toFixed(2)} s)`
        );
      const record: EvidenceClipRecord = {
        file: path.relative(root, output),
        sha256: sha256Of(bytes),
        bytes: bytes.length,
        sourceFile: path.relative(root, video.file),
        sourceSha256: video.sha256,
        sourceBytes: video.bytes,
        durationSeconds: source.duration,
        encodedDurationSeconds: encoded.duration,
        width: encoded.width,
        fps: step.fps,
        encoder: `libvpx-vp9 two-pass ${bitrate} b/s`,
        caption: evidenceCaption(step, encoded.height, source.duration, video.sha256),
        reason: `The graded clip is ${video.bytes} B, over the ${PUBLISH_VIDEO_LIMIT} B publication limit; the full recording is kept and this smaller clip of the same recording is published`,
      };
      entry.evidenceClip = record;
      await fs.writeFile(`${manifestPath}.pending`, `${JSON.stringify(manifest, null, 2)}\n`);
      await fs.rename(`${manifestPath}.pending`, manifestPath);
      for (const stale of await fs.readdir(outDir))
        if (path.join(outDir, stale) !== output) await fs.rm(path.join(outDir, stale), { force: true });
      return { video: asVideo(record), evidence: record };
    }
    step = nextEncodeStep(step, bytes.length, target);
  }
  throw new Error(
    `No evidence clip of the graded recording fits the 4 MiB publication limit (measured: ${measured.join('; ')})`
  );
}
