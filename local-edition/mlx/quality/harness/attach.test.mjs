// node --test local-edition/mlx/quality/harness/attach.test.mjs — the attach plan and its loud failures (pure half),
// against the real 2026-10-31-3 brief and its seed files. The DOM half: attach.dom.test.mjs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, mkdirSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { attachPlan, missingFails, attachedCell, chipLanded, storedAttachments, kindOf } from './attach.mjs';

const BRIEFS = fileURLToPath(new URL('../briefs/', import.meta.url));
const clinic = JSON.parse(readFileSync(`${BRIEFS}2026-10-31-3-clinic-booking-screenshot-voice-notes.json`, 'utf8'));
const SEED = `${BRIEFS}seeds/2026-10-31-3`;
const seededWork = () => {
  const work = mkdtempSync(`${tmpdir()}/attach-test-`); mkdirSync(`${work}/inbox`);
  for (const f of ['booking-screenshot.png', 'voice-note-1.wav', 'voice-note-2.m4a']) copyFileSync(`${SEED}/${f}`, `${work}/inbox/${f}`);
  return work;
};

test('the clinic brief: every attach turn plans its file under {WORK}, with its kind, and the seeded files exist', () => {
  const work = seededWork();
  try {
    const plans = clinic.turns.map((t, i) => [i, attachPlan(t, work)]).filter(([, p]) => p.length);
    assert.deepEqual(plans.map(([i, p]) => [i, p.map((f) => `${f.name}:${f.kind}`).join()]), [
      [0, 'booking-screenshot.png:image'], [4, 'voice-note-1.wav:audio'], [7, 'booking-screenshot.png:image'],
      [8, 'voice-note-2.m4a:audio'], [12, 'booking-screenshot.png:image'], [17, 'voice-note-1.wav:audio'],
    ]);
    for (const [, p] of plans) for (const f of p) { assert.equal(f.path, `${work}/inbox/${f.name}`); assert.equal(f.exists, true); }
    assert.deepEqual(plans.flatMap(([, p]) => missingFails(p)), []);
    assert.deepEqual(attachPlan(clinic.turns[1], work), [], 'a turn with no attach list plans nothing');
  } finally { rmSync(work, { recursive: true, force: true }); }
});

test('relative paths read against the work dir, absolute ones are kept', () => {
  const plan = attachPlan({ attach: ['inbox/a.png', '/abs/b.wav', '{WORK}/c.pdf'] }, '/w', { exists: () => true });
  assert.deepEqual(plan.map((f) => [f.path, f.kind]), [['/w/inbox/a.png', 'image'], ['/abs/b.wav', 'audio'], ['/w/c.pdf', 'file']]);
});

test('a missing file is a loud ATTACH_FAIL naming the path and the brief entry — never dropped quietly', () => {
  const work = seededWork();
  try {
    rmSync(`${work}/inbox/voice-note-1.wav`);
    const plan = attachPlan(clinic.turns[4], work);
    assert.equal(plan[0].exists, false);
    const fails = missingFails(plan);
    assert.equal(fails.length, 1);
    assert.match(fails[0], /^ATTACH_FAIL missing file .*inbox\/voice-note-1\.wav \(brief: \{WORK\}\/inbox\/voice-note-1\.wav\)$/);
    assert.equal(attachedCell([{ ...plan[0], ok: false, why: 'missing file' }]), 'voice-note-1.wav:audio:FAIL(missing file)');
  } finally { rmSync(work, { recursive: true, force: true }); }
});

test('a directory is not a file to attach', () => {
  const work = seededWork();
  try { assert.equal(attachPlan({ attach: ['inbox'] }, work)[0].exists, false); } finally { rmSync(work, { recursive: true, force: true }); }
});

test('a malformed attach list refuses instead of being read as no attachments', () => {
  assert.throws(() => attachPlan({ attach: 'inbox/a.png' }, '/w'), /must be a list/);
  assert.throws(() => attachPlan({ attach: [''] }, '/w'), /non-path entry/);
});

test('chip wait: an image is attached only once a NEW tile holds its data, an audio chip only by its own name', () => {
  const img = { kind: 'image', name: 's.png', path: '/w/s.png' };
  const none = { found: true, images: [], media: [], value: '' };
  assert.equal(chipLanded(img, none, none).state, 'wait');
  assert.equal(chipLanded(img, none, { ...none, images: [{ ready: false, error: '' }] }).state, 'wait');
  assert.equal(chipLanded(img, none, { ...none, images: [{ ready: true, error: '' }] }).state, 'ok');
  assert.equal(chipLanded(img, { ...none, images: [{ ready: true, error: '' }] }, { ...none, images: [{ ready: true, error: '' }] }).state, 'wait', 'an older tile is not this pick');
  assert.equal(chipLanded(img, none, { ...none, images: [{ ready: false, error: 'Failed to read image' }] }).state, 'error');
  const wav = { kind: 'audio', name: 'v.wav', path: '/w/v.wav' };
  assert.equal(chipLanded(wav, none, { ...none, media: [{ kind: 'audio', name: 'other.wav', ready: true, error: '' }] }).state, 'wait');
  assert.equal(chipLanded(wav, none, { ...none, media: [{ kind: 'audio', name: 'v.wav', ready: false, error: '' }] }).state, 'wait');
  assert.equal(chipLanded(wav, none, { ...none, media: [{ kind: 'audio', name: 'v.wav', ready: true, error: '' }] }).state, 'ok');
  assert.equal(chipLanded(wav, none, { ...none, media: [{ kind: 'audio', name: 'v.wav', ready: false, error: 'Over 50 MB' }] }).state, 'error');
  const pdf = { kind: 'file', name: 'q.pdf', path: '/w/q.pdf' };
  assert.equal(chipLanded(pdf, none, { ...none, value: 'read this /w/q.pdf' }).state, 'ok');
  assert.equal(chipLanded(pdf, none, { ...none, value: 'read this' }).state, 'wait');
  assert.equal(chipLanded(img, none, { found: false, images: [], media: [], value: '' }).state, 'wait');
});

test('the turns.tsv cell and the stored-row count', () => {
  assert.equal(attachedCell([]), '');
  assert.equal(attachedCell([{ name: 'a.png', kind: 'image', ok: true }, { name: 'b.wav', kind: 'audio', ok: false, why: 'no chip after 30s:\tx' }]), 'a.png:image:ok, b.wav:audio:FAIL(no chip after 30s: x)');
  assert.deepEqual(storedAttachments(JSON.stringify([{ type: 'text', text: 'hi' }, { type: 'image', data: 'x' }, { type: 'media', kind: 'audio' }])), { images: 1, media: 1 });
  assert.equal(storedAttachments('not json'), null);
  assert.equal(kindOf('/x/Y.M4A'), 'audio');
});
