import { describe, expect, it, vi } from 'vitest';
import type { FormingStatus } from '@aaif/goose-sdk';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { EngineGlanceCard, type EngineGlanceCardProps } from './EngineGlanceCard';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';
import { attributeServing } from '../../utils/mlxServing';
import { toMlxDistributedReport } from '../../utils/mlxDistributedReport';
import { assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  CHAT_ROW,
  GLANCE_MODEL,
  SPLIT_READING_BODY,
  TOOL_LABEL_ROW,
  TURN_BESIDE_SIDE_CALL_BODY,
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
const sideCall = glancePush(
  runningSnapshot(TURN_BESIDE_SIDE_CALL_BODY, {
    engine: 'distributed',
    modelId: FLASH_MODEL,
    serving: attributeServing([CHAT_ROW, TOOL_LABEL_ROW], 2, [], null),
  }),
  { distributed: { report: toMlxDistributedReport(FLASH_READY), ageMs: 0 } }
);
const turnWriting = glancePush(
  runningSnapshot(GENERATING_STATUS, { serving: attributeServing([CHAT_ROW], 3, [], null) })
);
const forming: FormingStatus = {
  calls: [
    { name: 'developer__text_editor', title: 'edit', argumentChars: 1204 },
    { name: 'developer__shell', title: 'shell', argumentChars: 88 },
  ],
  argumentChars: 1292,
  reasoningChars: 0,
  text: 'Now I will update the config.',
  repeatedCalls: 0,
  repeatedTitle: null,
};
const idle = glancePush(
  runningSnapshot(IDLE_STATUS, { measured: measuredRead({ writing: figure(29.6, 29.6, 29.6, 1) }) })
);
const question = {
  sessionId: 'q1',
  sessionName: 'Deploy the site',
  question: 'Push to production or staging?',
};

function choices() {
  return { onHideForNow: vi.fn(), onTurnOff: vi.fn() };
}

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

  it('the docked card has no pill control and no close; the desktop one has both', () => {
    renderCard(writing);
    expect(screen.queryByTestId('engine-glance-collapse')).toBeNull();
    expect(screen.queryByTestId('engine-glance-close')).toBeNull();
  });

  it('Q-218: the docked card hides from its own control, and that click opens nothing', () => {
    const onHide = vi.fn();
    const props = renderCard(writing, { onHide });
    const hide = screen.getByTestId('engine-glance-hide');
    expect(hide.getAttribute('aria-label')).toMatch(/Hide this card/);
    fireEvent.click(hide);
    expect(onHide).toHaveBeenCalledOnce();
    expect(props.onOpenEngine).not.toHaveBeenCalled();
  });

  it('negative control: the desktop window has no hide (it closes for the spell instead)', () => {
    renderCard(writing, { variant: 'desktop', hideChoices: choices() });
    expect(screen.queryByTestId('engine-glance-hide')).toBeNull();
  });

  it('Q-218: the chat’s 77k prompt leads; goose’s tool-label call is named beside it, never as the prompt', () => {
    renderCard(sideCall);
    expect(screen.getByTestId('engine-glance-stage').textContent).toBe('Reading prompt');
    expect(screen.getByTestId('engine-glance-hero').textContent).toBe('77K');
    expect(screen.getByTestId('engine-glance-progress').getAttribute('aria-valuenow')).toBe('1');
    expect(screen.getByTestId('engine-glance-chat').textContent).toBe(
      'Chat · Refactor the auth flow'
    );
    expect(screen.getByTestId('engine-glance-side').textContent).toBe(
      'Beside it: Labeling tool calls'
    );
    expect(screen.queryByTestId('engine-glance-others')).toBeNull();
    expect(document.body.textContent).not.toContain('174');
  });

  it('desktop: shrink to a pill, and close', () => {
    const hideChoices = choices();
    const props = renderCard(writing, { variant: 'desktop', hideChoices });
    fireEvent.click(screen.getByTestId('engine-glance-collapse'));
    expect(props.onCollapsedChange).toHaveBeenCalledWith(true);
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    fireEvent.click(screen.getByTestId('engine-glance-hide-for-now'));
    expect(hideChoices.onHideForNow).toHaveBeenCalledOnce();
    expect(props.onOpenEngine).not.toHaveBeenCalled();
  });
});

