import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { PEER_FOCUS } from './tokens';

/**
 * Q-477 — EVERY keyboard stop shows the Q-334 ring. The 3.0.76 walk found two holes the token
 * guard (theme/focusRing.test.ts, which only checks the token's colour) could not see:
 *  1. raw <button>s that name no focus style (session-card actions, Conversation Limits, the
 *     dictation trigger, the sidebar section headers) fell back to Chromium's 1px 'auto' ring in
 *     the system accent — not the token;
 *  2. the settings radios are `peer sr-only` inputs: Tab lands on an invisible element, so nothing
 *     on screen changed at all.
 * Refused here: a stylesheet without the base-layer default ring; a `peer sr-only` input whose
 * visible sibling does not carry PEER_FOCUS; and any `outline-none` / `outline-hidden` whose class
 * expression puts no other focus indicator in its place — unless it is listed below with why.
 */

const SRC = resolve(__dirname, '../..');

/**
 * `outline-none` without a replacement ring, allowed ONLY for these, with the reason. A listed
 * file that no longer carries the bare `outline-none` fails as stale.
 */
const NO_RING_BY_DESIGN: Record<string, string> = {
  'components/ChatInput.tsx':
    'the composer textarea: the page’s primary field, focused whenever the chat is; its caret is the indicator',
  'components/LauncherView.tsx':
    'the launcher window IS one text field (autoFocus, nothing else to Tab to); its caret is the indicator',
  'components/Layout/NavigationPanel.tsx':
    'the nav container is a programmatic focus target (tabIndex -1), never a Tab stop',
  'components/session-rail/SessionRail.tsx':
    'the rail panel is a programmatic focus target (tabIndex -1), never a Tab stop',
  'components/conversation/SearchBar.tsx':
    'the find bar is one text field on the inverse surface, focused whenever the bar is open; its caret is the indicator',
  'components/ui/OverlayDialog.tsx':
    'Radix dialog content is a programmatic focus target (tabIndex -1); the controls inside carry the ring',
  'components/benchmark/BenchmarkView.tsx':
    'the run-setup section is a programmatic scroll/focus target (tabIndex -1), never a Tab stop',
};

const NO_OUTLINE = /(^|[\s'"`])(?:focus:|focus-visible:)?outline-(?:none|hidden)(?=[\s'"`]|$)/;
/** Every NO_OUTLINE match contains this, so a file without it has nothing to report. */
const OUTLINE_WORD = /outline-(?:none|hidden)/;
const INDICATOR =
  /focus-visible:(?:outline-(?:solid|2)|ring-(?!0\b))|focus:ring-(?!0\b)|focus:bg-|focus-within:|data-\[highlighted\]:bg-|\bFOCUS\b|PEER_FOCUS/;

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && entry.name !== '__tests__') sourceFiles(path, out);
    } else if (/\.tsx?$/.test(entry.name) && !/\.(test|spec)\.tsx?$/.test(entry.name)) {
      out.push(path);
    }
  }
  return out;
}

/** The whole class expression a literal belongs to: its JSX attribute, const, or cva entry. */
function owner(node: ts.Node): ts.Node {
  let n: ts.Node = node;
  while (n.parent) {
    if (
      ts.isJsxAttribute(n.parent) ||
      ts.isVariableDeclaration(n.parent) ||
      ts.isPropertyAssignment(n.parent) ||
      ts.isCallExpression(n.parent)
    ) {
      // cx(...)/cn(...) join their arguments into one class list: widen to the call.
      if (ts.isCallExpression(n.parent) && ts.isIdentifier(n.parent.expression)) {
        if (['cx', 'cn', 'clsx', 'cva'].includes(n.parent.expression.text)) {
          n = n.parent;
          continue;
        }
      }
      return n.parent;
    }
    n = n.parent;
  }
  return n;
}

