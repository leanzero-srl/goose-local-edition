// attach.mjs — a brief turn's `attach` list, attached the way a person's file pick lands (owner gap 2026-10-31:
// real users attach screenshots and voice notes; r1 only ever typed). Pure half: the plan (which files, where,
// what kind, which are missing) and the turns.tsv cell. DOM half: one file at a time on the composer's hidden
// <input type=file> (ChatInput.tsx: handleFileInputChange reads files[0] only, then clears the input), then a
// WAIT until the composer draws that file as attached — never a send without it.
//
// What "attached" looks like in ChatInput.tsx (the preview row under the form):
//   image  → a pastedImages entry: <img alt="Pasted image <id>" src="data:…"> once FileReader + compression end
//            (spinner while isLoading, a red error box when reading failed)
//   audio/video (types/message.ts mediaKindOf: audio/* or video/*) → a DroppedFile drawn by MediaAttachment:
//            [data-testid=media-attachment][data-kind=audio|video], the name in the label's title, and its
//            <audio>/<video src> only once readMediaFile has the data URL (spinner/error overlay otherwise)
//   anything else → no chip at all: the picked file's PATH is appended to the composer text
import { existsSync, statSync } from 'node:fs';
import { isAbsolute, resolve, basename, extname } from 'node:path';

const IMAGE = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp', '.heic', '.tif', '.tiff', '.svg']);
const AUDIO = new Set(['.wav', '.mp3', '.m4a', '.aac', '.ogg', '.oga', '.flac', '.opus', '.weba', '.aiff', '.aif']);
const VIDEO = new Set(['.mp4', '.mov', '.webm', '.mkv', '.avi', '.m4v']);
export const kindOf = (path) => { const e = extname(path).toLowerCase(); return IMAGE.has(e) ? 'image' : AUDIO.has(e) ? 'audio' : VIDEO.has(e) ? 'video' : 'file'; };

// One entry per listed file: {WORK} expanded, a relative path read against the work dir, absolute kept.
export function attachPlan(turn, work, { exists = (p) => existsSync(p) && statSync(p).isFile() } = {}) {
  const list = turn?.attach ?? [];
  if (!Array.isArray(list)) throw new Error(`brief turn "attach" must be a list of file paths, got ${JSON.stringify(list).slice(0, 80)}`);
  return list.map((given) => {
    if (typeof given !== 'string' || !given.trim()) throw new Error(`brief turn "attach" holds a non-path entry ${JSON.stringify(given)}`);
    const expanded = given.replaceAll('{WORK}', work);
    const path = isAbsolute(expanded) ? expanded : resolve(work, expanded);
    return { given, path, name: basename(path), kind: kindOf(path), exists: !!exists(path) };
  });
}

// The loud lines for files that are not there: the turn must not go out as if they were attached.
export const missingFails = (plan) => plan.filter((f) => !f.exists).map((f) => `ATTACH_FAIL missing file ${f.path} (brief: ${f.given})`);

// turns.tsv `attached`: '' for a turn with no attach list; else one item per file, name:kind:verdict.
export function attachedCell(results) {
  if (!results || !results.length) return '';
  return results.map((r) => `${r.name}:${r.kind}:${r.ok ? 'ok' : `FAIL(${(r.why ?? '').replace(/[\t\n]+/g, ' ').slice(0, 120)})`}`).join(', ');
}

// What the composer shows as attached right now (runs in the page). `root` = the composer around the visible
// chat input ([data-drop-zone] in ChatInput.tsx).
export const composerAttachments = (p) => p.evaluate(() => {
  const input = [...document.querySelectorAll('[data-testid=chat-input]')].find((e) => e.offsetParent);
  const root = input?.closest('[data-drop-zone]');
  if (!root) return { found: false, images: [], media: [], value: '' };
  const tile = (el) => el.closest('.relative') ?? el.parentElement;
  const spinning = (el) => !!tile(el)?.querySelector('.animate-spin');
  const err = (el) => tile(el)?.querySelector('[class*=bg-lz-err]')?.textContent.trim() ?? '';
  const images = [...root.querySelectorAll('img[alt^="Pasted image"]')].map((img) => ({ ready: (img.getAttribute('src') ?? '').startsWith('data:') && !spinning(img), error: err(img) }));
  // A loading image tile has no <img> yet, only its spinner: count it so a wait sees "still loading", not "absent".
  const loadingImages = [...root.querySelectorAll('.w-20.h-20 .animate-spin')].filter((s) => !s.closest('.w-20.h-20').querySelector('img')).length;
  for (let i = 0; i < loadingImages; i++) images.push({ ready: false, error: '' });
  const media = [...root.querySelectorAll('[data-testid=media-attachment]')].map((m) => ({ kind: m.dataset.kind, name: m.querySelector('[title]')?.getAttribute('title') ?? '', ready: !!m.querySelector('audio[src], video[src]') && !spinning(m), error: err(m) }));
  return { found: true, images, media, value: input.value ?? '' };
});

