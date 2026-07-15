import React, { memo } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';

// Inline markdown for the swarm panel's DENSE one-line rows: clarify questions, judge hints, plan lines.
//
// These strings are model-authored, so they arrive full of `backticks`, **bold** and the odd heading. The
// panel used to print them raw, so the operator read literal asterisks. The full MarkdownContent renderer
// fixes that but is block-level (2xl headings, fenced+highlighted code, KaTeX, copy buttons) and would
// wreck an 11px row — so this constrains the SAME parser to inline output instead of hand-rolling regex.
//
// Block constructs degrade rather than disappear: a heading becomes bold, a list item becomes its text.
// Links render as styled text, never anchors — nothing here should navigate, which also keeps this off
// the app's external-link security path.
const INLINE_ONLY: Components = {
  p: ({ children }) => <>{children}</>,
  h1: ({ children }) => <strong>{children}</strong>,
  h2: ({ children }) => <strong>{children}</strong>,
  h3: ({ children }) => <strong>{children}</strong>,
  h4: ({ children }) => <strong>{children}</strong>,
  h5: ({ children }) => <strong>{children}</strong>,
  h6: ({ children }) => <strong>{children}</strong>,
  strong: ({ children }) => <strong className="font-semibold text-text-primary">{children}</strong>,
  em: ({ children }) => <em>{children}</em>,
  del: ({ children }) => <del>{children}</del>,
  code: ({ children }) => (
    <code
      className="font-mono bg-inline-code text-text-primary px-1 py-px break-all"
      style={{ borderRadius: 2 }}
    >
      {children}
    </code>
  ),
  pre: ({ children }) => <>{children}</>,
  blockquote: ({ children }) => <>{children}</>,
  ul: ({ children }) => <>{children}</>,
  ol: ({ children }) => <>{children}</>,
  li: ({ children }) => <>{children} </>,
  hr: () => null,
  br: () => <> </>,
  a: ({ children, href }) => (
    <span className="underline" title={href}>
      {children}
    </span>
  ),
};

const InlineMarkdown: React.FC<{ content: string; className?: string }> = memo(
  ({ content, className }) => (
    <span className={className}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={INLINE_ONLY}
        disallowedElements={['img', 'table', 'input']}
        unwrapDisallowed
      >
        {content}
      </ReactMarkdown>
    </span>
  )
);
InlineMarkdown.displayName = 'InlineMarkdown';

export default InlineMarkdown;
