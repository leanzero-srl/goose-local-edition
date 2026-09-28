// workdir.mjs — Q-390: r1 opens the round's chat in <round>/work, and proves it from sessions.db.
// Until 2026-09-28 every real-use chat ran with its folder = $HOME (sessions 20260928_16.._21), because r1
// clicked the landing's "New session in <latest folder>" and the latest folder was $HOME. A rubric item like
// "lands in the work folder" then graded the harness, not goose.
//
// HOW A PERSON DOES IT, and the one step CDP cannot: the sidebar's Projects "+" (Add a project folder) runs the
// OS directory chooser, registers the pick (window.electron.addProject) and broadcasts PROJECTS_CHANGED
// (ui/desktop/src/utils/addProjectFlow.ts chooseAndAddProject); the folder's row then carries "New session
// here", which starts the chat through the same startNewSession(…, projectPath) every surface uses. The OS
// chooser is a native dialog CDP cannot drive, so r1 performs exactly what the flow does with the dialog's
// answer — addProject(<work>) + the PROJECTS_CHANGED broadcast — and clicks the rest like a person.
// Rejected: the chat's folder chip (DirSwitcher) MOVES a chat that already started in $HOME, so the session is
// born in the wrong folder; a raw ACP newSession skips every product surface.
import { execFileSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { resolve } from 'node:path';
import { SESSIONS_DB } from './needsyou.mjs';

/** The round's work folder, absolute: addProject refuses a relative path, and the model is told this path. */
export const workDirOf = (roundDir) => resolve(String(roundDir ?? ''), 'work');

/** The project row the sidebar draws for a folder (ProjectsSection normalizeDirPath: trailing slashes off). */
export const projectRowTestId = (dir) => `project-row-${String(dir).replace(/\/+$/, '') || '/'}`;

/** The session's working_dir as the engine stored it. Unreadable or missing says so — never an empty folder. */
export function readWorkingDir(sessionId, db = SESSIONS_DB) {
  if (!/^[\w.-]+$/.test(sessionId ?? '')) return { ok: false, error: `no session id (${sessionId})` };
  try {
    const out = execFileSync('sqlite3', ['-readonly', db, `select working_dir from sessions where id='${sessionId}'`], { encoding: 'utf8' });
    const rows = out.split('\n').filter((l) => l !== '');
    if (rows.length === 0) return { ok: false, error: `session ${sessionId} is not in ${db}` };
    return { ok: true, workingDir: rows[0] };
  } catch (e) { return { ok: false, error: String(e.message ?? e).split('\n')[0].slice(0, 200) }; }
}

const real = (p) => { try { return realpathSync(p); } catch { return null; } };

/** Is the stored folder the round's work folder? Compared through realpath (/tmp vs /private/tmp), stated raw. */
export function checkWorkingDir(row, work) {
  if (!row.ok) return { ok: false, says: `sessions.db unreadable for this chat: ${row.error}` };
  const want = real(work); const got = real(row.workingDir);
  if (want && got && want === got) return { ok: true, says: `working_dir ${row.workingDir}` };
  return { ok: false, says: `working_dir is ${JSON.stringify(row.workingDir)}, not ${work}${got ? '' : ' (that folder does not exist)'}` };
}
