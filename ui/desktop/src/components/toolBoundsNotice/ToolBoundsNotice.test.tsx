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
  withinBounds,
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
  sessionTools = { leanzerodocuments: DOCS, playwright: PLAYWRIGHT, developer: DEVELOPER };
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

describe('ToolBoundsNotice', () => {
  const bounds = parseToolBoundsError(OWNER_TEXT)!;

  it('ranks the session’s extensions by engine bytes, names the unowned tools, clean of bans', async () => {
    const { container } = wrap(
      <ToolBoundsNotice bounds={bounds} sessionId={SESSION} live retryText="hi" onRetry={vi.fn()} />
    );
    const ranking = await screen.findByTestId('tool-bounds-ranking');
    const rows = within(ranking)
      .getAllByRole('listitem')
      .map((li) => li.getAttribute('data-testid'));
    expect(rows).toEqual([
      'tool-bounds-row-leanzerodocuments',
      'tool-bounds-row-playwright',
      'tool-bounds-row-developer',
      'tool-bounds-row-unowned',
    ]);
    const docsBytes = measureTools(DOCS).bytes;
    expect(
      within(screen.getByTestId('tool-bounds-row-leanzerodocuments')).getByText(
        `8 tools · ${docsBytes.toLocaleString('en')} bytes`
      )
    ).toBeInTheDocument();
    expect(within(screen.getByTestId('tool-bounds-stated')).getByText('65,536 bytes')).toBeTruthy();
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
    await screen.findByTestId('tool-bounds-ranking');
    expect(screen.queryByTestId('tool-bounds-within')).toBeNull();
    expect(screen.getByTestId('tool-bounds-retry').className).not.toContain('bg-lz-accent');
    const overBytes = within(screen.getByTestId('tool-bounds-total')).getByText(/bytes$/);
    expect(overBytes.closest('[class*="bg-lz-err-solid"]')).not.toBeNull();

    await user.click(screen.getByTestId('tool-bounds-turn-off-leanzerodocuments'));
    expect(mockRemove).toHaveBeenCalledWith(SESSION, 'leanzerodocuments');
    await screen.findByTestId('tool-bounds-within');
    expect(screen.queryByTestId('tool-bounds-row-leanzerodocuments')).toBeNull();
    expect(screen.getByTestId('tool-bounds-retry').className).toContain('bg-lz-accent');
    expect(menuRefresh).toHaveBeenCalled();
    window.removeEventListener(AppEvents.SESSION_EXTENSIONS_LOADED, menuRefresh);

    await user.click(screen.getByTestId('tool-bounds-retry'));
    expect(onRetry).toHaveBeenCalledWith('edit my skill');
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
    await screen.findByTestId('tool-bounds-ranking');
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
    await screen.findByTestId('tool-bounds-ranking');
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
