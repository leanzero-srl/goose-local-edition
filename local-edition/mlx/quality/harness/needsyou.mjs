// needsyou.mjs — Q-376: the E2E answers goose's needs-you cards the way a person would, so the ANSWERED path
// (Q-298 supersede, Q-340 fold, Q-341 mid-turn queue, Q-344 card-answer mark) is exercised inside every E2E.
// Owner, 2026-09-28: "you should also reply to any of those need you to ensure that the session works well
// when these are responded to!"
//
// Pure half (unit-tested in needsyou.test.mjs): which guidance entry a question matches, what the persona
// answers, and which control on the card carries that answer. DOM half: read the tray of the chat on screen,
// answer one card through its own controls (option / recommended / text box + Answer — the only other click is
// opening a folded bar), read the chat's user messages. sessions.db is read READ-ONLY as ground truth.
//
// The brief carries the guidance (goose-task-author briefs, local-edition/mlx/quality/briefs/*.json):
//   "needsYou": [ { "match": "<words that must all appear in the question>", "answer": "<option text or reply>" } ],
//   "defaultStance": "<one persona sentence, the free-text reply when nothing matches>"
// First matching entry wins, so list the specific ones first. Sources recorded per answer:
//   guided          — a needsYou entry matched;
//   fallback-choice — nothing matched and the card has options: its recommended answer, else its first option;
//   stance          — nothing matched, free-text card: defaultStance;
//   unanswerable    — nothing matched, free text, no defaultStance: the card is left open, never invented.
//
// CLI (READ-ONLY, never clicks, never navigates):
//   node needsyou.mjs --dry-run [--brief <brief.json>] [--db-session <id> ...]
// prints the cards of the chat on screen and what r1 would answer; --db-session also runs the choice over that
// session's stored questions (open or not), for proving guidance against questions goose really asked.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const SESSIONS_DB = `${process.env.HOME}/.local/share/goose/sessions/sessions.db`;

export const norm = (s) => String(s ?? '').replace(/[‘’]/g, "'").replace(/[“”]/g, '"').replace(/\s+/g, ' ').trim().toLowerCase();
export const words = (s) => norm(s).split(/[^a-z0-9]+/).filter(Boolean);

/** A brief's guidance, validated once at load — a malformed entry is a loud error, never skipped. */
export function loadGuidance(brief) {
  const list = brief?.needsYou ?? [];
  if (!Array.isArray(list)) throw new Error('brief.needsYou must be a list of {match, answer}');
  list.forEach((g, i) => {
    if (typeof g?.match !== 'string' || !words(g.match).length) throw new Error(`brief.needsYou[${i}].match must be non-empty words`);
    if (typeof g?.answer !== 'string' || !g.answer.trim()) throw new Error(`brief.needsYou[${i}].answer must be non-empty text`);
  });
  const stance = brief?.defaultStance;
  if (stance !== undefined && (typeof stance !== 'string' || !stance.trim())) throw new Error('brief.defaultStance must be a non-empty sentence when present');
  return { answers: list, defaultStance: stance?.trim() ?? '' };
}

/** The MOST SPECIFIC entry whose every word appears in the question (word match, case and punctuation
 * free): the one with the most match words, ties to the earlier entry. E2E #3w (2026-09-29): first-match let
 * "mince" (the price entry) answer "is the sulphite on the peel only, or the whole mincemeat?" with a price, twice. */
export function matchGuidance(question, answers = []) {
  const q = new Set(words(question));
  let best = null;
  for (let i = 0; i < answers.length; i++) {
    const w = words(answers[i].match);
    if (w.every((x) => q.has(x)) && (!best || w.length > best.size)) best = { index: i, entry: answers[i], size: w.length };
  }
  return best && { index: best.index, entry: best.entry };
}

/**
 * What the persona answers. card = { question, recommended, options } where options are the question's own
 * options as goose stored them (sessions.db), or the chips on screen when the store could not be read.
 */
export function chooseAnswer(card, guidance) {
  const m = matchGuidance(card.question, guidance.answers);
  if (m) return { answer: m.entry.answer, source: 'guided', match: m.entry.match };
  const options = (card.options ?? []).filter((o) => norm(o));
  if (options.length) {
    return norm(card.recommended)
      ? { answer: card.recommended, source: 'fallback-choice', match: 'recommended' }
      : { answer: options[0], source: 'fallback-choice', match: 'first option' };
  }
  if (guidance.defaultStance) return { answer: guidance.defaultStance, source: 'stance', match: '' };
  return { answer: null, source: 'unanswerable', match: '', reason: 'no guidance' };
}

