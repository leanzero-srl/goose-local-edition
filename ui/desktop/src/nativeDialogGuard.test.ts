import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Q-472, owner rule: the app never shows a native OS dialog — no message box (it hung the app on a
 * recipe delete), no alert/confirm/prompt, no native <select>. Startup failures use
 * `showAppDialog` (appDialogWindow.ts); everything in a window uses the app's own dialogs and
 * dropdowns. File pickers (showOpenDialog / showSaveDialog) are the one OS surface kept: the app
 * has no file browser of its own.
 */
const BANNED: Array<[string, RegExp]> = [
  ['dialog.showMessageBox / showMessageBoxSync', /\bshowMessageBox(Sync)?\b/],
  ['dialog.showErrorBox', /\bshowErrorBox\b/],
  // A BARE global alert()/confirm()/prompt() is refused by eslint's scope-aware `no-alert`
  // (eslint.config.js), which a regex cannot tell from a local function named `confirm`.
  ['window.alert / confirm / prompt', /\b(window|globalThis)\.(alert|confirm|prompt)\s*\(/],
  ['a native <select>', /<select[\s>]/],
];

const SRC = __dirname;

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === 'node_modules' ? [] : sourceFiles(path);
    if (!/\.(ts|tsx)$/.test(name) || /\.d\.ts$/.test(name)) return [];
    if (/\.(test|spec)\.(ts|tsx)$/.test(name) || path.includes(`${join('src', 'test')}`)) return [];
    return [path];
  });
}

// Comments and plain string literals may NAME a banned call ("never window.confirm", an assert's
// label "a native <select>"); only code is checked.
function withoutComments(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/(^|[^:'"`])\/\/.*$/gm, '$1')
    .replace(/'(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*"/g, "''");
}

describe('no native dialog anywhere in the desktop app (Q-472)', () => {
  it('finds none in src/ and main.ts', () => {
    const files = sourceFiles(SRC);
    expect(files.some((file) => file.endsWith('main.ts'))).toBe(true);
    const hits: string[] = [];
    for (const file of files) {
      const lines = withoutComments(readFileSync(file, 'utf8')).split('\n');
      lines.forEach((line, index) => {
        for (const [what, pattern] of BANNED) {
          if (pattern.test(line)) {
            hits.push(`${relative(SRC, file)}:${index + 1} ${what}: ${line.trim()}`);
          }
        }
      });
    }
    expect(hits).toEqual([]);
  });

  it('the patterns catch each banned form (the guard is not blind)', () => {
    const samples = [
      'await dialog.showMessageBox({ type: "info" })',
      'dialog.showMessageBoxSync({ type: "error" })',
      'window.electron.showMessageBox(opts)',
      'dialog.showErrorBox("x", "y")',
      'if (!window.confirm(msg)) return;',
      'window.alert("x")',
      'const name = window.prompt("Name?")',
      'globalThis.confirm("sure?")',
      '<select value={v} onChange={f}>',
    ];
    for (const sample of samples) {
      expect(
        BANNED.some(([, pattern]) => pattern.test(withoutComments(sample))),
        sample
      ).toBe(true);
    }
    for (const allowed of [
      '// never window.confirm(...) here',
      'dialog.showOpenDialog({ properties: ["openFile"] })',
      'dialog.showSaveDialog(options)',
      'closeGuard.confirm(windowId)',
      'onConfirm={() => void confirm()}',
      '<SelectTrigger>',
      "expect(el.querySelector('select'), 'a native <select>').toBeNull();",
    ]) {
      expect(
        BANNED.some(([, pattern]) => pattern.test(withoutComments(allowed))),
        allowed
      ).toBe(false);
    }
  });
});
