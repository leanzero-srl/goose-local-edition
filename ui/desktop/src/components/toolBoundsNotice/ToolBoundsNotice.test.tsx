import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlProvider } from 'react-intl';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ToolListItem } from '@aaif/goose-sdk';
import type { ExtensionConfig } from '../../types/extensions';
import { createUserMessage, type Message } from '../../types/message';
import { AppEvents } from '../../constants/events';
import { createAcpSessionNotificationAdapter } from '../../acp/sessionNotificationAdapter';
import { assertStudioClean } from '../lz/assertStudioClean';
import GooseMessage from '../GooseMessage';
import ToolBoundsNotice from './ToolBoundsNotice';
import {
  engineToolSize,
  listEnvelopeBytes,
  measureTools,
  parseToolBoundsError,
  planTurnOff,
  withinBounds,
  type NamedToolGroup,
  type ToolBounds,
} from './toolSchemaBounds';

const mockListTools =
  vi.fn<(sessionId: string, extensionName?: string) => Promise<ToolListItem[]>>();
vi.mock('../../acp/permissions', () => ({
  listTools: (sessionId: string, extensionName?: string) => mockListTools(sessionId, extensionName),
}));
const mockSessionExtensions = vi.fn<(sessionId: string) => Promise<ExtensionConfig[]>>();
const mockRemove = vi.fn<(sessionId: string, name: string) => Promise<void>>();
vi.mock('../../acp/session-extensions', () => ({
  getSessionExtensions: (sessionId: string) => mockSessionExtensions(sessionId),
  removeSessionExtension: (sessionId: string, name: string) => mockRemove(sessionId, name),
  addSessionExtension: vi.fn(),
}));
vi.mock('../../toasts', () => ({
  toastService: { loading: vi.fn(() => 1), dismiss: vi.fn(), success: vi.fn(), error: vi.fn() },
}));

/** The engine's refusal, verbatim as the agent loop wrapped and persisted it (2026-09-23). */
const OWNER_TEXT =
  'Ran into this error: Request failed: Bad request (400): tool schema exceeds grammar-compile bounds (max 256 tools, 65536 bytes, depth 32); reduce the tool schema or set RAPID_MLX_CONSTRAIN_TOOLS=0 to fall back to free-form tool calling..\n\nPlease retry if you think this is a transient or recoverable error.';

const SESSION = 'sess-1';

function tool(name: string, properties: Record<string, unknown>): ToolListItem {
  return {
    name,
    description: 'x'.repeat(5000),
    parameters: Object.keys(properties),
    inputSchema: { type: 'object', properties },
  };
}

const DOCS = Array.from({ length: 8 }, (_, i) =>
  tool(`leanzerodocuments__op${i}`, { query: { type: 'string', description: 'd'.repeat(9000) } })
);
const PLAYWRIGHT = Array.from({ length: 3 }, (_, i) =>
  tool(`playwright__op${i}`, { url: { type: 'string', description: 'p'.repeat(2000) } })
);
const DEVELOPER = [tool('shell', { command: { type: 'string' } })];
const SCHEDULE = tool('platform__manage_schedule', { action: { type: 'string' } });

let sessionTools: Record<string, ToolListItem[]>;

function serveSession() {
  mockSessionExtensions.mockImplementation(async () =>
    Object.keys(sessionTools).map((name) => ({ type: 'builtin', name }) as ExtensionConfig)
  );
  mockListTools.mockImplementation(async (_sid, extensionName) =>
    extensionName == null
      ? [...Object.values(sessionTools).flat(), SCHEDULE]
      : (sessionTools[extensionName] ?? [])
  );
}

function wrap(ui: React.ReactElement) {
  return render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter initialEntries={['/pair']}>
        <Routes>
          <Route path="/pair" element={ui} />
          <Route path="/extensions" element={<div data-testid="mcps-view" />} />
        </Routes>
      </MemoryRouter>
    </IntlProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  sessionTools = {
    leanzerodocuments: DOCS,
    playwright: PLAYWRIGHT,
    developer: DEVELOPER,
    recall: [],
    tom: [],
  };
  serveSession();
  mockRemove.mockImplementation(async (_sid, name) => {
    delete sessionTools[name];
  });
});

