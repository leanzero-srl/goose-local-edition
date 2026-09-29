import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import {
  SINGLE_UNLOOKED_READ,
  SINGLE_WARM_READ,
  SPLIT_COLD_READ,
  SPLIT_EVICTED_FOR_ROOM_READ,
  SPLIT_EVICTED_READ,
  SPLIT_PARTLY_CACHED_READ,
  SPLIT_UNKNOWN_READ,
  SPLIT_WARM_READ,
} from './promptCache.fixtures';
import { promptCacheOf, promptRead, readBarOf, readProgressOf } from './engineFigures';
import { PromptReadBar } from './PromptReadBar';
import { MlxStateTile } from './MlxStateTile';
import { FLASH_READY, FLASH_SERVING } from './mlxDistributed.fixtures';
import { EngineGlanceCard } from '../engineGlance/EngineGlanceCard';
import { TurnWorkingRow } from '../turnWorking/TurnWorkingRow';
import { resetTurnReadForTests, usePublishTurnRead } from '../turnWorking/turnReadStore';
import { resetNowForTests } from '../sessionActivity/ActivityPills';
import { resetSessionActivityForTests } from '../sessionActivity/sessionActivityStore';
import { rememberLocalMlxEngineStatus } from '../../acp/mlx-engine-latest';
import { CHAT_ROW, glancePush, runningSnapshot, statsOf } from '../../utils/engineGlance.fixtures';
import { attributeServing } from '../../utils/mlxServing';
import { toMlxDistributedReport } from '../../utils/mlxDistributedReport';
import { buildMlxTrayModel, mlxTrayTitle, type MlxTrayItem } from '../../utils/mlxTray';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import { contrast, resolvedPaint, studioToken, type Theme } from '../lz/resolvedPaint';
import type { EnginePhase } from '../lz';

/**
 * Q-337 — "it shows it has to read 97k or 148k but it finishes really quickly … highlight it
 * visually when it actually does come from prompt caching". Every read surface draws the ONE split
 * (engineFigures.ts): the cached part (teal, full at once) and the new part read so far (the
 * reading colour), and says it in words. Four cases, each as the engine reports it:
 *  - WARM: E2E #3p turn 7 — 114,948 tokens, 113,824 from the cache, 576 of the 1,124 new ones read;
 *  - COLD: #3o turn 5 — nothing cached, 45,056 of 109,655 read;
 *  - UNKNOWN: rank 0 has not looked the prompt up (cached_tokens null) — today's plain bar;
 *  - SINGLE: Rapid-MLX knows the split (cache_hit_type set) but reports no position — words, no bar.
 */

const SESSION = 'q337-chat';
const THEMES: Theme[] = ['light', 'dark'];

const leadOf = (body: unknown) => statsOf(body).requests[0];
const pct = (n: number, total: number) => (n / total) * 100;