describe('EngineGlanceCard — Q-224: the desktop X offers "Hide for now" and "Turn off"', () => {
  for (const collapsed of [false, true]) {
    const size = collapsed ? 'pill' : 'card';

    it(`${size}: the X opens the two choices, in words; it closes nothing by itself`, () => {
      const hideChoices = choices();
      const props = renderCard(writing, { variant: 'desktop', collapsed, hideChoices });
      const x = screen.getByTestId('engine-glance-close');
      expect(x.getAttribute('aria-label')).toBe('Hide or turn off the floating window');
      expect(x.getAttribute('aria-expanded')).toBe('false');
      expect(screen.queryByTestId('engine-glance-hide-choices')).toBeNull();
      fireEvent.click(x);
      expect(x.getAttribute('aria-expanded')).toBe('true');
      const menu = screen.getByTestId('engine-glance-hide-choices');
      expect(screen.getByTestId('engine-glance-hide-for-now').textContent).toBe(
        'Hide for nowBack the next time the engine works'
      );
      expect(screen.getByTestId('engine-glance-turn-off').textContent).toBe(
        'Turn off the floating windowTurn it back on in Settings › App'
      );
      expect(within(menu).getAllByRole('button')).toHaveLength(2);
      expect(hideChoices.onHideForNow).not.toHaveBeenCalled();
      expect(hideChoices.onTurnOff).not.toHaveBeenCalled();
      // The X again folds them away.
      fireEvent.click(x);
      expect(screen.queryByTestId('engine-glance-hide-choices')).toBeNull();
      expect(props.onOpenEngine).not.toHaveBeenCalled();
    });

    it(`${size}: "Hide for now" snoozes — and only that`, () => {
      const hideChoices = choices();
      const props = renderCard(writing, { variant: 'desktop', collapsed, hideChoices });
      fireEvent.click(screen.getByTestId('engine-glance-close'));
      fireEvent.click(screen.getByTestId('engine-glance-hide-for-now'));
      expect(hideChoices.onHideForNow).toHaveBeenCalledOnce();
      expect(hideChoices.onTurnOff).not.toHaveBeenCalled();
      expect(props.onOpenEngine).not.toHaveBeenCalled();
      // Taken: the choices are gone, so the window never comes back with them still open.
      expect(screen.queryByTestId('engine-glance-hide-choices')).toBeNull();
    });

    it(`${size}: "Turn off the floating window" turns it off — and only that`, () => {
      const hideChoices = choices();
      const props = renderCard(writing, { variant: 'desktop', collapsed, hideChoices });
      fireEvent.click(screen.getByTestId('engine-glance-close'));
      fireEvent.click(screen.getByTestId('engine-glance-turn-off'));
      expect(hideChoices.onTurnOff).toHaveBeenCalledOnce();
      expect(hideChoices.onHideForNow).not.toHaveBeenCalled();
      expect(props.onOpenEngine).not.toHaveBeenCalled();
      expect(screen.queryByTestId('engine-glance-hide-choices')).toBeNull();
    });
  }

  it('the choices open on the side away from the corner, so the card never moves under the pointer', () => {
    const first = renderCard(writing, { variant: 'desktop', hideChoices: choices() });
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    const bottomRight = screen.getByTestId('engine-glance-stack');
    // Default corner (bottom-right): stacked ABOVE the card, flush right.
    expect(bottomRight.className).toContain('flex-col-reverse');
    expect(bottomRight.className).toContain('items-end');
    expect(bottomRight.firstElementChild?.getAttribute('data-testid')).toBe('engine-glance');
    expect(first.onOpenEngine).not.toHaveBeenCalled();
  });

  it('top-left: below the card, flush left', () => {
    renderCard(writing, { variant: 'desktop', hideChoices: choices(), corner: 'top-left' });
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    const stack = screen.getByTestId('engine-glance-stack');
    expect(stack.className).toMatch(/\bflex-col\b(?!-)/);
    expect(stack.className).toContain('items-start');
  });

  it('the one-time hint says it turns off from here, and "Got it" dismisses it', () => {
    const onDismissHint = vi.fn();
    renderCard(writing, {
      variant: 'desktop',
      hideChoices: choices(),
      turnOffHint: true,
      onDismissHint,
    });
    const hint = screen.getByTestId('engine-glance-hint');
    expect(hint.textContent).toBe('You can turn this off from hereGot it');
    fireEvent.click(screen.getByTestId('engine-glance-hint-dismiss'));
    expect(onDismissHint).toHaveBeenCalledOnce();
  });

  it('opening the X is the hint found: it is dismissed, and the choices take its place', () => {
    const onDismissHint = vi.fn();
    renderCard(writing, {
      variant: 'desktop',
      collapsed: true,
      hideChoices: choices(),
      turnOffHint: true,
      onDismissHint,
    });
    fireEvent.click(screen.getByTestId('engine-glance-close'));
    expect(onDismissHint).toHaveBeenCalledOnce();
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
    expect(screen.getByTestId('engine-glance-hide-choices')).toBeTruthy();
  });

  it('negative controls: no hint unless asked; the docked card has neither', () => {
    renderCard(writing, { variant: 'desktop', hideChoices: choices() });
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
    expect(screen.getByTestId('engine-glance-stack').children).toHaveLength(1);
  });

  it('negative control: the docked card never offers the choices or the hint', () => {
    renderCard(writing, { turnOffHint: true, onDismissHint: vi.fn() });
    expect(screen.queryByTestId('engine-glance-close')).toBeNull();
    expect(screen.queryByTestId('engine-glance-hint')).toBeNull();
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
    renderCard(splitReading, { variant: 'desktop', collapsed: true });
    expect(screen.getByTestId('engine-glance-pill-figure').textContent).toBe('237 tok/s');
  });

  it('idle: the word alone — a median is not a live figure', () => {
    renderCard(idle, { variant: 'desktop', collapsed: true });
    expect(screen.queryByTestId('engine-glance-pill-figure')).toBeNull();
  });

  it('a question waiting shows as a count on the pill; expand brings the card back', () => {
    const props = renderCard(
      { ...writing, sessions: { running: 1, needsYou: [question] } },
      { variant: 'desktop', collapsed: true }
    );
    expect(screen.getByTestId('engine-glance-pill-needs').textContent).toBe('1 needs you');
    fireEvent.click(screen.getByTestId('engine-glance-expand'));
    expect(props.onCollapsedChange).toHaveBeenCalledWith(false);
  });
});