describe('parseToolBoundsError', () => {
  it('reads the bounds from the engine’s own words and keeps its sentence verbatim', () => {
    const bounds = parseToolBoundsError(OWNER_TEXT);
    expect(bounds).toMatchObject({ maxTools: 256, maxBytes: 65536, maxDepth: 32 });
    expect(bounds?.raw.startsWith('tool schema exceeds grammar-compile bounds (max 256')).toBe(
      true
    );
    expect(bounds?.raw).not.toContain('Please retry');
  });

  it('an engine with other caps is read as stated — nothing is assumed', () => {
    const bounds = parseToolBoundsError(
      'Request failed: tool schema exceeds grammar-compile bounds (max 64 tools, 16384 bytes, depth 8); reduce it.'
    );
    expect(bounds).toMatchObject({ maxTools: 64, maxBytes: 16384, maxDepth: 8 });
  });

  it('a refusal that states no numbers yields null bounds, and other text is not a refusal', () => {
    expect(
      parseToolBoundsError('tool schema exceeds grammar-compile bounds; reduce the schema')
    ).toMatchObject({ maxTools: null, maxBytes: null, maxDepth: null });
    expect(parseToolBoundsError('Ran into this error: Server error: boom.')).toBeNull();
  });
});

describe('engine-unit measurement', () => {
  it('matches the engine’s walker byte-for-byte (name + parameters, ensure_ascii, compact)', () => {
    // Reference: python3 json.dumps({"name":…,"parameters":…}, separators=(',',':')) → 213 and
    // 113 bytes, list 329 — the same arithmetic rapid-mlx's _walk_size_and_depth charges.
    const a = {
      name: 'leanzerodocuments__search',
      inputSchema: {
        type: 'object',
        properties: {
          query: { type: 'string', description: 'Café — search text' },
          limit: { type: 'integer', minimum: 1 },
        },
        required: ['query'],
      },
    };
    const b = {
      name: 'shell',
      inputSchema: {
        type: 'object',
        properties: { command: { type: 'string' } },
        required: ['command'],
      },
    };
    expect(engineToolSize(a)).toEqual({ bytes: 213, depth: 3 });
    expect(engineToolSize(b).bytes).toBe(113);
    const total = measureTools([a, b]);
    expect(listEnvelopeBytes(total.tools, total.bytes)).toBe(329);
  });

  it('the description is not counted, and a bound the engine did not state is not compared', () => {
    const long = tool('x__y', { q: { type: 'string' } });
    expect(engineToolSize(long).bytes).toBe(
      engineToolSize({ name: long.name, inputSchema: long.inputSchema }).bytes
    );
    const size = { tools: 3, bytes: 10, depth: 1 };
    expect(withinBounds({ maxTools: null, maxBytes: 100, maxDepth: null, raw: '' }, size)).toBe(
      true
    );
    expect(withinBounds({ maxTools: null, maxBytes: null, maxDepth: null, raw: '' }, size)).toBe(
      null
    );
  });
});

