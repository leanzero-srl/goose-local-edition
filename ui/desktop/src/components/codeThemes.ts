import type { CSSProperties } from 'react';

/**
 * The syntax-highlighter themes for fenced code blocks — one per app theme.
 *
 * WHY THIS FILE EXISTS: the chat rendered every block with Prism's oneDark and passed its own
 * `codeTagProps`, which REPLACES (not merges) the theme's `code[class*="language-"]` style — so the
 * <code> element carried no colour, and every untokenised run (identifiers, `self`, parameters,
 * argument lists) inherited Tailwind Typography's `--tw-prose-code`: near-black in light theme, on a
 * #282c34 panel. The owner's screenshot (2026-09-25) was that block, italic from the Thinking
 * wrapper, with typography's `code::before/::after { content: "`" }` painting a stray backtick at
 * each end.
 *
 * The contract, pinned by codeThemes.test.tsx: every colour here meets WCAG AA (4.5:1) against its
 * block background, nothing is italic, and the <code> element always carries the base colour so no
 * surrounding text colour can reach an untokenised run. Hues follow One Light / One Dark, darkened
 * or lifted until they pass.
 */
export type CodeThemeName = 'light' | 'dark';

interface CodePalette {
  background: string;
  border: string;
  base: string;
  comment: string;
  punctuation: string;
  keyword: string;
  function: string;
  string: string;
  number: string;
  property: string;
  className: string;
  variable: string;
  operator: string;
  regex: string;
  url: string;
}

const PALETTES: Record<CodeThemeName, CodePalette> = {
  light: {
    background: '#f6f8fa',
    border: '#d0d7de',
    base: '#24292f',
    comment: '#57606a',
    punctuation: '#3d4450',
    keyword: '#a626a4',
    function: '#2350c4',
    string: '#2f6f1f',
    number: '#9a4d00',
    property: '#b42318',
    className: '#8a5300',
    variable: '#0b5f8a',
    operator: '#0b5f8a',
    regex: '#16704f',
    url: '#0a61a8',
  },
  dark: {
    background: '#282c34',
    border: '#3e4451',
    base: '#dcdfe4',
    comment: '#9ba3af',
    punctuation: '#c3c8d1',
    keyword: '#d38ae8',
    function: '#6cb6f5',
    string: '#a3d17c',
    number: '#e5a66d',
    property: '#f2828b',
    className: '#e8c07d',
    variable: '#e5818a',
    operator: '#62c3d0',
    regex: '#56c2b6',
    url: '#62c3d0',
  },
};

/** Prism token classes by palette role. A class not listed inherits the base colour of <code>. */
const TOKEN_ROLES: Record<
  Exclude<keyof CodePalette, 'background' | 'border' | 'base'>,
  string[]
> = {
  comment: ['comment', 'prolog', 'doctype', 'cdata', 'shebang'],
  punctuation: ['punctuation', 'entity', 'template-punctuation', 'interpolation-punctuation'],
  keyword: ['keyword', 'atrule', 'important', 'rule', 'directive', 'module', 'control-flow'],
  function: ['function', 'function-variable', 'method', 'macro'],
  string: [
    'string',
    'char',
    'attr-value',
    'template-string',
    'inserted',
    'selector',
    'builtin',
    'code-snippet',
  ],
  number: ['number', 'boolean', 'constant', 'null', 'nil', 'unit', 'hexcode', 'color'],
  property: ['property', 'tag', 'symbol', 'deleted', 'key', 'attr-name', 'title'],
  className: [
    'class-name',
    'maybe-class-name',
    'known-class-name',
    'namespace',
    'decorator',
    'annotation',
  ],
  variable: ['variable', 'parameter', 'property-access'],
  operator: ['operator', 'arrow'],
  regex: ['regex', 'regex-source', 'regex-flags'],
  url: ['url', 'url-reference'],
};

const CODE_FONT_SIZE = '14px';

function buildTheme(p: CodePalette): Record<string, CSSProperties> {
  const code: CSSProperties = {
    color: p.base,
    background: 'none',
    fontStyle: 'normal',
    fontWeight: 400,
    fontFamily: 'var(--font-mono)',
    fontSize: CODE_FONT_SIZE,
    textShadow: 'none',
    tabSize: 4,
  };
  const theme: Record<string, CSSProperties> = {
    'code[class*="language-"]': code,
    'pre[class*="language-"]': {
      ...code,
      background: p.background,
      border: `1px solid ${p.border}`,
      borderRadius: '6px',
      padding: '12px 14px',
      margin: 0,
      overflow: 'auto',
      lineHeight: 1.55,
    },
  };
  for (const [role, classes] of Object.entries(TOKEN_ROLES)) {
    const color = p[role as keyof CodePalette];
    for (const cls of classes) theme[cls] = { color, fontStyle: 'normal' };
  }
  theme.bold = { fontWeight: 700, fontStyle: 'normal' };
  theme.italic = { fontStyle: 'normal' };
  return theme;
}

export const CODE_THEMES: Record<CodeThemeName, Record<string, CSSProperties>> = {
  light: buildTheme(PALETTES.light),
  dark: buildTheme(PALETTES.dark),
};

/** The theme's block background — the colour every token is measured against. */
export function codeBackground(name: CodeThemeName): string {
  return PALETTES[name].background;
}