describe('Q-337: the split, as the engine reports it (engineFigures.ts, the one derivation)', () => {
  it('the parser: unknown until looked up — the split rank 0 sends null, Rapid-MLX names the lookup', () => {
    expect(leadOf(SPLIT_WARM_READ).cachedTokens).toBe(113_824);
    expect(leadOf(SPLIT_COLD_READ).cachedTokens).toBe(0);
    expect(leadOf(SPLIT_UNKNOWN_READ).cachedTokens).toBeNull();
    expect(leadOf(SINGLE_WARM_READ).cachedTokens).toBe(113_824);
    // Rapid-MLX's Request defaults cached_tokens to 0 before its lookup: that 0 is not a figure.
    expect(leadOf(SINGLE_UNLOOKED_READ).cachedTokens).toBeNull();
  });

  it('WARM: 113.8K cached, 1,124 new, 576 of them read — the bar, and the time left over the new part only', () => {
    const lead = leadOf(SPLIT_WARM_READ);
    const cache = promptCacheOf(lead)!;
    expect(cache).toEqual({
      total: 114_948,
      cached: 113_824,
      fresh: 1_124,
      freshDone: 576,
      evicted: null,
    });
    expect(readBarOf(readProgressOf(lead), cache)).toEqual({
      cached: 113_824 / 114_948,
      read: 576 / 114_948,
    });
    const read = promptRead(lead)!;
    expect(read.cache).toEqual(cache);
    expect(read.leftS).toBeCloseTo((1_124 - 576) / 412);
  });

  it('PARTLY CACHED (#3o turn 6): 40.2K cached, 22.5K of the 62K new read — the cached part then the read part', () => {
    const lead = leadOf(SPLIT_PARTLY_CACHED_READ);
    const cache = promptCacheOf(lead)!;
    expect(cache).toEqual({
      total: 102_210,
      cached: 40_244,
      fresh: 61_966,
      freshDone: 22_528,
      evicted: null,
    });
    expect(readBarOf(readProgressOf(lead), cache)).toEqual({
      cached: 40_244 / 102_210,
      read: 22_528 / 102_210,
    });
    expect(promptRead(lead)!.leftS).toBeCloseTo((61_966 - 22_528) / 412);
  });

  it('a position reported before the first chunk (0) is none of the new part read — never the cached part as work left', () => {
    const lead = { ...leadOf(SPLIT_WARM_READ), prefilledTokens: 0 };
    expect(promptCacheOf(lead)!.freshDone).toBe(0);
    expect(readBarOf(readProgressOf(lead), promptCacheOf(lead))).toEqual({
      cached: 113_824 / 114_948,
      read: 0,
    });
    expect(promptRead(lead)!.leftS).toBeCloseTo(1_124 / 412);
  });

  it('COLD, UNKNOWN and SINGLE', () => {
    const cold = leadOf(SPLIT_COLD_READ);
    expect(promptCacheOf(cold)).toEqual({
      total: 109_655,
      cached: 0,
      fresh: 109_655,
      freshDone: 45_056,
      evicted: null,
    });
    expect(readBarOf(readProgressOf(cold), promptCacheOf(cold))).toEqual({
      cached: 0,
      read: 45_056 / 109_655,
    });
    expect(promptRead(cold)!.leftS).toBeCloseTo((109_655 - 45_056) / 412);

    const unknown = leadOf(SPLIT_UNKNOWN_READ);
    expect(promptCacheOf(unknown)).toBeNull();
    expect(readBarOf(readProgressOf(unknown), null)).toEqual({ cached: 0, read: 0 });

    const single = leadOf(SINGLE_WARM_READ);
    expect(promptCacheOf(single)).toEqual({
      total: 114_948,
      cached: 113_824,
      fresh: 1_124,
      freshDone: null,
      evicted: null,
    });
    // No position: no bar, no time left — the split is a fact, a share would be a guess.
    expect(readBarOf(readProgressOf(single), promptCacheOf(single))).toBeNull();
    expect(promptRead(single)!.leftS).toBeNull();
  });
});

function Publish({ body }: { body: unknown }) {
  usePublishTurnRead(SESSION, promptRead(leadOf(body)));
  return null;
}

function renderRow(body: unknown) {
  return render(
    <IntlProvider locale="en" messages={{}}>
      <Publish body={body} />
      <TurnWorkingRow sessionId={SESSION} />
    </IntlProvider>
  );
}

/** The two parts' widths, in percent of the whole prompt (drawn to two decimals). */
function expectSegments(bar: HTMLElement, cached: number | null, read: number) {
  const width = (el: HTMLElement) => {
    expect(el.style.width).toMatch(/%$/);
    return parseFloat(el.style.width);
  };
  const cachedPart = within(bar).queryByTestId('prompt-read-cached');
  if (cached == null) expect(cachedPart).toBeNull();
  else expect(width(cachedPart!)).toBeCloseTo(cached, 2);
  expect(width(within(bar).getByTestId('prompt-read-new'))).toBeCloseTo(read, 2);
}

async function expectDesigned(container: HTMLElement) {
  assertStudioClean(container);
  const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
  expect(await missingUtilities(classes)).toEqual([]);
}

