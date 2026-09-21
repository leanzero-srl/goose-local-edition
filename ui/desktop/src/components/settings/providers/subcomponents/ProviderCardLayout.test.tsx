import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import GridLayout, { GRID_LAYOUT_CLASS } from './GridLayout';
import { ProviderCard } from './ProviderCard';
import type { ProviderDetails } from '../../../../types/providers';

/**
 * Owner (2026-09-21): the Providers page's cards were fixed-height tiles in an 8-wide grid, so a
 * description longer than three lines (Amazon Bedrock, Azure Foundry, Moonshot) scrolled INSIDE
 * its card. The contract now: two columns, every card sized to its content, the description in
 * full with no inner scroll, the Configure button in the card's footer row.
 */

const LONG_DESCRIPTION =
  'Amazon Bedrock gives access to Anthropic Claude, Meta Llama, Mistral and Amazon Titan models ' +
  'through one AWS endpoint. Configure an access key and secret with bedrock:InvokeModel ' +
  'permission, pick a region where the model is enabled, and optionally a bearer token for ' +
  'API-key style auth. Cross-region inference profiles are supported when the model id carries ' +
  'the region prefix.';

function provider(name: string, description: string): ProviderDetails {
  return {
    name,
    is_configured: false,
    provider_type: 'Native',
    metadata: {
      name,
      display_name: name,
      description,
      default_model: '',
      model_doc_link: '',
      model_selection_hint: null,
      config_keys: [],
      known_models: [],
      setup_steps: [],
    },
  } as unknown as ProviderDetails;
}

afterEach(() => cleanup());

describe('provider card layout', () => {
  it('the grid is two columns of top-aligned cards, one column when narrow', () => {
    render(
      <GridLayout>
        <div>a</div>
      </GridLayout>,
      { wrapper: IntlTestWrapper }
    );
    const grid = screen.getByTestId('provider-grid-layout');
    expect(grid.className).toBe(GRID_LAYOUT_CLASS);
    expect(grid.className).toContain('min-[720px]:grid-cols-2');
    expect(grid.className).toContain('grid-cols-1');
    expect(grid.className).toContain('items-start');
    expect(grid.style.gridTemplateColumns).toBe('');
  });

  it('a long description is shown in full — no inner scrollbar, no clamped height', () => {
    render(
      <GridLayout>
        <ProviderCard
          provider={provider('aws_bedrock', LONG_DESCRIPTION)}
          onConfigure={vi.fn()}
          onLaunch={vi.fn()}
          isOnboarding={false}
        />
      </GridLayout>,
      { wrapper: IntlTestWrapper }
    );
    const description = screen.getByTestId('provider-description');
    expect(description).toHaveTextContent(LONG_DESCRIPTION);
    expect(description.className).not.toMatch(/overflow-y-auto|overflow-auto|overflow-y-scroll/);
    expect(description.className).not.toMatch(/max-h-/);
    expect(description.className).not.toMatch(/line-clamp/);
    expect(description.className).toContain('overflow-visible');

    const card = screen.getByTestId('provider-card-aws_bedrock');
    const surface = card.querySelector('.bg-background-primary') as HTMLElement;
    expect(surface.className).not.toMatch(/(^|\s)h-\[\d+px\]/);
    expect(surface.className).toContain('h-auto');
    expect(surface.className).toContain('flex-col');
    expect(surface.className).toContain('overflow-visible');
  });

  it('the Configure button sits in the footer row at the bottom of the card', () => {
    render(
      <ProviderCard
        provider={provider('moonshot', LONG_DESCRIPTION)}
        onConfigure={vi.fn()}
        onLaunch={vi.fn()}
        isOnboarding={false}
      />,
      { wrapper: IntlTestWrapper }
    );
    const button = screen.getByRole('button');
    const footer = button.closest('.mt-auto');
    expect(footer).not.toBeNull();
    const description = screen.getByTestId('provider-description');
    expect(
      description.compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });
});
