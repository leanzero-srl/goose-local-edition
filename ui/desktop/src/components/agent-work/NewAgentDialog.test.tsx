import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { NewAgentDialog } from './NewAgentDialog';
import { parse } from 'yaml';
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({
    extensionsList: [
      { name: 'LeanZero Web Search', type: 'stdio', description: 'Find sources and collect pages' },
      {
        name: 'computercontroller',
        display_name: 'Computer Controller',
        type: 'builtin',
      },
      { name: 'summon', type: 'platform' },
    ],
  }),
}));
beforeEach(() => {
  window.electron.agentWorkInit = vi.fn().mockResolvedValue({ ok: true });
});
function fill() {
  fireEvent.change(screen.getByLabelText('Agent directory'), {
    target: { value: '/tmp/research-demo' },
  });
  fireEvent.change(screen.getByLabelText('Agent name'), { target: { value: 'research-demo' } });
  fireEvent.change(screen.getByLabelText('Charter'), {
    target: { value: 'Read public pages and report their URLs. Do not post.' },
  });
}
describe('full workspace agent creation', () => {
  it('writes selected MCP settings by reference and keeps the complete charter', async () => {
    const created = vi.fn();
    render(<NewAgentDialog onClose={vi.fn()} onCreated={created} />);
    fill();
    fireEvent.click(screen.getByLabelText('LeanZero Web Search'));
    fireEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(created).toHaveBeenCalledWith('/tmp/research-demo'));
    const args = vi.mocked(window.electron.agentWorkInit).mock.calls[0];
    expect(parse(args[1])).toMatchObject({
      name: 'research-demo',
      extensions: ['LeanZero Web Search'],
    });
    expect(parse(args[1]).post).toBeUndefined();
    expect(args[2]).toContain('Do not post.');
  });
  it('rejects zero cadence and malformed timezones before writing', async () => {
    render(<NewAgentDialog onClose={vi.fn()} onCreated={vi.fn()} />);
    fill();
    fireEvent.change(screen.getByLabelText('Cadence'), { target: { value: '0m' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Cadence');
    fireEvent.change(screen.getByLabelText('Cadence'), { target: { value: '5m' } });
    fireEvent.change(screen.getByLabelText('Timezone'), { target: { value: 'invalid/timezone' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('valid timezone');
    expect(window.electron.agentWorkInit).not.toHaveBeenCalled();
  });
  it('keeps all input after a disk write failure', async () => {
    window.electron.agentWorkInit = vi.fn().mockRejectedValue(new Error('Permission denied'));
    const created = vi.fn();
    render(<NewAgentDialog onClose={vi.fn()} onCreated={created} />);
    fill();
    fireEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Permission denied');
    expect(screen.getByLabelText('Agent name')).toHaveValue('research-demo');
    expect(created).not.toHaveBeenCalled();
  });
});

describe("the dialog wears the app's own controls (UX audit N1)", () => {
  it('has a real dialog title and no native checkbox, <details> or <select>', () => {
    render(<NewAgentDialog onClose={vi.fn()} onCreated={vi.fn()} />);
    const dialog = screen.getByRole('dialog', { name: 'Create an agent' });
    expect(screen.getByRole('heading', { name: 'Create an agent' }).className).toContain(
      'text-lz-h1'
    );
    expect(dialog.querySelector('input[type="checkbox"], details, summary, select')).toBeNull();
  });

  it('lists tools by display name with their description, never the internal id; platform tools stay out', () => {
    render(<NewAgentDialog onClose={vi.fn()} onCreated={vi.fn()} />);
    const cc = screen.getByRole('checkbox', { name: 'Computer Controller' });
    expect(cc).toHaveAccessibleDescription(/General computer control tools/);
    expect(screen.queryByText('computercontroller')).toBeNull();
    expect(
      screen.getByRole('checkbox', { name: 'LeanZero Web Search' })
    ).toHaveAccessibleDescription('Find sources and collect pages');
    expect(screen.queryByRole('checkbox', { name: /summon/i })).toBeNull();
  });

  it('writes the tool id (not its display name) and the preset cadence — daily is 24h', async () => {
    const created = vi.fn();
    render(<NewAgentDialog onClose={vi.fn()} onCreated={created} />);
    fill();
    fireEvent.click(screen.getByRole('checkbox', { name: 'Computer Controller' }));
    expect(screen.getByRole('checkbox', { name: 'Computer Controller' })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    fireEvent.click(screen.getByRole('radio', { name: 'daily' }));
    expect(screen.getByLabelText('Cadence')).toHaveValue('24h');
    expect(screen.getByRole('radio', { name: 'daily' })).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Create agent' }));
    await waitFor(() => expect(created).toHaveBeenCalled());
    const calls = vi.mocked(window.electron.agentWorkInit).mock.calls;
    const yaml = parse(calls[calls.length - 1][1]);
    expect(yaml.cadence).toBe('24h');
    expect(yaml.extensions).toEqual(['computercontroller']);
  });

  it('a typed cadence outside the presets selects "custom"', () => {
    render(<NewAgentDialog onClose={vi.fn()} onCreated={vi.fn()} />);
    expect(screen.getByRole('radio', { name: '30m' })).toHaveAttribute('aria-checked', 'true');
    fireEvent.change(screen.getByLabelText('Cadence'), { target: { value: '90m' } });
    expect(screen.getByRole('radio', { name: 'custom' })).toHaveAttribute('aria-checked', 'true');
  });

  it('the timezone is a searchable list of real zones', () => {
    render(<NewAgentDialog onClose={vi.fn()} onCreated={vi.fn()} />);
    const tz = screen.getByRole('combobox', { name: 'Timezone' });
    fireEvent.change(tz, { target: { value: 'bucharest' } });
    fireEvent.click(screen.getByRole('option', { name: /Europe\/Bucharest/ }));
    expect(tz).toHaveValue('Europe/Bucharest');
  });

  it('the advanced workflow is a disclosure that opens onto its fields', () => {
    render(<NewAgentDialog onClose={vi.fn()} onCreated={vi.fn()} />);
    const toggle = screen.getByRole('button', { name: /Advanced workflow/ });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(screen.getByLabelText('Env file')).not.toBeVisible();
    fireEvent.click(toggle);
    expect(screen.getByLabelText('Env file')).toBeVisible();
    expect(
      screen.getByRole('checkbox', { name: 'Commit the agent directory after every tick' })
    ).toHaveAttribute('aria-checked', 'true');
  });
});
