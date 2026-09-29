import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { displaySessionListName } from '../../sessions';
import { noteWords } from '../notes/noteWords';
import type { ActiveSession } from './sessionActivityStore';

/**
 * Q-486: the top bar said "2 need you" (open questions), the engine glance "1 needs you" (chats)
 * and the Active now row "Needs you · 2" — one screen, three counts, the same words. Every surface
 * that counts what waits on the person says it through HERE: when each waiting chat asks one
 * question the two counts agree and "N need you" says both; when they differ, the words name
 * what is counted — "2 questions in 1 chat".
 *
 * Q-489: and every surface that LISTS what waits on the person names it through here too — the
 * chat's name (`activeRowName`), what it is doing (`activeRowDetail`) and, where a question is
 * shown on one line, its first sentence (`questionGist`), the whole text left to a tooltip.
 */
export const needsYouWords = defineMessages({
  chats: {
    id: 'needsYou.count.chats',
    defaultMessage: '{count, plural, one {# needs you} other {# need you}}',
  },
  questionsInChats: {
    id: 'needsYou.count.questionsInChats',
    defaultMessage:
      '{questions, plural, one {# question} other {# questions}} in {chats, plural, one {# chat} other {# chats}}',
  },
});

const rowWords = defineMessages({
  unnamed: { id: 'activeNowSection.unnamed', defaultMessage: 'Untitled session' },
  startedAt: { id: 'activeNowSection.startedAt', defaultMessage: 'started {time}' },
  waitingSince: {
    id: 'activeNowSection.waitingSince',
    defaultMessage: 'waiting for your answer since {time}',
  },
  waiting: { id: 'activeNowSection.waiting', defaultMessage: 'waiting for your answer' },
});

export interface NeedsYouCount {
  /** Open questions (and live elicitations) across the waiting chats. */
  questions: number;
  /** Chats with at least one of them. */
  chats: number;
}

export function needsYouCountLabel(intl: IntlShape, { questions, chats }: NeedsYouCount): string {
  return questions === chats
    ? intl.formatMessage(needsYouWords.chats, { count: chats })
    : intl.formatMessage(needsYouWords.questionsInChats, { questions, chats });
}

/** The chat's name as every active-chat list says it; a chat with none is "Untitled session". */
export function activeRowName(intl: IntlShape, sessionName: string): string {
  return sessionName ? displaySessionListName(sessionName) : intl.formatMessage(rowWords.unnamed);
}

const clock = (intl: IntlShape, iso: string) =>
  intl.formatTime(Date.parse(iso), { hour: '2-digit', minute: '2-digit' });

/**
 * THE one derivation of an active chat's state line (Q-484): the folder, then what the chat is
 * doing — waiting for the person's answer (since when the oldest question was asked) or running a
 * turn (since when it started) — then any notes waiting. Never the question's own words: where a
 * surface shows the question, it shows `questionGist` on a line of its own.
 */
export function activeRowDetail(intl: IntlShape, row: ActiveSession, project: string): string {
  const doing =
    row.needsYou > 0
      ? row.waitingSince
        ? intl.formatMessage(rowWords.waitingSince, { time: clock(intl, row.waitingSince) })
        : intl.formatMessage(rowWords.waiting)
      : row.runningSince
        ? intl.formatMessage(rowWords.startedAt, { time: clock(intl, row.runningSince) })
        : '';
  const notes =
    row.notesWaiting > 0 ? intl.formatMessage(noteWords.waiting, { count: row.notesWaiting }) : '';
  return [project, doing, notes].filter(Boolean).join(' · ');
}

/**
 * A question is a paragraph at most; its gist is one line. Only this much of it is read, so a
 * pasted page never reaches the scan below.
 */
const GIST_READ = 600;

/**
 * THE one-line form of a question (Q-489): its first line, cut after its first sentence (". ",
 * "? ", "! ", "; "), whitespace collapsed. The line it sits on truncates what is still too long;
 * the full question belongs in that line's tooltip and in the card itself. A period inside a
 * word — "report.docx", "3.0.78" — is not a sentence end.
 */
export function questionGist(question: string): string {
  const text = question.slice(0, GIST_READ).trim();
  const line = text.split(/\r?\n/, 1)[0] ?? '';
  const end = /[.?!;](?=\s|$)/.exec(line);
  const sentence = end ? line.slice(0, end.index + (end[0] === ';' ? 0 : 1)) : line;
  return sentence.replace(/\s+/g, ' ').trim();
}