describe('Q-337: the chat’s working row (TurnWorkingRow)', () => {
  beforeEach(() => {
    resetNowForTests(Date.parse('2026-09-28T10:22:00Z'));
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
  });
  afterEach(() => {
    resetTurnReadForTests();
    resetSessionActivityForTests();
  });

  it('WARM: the split in words, the new part’s progress, the time left, the two-part bar and why', async () => {
    const { container } = renderRow(SPLIT_WARM_READ);
    expect(screen.getByTestId('turn-working-figures').textContent).toBe(
      '114.9K tokens · 113.8K from cache · 1.1K new — reading the new part · 576 of 1.1K new read · 412 tok/s · about 1s left at this rate'
    );
    const bar = screen.getByTestId('turn-working-progress');
    expectSegments(bar, pct(113_824, 114_948), pct(576, 114_948));
    expect(bar.getAttribute('aria-valuenow')).toBe('99');
    expect(bar.getAttribute('aria-valuetext')).toBe('113.8K from cache, 576 of 1.1K new read');
    expect(screen.getByTestId('turn-working-why').textContent).toBe(
      'goose keeps the start of this conversation in the engine’s memory; only the new part is read again'
    );
    // The legend: a dot in each part's colour before the words that name it.
    expect(screen.getByTestId('prompt-read-swatch-cached').className).toContain('bg-lz-cache');
    expect(screen.getByTestId('prompt-read-swatch-read').className).toContain(
      'bg-lz-phase-reading'
    );
    await expectDesigned(container);
  }, 30_000);

  it('COLD: "nothing cached — reading all of it", the plain bar', () => {
    renderRow(SPLIT_COLD_READ);
    expect(screen.getByTestId('turn-working-figures').textContent).toBe(
      '109.7K tokens · nothing cached — reading all of it · 45.1K of 109.7K tokens read · 412 tok/s · about 2m 37s left at this rate'
    );
    expectSegments(screen.getByTestId('turn-working-progress'), null, pct(45_056, 109_655));
    expect(screen.queryByTestId('turn-working-why')).toBeNull();
  });

  it('UNKNOWN: today’s plain bar and words — no cached part is guessed', () => {
    renderRow(SPLIT_UNKNOWN_READ);
    expect(screen.getByTestId('turn-working-figures').textContent).toBe('0 of 114.9K tokens read');
    expectSegments(screen.getByTestId('turn-working-progress'), null, 0);
    expect(screen.queryByTestId('prompt-read-swatch-cached')).toBeNull();
  });

  it('SINGLE: the split and the time so far — no bar, no time left', () => {
    renderRow(SINGLE_WARM_READ);
    expect(screen.getByTestId('turn-working-figures').textContent).toBe(
      '114.9K tokens · 113.8K from cache · 1.1K new — reading the new part · 3s so far'
    );
    expect(screen.queryByTestId('turn-working-progress')).toBeNull();
    expect(screen.getByTestId('turn-working-why')).toBeTruthy();
  });
});

const splitPush = (body: unknown) =>
  glancePush(
    runningSnapshot(body, {
      engine: 'distributed',
      serving: attributeServing([CHAT_ROW], 1, [], null),
    }),
    { distributed: { report: toMlxDistributedReport(FLASH_READY), ageMs: 0 } }
  );

function renderGlance(body: unknown, single = false, expanded = false) {
  const push = single
    ? glancePush(runningSnapshot(body, { serving: attributeServing([CHAT_ROW], 1, [], null) }))
    : splitPush(body);
  return render(
    <IntlProvider locale="en" messages={{}}>
      <EngineGlanceCard
        push={push}
        variant="dock"
        collapsed={false}
        expanded={expanded}
        onOpenEngine={() => {}}
        onOpenSession={() => {}}
        onToggleExpanded={() => {}}
        onCollapsedChange={() => {}}
      />
    </IntlProvider>
  );
}

