import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, statSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname, resolve, relative } from 'node:path';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

export function videoDuration(file) {
  const data = JSON.parse(execFileSync(process.env.BENCH_FFPROBE || 'ffprobe',
    ['-v', 'error', '-show_entries', 'format=duration', '-of', 'json', file],
    { encoding: 'utf8', timeout: 10000 }));
  const duration = Number(data.format?.duration);
  if (!Number.isFinite(duration) || duration <= 0) throw new Error('Recording duration unavailable');
  return duration;
}

export function encodeFullRecording(raw, output) {
  const duration = videoDuration(raw);
  const directory = mkdtempSync(join(tmpdir(), 'sb71-video-'));
  // Leave container overhead below the existing 4 MiB upload limit.
  const bitrate = Math.floor(3 * 1024 * 1024 * 8 / (duration + 1));
  const shared = ['-hide_banner', '-loglevel', 'error', '-i', raw, '-an',
    '-vf', 'scale=1280:-2', '-c:v', 'libvpx-vp9', '-b:v', String(bitrate),
    '-threads', '1', '-passlogfile', join(directory, 'encode'), '-y'];
  const options = { timeout: Math.max(60000, Math.ceil(duration * 5000)) };
  try {
    execFileSync(process.env.BENCH_FFMPEG || 'ffmpeg', [...shared, '-pass', '1', '-f', 'null', '/dev/null'], options);
    execFileSync(process.env.BENCH_FFMPEG || 'ffmpeg', [...shared, '-pass', '2', output], options);
    const encodedDuration = videoDuration(output);
    if (Math.abs(encodedDuration - duration) > .25) throw new Error('Encoded recording lost source time');
    if (statSync(output).size > 4 * 1024 * 1024) throw new Error('Full recording exceeds the 4 MiB publication limit');
    return { startSeconds: 0, endSeconds: duration, durationSeconds: duration,
      encodedDurationSeconds: encodedDuration, reason: 'Complete graded browser recording, in original order and at original speed' };
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

export function encodeManifest(manifestPath) {
  const media = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const root = dirname(dirname(manifestPath));
  for (const video of media.videos) {
    const raw = resolve(root, video.sourceFile);
    const output = join(dirname(manifestPath), 'payment-towers.webm');
    try {
      const sourceInterval = encodeFullRecording(raw, output);
      const bytes = readFileSync(output);
      Object.assign(video, { file: relative(root, output),
        caption: 'Full graded browser recording: payment field, structure inspection, committed updates and replay',
        sha256: createHash('sha256').update(bytes).digest('hex'), bytes: bytes.length,
        selection: 'Complete recording in original order and at original speed',
        publishable: true, sourceInterval });
    } catch (error) {
      media.errors.push('Full recording encoding failed; original retained: ' + String(error.message).slice(0, 180));
      video.caption = 'Original graded browser recording; compressed copy unavailable';
    }
  }
  writeFileSync(manifestPath, JSON.stringify(media, null, 2) + '\n');
  return media;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3) throw new Error('Usage: media_sb71.mjs <media-manifest.json>');
  encodeManifest(resolve(process.argv[2]));
}
