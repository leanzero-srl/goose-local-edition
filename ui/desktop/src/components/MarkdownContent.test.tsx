import { describe, it, expect, vi } from 'vitest';
import { render, type RenderOptions } from '@testing-library/react';
import { screen, waitFor } from '@testing-library/dom';
import MarkdownContent from './MarkdownContent';
import { IntlTestWrapper } from '../i18n/test-utils';

const renderWithIntl = (ui: React.ReactElement, options?: RenderOptions) =>
  render(ui, { wrapper: IntlTestWrapper, ...options });

// Mock the icons to avoid import issues
vi.mock('./icons', () => ({
  Check: () => <div data-testid="check-icon">✓</div>,
  Copy: () => <div data-testid="copy-icon">📋</div>,
}));

describe('MarkdownContent', () => {
  describe('HTML Security Integration', () => {
    it('renders safe markdown content normally', async () => {
      const content = `# Test Title

Visit <https://example.com> for more info.

Contact <admin@example.com> for support.

Use \`Array<T>\` for generics.`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Test Title')).toBeInTheDocument();
        expect(screen.getByText(/Visit/)).toBeInTheDocument();
        expect(screen.getByText(/for more info/)).toBeInTheDocument();
        expect(screen.getByText(/Contact/)).toBeInTheDocument();
        expect(screen.getByText(/for support/)).toBeInTheDocument();
      });

      // Should not create extra code blocks for safe content
      const codeBlocks = screen.queryAllByText(/```html/);
      expect(codeBlocks).toHaveLength(0);
    });

    it('wraps dangerous HTML in code blocks', async () => {
      const content = `# Security Test

This is safe text.

<script>alert('xss')</script>

More safe text.`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Security Test')).toBeInTheDocument();
        expect(screen.getByText('This is safe text.')).toBeInTheDocument();
        expect(screen.getByText('More safe text.')).toBeInTheDocument();
      });

      // The script tag should be in a code block, not executed
      const scriptElements = document.querySelectorAll('script');
      expect(scriptElements).toHaveLength(0); // No actual script tags should be created

      // Should find the script content in a code block (text may be split across spans)
      await waitFor(() => {
        expect(screen.getByText(/alert/)).toBeInTheDocument();
        expect(screen.getByText(/xss/)).toBeInTheDocument();
      });
    });

    it('handles HTML comments securely', async () => {
      const content = `# Comment Test

<!-- This is a malicious comment -->

Normal text continues.`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Comment Test')).toBeInTheDocument();
        expect(screen.getByText('Normal text continues.')).toBeInTheDocument();
      });

      // Comment should be in a code block
      await waitFor(() => {
        expect(screen.getByText(/This is a malicious comment/)).toBeInTheDocument();
      });
    });

    it('preserves existing code blocks', async () => {
      const content = `# Code Block Test

\`\`\`javascript
const html = "<div>This is safe in a code block</div>";
console.log(html);
\`\`\`

<div>This should be wrapped</div>`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Code Block Test')).toBeInTheDocument();
      });

      // Should preserve the original JavaScript code block (text may be split)
      await waitFor(() => {
        expect(screen.getByText(/const/)).toBeInTheDocument();
        expect(screen.getAllByText(/html/)).toHaveLength(2); // Variable name and function parameter
      });

      // The div outside the code block should be wrapped
      await waitFor(() => {
        expect(screen.getByText(/This should be wrapped/)).toBeInTheDocument();
      });
    });

    it('handles mixed safe and unsafe content', async () => {
      const content = `# Mixed Content Test

1. Auto-link: <https://block.dev>
2. Inline code: \`const x = Array<T>();\`
3. Real markup: <input type="text" disabled>
4. Placeholder path: <project-root>/src`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Mixed Content Test')).toBeInTheDocument();
        expect(screen.getByText(/Auto-link/)).toBeInTheDocument();
        expect(screen.getByText(/Inline code/)).toBeInTheDocument();
        expect(screen.getByText(/Real markup/)).toBeInTheDocument();
        expect(screen.getByText(/Placeholder path/)).toBeInTheDocument();
      });

      // Only the input tag should be wrapped
      await waitFor(() => {
        expect(screen.getByText(/input/)).toBeInTheDocument();
        expect(screen.getByText(/type/)).toBeInTheDocument();
        expect(screen.getByText(/disabled/)).toBeInTheDocument();
      });

      // Should not have actual input elements in the DOM
      const inputElements = document.querySelectorAll('input');
      expect(inputElements).toHaveLength(0);
    });
  });

  describe('Code Block Functionality', () => {
    it('renders code blocks with syntax highlighting', async () => {
      const content = `\`\`\`javascript
console.log('Hello, World!');
\`\`\``;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText(/console/)).toBeInTheDocument();
        expect(screen.getByText(/log/)).toBeInTheDocument();
        expect(screen.getByText(/Hello, World!/)).toBeInTheDocument();
      });
    });

    it('renders inline code', async () => {
      const content = 'Use `console.log()` to debug.';

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText(/Use/)).toBeInTheDocument();
        expect(screen.getByText(/to debug/)).toBeInTheDocument();
        expect(screen.getByText('console.log()')).toBeInTheDocument();
      });
    });
  });

  describe('Markdown Features', () => {
    it('renders headers correctly', async () => {
      const content = `# H1 Header
## H2 Header
### H3 Header`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByRole('heading', { level: 1, name: 'H1 Header' })).toBeInTheDocument();
        expect(screen.getByRole('heading', { level: 2, name: 'H2 Header' })).toBeInTheDocument();
        expect(screen.getByRole('heading', { level: 3, name: 'H3 Header' })).toBeInTheDocument();
      });
    });

    it('renders lists correctly', async () => {
      const content = `- Item 1
- Item 2
- Item 3

1. Numbered 1
2. Numbered 2`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Item 1')).toBeInTheDocument();
        expect(screen.getByText('Item 2')).toBeInTheDocument();
        expect(screen.getByText('Item 3')).toBeInTheDocument();
        expect(screen.getByText('Numbered 1')).toBeInTheDocument();
        expect(screen.getByText('Numbered 2')).toBeInTheDocument();
      });
    });

    it('renders links with correct attributes', async () => {
      const content = '[Visit Block](https://block.dev)';

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        const link = screen.getByRole('link', { name: 'Visit Block' });
        expect(link).toBeInTheDocument();
        expect(link).toHaveAttribute('href', 'https://block.dev');
        expect(link).toHaveAttribute('target', '_blank');
        expect(link).toHaveAttribute('rel', 'noopener noreferrer');
      });
    });

    it('renders tables correctly', async () => {
      const content = `| Name | Value |
|------|-------|
| Test | 123   |
| Demo | 456   |`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText('Name')).toBeInTheDocument();
        expect(screen.getByText('Value')).toBeInTheDocument();
        expect(screen.getByText('Test')).toBeInTheDocument();
        expect(screen.getByText('123')).toBeInTheDocument();
        expect(screen.getByText('Demo')).toBeInTheDocument();
        expect(screen.getByText('456')).toBeInTheDocument();
      });
    });
  });

  describe('Error Handling', () => {
    it('handles empty content gracefully', async () => {
      renderWithIntl(<MarkdownContent content="" />);

      // Should not throw and should render the component
      const container = document.querySelector('.w-full.overflow-x-hidden');
      expect(container).toBeInTheDocument();
    });

    it('handles malformed markdown gracefully', async () => {
      const content = `# Unclosed header
[Unclosed link(https://example.com
\`\`\`
Unclosed code block`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        // Should still render what it can
        expect(screen.getByText('Unclosed header')).toBeInTheDocument();
      });
    });
  });

  describe('Line Break Functionality', () => {
    it('preserves single line breaks with remark-breaks plugin', async () => {
      const content = `First line
Second line
Third line`;

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        // Check that all text content is present (text may be split by <br> tags)
        expect(container).toHaveTextContent('First line');
        expect(container).toHaveTextContent('Second line');
        expect(container).toHaveTextContent('Third line');
      });

      // Check that line breaks are preserved (rendered as <br> tags)
      const brElements = container.querySelectorAll('br');
      expect(brElements.length).toBeGreaterThan(0);
    });

    it('handles mixed content with line breaks', async () => {
      const content = `# Header
Paragraph with
line breaks.

- List item 1
- List item 2`;

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByRole('heading', { level: 1, name: 'Header' })).toBeInTheDocument();

        // Check that text content is present (text may be split by <br> tags)
        expect(container).toHaveTextContent('Paragraph with');
        expect(container).toHaveTextContent('line breaks.');
        expect(screen.getByText('List item 1')).toBeInTheDocument();
        expect(screen.getByText('List item 2')).toBeInTheDocument();
      });
    });

    it('maintains existing markdown features with line breaks', async () => {
      const content = `**Bold text**
with line break

\`code\` and
more text`;

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        // Bold text should still work
        const boldElement = container.querySelector('strong');
        expect(boldElement).toBeInTheDocument();
        expect(boldElement).toHaveTextContent('Bold text');

        // Code should still work
        expect(screen.getByText('code')).toBeInTheDocument();
      });
    });
  });

  describe('URL Overflow Handling', () => {
    it('handles very long URLs without overflow', async () => {
      const longUrl =
        'https://example-docs.com/document/d/1oruk3lcrnhoOXMFzBJB8X6qQ5AtQTmj4XXxXk3xK-3g/edit?usp=sharing&mode=edit&version=1';
      const content = `Check out this document: ${longUrl}

Another very long URL: https://www.example.com/very/long/path/with/many/segments/and/parameters?param1=value1&param2=value2&param3=value3&param4=value4&param5=value5`;

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText(/Check out this document/)).toBeInTheDocument();
        expect(screen.getByText(/Another very long URL/)).toBeInTheDocument();
      });

      // Check that URLs are rendered as links
      const links = container.querySelectorAll('a');
      expect(links.length).toBeGreaterThan(0);

      // Check that links have proper CSS classes for word breaking
      links.forEach((link) => {
        // The CSS should allow the text to break
        expect(link).toBeInTheDocument();
      });
    });

    it('handles markdown links with long URLs', async () => {
      const longUrl =
        'https://example-docs.com/document/d/1oruk3lcrnhoOXMFzBJB8X6qQ5AtQTmj4XXxXk3xK-3g/edit?usp=sharing&mode=edit&version=1';
      const content = `[Click here for the document](${longUrl})`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        const link = screen.getByRole('link', { name: 'Click here for the document' });
        expect(link).toBeInTheDocument();
        expect(link).toHaveAttribute('href', longUrl);
      });
    });

    it('handles multiple long URLs in the same message', async () => {
      const content = `Here are some long URLs:

1. Example Doc: https://example-docs.com/document/d/1oruk3lcrnhoOXMFzBJB8X6qQ5AtQTmj4XXxXk3xK-3g/edit?usp=sharing&mode=edit&version=1
2. Another long URL: https://www.example.com/very/long/path/with/many/segments/and/parameters?param1=value1&param2=value2&param3=value3
3. Third URL: https://api.example.com/v1/users/12345/documents/67890/attachments/abcdef123456789?format=json&include=metadata&sort=created_at`;

      renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(screen.getByText(/Here are some long URLs/)).toBeInTheDocument();
        expect(screen.getByText(/Example Doc/)).toBeInTheDocument();
        expect(screen.getByText(/Another long URL/)).toBeInTheDocument();
        expect(screen.getByText(/Third URL/)).toBeInTheDocument();
      });
    });

    it('applies word-break CSS classes to the container', () => {
      const content = 'Test content';
      renderWithIntl(<MarkdownContent content={content} />);

      const markdownContainer = document.querySelector('.prose');
      expect(markdownContainer).toBeInTheDocument();
      expect(markdownContainer).toHaveClass('break-words');
      expect(markdownContainer).toHaveClass('prose-a:break-all');
      // MEASURED: `prose-a:overflow-wrap-anywhere` compiles to NO rule (there is no such utility);
      // the real long-URL rule is `prose-a:break-all`, proven through the pipeline below.
      expect(markdownContainer).not.toHaveClass('prose-a:overflow-wrap-anywhere');
    });

    it('the long-URL rule is prose-a:break-all, and it compiles; the old overflow-wrap class never did', async () => {
      const { missingUtilities } = await import('./lz/compileStudioCss');
      expect(await missingUtilities(['prose-a:break-all', 'break-words'])).toEqual([]);
      expect(await missingUtilities(['prose-a:overflow-wrap-anywhere'])).toEqual([
        'prose-a:overflow-wrap-anywhere',
      ]);
    });
  });

  describe('Code copy button (LeanZero Studio)', () => {
    // The control used to be a faded tint (`bg-gray-700/50`) revealed by opacity on hover — banned
    // (DESIGN.md ban 2) and invisible to keyboard users, who reach a button they cannot see. It is a
    // solid Studio ghost now: surface-2 fill, ink-3 icon, a hairline, always visible.
    it('is a solid, always-visible Studio control — no tint, no opacity reveal, and every class compiles', async () => {
      const { missingUtilities } = await import('./lz/compileStudioCss');
      renderWithIntl(<MarkdownContent content={'```js\nconsole.log(1);\n```'} />);

      const button = await screen.findByTitle('Copy code');
      expect(button.tagName).toBe('BUTTON');
      expect(button.getAttribute('type')).toBe('button');
      const cls = button.className;
      for (const c of [
        'bg-lz-surface-2',
        'text-lz-ink-3',
        'border-lz-border-strong',
        'rounded-lz-control',
      ]) {
        expect(cls).toContain(c);
      }
      expect(cls).not.toMatch(/opacity|group-hover|\/50|bg-gray/);
      expect(button.parentElement?.className).not.toMatch(/\bgroup\b/);
      expect(await missingUtilities(Array.from(button.classList))).toEqual([]);
    }, 30_000);
  });

  describe('Heading weights (LeanZero Studio)', () => {
    // MEASURED: `font-normal` (and every `prose-hN:font-normal`) compiles to NO rule in this app —
    // the MCP theme registration sets --font-weight-normal to `initial` — so the old heading classes
    // were silent no-ops and the headings rendered at the typography plugin's 800/700/600. The
    // Studio heading register is 600 (--font-weight-lz-semibold); the size step carries the
    // hierarchy. This pins the classes AND proves they produce a rule through the real pipeline.
    it('headings carry the Studio semibold weight through a token utility that compiles', async () => {
      const { missingUtilities } = await import('./lz/compileStudioCss');
      renderWithIntl(<MarkdownContent content="# Title" />);

      const container = document.querySelector('.prose');
      expect(container).toBeInTheDocument();
      const classes = Array.from(container!.classList);
      const headingWeights = classes.filter((c) => /^prose-h[1-3]:font-/.test(c));
      expect(headingWeights.sort()).toEqual([
        'prose-h1:font-lz-semibold',
        'prose-h1:font-sans',
        'prose-h2:font-lz-semibold',
        'prose-h2:font-sans',
        'prose-h3:font-lz-semibold',
        'prose-h3:font-sans',
      ]);
      expect(classes.some((c) => /font-(normal|medium|semibold|bold)$/.test(c))).toBe(false);
      expect(await missingUtilities([...headingWeights, 'break-words'])).toEqual([]);
    });
  });

  describe('KaTeX Math Rendering - singleDollarTextMath: false', () => {
    it('treats single dollar signs as plain text', async () => {
      const content = 'The formula $x_i$ represents the i-th element.';

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        const katexElements = container.querySelectorAll('.katex');
        expect(katexElements.length).toBe(0);
        expect(container).toHaveTextContent('$x_i$');
      });
    });

    it('renders double dollar signs as display math', async () => {
      const content = `Calculate

$$
x^2 + y^2
$$

for the result.`;

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        const katexDisplay = container.querySelector('.katex-display');
        expect(katexDisplay).toBeInTheDocument();
      });
    });

    it('handles shell commands without triggering math mode', async () => {
      const content = 'Run echo "$FOO_BAR" to see the value.';

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        const katexElements = container.querySelectorAll('.katex');
        expect(katexElements.length).toBe(0);
        expect(container).toHaveTextContent('$FOO_BAR');
      });
    });

    it('preserves math in code blocks', async () => {
      const content = 'The formula `math\nx^2\n` uses inline code.';

      const { container } = renderWithIntl(<MarkdownContent content={content} />);

      await waitFor(() => {
        expect(container).toHaveTextContent('x^2');
      });
    });
  });
});
