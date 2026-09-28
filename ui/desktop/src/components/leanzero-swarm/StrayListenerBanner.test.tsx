import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { StrayListenerBanner, shortCommand, strayStep } from './StrayListenerBanner';
import type { MlxEngineStatus, MlxStrayListenerHolder } from '../../acp/mlx-engine';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * The Q-240 shapes as goose-sidecar's status reports them (engine.rs `StrayListenerHolder`): the
 * live engine measured 2026-09-28 — Rapid-MLX's python under `uv tool uvx` on 8090.
 */
const ENGINE_ARGV = [
  '/Users/me/.cache/uv/archive-v0/U_t/bin/python',
  '/Users/me/.cache/uv/archive-v0/U_t/bin/rapid-mlx',
  'serve',
  '/Users/me/.lmstudio/models/mlx-community/Qwen3.6-27B-4bit',
  '--port',
  '8090',
];

const liveStarter: MlxStrayListenerHolder = {
  pid: 35319,
  argv: ENGINE_ARGV,
  ours: false,
  notOursRule: 'liveStarter',
  notOursReason:
    'the process that started it, pid 73403 (`goose serve`), is alive — another goose on this Mac, or a shell, runs it',
  liveStarterPid: 73403,
  liveStarterArgv: ['/Applications/Goose.app/Contents/Resources/bin/goosed', 'serve'],
};

const unmarked: MlxStrayListenerHolder = {
  pid: 35319,
  argv: ENGINE_ARGV,
  ours: false,
  notOursRule: 'noMarker',
  notOursReason:
    'pid 35319 carries no GOOSE_SIDECAR in its environment — every engine a goose sidecar starts carries GOOSE_SIDECAR=mlx-engine@http://127.0.0.1:8090 (a goose older than this check stamped none)',
};

const leftover: MlxStrayListenerHolder = { pid: 35319, argv: ENGINE_ARGV, ours: true };

type Holders = Pick<MlxEngineStatus, 'strayListenerHolders' | 'strayListenerHoldersError'>;

function banner(status: Holders) {
  return render(
    <IntlTestWrapper>
      <StrayListenerBanner port={8090} status={status} />
    </IntlTestWrapper>
  );
}

