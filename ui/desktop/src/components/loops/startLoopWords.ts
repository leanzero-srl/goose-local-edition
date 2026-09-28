import { defineMessages } from '../../i18n';

/**
 * The words of the composer's loop controls and the `/loop` reply line (DESIGN-SESSION-LOOPS §7.1,
 * §7.2, §8.1). The reply lines render EXACTLY the English goosed's `/loop` handler says
 * (`execute_commands.rs`), so a control answers the same words whichever door it went through.
 */
export const composerWords = defineMessages({
  loopButton: { id: 'loops.composer.loop', defaultMessage: 'Loop' },
  loopButtonTitle: {
    id: 'loops.composer.loopTitle',
    defaultMessage: "Run this chat's goal again and again — each run is a tick",
  },
  loopButtonSwarmBuild: {
    id: 'loops.composer.swarmBuild',
    defaultMessage:
      'Loops run chat turns. This chat builds with the swarm — every tick would start a full build. Use Agent Work for recurring builds.',
  },
  placeholderTickRunning: {
    id: 'loops.composer.placeholderTickRunning',
    defaultMessage: 'Tick {n} is running — what you send waits for it. Use Send now to steer it.',
  },
  placeholderTickElsewhere: {
    id: 'loops.composer.placeholderTickElsewhere',
    defaultMessage: 'Tick {n} is finishing in the background — you can send when it ends',
  },
  stopTick: { id: 'loops.composer.stopTick', defaultMessage: 'Stop tick {n} — the loop pauses' },
  replyLabel: { id: 'loops.composer.replyLabel', defaultMessage: 'The loop answered' },
  replyDismiss: { id: 'loops.composer.replyDismiss', defaultMessage: 'Dismiss' },
  replyNoLoop: {
    id: 'loops.reply.noLoop',
    defaultMessage: 'No loop in this chat. Use {command} or the Loop button.',
  },
  replyStatus: { id: 'loops.reply.status', defaultMessage: 'Loop: {goal} · {sentence}' },
  replyStatusTick: {
    id: 'loops.reply.statusTick',
    defaultMessage: 'Loop: {goal} · {sentence} · tick {n}',
  },
  replyStatusUnreadable: {
    id: 'loops.reply.statusUnreadable',
    defaultMessage: 'its status could not be read: {error}',
  },
  replyPausedAfter: {
    id: 'loops.reply.pausedAfter',
    defaultMessage: 'Loop paused after tick {n}.',
  },
  replyPausedBefore: {
    id: 'loops.reply.pausedBefore',
    defaultMessage: 'Loop paused before its first tick.',
  },
  replyStoppedAfter: {
    id: 'loops.reply.stoppedAfter',
    defaultMessage: 'Loop stopped after tick {n}.',
  },
  replyStoppedBefore: {
    id: 'loops.reply.stoppedBefore',
    defaultMessage: 'Loop stopped before its first tick.',
  },
  replyTickNow: { id: 'loops.reply.tickNow', defaultMessage: 'Tick {n} starts now.' },
  replyResumedAt: {
    id: 'loops.reply.resumedAt',
    defaultMessage: 'Loop resumed — next tick {time}.',
  },
  replyResumedUnreadable: {
    id: 'loops.reply.resumedUnreadable',
    defaultMessage: 'Loop resumed. Its next tick could not be read: {error}',
  },
  replyResumedSentence: {
    id: 'loops.reply.resumedSentence',
    defaultMessage: 'Loop resumed. {sentence}',
  },
  replyNeither: {
    id: 'loops.reply.neither',
    defaultMessage: 'The loop runner answered with neither a loop nor a refusal.',
  },
});

/** The queue row of a message typed while a tick runs (§8.1): the tick it would steer. */
export const queueWords = defineMessages({
  queuedSteers: {
    id: 'loops.queue.queuedSteers',
    defaultMessage: 'Queued · Send now steers tick {n}',
  },
  sendNowSteers: {
    id: 'loops.queue.sendNowSteers',
    defaultMessage: 'Send now — this steers tick {n}',
  },
});

