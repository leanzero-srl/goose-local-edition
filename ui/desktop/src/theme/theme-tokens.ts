/**
 * Theme tokens — the single source of truth for all MCP semantic token values.
 *
 * Every key in McpUiStyleVariableKey must be present in both lightTokens and
 * darkTokens. The TypeScript compiler enforces this: if the SDK adds a new key,
 * the build breaks until both maps are updated.
 *
 * Values are applied to :root via style.setProperty() before first paint
 * (see renderer.tsx). main.css only registers the variable names for Tailwind
 * class generation — it does NOT define values.
 *
 * These tokens serve two purposes:
 *  1. Goose desktop — applied to :root per resolved theme.
 *  2. MCP apps — encoded as light-dark() in hostContext.styles.variables.
 */
import type {
  McpUiHostStyles,
  McpUiStyleVariableKey,
  McpUiStyles,
} from '@modelcontextprotocol/ext-apps/app-bridge';

type ThemeTokens = Record<McpUiStyleVariableKey, string>;

// Subset of keys that are the same across both themes.
type BaseTokenKey = Extract<
  McpUiStyleVariableKey,
  `--font-${string}` | `--border-radius-${string}` | `--border-width-${string}`
>;

type ColorTokenKey = Exclude<McpUiStyleVariableKey, BaseTokenKey>;

// ---------------------------------------------------------------------------
// Base tokens — shared across light and dark themes
// ---------------------------------------------------------------------------
const baseTokens: Pick<ThemeTokens, BaseTokenKey> = {
  // Typography — families
  '--font-sans': "'Cash Sans', sans-serif",
  '--font-mono': 'monospace',

  // Typography — weights
  '--font-weight-normal': '400',
  '--font-weight-medium': '500',
  '--font-weight-semibold': '600',
  '--font-weight-bold': '700',

  // Typography — text sizes
  '--font-text-xs-size': '0.75rem',
  '--font-text-sm-size': '0.875rem',
  '--font-text-md-size': '1rem',
  '--font-text-lg-size': '1.125rem',

  // Typography — heading sizes
  '--font-heading-xs-size': '1rem',
  '--font-heading-sm-size': '1.125rem',
  '--font-heading-md-size': '1.25rem',
  '--font-heading-lg-size': '1.5rem',
  '--font-heading-xl-size': '1.875rem',
  '--font-heading-2xl-size': '2.25rem',
  '--font-heading-3xl-size': '3rem',

  // Typography — text line heights
  '--font-text-xs-line-height': '1rem',
  '--font-text-sm-line-height': '1.25rem',
  '--font-text-md-line-height': '1.5rem',
  '--font-text-lg-line-height': '1.75rem',

  // Typography — heading line heights
  '--font-heading-xs-line-height': '1.5rem',
  '--font-heading-sm-line-height': '1.75rem',
  '--font-heading-md-line-height': '1.75rem',
  '--font-heading-lg-line-height': '2rem',
  '--font-heading-xl-line-height': '2.25rem',
  '--font-heading-2xl-line-height': '2.5rem',
  '--font-heading-3xl-line-height': '3.5rem',

  // Border radius
  '--border-radius-xs': '2px',
  '--border-radius-sm': '4px',
  '--border-radius-md': '8px',
  '--border-radius-lg': '12px',
  '--border-radius-xl': '16px',
  '--border-radius-full': '9999px',

  // Border width
  '--border-width-regular': '1px',
};

// Theme-specific color/shadow tokens only.
type ColorTokens = Pick<ThemeTokens, ColorTokenKey>;

