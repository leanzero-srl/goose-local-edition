import { describe, expect, it } from 'vitest';
import { arrayBufferToBase64, encodeWavPcm16, mixdown, speechWavDataUrl } from './speechWav';

describe('speech WAV re-encoding', () => {
  it('writes a 16-bit mono PCM WAV header the engine and the splitter read', () => {
    const wav = encodeWavPcm16(new Float32Array([0, 1, -1, 0.5]), 16_000);
    const view = new DataView(wav);
    const ascii = (o: number) => String.fromCharCode(...new Uint8Array(wav, o, 4));
    expect([ascii(0), ascii(8), ascii(12), ascii(36)]).toEqual(['RIFF', 'WAVE', 'fmt ', 'data']);
    expect(view.getUint16(20, true)).toBe(1);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint32(28, true)).toBe(32_000);
    expect(view.getUint32(40, true)).toBe(8);
    expect([1, 2, 3].map((i) => view.getInt16(44 + i * 2, true))).toEqual([32767, -32768, 16383]);
  });

  it('mixes channels down to their average', () => {
    expect(Array.from(mixdown([new Float32Array([1, 0]), new Float32Array([0, 0])]))).toEqual([
      0.5, 0,
    ]);
  });

  it('base64-encodes large buffers in slices', () => {
    const big = new Uint8Array(100_000).fill(65);
    expect(arrayBufferToBase64(big.buffer).slice(0, 8)).toBe('QUFBQUFB');
  });

  it('gives null where the renderer cannot decode audio, so the file goes as it is', async () => {
    expect(await speechWavDataUrl(new ArrayBuffer(8))).toBeNull();
  });
});
