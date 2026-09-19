import fs from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pins = JSON.parse(fs.readFileSync(path.join(root, 'scripts/bundled-mcps.json'), 'utf8'));
const output = path.join(root, 'bundled-mcps');
fs.mkdirSync(output, { recursive: true });
for (const pin of pins) {
  const dest = path.join(output, pin.id);
  const patch = path.join(root, 'scripts/mcp-patches', `${pin.id}.patch`);
  const lock = path.join(root, 'scripts/mcp-locks', `${pin.id}.json`);
  const revision =
    pin.commit + (fs.existsSync(lock) ? ':' + createHash('sha256').update(fs.readFileSync(lock)).digest('hex') : '') +
    (fs.existsSync(patch)
      ? ':' + createHash('sha256').update(fs.readFileSync(patch)).digest('hex')
      : '');
  if (
    fs.existsSync(path.join(dest, '.revision')) &&
    fs.readFileSync(path.join(dest, '.revision'), 'utf8') === revision
  )
    continue;
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  const archive = path.join(output, `${pin.id}.tar.gz`);
  execFileSync(
    'curl',
    [
      '--fail',
      '--location',
      '--retry',
      '3',
      `https://api.github.com/repos/${pin.repo}/tarball/${pin.commit}`,
      '--output',
      archive,
    ],
    { stdio: 'inherit' }
  );
  execFileSync('tar', ['-xzf', archive, '--strip-components=1', '-C', dest]);
  fs.unlinkSync(archive);
  if (fs.existsSync(patch))
    execFileSync('patch', ['-p1', '-i', patch], { cwd: dest, stdio: 'inherit' });
  if (fs.existsSync(lock)) fs.copyFileSync(lock, path.join(dest, 'package-lock.json'));
  const env = {
    ...process.env,
    PUPPETEER_SKIP_DOWNLOAD: 'true',
    PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD: '1',
  };
  execFileSync('npm', ['ci', ...(pin.build ? [] : ['--omit=dev'])], {
    cwd: dest,
    env,
    stdio: 'inherit',
  });
  if (pin.build) {
    execFileSync('npm', ['run', 'build'], { cwd: dest, env, stdio: 'inherit' });
    execFileSync('npm', ['prune', '--omit=dev'], { cwd: dest, env, stdio: 'inherit' });
  }
  if (!fs.existsSync(path.join(dest, pin.entry))) throw new Error(`Missing MCP entry: ${pin.id}`);
  fs.writeFileSync(path.join(dest, '.revision'), revision);
}
fs.writeFileSync(path.join(output, 'manifest.json'), JSON.stringify(pins, null, 2));
const docRoot = path.join(output, 'leanzero-documents');
const browserRoot = path.join(output, 'browser');
execFileSync(
  process.execPath,
  [
    path.join(docRoot, 'node_modules/puppeteer/lib/puppeteer/node/cli.js'),
    'browsers',
    'install',
    'chrome-headless-shell',
    '--path',
    browserRoot,
  ],
  { cwd: docRoot, stdio: 'inherit' }
);
const { createRequire } = await import('node:module');
const docRequire = createRequire(path.join(docRoot, 'package.json'));
const { getInstalledBrowsers } = docRequire('@puppeteer/browsers');
const browsers = await getInstalledBrowsers({ cacheDir: browserRoot });
const shell = browsers.find((browser) => browser.browser === 'chrome-headless-shell');
if (!shell) throw new Error('Bundled PDF browser is missing');
fs.writeFileSync(
  path.join(output, 'browser.json'),
  JSON.stringify({ executable: path.relative(output, shell.executablePath) })
);
