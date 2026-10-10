import fs from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import http from 'node:http';

export interface BenchVideo {
  file: string;
  caption: string;
  mimeType: 'video/webm';
  sha256: string;
  bytes: number;
}
export interface BenchMedia {
  videos: BenchVideo[];
  error?: string;
}

/** How every scorer's probe opens the caption of its full publication-encoded recording; leanzero.net reads
 *  the same opening to tell a full recording from an excerpt. */
const FULL_RECORDING = 'Full graded browser recording:';

/**
 * The caption of a Forge 2.0 full recording, owned here because the probe's own is wrong for this era
 * (forge2_probe.mjs assembleRecording, the string beginning "Full graded browser recording: dashboard widget
 * (no config, edit + Save, light and dark, 380 and 1180 px, a second instance), sprint action modal (sort,
 * router, select, comment post through the 429 retry, …"): Forge 2.0 arms no 429 on the comment path, and
 * the widget is graded in both themes at 380 px only — 1180 px is the second instance. This one names what
 * forge2_probe.mjs drives while it records (probeUi, exerciseModal, llmCases), in grading order, and that
 * the UI Kit admin panel is drawn outside the recording. It is what the card shows AND what a publish signs
 * into the site's receipt, so both surfaces print one caption.
 *
 * ASCII only and at most 400 characters: it travels as the `x-benchmark-caption` header, and leanzero.net
 * refuses a longer one (MAX_BENCHMARK_VIDEO_CAPTION_CHARS).
 */
export const FORGE2_RECORDING_CAPTION =
  `${FULL_RECORDING} dashboard widget (before configuration, edit and Save, light and dark at 380 px, ` +
  'a second instance at 1180 px), sprint action (light and dark, sort, issue link, select, comment post, ' +
  'double click, forbidden post, explain, close), the not-started sprint and the Forge LLM cases, where the ' +
  'app has each. The UI Kit admin panel is not in the recording.';

/** The caption a clip is shown and published under: the manifest's, except a Forge 2.0 full recording's
 *  (the manifest states its own scorer). Any other caption — another era's, an encoding-pending note — is
 *  the manifest's own words. */
function recordingCaption(manifestScorer: unknown, caption: string): string {
  return manifestScorer === 'forge-2.0' && caption.startsWith(FULL_RECORDING)
    ? FORGE2_RECORDING_CAPTION
    : caption;
}

export async function readBenchMedia(workdir: string): Promise<BenchMedia> {
  try {
    const root = await fs.realpath(workdir);
    const manifest = JSON.parse(
      await fs.readFile(path.join(root, 'bench-media', 'media-manifest.json'), 'utf8')
    );
    if (
      manifest.schemaVersion !== 1 ||
      manifest.recording !== 'graded-browser' ||
      !Array.isArray(manifest.videos)
    )
      throw new Error('Unsupported graded-browser media manifest');
    const videos: BenchVideo[] = [];
    for (const entry of manifest.videos) {
      if (
        typeof entry.file !== 'string' ||
        path.isAbsolute(entry.file) ||
        entry.mimeType !== 'video/webm' ||
        typeof entry.caption !== 'string'
      )
        throw new Error('Invalid video evidence entry');
      const file = await fs.realpath(path.join(root, entry.file));
      if (!file.startsWith(root + path.sep))
        throw new Error('Video evidence escapes its run directory');
      const bytes = await fs.readFile(file);
      if (bytes.length < 4 || bytes.readUInt32BE(0) !== 0x1a45dfa3)
        throw new Error('Video evidence is not WebM');
      const sha256 = crypto.createHash('sha256').update(bytes).digest('hex');
      if (sha256 !== entry.sha256 || bytes.length !== entry.bytes)
        throw new Error('Video evidence hash or size does not match its manifest');
      videos.push({
        file,
        caption: recordingCaption(manifest.scorerVersion, entry.caption),
        mimeType: 'video/webm',
        sha256,
        bytes: bytes.length,
      });
    }
    return { videos };
  } catch (error) {
    return { videos: [], error: error instanceof Error ? error.message : String(error) };
  }
}

/** Unpredictable URL tokens expose only validated run videos, with byte ranges for playback/seek. */
export class BenchMediaServer {
  private readonly files = new Map<string, BenchVideo>();
  private server?: http.Server;
  private starting?: Promise<number>;

  private port(): Promise<number> {
    if (this.starting) return this.starting;
    this.starting = new Promise((resolve, reject) => {
      this.server = http.createServer((request, response) => {
        const video = this.files.get(request.url?.slice(1) ?? '');
        if (!video || !['GET', 'HEAD'].includes(request.method ?? '')) {
          response.writeHead(404).end();
          return;
        }
        let start = 0;
        let end = video.bytes - 1;
        if (request.headers.range) {
          const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range);
          if (!match) {
            response.writeHead(416, { 'Content-Range': `bytes */${video.bytes}` }).end();
            return;
          }
          start = Number(match[1]);
          end = match[2] ? Math.min(Number(match[2]), end) : end;
          if (start > end || start >= video.bytes) {
            response.writeHead(416, { 'Content-Range': `bytes */${video.bytes}` }).end();
            return;
          }
        }
        response.writeHead(request.headers.range ? 206 : 200, {
          'Content-Type': video.mimeType,
          'Content-Length': end - start + 1,
          'Accept-Ranges': 'bytes',
          'Cache-Control': 'private, no-store',
          'X-Content-Type-Options': 'nosniff',
          ...(request.headers.range
            ? { 'Content-Range': `bytes ${start}-${end}/${video.bytes}` }
            : {}),
        });
        if (request.method === 'HEAD') {
          response.end();
          return;
        }
        const stream = createReadStream(video.file, { start, end });
        stream.on('error', () => response.destroy());
        response.on('close', () => stream.destroy());
        stream.pipe(response);
      });
      this.server.once('error', reject);
      this.server.listen(0, '127.0.0.1', () => {
        this.server!.unref();
        const address = this.server!.address();
        if (!address || typeof address === 'string')
          return reject(new Error('Media listener has no port'));
        resolve(address.port);
      });
    });
    return this.starting;
  }

  async expose(video: BenchVideo): Promise<string> {
    const port = await this.port();
    const token = crypto.randomUUID();
    this.files.set(token, video);
    return `http://127.0.0.1:${port}/${token}`;
  }

  close(): void {
    this.server?.close();
    this.files.clear();
    this.starting = undefined;
  }
}
