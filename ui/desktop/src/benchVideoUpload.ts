import fs from 'node:fs/promises';
import crypto from 'node:crypto';
import type { BenchVideo } from './benchMedia';

export async function uploadBenchmarkVideo(
  video: BenchVideo,
  publishUrl: string,
  installId: string,
  scorerVersion: string
): Promise<unknown> {
  if (video.bytes > 4 * 1024 * 1024)
    throw new Error(
      'The graded browser clip exceeds the 4 MiB publication limit; preserve the full recording and generate a smaller evidence clip.'
    );
  const bytes = await fs.readFile(video.file);
  if (
    bytes.length !== video.bytes ||
    crypto.createHash('sha256').update(bytes).digest('hex') !== video.sha256
  )
    throw new Error('Video evidence changed since validation');
  const response = await fetch(new URL('/api/benchmark-media', publishUrl), {
    method: 'POST',
    headers: {
      'Content-Type': 'video/webm',
      'x-benchmark-install-id': installId,
      'x-benchmark-scorer': scorerVersion,
      'x-benchmark-caption': video.caption,
    },
    body: bytes,
  });
  const data = (await response.json()) as { receipt?: { sha256?: string }; error?: string };
  if (!response.ok) throw new Error(data.error ?? `Video upload failed (${response.status})`);
  if (data.receipt?.sha256 !== video.sha256)
    throw new Error('The uploaded video receipt does not match the graded recording');
  return data.receipt;
}