describe('planTurnOff — the smallest set whose removal brings the session within the bounds', () => {
  const r4Bounds = parseToolBoundsError(OWNER_TEXT)!;
  /** The r4 session (2026-09-23, 3.0.12): per-extension engine bytes as the notice measured them. */
  const r4: NamedToolGroup[] = [
    { name: 'leanzerodocuments', tools: 17, bytes: 39_918, depth: 4 },
    { name: 'playwright', tools: 25, bytes: 15_442, depth: 4 },
    { name: 'leanzerowebsearch', tools: 11, bytes: 8_357, depth: 4 },
    { name: 'memory', tools: 6, bytes: 4_094, depth: 3 },
    { name: 'developer', tools: 5, bytes: 2_191, depth: 3 },
    { name: 'summon', tools: 2, bytes: 1_835, depth: 3 },
    { name: 'apps', tools: 4, bytes: 1_192, depth: 3 },
    { name: 'extensionmanager', tools: 4, bytes: 992, depth: 3 },
    { name: 'analyze', tools: 1, bytes: 785, depth: 3 },
    { name: 'ledger', tools: 2, bytes: 784, depth: 3 },
    { name: 'skills', tools: 1, bytes: 301, depth: 3 },
    { name: 'recall', tools: 0, bytes: 0, depth: 0 },
    { name: 'tom', tools: 0, bytes: 0, depth: 0 },
  ];
  const r4Unowned = { tools: 1, bytes: 881, depth: 3 };
  // The refused list: 79 tools charged 77,238 bytes with the envelope (`[` `]` + 78 commas).
  const r4Total = { tools: 79, bytes: 77_238 - 2 - 78, depth: 4 };

  it('r4: 77,238 of 65,536 bytes → turning off LeanZero Documents alone is enough', () => {
    expect(listEnvelopeBytes(r4Total.tools, r4Total.bytes)).toBe(77_238);
    const plan = planTurnOff(r4Bounds, r4Total, r4, r4Unowned);
    expect(plan.kind).toBe('fix');
    if (plan.kind !== 'fix') return;
    expect(plan.remove.map((e) => e.name)).toEqual(['leanzerodocuments']);
    expect(plan.after.tools).toBe(62);
    // 77,158 − 39,918 tool bytes, plus `[]` and 61 commas.
    expect(listEnvelopeBytes(plan.after.tools, plan.after.bytes)).toBe(37_303);
  });

  it('a tighter limit takes the next largest until it fits — never a smaller one first', () => {
    const tight: ToolBounds = { ...r4Bounds, maxBytes: 30_000 };
    const plan = planTurnOff(tight, r4Total, r4, r4Unowned);
    expect(plan.kind === 'fix' && plan.remove.map((e) => e.name)).toEqual([
      'leanzerodocuments',
      'playwright',
    ]);
  });

  it('a tool-count overflow is solved by the extensions with the most tools', () => {
    const fewTools: ToolBounds = { maxTools: 60, maxBytes: null, maxDepth: null, raw: '' };
    const plan = planTurnOff(fewTools, r4Total, r4, r4Unowned);
    // 79 − 25 (Playwright) = 54 ≤ 60: one extension, and not the byte-largest one.
    expect(plan.kind === 'fix' && plan.remove.map((e) => e.name)).toEqual(['playwright']);
  });

  it('an extension deeper than the depth bound is always in the set', () => {
    const deep = r4.map((e) => (e.name === 'ledger' ? { ...e, depth: 40 } : e));
    const plan = planTurnOff(r4Bounds, { ...r4Total, depth: 40 }, deep, r4Unowned);
    expect(plan.kind === 'fix' && plan.remove.map((e) => e.name)).toEqual([
      'leanzerodocuments',
      'ledger',
    ]);
  });

  it('within, unstated and unreachable are told apart', () => {
    const small = { tools: 3, bytes: 100, depth: 1 };
    expect(planTurnOff(r4Bounds, small, [], r4Unowned).kind).toBe('within');
    expect(
      planTurnOff(
        { maxTools: null, maxBytes: null, maxDepth: null, raw: '' },
        r4Total,
        r4,
        r4Unowned
      ).kind
    ).toBe('unstated');
    // goose's own tools alone are 881 bytes: no set of extensions gets under 500.
    expect(planTurnOff({ ...r4Bounds, maxBytes: 500 }, r4Total, r4, r4Unowned).kind).toBe(
      'unreachable'
    );
  });
});

