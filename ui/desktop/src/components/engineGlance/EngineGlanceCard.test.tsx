import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { EngineGlanceCard, type EngineGlanceCardProps } from './EngineGlanceCard';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';
import { attributeServing } from '../../utils/mlxServing';
import { toMlxDistributedReport } from '../../utils/mlxDistributedReport';
import {
  CHAT_ROW,
  SPLIT_READING_BODY,
  figure,
  glancePush,
  measuredRead,
  runningSnapshot,
} from '../../utils/engineGlance.fixtures';
import type { GlancePush } from '../../utils/engineGlance';
import { GENERATING_STATUS, IDLE_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';
import { FLASH_MODEL, FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';

const splitReading = glancePush(
  runningSnapshot(SPLIT_READING_BODY, {
    engine: 'distributed',
    modelId: FLASH_MODEL,
    serving: attributeServing([CHAT_ROW], 1, [], null),
    measured: measuredRead({ writing: figure(31.2, 29.0, 33.5, 6) }),
  }),
  { distributed: { report: toMlxDistributedReport(FLASH_READY), ageMs: 0 } }
);
const writing = glancePush(runningSnapshot(GENERATING_STATUS));
const idle = glancePush(
  runningSnapshot(IDLE_STATUS, { measured: measuredRead({ writing: figure(29.6, 29.6, 29.6, 1) }) })
);
const question = {
  sessionId: 'q1',
  sessionName: 'Deploy the site',
  question: 'Push to production or staging?',
};

function renderCard(push: GlancePush, over: Partial<EngineGlanceCardProps> = {}) {
  const props: EngineGlanceCardProps = {
    push,
    variant: 'dock',
    collapsed: false,
    expanded: false,
    onOpenEngine: vi.fn(),
    onOpenSession: vi.fn(),
    onToggleExpanded: vi.fn(),
    onCollapsedChange: vi.fn(),
    ...over,
  };
  render(
    <IntlTestWrapper>
      <EngineGlanceCard {...props} />
    </IntlTestWrapper>
  );
  return props;
}

describe('EngineGlanceCard — the Engine tile, small', () => {
  it('reading on the split: the owner’s card in fewer words — stage, mode, model, prompt, bar, rate, chat', () => {
    renderCard(splitReading);
    const card = screen.getByTestId('engine-glance');
    expect(card.dataset.phase).toBe('reading');
    expect(card.className).toContain('bg-lz-phase-reading');
    expect(screen.getByTestId('engine-glance-stage').textContent).toBe('Reading prompt');
    expect(screen.getByTestId('engine-glance-mode').textContent).toBe(
      'Split across 2 Macs · over Thunderbolt'
    );
    expect(screen.getByTestId('engine-glance-model').textContent).toBe(FLASH_MODEL);
    expect(screen.getByTestId('engine-glance-hero').textContent).toBe('80.3K');
    expect(screen.getByText('prompt tokens, reading for 3m 11s')).toBeTruthy();
    const bar = screen.getByTestId('engine-glance-progress');
    expect(bar.getAttribute('aria-valuenow')).toBe(String(Math.round((45200 / 80300) * 100)));
    expect(screen.getByTestId('engine-glance-second').textContent).toBe(
      '237 tok/s reading this prompt'
    );
    expect(screen.getByTestId('engine-glance-chat').textContent).toBe(
      'Chat · Refactor the auth flow'
    );
  });

  it('Q-185: goose’s fact check is named as that, by the name this window’s lists show', () => {
    const push: GlancePush = {
      ...splitReading,
      engine: {
        ...splitReading.engine,
        chat: { sessionId: CHAT_ROW.sessionId!, name: 'Refactor the auth flow', work: 'factCheck' },
      },
    };
    renderCard(push, { chatName: (_id, name) => `${name} · 5` });
    const chat = screen.getByTestId('engine-glance-chat');
    expect(chat.textContent).toBe('Checking the reply · Refactor the auth flow · 5');
    expect(chat.dataset.work).toBe('factCheck');
  });

  it('the ranges and each Mac’s memory wait behind More', () => {
    renderCard(splitReading);
    expect(screen.queryByTestId('engine-glance-details')).toBeNull();
    expect(screen.getByTestId('engine-glance-details-toggle')).toBeTruthy();
  });

  it('More open: the writing range, and one bar per Mac against its budget', () => {
    renderCard(splitReading, { expanded: true });
    const details = screen.getByTestId('engine-glance-details');
    expect(within(details).getByText('Writing 29.0–33.5 tok/s, middle half of runs')).toBeTruthy();
    const nodes = within(details).getAllByTestId('engine-glance-node');
    expect(nodes.map((n) => n.textContent)).toEqual([
      expect.stringContaining('MacBook Pro'),
      expect.stringContaining('workhorse'),
    ]);
  });

  it('a click on the card opens the Engine; the chat line opens that chat instead', () => {
    const props = renderCard(splitReading);
    fireEvent.click(screen.getByTestId('engine-glance-open'));
    expect(props.onOpenEngine).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId('engine-glance-chat'));
    expect(props.onOpenSession).toHaveBeenCalledWith(CHAT_ROW.sessionId);
    expect(props.onOpenEngine).toHaveBeenCalledOnce();
  });

  it('the click that ends a drag opens nothing', () => {
    const props = renderCard(writing, { variant: 'desktop', consumeDrag: () => true });
    fireEvent.click(screen.getByTestId('engine-glance-open'));
    expect(props.onOpenEngine).not.toHaveBeenCalled();
  });

  it('idle: the grey card, the quiet median — never presented as work', () => {
    renderCard(idle);
    const card = screen.getByTestId('engine-glance');
    expect(card.dataset.stage).toBe('idle');
    expect(card.className).toContain('bg-lz-phase-idle');
    expect(screen.getByTestId('engine-glance-hero').className).not.toContain('text-[28px]');
    expect(screen.queryByTestId('engine-glance-progress')).toBeNull();
  });

  it('a failed engine says why, in its own words', () => {
    renderCard(
      glancePush({ ...INITIAL_SNAPSHOT, mode: 'failed', failedError: 'out of memory at layer 41' })
    );
    expect(screen.getByTestId('engine-glance').className).toContain('bg-lz-phase-failed');
    expect(screen.getByTestId('engine-glance-detail').textContent).toBe(
      'out of memory at layer 41'
    );
  });

  it('a question waiting rides the card as a solid strip that opens that chat', () => {
    const props = renderCard({ ...writing, sessions: { running: 1, needsYou: [question] } });
    const strip = screen.getByTestId('engine-glance-needs-you');
    expect(strip.className).toContain('bg-lz-warn-solid');
    expect(strip.textContent).toContain('1 needs you');
    fireEvent.click(strip);
    expect(props.onOpenSession).toHaveBeenCalledWith('q1');
  });

  it('no engine, only a question: the card IS the question, and a click opens its chat', () => {
    const props = renderCard({
      ...glancePush({ ...INITIAL_SNAPSHOT, mode: 'off' }),
      sessions: { running: 0, needsYou: [question] },
    });
    expect(screen.getByTestId('engine-glance').dataset.stage).toBe('needs-you');
    expect(screen.getByTestId('engine-glance-question').textContent).toContain(
      'Push to production or staging?'
    );
    expect(screen.queryByTestId('engine-glance-mode')).toBeNull();
    fireEvent.click(screen.getByTestId('engine-glance-open'));
    expect(props.onOpenSession).toHaveBeenCalledWith('q1');
    expect(props.onOpenEngine).not.toHaveBeenCalled();
  });

  it('the docked card has no pill control; the floating ones do, and the desktop one can close', () => {
    renderCard(writing);
    expect(screen.queryByTestId('engine-glance-collapse')).toBeNull();
    expect(screen.queryByTestId('engine-glance-close')).toBeNull();
  });

  it('desktop: shrink to a pill, and close', () => {
    const onClose = vi.fn();
    const props = renderCard(writing, { variant: 'desktop', onClose });
    fireEvent.click(screen.getByTestId('engine-glance-collapse'));
    expect(props.onCollapsedChange).toHaveBeenCalledWith(true);
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    expect(onClose).toHaveBeenCalledOnce();
    expect(props.onOpenEngine).not.toHaveBeenCalled();
  });
});

describe('EngineGlanceCard — the pill', () => {
  it('writing: the stage and its live rate, in the phase colour', () => {
    renderCard(writing, { variant: 'desktop', collapsed: true });
    const pill = screen.getByTestId('engine-glance');
    expect(pill.dataset.collapsed).toBe('true');
    expect(pill.className).toContain('bg-lz-phase-writing');
    expect(pill.textContent).toContain('Writing');
    expect(screen.getByTestId('engine-glance-pill-figure').textContent).toBe('19.9 tok/s');
  });

  it('reading on the split: its reading rate', () => {
    renderCard(splitReading, { variant: 'float', collapsed: true });
    expect(screen.getByTestId('engine-glance-pill-figure').textContent).toBe('237 tok/s');
  });

  it('idle: the word alone — a median is not a live figure', () => {
    renderCard(idle, { variant: 'float', collapsed: true });
    expect(screen.queryByTestId('engine-glance-pill-figure')).toBeNull();
  });

  it('a question waiting shows as a count on the pill; expand brings the card back', () => {
    const props = renderCard(
      { ...writing, sessions: { running: 1, needsYou: [question] } },
      { variant: 'float', collapsed: true }
    );
    expect(screen.getByTestId('engine-glance-pill-needs').textContent).toBe('1 needs you');
    fireEvent.click(screen.getByTestId('engine-glance-expand'));
    expect(props.onCollapsedChange).toHaveBeenCalledWith(false);
  });
});