describe('Q-337: the sidebar glance (EngineGlanceCard)', () => {
  it('WARM: the two-part bar, the split in words, the why in Details', async () => {
    const { container } = renderGlance(SPLIT_WARM_READ, false, true);
    expectSegments(
      screen.getAllByTestId('engine-glance-progress')[0],
      pct(113_824, 114_948),
      pct(576, 114_948)
    );
    expect(screen.getByTestId('engine-glance-cache').textContent).toBe(
      '113.8K from cache · 1.1K new — reading the new part'
    );
    expect(screen.getByTestId('engine-glance-cache-why').textContent).toContain(
      'only the new part is read again'
    );
    expect(screen.getByTestId('engine-glance-details-toggle').getAttribute('aria-label')).toBe(
      'Hide why this prompt reads fast, rates and memory'
    );
    await expectDesigned(container);
  }, 30_000);

  it('COLD, UNKNOWN, SINGLE', () => {
    const cold = renderGlance(SPLIT_COLD_READ);
    expect(screen.getByTestId('engine-glance-cache').textContent).toBe(
      'nothing cached — reading all of it'
    );
    expectSegments(screen.getAllByTestId('engine-glance-progress')[0], null, pct(45_056, 109_655));
    cold.unmount();

    const unknown = renderGlance(SPLIT_UNKNOWN_READ);
    expect(screen.queryByTestId('engine-glance-cache')).toBeNull();
    expectSegments(screen.getAllByTestId('engine-glance-progress')[0], null, 0);
    unknown.unmount();

    renderGlance(SINGLE_WARM_READ, true);
    expect(screen.getByTestId('engine-glance-cache').textContent).toBe(
      '113.8K from cache · 1.1K new — reading the new part'
    );
    expect(screen.queryByTestId('engine-glance-progress')).toBeNull();
  });
});

function renderTile(body: unknown, single = false) {
  return render(
    <IntlProvider locale="en" messages={{}}>
      <MlxStateTile
        state={single ? 'running' : 'stopped'}
        unreachable={false}
        live={{ ok: true, stats: statsOf(body) }}
        history={[]}
        serving={null}
        mount={null}
        cost={null}
        failedError={null}
        action={null}
        modeLabel="x"
        distributed={single ? null : { ...FLASH_SERVING, inflight: 1, slotsInUse: 1 }}
      />
    </IntlProvider>
  );
}

describe('Q-337: the Engine tile’s request rows (MlxStateTile)', () => {
  beforeEach(() => {
    rememberLocalMlxEngineStatus({
      state: 'running',
      modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      restartRequired: false,
      availableMemoryGb: 0,
      totalMemoryGb: 0,
    });
  });

  it('WARM: 99% in (never a rounded-up 100% while it reads), the two-part bar, the words and why', async () => {
    const { container } = renderTile(SPLIT_WARM_READ);
    const [row] = screen.getAllByTestId('mlx-live-request');
    expect(row).toHaveTextContent('Reading prompt · 114.9K tokens');
    expect(row).toHaveTextContent('99% · 4s');
    expectSegments(
      within(row).getByTestId('mlx-live-request-bar'),
      pct(113_824, 114_948),
      pct(576, 114_948)
    );
    expect(within(row).getByTestId('mlx-live-request-cache').textContent).toBe(
      '113.8K from cache · 1.1K new — reading the new part · 576 of 1.1K new read'
    );
    expect(within(row).getByTestId('mlx-live-request-why')).toBeTruthy();
    await expectDesigned(container);
  }, 30_000);

  it('COLD, UNKNOWN, SINGLE', () => {
    const cold = renderTile(SPLIT_COLD_READ);
    let [row] = screen.getAllByTestId('mlx-live-request');
    expect(within(row).getByTestId('mlx-live-request-cache').textContent).toBe(
      'nothing cached — reading all of it'
    );
    expectSegments(within(row).getByTestId('mlx-live-request-bar'), null, pct(45_056, 109_655));
    cold.unmount();

    const unknown = renderTile(SPLIT_UNKNOWN_READ);
    [row] = screen.getAllByTestId('mlx-live-request');
    expect(within(row).queryByTestId('mlx-live-request-cache')).toBeNull();
    expectSegments(within(row).getByTestId('mlx-live-request-bar'), null, 0);
    unknown.unmount();

    renderTile(SINGLE_WARM_READ, true);
    [row] = screen.getAllByTestId('mlx-live-request');
    expect(within(row).getByTestId('mlx-live-request-cache').textContent).toBe(
      '113.8K from cache · 1.1K new — reading the new part'
    );
    expect(within(row).queryByTestId('mlx-live-request-bar')).toBeNull();
  });
});