describe('ToolBoundsNotice', () => {
  const bounds = parseToolBoundsError(OWNER_TEXT)!;

  it('leads with the measured total against the stated limit, as a bar with the limit marked', async () => {
    const { container } = wrap(
      <ToolBoundsNotice bounds={bounds} sessionId={SESSION} live retryText="hi" onRetry={vi.fn()} />
    );
    const line = await screen.findByTestId('tool-bounds-bytes-line');
    const total = measureTools([...DOCS, ...PLAYWRIGHT, ...DEVELOPER, SCHEDULE]);
    const listBytes = listEnvelopeBytes(total.tools, total.bytes);
    expect(listBytes).toBeGreaterThan(65_536);
    expect(line).toHaveTextContent(
      `${listBytes.toLocaleString('en')} of 65,536 bytes — ${(listBytes - 65_536).toLocaleString('en')} over`
    );
    expect(line.className).toContain('text-lz-err');
    const bar = screen.getByTestId('tool-bounds-bar');
    const mark = within(bar).getByTestId('tool-bounds-limit-mark');
    expect(mark.style.left).toBe(`${(65_536 / listBytes) * 100}%`);
    expect(within(bar).getByTestId('tool-bounds-bar-over').className).toContain('bg-lz-err');
    assertStudioClean(container);
  });

  it('offers the smallest set first — its bytes, its own Turn off, one primary action', async () => {
    const { container } = wrap(
      <ToolBoundsNotice bounds={bounds} sessionId={SESSION} live retryText="hi" onRetry={vi.fn()} />
    );
    const fix = await screen.findByTestId('tool-bounds-fix');
    expect(
      within(fix)
        .getAllByRole('listitem')
        .map((li) => li.getAttribute('data-testid'))
    ).toEqual(['tool-bounds-fix-row-leanzerodocuments']);
    const docsBytes = measureTools(DOCS).bytes;
    expect(
      within(fix).getByText(`8 tools · ${docsBytes.toLocaleString('en')} bytes`)
    ).toBeInTheDocument();
    expect(screen.getByTestId('tool-bounds-fix-turn-off-leanzerodocuments')).toHaveTextContent(
      'Turn off for this session'
    );
    const action = screen.getByTestId('tool-bounds-plan-action');
    expect(action).toHaveTextContent('Turn off this one and retry');
    expect(action.className).toContain('bg-lz-accent');
    expect(screen.getByTestId('tool-bounds-retry').className).not.toContain('bg-lz-accent');
    assertStudioClean(container);
  });

  it('every other extension waits behind Show all (N); tool-less ones are a count, not rows', async () => {
    const user = userEvent.setup();
    const { container } = wrap(
      <ToolBoundsNotice bounds={bounds} sessionId={SESSION} live retryText="hi" onRetry={vi.fn()} />
    );
    const all = await screen.findByTestId('tool-bounds-all');
    expect(all.getAttribute('data-state')).toBe('closed');
    const toggle = within(all).getByRole('button', { name: 'Show all extensions (3)' });
    expect(screen.getByTestId('tool-bounds-no-tools')).toHaveTextContent(
      '2 extensions add no tools'
    );
    await user.click(toggle);
    expect(all.getAttribute('data-state')).toBe('open');
    within(all).getByRole('button', { name: 'Hide all extensions (3)' });
    const ranking = screen.getByTestId('tool-bounds-ranking');
    const rows = within(ranking)
      .getAllByRole('listitem')
      .map((li) => li.getAttribute('data-testid'));
    expect(rows).toEqual([
      'tool-bounds-row-leanzerodocuments',
      'tool-bounds-row-playwright',
      'tool-bounds-row-developer',
      'tool-bounds-row-unowned',
    ]);
    expect(screen.queryByTestId('tool-bounds-row-recall')).toBeNull();
    expect(mockListTools).toHaveBeenCalledWith(SESSION, 'leanzerodocuments');
    expect(screen.getByTestId('tool-bounds-raw').textContent).toContain(
      'RAPID_MLX_CONSTRAIN_TOOLS'
    );
    assertStudioClean(container);
  });

  it('Turn off removes the extension from THIS session, refreshes the menu, re-measures; Retry resends', async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    const menuRefresh = vi.fn();
    window.addEventListener(AppEvents.SESSION_EXTENSIONS_LOADED, menuRefresh);
    wrap(
      <ToolBoundsNotice
        bounds={bounds}
        sessionId={SESSION}
        live
        retryText="edit my skill"
        onRetry={onRetry}
      />
    );
    await screen.findByTestId('tool-bounds-fix');
    expect(screen.queryByTestId('tool-bounds-within')).toBeNull();
    expect(screen.getByTestId('tool-bounds-retry').className).not.toContain('bg-lz-accent');

    await user.click(screen.getByTestId('tool-bounds-fix-turn-off-leanzerodocuments'));
    expect(mockRemove).toHaveBeenCalledWith(SESSION, 'leanzerodocuments');
    await screen.findByTestId('tool-bounds-within');
    // The numbers are the re-measured session's, not the refused one's.
    const rest = measureTools([...PLAYWRIGHT, ...DEVELOPER, SCHEDULE]);
    const line = screen.getByTestId('tool-bounds-bytes-line');
    expect(line).toHaveTextContent(
      `${listEnvelopeBytes(rest.tools, rest.bytes).toLocaleString('en')} of 65,536 bytes — within the limit`
    );
    expect(line.className).toContain('text-lz-ok');
    expect(screen.queryByTestId('tool-bounds-bar-over')).toBeNull();
    expect(screen.queryByTestId('tool-bounds-fix')).toBeNull();
    expect(screen.queryByTestId('tool-bounds-row-leanzerodocuments')).toBeNull();
    expect(onRetry).not.toHaveBeenCalled();
    expect(screen.getByTestId('tool-bounds-retry').className).toContain('bg-lz-accent');
    expect(menuRefresh).toHaveBeenCalled();
    window.removeEventListener(AppEvents.SESSION_EXTENSIONS_LOADED, menuRefresh);

    await user.click(screen.getByTestId('tool-bounds-retry'));
    expect(onRetry).toHaveBeenCalledWith('edit my skill');
  });

  it('Turn off these N and retry: turns every one off in THIS session, re-measures, then resends', async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    const menuRefresh = vi.fn();
    window.addEventListener(AppEvents.SESSION_EXTENSIONS_LOADED, menuRefresh);
    const docsList = listEnvelopeBytes(DOCS.length, measureTools(DOCS).bytes);
    // A limit only the two largest together can meet.
    const tight: ToolBounds = { ...bounds, maxBytes: 5_000 };
    expect(docsList).toBeGreaterThan(5_000);
    wrap(
      <ToolBoundsNotice
        bounds={tight}
        sessionId={SESSION}
        live
        retryText="edit my skill"
        onRetry={onRetry}
      />
    );
    const fix = await screen.findByTestId('tool-bounds-fix');
    expect(
      within(fix)
        .getAllByRole('listitem')
        .map((li) => li.getAttribute('data-testid'))
    ).toEqual(['tool-bounds-fix-row-leanzerodocuments', 'tool-bounds-fix-row-playwright']);
    const action = screen.getByTestId('tool-bounds-plan-action');
    expect(action).toHaveTextContent('Turn off these 2 and retry');

    await user.click(action);
    await vi.waitFor(() => expect(onRetry).toHaveBeenCalledWith('edit my skill'));
    expect(mockRemove.mock.calls).toEqual([
      [SESSION, 'leanzerodocuments'],
      [SESSION, 'playwright'],
    ]);
    expect(menuRefresh).toHaveBeenCalledTimes(1);
    window.removeEventListener(AppEvents.SESSION_EXTENSIONS_LOADED, menuRefresh);
    expect(await screen.findByTestId('tool-bounds-within')).toBeInTheDocument();
  });

  it('the set action stops at a refusal, says which, and does not resend', async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();
    mockRemove.mockImplementation(async (_sid, name) => {
      if (name === 'playwright') throw new Error('extension is required');
      delete sessionTools[name];
    });
    wrap(
      <ToolBoundsNotice
        bounds={{ ...bounds, maxBytes: 5_000 }}
        sessionId={SESSION}
        live
        retryText="hi"
        onRetry={onRetry}
      />
    );
    await user.click(await screen.findByTestId('tool-bounds-plan-action'));
    expect(
      await within(screen.getByTestId('tool-bounds-fix-row-playwright')).findByText(
        /Could not turn it off: extension is required/
      )
    ).toBeInTheDocument();
    expect(onRetry).not.toHaveBeenCalled();
    // Re-measured: Documents is gone, so the remaining set is Playwright alone.
    expect(screen.queryByTestId('tool-bounds-fix-row-leanzerodocuments')).toBeNull();
    expect(screen.getByTestId('tool-bounds-plan-action')).toHaveTextContent(
      'Turn off this one and retry'
    );
  });

  it('with no user turn to resend, the set action only turns off', async () => {
    wrap(
      <ToolBoundsNotice
        bounds={bounds}
        sessionId={SESSION}
        live
        retryText={null}
        onRetry={vi.fn()}
      />
    );
    expect(await screen.findByTestId('tool-bounds-plan-action')).toHaveTextContent(
      /^Turn off this one$/
    );
  });

  it('a failed turn-off is stated on its row, and the list stays', async () => {
    const user = userEvent.setup();
    mockRemove.mockRejectedValueOnce(new Error('extension is required'));
    wrap(
      <ToolBoundsNotice
        bounds={bounds}
        sessionId={SESSION}
        live
        retryText={null}
        onRetry={vi.fn()}
      />
    );
    await user.click(await screen.findByRole('button', { name: 'Show all extensions (3)' }));
    await user.click(screen.getByTestId('tool-bounds-turn-off-playwright'));
    expect(
      await within(screen.getByTestId('tool-bounds-row-playwright')).findByText(
        /Could not turn it off: extension is required/
      )
    ).toBeInTheDocument();
  });

  it('when the tool list cannot be read it says so — no ranking, no invented numbers', async () => {
    mockListTools.mockRejectedValue(new Error('session not found'));
    wrap(
      <ToolBoundsNotice bounds={bounds} sessionId={SESSION} live retryText="hi" onRetry={vi.fn()} />
    );
    expect(await screen.findByTestId('tool-bounds-measure-failed')).toHaveTextContent(
      'session not found'
    );
    expect(screen.queryByTestId('tool-bounds-ranking')).toBeNull();
    expect(screen.queryByTestId('tool-bounds-total')).toBeNull();
    expect(screen.queryByTestId('tool-bounds-fix')).toBeNull();
    expect(within(screen.getByTestId('tool-bounds-stated')).getByText('65,536 bytes')).toBeTruthy();
  });

  it('an older notice is a record: no measuring, no Turn off, no Retry; Open MCPs still navigates', async () => {
    const user = userEvent.setup();
    wrap(
      <ToolBoundsNotice
        bounds={bounds}
        sessionId={SESSION}
        live={false}
        retryText="hi"
        onRetry={vi.fn()}
      />
    );
    expect(mockListTools).not.toHaveBeenCalled();
    expect(screen.queryByTestId('tool-bounds-retry')).toBeNull();
    expect(screen.queryByTestId('tool-bounds-plan-action')).toBeNull();
    expect(within(screen.getByTestId('tool-bounds-stated')).getByText('65,536 bytes')).toBeTruthy();
    await user.click(screen.getByTestId('tool-bounds-open-mcps'));
    expect(screen.getByTestId('mcps-view')).toBeInTheDocument();
  });
});

