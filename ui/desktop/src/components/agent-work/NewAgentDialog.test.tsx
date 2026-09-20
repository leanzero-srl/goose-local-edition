import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { NewAgentDialog } from './NewAgentDialog';
import { parse } from 'yaml';
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ extensionsList: [{ name: 'LeanZero Web Search', type: 'stdio' }] }),
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
