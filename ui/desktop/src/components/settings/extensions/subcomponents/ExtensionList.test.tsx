import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import ExtensionList, { getFriendlyTitle } from './ExtensionList';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import type { FixedExtensionEntry } from '../../../ConfigContext';

vi.mock('./ExtensionItem', () => ({
  default: ({ extension }: { extension: { name: string } }) => <div>{extension.name}</div>,
}));

const ext = (name: string, enabled: boolean, extra: object = {}) =>
  ({ name, type: 'stdio', enabled, ...extra }) as unknown as FixedExtensionEntry;

describe('ExtensionList (UX audit M1)', () => {
  it('heads each group with the zone SectionHeader whose count is the cards it shows — no coloured dot', () => {
    const { container } = render(
      <IntlTestWrapper>
        <ExtensionList
          extensions={[ext('a', true), ext('b', true), ext('c', false)]}
          onToggle={vi.fn()}
        />
      </IntlTestWrapper>
    );
    const headers = screen.getAllByTestId('lz-section-header');
    expect(headers.map((h) => h.textContent)).toEqual([
      'Default extensions2',
      'Available extensions1',
    ]);
    expect(container.querySelector('.rounded-full')).toBeNull();
  });

  it('names a builtin by its catalogue name when the entry carries no display_name', () => {
    expect(
      getFriendlyTitle({ name: 'computercontroller', type: 'builtin' } as FixedExtensionEntry)
    ).toBe('Computer Controller');
    expect(
      getFriendlyTitle({
        name: 'developer',
        type: 'builtin',
        display_name: 'Developer',
      } as FixedExtensionEntry)
    ).toBe('Developer');
    expect(getFriendlyTitle(ext('my-server', true))).toBe('My Server');
  });
});