// Mirrors ui/desktop sessionActivityStore.pickOptions (Q-319): an option the recommendation already IS — the
// same words, or those words followed by its reason — is not drawn as a chip; the recommended button carries it.
export const REASON_BREAK = /^\s*(?:[.;:,!?]|[—–]\s|\()/;
const isRecommended = (recommended, choice) => {
  const r = norm(recommended); const c = norm(choice);
  return !!c && (r === c || (r.startsWith(c) && REASON_BREAK.test(r.slice(c.length))));
};
export function pickOptions(recommended, options) {
  return options.filter((o) => norm(o) && !isRecommended(recommended, o));
}

/**
 * The control that carries `answer` on the card as drawn: a chip with those words, the recommended button
 * (its words, or its words plus a reason), else the text box. chips = the option buttons on screen, in order.
 */
export function planClick(card, answer) {
  const i = (card.chips ?? []).findIndex((c) => norm(c) === norm(answer));
  if (i >= 0) return { kind: 'option', index: i, text: card.chips[i] };
  if (isRecommended(card.recommended, answer)) return { kind: 'recommended', text: card.recommended };
  return { kind: 'text', text: answer };
}

/** The desktop's answer message (sessionActivityStore.answerMessage): `Answer to your question "<q>": <a>`. */
export function isAnswerMessage(text, { question, answer }) {
  const t = words(text).join(' ');
  return t.includes('answer to your question')
    && t.includes(words(question).slice(0, 6).join(' '))
    && t.includes(words(answer).slice(0, 12).join(' '));
}

export function parseDbItems(out) {
  const s = String(out ?? '').trim();
  if (!s) return { ok: true, absent: true, items: [] };
  const v = JSON.parse(s);
  if (!Array.isArray(v?.items)) throw new Error(`needs_you.v0 has no items list: ${s.slice(0, 120)}`);
  return { ok: true, absent: false, items: v.items };
}

/** The session's needs-you items from sessions.db, read-only. An unreadable store says so; it is never empty. */
export function readDbItems(sessionId, db = SESSIONS_DB) {
  if (!/^[\w.-]+$/.test(sessionId ?? '')) return { ok: false, error: `no session id (${sessionId})`, items: [] };
  try {
    const out = execFileSync('sqlite3', ['-readonly', db, `select json_extract(extension_data, '$."needs_you.v0"') from sessions where id='${sessionId}'`], { encoding: 'utf8' });
    return parseDbItems(out);
  } catch (e) { return { ok: false, error: String(e.message ?? e).split('\n')[0].slice(0, 200), items: [] }; }
}

export const sessionIdOf = (url) => (String(url ?? '').match(/resumeSessionId=([^&#]+)/) ?? [])[1] ?? '';

// ---------------------------------------------------------------- DOM (the chat on screen only)

/** The needs-you tray of the VISIBLE chat (the app keeps other chats mounted but hidden). Reads only. */
export const readTray = (p) => p.evaluate(() => {
  const tx = (e) => (e?.textContent ?? '').replace(/\s+/g, ' ').trim();
  const trays = [...document.querySelectorAll('[data-testid=needs-you-tray]')].filter((t) => t.offsetParent);
  const stack = trays.map((t) => t.querySelector('[data-testid=needs-you-stack]')).find(Boolean);
  return {
    trays: trays.length,
    busy: !!document.querySelector('button[aria-label="Stop"]'),
    stackFolded: stack ? stack.getAttribute('aria-expanded') === 'false' : null,
    cards: trays.flatMap((t) => [...t.querySelectorAll('[data-testid=needs-you-card]')].map((c) => ({
      id: c.dataset.itemId,
      folded: c.dataset.folded === 'true',
      question: tx(c.querySelector('[data-testid=needs-you-question]')),
      recommended: tx(c.querySelector('[data-testid=needs-you-recommended]')),
      chips: [...c.querySelectorAll('[data-testid=needs-you-option]')].map(tx),
      queued: !!c.querySelector('[data-testid=needs-you-queued]'),
      queuedAnswer: tx(c.querySelector('[data-testid=needs-you-queued-answer]')),
    }))),
    elicitations: trays.flatMap((t) => [...t.querySelectorAll('[data-testid=needs-you-elicitation]')].map((c) => ({ id: c.dataset.itemId, message: tx(c) }))),
    unsent: trays.flatMap((t) => [...t.querySelectorAll('[data-testid=needs-you-unsent]')].map(tx)),
  };
});

/** Visible user messages (texts) and visible assistant message count, for delivery and the model's reply. */
export const readChat = (p) => p.evaluate(() => ({
  users: [...document.querySelectorAll('[data-testid=user-message-body]')].filter((e) => e.offsetParent).map((e) => e.textContent ?? ''),
  assistant: [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).length,
}));

/**
 * Answer one card through its own controls, as a person does. The only other click is opening a folded bar
 * (the stack of 2+, then the card itself) by its summary. Never navigates.
 */
export async function answerCard(p, itemId, plan) {
  const tray = p.locator('[data-testid=needs-you-tray]:visible').first();
  const stack = tray.locator('[data-testid=needs-you-stack]');
  if (await stack.count() && (await stack.getAttribute('aria-expanded')) === 'false') await tray.getByTestId('needs-you-stack-first').click();
  const card = tray.locator(`[data-testid=needs-you-card][data-item-id="${itemId}"]`);
  if ((await card.getAttribute('data-folded')) === 'true') await card.getByTestId('needs-you-fold-summary').click();
  if (plan.kind === 'option') {
    const chip = card.getByTestId('needs-you-option').nth(plan.index);
    const shown = await chip.textContent();
    if (norm(shown) !== norm(plan.text)) throw new Error(`option ${plan.index} reads "${shown}", planned "${plan.text}"`);
    await chip.click();
  } else if (plan.kind === 'recommended') {
    await card.getByTestId('needs-you-recommended').click();
  } else {
    await card.getByTestId('needs-you-input').fill(plan.text);
    await card.getByTestId('needs-you-answer').click();
  }
  return card;
}

// ---------------------------------------------------------------- CLI: the read-only dry run

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arg = (k) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : ''; };
  const args = (k) => process.argv.flatMap((a, i) => (a === k ? [process.argv[i + 1]] : []));
  if (!process.argv.includes('--dry-run')) { console.error('usage: node needsyou.mjs --dry-run [--brief <brief.json>] [--db-session <id> ...]'); process.exit(2); }
  const guidance = loadGuidance(arg('--brief') ? JSON.parse(readFileSync(arg('--brief'), 'utf8')) : null);
  const say = (card, dbItem) => {
    const options = dbItem ? dbItem.options ?? [] : card.chips;
    const recommended = card.recommended ?? dbItem?.recommended_answer ?? '';
    const chips = card.chips ?? pickOptions(recommended, options);
    const choice = chooseAnswer({ question: card.question, recommended, options }, guidance);
    const plan = choice.answer === null ? null : planClick({ chips, recommended }, choice.answer);
    return { question: card.question, options, recommended, optionsFrom: dbItem ? 'db' : 'dom', choice, click: plan };
  };
  for (const sid of args('--db-session')) {
    const db = readDbItems(sid);
    console.log(JSON.stringify({ dbSession: sid, ok: db.ok, error: db.error, items: db.items.length }));
    for (const it of db.items) console.log(JSON.stringify({ id: it.id, status: it.status, ...say({ question: it.question, recommended: it.recommended_answer }, it) }, null, 1));
  }
  const { chromium } = await import('/Users/mihaiperdum/Projects/goose/ui/node_modules/playwright-core/index.mjs');
  const { mainPage } = await import('./mainpage.mjs');
  const b = await chromium.connectOverCDP('http://127.0.0.1:9333');
  const p = await mainPage(b);
  const sid = sessionIdOf(p.url());
  const tray = await readTray(p);
  const db = readDbItems(sid);
  const open = db.items.filter((it) => it.status === 'open');
  console.log(JSON.stringify({ openChat: p.url().split('#')[1] ?? '', session: sid, busy: tray.busy, visibleTrays: tray.trays, cardsOnScreen: tray.cards.length, elicitations: tray.elicitations.length, stackFolded: tray.stackFolded, db: db.ok ? { items: db.items.length, open: open.length } : { error: db.error } }));
  for (const c of tray.cards) console.log(JSON.stringify({ id: c.id, folded: c.folded, queued: c.queued, ...say(c, db.items.find((it) => it.id === c.id)) }, null, 1));
  for (const it of open) if (!tray.cards.some((c) => c.id === it.id)) console.log(JSON.stringify({ openInDbNotOnScreen: it.id, question: it.question }));
  if (!tray.cards.length) console.log(`no needs-you cards on screen in ${sid || 'the open view'} — nothing would be answered`);
  process.exit(0);
}
