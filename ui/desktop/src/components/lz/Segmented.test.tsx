import { fireEvent, render, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { Segmented } from './Segmented';
import { SURFACE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

const options = [
  { value: 'all', label: 'All' },
  { value: 'live', label: 'Live' },
  { value: 'gone', label: 'Gone', disabled: true },
  { value: 'done', label: 'Done' },
] as const;

describe('lz/Segmented', () => {
  it('is a radiogroup whose active segment is the accent fill with white ink', () => {
    const { container, getAllByRole, getByRole } = render(
      <Segmented aria-label="Lane filter" options={options} value="live" onChange={() => {}} />
    );
    expect(getByRole('radiogroup').getAttribute('aria-label')).toBe('Lane filter');
    const radios = getAllByRole('radio');
    expect(radios).toHaveLength(4);
    const active = radios[1];
    expect(active.getAttribute('aria-checked')).toBe('true');
    for (const c of SURFACE.selected.split(' ')) expect(active.className).toContain(c);
    expect(radios[0].className).not.toContain('bg-lz-accent');
    expect(radios[0].className).toContain('hover:bg-lz-surface-2');
    // roving tabindex: only the active segment is in the tab order
    expect(radios.map((r) => r.getAttribute('tabindex'))).toEqual(['-1', '0', '-1', '-1']);
    expect((radios[2] as HTMLButtonElement).disabled).toBe(true);
    assertStudioClean(container);
  });

  it('clicks select, arrows move and skip disabled segments, Home/End jump', () => {
    const onChange = vi.fn();
    const { getByRole, getAllByRole } = render(
      <Segmented aria-label="f" options={options} value="live" onChange={onChange} />
    );
    fireEvent.click(getAllByRole('radio')[0]);
    expect(onChange).toHaveBeenLastCalledWith('all');
    const group = getByRole('radiogroup');
    fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('done'); // skipped the disabled "gone"
    fireEvent.keyDown(group, { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('all');
    fireEvent.keyDown(group, { key: 'End' });
    expect(onChange).toHaveBeenLastCalledWith('done');
    fireEvent.keyDown(group, { key: 'Home' });
    expect(onChange).toHaveBeenLastCalledWith('all');
    expect(onChange).toHaveBeenCalledTimes(5);
  });
});

describe('lz/Segmented — per-option slots', () => {
  it('carries title, aria-describedby, id and data-testid onto the segment', () => {
    const { getAllByRole } = render(
      <Segmented
        aria-label="Nodes"
        options={[
          {
            value: '1',
            label: '1',
            title: 'Run on 1 node',
            describedBy: 'why-1',
            id: 'node-1',
            testId: 'node-one',
          },
          { value: '2', label: '2' },
        ]}
        value="2"
        onChange={() => {}}
      />
    );
    const [one, two] = getAllByRole('radio');
    expect(one.getAttribute('title')).toBe('Run on 1 node');
    expect(one.getAttribute('aria-describedby')).toBe('why-1');
    expect(one.id).toBe('node-1');
    expect(one.getAttribute('data-testid')).toBe('node-one');
    expect(two.getAttribute('title')).toBeNull();
    expect(two.getAttribute('aria-describedby')).toBeNull();
    expect(two.id).toBe('');
    expect(two.getAttribute('data-testid')).toBeNull();
  });

  it('a group-level disabled locks every segment; the active one KEEPS the accent fill, the rest take the solid neutral', () => {
    const { container, getAllByRole } = render(
      <Segmented aria-label="f" options={options} value="live" onChange={() => {}} disabled />
    );
    const radios = getAllByRole('radio') as HTMLButtonElement[];
    expect(radios.every((r) => r.disabled)).toBe(true);
    const active = radios[1];
    for (const c of SURFACE.selected.split(' ')) expect(active.className).toContain(c);
    expect(active.className).not.toContain('disabled:bg-lz-surface-2');
    expect(active.className).toContain('disabled:pointer-events-none');
    expect(radios[0].className).toContain('disabled:bg-lz-surface-2');
    expect(radios[0].className).toContain('disabled:text-lz-ink-3');
    expect(container.innerHTML).not.toMatch(/opacity/);
    assertStudioClean(container);
  });

  it('forwards its ref to the strip and spreads unknown props onto it', () => {
    const ref = { current: null as HTMLDivElement | null };
    const { getByTestId } = render(
      <Segmented
        ref={ref}
        id="strip"
        data-orientation="horizontal"
        aria-label="f"
        options={options}
        value="all"
        onChange={() => {}}
      />
    );
    const strip = getByTestId('lz-segmented');
    expect(ref.current).toBe(strip);
    expect(strip.id).toBe('strip');
    expect(strip.getAttribute('data-orientation')).toBe('horizontal');
    expect(strip.getAttribute('role')).toBe('radiogroup');
  });
});

describe('lz/Segmented as="buttons"', () => {
  const nodeOptions = (running: boolean) =>
    (['1', '2', '3'] as const).map((n) => ({
      value: n,
      label: n,
      title: running ? 'Locked while a run is live' : `Run on ${n} node${n === '1' ? '' : 's'}`,
      describedBy: running ? 'locked' : undefined,
    }));

  it('is a role=group of aria-pressed buttons, every one in the tab order, no radio semantics', () => {
    const onChange = vi.fn();
    const { container, getByRole, getAllByRole, queryByRole } = render(
      <Segmented
        as="buttons"
        aria-label="Nodes"
        options={nodeOptions(false)}
        value="3"
        onChange={onChange}
      />
    );
    expect(getByRole('group').getAttribute('aria-label')).toBe('Nodes');
    expect(queryByRole('radiogroup')).toBeNull();
    expect(queryByRole('radio')).toBeNull();
    const buttons = getAllByRole('button');
    expect(buttons).toHaveLength(3);
    expect(buttons.map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'false', 'true']);
    expect(buttons.map((b) => b.getAttribute('tabindex'))).toEqual([null, null, null]);
    for (const c of SURFACE.selected.split(' ')) expect(buttons[2].className).toContain(c);
    expect(getByRole('button', { name: '1' }).getAttribute('title')).toBe('Run on 1 node');
    expect(getByRole('button', { name: '1' }).getAttribute('aria-describedby')).toBeNull();
    fireEvent.click(buttons[0]);
    expect(onChange).toHaveBeenLastCalledWith('1');
    // No arrow handling in buttons mode — the keys do nothing.
    fireEvent.keyDown(getByRole('group'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenCalledTimes(1);
    assertStudioClean(container);
  });

  it('locked: every button disabled, each still saying why through title and aria-describedby; the selection stays readable', () => {
    const { getByRole } = render(
      <>
        <span id="locked">locked while the run is live</span>
        <Segmented
          as="buttons"
          aria-label="Nodes"
          options={nodeOptions(true)}
          value="3"
          onChange={() => {}}
          disabled
        />
      </>
    );
    const two = getByRole('button', { name: '2' });
    const three = getByRole('button', { name: '3' });
    expect(two).toBeDisabled();
    expect(three).toBeDisabled();
    expect(two.getAttribute('title')).toMatch(/Locked while a run is live/);
    expect(document.getElementById(two.getAttribute('aria-describedby')!)?.textContent).toMatch(
      /locked while the run is live/
    );
    expect(three.className).toContain('bg-lz-accent');
    expect(three.className).not.toMatch(/opacity/);
    expect(two.className).not.toContain('bg-lz-accent');
  });
});

describe('lz/Segmented as="tabs"', () => {
  it('without renderOption: role=tablist over role=tab buttons with aria-selected, data-state and roving focus', () => {
    const onChange = vi.fn();
    const { container, getByRole, getAllByRole } = render(
      <Segmented
        as="tabs"
        aria-label="Settings"
        options={options}
        value="live"
        onChange={onChange}
      />
    );
    expect(getByRole('tablist').getAttribute('aria-label')).toBe('Settings');
    const tabs = getAllByRole('tab');
    expect(tabs).toHaveLength(4);
    expect(tabs.map((t) => t.getAttribute('aria-selected'))).toEqual([
      'false',
      'true',
      'false',
      'false',
    ]);
    expect(tabs.map((t) => t.getAttribute('data-state'))).toEqual([
      'inactive',
      'active',
      'inactive',
      'inactive',
    ]);
    expect(tabs.map((t) => t.getAttribute('tabindex'))).toEqual(['-1', '0', '-1', '-1']);
    for (const c of SURFACE.selected.split(' ')) expect(tabs[1].className).toContain(c);
    fireEvent.keyDown(getByRole('tablist'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('done');
    assertStudioClean(container);
  });

  it('with renderOption: the slot receives the option, its active state, the recipe and the content', () => {
    const onChange = vi.fn();
    const seen: Array<{ value: string; active: boolean; disabled: boolean }> = [];
    const { getByRole, getAllByRole, getByTestId } = render(
      <Segmented
        as="tabs"
        aria-label="Settings"
        options={[
          {
            value: 'chat',
            label: 'Chat',
            icon: <svg data-testid="chat-icon" />,
            testId: 'settings-chat-tab',
          },
          { value: 'app', label: 'App', disabled: true, testId: 'settings-app-tab' },
        ]}
        value="chat"
        onChange={onChange}
        renderOption={({ option, active, disabled, className, content, select }) => {
          seen.push({ value: option.value, active, disabled });
          return (
            <button
              type="button"
              role="tab"
              aria-selected={active}
              data-testid={option.testId}
              className={className}
              onClick={select}
            >
              {content}
            </button>
          );
        }}
      />
    );
    expect(seen).toEqual([
      { value: 'chat', active: true, disabled: false },
      { value: 'app', active: false, disabled: true },
    ]);
    expect(getByRole('tablist')).toBe(getByTestId('lz-segmented'));
    const [chat, app] = getAllByRole('tab');
    expect(chat.getAttribute('data-testid')).toBe('settings-chat-tab');
    for (const c of SURFACE.selected.split(' ')) expect(chat.className).toContain(c);
    expect(chat.className).toContain('font-lz-medium');
    expect(app.className).toContain('hover:bg-lz-surface-2');
    expect(app.className).not.toContain('bg-lz-accent');
    // The content is the icon (aria-hidden) and the label, as the primitive renders them.
    expect(getByTestId('chat-icon').parentElement?.getAttribute('aria-hidden')).toBe('true');
    expect(chat.textContent).toBe('Chat');
    fireEvent.click(app);
    expect(onChange).toHaveBeenLastCalledWith('app');
    // The caller's tab machinery owns focus: the primitive attaches no arrow handling.
    fireEvent.keyDown(getByRole('tablist'), { key: 'ArrowRight' });
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('wears a Radix Tabs.List through asChild: tablist semantics, data-state, and arrow keys move focus through the forwarded ref', async () => {
    const Tabs = await import('@radix-ui/react-tabs');
    function Harness() {
      const [tab, setTab] = useState('chat');
      return (
        <Tabs.Root value={tab} onValueChange={setTab}>
          <Tabs.List asChild>
            <Segmented
              as="tabs"
              aria-label="Settings"
              options={[
                { value: 'chat', label: 'Chat', testId: 'settings-chat-tab' },
                { value: 'app', label: 'App', testId: 'settings-app-tab' },
              ]}
              value={tab}
              onChange={setTab}
              renderOption={({ option, className, content }) => (
                <Tabs.Trigger
                  value={option.value}
                  className={className}
                  data-testid={option.testId}
                >
                  {content}
                </Tabs.Trigger>
              )}
            />
          </Tabs.List>
          <Tabs.Content value="chat">chat body</Tabs.Content>
          <Tabs.Content value="app">app body</Tabs.Content>
        </Tabs.Root>
      );
    }
    const { container, getByRole, getByTestId, getByText, queryByText } = render(<Harness />);
    const strip = getByTestId('lz-segmented');
    expect(strip.getAttribute('role')).toBe('tablist');
    expect(strip.className).toContain('rounded-lz-control');
    const chat = getByTestId('settings-chat-tab');
    const app = getByTestId('settings-app-tab');
    expect(chat.getAttribute('data-state')).toBe('active');
    expect(chat.getAttribute('aria-selected')).toBe('true');
    expect(chat.className).toContain('bg-lz-accent');
    expect(app.getAttribute('data-state')).toBe('inactive');
    expect(app.className).not.toContain('bg-lz-accent');
    expect(getByText('chat body')).toBeTruthy();

    fireEvent.mouseDown(app, { button: 0 });
    expect(app.getAttribute('data-state')).toBe('active');
    expect(app.className).toContain('bg-lz-accent');
    expect(chat.className).not.toContain('bg-lz-accent');
    expect(queryByText('chat body')).toBeNull();
    expect(getByText('app body')).toBeTruthy();

    // Radix collects its roving items through the tablist's DOM node — only reachable because
    // the primitive forwards its ref. ArrowLeft from App must land focus on Chat and select it
    // (react-roving-focus moves focus inside a setTimeout, hence waitFor).
    app.focus();
    fireEvent.keyDown(app, { key: 'ArrowLeft' });
    await waitFor(() => expect(document.activeElement).toBe(chat));
    expect(chat.getAttribute('data-state')).toBe('active');
    expect(getByRole('tablist')).toBe(strip);
    assertStudioClean(container);
  });
});