describe('EngineGlanceCard — what the chat’s turn is forming (Q-215: the disclosure moved here)', () => {
  it('the chat line is a turn with calls forming: "What it’s writing" opens the calls beside the card', () => {
    renderCard(turnWriting, { forming });
    const toggle = screen.getByTestId('engine-glance-forming-toggle');
    expect(toggle.textContent).toBe('What it’s writing · 2 tool calls');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByTestId('forming-panel')).toBeNull();
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    const panel = screen.getByTestId('forming-panel');
    // Portalled out of the sidebar's clipping frame, fixed beside the card.
    expect(panel.parentElement).toBe(document.body);
    expect(panel.className).toContain('fixed');
    expect(within(panel).getByTestId('forming-panel-calls').textContent).toContain('edit');
    expect(within(panel).getByTestId('forming-panel-text').textContent).toBe(
      'Now I will update the config.'
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByTestId('forming-panel')).toBeNull();
  });

  it('opening it opens nothing else — neither the Engine nor the chat', () => {
    const props = renderCard(turnWriting, { forming });
    fireEvent.click(screen.getByTestId('engine-glance-forming-toggle'));
    expect(props.onOpenEngine).not.toHaveBeenCalled();
    expect(props.onOpenSession).not.toHaveBeenCalled();
  });

  it('negative controls: nothing forming, goose’s own call on the chat line, or no chat — no toggle', () => {
    renderCard(turnWriting, { forming: null });
    expect(screen.queryByTestId('engine-glance-forming-toggle')).toBeNull();
  });

  it('negative control: the chat line is goose’s fact check — its forming is not the answer', () => {
    const check = glancePush(
      runningSnapshot(GENERATING_STATUS, {
        serving: attributeServing([{ ...CHAT_ROW, work: 'factCheck' }], 1, [], null),
      })
    );
    renderCard(check, { forming });
    expect(screen.queryByTestId('engine-glance-forming-toggle')).toBeNull();
  });

  it('negative control: an engine serving no chat of this app offers nothing', () => {
    renderCard(writing, { forming });
    expect(screen.queryByTestId('engine-glance-forming-toggle')).toBeNull();
  });
});