describe('GooseMessage renders the refusal as the notice', () => {
  it('from the message replayed on reopen (the persisted user-only failure, 3a98d9974)', async () => {
    const adapter = createAcpSessionNotificationAdapter();
    adapter.apply({
      sessionId: SESSION,
      update: {
        sessionUpdate: 'user_message_chunk',
        content: { type: 'text', text: 'Start an AI session about my skill' },
        _meta: { goose: { messageId: 'u1', created: 1 } },
      },
    });
    adapter.apply({
      sessionId: SESSION,
      update: {
        sessionUpdate: 'agent_message_chunk',
        content: { type: 'text', text: OWNER_TEXT },
        _meta: { goose: { messageId: 'a1', created: 2 } },
      },
    });
    const messages: Message[] = adapter.getMessages();
    const append = vi.fn();
    const user = userEvent.setup();
    wrap(
      <GooseMessage
        sessionId={SESSION}
        message={messages[1]}
        messages={messages}
        toolCallNotifications={new Map()}
        append={append}
        isStreaming={false}
      />
    );
    expect(screen.getByTestId('tool-bounds-notice')).toBeInTheDocument();
    expect(screen.queryByText(/Please retry if you think/)).toBeNull();
    await screen.findByTestId('tool-bounds-fix');
    await user.click(screen.getByTestId('tool-bounds-retry'));
    expect(append).toHaveBeenCalledWith('Start an AI session about my skill');
  });

  it('while streaming the raw text stays text (the notice renders only for a finished turn)', () => {
    const userTurn = createUserMessage('hi');
    const refusal: Message = {
      id: 'a1',
      role: 'assistant',
      created: 2,
      content: [{ type: 'text', text: OWNER_TEXT }],
      metadata: { userVisible: true, agentVisible: false },
    };
    wrap(
      <GooseMessage
        sessionId={SESSION}
        message={refusal}
        messages={[userTurn, refusal]}
        toolCallNotifications={new Map()}
        append={vi.fn()}
        isStreaming
      />
    );
    expect(screen.queryByTestId('tool-bounds-notice')).toBeNull();
  });
});
