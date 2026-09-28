// node --test local-edition/mlx/quality/harness/workdir.test.mjs — Q-390's pure half: the round's work path, the
// sidebar row it is found by, and the sessions.db read that proves where the chat really runs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { workDirOf, projectRowTestId, readWorkingDir, checkWorkingDir } from './workdir.mjs';

const scratch = mkdtempSync(join(tmpdir(), 'q390-'));
const round = join(scratch, 'RU-2026-09-28-9z-split-tensor');
const work = join(round, 'work');
mkdirSync(work, { recursive: true });
const db = join(scratch, 'sessions.db');
execFileSync('sqlite3', [db, `create table sessions (id text primary key, working_dir text);
  insert into sessions values ('20260928_21', '${process.env.HOME}');
  insert into sessions values ('20260928_30', '${work}');
  insert into sessions values ('20260928_31', '${work}/');`]);
test.after(() => rmSync(scratch, { recursive: true, force: true }));

test('the work folder is <round>/work, absolute, from an absolute, relative or slash-ended round dir', () => {
  assert.equal(workDirOf('/Users/x/goose-builds/quality/RU-1'), '/Users/x/goose-builds/quality/RU-1/work');
  assert.equal(workDirOf('/Users/x/goose-builds/quality/RU-1/'), '/Users/x/goose-builds/quality/RU-1/work');
  assert.equal(workDirOf('RU-1'), join(process.cwd(), 'RU-1', 'work'));
});

test('the sidebar row id is the folder with trailing slashes off (ProjectsSection normalizeDirPath)', () => {
  assert.equal(projectRowTestId(work), `project-row-${work}`);
  assert.equal(projectRowTestId(`${work}//`), `project-row-${work}`);
  assert.equal(projectRowTestId('/'), 'project-row-/');
});

test('a chat opened in the work folder passes; the $HOME chat of E2E #3r (20260928_21) is refused, loudly', () => {
  const ok = readWorkingDir('20260928_30', db);
  assert.deepEqual(ok, { ok: true, workingDir: work });
  assert.equal(checkWorkingDir(ok, work).ok, true);
  assert.equal(checkWorkingDir(readWorkingDir('20260928_31', db), work).ok, true, 'a trailing slash is the same folder');
  const home = checkWorkingDir(readWorkingDir('20260928_21', db), work);
  assert.equal(home.ok, false);
  assert.match(home.says, new RegExp(`working_dir is "${process.env.HOME}", not ${work}`));
});

test('a symlinked spelling of the work folder is the same folder (/tmp vs /private/tmp)', () => {
  assert.equal(checkWorkingDir({ ok: true, workingDir: '/tmp' }, '/private/tmp').ok, true);
});

test('a missing session, a bad id or an unreadable store is never read as a folder', () => {
  const missing = readWorkingDir('20260928_99', db);
  assert.equal(missing.ok, false); assert.match(missing.error, /not in/);
  assert.equal(checkWorkingDir(missing, work).ok, false);
  assert.equal(readWorkingDir("x' or '1'='1", db).ok, false);
  assert.equal(readWorkingDir('', db).ok, false);
  const gone = readWorkingDir('20260928_30', join(scratch, 'nope', 'sessions.db'));
  assert.equal(gone.ok, false);
  assert.match(checkWorkingDir(gone, work).says, /unreadable/);
  assert.match(checkWorkingDir({ ok: true, workingDir: join(round, 'elsewhere') }, work).says, /does not exist/);
});
