import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Panel } from './Panel';
import { SURFACE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

describe('lz/Panel', () => {
  it('is the surface card — 1px border, radius 10, no shadow — with a zone-register header and 16px body', () => {
    const { container, getByTestId, getByRole } = render(
      <Panel title="Fleet" count={3} headerRight={<span>right</span>}>
        <p>body</p>
      </Panel>
    );
    const panel = getByTestId('lz-panel');
    for (const c of SURFACE.card.split(' ')) expect(panel.className).toContain(c);
    expect(panel.className).not.toMatch(/shadow/);
    expect(getByRole('heading', { level: 2 }).textContent).toBe('Fleet');
    expect(getByTestId('lz-section-count').textContent).toBe('3');
    expect(panel.lastElementChild?.className).toContain('p-lz-card');
    assertStudioClean(container);
  });

  it('a custom header wins over title, and padded=false leaves the body flush for a table', () => {
    const { container, getByTestId, queryByTestId, getByText } = render(
      <Panel header={<b>custom</b>} title="ignored" padded={false}>
        <table />
      </Panel>
    );
    expect(getByText('custom')).not.toBeNull();
    expect(queryByTestId('lz-section-header')).toBeNull();
    expect(getByTestId('lz-panel').lastElementChild?.className ?? '').not.toContain('p-lz-card');
    assertStudioClean(container);
  });

  it('has no header row at all when neither title nor header is given', () => {
    const { getByTestId } = render(<Panel>x</Panel>);
    expect(getByTestId('lz-panel').children).toHaveLength(1);
  });
});
