/**
 * A solid anti-aliased dot as a raw BGRA bitmap (Electron `nativeImage.createFromBitmap`), for the
 * menu-bar tray's state lines: the engine-phase colour (lz tokens PHASE_HEX) drawn in its exact hex,
 * since a native menu item carries no CSS. Pure — main turns the buffer into an image.
 */
export function phaseDotBitmap(hex: string, size: number): Buffer {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const buf = Buffer.alloc(size * size * 4);
  const c = size / 2;
  const radius = size / 2 - 0.5;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const d = Math.hypot(x + 0.5 - c, y + 0.5 - c);
      const coverage = Math.max(0, Math.min(1, radius - d + 0.5));
      const i = (y * size + x) * 4;
      // premultiplied BGRA, the layout createFromBitmap expects
      buf[i] = Math.round(b * coverage);
      buf[i + 1] = Math.round(g * coverage);
      buf[i + 2] = Math.round(r * coverage);
      buf[i + 3] = Math.round(255 * coverage);
    }
  }
  return buf;
}
