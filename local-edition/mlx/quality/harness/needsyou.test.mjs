// node --test local-edition/mlx/quality/harness/needsyou.test.mjs — Q-376's pure half: matching, the answer
// choice and the control that carries it. The three questions below are the ones goose really asked in
// earlier E2Es (sessions.db 20260928_17, 20260928_19), verbatim.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { loadGuidance, matchGuidance, chooseAnswer, planClick, pickOptions, isAnswerMessage, parseDbItems, sessionIdOf, textUse, textUseCell, contentWords, openCardFails } from './needsyou.mjs';

const BRIEF = fileURLToPath(new URL('../briefs/2026-09-25-1-jira-migration-readiness.json', import.meta.url));
const jira = loadGuidance(JSON.parse(readFileSync(BRIEF, 'utf8')));

const FOLDER = {
  question: "Want /Users/mihaiperdum/goose-builds/quality/RU-2026-09-28-3p-split-tensor set as this chat's folder?",
  recommended: 'Yes', options: ['Yes', 'No — keep it on the home folder'],
};
const SVC_EDI = {
  question: 'On the lead question (EDU\'s svc-edi) and the inactivity rule — is "inactive" defined by the active field or by last_login older than 24 months, and do you want the svc-edi lead represented in users.csv?',
  recommended: 'Inactive = the active field (active=false), so an active-but-stale lead is the thing to protect; and yes, give EDI a stand-in svc-edi row so every lead resolves.',
  options: [],
};
const LEAD = {
  question: 'An inactive project lead — which decision does the script give them?',
  recommended: 'migrate — apply the lead override in every case. The "never skipped" rule is satisfied literally, and the reason column notes the lead so the check can confirm those 6 rows are migrate, never skip.',
  options: ['migrate — apply the lead override in every case', "migrate + special 'lead-inactive' marker so the review list catches exactly 6", "skip unless they have an email — a lead without an email can't be created in Cloud anyway"],
};
const drawn = (q) => ({ ...q, chips: pickOptions(q.recommended, q.options) });

test('the chat-folder offer: guided Yes, carried by the recommended button', () => {
  const c = chooseAnswer(FOLDER, jira);
  assert.equal(c.source, 'guided'); assert.equal(c.answer, 'Yes');
  assert.deepEqual(drawn(FOLDER).chips, ['No — keep it on the home folder']);
  assert.deepEqual(planClick(drawn(FOLDER), c.answer), { kind: 'recommended', text: 'Yes' });
});

test('the svc-edi / inactivity question: guided free text from the brief facts, not the card recommendation', () => {
  const c = chooseAnswer(SVC_EDI, jira);
  assert.equal(c.source, 'guided'); assert.equal(c.match, 'svc-edi');
  assert.match(c.answer, /24 months/); assert.match(c.answer, /last_login/); assert.match(c.answer, /never skipped/);
  assert.doesNotMatch(c.answer, /active=false/);
  assert.equal(planClick(drawn(SVC_EDI), c.answer).kind, 'text');
});

test('the inactive-lead decision: the lead-override option, which the card draws as its recommended button (Q-319)', () => {
  const c = chooseAnswer(LEAD, jira);
  assert.equal(c.source, 'guided'); assert.equal(c.match, 'inactive project lead');
  assert.equal(drawn(LEAD).chips.length, 2, 'the option the recommendation already is, is not a chip');
  assert.equal(planClick(drawn(LEAD), c.answer).kind, 'recommended');
});

test('the first matching entry wins, and every word of match must be in the question', () => {
  const g = { answers: [{ match: 'inactive project lead', answer: 'A' }, { match: 'lead', answer: 'B' }], defaultStance: '' };
  assert.equal(matchGuidance('Which lead owns FRT?', g.answers).entry.answer, 'B');
  assert.equal(matchGuidance('An INACTIVE project lead?', g.answers).entry.answer, 'A');
  assert.equal(matchGuidance('Leads of inactive projects?', g.answers), null, 'words, not substrings: "leads" is not "lead"');
  const mp = [{ match: 'mince', answer: 'price' }, { match: 'mince sulphite', answer: 'tag it' }];
  assert.equal(matchGuidance('Is the sulphite note on the mince pie peel only?', mp).entry.answer, 'tag it', 'the more specific entry wins over an earlier one-word match');
  assert.equal(matchGuidance('What does the mince pie cost?', mp).entry.answer, 'price');
  assert.equal(matchGuidance("Set as this chat’s folder?", [{ match: "this chat's folder", answer: 'Yes' }]).entry.answer, 'Yes', 'curly apostrophe');
});

test('no match, options: the recommended answer, recorded as fallback-choice', () => {
  const c = chooseAnswer({ question: 'Which colour?', recommended: 'Blue', options: ['Red', 'Blue'] }, jira);
  assert.deepEqual([c.source, c.answer, c.match], ['fallback-choice', 'Blue', 'recommended']);
});

test('no match, options, no recommendation: the first option', () => {
  const c = chooseAnswer({ question: 'Which colour?', recommended: '  ', options: ['Red', 'Blue'] }, jira);
  assert.deepEqual([c.source, c.answer, c.match], ['fallback-choice', 'Red', 'first option']);
});

test('no match, free text: the persona stance', () => {
  const c = chooseAnswer({ question: 'Anything else?', recommended: 'Nothing', options: [] }, jira);
  assert.equal(c.source, 'stance'); assert.equal(c.answer, jira.defaultStance);
});

test('no match, free text, no stance: unanswerable — nothing is invented', () => {
  const c = chooseAnswer({ question: 'Anything else?', recommended: 'Nothing', options: [] }, loadGuidance(null));
  assert.deepEqual([c.source, c.answer, c.reason], ['unanswerable', null, 'no guidance']);
});