describe('EngineGlanceCard — the node the serving way belongs to (design §7.3, S7)', () => {
  const NODE = { id: '27b-split', name: '27B · both Macs' };
  const served = (nodes: { id: string; name: string }[]) =>
    glancePush(runningSnapshot(GENERATING_STATUS), {
      served: [{ way: { kind: 'single', modelId: GLANCE_MODEL, servedModelId: 'q' }, nodes }],
    });

  it('the sidebar card names it under the mode line and links to its card on the Nodes page', () => {
    const onOpenNode = vi.fn();
    const props = renderCard(served([NODE]), { onOpenNode });
    const line = screen.getByTestId('engine-glance-served-node');
    expect(line.textContent).toBe('Node · 27B · both Macs');
    expect(line.getAttribute('aria-label')).toBe('Open 27B · both Macs on the Nodes page');
    fireEvent.click(line);
    expect(onOpenNode).toHaveBeenCalledWith('27b-split');
    // The node link is its own control: it never also opens the Engine.
    expect(props.onOpenEngine).not.toHaveBeenCalled();
  });

  it('two nodes naming one way are both said; the link opens the first', () => {
    const onOpenNode = vi.fn();
    renderCard(served([NODE, { id: 'mac-engine', name: 'Mihai Macbook engine' }]), {
      onOpenNode,
    });
    const line = screen.getByTestId('engine-glance-served-node');
    expect(line.textContent).toBe('Node · 27B · both Macs and Mihai Macbook engine');
    fireEvent.click(line);
    expect(onOpenNode).toHaveBeenCalledWith('27b-split');
  });

  it('the desktop window, which cannot navigate, says the name as plain words', () => {
    renderCard(served([NODE]), { variant: 'desktop' });
    const line = screen.getByTestId('engine-glance-served-node');
    expect(line.tagName).toBe('SPAN');
    expect(line.textContent).toBe('Node · 27B · both Macs');
  });

  it('no report of this way: no node line at all — nothing guessed', () => {
    renderCard(writing, { onOpenNode: vi.fn() });
    expect(screen.queryByTestId('engine-glance-served-node')).toBeNull();
    expect(screen.queryByTestId('engine-glance-node-unknown')).toBeNull();
  });

  it('a failed read is said in its words', () => {
    renderCard(
      glancePush(runningSnapshot(GENERATING_STATUS), {
        served: [{ error: 'nodes/residency: goosed unreachable' }],
      })
    );
    expect(screen.getByTestId('engine-glance-node-unknown').textContent).toBe(
      'Which node serves is not known: nodes/residency: goosed unreachable'
    );
  });

  it('carries no banned pattern, and every class it adds compiles', async () => {
    renderCard(served([NODE]), { onOpenNode: vi.fn() });
    const card = screen.getByTestId('engine-glance');
    assertStudioClean(card);
    const line = screen.getByTestId('engine-glance-served-node');
    expect(await missingUtilities([...line.classList])).toEqual([]);
  }, 30_000);
});
