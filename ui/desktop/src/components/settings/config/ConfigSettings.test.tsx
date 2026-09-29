import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import ConfigSettings, { describeConfigValue, parseConfigDraft } from './ConfigSettings';

const ctx = vi.hoisted(() => ({
  config: {} as Record<string, unknown>,
  upsert: vi.fn(),
}));

vi.mock('../../ConfigContext', () => ({
  useConfig: () => ({ config: ctx.config, upsert: ctx.upsert }),
}));
vi.mock('../../../toasts', () => ({ toastSuccess: vi.fn(), toastError: vi.fn() }));

const NESTED = { enabled: true, hosts: ['a.local', 'b.local'] };

async function openEditor() {
  render(
    <IntlTestWrapper>
      <ConfigSettings />
    </IntlTestWrapper>
  );
  fireEvent.click(screen.getByRole('button', { name: /Edit Configuration/ }));
  return screen.findByRole('dialog');
}

describe('ConfigSettings (Q-473)', () => {
  beforeEach(() => {
    ctx.upsert.mockReset();
    ctx.upsert.mockResolvedValue(undefined);
    ctx.config = {
      GOOSE_MODE: 'auto',
      GOOSE_SWARM_POOL: NESTED,
      GOOSE_MAX_TURNS: 0,
      GOOSE_TOOLSHIM: false,
    };
  });

  it('never shows "[object Object]" and shows a nested value as its JSON, read-only', async () => {
    const dialog = await openEditor();
    expect(dialog.textContent).not.toContain('[object Object]');
    const row = within(dialog).getByTestId('config-structured-GOOSE_SWARM_POOL');
    expect(row.textContent).toContain('"a.local"');
    expect(within(row).queryByRole('textbox')).toBeNull();
    expect(within(row).queryByRole('button')).toBeNull();
  });

  it('shows false and 0 instead of a blank field', async () => {
    const dialog = await openEditor();
    const inputs = within(dialog).getAllByRole('textbox') as HTMLInputElement[];
    const values = inputs.map((input) => input.value);
    expect(values).toContain('0');
    expect(values).toContain('false');
  });

  it('saves only the field that was touched, with its own type, and never the nested one', async () => {
    const dialog = await openEditor();
    const turns = within(dialog)
      .getAllByRole('textbox')
      .find((input) => (input as HTMLInputElement).value === '0') as HTMLInputElement;
    fireEvent.change(turns, { target: { value: '25' } });
    const saveButtons = within(dialog)
      .getAllByRole('button')
      .filter((button) => !(button as HTMLButtonElement).disabled && !button.textContent);
    expect(saveButtons).toHaveLength(1);
    fireEvent.click(saveButtons[0]);
    await waitFor(() => expect(ctx.upsert).toHaveBeenCalledTimes(1));
    expect(ctx.upsert).toHaveBeenCalledWith('GOOSE_MAX_TURNS', 25, false);
    for (const call of ctx.upsert.mock.calls) {
      expect(call[0]).not.toBe('GOOSE_SWARM_POOL');
      expect(String(call[1])).not.toContain('[object Object]');
    }
  });

  it('keeps each value kind through a round trip', () => {
    expect(describeConfigValue(NESTED)).toEqual({
      kind: 'structured',
      text: JSON.stringify(NESTED, null, 2),
    });
    expect(describeConfigValue(['x'])).toMatchObject({ kind: 'structured' });
    expect(describeConfigValue(false)).toEqual({ kind: 'boolean', text: 'false' });
    expect(describeConfigValue(0)).toEqual({ kind: 'number', text: '0' });
    expect(describeConfigValue(null)).toEqual({ kind: 'text', text: '' });
    expect(parseConfigDraft('number', '12')).toEqual({ ok: true, value: 12 });
    expect(parseConfigDraft('number', 'twelve')).toEqual({ ok: false });
    expect(parseConfigDraft('number', ' ')).toEqual({ ok: false });
    expect(parseConfigDraft('boolean', 'TRUE')).toEqual({ ok: true, value: true });
    expect(parseConfigDraft('boolean', 'yes')).toEqual({ ok: false });
    expect(parseConfigDraft('structured', '[object Object]')).toEqual({ ok: false });
    expect(parseConfigDraft('text', 'hello')).toEqual({ ok: true, value: 'hello' });
  });
});
