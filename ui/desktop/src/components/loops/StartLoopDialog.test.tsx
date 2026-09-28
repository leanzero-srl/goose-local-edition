import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LoopsStartRequest_unstable } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { GlanceNodesState } from '../engineGlance/glanceStore';
import { loopRecord, waitingRecord } from './railFixtures';
import { StartLoopDialog, swapFacts, type StartLoopFacts } from './StartLoopDialog';
import { TEMPLATES } from './startLoopFixtures';
import { requestStartLoop, type StartLoopRequest } from './startLoopRequest';
import type { SessionLoop } from './useSessionLoop';

const loops = vi.hoisted(() => ({ templates: vi.fn(), start: vi.fn(), update: vi.fn() }));
const nodes = vi.hoisted(() => ({ state: { kind: 'unread' } as GlanceNodesState }));

vi.mock('../../acp/loops', () => ({
  loopsTemplates: loops.templates,
  loopsStart: loops.start,
  loopsUpdate: loops.update,
}));
vi.mock('../engineGlance/glanceStore', () => ({ useGlanceNodes: () => nodes.state }));

const wakelock = { get: vi.fn(), set: vi.fn() };

beforeEach(() => {
  vi.spyOn(Date.prototype, 'getTimezoneOffset').mockReturnValue(0);
  loops.templates.mockReset().mockResolvedValue({ templates: TEMPLATES });
  loops.start.mockReset();
  loops.update.mockReset();
  nodes.state = { kind: 'unread' };
  wakelock.get.mockReset().mockResolvedValue({ enabled: false, holding: false, error: null });
  wakelock.set
    .mockReset()
    .mockImplementation(async (enabled: boolean) => ({ enabled, holding: enabled, error: null }));
  window.electron = {
    ...window.electron,
    getWakelockState: wakelock.get,
    setWakelock: wakelock.set,
  } as typeof window.electron;
});
afterEach(() => vi.restoreAllMocks());

function mount(facts: Partial<StartLoopFacts> = {}, sessionId = 's1') {
  const onChanged = vi.fn();
  const view = render(
    <IntlTestWrapper>
      <StartLoopDialog
        sessionId={sessionId}
        workingDir="/w"
        swarmBuild={false}
        servedLabel="27B · Mihai Macbook and Work’s Mac Studio"
        chatProvider="swarm"
        chatModel="swarm"
        current={{ kind: 'none' }}
        onChanged={onChanged}
        {...facts}
      />
    </IntlTestWrapper>
  );
  return { ...view, onChanged };
}

async function open(request: Partial<StartLoopRequest> = {}) {
  let taken = false;
  act(() => {
    taken = requestStartLoop({ sessionId: 's1', mode: 'start', ...request });
  });
  expect(taken).toBe(true);
  const dialog = await screen.findByTestId('start-loop-dialog');
  await waitFor(() => expect(within(dialog).getByTestId('loop-goal')).toBeInTheDocument());
  return dialog;
}

const startButton = () => screen.getByTestId('loop-start');
const typeGoal = (text: string) =>
  fireEvent.change(screen.getByTestId('loop-goal'), { target: { value: text } });

