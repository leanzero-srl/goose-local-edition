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

describe('lz/DataTable — rowProps, rowTestId, renderSubRow', () => {
  it('rowProps puts data-* hooks, a title and a joined className on the row; its handlers run before the table’s own', () => {
    const onRowClick = vi.fn();
    const rowClick = vi.fn();
    const { container, getAllByTestId } = render(
      <DataTable
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        onRowClick={onRowClick}
        rowProps={(r) => ({
          'data-node': r.device,
          'data-live': r.tokens > 100,
          title: `${r.device} lane`,
          className: 'group',
          onClick: r.id === 'b' ? (e) => e.preventDefault() : rowClick,
        })}
      />
    );
    const [a, b] = getAllByTestId('lz-row');
    expect(a.getAttribute('data-node')).toBe('m4-max');
    expect(a.getAttribute('data-live')).toBe('true');
    expect(a.getAttribute('title')).toBe('m4-max lane');
    expect(a.className).toContain('group');
    expect(a.className).toContain('h-lz-row');
    expect(a.className).toContain('hover:bg-lz-surface-2');
    // The table keeps its identity attributes.
    expect(a.getAttribute('data-key')).toBe('a');
    expect(a.getAttribute('tabindex')).toBe('0');
    fireEvent.click(a);
    expect(rowClick).toHaveBeenCalledTimes(1);
    expect(onRowClick).toHaveBeenCalledTimes(1);
    // A rowProps onClick that prevents default swallows the row click.
    fireEvent.click(b);
    expect(onRowClick).toHaveBeenCalledTimes(1);
    assertStudioClean(container);
  });

  it('rowTestId replaces the row’s data-testid (the fleet pins fleet-node); data-key stays', () => {
    const { container, queryAllByTestId, getAllByTestId } = render(
      <DataTable columns={columns} rows={rows} rowKey={(r) => r.id} rowTestId={() => 'fleet-node'} />
    );
    expect(queryAllByTestId('lz-row')).toHaveLength(0);
    const nodes = getAllByTestId('fleet-node');
    expect(nodes.map((n) => n.getAttribute('data-key'))).toEqual(['a', 'b']);
    assertStudioClean(container);
  });

  it('a data-testid from rowProps is honoured when no rowTestId is given', () => {
    const { getAllByTestId } = render(
      <DataTable
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        rowProps={(r) => ({ 'data-testid': `lane-${r.id}` })}
      />
    );
    expect(getAllByTestId('lane-a')).toHaveLength(1);
    expect(getAllByTestId('lane-b')).toHaveLength(1);
  });

  it('renderSubRow adds one full-width row under the row when it returns content, keyed by the row, carrying its selection', () => {
    const { container, getAllByTestId, getByText } = render(
      <DataTable
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        selectedKey="a"
        rowAction={() => <button type="button">…</button>}
        renderSubRow={(r) => (r.id === 'a' ? <span>downloading 42%</span> : null)}
      />
    );
    const subs = getAllByTestId('lz-sub-row');
    expect(subs).toHaveLength(1);
    const sub = subs[0];
    expect(sub.getAttribute('data-key')).toBe('a');
    expect(getByText('downloading 42%').closest('td')?.getAttribute('colspan')).toBe('3');
    // Sits directly under its row, before the next row.
    const trs = Array.from(container.querySelectorAll('tbody tr'));
    expect(trs.map((t) => t.getAttribute('data-testid'))).toEqual(['lz-row', 'lz-sub-row', 'lz-row']);
    // The selected row's accent fill continues into its sub row; no divider between them.
    for (const c of SURFACE.selected.split(' ')) expect(sub.className).toContain(c);
    expect(sub.className).not.toContain('border-t');
    expect(sub.getAttribute('aria-selected')).toBe('true');
    assertStudioClean(container);
  });

  it('renderSubRow that returns nothing adds no row at all', () => {
    const { container, queryByTestId } = render(
      <DataTable columns={columns} rows={rows} rowKey={(r) => r.id} renderSubRow={() => null} />
    );
    expect(queryByTestId('lz-sub-row')).toBeNull();
    expect(container.querySelectorAll('tbody tr')).toHaveLength(2);
  });
});
