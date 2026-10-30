import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { buildCSP } from './csp';

const mediaSrc = (csp: string) =>
  csp
    .split(';')
    .map((d) => d.trim())
    .find((d) => d.startsWith('media-src'))
    ?.split(/\s+/)
    .slice(1) ?? [];

// The composer and the sent message play attached audio/video from data URLs (MediaAttachment).
// Without data: in media-src Chromium refuses the load ("Media load rejected by URL safety check"),
// and both policies apply at once: the response header (buildCSP) and index.html's meta tag.
describe('media-src lets attached audio and video play', () => {
  it('the header policy allows data: and blob: media', () => {
    expect(mediaSrc(buildCSP())).toEqual(expect.arrayContaining(['data:', 'blob:']));
  });

  it("index.html's meta policy allows the same media sources", () => {
    const html = readFileSync(join(__dirname, '..', '..', 'index.html'), 'utf8');
    const meta = /http-equiv="Content-Security-Policy"\s+content="([^"]+)"/.exec(html)?.[1] ?? '';
    expect(mediaSrc(meta)).toEqual(expect.arrayContaining(['data:', 'blob:']));
    expect(new Set(mediaSrc(meta))).toEqual(new Set(mediaSrc(buildCSP())));
  });
});
