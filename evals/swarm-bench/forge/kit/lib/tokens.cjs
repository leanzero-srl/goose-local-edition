'use strict';
// Design-token theming as the Atlassian host applies it on `view.theme.enable()`: the html attributes
// from @atlaskit/tokens getThemeHtmlAttrs (data-theme="dark:dark light:light spacing:spacing",
// data-color-mode) and the CSS of every theme they reference, from the pinned @atlaskit/tokens
// artifacts (dist/cjs/artifacts/themes/atlassian-<id>.js).
const path = require('path');

const cache = new Map();

function themeFor(paths, colorMode) {
  const key = `${paths.appModules}|${colorMode}`;
  if (cache.has(key)) return cache.get(key);
  const root = path.dirname(paths.resolve('@atlaskit/tokens/package.json'));
  const getAttrs = require(path.join(root, 'dist', 'cjs', 'get-theme-html-attrs.js')).default;
  const attrs = getAttrs({ colorMode, light: 'light', dark: 'dark' });
  const ids = attrs['data-theme'].split(' ').map((pair) => pair.split(':')[1]).filter(Boolean);
  const css = [...new Set(ids)].map((id) => require(path.join(root, 'dist', 'cjs', 'artifacts', 'themes', `atlassian-${id}.js`)).default).join('\n');
  const version = require(path.join(root, 'package.json')).version;
  const out = { attrs, css, themes: ids, tokensVersion: version };
  cache.set(key, out);
  return out;
}

// The resolved value of every --ds-* custom property for one colour mode (for graders comparing computed styles).
function tokenValues(paths, colorMode) {
  const { css } = themeFor(paths, colorMode);
  const values = {};
  const blockRe = /([^{}]+)\{([^}]*)\}/g;
  for (const [, selector, body] of css.matchAll(blockRe)) {
    const applies = selector.includes(colorMode === 'dark' ? 'dark:dark' : 'light:light');
    const generic = !selector.includes('light:') && !selector.includes('dark:');
    if (!applies && !generic) continue;
    for (const [, name, value] of body.matchAll(/(--ds-[a-z0-9-]+)\s*:\s*([^;]+);/gi)) values[name] = value.trim();
  }
  return values;
}

module.exports = { themeFor, tokenValues };
