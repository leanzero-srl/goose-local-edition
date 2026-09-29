import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import ParameterInput from './ParameterInput';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { Parameter } from '../../recipe';

const parameter: Parameter = {
  key: 'topic',
  description: 'Topic',
  input_type: 'string',
  requirement: 'required',
};

const renderInput = (onChange = vi.fn()) =>
  render(<ParameterInput parameter={parameter} onChange={onChange} />, {
    wrapper: IntlTestWrapper,
  });

describe('ParameterInput', () => {
  it('renders input type and requirement as the Studio listbox, never a native <select>', () => {
    const { container } = renderInput();

    expect(container.querySelector('select')).toBeNull();
    expect(screen.getByRole('combobox', { name: 'Input Type' })).toHaveTextContent('String');
    expect(screen.getByRole('combobox', { name: 'Requirement' })).toHaveTextContent('Required');
  });

  it('picking an input type calls onChange with the chosen type', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderInput(onChange);

    await user.click(screen.getByRole('combobox', { name: 'Input Type' }));
    expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'String',
      'Select',
      'Number',
      'Boolean',
    ]);
    await user.click(screen.getByRole('option', { name: 'Boolean' }));

    expect(onChange).toHaveBeenCalledWith('topic', { input_type: 'boolean' });
  });

  it('picking a requirement calls onChange with the chosen requirement', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderInput(onChange);

    await user.click(screen.getByRole('combobox', { name: 'Requirement' }));
    await user.click(screen.getByRole('option', { name: 'Optional' }));

    expect(onChange).toHaveBeenCalledWith('topic', { requirement: 'optional' });
  });
});