/** The Start dialog (§8.2): every label, help line, state and refusal. */
export const startWords = defineMessages({
  title: { id: 'loops.start.title', defaultMessage: 'Loop this chat' },
  titleEdit: { id: 'loops.start.titleEdit', defaultMessage: 'Edit loop' },
  close: { id: 'loops.start.close', defaultMessage: 'Close' },
  intro: {
    id: 'loops.start.intro',
    defaultMessage:
      'goose runs your goal again and again in this chat — each run is a tick. Every tick reads its state file, works, proves what it did, and says what comes next. You can steer, pause or stop it any time.',
  },
  startFrom: { id: 'loops.start.startFrom', defaultMessage: 'Start from' },
  templateQuality: { id: 'loops.start.template.quality', defaultMessage: 'Software quality loop' },
  templateUntilCheck: {
    id: 'loops.start.template.untilCheck',
    defaultMessage: 'Until a check passes',
  },
  templateWatch: { id: 'loops.start.template.watch', defaultMessage: 'Watch and act' },
  templateBlank: { id: 'loops.start.template.blank', defaultMessage: 'Blank' },
  descQuality: {
    id: 'loops.start.template.qualityDescription',
    defaultMessage:
      "Discover what's broken, pick the one thing that matters most, fix it, prove it with a check. Repeat.",
  },
  descUntilCheck: {
    id: 'loops.start.template.untilCheckDescription',
    defaultMessage: 'Keep working until a command you name succeeds — tests, a build, a lint.',
  },
  descWatch: {
    id: 'loops.start.template.watchDescription',
    defaultMessage:
      'Look at something on a schedule and act when it changes — a build, a deploy, a folder.',
  },
  descBlank: {
    id: 'loops.start.template.blankDescription',
    defaultMessage: 'Your goal, your steps.',
  },
  templatesLoading: {
    id: 'loops.start.templatesLoading',
    defaultMessage: 'Reading the templates…',
  },
  templatesFailed: {
    id: 'loops.start.templatesFailed',
    defaultMessage: 'The templates could not be read: {error}',
  },
  retry: { id: 'loops.start.retry', defaultMessage: 'Try again' },
  goal: { id: 'loops.start.goal', defaultMessage: 'Goal' },
  goalPlaceholder: {
    id: 'loops.start.goalPlaceholder',
    defaultMessage:
      'What should every tick move forward? e.g. Make every test in ui/desktop pass without changing the tests',
  },
  goalEmpty: { id: 'loops.start.goalEmpty', defaultMessage: 'Say what the loop should do.' },
  steps: { id: 'loops.start.steps', defaultMessage: 'What each tick does' },
  resetSteps: { id: 'loops.start.resetSteps', defaultMessage: 'Reset steps' },
  stepsBlankPlaceholder: {
    id: 'loops.start.stepsBlankPlaceholder',
    defaultMessage:
      'No steps — every tick works from your goal alone. Write steps here if you want some.',
  },
  stepsPreview: { id: 'loops.start.stepsPreview', defaultMessage: 'What goose reads each tick' },
  stepsPreviewHelp: {
    id: 'loops.start.stepsPreviewHelp',
    defaultMessage:
      'The highlighted parts are facts of this loop, filled in again at every tick — change the check or the state file and the steps follow.',
  },
  slotTitle: { id: 'loops.start.slotTitle', defaultMessage: 'A fact of this loop: {name}' },
  slotUnknown: {
    id: 'loops.start.slotUnknown',
    defaultMessage: '{slot} is not a fact goose knows',
  },
  goalFirstLineEmpty: {
    id: 'loops.start.goalFirstLineEmpty',
    defaultMessage: 'the first line of your goal',
  },
  check: { id: 'loops.start.check', defaultMessage: 'How goose proves it' },
  command: { id: 'loops.start.command', defaultMessage: 'Command' },
  checkPlaceholder: { id: 'loops.start.checkPlaceholder', defaultMessage: 'pnpm test' },
  checkHelp: {
    id: 'loops.start.checkHelp',
    defaultMessage:
      "Runs in {dir} after every tick. When it succeeds after goose says it's done, the loop ends.",
  },
  checkRequired: {
    id: 'loops.start.checkRequired',
    defaultMessage: 'This template needs a command to check.',
  },
  cadence: { id: 'loops.start.cadence', defaultMessage: 'When the next tick runs' },
  cadenceEvery: { id: 'loops.start.cadenceEvery', defaultMessage: 'Every' },
  cadenceSelfPaced: { id: 'loops.start.cadenceSelfPaced', defaultMessage: 'When goose decides' },
  cadenceBackToBack: {
    id: 'loops.start.cadenceBackToBack',
    defaultMessage: 'Right after each tick',
  },
  presets: { id: 'loops.start.presets', defaultMessage: 'How often' },
  presetCustom: { id: 'loops.start.presetCustom', defaultMessage: 'custom' },
  customEvery: { id: 'loops.start.customEvery', defaultMessage: 'Custom interval' },
  customHelp: {
    id: 'loops.start.customHelp',
    defaultMessage: 'custom: a number and s, m or h (90m, 2h)',
  },
  cadenceInvalid: {
    id: 'loops.start.cadenceInvalid',
    defaultMessage: 'Use a number and s, m or h — 90m, 2h',
  },
  selfPacedHelp: {
    id: 'loops.start.selfPacedHelp',
    defaultMessage:
      "After each tick goose names when to come back and why — you'll see its reason.",
  },
  backToBackHelp: {
    id: 'loops.start.backToBackHelp',
    defaultMessage: 'The next tick starts as soon as one ends.',
  },
  stateFile: { id: 'loops.start.stateFile', defaultMessage: 'State file' },
  stateFileHelp: {
    id: 'loops.start.stateFileHelp',
    defaultMessage:
      'goose reads this first and rewrites it last, so the loop survives long chats and compaction. You can edit it too.',
  },
  stateFileOutside: {
    id: 'loops.start.stateFileOutside',
    defaultMessage: 'Keep the state file inside {dir}.',
  },
  stateFileEmpty: {
    id: 'loops.start.stateFileEmpty',
    defaultMessage: 'Name the state file — goose reads it first and rewrites it last.',
  },
  stopWhen: { id: 'loops.start.stopWhen', defaultMessage: 'Stop when' },
  stopCheck: {
    id: 'loops.start.stopCheck',
    defaultMessage: 'the check succeeds after goose reports done',
  },
  stopDone: { id: 'loops.start.stopDone', defaultMessage: 'goose reports the goal is done' },
  stopYou: { id: 'loops.start.stopYou', defaultMessage: 'you stop it' },
  stopAfter: { id: 'loops.start.stopAfter', defaultMessage: 'after {field} ticks' },
  stopAfterField: {
    id: 'loops.start.stopAfterField',
    defaultMessage: 'Stop after this many ticks',
  },
  stopAfterHelp: {
    id: 'loops.start.stopAfterHelp',
    defaultMessage: 'Leave empty to run until the goal is met or you stop it.',
  },
  stopAfterInvalid: {
    id: 'loops.start.stopAfterInvalid',
    defaultMessage: 'Use a whole number of ticks, 1 or more.',
  },
  stopAfterZero: {
    id: 'loops.start.stopAfterZero',
    defaultMessage:
      'Stop after needs at least one tick — leave it empty to run until the goal is met or you stop it.',
  },
  costLine: {
    id: 'loops.start.costLine',
    defaultMessage:
      'Each tick is one turn on {served}. Your turns in this window always go first — a tick waits or pauses while you chat.',
  },
  costLineNoModel: {
    id: 'loops.start.costLineNoModel',
    defaultMessage:
      'Each tick is one turn in this chat. Your turns in this window always go first — a tick waits or pauses while you chat.',
  },
  swapLine: {
    id: 'loops.start.swapLine',
    defaultMessage: 'Each tick may load {node} and stop {way}.',
  },
  sleepLine: {
    id: 'loops.start.sleepLine',
    defaultMessage: 'Your Mac may sleep; ticks wait until it wakes.',
  },
  keepAwake: {
    id: 'loops.start.keepAwake',
    defaultMessage: 'Keep this Mac awake while goose is open',
  },
  keepAwakeHelp: {
    id: 'loops.start.keepAwakeHelp',
    defaultMessage: "This is the app's Prevent Sleep setting — it changes now, for every chat.",
  },
  keepAwakeFailed: {
    id: 'loops.start.keepAwakeFailed',
    defaultMessage: 'Keep awake is not working: {reason}',
  },
  cancel: { id: 'loops.start.cancel', defaultMessage: 'Cancel' },
  start: { id: 'loops.start.start', defaultMessage: 'Start loop' },
  save: { id: 'loops.start.save', defaultMessage: 'Save' },
  firstTickNow: { id: 'loops.start.firstTickNow', defaultMessage: 'The first tick runs now.' },
  swarmBuild: {
    id: 'loops.start.swarmBuild',
    defaultMessage:
      'Loops run chat turns. This chat builds with the swarm, so every tick would start a full build. Use Agent Work for recurring builds.',
  },
  replaceTitle: { id: 'loops.start.replaceTitle', defaultMessage: 'Replace the loop?' },
  replaceBody: {
    id: 'loops.start.replaceBody',
    defaultMessage: 'The current loop ends after tick {n}.',
  },
  replaceBodyNoTick: {
    id: 'loops.start.replaceBodyNoTick',
    defaultMessage: 'The current loop ends before its first tick.',
  },
  replaceBodyUnreadable: {
    id: 'loops.start.replaceBodyUnreadable',
    defaultMessage: 'The current loop could not be read — starting a new one replaces it.',
  },
  replaceKeep: { id: 'loops.start.replaceKeep', defaultMessage: 'Keep it' },
  replace: { id: 'loops.start.replace', defaultMessage: 'Replace' },
});
