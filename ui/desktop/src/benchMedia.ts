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
        caption: entry.caption,
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
