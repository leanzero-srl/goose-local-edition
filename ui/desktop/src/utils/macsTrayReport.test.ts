import { describe, expect, it } from 'vitest';
import {
  MAC_TRAY_CHARS,
  clipTrayText,
  isMacsTrayReport,
  pickMacsTrayReport,
  type MacsTrayReport,
} from './macsTrayReport';

const report = (names: string[]): MacsTrayReport => ({
  lines: names.map((name) => ({ name, phase: 'idle', text: `${name} — Qwen3.8-27B` })),
  openLabel: 'Open My Macs',
});

describe('the Macs tray report', () => {
  it('keeps a menu item on one readable line', () => {
    expect(clipTrayText('Studio —\n  Qwen')).toBe('Studio — Qwen');
    const long = clipTrayText('x'.repeat(500));
    expect(long).toHaveLength(MAC_TRAY_CHARS);
    expect(long.endsWith('…')).toBe(true);
  });

  it('validates what crosses IPC: a known phase or null, strings where strings go', () => {
    expect(isMacsTrayReport(null)).toBe(true);
    expect(isMacsTrayReport(report(['a']))).toBe(true);
    expect(
      isMacsTrayReport({ openLabel: 'Open', lines: [{ name: 'a', phase: null, text: 'a — Off' }] })
    ).toBe(true);
    expect(
      isMacsTrayReport({ openLabel: 'Open', lines: [{ name: 'a', phase: 'purple', text: 'x' }] })
    ).toBe(false);
    expect(
      isMacsTrayReport({ openLabel: 'Open', lines: [{ name: 1, phase: null, text: 'x' }] })
    ).toBe(false);
    expect(isMacsTrayReport({ lines: [] })).toBe(false);
    expect(isMacsTrayReport('nope')).toBe(false);
  });

  it('shows the window that names the most Macs; one off the mesh never hides one on it', () => {
    const two = report(['MacBook', 'Studio']);
    expect(pickMacsTrayReport([null, report([]), two, report(['MacBook'])])).toBe(two);
    expect(pickMacsTrayReport([null, report([])])).toBeNull();
  });
});
