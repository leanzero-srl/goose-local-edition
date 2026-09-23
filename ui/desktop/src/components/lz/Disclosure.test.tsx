import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Disclosure } from './Disclosure';
import { assertStudioClean } from './assertStudioClean';

describe('lz/Disclosure', () => {
  it('is a button with aria-expanded over a hidden-but-mounted body — never <details>', () => {
    const { container } = render(
      <Disclosure title="Advanced workflow">
        <label>
          Env file
          <input aria-label="Env file" />
        </label>
      </Disclosure>
    );
    expect(container.querySelector('details, summary')).toBeNull();
    const toggle = screen.getByRole('button', { name: 'Advanced workflow' });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    const body = document.getElementById(toggle.getAttribute('aria-controls')!)!;
    expect(body).not.toBeVisible();
    // Mounted while closed: controlled fields keep their DOM (the <details> contract).
    expect(screen.getByLabelText('Env file')).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(body).toBeVisible();
    expect(toggle.querySelector('svg')!.getAttribute('class')).toContain('rotate-90');
    assertStudioClean(container);
  });

  it('can be controlled, and the meta slot sits outside the toggle', () => {
    const onOpenChange = vi.fn();
    render(
      <Disclosure
        title="Engine details"
        open={false}
        onOpenChange={onOpenChange}
        meta={<span>9</span>}
      >
        body
      </Disclosure>
    );
    const toggle = screen.getByRole('button', { name: 'Engine details' });
    fireEvent.click(toggle);
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    expect(toggle.textContent).not.toContain('9');
  });
});
