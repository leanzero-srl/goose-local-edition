import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { KeyValue } from './KeyValue';
import { assertStudioClean } from './assertStudioClean';

describe('lz/KeyValue', () => {
  it('renders keyed label/value rows with tabular right-aligned values, tone and mono registers', () => {
    const { container, getAllByTestId, getByText } = render(
      <KeyValue
        aria-label="run"
        items={[
          { key: 'phase', label: 'Phase', value: 'BUILD' },
          { key: 'failed', label: 'Failed', value: 2, tone: 'err' },
          { key: 'sha', label: 'Commit', value: 'd64068fb6', mono: true },
        ]}
      />
    );
    const rows = getAllByTestId('lz-key-value-row');
    expect(rows).toHaveLength(3);
    expect(rows[0].className).toContain('h-lz-row');
    const failed = getByText('2');
    expect(failed.tagName).toBe('DD');
    expect(failed.className).toContain('tnum');
    expect(failed.className).toContain('text-right');
    expect(failed.className).toContain('text-lz-err');
    const sha = getByText('d64068fb6');
    expect(sha.className).toContain('font-mono');
    expect(sha.className).not.toContain('text-lz-body');
    expect(getByText('BUILD').className).toContain('text-lz-ink');
    assertStudioClean(container);
  });

  it('dense rows are 32px', () => {
    const { getAllByTestId } = render(
      <KeyValue dense items={[{ key: 'k', label: 'Nodes', value: 3 }]} />
    );
    expect(getAllByTestId('lz-key-value-row')[0].className).toContain('h-lz-row-dense');
  });
});
