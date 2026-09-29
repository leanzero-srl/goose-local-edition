// node --test local-edition/mlx/quality/harness/needsyou.test.mjs — Q-376's pure half: matching, the answer
// choice and the control that carries it. The three questions below are the ones goose really asked in
// earlier E2Es (sessions.db 20260928_17, 20260928_19), verbatim.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { loadGuidance, matchGuidance, chooseAnswer, planClick, pickOptions, isAnswerMessage, parseDbItems, sessionIdOf } from './needsyou.mjs';

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
