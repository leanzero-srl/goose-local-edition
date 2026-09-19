import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Button } from '../components/lz/Button';

describe('button operation feedback', () => {
  it('stays busy until the operation resolves and prevents duplicate work', async () => {
    let finish!: () => void;
    const action = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    render(<Button onClick={action}>Refresh</Button>);
    const button = screen.getByRole('button', { name: 'Refresh' });
    fireEvent.click(button);
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(action).toHaveBeenCalledOnce();
    await act(async () => finish());
    expect(button).toBeEnabled();
    expect(button).not.toHaveAttribute('aria-busy');
  });

  it('shows a failed action and permits retry', async () => {
    render(<Button onClick={async () => { throw new Error('Connection lost'); }}>Reconnect</Button>);
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Connection lost');
    expect(screen.getByRole('button')).toBeEnabled();
  });
});