describe('Q-337: the tray', () => {
  const OPTS = { canAct: true, mountModelId: null, distributed: null };
  const labels = (items: MlxTrayItem[]) =>
    items.map((i) => (i.type === 'separator' ? '---' : i.label));
  const reading = (body: unknown) =>
    labels(buildMlxTrayModel(runningSnapshot(body), OPTS).items).find((l) =>
      l.startsWith('Reading a ')
    );

  it('the title names what is computed; the reading line says the split', () => {
    expect(mlxTrayTitle(runningSnapshot(SPLIT_WARM_READ))).toBe('Reading 1.1k new');
    expect(reading(SPLIT_WARM_READ)).toBe(
      'Reading a 115k-token prompt for 4s: 114k from cache, 1.1k new (576 of it read)'
    );
    expect(mlxTrayTitle(runningSnapshot(SPLIT_COLD_READ))).toBe('Reading 110k');
    expect(reading(SPLIT_COLD_READ)).toBe(
      'Reading a 110k-token prompt for 1m 49s: nothing cached, 45k read'
    );
    expect(mlxTrayTitle(runningSnapshot(SPLIT_UNKNOWN_READ))).toBe('Reading 115k');
    expect(reading(SPLIT_UNKNOWN_READ)).toBe('Reading a 115k-token prompt, 0 read for 4s');
    expect(mlxTrayTitle(runningSnapshot(SINGLE_WARM_READ))).toBe('Reading 1.1k new');
    expect(reading(SINGLE_WARM_READ)).toBe(
      'Reading a 115k-token prompt for 3s: 114k from cache, 1.1k new'
    );
  });
});

/**
 * Q-498 — two chats on the split evict each other's prompt cache: E2E #3x's 200,456-token call read
 * cold for ~11 minutes after a second chat's side call pushed its 199,798-token prefix out, and
 * every surface said only "nothing cached — reading all of it". rank 0 names the eviction
 * (`evicted_prefix`); every read surface says it after the split, and the tray too.
 */
describe('Q-498: a prefix another chat pushed out is named, on every read surface', () => {
  beforeEach(() => {
    resetNowForTests(Date.parse('2026-09-29T12:17:00Z'));
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
    rememberLocalMlxEngineStatus({
      state: 'running',
      modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      restartRequired: false,
      availableMemoryGb: 0,
      totalMemoryGb: 0,
    });
  });
  afterEach(() => {
    resetTurnReadForTests();
    resetSessionActivityForTests();
  });

  it('the parser and the split: 199.8K evicted while another chat was kept — absent everywhere else', () => {
    const lead = leadOf(SPLIT_EVICTED_READ);
    expect(lead.evictedPrefix).toEqual({
      tokens: 199_798,
      whileKeeping: 'anotherConversation',
      rows: 2,
      width: 77_683,
      agoS: 42.93,
    });
    expect(promptCacheOf(lead)!.evicted).toEqual(lead.evictedPrefix);
    expect(leadOf(SPLIT_EVICTED_FOR_ROOM_READ).evictedPrefix!.whileKeeping).toBeNull();
    for (const body of [SPLIT_COLD_READ, SPLIT_WARM_READ, SPLIT_UNKNOWN_READ, SINGLE_WARM_READ]) {
      expect(leadOf(body).evictedPrefix).toBeNull();
    }
    // What the cache supplied already covers it: nothing is said to be lost.
    expect(promptCacheOf({ ...lead, cachedTokens: 199_798 })!.evicted).toBeNull();
  });

  it('the chat’s working row: the reason after the split, the cold bar unchanged', () => {
    renderRow(SPLIT_EVICTED_READ);
    const figures = screen.getByTestId('turn-working-figures').textContent!;
    expect(figures).toMatch(
      /^200\.5K tokens · nothing cached — reading all of it · another chat pushed 199\.8K of this conversation out of the engine’s memory · 91\.7K of 200\.5K tokens read · /
    );
    expect(screen.getByTestId('prompt-read-evicted').textContent).toBe(
      'another chat pushed 199.8K of this conversation out of the engine’s memory'
    );
    expectSegments(screen.getByTestId('turn-working-progress'), null, pct(91_689, 200_456));
  });

  it('no other chat kept: the prefix was pushed out, and nothing blames another chat', () => {
    renderRow(SPLIT_EVICTED_FOR_ROOM_READ);
    expect(screen.getByTestId('prompt-read-evicted').textContent).toBe(
      '199.8K of it was cached, then pushed out of the engine’s memory'
    );
  });

  it('the sidebar glance and the Engine tile say it short', () => {
    const glance = renderGlance(SPLIT_EVICTED_READ);
    expect(screen.getByTestId('engine-glance-cache').textContent).toBe(
      'nothing cached — reading all of it · another chat pushed 199.8K of it out of memory'
    );
    glance.unmount();
    renderTile(SPLIT_EVICTED_READ);
    const [row] = screen.getAllByTestId('mlx-live-request');
    expect(within(row).getByTestId('mlx-live-request-cache').textContent).toBe(
      'nothing cached — reading all of it · another chat pushed 199.8K of it out of memory'
    );
  });

  it('the tray', () => {
    const line = labels(
      buildMlxTrayModel(runningSnapshot(SPLIT_EVICTED_READ), {
        canAct: true,
        mountModelId: null,
        distributed: null,
      }).items
    ).find((l) => l.startsWith('Reading a '));
    expect(line).toBe(
      'Reading a 200k-token prompt for 10m 53s: nothing cached, 92k read — another chat pushed 200k of it out of memory'
    );
  });
});

