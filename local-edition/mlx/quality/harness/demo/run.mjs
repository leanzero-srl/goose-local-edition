// node run.mjs '<js body using p, shot, txt>'
import { page, shot, txt, DIR } from './lib.mjs';
const { b, p } = await page();
const fn = new Function('p', 'shot', 'txt', 'DIR', `return (async () => { ${process.argv[2]} })()`);
try { const r = await fn(p, shot, txt, DIR); if (r !== undefined) console.log(typeof r === 'string' ? r : JSON.stringify(r, null, 1)); }
catch (e) { console.log('ERR', e.message.slice(0, 800)); }
process.exit(0);