describe('StartLoopDialog', () => {
  it('opens only for its own chat, on the Software quality loop with goosed’s steps', async () => {
    mount();
    let taken = true;
    act(() => {
      taken = requestStartLoop({ sessionId: 'other', mode: 'start' });
    });
    expect(taken).toBe(false);
    expect(screen.queryByTestId('start-loop-dialog')).not.toBeInTheDocument();

    const dialog = await open();
    expect(within(dialog).getByRole('heading', { name: 'Loop this chat' })).toBeInTheDocument();
    expect(within(dialog).getByTestId('loop-template-quality')).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(within(dialog).getByTestId('loop-template-description')).toHaveTextContent(
      "Discover what's broken, pick the one thing that matters most"
    );
    expect(screen.getByTestId('loop-steps')).toHaveValue(TEMPLATES[0].steps);
    expect(screen.getByTestId('loop-cadence-every')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('loop-every-10m')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('loop-first-tick-now')).toHaveTextContent('The first tick runs now.');
    expect(screen.getByTestId('loop-cost-line')).toHaveTextContent(
      'Each tick is one turn on 27B · Mihai Macbook and Work’s Mac Studio. Your turns in this window always go first'
    );
    expect(screen.getByTestId('loop-sleep-line')).toHaveTextContent(
      'Your Mac may sleep; ticks wait until it wakes.'
    );
  });

  it('refuses to start without a goal, and says so under the field', async () => {
    mount();
    await open();
    expect(startButton()).toBeDisabled();
    expect(screen.getByTestId('loop-goal-empty')).toHaveTextContent('Say what the loop should do.');
    expect(screen.getByTestId('loop-start-blocked')).toHaveTextContent(
      'Say what the loop should do.'
    );
    typeGoal('Make every test in ui/desktop pass');
    expect(screen.queryByTestId('loop-goal-empty')).not.toBeInTheDocument();
    expect(screen.queryByTestId('loop-start-blocked')).not.toBeInTheDocument();
    expect(startButton()).toBeEnabled();
    expect(screen.getByTestId('loop-state-file')).toHaveValue(
      '.goose/loops/make-every-test-in/NOW.md'
    );
  });

  it('shows the steps as goose reads them: each slot filled with this loop’s fact', async () => {
    mount();
    await open({ goal: 'Make the generator produce every class' });
    fireEvent.change(screen.getByTestId('loop-check'), {
      target: { value: 'node scripts/validate_users.js' },
    });
    const slots = within(screen.getByTestId('loop-steps-preview')).getAllByTestId('loop-slot');
    const facts = Object.fromEntries(slots.map((s) => [s.dataset.slot, s.textContent]));
    expect(facts).toEqual({
      state_file: '.goose/loops/make-the-generator-produce/NOW.md',
      goal_first_line: 'Make the generator produce every class',
      working_dir: '/w',
      last_next_step: 'this is the first tick',
      check: 'node scripts/validate_users.js',
    });
  });

  it('flags a slot goose does not know and refuses to start with it', async () => {
    mount();
    await open({ goal: 'Ship it' });
    fireEvent.change(screen.getByTestId('loop-steps'), {
      target: { value: 'Read {state_file} and {foo}' },
    });
    const unknown = screen.getByTestId('loop-slot-unknown');
    expect(unknown).toHaveTextContent('{foo}');
    expect(screen.getAllByText('{foo} is not a fact goose knows')).toHaveLength(2);
    expect(startButton()).toBeDisabled();
  });

  it('maps every cadence choice to the one grammar, and refuses a custom value it cannot read', async () => {
    loops.start.mockResolvedValue({ loop: waitingRecord() });
    mount();
    await open({ goal: 'Ship it' });
    fireEvent.click(screen.getByTestId('loop-every-30m'));
    fireEvent.click(startButton());
    await waitFor(() => expect(loops.start).toHaveBeenCalledTimes(1));
    const sent = loops.start.mock.calls[0][0] as LoopsStartRequest_unstable;
    expect(sent).toMatchObject({
      sessionId: 's1',
      goal: 'Ship it',
      template: 'quality',
      cadence: { kind: 'every', every: '30m' },
      stateFile: '.goose/loops/ship-it/NOW.md',
      steps: TEMPLATES[0].steps,
    });
    expect(sent.check).toBeUndefined();

    await open({ goal: 'Ship it' });
    fireEvent.click(screen.getByTestId('loop-every-custom'));
    fireEvent.change(screen.getByTestId('loop-custom-every'), { target: { value: 'soon' } });
    expect(screen.getByTestId('loop-cadence-error')).toHaveTextContent(
      'Use a number and s, m or h — 90m, 2h'
    );
    expect(startButton()).toBeDisabled();
    fireEvent.change(screen.getByTestId('loop-custom-every'), { target: { value: '90m' } });
    fireEvent.click(screen.getByTestId('loop-cadence-self_paced'));
    expect(
      screen.getByText(
        "After each tick goose names when to come back and why — you'll see its reason."
      )
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('loop-cadence-back_to_back'));
    expect(screen.getByText('The next tick starts as soon as one ends.')).toBeInTheDocument();
    fireEvent.click(startButton());
    await waitFor(() => expect(loops.start).toHaveBeenCalledTimes(2));
    expect(loops.start.mock.calls[1][0]).toMatchObject({ cadence: { kind: 'back_to_back' } });
  });

  it('Until a check passes needs a command, and Stop when names what ends the loop', async () => {
    mount();
    await open({ goal: 'Make pnpm test pass' });
    expect(screen.getByText('goose reports the goal is done')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('loop-template-until_check'));
    expect(screen.getByTestId('loop-steps')).toHaveValue(TEMPLATES[1].steps);
    expect(screen.getByTestId('loop-cadence-back_to_back')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('loop-check-error')).toHaveTextContent(
      'This template needs a command to check.'
    );
    expect(startButton()).toBeDisabled();
    // Said beside the disabled Start too: the check field may be scrolled out of view.
    expect(screen.getByTestId('loop-start-blocked')).toHaveTextContent(
      'This template needs a command to check.'
    );
    fireEvent.change(screen.getByTestId('loop-check'), { target: { value: 'pnpm test' } });
    expect(startButton()).toBeEnabled();
    expect(screen.getByText('the check succeeds after goose reports done')).toBeInTheDocument();
    expect(screen.getByText('you stop it')).toBeInTheDocument();
    expect(screen.getByText(/Runs in \/w after every tick/)).toBeInTheDocument();
  });

  it('keeps the state file inside the working dir and the tick count a whole number', async () => {
    mount();
    await open({ goal: 'Ship it' });
    fireEvent.change(screen.getByTestId('loop-state-file'), { target: { value: '../NOW.md' } });
    expect(screen.getByTestId('loop-state-file-error')).toHaveTextContent(
      'Keep the state file inside /w.'
    );
    expect(startButton()).toBeDisabled();
    fireEvent.change(screen.getByTestId('loop-state-file'), { target: { value: 'notes/NOW.md' } });
    expect(startButton()).toBeEnabled();
    fireEvent.change(screen.getByTestId('loop-stop-after'), { target: { value: 'five' } });
    expect(screen.getByTestId('loop-stop-after-error')).toHaveTextContent(
      'Use a whole number of ticks, 1 or more.'
    );
    expect(startButton()).toBeDisabled();
    fireEvent.change(screen.getByTestId('loop-stop-after'), { target: { value: '0' } });
    expect(screen.getByTestId('loop-stop-after-error')).toHaveTextContent(
      'Stop after needs at least one tick'
    );
    fireEvent.change(screen.getByTestId('loop-stop-after'), { target: { value: '5' } });
    expect(startButton()).toBeEnabled();
  });

  it('shows the runner’s refusal in its own words and stays open', async () => {
    loops.start.mockResolvedValue({
      refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
    });
    const { onChanged } = mount();
    await open({ goal: 'Ship it' });
    fireEvent.click(startButton());
    expect(await screen.findByTestId('loop-start-said')).toHaveTextContent(
      'goose refused: The loop runner is not in this build'
    );
    expect(screen.getByTestId('start-loop-dialog')).toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();

    loops.start.mockRejectedValue(new Error('socket closed'));
    fireEvent.click(startButton());
    expect(await screen.findByText('The request failed: socket closed')).toBeInTheDocument();
  });

  it('a started loop closes the dialog and reads the loop again', async () => {
    loops.start.mockResolvedValue({ loop: waitingRecord() });
    const { onChanged } = mount();
    await open({ goal: 'Ship it' });
    fireEvent.click(startButton());
    await waitFor(() => expect(screen.queryByTestId('start-loop-dialog')).not.toBeInTheDocument());
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('asks before replacing a live loop, and Keep it starts nothing', async () => {
    loops.start.mockResolvedValue({ loop: waitingRecord() });
    const current: SessionLoop = { kind: 'loop', loop: loopRecord(), status: 'running' };
    mount({ current });
    await open({ goal: 'A different goal' });
    fireEvent.click(startButton());
    const confirm = await screen.findByTestId('loop-replace-dialog');
    expect(confirm).toHaveTextContent('Replace the loop?');
    expect(confirm).toHaveTextContent('The current loop ends after tick 5.');
    fireEvent.click(screen.getByTestId('loop-replace-keep'));
    await waitFor(() =>
      expect(screen.queryByTestId('loop-replace-dialog')).not.toBeInTheDocument()
    );
    expect(loops.start).not.toHaveBeenCalled();
    fireEvent.click(startButton());
    fireEvent.click(await screen.findByTestId('loop-replace'));
    await waitFor(() => expect(loops.start).toHaveBeenCalledTimes(1));
  });

  it('an ended loop is not asked about, and Start a new loop opens prefilled from it', async () => {
    loops.start.mockResolvedValue({ loop: waitingRecord() });
    const ended = loopRecord({ status: 'ended', stopAfterTicks: 8 });
    mount({ current: { kind: 'loop', loop: ended, status: 'ended' } });
    await open({ from: ended });
    expect(screen.getByTestId('loop-goal')).toHaveValue(ended.goal);
    expect(screen.getByTestId('loop-check')).toHaveValue(ended.check);
    expect(screen.getByTestId('loop-stop-after')).toHaveValue('8');
    fireEvent.click(startButton());
    await waitFor(() => expect(loops.start).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId('loop-replace-dialog')).not.toBeInTheDocument();
  });

  it('Edit saves the loop in place and starts no tick', async () => {
    loops.update.mockResolvedValue({ loop: waitingRecord() });
    const loop = waitingRecord();
    mount({ current: { kind: 'loop', loop, status: 'waiting' } });
    act(() => {
      requestStartLoop({ sessionId: 's1', mode: 'edit', from: loop });
    });
    const dialog = await screen.findByTestId('start-loop-dialog');
    expect(within(dialog).getByRole('heading', { name: 'Edit loop' })).toBeInTheDocument();
    expect(screen.queryByTestId('loop-first-tick-now')).not.toBeInTheDocument();
    expect(screen.getByTestId('loop-start')).toHaveTextContent('Save');
    fireEvent.change(screen.getByTestId('loop-check'), { target: { value: 'pnpm test' } });
    fireEvent.click(startButton());
    await waitFor(() => expect(loops.update).toHaveBeenCalledTimes(1));
    expect(loops.update.mock.calls[0][0]).toBe('s1');
    expect(loops.update.mock.calls[0][1]).toMatchObject({ check: 'pnpm test', goal: loop.goal });
    expect(loops.start).not.toHaveBeenCalled();
  });

  it('a swarm-build chat cannot start one, and the dialog says why', async () => {
    mount({ swarmBuild: true });
    await open({ goal: 'Ship it' });
    expect(screen.getByTestId('loop-start-blocked')).toHaveTextContent(
      'Loops run chat turns. This chat builds with the swarm, so every tick would start a full build.'
    );
    expect(startButton()).toBeDisabled();
  });

  it('the Keep-awake toggle is the Prevent Sleep setting, and says when it is not working', async () => {
    mount();
    await open({ goal: 'Ship it' });
    const toggle = screen.getByTestId('loop-keep-awake');
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).toHaveAttribute('aria-checked', 'false');
    fireEvent.click(toggle);
    await waitFor(() => expect(wakelock.set).toHaveBeenCalledWith(true));
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'));

    wakelock.set.mockResolvedValueOnce({
      enabled: false,
      holding: false,
      error: 'could not release power save blocker 3: gone',
    });
    fireEvent.click(toggle);
    expect(await screen.findByTestId('loop-keep-awake-failed')).toHaveTextContent(
      'Keep awake is not working: could not release power save blocker 3: gone'
    );
  });

  it('names a template read that failed, and reads again on Try again', async () => {
    loops.templates.mockRejectedValueOnce(new Error('goosed is gone'));
    mount();
    act(() => {
      requestStartLoop({ sessionId: 's1', mode: 'start' });
    });
    expect(await screen.findByTestId('loop-templates-failed')).toHaveTextContent(
      'The templates could not be read: goosed is gone'
    );
    expect(startButton()).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(await screen.findByTestId('loop-goal')).toBeInTheDocument();
  });

  it('says a tick may swap the engine only when the nodes say so', () => {
    const read = (residency: object): GlanceNodesState =>
      ({
        kind: 'read',
        servedNode: null,
        read: { nodes: [{ def: { id: 'studio-27b', name: '27B on the Studio' } }] },
        residency: { nodes: [{ node: 'studio-27b', residency }], loaderInstalled: true },
      }) as unknown as GlanceNodesState;
    expect(
      swapFacts(read({ kind: 'notRunning', otherWay: 'the split' }), 'swarm', 'node:studio-27b')
    ).toEqual({ node: '27B on the Studio', way: 'the split' });
    expect(swapFacts(read({ kind: 'serving' }), 'swarm', 'node:studio-27b')).toBeNull();
    expect(swapFacts(read({ kind: 'notRunning' }), 'swarm', 'node:studio-27b')).toBeNull();
    expect(swapFacts(read({ kind: 'notRunning', otherWay: 'x' }), 'swarm', 'swarm')).toBeNull();
    expect(swapFacts({ kind: 'unread' }, 'swarm', 'node:studio-27b')).toBeNull();
  });
});
