import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { PageHeader } from './PageHeader';
import { TYPE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

describe('lz/PageHeader', () => {
  it('sets the title in the display register, the eyebrow in the zone register, actions on the right', () => {
    const { container, getByRole, getByText } = render(
      <PageHeader
        eyebrow="Benchmark · sb-7"
        title="Flock run r5"
        subtitle="Three nodes, one spec, one verdict."
        actions={<button type="button">Stop</button>}
      />
    );
    const h1 = getByRole('heading', { level: 1 });
    expect(h1.textContent).toBe('Flock run r5');
    for (const c of TYPE.display.split(' ')) expect(h1.className).toContain(c);
    for (const c of TYPE.zone.split(' '))
      expect(getByText('Benchmark · sb-7').className).toContain(c);
    expect(getByText('Stop').closest('header')).toBe(container.firstElementChild);
    assertStudioClean(container);
  });

  it('renders no eyebrow, subtitle or actions box when they are absent', () => {
    const { container } = render(<PageHeader title="Models" />);
    expect(container.querySelectorAll('div').length).toBe(1);
    expect(container.querySelector('p')).toBeNull();
    assertStudioClean(container);
  });
});