// Pure: has `item` (the file just picked) landed, given the composer before (`base`) and now (`cur`)?
// → { state: 'ok' | 'wait' | 'error', why }
export function chipLanded(item, base, cur) {
  if (!cur.found) return { state: 'wait', why: 'no composer on screen' };
  if (item.kind === 'image') {
    const fresh = cur.images.slice(base.images.length);
    if (!fresh.length) return { state: 'wait', why: 'no new image tile' };
    const e = fresh.find((i) => i.error); if (e) return { state: 'error', why: `image tile says "${e.error}"` };
    return fresh.every((i) => i.ready) ? { state: 'ok', why: 'image tile drawn with its data' } : { state: 'wait', why: 'image tile still loading' };
  }
  if (item.kind === 'audio' || item.kind === 'video') {
    const had = base.media.filter((m) => m.name === item.name && m.kind === item.kind).length;
    const mine = cur.media.filter((m) => m.name === item.name && m.kind === item.kind);
    if (mine.length <= had) return { state: 'wait', why: `no ${item.kind} chip named ${item.name}` };
    const m = mine.at(-1);
    if (m.error) return { state: 'error', why: `${item.kind} chip says "${m.error}"` };
    return m.ready ? { state: 'ok', why: `${item.kind} chip drawn with its data` } : { state: 'wait', why: `${item.kind} chip still loading` };
  }
  // A plain file: ChatInput appends its path to the composer text.
  return cur.value.includes(item.path) ? { state: 'ok', why: 'path in the composer' } : { state: 'wait', why: 'path not in the composer yet' };
}

export const CHIP_WAIT_MS = 30000; // ratio-free bound: a FileReader + image compression / a 16 kHz WAV re-encode of a voice note, never model work
const FILE_INPUT = 'input[type=file]';

// Attach every planned file through the composer's own file input, one change event each, waiting for its chip.
// Returns one result per file ({...item, ok, why}); a missing file is never handed to the input.
export async function attachFiles(p, plan, { waitMs = CHIP_WAIT_MS, pollMs = 250 } = {}) {
  const results = [];
  for (const item of plan) {
    if (!item.exists) { results.push({ ...item, ok: false, why: 'missing file' }); continue; }
    const base = await composerAttachments(p);
    const input = p.locator(`[data-drop-zone]:has([data-testid=chat-input]:visible) > ${FILE_INPUT}`).first();
    if (!(await input.count())) { results.push({ ...item, ok: false, why: 'no file input in the composer' }); continue; }
    try { await input.setInputFiles(item.path); } catch (e) { results.push({ ...item, ok: false, why: `setInputFiles: ${String(e.message ?? e).split('\n')[0]}` }); continue; }
    let v = { state: 'wait', why: 'not looked yet' };
    const until = Date.now() + waitMs;
    while (Date.now() < until) {
      v = chipLanded(item, base, await composerAttachments(p));
      if (v.state !== 'wait') break;
      await p.waitForTimeout(pollMs);
    }
    results.push({ ...item, ok: v.state === 'ok', why: v.state === 'wait' ? `no chip after ${waitMs / 1000}s: ${v.why}` : v.why });
  }
  return results;
}

// Does the composer still hold attachments (a lost send can leave them there)? Used before a resend.
export const composerHasAttachments = async (p) => { const c = await composerAttachments(p); return c.images.length + c.media.length > 0; };

// Pure: the stored user row's content (sessions.db content_json) — how many images and media went out with it.
export function storedAttachments(contentJson) {
  let c; try { c = JSON.parse(contentJson); } catch { return null; }
  if (!Array.isArray(c)) return null;
  return { images: c.filter((x) => x.type === 'image').length, media: c.filter((x) => x.type === 'media').length };
}
