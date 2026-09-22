import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ProviderTile } from './ProviderTile';

/** Every supported cloud provider's tile carries its REAL mark (Mihai 2026-09-22: "the real icons
 *  of each provider!!!"), not a letter — except the one with no published mark, which says so. */
describe('ProviderTile', () => {
  it.each([
    'openai',
    'anthropic',
    'google',
    'aws_bedrock',
    'azure_openai',
    'custom_deepseek',
    'mistral',
    'ollama_cloud',
    'alibaba',
    'openrouter',
    'minimax',
    'moonshot',
  ])('%s draws its brand SVG in white on its own solid hue', (id) => {
    render(<ProviderTile providerId={id} label="X" />);
    const tile = screen.getByTestId(`provider-tile-${id}`);
    expect(tile.dataset.mark).toBe('svg');
    expect(tile.querySelector('svg')).not.toBeNull();
    expect(tile.querySelector('svg')?.querySelector('path')).not.toBeNull();
    // The only text is the SVG's own <title>; no initial letter is drawn beside the mark.
    expect([...tile.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE)).toHaveLength(0);
    expect(tile.querySelector('svg > title')?.textContent).toBeTruthy();
    expect(tile.style.backgroundColor).not.toBe('');
  });

  it('xAI ships as its PNG mark; Z.ai has no published mark and keeps its initial', () => {
    render(<ProviderTile providerId="xai" label="xAI" />);
    expect(screen.getByTestId('provider-tile-xai').querySelector('img')).not.toBeNull();
    render(<ProviderTile providerId="zai" label="Z.AI" />);
    const zai = screen.getByTestId('provider-tile-zai');
    expect(zai.dataset.mark).toBe('initial');
    expect(zai.textContent).toBe('Z');
  });
});
