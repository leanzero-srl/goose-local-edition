#!/usr/bin/env node
/**
 * Render every shipped brand asset FROM the geometry the UI actually draws
 * (src/components/icons/leanzeroMark.tsx), so the app icon can never drift from the mark in the
 * sidebar. Run it after any change to that file:
 *
 *   node scripts/build-brand-icons.mjs
 *
 * Produces, in src/images/: icon.icns (full retina iconset), icon.png, icon@2x.png, icon-512.png,
 * icon.svg (Linux scalable), icon-light.{icns,png}, and the menu-bar templates
 * iconTemplate{,@2x}.png plus iconTemplateUpdate{,@2x}.png.
 *
 * macOS only, and it needs Google Chrome to rasterise (sips/iconutil ship with the OS). It has no
 * fallback on purpose: if an input is missing it stops and says which one, rather than quietly
 * emitting a stale or half-built icon set.
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const MARK_TS = path.join(HERE, '..', 'src', 'components', 'icons', 'leanzeroMark.tsx');
const IMAGES = path.join(HERE, '..', 'src', 'images');
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const PLATE = '#1d4ed8'; // --color-action-solid, the one LeanZero accent

const die = (msg) => {
  console.error(`build-brand-icons: ${msg}`);
  process.exit(1);
};

/**
 * Pull the geometry out of the TypeScript module. Every extraction is asserted: a rename in
 * leanzeroMark.tsx must FAIL here loudly, never silently render the wrong mark.
 */
function readMark() {
  const src = readFileSync(MARK_TS, 'utf8');
  const grab = (re, what) => {
    const m = src.match(re);
    if (!m) die(`could not read ${what} from ${path.relative(process.cwd(), MARK_TS)} — the module changed shape; fix this script rather than shipping a stale icon`);
    return m;
  };
  const goose = grab(/export const GOOSE_PATH =\s*'([^']+)'/, 'GOOSE_PATH')[1];
  const lPath = grab(/export const LEANZERO_MARK_L = '([^']+)'/, 'LEANZERO_MARK_L')[1];
  const centre = grab(/GOOSE_CENTRE = \{ x: ([\d.]+), y: ([\d.]+) \}/, 'GOOSE_CENTRE');
  const geeseBlock = grab(/export const LEANZERO_MARK_GEESE = \[([\s\S]*?)\];/, 'LEANZERO_MARK_GEESE')[1];
  const geese = [...geeseBlock.matchAll(/\{ x: (-?[\d.]+), y: (-?[\d.]+), scale: (-?[\d.]+), rotate: (-?[\d.]+) \}/g)].map(
    (m) => ({ x: +m[1], y: +m[2], scale: +m[3], rotate: +m[4] })
  );
  if (geese.length !== 2) die(`expected 2 geese in LEANZERO_MARK_GEESE, found ${geese.length}`);
  return { goose, lPath, cx: +centre[1], cy: +centre[2], geese };
}

const M = readMark();
const at = (g) => `translate(${g.x} ${g.y}) rotate(${g.rotate}) scale(${g.scale}) translate(${-M.cx} ${-M.cy})`;

/** The mark's inner SVG — the string form of LeanZeroMarkContent. The geese simply overlap and
 *  merge; the back one's leading wing passes under the front one and is not drawn. */
const markInner = () =>
  `<path d="${M.lPath}"/>` + M.geese.map((g) => `<path d="${M.goose}" transform="${at(g)}"/>`).join('');

/** The app icon: a solid plate on Apple's grid (824 of 1024) carrying the mark in white. */
const appIconSvg = (px) => `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="0 0 1024 1024">
  <rect x="100" y="100" width="824" height="824" rx="185" ry="185" fill="${PLATE}"/>
  <g transform="translate(512 512) scale(7.1) translate(-32 -32)" fill="#ffffff">${markInner()}</g>
</svg>`;

const traySvg = (extra = '', viewBox = '0 0 64 64', w = 512, h = 512) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="${viewBox}" fill="#000000">${markInner()}${extra}</svg>`;

const WORK = mkdtempSync(path.join(tmpdir(), 'lz-icons-'));
const shot = (svg, out, w, h) => {
  const html = path.join(WORK, `${path.basename(out, '.png')}.html`);
  writeFileSync(html, `<!doctype html><meta charset="utf-8"><style>html,body{margin:0;background:transparent}</style>${svg}`);
  execFileSync(CHROME, ['--headless', '--disable-gpu', '--default-background-color=00000000',
    `--screenshot=${out}`, `--window-size=${w},${h}`, `file://${html}`], { stdio: 'ignore' });
};
const sips = (src, w, h, out) => execFileSync('/usr/bin/sips', ['-z', String(h), String(w), src, '--out', out], { stdio: 'ignore' });

try {
  execFileSync('/bin/test', ['-x', CHROME]);
} catch {
  die(`Google Chrome not found at ${CHROME} — it is the rasteriser; install it or render the SVGs another way`);
}

const master2048 = path.join(WORK, 'app-2048.png');
const master1024 = path.join(WORK, 'app-1024.png');
shot(appIconSvg(2048), master2048, 2048, 2048);
shot(appIconSvg(1024), master1024, 1024, 1024);

const iconset = path.join(WORK, 'Goose.iconset');
mkdirSync(iconset);
for (const [name, px] of [['16x16', 16], ['32x32', 32], ['128x128', 128], ['256x256', 256], ['512x512', 512],
  ['16x16@2x', 32], ['32x32@2x', 64], ['128x128@2x', 256], ['256x256@2x', 512], ['512x512@2x', 1024]])
  sips(master2048, px, px, path.join(iconset, `icon_${name}.png`));
const icns = path.join(WORK, 'icon.icns');
execFileSync('/usr/bin/iconutil', ['-c', 'icns', iconset, '-o', icns], { stdio: 'inherit' });

const tray512 = path.join(WORK, 'tray.png');
const trayUpd512 = path.join(WORK, 'tray-update.png');
shot(traySvg(), tray512, 512, 512);
shot(traySvg('<circle cx="64" cy="8" r="7"/>', '0 0 72 64', 576, 512), trayUpd512, 576, 512);

const out = (f) => path.join(IMAGES, f);
execFileSync('/bin/cp', [icns, out('icon.icns')]);
execFileSync('/bin/cp', [icns, out('icon-light.icns')]);
execFileSync('/bin/cp', [master1024, out('icon.png')]);
execFileSync('/bin/cp', [master1024, out('icon-light.png')]);
execFileSync('/bin/cp', [master2048, out('icon@2x.png')]);
sips(master2048, 512, 512, out('icon-512.png'));
sips(tray512, 22, 22, out('iconTemplate.png'));
sips(tray512, 44, 44, out('iconTemplate@2x.png'));
sips(trayUpd512, 22, 25, out('iconTemplateUpdate.png'));
sips(trayUpd512, 44, 50, out('iconTemplateUpdate@2x.png'));
writeFileSync(out('icon.svg'), `<?xml version="1.0" encoding="UTF-8" standalone="no"?>\n${appIconSvg(2048)}\n`);

rmSync(WORK, { recursive: true, force: true });
console.log('build-brand-icons: wrote icon.icns, icon{,-light}.png, icon@2x.png, icon-512.png, icon.svg and the 4 menu-bar templates');