// ---------------------------------------------------------------------------
// Light theme — colors & shadows
// ---------------------------------------------------------------------------
const lightColorTokens: ColorTokens = {
  // Backgrounds
  '--color-background-primary': '#ffffff',
  '--color-background-secondary': '#f4f6f7',
  '--color-background-tertiary': '#e3e6ea',
  '--color-background-inverse': '#000000',
  '--color-background-ghost': 'transparent',
  '--color-background-info': '#5c98f9',
  '--color-background-danger': '#f94b4b',
  '--color-background-success': '#91cb80',
  '--color-background-warning': '#fbcd44',
  '--color-background-disabled': '#e3e6ea',

  // Text
  '--color-text-primary': '#3f434b',
  // Q-479: secondary and danger are WORDS on the three host surfaces (#ffffff / #f4f6f7 / #e3e6ea),
  // so each clears 4.5:1 on all three — secondary #666666 5.7 / 5.3 / 4.6 (was #878787 3.6 / 3.3),
  // danger #c81e1e 5.7 / 5.3 / 4.6 (was #f94b4b 3.4 / 3.2). Primary stays 9.9:1, so secondary
  // still reads as the quieter grey. Ghost is secondary's twin.
  '--color-text-secondary': '#666666',
  '--color-text-tertiary': '#a7b0b9',
  '--color-text-inverse': '#ffffff',
  '--color-text-ghost': '#666666',
  '--color-text-info': '#5c98f9',
  '--color-text-danger': '#c81e1e',
  '--color-text-success': '#91cb80',
  '--color-text-warning': '#fbcd44',
  '--color-text-disabled': '#cbd1d6',

  // Borders
  '--color-border-primary': '#e3e6ea',
  '--color-border-secondary': '#e3e6ea',
  '--color-border-tertiary': '#cbd1d6',
  '--color-border-inverse': '#000000',
  '--color-border-ghost': 'transparent',
  '--color-border-info': '#5c98f9',
  '--color-border-danger': '#f94b4b',
  '--color-border-success': '#91cb80',
  '--color-border-warning': '#fbcd44',
  '--color-border-disabled': '#e3e6ea',

  // Rings. The primary ring IS the keyboard focus ring (FOCUS, `ring-ring`, `outline-ring`), so it is
  // the solid LeanZero blue in every edition (Q-334): the old #e3e6ea drew a near-white hairline on
  // the white surface.
  '--color-ring-primary': '#1d4ed8',
  '--color-ring-secondary': '#cbd1d6',
  '--color-ring-inverse': '#ffffff',
  '--color-ring-info': '#5c98f9',
  '--color-ring-danger': '#f94b4b',
  '--color-ring-success': '#91cb80',
  '--color-ring-warning': '#fbcd44',

  // Shadows
  '--shadow-hairline': '0 0 0 1px rgba(0, 0, 0, 0.05)',
  '--shadow-sm': '0 1px 2px 0 rgba(0, 0, 0, 0.05)',
  '--shadow-md': '0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -2px rgba(0, 0, 0, 0.1)',
  '--shadow-lg': '0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -4px rgba(0, 0, 0, 0.1)',
};

// ---------------------------------------------------------------------------
// Dark theme — colors & shadows
// ---------------------------------------------------------------------------
const darkColorTokens: ColorTokens = {
  // Backgrounds
  '--color-background-primary': '#22252a',
  '--color-background-secondary': '#3f434b',
  '--color-background-tertiary': '#474e57',
  '--color-background-inverse': '#cbd1d6',
  '--color-background-ghost': 'transparent',
  '--color-background-info': '#7cacff',
  '--color-background-danger': '#ff6b6b',
  '--color-background-success': '#a3d795',
  '--color-background-warning': '#ffd966',
  '--color-background-disabled': '#474e57',

  // Text
  '--color-text-primary': '#ffffff',
  // Q-479: secondary is WORDS on all three dark host surfaces (#22252a / #3f434b / #474e57), not only the
  // card: #969696 cleared the card (5.2) but measured 3.36:1 on background-secondary, where the sidebar,
  // settings rows and hovered items sit. #c0c0c0 is 8.5 / 5.5 / 4.6 while white stays 15.4 / 9.9 / 8.4,
  // so it still reads as the quieter grey. Danger #ff9b9b: 7.6 on the card, 4.9 on background-secondary
  // (was #ff6b6b 3.6 there). Ghost is secondary's twin.
  '--color-text-secondary': '#c0c0c0',
  '--color-text-tertiary': '#7a8694',
  '--color-text-inverse': '#000000',
  '--color-text-ghost': '#c0c0c0',
  '--color-text-info': '#7cacff',
  '--color-text-danger': '#ff9b9b',
  '--color-text-success': '#a3d795',
  '--color-text-warning': '#ffd966',
  '--color-text-disabled': '#525b68',

  // Borders
  '--color-border-primary': '#3f434b',
  '--color-border-secondary': '#525b68',
  '--color-border-tertiary': '#474e57',
  '--color-border-inverse': '#ffffff',
  '--color-border-ghost': 'transparent',
  '--color-border-info': '#7cacff',
  '--color-border-danger': '#ff6b6b',
  '--color-border-success': '#a3d795',
  '--color-border-warning': '#ffd966',
  '--color-border-disabled': '#3f434b',

  // Rings — the focus ring in dark (Q-334; #525b68 was grey on grey). #60a5fa, not the local
  // edition's old #3b82f6: that measured 2.6:1 on background-secondary, under the 3:1 a focus
  // indicator needs; this clears 3:1 on every dark surface (theme/focusRing.test.ts).
  '--color-ring-primary': '#60a5fa',
  '--color-ring-secondary': '#474e57',
  '--color-ring-inverse': '#000000',
  '--color-ring-info': '#7cacff',
  '--color-ring-danger': '#ff6b6b',
  '--color-ring-success': '#a3d795',
  '--color-ring-warning': '#ffd966',

  // Shadows (darker for dark mode)
  '--shadow-hairline': '0 0 0 1px rgba(0, 0, 0, 0.2)',
  '--shadow-sm': '0 1px 2px 0 rgba(0, 0, 0, 0.2)',
  '--shadow-md': '0 4px 6px -1px rgba(0, 0, 0, 0.3), 0 2px 4px -2px rgba(0, 0, 0, 0.2)',
  '--shadow-lg': '0 10px 15px -3px rgba(0, 0, 0, 0.3), 0 4px 6px -4px rgba(0, 0, 0, 0.2)',
};

