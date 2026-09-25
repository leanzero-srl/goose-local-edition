/**
 * HTML Security Detection Utilities
 *
 * These functions detect potentially dangerous HTML content in markdown
 * and wrap it safely in code blocks to prevent execution.
 */

/**
 * Detects if content contains potentially dangerous HTML
 * @param str - The content to check
 * @returns true if dangerous HTML is detected
 */
export function containsHTML(str: string): boolean {
  // Remove fenced code blocks and inline code first
  const withoutCodeBlocks = str.replace(/```[\s\S]*?```/g, '').replace(/`[^`]*`/g, '');

  // Check for HTML comments first
  const commentRegex = /<!--[\s\S]*?-->/;
  const hasComments = commentRegex.test(withoutCodeBlocks);

  // Only detect potentially dangerous HTML tags that could execute or affect layout
  const dangerousHTMLRegex =
    /<(script|style|iframe|object|embed|form|input|button|link|meta|base|br|hr|img|div|span|p|h[1-6]|a|strong|em|b|i|u|s|pre|code|blockquote|section|article|header|footer|nav|aside|main|table|tr|td|th|ul|ol|li)(?:\s[^>]*)?(?:\s*\/?>|>[^<]*<\/\1>)/i;
  const hasDangerousHTML = dangerousHTMLRegex.test(withoutCodeBlocks);

  return hasComments || hasDangerousHTML;
}

const FENCE_LINE = /^\s*(`{3,}|~{3,})(.*)$/;

type Fence = { char: string; length: number };

/**
 * The fence a line OPENS, the CommonMark way (as GitHub reads it): three or more backticks or
 * tildes, and a backtick fence's info string may not contain a backtick — "```js x```" at the
 * start of a line is inline code, not a fence. Indentation is not limited to three spaces so a
 * fence nested in a list item still counts.
 */
function opensFence(line: string): Fence | null {
  const m = FENCE_LINE.exec(line);
  if (!m) return null;
  const char = m[1][0];
  if (char === '`' && m[2].includes('`')) return null;
  return { char, length: m[1].length };
}

/** A closing fence: the opener's character, at least as long, nothing after it but spaces. */
function closesFence(line: string, open: Fence): boolean {
  const m = FENCE_LINE.exec(line);
  return !!m && m[1][0] === open.char && m[1].length >= open.length && m[2].trim() === '';
}

/** A fence longer than any backtick run in the line, so the wrapped line cannot close it early. */
function fenceFor(line: string): string {
  const longest = Math.max(0, ...(line.match(/`+/g) ?? []).map((run) => run.length));
  return '`'.repeat(Math.max(3, longest + 1));
}

/**
 * Wraps HTML content in code blocks for safe display
 * @param content - The markdown content to process
 * @returns Processed content with HTML wrapped in code blocks
 *
 * Fence tracking follows CommonMark: a ```` fence is closed only by ```` or longer, a ~~~ fence
 * only by ~~~, and an unclosed fence runs to the end of the text. The old toggle on any line
 * starting with ``` flipped inside a ```` or ~~~ block and injected an ```html fence into the
 * middle of a real code block — literal backticks in the rendered block.
 */
export function wrapHTMLInCodeBlock(content: string): string {
  let open: Fence | null = null;

  return content
    .split('\n')
    .map((line) => {
      if (open) {
        if (closesFence(line, open)) open = null;
        return line;
      }
      const fence = opensFence(line);
      if (fence) {
        open = fence;
        return line;
      }
      if (containsHTML(line)) {
        const f = fenceFor(line);
        return `${f}html\n${line}\n${f}`;
      }
      return line;
    })
    .join('\n');
}
