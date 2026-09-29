import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import JsonSchemaForm, { type JsonSchema } from './JsonSchemaForm';
import { IntlTestWrapper } from '../../i18n/test-utils';

const schema: JsonSchema = {
  type: 'object',
  properties: {
    color: { type: 'string', enum: ['red', 'green', 'blue'] },
    size: { type: 'string', enum: ['s', 'm', 'l'] },
  },
  required: ['size'],
};

const renderForm = (onSubmit = vi.fn(), disabled = false) =>
  render(<JsonSchemaForm schema={schema} onSubmit={onSubmit} disabled={disabled} />, {
    wrapper: IntlTestWrapper,
  });

describe('JsonSchemaForm enum fields', () => {
  it('renders an enum as the Studio listbox, never a native <select>', () => {
    const { container } = renderForm();

    expect(container.querySelector('select')).toBeNull();
    expect(screen.getByRole('combobox', { name: 'color' })).toHaveTextContent('Select...');
    expect(screen.getByRole('combobox', { name: 'size' })).toHaveTextContent('s');
  });

  it('picks an option through the listbox and submits it', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderForm(onSubmit);

    await user.click(screen.getByRole('combobox', { name: 'color' }));
    await user.click(screen.getByRole('option', { name: 'green' }));
    await user.click(screen.getByRole('combobox', { name: 'size' }));
    await user.click(screen.getByRole('option', { name: 'l' }));
    await user.click(screen.getByRole('button', { name: 'Submit' }));

    expect(onSubmit).toHaveBeenCalledWith({ color: 'green', size: 'l' });
  });

  it('offers the empty "Select..." row only for an optional enum, and it clears the value', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderForm(onSubmit);

    await user.click(screen.getByRole('combobox', { name: 'size' }));
    expect(screen.queryByRole('option', { name: 'Select...' })).toBeNull();
    await user.click(screen.getByRole('option', { name: 'm' }));

    await user.click(screen.getByRole('combobox', { name: 'color' }));
    await user.click(screen.getByRole('option', { name: 'blue' }));
    await user.click(screen.getByRole('combobox', { name: 'color' }));
    await user.click(screen.getByRole('option', { name: 'Select...' }));
    await user.click(screen.getByRole('button', { name: 'Submit' }));

    expect(onSubmit).toHaveBeenCalledWith({ color: '', size: 'm' });
  });

  it('disables the listbox when the form is disabled', () => {
    renderForm(vi.fn(), true);

    expect(screen.getByRole('combobox', { name: 'color' })).toBeDisabled();
  });
});
