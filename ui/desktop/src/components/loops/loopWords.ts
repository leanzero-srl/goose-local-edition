import type { MessageDescriptor } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { Sentence } from './model';

/**
 * The NOW sentences of `model.ts`'s `statusSentence`, keyed by the key it returns. Each default
 * message renders, with the sentence's facts, EXACTLY the English the model returns (the fixture
 * test `loopWords.test.ts` pins every case of `loops.fixture.json`), so the model owns the facts
 * and the words live in the catalogs.
 */
export const sentenceMessages = defineMessages({
  'loops.now.running': {
    id: 'loops.now.running',
    defaultMessage: 'Tick {n} · started {time} · {elapsed}',
  },
  'loops.now.runningOn': {
    id: 'loops.now.runningOn',
    defaultMessage: 'Tick {n} · started {time} · {elapsed} · on {node}',
  },
  'loops.now.checking': {
    id: 'loops.now.checking',
    defaultMessage: 'Checking `{check}` · {elapsed}',
  },
  'loops.now.selfPaced': {
    id: 'loops.now.selfPaced',
    defaultMessage: 'Next tick {time} — goose chose {interval}: "{reason}"',
  },
  'loops.now.selfPacedNoReason': {
    id: 'loops.now.selfPacedNoReason',
    defaultMessage: 'Next tick {time} — goose chose {interval} and gave no reason',
  },
  'loops.now.startsNow': {
    id: 'loops.now.startsNow',
    defaultMessage: 'Next tick starts now',
  },
  'loops.now.nextAt': {
    id: 'loops.now.nextAt',
    defaultMessage: 'Next tick {time} · in {rel}',
  },
  'loops.now.dueAfterYourTurn': {
    id: 'loops.now.dueAfterYourTurn',
    defaultMessage: 'Tick {next} is due — it starts when your turn in "{chat}" ends',
  },
  'loops.now.dueAfterYourMessage': {
    id: 'loops.now.dueAfterYourMessage',
    defaultMessage: 'Tick {next} is due — it starts after your message here',
  },
  'loops.now.dueAfterStop': {
    id: 'loops.now.dueAfterStop',
    defaultMessage: 'Tick {next} is due — it starts when the answer you stopped here has settled',
  },
  'loops.now.dueLoadFailed': {
    id: 'loops.now.dueLoadFailed',
    defaultMessage: 'Tick {next} is due — this chat could not be opened in the window: {error}',
  },
  'loops.now.dueSubmitFailed': {
    id: 'loops.now.dueSubmitFailed',
    defaultMessage: 'Tick {next} is due — goose refused its message: {error}',
  },
  'loops.now.dueAfterReviewers': {
    id: 'loops.now.dueAfterReviewers',
    defaultMessage: "Tick {next} is due — it starts when goose's check of tick {n} ends",
  },
  'loops.now.dueWayHeld': {
    id: 'loops.now.dueWayHeld',
    defaultMessage:
      'Tick {next} is due — {node} is answering you in "{chat}"; the tick loads {target} after',
  },
  'loops.now.noDelay': {
    id: 'loops.now.noDelay',
    defaultMessage: "Tick {n} didn't say when to come back.",
  },
  'loops.now.badDelay': {
    id: 'loops.now.badDelay',
    defaultMessage: 'Tick {n} named a delay goose can\'t read: "{given}".',
  },
  'loops.now.asked': {
    id: 'loops.now.asked',
    defaultMessage: 'Tick {n} asked you: "{question}"',
  },
  'loops.now.answerRunning': {
    id: 'loops.now.answerRunning',
    defaultMessage: 'Your answer to tick {n} is running — the next tick starts after it',
  },
  'loops.now.elsewhere': {
    id: 'loops.now.elsewhere',
    defaultMessage: 'This loop runs in another goose window.',
  },
  'loops.paused.byYou': {
    id: 'loops.paused.byYou',
    defaultMessage: 'Paused by you after tick {n}.',
  },
  'loops.paused.byYouBeforeFirst': {
    id: 'loops.paused.byYouBeforeFirst',
    defaultMessage: 'Paused by you before the first tick.',
  },
  'loops.paused.youStoppedTick': {
    id: 'loops.paused.youStoppedTick',
    defaultMessage: 'You stopped tick {n}.',
  },
  'loops.paused.blocked': {
    id: 'loops.paused.blocked',
    defaultMessage: 'Blocked — {blockedOn}',
  },
  'loops.paused.checkCouldNotRun': {
    id: 'loops.paused.checkCouldNotRun',
    defaultMessage: 'The check could not run: {error}',
  },
  'loops.paused.sameFailureTwice': {
    id: 'loops.paused.sameFailureTwice',
    defaultMessage: 'Ticks {prev} and {n} failed the same way: {error}',
  },
  'loops.paused.noReportTwice': {
    id: 'loops.paused.noReportTwice',
    defaultMessage: 'Ticks {prev} and {n} ended without a loop report',
  },
  'loops.paused.stalled': {
    id: 'loops.paused.stalled',
    defaultMessage:
      'Stalled — tick {n} named the same next step as tick {prev} and made no write or edit outside the state file',
  },
  'loops.paused.closedAt': {
    id: 'loops.paused.closedAt',
    defaultMessage:
      'goose was closed at {time}; {due, plural, one {# tick was due} other {# ticks were due}}.',
  },
  'loops.paused.closed': {
    id: 'loops.paused.closed',
    defaultMessage:
      'goose was closed; {due, plural, one {# tick was due} other {# ticks were due}}.',
  },
  'loops.paused.finishingElsewhere': {
    id: 'loops.paused.finishingElsewhere',
    defaultMessage: 'Paused — tick {n} is finishing in the other window',
  },
  'loops.ended.goalMet': {
    id: 'loops.ended.goalMet',
    defaultMessage: 'Goal met — `{check}` passed after tick {n}',
  },
  'loops.ended.reportedDone': {
    id: 'loops.ended.reportedDone',
    defaultMessage: 'goose reported the goal done after tick {n} — no check was set',
  },
  'loops.ended.reachedCount': {
    id: 'loops.ended.reachedCount',
    defaultMessage: 'Reached {k} ticks, as you set',
  },
  'loops.ended.stoppedByYou': {
    id: 'loops.ended.stoppedByYou',
    defaultMessage: 'Stopped by you after tick {n}',
  },
});