function bareOutlines(file: string, text: string): string[] {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const out: string[] = [];
  const visit = (node: ts.Node) => {
    if (
      (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
      NO_OUTLINE.test(node.text)
    ) {
      const expr = owner(node).getText(source);
      if (!INDICATOR.test(expr)) {
        const line = source.getLineAndCharacterOfPosition(node.getStart()).line + 1;
        out.push(`${file}:${line}`);
      }
    }
    if (ts.isTemplateExpression(node)) {
      const raw = [node.head.text, ...node.templateSpans.map((s) => s.literal.text)].join(' ');
      if (NO_OUTLINE.test(raw) && !INDICATOR.test(owner(node).getText(source))) {
        const line = source.getLineAndCharacterOfPosition(node.getStart()).line + 1;
        out.push(`${file}:${line}`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

describe('every keyboard stop shows the focus ring (Q-477)', () => {
  it('the stylesheet makes the token ring the DEFAULT for every focusable, in the base layer', () => {
    const css = readFileSync(resolve(__dirname, '../../styles/main.css'), 'utf8');
    const base = /@layer base\s*\{([\s\S]*?)\n\}/.exec(css)?.[1] ?? '';
    const rule = /:where\(([^)]*(?:\([^)]*\)[^)]*)*)\):focus-visible\s*\{([^}]*)\}/.exec(base);
    expect(rule, 'no :where(...):focus-visible default ring in @layer base').not.toBeNull();
    const [, targets, body] = rule!;
    for (const t of ['a[href]', 'button', 'input', 'select', 'textarea', 'summary', '[tabindex]']) {
      expect(targets, t).toContain(t);
    }
    expect(body).toMatch(/outline:\s*2px\s+solid\s+var\(--color-ring-primary\)/);
  });

  it('a visually hidden peer input hands its ring to the visible sibling (PEER_FOCUS)', () => {
    expect(PEER_FOCUS).toContain('peer-focus-visible:outline-ring');
    const missing: string[] = [];
    for (const file of sourceFiles(SRC)) {
      const text = readFileSync(file, 'utf8');
      for (const m of text.matchAll(/(["'`\s])peer sr-only|sr-only peer(["'`\s])/g)) {
        const after = text.slice(m.index!, m.index! + 600);
        if (!/PEER_FOCUS|peer-focus-visible:outline/.test(after)) {
          const line = text.slice(0, m.index!).split('\n').length;
          missing.push(`${relative(SRC, file)}:${line}`);
        }
      }
    }
    expect(missing).toEqual([]);
  });

  it('no outline-none without a replacement indicator, outside the listed exceptions', () => {
    // Q-466: only a file whose text names outline-none/-hidden can hold a hit, so only those are
    // parsed — the TypeScript parse of every source file was 0.8 s alone and 2.8 s under load.
    const found = sourceFiles(SRC).flatMap((file) => {
      const text = readFileSync(file, 'utf8');
      return OUTLINE_WORD.test(text) ? bareOutlines(relative(SRC, file), text) : [];
    });
    const unlisted = found.filter((hit) => !(hit.split(':')[0] in NO_RING_BY_DESIGN));
    expect(unlisted).toEqual([]);
    const stale = Object.keys(NO_RING_BY_DESIGN).filter(
      (file) => !found.some((hit) => hit.startsWith(`${file}:`))
    );
    expect(stale, 'listed but no longer bare — delete the entry').toEqual([]);
  });

  it('the scan refuses the shipped shapes (fixture)', () => {
    expect(
      bareOutlines('f.tsx', `<button className="text-[11px] underline outline-none">x</button>`)
    ).toEqual(['f.tsx:1']);
    expect(
      bareOutlines('f.tsx', `const C = 'rounded border focus-visible:outline-none px-3';`)
    ).toEqual(['f.tsx:1']);
    expect(
      bareOutlines(
        'f.tsx',
        `const C = cx('flex outline-none', 'data-[highlighted]:bg-lz-surface-2');`
      )
    ).toEqual([]);
    expect(
      bareOutlines('f.tsx', `<a className="outline-none focus-visible:outline-solid">x</a>`)
    ).toEqual([]);
  });
});
