import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import InlineMarkdown from './InlineMarkdown';

// These rows are 11px one-liners (clarify questions, judge hints, plan lines). The bugs worth guarding
// are (a) markup leaking through as literal ** / backticks, and (b) a block element sneaking in and
// blowing up the row height. Assert the rendered DOM, not that it compiled.
describe('InlineMarkdown', () => {
  it('renders bold as <strong>, not literal asterisks', () => {
    const { container } = render(<InlineMarkdown content="Sort **oldest first** by default" />);
    expect(container.querySelector('strong')?.textContent).toBe('oldest first');
    expect(container.textContent).toBe('Sort oldest first by default');
    expect(container.textContent).not.toContain('**');
  });

  it('renders inline code as <code>, not literal backticks', () => {
    const { container } = render(<InlineMarkdown content="join tags with `X :: Y` exactly" />);
    expect(container.querySelector('code')?.textContent).toBe('X :: Y');
    expect(container.textContent).not.toContain('`');
  });

  // CommonMark strips ONE leading and trailing space inside a code span, so `<space>::<space>` displays
  // as "::" — whitespace-significant content loses its edges. That is spec behaviour shared by every
  // markdown renderer (MarkdownContent already did this to task specs), not something introduced here.
  // Pinned as a test because a separator's exact padding is the kind of detail an operator reads off
  // this panel and assumes is verbatim.
  it('drops a code span’s edge spaces, per CommonMark', () => {
    const { container } = render(<InlineMarkdown content="use ` :: ` exactly" />);
    expect(container.querySelector('code')?.textContent).toBe('::');
  });

  it('degrades a heading to bold instead of emitting an h1 that would wreck the row', () => {
    const { container } = render(<InlineMarkdown content="# Persistence format" />);
    expect(container.querySelector('h1')).toBeNull();
    expect(container.querySelector('strong')?.textContent).toBe('Persistence format');
  });

  it('emits no block-level elements that would break a dense row', () => {
    const { container } = render(
      <InlineMarkdown content={'- one\n- two\n\n> quoted\n\n```\ncode block\n```'} />
    );
    for (const tag of ['p', 'ul', 'ol', 'li', 'blockquote', 'pre', 'hr', 'table']) {
      expect(container.querySelector(tag), `${tag} must not render inline`).toBeNull();
    }
    expect(container.textContent).toContain('one');
    expect(container.textContent).toContain('quoted');
  });

  it('renders link text without an anchor, so nothing in the panel can navigate', () => {
    const { container } = render(<InlineMarkdown content="see [the docs](https://example.com)" />);
    expect(container.querySelector('a')).toBeNull();
    expect(screen.getByTitle('https://example.com').textContent).toBe('the docs');
  });

  it('leaves plain prose byte-identical', () => {
    const s = 'What should search match: title only, or the body too?';
    const { container } = render(<InlineMarkdown content={s} />);
    expect(container.textContent).toBe(s);
  });
});
