import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { PendingInput } from '../../acp/controllerTypes';
import ControllerInput from './ControllerInput';

const permission: PendingInput = {
  input_id: 'input-a',
  run_id: 'run-a',
  prompt: 'Allow this operation?',
  options: [
    { option_id: 'allow', label: 'Allow once', accepts_value: false },
    { option_id: 'deny', label: 'Deny', accepts_value: false },
  ],
  payload: {
    kind: 'permission',
    request_id: 'request-a',
    tool_name: 'filesystem.write',
    arguments: { path: '/owner/report.txt', content: 'Exact replacement' },
  },
};
describe('controller input decisions', () => {
  it('shows the exact operation and arguments before allowing a one-use answer', async () => {
    const answer = vi
      .fn()
      .mockRejectedValue(new Error('Another controller already answered this request.'));
    render(<ControllerInput input={permission} disabled={false} onAnswer={answer} />, {
      wrapper: IntlTestWrapper,
    });
    expect(screen.getByText('filesystem.write')).toBeInTheDocument();
    expect(screen.getByText(/Exact replacement/)).toHaveTextContent('/owner/report.txt');
    fireEvent.click(screen.getByRole('button', { name: 'Allow once' }));
    await waitFor(() => expect(answer).toHaveBeenCalledWith(permission.options[0], null));
    expect(await screen.findByRole('alert')).toHaveTextContent('already answered');
    expect(screen.queryByText('Delivered')).not.toBeInTheDocument();
  });
  it('unknown payloads and unsupported form constraints cannot send a permission', () => {
    const answer = vi.fn();
    const view = render(
      <ControllerInput
        input={{
          ...permission,
          payload: { kind: 'unknown' } as unknown as PendingInput['payload'],
        }}
        disabled={false}
        onAnswer={answer}
      />,
      { wrapper: IntlTestWrapper }
    );
    expect(screen.getByRole('button', { name: 'Allow once' })).toBeDisabled();
    view.rerender(
      <ControllerInput
        input={{
          ...permission,
          payload: {
            kind: 'form',
            schema: { type: 'object', properties: { x: { type: 'string', pattern: '^safe$' } } },
          },
        }}
        disabled={false}
        onAnswer={answer}
      />
    );
    expect(screen.getByRole('button', { name: 'Allow once' })).toBeDisabled();
    expect(answer).not.toHaveBeenCalled();
  });
  it('submits a validated form object and leaves invalid values open', async () => {
    const answer = vi.fn().mockResolvedValue(undefined);
    const form: PendingInput = {
      ...permission,
      options: [{ option_id: 'submit', label: 'Submit', accepts_value: true }],
      payload: {
        kind: 'form',
        schema: {
          type: 'object',
          required: ['count'],
          properties: { count: { type: 'integer', minimum: 2 } },
        },
      },
    };
    render(<ControllerInput input={form} disabled={false} onAnswer={answer} />, {
      wrapper: IntlTestWrapper,
    });
    fireEvent.change(screen.getByRole('spinbutton', { name: 'count' }), { target: { value: '1' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('minimum is 2');
    expect(answer).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole('spinbutton', { name: 'count' }), { target: { value: '3' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));
    await waitFor(() => expect(answer).toHaveBeenCalledWith(form.options[0], '{"count":3}'));
  });
});