test('planClick: an exact chip, the recommended words, recommended + its reason, else typed', () => {
  const card = { recommended: 'migrate — keep them. Reason follows.', chips: ['skip them', 'Review'] };
  assert.deepEqual(planClick(card, 'review'), { kind: 'option', index: 1, text: 'Review' });
  assert.equal(planClick(card, 'migrate — keep them').kind, 'recommended');
  assert.equal(planClick(card, 'migrate — keep them. Reason follows.').kind, 'recommended');
  assert.equal(planClick(card, 'migrate').kind, 'recommended', 'as the UI: " — keep them…" after "migrate" is a reason break');
  assert.equal(planClick(card, 'migr').kind, 'text', 'a prefix without a reason break is not the recommendation');
  assert.deepEqual(planClick(card, 'something else'), { kind: 'text', text: 'something else' });
});

test('the answer message is recognised in the chat, including inside a batch of two', () => {
  const one = `Answer to your question "${LEAD.question}": migrate — apply the lead override in every case`;
  assert.ok(isAnswerMessage(one, { question: LEAD.question, answer: 'migrate — apply the lead override in every case' }));
  assert.ok(!isAnswerMessage(one, { question: FOLDER.question, answer: 'Yes' }));
  const batch = `${one}\n\nAnswer to your question "${FOLDER.question}": Yes`;
  assert.ok(isAnswerMessage(batch, { question: FOLDER.question, answer: 'Yes' }));
  assert.ok(!isAnswerMessage('Yes please, set the folder', { question: FOLDER.question, answer: 'Yes' }), 'a typed message is not a card answer');
});

test('the store: absent key = no items (said so), a malformed value throws', () => {
  assert.deepEqual(parseDbItems('\n'), { ok: true, absent: true, items: [] });
  assert.equal(parseDbItems('{"items":[{"id":"ny_1","status":"open"}]}').items[0].id, 'ny_1');
  assert.throws(() => parseDbItems('{"nope":1}'));
});

test('the brief guidance is validated loudly', () => {
  assert.throws(() => loadGuidance({ needsYou: { match: 'x' } }), /list/);
  assert.throws(() => loadGuidance({ needsYou: [{ match: '  ', answer: 'x' }] }), /match/);
  assert.throws(() => loadGuidance({ needsYou: [{ match: 'x', answer: '' }] }), /answer/);
  assert.throws(() => loadGuidance({ defaultStance: '' }), /defaultStance/);
  assert.deepEqual(loadGuidance(null), { answers: [], defaultStance: '' });
});

test('session id from the chat URL', () => {
  assert.equal(sessionIdOf('file:///x/index.html#/pair?resumeSessionId=20260928_21'), '20260928_21');
  assert.equal(sessionIdOf('file:///x/index.html#/'), '');
});

// The outcome column (owner 2026-09-29): answers and replies verbatim from E2E #3w (session 20260929_12,
// needsyou.tsv) — two replies that used the answer, and the price answer given to a sulphite question.
const SCONE = { a: "€3.50 in both shops. Douglas goes up on 1 October, so there's just the one price.", r: 'On the scone: €3.50 in both shops now, and Douglas catches up on 1 October. One price, so the two-board note is spent.' };
const SODA = { a: 'Oat milk, in both shops, since August. The buttermilk version is gone. Butter still comes on the side with every slice, and the soup and the salmon come with buttered soda bread too.', r: "On the soda bread: oat milk in both shops since August, the buttermilk version is gone. That flips three dishes — the soup and the salmon lose milk and the bread's only allergen is now cereals, and everything comes with butter on the side." };
const MINCE = { a: '€2.50 each, or four for €9.00. Same in both shops.', r: 'That answers the price, not the sulphites — and the two are linked, so I want it rather than leave the tag off and risk an inspector finding it on the mince pie.' };

test('textUse: a reply that repeats the answer quotes it; one that talks past it does not', () => {
  assert.equal(textUse(SCONE.a, SCONE.r).verdict, 'quote');
  assert.match(textUseCell(SODA.a, SODA.r), /^quote "/);
  const m = textUse(MINCE.a, MINCE.r);
  assert.equal(m.verdict, 'no', `the #3w mince reply does not use the price: ${JSON.stringify(m)}`);
  assert.equal(textUseCell(MINCE.a, ''), 'no reply');
  assert.equal(textUseCell('Yes', 'Done, the folder is set.'), 'unmeasurable (0 content words)', 'a bare Yes cannot be proven used by overlap');
});

test('textUse: the facts paraphrased with no five-word run is "uses"; numbers count whole', () => {
  assert.deepEqual(contentWords('€3.50 in both shops, 1 October'), ['3.50', 'shops', '1', 'october']);
  const u = textUse(SCONE.a, 'Douglas moves to 3.50 from 1 October — one price across shops.');
  assert.equal(u.verdict, 'uses'); assert.ok(u.share >= 0.3);
});

test('openCardFails: every open question from the store and the tray, once each; an unreadable store says so', () => {
  const db = { ok: true, items: [{ id: 'ny_1', status: 'open', question: 'Which price?' }, { id: 'ny_2', status: 'answered', question: 'x' }] };
  assert.deepEqual(openCardFails(db, { cards: [{ id: 'ny_1', question: 'Which price?' }, { id: 'ny_3', question: 'Folder?' }] }),
    ['FAIL: open card ny_1 Which price?', 'FAIL: open card ny_3 Folder?']);
  assert.deepEqual(openCardFails({ ok: true, items: [] }), []);
  assert.match(openCardFails({ ok: false, error: 'locked', items: [] })[0], /could not be read: locked/);
});
