import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import { Combobox } from './Combobox';
import { assertStudioClean } from './assertStudioClean';

const ZONES = [
  { value: 'Europe/Bucharest', hint: 'UTC+3' },
  { value: 'Europe/London', hint: 'UTC+1' },
  { value: 'America/New_York', hint: 'UTC-4' },
];

function Controlled({ free = false }: { free?: boolean }) {
  const [v, setV] = useState('Europe/Bucharest');
  return (
    <>
      <Combobox
        aria-label="Timezone"
        options={ZONES}
        value={v}
        onChange={setV}
        allowFreeText={free}
      />
      <output data-testid="value">{v}</output>
    </>
  );
}

describe('lz/Combobox', () => {
  it('opens the whole list on focus, filters as you type, picks with Enter — no native select', () => {
    const { container } = render(<Controlled />);
    expect(container.querySelector('select, datalist')).toBeNull();
    const field = screen.getByRole('combobox', { name: 'Timezone' });
    fireEvent.focus(field);
    expect(screen.getAllByRole('option')).toHaveLength(3);
    expect(screen.getByRole('option', { name: /Europe\/Bucharest/ })).toHaveAttribute(
      'aria-selected',
      'true'
    );
    fireEvent.change(field, { target: { value: 'york' } });
    expect(screen.getAllByRole('option')).toHaveLength(1);
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(screen.getByTestId('value')).toHaveTextContent('America/New_York');
    expect(screen.queryByRole('listbox')).toBeNull();
    assertStudioClean(container);
  });

  it('without free text, a typed query that is never picked does not become the value', () => {
    render(<Controlled />);
    const field = screen.getByRole('combobox', { name: 'Timezone' });
    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: 'Mars/Olympus' } });
    expect(screen.getByText('No matches.')).toBeInTheDocument();
    fireEvent.keyDown(field, { key: 'Escape' });
    expect(screen.getByTestId('value')).toHaveTextContent('Europe/Bucharest');
    expect(field).toHaveValue('Europe/Bucharest');
  });

  it('with free text, what is typed IS the value — the caller validates it', () => {
    render(<Controlled free />);
    const field = screen.getByRole('combobox', { name: 'Timezone' });
    fireEvent.change(field, { target: { value: 'invalid/timezone' } });
    expect(screen.getByTestId('value')).toHaveTextContent('invalid/timezone');
    fireEvent.mouseDown(document.body);
    expect(field).toHaveValue('invalid/timezone');
  });

  it('a value set from outside an unfocused field opens no list (automation drivers)', () => {
    render(<Controlled free />);
    const field = screen.getByRole('combobox', { name: 'Timezone' });
    fireEvent.change(field, { target: { value: 'Europe/London' } });
    expect(screen.getByTestId('value')).toHaveTextContent('Europe/London');
    expect(screen.queryByRole('listbox')).toBeNull();
    field.focus();
    fireEvent.change(field, { target: { value: 'Europe' } });
    expect(screen.getByRole('listbox')).toBeInTheDocument();
  });

  it('arrows move the highlight and a click picks', () => {
    render(<Controlled />);
    const field = screen.getByRole('combobox', { name: 'Timezone' });
    fireEvent.keyDown(field, { key: 'ArrowDown' });
    fireEvent.keyDown(field, { key: 'ArrowDown' });
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(screen.getByTestId('value')).toHaveTextContent('Europe/London');
    fireEvent.focus(field);
    fireEvent.click(screen.getByRole('option', { name: /America/ }));
    expect(screen.getByTestId('value')).toHaveTextContent('America/New_York');
  });
  it('shows the picked option label in the field, never its raw value (D12)', () => {
    function Labelled() {
      const [v, setV] = useState('__best__');
      return (
        <Combobox
          aria-label="Run on"
          options={[
            { value: '__best__', label: 'Best Mac' },
            { value: 'node-1', label: 'Studio' },
          ]}
          value={v}
          onChange={setV}
        />
      );
    }
    render(<Labelled />);
    const field = screen.getByRole('combobox', { name: 'Run on' });
    expect(field).toHaveValue('Best Mac');
    fireEvent.focus(field);
    fireEvent.click(screen.getByRole('option', { name: /Studio/ }));
    expect(field).toHaveValue('Studio');
    fireEvent.blur(field);
    expect(field).toHaveValue('Studio');
  });

  it('lists a disabled option with its reason but never picks it (D11)', () => {
    function WithOff() {
      const [v, setV] = useState('a');
      return (
        <>
          <Combobox
            aria-label="Run on"
            options={[
              { value: 'a', label: 'Alpha' },
              { value: 'b', label: 'Beta', hint: 'jobs are off', disabled: true },
            ]}
            value={v}
            onChange={setV}
          />
          <output data-testid="value">{v}</output>
        </>
      );
    }
    render(<WithOff />);
    const field = screen.getByRole('combobox', { name: 'Run on' });
    fireEvent.focus(field);
    const off = screen.getByRole('option', { name: /Beta/ });
    expect(off).toHaveAttribute('aria-disabled', 'true');
    expect(off).toHaveTextContent('jobs are off');
    fireEvent.click(off);
    expect(screen.getByTestId('value')).toHaveTextContent('a');
    fireEvent.keyDown(field, { key: 'ArrowDown' });
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(screen.getByTestId('value')).toHaveTextContent('a');
  });
});