// ---------------------------------------------------------------------------
// Merged token maps — used by applyThemeTokens() and buildMcpHostStyles()
// ---------------------------------------------------------------------------
export const lightTokens: ThemeTokens = { ...baseTokens, ...lightColorTokens };
export const darkTokens: ThemeTokens = { ...baseTokens, ...darkColorTokens };

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// @font-face rules passed to MCP apps so sandboxed iframes can load host fonts.
const HOST_FONT_CSS = `
@font-face {
  font-family: 'Cash Sans';
  src: url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff2/CashSans-Light.woff2) format('woff2'),
       url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff/CashSans-Light.woff) format('woff');
  font-weight: 300;
  font-style: normal;
}
@font-face {
  font-family: 'Cash Sans';
  src: url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff2/CashSans-Regular.woff2) format('woff2'),
       url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff/CashSans-Regular.woff) format('woff');
  font-weight: 400;
  font-style: normal;
}
@font-face {
  font-family: 'Cash Sans';
  src: url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff2/CashSans-Medium.woff2) format('woff2'),
       url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff/CashSans-Medium.woff) format('woff');
  font-weight: 500;
  font-style: normal;
}
@font-face {
  font-family: 'Cash Sans';
  src: url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff2/CashSans-Bold.woff2) format('woff2'),
       url(https://cash-f.squarecdn.com/static/fonts/cashsans/woff/CashSans-Bold.woff) format('woff');
  font-weight: 700;
  font-style: normal;
}
`.trim();

/**
 * Build the McpUiHostStyles object for MCP apps.
 * Color keys use light-dark() so a single payload works for both themes.
 * Non-color keys (fonts, radii, shadows) use plain values from baseTokens
 * (or light as the default when values differ, e.g. shadows).
 * css.fonts provides @font-face rules so sandboxed apps can load host fonts.
 */
export function buildMcpHostStyles(): McpUiHostStyles {
  const variables: McpUiStyles = {} as McpUiStyles;
  for (const key of Object.keys(lightTokens) as McpUiStyleVariableKey[]) {
    const light = lightTokens[key];
    const dark = darkTokens[key];
    if (key.startsWith('--color-')) {
      variables[key] = `light-dark(${light}, ${dark})`;
    } else {
      variables[key] = light;
    }
  }
  return { variables, css: { fonts: HOST_FONT_CSS } };
}

/**
 * Resolve the current theme from localStorage / system preference.
 */
export function getResolvedTheme(): 'light' | 'dark' {
  const useSystem = localStorage.getItem('use_system_theme') !== 'false';
  if (useSystem) {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  return localStorage.getItem('theme') === 'dark' ? 'dark' : 'light';
}

/**
 * Apply theme tokens to the document root as CSS custom properties.
 * When called without an argument, resolves the theme from localStorage.
 */
export function applyThemeTokens(theme?: 'light' | 'dark'): void {
  const resolved = theme ?? getResolvedTheme();
  const tokens = resolved === 'dark' ? darkTokens : lightTokens;
  const root = document.documentElement;
  for (const [key, value] of Object.entries(tokens)) {
    root.style.setProperty(key, value);
  }
}
