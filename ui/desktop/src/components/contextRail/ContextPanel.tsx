import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import type { KeptPillarDto, KeptPillarId } from '@aaif/goose-sdk';
import { Archive, BookMarked, Pin, Plus, ScrollText, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { compactionPreview, compactCommand, type CompactionPreview } from '../../acp/compaction';
import { errorMessage } from '../../utils/conversionUtils';
import {
  Button,
  Chip,
  Disclosure,
  FOCUS,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { compactionWords } from '../compaction/compactionWords';
import { NOTE_FIELD, NoteFields } from '../compaction/NoteEditor';
import { useCompactionSteer } from '../compaction/useCompactionSteer';
import { onCompactionSteerChanged } from './contextRailRequest';

export const contextRailWords = defineMessages({
  tab: { id: 'contextRail.tab', defaultMessage: 'Context' },
  tabsLabel: { id: 'contextRail.tabsLabel', defaultMessage: 'Loop, changes and context' },
  close: { id: 'contextRail.close', defaultMessage: 'Close context' },
  keptTitle: { id: 'contextRail.keptTitle', defaultMessage: 'Kept word for word by goose' },
  keptWhy: {
    id: 'contextRail.keptWhy',
    defaultMessage:
      'A compaction keeps these exactly as they are — built by goose from the whole chat, never rewritten by the model.',
  },
  budget: {
    id: 'contextRail.budget',
    defaultMessage: 'Kept within {tokens} tokens — a sixteenth of the window',
  },
  budgetUnknown: {
    id: 'contextRail.budgetUnknown',
    defaultMessage: 'The window is unknown, so nothing is cut',
  },
  asked: { id: 'contextRail.asked', defaultMessage: 'What you asked' },
  files: { id: 'contextRail.files', defaultMessage: 'Files written' },
  failed: { id: 'contextRail.failed', defaultMessage: 'Tool calls that failed' },
  notes: { id: 'contextRail.notes', defaultMessage: 'Your note and pins' },
  ledger: { id: 'contextRail.ledger', defaultMessage: 'Ledger' },
  tokens: { id: 'contextRail.tokens', defaultMessage: '{tokens} tokens' },
  count: {
    id: 'contextRail.count',
    defaultMessage: '{count, plural, one {# item} other {# items}}',
  },
  nothing: { id: 'contextRail.nothing', defaultMessage: 'Nothing yet' },
  leftOut: {
    id: 'contextRail.leftOut',
    defaultMessage:
      '{count, plural, one {# older one is} other {# older ones are}} not kept here to fit',
  },
  cut: {
    id: 'contextRail.cut',
    defaultMessage: '{count, plural, one {# is} other {# are}} cut to their start to fit',
  },
  unreadable: { id: 'contextRail.unreadable', defaultMessage: 'Could not be read: {error}' },
  writesTitle: { id: 'contextRail.writesTitle', defaultMessage: 'goose writes' },
  writesWhy: {
    id: 'contextRail.writesWhy',
    defaultMessage:
      'What only the model knows, written at each compaction from the conversation itself:',
  },
  lastCompaction: {
    id: 'contextRail.lastCompaction',
    defaultMessage: 'Last compaction: {before} → {after} tokens · {elapsed} · {when}',
  },
  lastCompactionNoSize: {
    id: 'contextRail.lastCompactionNoSize',
    defaultMessage: 'Last compaction: {elapsed} · {when}',
  },
  neverCompacted: {
    id: 'contextRail.neverCompacted',
    defaultMessage: 'This chat has not been compacted yet.',
  },
  alwaysTitle: { id: 'contextRail.alwaysTitle', defaultMessage: 'Always here' },
  alwaysWhy: {
    id: 'contextRail.alwaysWhy',
    defaultMessage: 'In every turn already, compacted or not.',
  },
  scratchpad: { id: 'contextRail.scratchpad', defaultMessage: 'Scratchpad' },
  noScratchpad: { id: 'contextRail.noScratchpad', defaultMessage: 'The scratchpad is empty.' },
  ledgerTail: { id: 'contextRail.ledgerTail', defaultMessage: 'Newest ledger entries' },
  noLedger: { id: 'contextRail.noLedger', defaultMessage: 'The ledger has no entries.' },
  noteTitle: { id: 'contextRail.noteTitle', defaultMessage: 'Your note' },
  saveNote: { id: 'contextRail.saveNote', defaultMessage: 'Save note' },
  pinnedTitle: { id: 'contextRail.pinnedTitle', defaultMessage: 'Pinned' },
  pinnedWhy: {
    id: 'contextRail.pinnedWhy',
    defaultMessage: 'Kept word for word in every compaction of this chat.',
  },
  pinPlaceholder: {
    id: 'contextRail.pinPlaceholder',
    defaultMessage: 'A line that must always survive',
  },
  addPin: { id: 'contextRail.addPin', defaultMessage: 'Pin' },
  removePin: { id: 'contextRail.removePin', defaultMessage: 'Unpin “{pin}”' },
  noPins: { id: 'contextRail.noPins', defaultMessage: 'Nothing pinned.' },
  lastKeptTitle: { id: 'contextRail.lastKeptTitle', defaultMessage: 'Last compaction kept' },
  lastKeptNone: {
    id: 'contextRail.lastKeptNone',
    defaultMessage: 'No compaction has run in this chat.',
  },
  loadFailed: {
    id: 'contextRail.loadFailed',
    defaultMessage: 'Couldn’t read what a compaction would keep: {error}',
  },
  loading: { id: 'contextRail.loading', defaultMessage: 'Reading this chat…' },
});

const w = contextRailWords;

const PILLAR_TITLE: Record<KeptPillarId, (typeof w)[keyof typeof w]> = {
  asked: w.asked,
  files: w.files,
  failed: w.failed,
  notes: w.notes,
  ledger: w.ledger,
};

type Preview =
  | { kind: 'loading' }
  | { kind: 'ready'; preview: CompactionPreview }
  | { kind: 'failed'; error: string };

function Section({
  title,
  why,
  icon,
  children,
  testId,
}: {
  title: string;
  why?: string;
  icon: ReactNode;
  children: ReactNode;
  testId: string;
}) {
  return (
    <section data-testid={testId} className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <span className="text-lz-accent [&_svg]:size-4">{icon}</span>
        <h3 className={TYPE.zone}>{title}</h3>
      </div>
      {why && <p className={TYPE.meta}>{why}</p>}
      {children}
    </section>
  );
}

function PillarRow({ pillar }: { pillar: KeptPillarDto }) {
  const intl = useIntl();
  const compact = (n: number) =>
    intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });
  const empty = pillar.items.length === 0 && !pillar.error && !pillar.leftOut;
  return (
    <Disclosure
      testId={`context-pillar-${pillar.id}`}
      title={intl.formatMessage(PILLAR_TITLE[pillar.id])}
      meta={
        <>
          <Chip>{intl.formatMessage(w.count, { count: pillar.items.length })}</Chip>
          {pillar.tokens != null && !empty && (
            <Chip tone="accent">
              {intl.formatMessage(w.tokens, { tokens: compact(pillar.tokens) })}
            </Chip>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-2">
        {pillar.error && (
          <p className={cx(TYPE.meta, TONE_TEXT.err)}>
            {intl.formatMessage(w.unreadable, { error: pillar.error })}
          </p>
        )}
        {empty && <p className={TYPE.meta}>{intl.formatMessage(w.nothing)}</p>}
        {(pillar.leftOut ?? 0) > 0 && (
          <p className={cx(TYPE.meta, TONE_TEXT.warn)}>
            {intl.formatMessage(w.leftOut, { count: pillar.leftOut })}
          </p>
        )}
        {(pillar.cut ?? 0) > 0 && (
          <p className={cx(TYPE.meta, TONE_TEXT.warn)}>
            {intl.formatMessage(w.cut, { count: pillar.cut })}
          </p>
        )}
        <ol className="flex flex-col gap-1.5">
          {pillar.items.map((item, i) => (
            <li
              key={i}
              className={cx(
                'whitespace-pre-wrap break-words px-2 py-1.5 text-[12px] leading-snug text-lz-ink',
                SURFACE.inset,
                RADIUS.control
              )}
            >
              {item}
            </li>
          ))}
        </ol>
      </div>
    </Disclosure>
  );
}

/**
 * The Context tab of the chat's rail (Q-357): what a compaction of this chat keeps and why, before
 * one runs. "Kept word for word by goose" is P1–P5 as the next compaction would build them now,
 * with their sizes; "goose writes" is the one part the model writes; "Always here" rides every turn
 * already; the note and the pins are the person's to set; the last compaction's stored summary is
 * one click away. All of it from goosed's preview — code only, no model call — re-read when the
 * chat changes or a compaction ends.
 */
export function ContextPanel({
  sessionId,
  refreshKey,
  onSend,
}: {
  sessionId: string;
  refreshKey: string;
  onSend?: (text: string) => void;
}) {
  const intl = useIntl();
  const [state, setState] = useState<Preview>({ kind: 'loading' });
  const asked = useRef(0);
  const steer = useCompactionSteer(sessionId);
  const [noteDraft, setNoteDraft] = useState<string | null>(null);
  const [standingDraft, setStandingDraft] = useState<boolean | null>(null);
  const [pinDraft, setPinDraft] = useState('');

  const read = useCallback(async () => {
    const mine = ++asked.current;
    try {
      const preview = await compactionPreview(sessionId);
      if (mine === asked.current) setState({ kind: 'ready', preview });
    } catch (error) {
      if (mine === asked.current) setState({ kind: 'failed', error: errorMessage(error) });
    }
  }, [sessionId]);

  useEffect(() => {
    void read();
  }, [read, refreshKey]);
  useEffect(
    () =>
      onCompactionSteerChanged((changed) => {
        if (changed === sessionId) void read();
      }),
    [read, sessionId]
  );

  const saved = steer.state.kind === 'ready' ? steer.state.steer : null;
  const note = noteDraft ?? saved?.note ?? '';
  const standing = standingDraft ?? saved?.standing ?? false;
  const pins = saved?.pins ?? [];
  const compact = (n: number) =>
    intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });

  if (state.kind === 'loading') {
    return <p className={cx(TYPE.meta, 'p-3')}>{intl.formatMessage(w.loading)}</p>;
  }
  if (state.kind === 'failed') {
    return (
      <p className={cx(TYPE.body, TONE_TEXT.err, 'p-3')} data-testid="context-rail-failed">
        {intl.formatMessage(w.loadFailed, { error: state.error })}
      </p>
    );
  }
  const { preview } = state;
  const last = preview.last;

  return (
    <div
      data-testid="context-rail-panel"
      className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-3"
    >
      <Section
        testId="context-rail-kept"
        title={intl.formatMessage(w.keptTitle)}
        why={intl.formatMessage(w.keptWhy)}
        icon={<Archive />}
      >
        <p className={cx(TYPE.meta, TNUM)}>
          {preview.keptBudgetTokens != null
            ? intl.formatMessage(w.budget, { tokens: compact(preview.keptBudgetTokens) })
            : intl.formatMessage(w.budgetUnknown)}
        </p>
        <div className="flex flex-col gap-2">
          {preview.kept.map((pillar) => (
            <PillarRow key={pillar.id} pillar={pillar} />
          ))}
        </div>
      </Section>

      <Section
        testId="context-rail-writes"
        title={intl.formatMessage(w.writesTitle)}
        why={intl.formatMessage(w.writesWhy)}
        icon={<ScrollText />}
      >
        <ol className="flex flex-col gap-1.5">
          {preview.writtenParts.map((part) => (
            <li key={part.heading} className="flex flex-col gap-0.5">
              <span className={cx(TYPE.body, WEIGHT.semibold)}>{part.heading}</span>
              <span className={TYPE.meta}>{part.ask}</span>
            </li>
          ))}
        </ol>
        <p className={cx(TYPE.meta, TNUM)} data-testid="context-rail-last">
          {last
            ? last.tokensBefore != null && last.tokensAfter != null
              ? intl.formatMessage(w.lastCompaction, {
                  before: compact(last.tokensBefore),
                  after: compact(last.tokensAfter),
                  elapsed: formatElapsed(last.elapsedMs / 1000),
                  when: intl.formatDate(last.at, { dateStyle: 'medium', timeStyle: 'short' }),
                })
              : intl.formatMessage(w.lastCompactionNoSize, {
                  elapsed: formatElapsed(last.elapsedMs / 1000),
                  when: intl.formatDate(last.at, { dateStyle: 'medium', timeStyle: 'short' }),
                })
            : intl.formatMessage(w.neverCompacted)}
        </p>
      </Section>

      <Section
        testId="context-rail-always"
        title={intl.formatMessage(w.alwaysTitle)}
        why={intl.formatMessage(w.alwaysWhy)}
        icon={<BookMarked />}
      >
        <span className={cx(TYPE.meta, WEIGHT.semibold, 'text-lz-ink-2')}>
          {intl.formatMessage(w.scratchpad)}
        </span>
        {preview.alwaysHere.scratchpad ? (
          <pre
            className={cx(
              'whitespace-pre-wrap break-words px-2 py-1.5 font-mono text-[12px] text-lz-ink',
              SURFACE.inset,
              RADIUS.control
            )}
          >
            {preview.alwaysHere.scratchpad}
          </pre>
        ) : (
          <p className={TYPE.meta}>{intl.formatMessage(w.noScratchpad)}</p>
        )}
        <span className={cx(TYPE.meta, WEIGHT.semibold, 'text-lz-ink-2')}>
          {intl.formatMessage(w.ledgerTail)}
        </span>
        {(preview.alwaysHere.ledgerTail ?? []).length > 0 ? (
          <ul className="flex flex-col gap-1">
            {(preview.alwaysHere.ledgerTail ?? []).map((entry, i) => (
              <li key={i} className="text-[12px] leading-snug text-lz-ink">
                {entry}
              </li>
            ))}
          </ul>
        ) : (
          <p className={TYPE.meta}>{intl.formatMessage(w.noLedger)}</p>
        )}
      </Section>

      <Section
        testId="context-rail-note"
        title={intl.formatMessage(w.noteTitle)}
        icon={<ScrollText />}
      >
        {steer.state.kind === 'unreadable' ? (
          <p className={cx(TYPE.meta, TONE_TEXT.err)}>
            {intl.formatMessage(compactionWords.noteReadFailed, { error: steer.state.error })}
          </p>
        ) : (
          <>
            <NoteFields
              note={note}
              standing={standing}
              onNote={setNoteDraft}
              onStanding={setStandingDraft}
              testIdPrefix="context-rail"
            />
            <div className="flex flex-wrap gap-2">
              <Button
                size="sm"
                variant="secondary"
                data-testid="context-rail-save-note"
                disabled={noteDraft === null && standingDraft === null}
                onClick={async () => {
                  if (await steer.save({ note, standing, followAsWritten: false })) {
                    setNoteDraft(null);
                    setStandingDraft(null);
                  }
                }}
              >
                {intl.formatMessage(w.saveNote)}
              </Button>
              {onSend && (
                <Button
                  size="sm"
                  variant="primary"
                  data-testid="context-rail-compact"
                  onClick={async () => {
                    if (noteDraft !== null || standingDraft !== null) {
                      if (!(await steer.save({ note, standing, followAsWritten: false }))) return;
                      setNoteDraft(null);
                      setStandingDraft(null);
                    }
                    onSend(compactCommand());
                  }}
                >
                  {intl.formatMessage(compactionWords.compactNow)}
                </Button>
              )}
            </div>
          </>
        )}
        {steer.saveError && (
          <p className={cx(TYPE.meta, TONE_TEXT.err)}>
            {intl.formatMessage(compactionWords.noteSaveFailed, { error: steer.saveError })}
          </p>
        )}
      </Section>

      <Section
        testId="context-rail-pins"
        title={intl.formatMessage(w.pinnedTitle)}
        why={intl.formatMessage(w.pinnedWhy)}
        icon={<Pin />}
      >
        {pins.length > 0 ? (
          <ul className="flex flex-col gap-1.5">
            {pins.map((pin) => (
              <li
                key={pin}
                className={cx('flex items-start gap-2 px-2 py-1.5', SURFACE.inset, RADIUS.control)}
              >
                <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-[12px] text-lz-ink">
                  {pin}
                </span>
                <button
                  type="button"
                  aria-label={intl.formatMessage(w.removePin, { pin })}
                  data-testid="context-rail-unpin"
                  onClick={() => void steer.save({ pins: pins.filter((p) => p !== pin) })}
                  className={cx(
                    'inline-flex size-6 shrink-0 items-center justify-center rounded-lz-control text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink',
                    FOCUS
                  )}
                >
                  <X aria-hidden className="size-3.5" />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className={TYPE.meta}>{intl.formatMessage(w.noPins)}</p>
        )}
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            const pin = pinDraft.trim();
            if (!pin || pins.includes(pin)) return;
            if (await steer.save({ pins: [...pins, pin] })) setPinDraft('');
          }}
        >
          <input
            data-testid="context-rail-pin-input"
            value={pinDraft}
            onChange={(e) => setPinDraft(e.target.value)}
            placeholder={intl.formatMessage(w.pinPlaceholder)}
            className={cx(NOTE_FIELD, 'resize-none')}
          />
          <Button
            type="submit"
            size="sm"
            variant="secondary"
            icon={<Plus />}
            disabled={!pinDraft.trim()}
            data-testid="context-rail-add-pin"
          >
            {intl.formatMessage(w.addPin)}
          </Button>
        </form>
      </Section>

      <Section
        testId="context-rail-last-kept"
        title={intl.formatMessage(w.lastKeptTitle)}
        icon={<Archive />}
      >
        {preview.lastKept ? (
          <Disclosure
            title={intl.formatMessage(w.lastKeptTitle)}
            testId="context-rail-last-kept-body"
          >
            <pre className="max-h-96 overflow-y-auto whitespace-pre-wrap break-words font-mono text-[12px] leading-snug text-lz-ink">
              {preview.lastKept}
            </pre>
          </Disclosure>
        ) : (
          <p className={TYPE.meta}>{intl.formatMessage(w.lastKeptNone)}</p>
        )}
      </Section>
    </div>
  );
}
