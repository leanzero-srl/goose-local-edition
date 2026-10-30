/**
 * Attached audio is re-encoded to 16 kHz mono 16-bit PCM WAV before it is sent. Every local
 * audio model goose serves reads 16 kHz speech, any format Chromium decodes (mp3, m4a, ogg,
 * flac, wav) becomes one the engine reads, and goose can cut a PCM WAV into the 30-second
 * pieces Gemma 4 hears (a longer clip is heard only for its first 30 s, measured 2026-10-30).
 */
export const SPEECH_SAMPLE_RATE = 16_000;

/** A mono 16-bit PCM WAV file of `samples` (-1..1) at `sampleRate`. */
export function encodeWavPcm16(samples: Float32Array, sampleRate: number): ArrayBuffer {
  const dataBytes = samples.length * 2;
  const buffer = new ArrayBuffer(44 + dataBytes);
  const view = new DataView(buffer);
  const ascii = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i));
  };
  ascii(0, 'RIFF');
  view.setUint32(4, 36 + dataBytes, true);
  ascii(8, 'WAVE');
  ascii(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  ascii(36, 'data');
  view.setUint32(40, dataBytes, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return buffer;
}

/** The average of every channel: a mono mixdown. */
export function mixdown(channels: Float32Array[]): Float32Array {
  if (channels.length === 1) return channels[0];
  const out = new Float32Array(channels[0]?.length ?? 0);
  for (const channel of channels) {
    for (let i = 0; i < out.length; i++) out[i] += channel[i] / channels.length;
  }
  return out;
}

export function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return globalThis.btoa(binary);
}

/**
 * The attached audio as a 16 kHz mono PCM WAV data URL, or null when this renderer cannot
 * decode it (the caller then sends the file as it is).
 */
export async function speechWavDataUrl(file: ArrayBuffer): Promise<string | null> {
  const Offline = globalThis.OfflineAudioContext;
  if (!Offline) return null;
  try {
    // decodeAudioData resamples to the context's rate.
    const decoded = await new Offline(1, 1, SPEECH_SAMPLE_RATE).decodeAudioData(file.slice(0));
    const channels = Array.from({ length: decoded.numberOfChannels }, (_, i) =>
      decoded.getChannelData(i)
    );
    const wav = encodeWavPcm16(mixdown(channels), decoded.sampleRate);
    return `data:audio/wav;base64,${arrayBufferToBase64(wav)}`;
  } catch {
    return null;
  }
}
