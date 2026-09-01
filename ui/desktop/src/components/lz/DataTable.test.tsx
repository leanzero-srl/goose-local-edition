import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { DataTable, type DataTableColumn } from './DataTable';
import { SURFACE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

interface Lane {
  id: string;
  device: string;
  tokens: number;
}

const columns: DataTableColumn<Lane>[] = [
  { key: 'device', header: 'Device', cell: (r) => r.device },
  { key: 'tokens', header: 'Tokens', cell: (r) => r.tokens, numeric: true, width: 96 },
];
const rows: Lane[] = [
  { id: 'a', device: 'm4-max', tokens: 1200 },
  { id: 'b', device: 'm3-ultra', tokens: 87 },
];

describe('lz/DataTable', () => {
  it('keys rows by identity, right-aligns numeric columns in tabular figures, header in the zone register', () => {
    const { container, getAllByTestId, getAllByRole } = render(
      <DataTable aria-label="lanes" columns={columns} rows={rows} rowKey={(r) => r.id} />
    );
    const trs = getAllByTestId('lz-row');
    expect(trs.map((t) => t.getAttribute('data-key'))).toEqual(['a', 'b']);
    const headers = getAllByRole('columnheader');
    expect(headers[0].className).toContain('text-lz-zone');
    expect(headers[0].className).toContain('uppercase');
    expect(headers[1].className).toContain('text-right');
    expect(headers[1].style.width).toBe('96px');
    const numeric = trs[0].querySelectorAll('td')[1];
    expect(numeric.className).toContain('text-right');
    expect(numeric.className).toContain('tnum');
    expect(numeric.textContent).toBe('1200');
    expect(trs[0].className).toContain('h-lz-row');
    expect(trs[0].className).toContain('border-t');
    expect(trs[0].className).toContain('hover:bg-lz-surface-2');
    assertStudioClean(container);
  });

  it('selected is the accent fill with white ink; clicks and Enter reach onRowClick', () => {
    const onRowClick = vi.fn();
    const { container, getAllByTestId } = render(
      <DataTable
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        selectedKey="b"
        onRowClick={onRowClick}
        dense
      />
    );
    const [a, b] = getAllByTestId('lz-row');
    for (const c of SURFACE.selected.split(' ')) expect(b.className).toContain(c);
    expect(b.getAttribute('aria-selected')).toBe('true');
    expect(b.className).not.toContain('hover:bg-lz-surface-2');
    expect(a.className).toContain('h-lz-row-dense');
    expect(a.getAttribute('tabindex')).toBe('0');
    fireEvent.click(a);
    fireEvent.keyDown(b, { key: 'Enter' });
    expect(onRowClick.mock.calls.map((c) => (c[0] as Lane).id)).toEqual(['a', 'b']);
    assertStudioClean(container);
  });

  it('renders the empty slot across every column when there are no rows, and a trailing action column', () => {
    const { container, getByText, queryByTestId, getAllByRole } = render(
      <DataTable
        columns={columns}
        rows={[]}
        rowKey={(r) => r.id}
        rowAction={() => <button type="button">…</button>}
        empty={<span>No lanes have started.</span>}
      />
    );
    expect(queryByTestId('lz-row')).toBeNull();
    const cell = getByText('No lanes have started.').closest('td');
    expect(cell?.getAttribute('colspan')).toBe('3');
    expect(getAllByRole('columnheader')).toHaveLength(3);
    assertStudioClean(container);
  });
});
