import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { TooltipProvider } from '../ui/Tooltip';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import { CostTracker } from './CostTracker';
import { ContextWindowIndicator } from './ContextWindowIndicator';
import { DirSwitcher } from './DirSwitcher';

/**
 * Studio remake: the bottom-bar readouts sit in quiet Chips (ChatInput), so their own ink must
 * read as META — `text-lz-meta` in `text-lz-ink-3`, tabular figures on every number, the status
 * triad for the context gauge — never `text-text-primary/70 text-xs font-mono`, never an
 * opacity for the busy state. Behaviour untouched: same hooks, handlers and menus.
 */

vi.mock('../../utils/canonical', () => ({
  fetchCanonicalModelInfo: vi.fn(async (provider: string) =>
    provider === 'ollama' ? null : { inputTokenCost: 3, outputTokenCost: 15, currency: '$' }
  ),
}));
vi.mock('./BottomMenuAlertPopover', () => ({
  default: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

const mount = (ui: React.ReactElement) =>
  render(
    <IntlTestWrapper>
      <TooltipProvider>{ui}</TooltipProvider>
    </IntlTestWrapper>
  );

const emitted = (container: HTMLElement) =>
  allClasses(container).filter((c) => !c.startsWith('lucide'));

beforeEach(() => {
  // The vitest setup already installs a non-configurable window.electron stub; extend it in place.
  Object.assign(window.electron as unknown as Record<string, unknown>, {
    getSetting: vi.fn(async () => true),
    listRecentDirs: vi.fn(async () => []),
    listGitWorktreeDirs: vi.fn(async () => []),
    addRecentDir: vi.fn(),
    directoryChooser: vi.fn(() => new Promise(() => {})),
    openDirectoryInExplorer: vi.fn(async () => {}),
  });
});

describe('CostTracker (Studio)', () => {
  it('the cost is the meta step in tabular figures, quiet ink, no mono, no nudge', async () => {
    const { container } = mount(
      <CostTracker
        inputTokens={1200}
        outputTokens={300}
        accumulatedCost={0.1234}
        model="claude-sonnet-4"
        provider="anthropic"
      />
    );
    const value = await screen.findByText('0.12');
    expect(value.className).toContain('text-lz-meta');
    expect(value.className).toContain('tnum');
    expect(value.className).not.toMatch(/font-mono|text-xs/);
    const row = value.parentElement as HTMLElement;
    expect(row.className).toContain('text-lz-ink-3');
    expect(row.className).toContain('hover:text-lz-ink');
    expect(row.className).not.toMatch(/\/70|translate-y|text-text-primary/);
    assertStudioClean(container);
    expect(await missingUtilities(emitted(container))).toEqual([]);
  }, 30_000);

  it('a free provider shows in/out tokens in the same register', async () => {
    const { container } = mount(
      <CostTracker
        inputTokens={1200}
        outputTokens={300}
        accumulatedCost={null}
        model="llama3"
        provider="ollama"
      />
    );
    const value = await screen.findByText(/1,200↑ 300↓/);
    expect(value.className).toContain('text-lz-meta');
    expect(value.className).toContain('tnum');
    expect((value.parentElement as HTMLElement).className).toContain('text-lz-ink-3');
    assertStudioClean(container);
  });
});

describe('ContextWindowIndicator (Studio)', () => {
  const gauge = (totalTokens: number) => {
    const r = mount(
      <ContextWindowIndicator totalTokens={totalTokens} tokenLimit={100_000} alerts={[]} />
    );
    const span = screen.getByText(`${Math.round(totalTokens / 1000)}k / 100k`);
    expect(span.className).toContain('text-lz-meta');
    expect(span.className).toContain('tnum');
    expect(span.className).not.toMatch(/font-mono|text-xs|text-orange|text-red|\/70/);
    assertStudioClean(r.container);
    return { span, unmount: r.unmount };
  };

  it('quiet ink to 75%, the warn tone to 90%, the err tone above', () => {
    let g = gauge(50_000);
    expect(g.span.className).toContain('text-lz-ink-3');
    g.unmount();
    g = gauge(80_000);
    expect(g.span.className).toContain('text-lz-warn');
    g.unmount();
    g = gauge(95_000);
    expect(g.span.className).toContain('text-lz-err');
  });
});

describe('DirSwitcher (Studio)', () => {
  it('the folder readout is the meta step in quiet ink', () => {
    const { container } = mount(
      <DirSwitcher className="" sessionId={undefined} workingDir="/Users/me/proj" />
    );
    const trigger = screen.getByRole('button');
    expect(trigger).toHaveTextContent('proj');
    expect(trigger.className).toContain('text-lz-meta');
    expect(trigger.className).toContain('text-lz-ink-3');
    expect(trigger.className).toContain('hover:text-lz-ink');
    expect(trigger.className).not.toMatch(/opacity|\/70|text-xs|text-text-primary/);
    assertStudioClean(container);
  });

  it('while the chooser is open the trigger is solid ink-4 and not-allowed — never an opacity', async () => {
    const { container } = mount(
      <DirSwitcher className="" sessionId={undefined} workingDir="/Users/me/proj" />
    );
    const trigger = screen.getByRole('button');
    fireEvent.pointerDown(trigger, new PointerEvent('pointerdown', { bubbles: true, button: 0 }));
    fireEvent.click(trigger);
    const choose = await screen.findByText('Choose directory…');
    fireEvent.click(choose);
    await waitFor(() => expect(screen.getByRole('button', { name: /proj/ })).toBeDisabled());
    const busy = screen.getByRole('button', { name: /proj/ });
    expect(busy.className).toContain('text-lz-ink-4');
    expect(busy.className).toContain('cursor-not-allowed');
    expect(busy.className).not.toMatch(/opacity/);
    assertStudioClean(container);
  });
});