async function expectDesigned(container: HTMLElement) {
  assertStudioClean(container);
  const utilities = allClasses(container).filter((c) => !c.startsWith('lucide'));
  expect(await missingUtilities(utilities)).toEqual([]);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('StrayListenerBanner (Q-249)', () => {
  it('names the holder, says it is not this goose’s, and gives Q-240’s step — quit what started it, no kill', async () => {
    const { container } = banner({ strayListenerHolders: [liveStarter] });
    const card = screen.getByTestId('stray-listener');
    expect(card).toHaveTextContent('Port 8090 is taken');
    const row = screen.getByTestId('stray-listener-holder');
    expect(row).toHaveAttribute('data-ours', 'false');
    expect(row).toHaveTextContent(
      'pid 35319 · python rapid-mlx serve Qwen3.6-27B-4bit --port 8090'
    );
    expect(row).toHaveTextContent(
      "Not this goose's — pid 73403 (goosed serve) started it and still runs it: another goose on this Mac, or a terminal"
    );
    // The full command line and the sidecar's own finding ride the titles.
    expect(within(row).getByTitle(ENGINE_ARGV.join(' '))).toBeInTheDocument();
    expect(within(row).getByTitle(liveStarter.notOursReason as string)).toBeInTheDocument();
    expect(screen.getByTestId('stray-listener-step')).toHaveTextContent(
      'Next: quit what started it (pid 73403), then start again in Run it.'
    );
    // A live starter's engine is never the thing to kill (Q-240's refusal says the same).
    expect(screen.queryByTestId('stray-listener-command')).toBeNull();
    expect(card).not.toHaveTextContent('kill');
    await expectDesigned(container);
  });

  it('an engine no goose marked: "kill <pid>" with a copy button that copies exactly it', async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
    const { container } = banner({ strayListenerHolders: [unmarked] });
    expect(screen.getByTestId('stray-listener-holder')).toHaveTextContent(
      "Not this goose's — it carries no goose engine mark (an older goose, or another program, started it)"
    );
    expect(screen.getByTestId('stray-listener-step')).toHaveTextContent(
      'Next: stop it with this command, then start again in Run it.'
    );
    expect(screen.getByTestId('stray-listener-command')).toHaveTextContent(/^kill 35319$/);
    const copy = screen.getByRole('button', { name: 'Copy the command kill 35319' });
    expect(copy).toHaveTextContent('Copy');
    await user.click(copy);
    expect(writeText).toHaveBeenCalledWith('kill 35319');
    expect(copy).toHaveTextContent('Copied');
    await expectDesigned(container);
  });

  it('this goose’s own leftover: said as ours, and the step is a start — nothing to kill', () => {
    banner({ strayListenerHolders: [leftover] });
    const row = screen.getByTestId('stray-listener-holder');
    expect(row).toHaveAttribute('data-ours', 'true');
    expect(row).toHaveTextContent(
      "This goose's own engine, left from an earlier run — nothing runs it now"
    );
    expect(screen.getByTestId('stray-listener')).toHaveAttribute('data-step', 'mount');
    expect(screen.getByTestId('stray-listener-step')).toHaveTextContent(
      'Next: start it again in Run it — the start stops this leftover first.'
    );
    expect(screen.queryByTestId('stray-listener-command')).toBeNull();
  });

  it('holders that could not be read say why, and that Unmount stops whatever listens', () => {
    banner({
      strayListenerHolders: null,
      strayListenerHoldersError: 'lsof not found; searched 3 location(s)',
    });
    const card = screen.getByTestId('stray-listener');
    expect(card).toHaveTextContent(
      'Who holds port 8090 could not be read (lsof not found; searched 3 location(s)). Unmount stops whatever listens on it.'
    );
    expect(screen.queryByTestId('stray-listener-holder')).toBeNull();
    expect(screen.queryByTestId('stray-listener-step')).toBeNull();
  });

  it('a port that answers with no listener named, and a goose before Q-249, each say what is known', () => {
    const { unmount } = banner({ strayListenerHolders: [] });
    expect(screen.getByTestId('stray-listener')).toHaveTextContent(
      'Something answers on port 8090, but no process listens there now — it may have just exited.'
    );
    unmount();
    banner({});
    expect(screen.getByTestId('stray-listener')).toHaveTextContent(
      'A process this goose does not run listens on port 8090 — Unmount stops it.'
    );
  });
});

describe('strayStep — the one next step, in Q-240’s order', () => {
  it('a live starter wins; this goosed on its own port asks for another port; ours are left to Mount', () => {
    expect(strayStep([unmarked, liveStarter])).toEqual({ kind: 'quit', pid: 73403 });
    expect(
      strayStep([{ pid: 999, argv: ['goosed'], ours: false, notOursRule: 'initOrSelf' }])
    ).toEqual({ kind: 'otherPort' });
    expect(strayStep([leftover, { ...unmarked, pid: 35400 }])).toEqual({
      kind: 'kill',
      command: 'kill 35400',
    });
    expect(strayStep([{ ...unmarked, notOursRule: 'unreadable', argv: [] }])).toEqual({
      kind: 'kill',
      command: 'kill 35319',
    });
    expect(strayStep([])).toBeNull();
  });

  it('shortCommand keeps the words and drops the paths, within a line', () => {
    expect(shortCommand(['/opt/homebrew/bin/uv', 'tool', 'uvx', 'rapid-mlx'])).toBe(
      'uv tool uvx rapid-mlx'
    );
    const long = shortCommand(['python', 'x'.repeat(200)]);
    expect(long.length).toBeLessThanOrEqual(72);
    expect(long.endsWith('…')).toBe(true);
  });
});