const labels = (items: MlxTrayItem[]) =>
  items.map((i) => (i.type === 'separator' ? '---' : i.label));

/**
 * Both parts hold 3:1 against the track they sit in, in both themes, through the compiled CSS
 * (Q-247's resolvedPaint): on a surface the track is its own solid fill; on a phase fill the track
 * is hollow, so the fill behind it is the ground — reading, and the writing / held fills a request
 * row can sit on.
 */
describe('Q-337: both parts are solid and contrast with the track', () => {
  const bar = { cached: 0.6, read: 0.2 };
  const cache = { total: 100, cached: 60, fresh: 40, freshDone: 20, evicted: null };

  function renderBar(paint: 'surface' | 'fill') {
    render(
      <IntlProvider locale="en" messages={{}}>
        <PromptReadBar
          bar={bar}
          cache={cache}
          paint={paint}
          label="Prompt read so far"
          height="h-2.5"
          testId={`bar-${paint}`}
        />
      </IntlProvider>
    );
    const track = screen.getByTestId(`bar-${paint}`);
    return {
      track,
      cached: within(track).getByTestId('prompt-read-cached'),
      read: within(track).getByTestId('prompt-read-new'),
    };
  }

  it('on a surface (the chat’s working row)', async () => {
    const { track, cached, read } = renderBar('surface');
    for (const theme of THEMES) {
      const ground = (await resolvedPaint(track, theme)).bg;
      const cachedBg = (await resolvedPaint(cached, theme)).bg;
      const readBg = (await resolvedPaint(read, theme)).bg;
      for (const hex of [ground, cachedBg, readBg]) expect(hex, theme).toMatch(/^#[0-9a-f]{6}$/);
      expect(contrast(cachedBg, ground), `${theme} cached`).toBeGreaterThanOrEqual(3);
      expect(contrast(readBg, ground), `${theme} read`).toBeGreaterThanOrEqual(3);
      expect(cachedBg, `${theme} two colours`).not.toBe(readBg);
    }
    expect((await resolvedPaint(cached, 'light')).bg).toBe('#0d9488');
    expect((await resolvedPaint(cached, 'dark')).bg).toBe('#2dd4bf');
  }, 30_000);

  it('on a phase fill (the Engine tile, the glance)', async () => {
    const { cached, read } = renderBar('fill');
    const grounds: EnginePhase[] = ['reading', 'writing', 'held', 'idle'];
    for (const theme of THEMES) {
      const cachedBg = (await resolvedPaint(cached, theme)).bg;
      expect(cachedBg, theme).toBe('#5eead4');
      // The read part is the fill's own ink (`bg-current`).
      expect((await resolvedPaint(read, theme)).bg).toBe('currentcolor');
      for (const phase of grounds) {
        const ground = studioToken(`--color-lz-phase-${phase}`, theme);
        const ink = studioToken(`--color-lz-phase-${phase}-ink`, theme);
        expect(contrast(cachedBg, ground), `${theme} ${phase} cached`).toBeGreaterThanOrEqual(3);
        expect(contrast(ink, ground), `${theme} ${phase} read`).toBeGreaterThanOrEqual(3);
      }
    }
  }, 30_000);
});
