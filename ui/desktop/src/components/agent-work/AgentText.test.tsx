import { fireEvent, render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { describe, expect, it } from 'vitest';
import { AgentText } from './AgentText';

function display(text: string) {
  return render(
    <IntlProvider locale="en">
      <AgentText text={text} raw />
    </IntlProvider>
  );
}

describe('Agent tick text', () => {
  it('renders the durable transcript as fields, evidence lists and Markdown, retaining original bytes', () => {
    const text =
      '\n===== swarm attempt 0 · dispatched 2026-09-20T08:32:56+00:00 =====\n' +
      JSON.stringify({
        homework: 'Read the page.\nChecked its title.',
        finding: '**JavaScript** reference\n\n[Source](https://developer.mozilla.org/)',
        confidence: 3,
        evidence: ['HTTP 200', 'Title verified'],
        next_step: 'Record the finding.',
      });
    const { container } = display(text);
    for (const name of ['Homework', 'Finding', 'Confidence', 'Evidence', 'Next step']) {
      expect(screen.getByRole('heading', { name })).toBeTruthy();
    }
    expect(container.querySelectorAll('li')).toHaveLength(2);
    expect(container.querySelector('strong')?.textContent).toBe('JavaScript');
    expect(container.querySelector('br')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Source' }).getAttribute('href')).toBe(
      'https://developer.mozilla.org/'
    );
    expect(container.querySelector('pre')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Original text' }));
    expect(container.querySelector('pre')?.textContent).toBe(text);
  });

  it('preserves HTML evidence, code and Markdown autolinks', () => {
    const { container } = display(
      JSON.stringify({
        evidence: ['<title>JavaScript | MDN</title>', '`<title>`', '<https://example.com>'],
      })
    );
    expect(screen.getByText('<title>JavaScript | MDN</title>')).toBeTruthy();
    expect(container.querySelector('code')?.textContent).toBe('<title>');
    expect(screen.getByRole('link', { name: 'https://example.com' }).getAttribute('href')).toBe(
      'https://example.com'
    );
    expect(container.querySelector('title')).toBeNull();
  });

  it('retains multiple attempts, incomplete JSON, nested fields and empty values', () => {
    const partial = '{"finding":"unfinished';
    const text =
      `===== swarm attempt 0 · dispatched earlier =====\n${partial}\n===== swarm attempt 1 · dispatched later =====\n` +
      JSON.stringify({
        finding: 'Completed',
        detail: { verified: false, count: 0, missing: null },
        evidence: [],
      });
    const { container } = display(text);
    expect(container.querySelector('pre')?.textContent).toContain(partial);
    for (const name of ['Completed', 'false', '0', 'Not provided', 'None'])
      expect(screen.getByText(name)).toBeTruthy();
  });

  it('renders ordinary Markdown and fenced JSON without stripping content', () => {
    const { rerender } = display('## Result\n\n- First\n- Second\n\n`a_b`');
    expect(screen.getByRole('heading', { name: 'Result' })).toBeTruthy();
    expect(screen.getAllByRole('listitem')).toHaveLength(2);
    rerender(
      <IntlProvider locale="en">
        <AgentText text={'```json\n{"finding":"Found it"}\n```'} />
      </IntlProvider>
    );
    expect(screen.getByRole('heading', { name: 'Finding' })).toBeTruthy();
    expect(screen.getByText('Found it')).toBeTruthy();
  });
});