/** The descriptor for a model sentence, or null for a key the catalogs do not carry. */
export function sentenceMessage(sentence: Sentence): MessageDescriptor | null {
  const messages = sentenceMessages as Record<string, MessageDescriptor>;
  return Object.prototype.hasOwnProperty.call(messages, sentence.key)
    ? messages[sentence.key]
    : null;
}

/** A sentence's facts as ICU values (the plural `due` is a number). */
export function sentenceValues(sentence: Sentence): Record<string, string | number> {
  const values: Record<string, string | number> = { ...sentence.facts };
  if (sentence.facts.due !== undefined) values.due = Number(sentence.facts.due);
  return values;
}

export const loopWords = defineMessages({
  tabLoop: { id: 'loops.rail.tabLoop', defaultMessage: 'Loop' },
  tabChanges: { id: 'loops.rail.tabChanges', defaultMessage: 'Changes {count}' },
  tabsLabel: { id: 'loops.rail.tabsLabel', defaultMessage: 'Loop and changes' },
  closeLoop: { id: 'loops.rail.closeLoop', defaultMessage: 'Close the loop panel' },
  pillOpen: { id: 'loops.pill.open', defaultMessage: "Show this chat's loop — {label}" },

  pillRunning: { id: 'loops.pill.running', defaultMessage: 'Tick {n} running · {elapsed}' },
  pillChecking: { id: 'loops.pill.checking', defaultMessage: 'Checking · {elapsed}' },
  pillNext: { id: 'loops.pill.next', defaultMessage: 'Next tick {time}' },
  pillStartsNow: { id: 'loops.pill.startsNow', defaultMessage: 'Next tick starts now' },
  pillAfterYourTurn: {
    id: 'loops.pill.afterYourTurn',
    defaultMessage: 'Next tick after your turn',
  },
  pillAfterReviewers: {
    id: 'loops.pill.afterReviewers',
    defaultMessage: "Next tick after goose's check of tick {n}",
  },
  pillAfterWayHeld: {
    id: 'loops.pill.afterWayHeld',
    defaultMessage: 'Next tick after {node} finishes answering you',
  },
  pillCouldNotStart: {
    id: 'loops.pill.couldNotStart',
    defaultMessage: 'Next tick could not start',
  },
  pillWaitingYou: { id: 'loops.pill.waitingYou', defaultMessage: 'Loop waiting for you' },
  pillNeedsYou: { id: 'loops.pill.needsYou', defaultMessage: 'Loop needs you' },
  pillPaused: { id: 'loops.pill.paused', defaultMessage: 'Loop paused' },
  pillEnded: { id: 'loops.pill.ended', defaultMessage: 'Loop ended' },
  pillElsewhere: { id: 'loops.pill.elsewhere', defaultMessage: 'Looping in another window' },
  pillUnreadable: { id: 'loops.pill.unreadable', defaultMessage: 'Loop unreadable' },

  statusRunning: { id: 'loops.status.running', defaultMessage: 'Running' },
  statusChecking: { id: 'loops.status.checking', defaultMessage: 'Checking' },
  statusWaiting: { id: 'loops.status.waiting', defaultMessage: 'Waiting' },
  statusWaitingTurn: { id: 'loops.status.waitingTurn', defaultMessage: 'Waiting for your turn' },
  statusWaitingYou: { id: 'loops.status.waitingYou', defaultMessage: 'Waiting for you' },
  statusNeedsYou: { id: 'loops.status.needsYou', defaultMessage: 'Needs you' },
  statusPaused: { id: 'loops.status.paused', defaultMessage: 'Paused' },
  statusEnded: { id: 'loops.status.ended', defaultMessage: 'Ended' },
  statusElsewhere: { id: 'loops.status.elsewhere', defaultMessage: 'In another window' },

  cadenceSeconds: { id: 'loops.cadence.seconds', defaultMessage: 'every {n} s' },
  cadenceMinutes: { id: 'loops.cadence.minutes', defaultMessage: 'every {n} min' },
  cadenceHours: { id: 'loops.cadence.hours', defaultMessage: 'every {n} h' },
  cadenceAsTyped: { id: 'loops.cadence.asTyped', defaultMessage: 'every {every}' },
  cadenceSelfPaced: { id: 'loops.cadence.selfPaced', defaultMessage: 'goose decides when' },
  cadenceBackToBack: { id: 'loops.cadence.backToBack', defaultMessage: 'back to back' },

  headerTick: { id: 'loops.header.tick', defaultMessage: 'tick {n}' },
  headerTickOf: { id: 'loops.header.tickOf', defaultMessage: 'tick {n} of {k}' },
  headerSince: { id: 'loops.header.since', defaultMessage: 'since {time}' },
  headerTokens: { id: 'loops.header.tokens', defaultMessage: '{tokens} tokens' },
  headerCheck: { id: 'loops.header.check', defaultMessage: 'Check: {command}' },
  headerNoCheck: {
    id: 'loops.header.noCheck',
    defaultMessage: 'No check — the loop ends when goose reports done or you stop it',
  },
  headerStateFile: { id: 'loops.header.stateFile', defaultMessage: 'State file: {path}' },
  headerStateFileNotWritten: {
    id: 'loops.header.stateFileNotWritten',
    defaultMessage: 'not written yet',
  },
  headerStateFileUnreadable: {
    id: 'loops.header.stateFileUnreadable',
    defaultMessage: 'could not be read: {error}',
  },
  open: { id: 'loops.header.open', defaultMessage: 'Open' },

  tickNow: { id: 'loops.control.tickNow', defaultMessage: 'Run a tick now' },
  tickAlreadyRunning: {
    id: 'loops.control.tickAlreadyRunning',
    defaultMessage: 'A tick is already running',
  },
  pause: { id: 'loops.control.pause', defaultMessage: 'Pause' },
  resume: { id: 'loops.control.resume', defaultMessage: 'Resume' },
  resumeOneTick: {
    id: 'loops.control.resumeOneTick',
    defaultMessage: 'Resume — run one tick now',
  },
  stopLoop: { id: 'loops.control.stopLoop', defaultMessage: 'Stop loop' },
  edit: { id: 'loops.control.edit', defaultMessage: 'Edit' },
  stopCheck: { id: 'loops.control.stopCheck', defaultMessage: 'Stop check' },
  runNextTick: { id: 'loops.control.runNextTick', defaultMessage: 'Run next tick' },
  showInChat: { id: 'loops.control.showInChat', defaultMessage: 'Show in chat' },
  goToQuestion: { id: 'loops.control.goToQuestion', defaultMessage: 'Go to the question' },
  startNewLoop: { id: 'loops.control.startNewLoop', defaultMessage: 'Start a new loop' },
  startLoop: { id: 'loops.control.startLoop', defaultMessage: 'Start a loop' },
  controlRefused: { id: 'loops.control.refused', defaultMessage: 'goose refused: {reason}' },
  controlFailed: { id: 'loops.control.failed', defaultMessage: 'The request failed: {error}' },
  startDialogAbsent: {
    id: 'loops.control.startDialogAbsent',
    defaultMessage: 'The loop dialog is not in this build yet.',
  },

  stopTitle: { id: 'loops.stop.title', defaultMessage: 'Stop the loop?' },
  stopRunning: {
    id: 'loops.stop.running',
    defaultMessage: 'Tick {n} stops now and keeps what it did.',
  },
  stopIdle: { id: 'loops.stop.idle', defaultMessage: 'No tick runs after tick {n}.' },
  stopBeforeFirst: {
    id: 'loops.stop.beforeFirst',
    defaultMessage: 'It ends before its first tick.',
  },
  stopKeep: { id: 'loops.stop.keep', defaultMessage: 'Keep running' },

  nowLabel: { id: 'loops.panel.now', defaultMessage: 'Now' },
  ticksLabel: { id: 'loops.panel.ticks', defaultMessage: 'Ticks' },
  noTicks: { id: 'loops.panel.noTicks', defaultMessage: 'No tick has ended yet.' },
  unreadable: {
    id: 'loops.panel.unreadable',
    defaultMessage: 'The loop record could not be read: {error}',
  },
  statusUnreadable: {
    id: 'loops.panel.statusUnreadable',
    defaultMessage: "The loop's status could not be read: {error}",
  },
  emptyTitle: { id: 'loops.empty.title', defaultMessage: 'No loop in this chat.' },
  emptyBody: {
    id: 'loops.empty.body',
    defaultMessage:
      'A loop runs a goal again and again here, on a schedule or when goose decides. Start one from the Loop button, with {command}, or with "Loop this" on a message.',
  },

  chipProgress: { id: 'loops.tick.chip.progress', defaultMessage: 'Progress' },
  chipDone: { id: 'loops.tick.chip.done', defaultMessage: 'Done' },
  chipGoalMet: { id: 'loops.tick.chip.goalMet', defaultMessage: 'Goal met' },
  chipBlocked: { id: 'loops.tick.chip.blocked', defaultMessage: 'Blocked' },
  chipAsked: { id: 'loops.tick.chip.asked', defaultMessage: 'Asked you' },
  chipFailed: { id: 'loops.tick.chip.failed', defaultMessage: 'Failed' },
  chipNoReport: { id: 'loops.tick.chip.noReport', defaultMessage: 'No report' },
  chipStalled: { id: 'loops.tick.chip.stalled', defaultMessage: 'Stalled' },
  chipYielded: { id: 'loops.tick.chip.yielded', defaultMessage: 'Yielded' },
  chipStoppedByYou: { id: 'loops.tick.chip.stoppedByYou', defaultMessage: 'Stopped by you' },

  tickWhen: { id: 'loops.tick.when', defaultMessage: '{time} · {duration}' },
  tickYielded: { id: 'loops.tick.yielded', defaultMessage: 'Yielded to your turn in "{chat}"' },
  tickFailed: { id: 'loops.tick.failed', defaultMessage: 'Failed: {error}' },
  tickNoReport: { id: 'loops.tick.noReport', defaultMessage: 'Ended without a loop report' },
  tickLastWords: { id: 'loops.tick.lastWords', defaultMessage: 'Its last words: "{line}"' },
  tickAsked: { id: 'loops.tick.asked', defaultMessage: 'Asked you: "{question}"' },
  tickAnswered: { id: 'loops.tick.answered', defaultMessage: 'You answered: "{answer}"' },
  tickStoppedByYou: { id: 'loops.tick.stoppedByYou', defaultMessage: 'You stopped this tick.' },
  tickRemoved: {
    id: 'loops.tick.removed',
    defaultMessage: "This tick's messages were removed by an edit",
  },
  tickQuiet: {
    id: 'loops.tick.quiet',
    defaultMessage: '{time} · no write or edit outside the state file · {nextStep}',
  },
  tickFiles: { id: 'loops.tick.files', defaultMessage: 'Written or edited by goose' },
  tickNoFiles: {
    id: 'loops.tick.noFiles',
    defaultMessage: 'goose wrote or edited no file in this tick.',
  },
  tickCheckPassed: { id: 'loops.tick.checkPassed', defaultMessage: 'Check {command} passed' },
  tickCheckExited: {
    id: 'loops.tick.checkExited',
    defaultMessage: 'Check {command} exited {code}',
  },
  tickCheckExitedTail: {
    id: 'loops.tick.checkExitedTail',
    defaultMessage: 'Check {command} exited {code} — "{line}"',
  },
  tickCheckCouldNotRun: {
    id: 'loops.tick.checkCouldNotRun',
    defaultMessage: 'Check {command} could not run: {error}',
  },
  tickCheckRunning: { id: 'loops.tick.checkRunning', defaultMessage: 'Checking {command}…' },
  tickCheckNoExit: {
    id: 'loops.tick.checkNoExit',
    defaultMessage: 'Check {command} ended without an exit code',
  },
  openLog: { id: 'loops.tick.openLog', defaultMessage: 'Open log' },
  tickNext: { id: 'loops.tick.next', defaultMessage: 'Next: {step}' },
  tickSelfPaced: {
    id: 'loops.tick.selfPaced',
    defaultMessage: 'goose chose {interval}: "{reason}"',
  },
  tickSelfPacedNoReason: {
    id: 'loops.tick.selfPacedNoReason',
    defaultMessage: 'goose chose {interval} and gave no reason',
  },
  tickOn: { id: 'loops.tick.on', defaultMessage: 'on {node}' },
  tickTokens: { id: 'loops.tick.tokens', defaultMessage: '{tokens} tokens' },
  tickExpand: { id: 'loops.tick.expand', defaultMessage: 'Show tick {n}' },
  tickCollapse: { id: 'loops.tick.collapse', defaultMessage: 'Hide tick {n}' },

  markerTitle: { id: 'loops.marker.title', defaultMessage: 'Loop tick {n} · {time}' },
  markerTitleCadence: {
    id: 'loops.marker.titleCadence',
    defaultMessage: 'Loop tick {n} · {time} · {cadence}',
  },
  markerShowPrompt: { id: 'loops.marker.showPrompt', defaultMessage: 'Show prompt' },
  markerHidePrompt: { id: 'loops.marker.hidePrompt', defaultMessage: 'Hide prompt' },
  markerCopyPrompt: { id: 'loops.marker.copyPrompt', defaultMessage: 'Copy prompt' },
  markerCopied: { id: 'loops.marker.copied', defaultMessage: 'Copied' },
  markerYielded: {
    id: 'loops.marker.yielded',
    defaultMessage:
      'Stopped at {time} for your message in "{chat}" — the loop continues after your turn.',
  },
  loopThis: { id: 'loops.message.loopThis', defaultMessage: 'Loop this' },
  loopThisTitle: {
    id: 'loops.message.loopThisTitle',
    defaultMessage: 'Run this message again and again as a loop',
  },
});
