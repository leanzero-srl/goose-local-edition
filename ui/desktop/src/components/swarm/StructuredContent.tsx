import React, { memo, useMemo } from 'react';
import MarkdownContent from '../MarkdownContent';
import { Chip, RADIUS, TONE_TEXT, WEIGHT, cx, type Tone } from '../lz';

// A model's output is EITHER prose or a structured payload, and the panel used to run both through the
// markdown renderer. That is wrong twice over for a payload:
//
//  1. It REFLOWS it. The plan skeleton — {"subtasks":[{"id":"init","description":...}]} — arrived as one
//     unreadable wall of run-together JSON, which is the single worst thing in the run panel.
//  2. It MANGLES it. Markdown reads `__x__` as bold, so the file list ["kanban/__init__.py",
//     "kanban/__main__.py"] renders as **init**.py and **main**.py. The payload is not just ugly, it is
//     WRONG — and a wrong filename in a panel about which files got written is a real lie.
//
// So: detect the payload, and give it a code path. Prose keeps the prose path.

/** The plan skeleton the architect emits — the one payload worth rendering as itself rather than as JSON. */
type Subtask = {
  id: string;
  description?: string;
  difficulty?: string;
  model?: string;
  files?: string[];
  depends_on?: string[];
};

type Parsed =
  | { kind: 'plan'; subtasks: Subtask[]; integration?: string }
  | { kind: 'json'; pretty: string }
  | { kind: 'prose' };

const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
const strArr = (v: unknown): string[] | undefined =>
  Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : undefined;

/** Classify a block of model output. Pure + exported so the behaviour is testable without rendering. */
export function classifyContent(text: string): Parsed {
  const t = (text ?? '').trim();
  // Cheap reject: a payload starts with a brace/bracket. Prose never does, and this keeps the common
  // case off JSON.parse entirely.
  if (!t.startsWith('{') && !t.startsWith('[')) return { kind: 'prose' };
  let obj: unknown;
  try {
    obj = JSON.parse(t);
  } catch {
    return { kind: 'prose' }; // a brace that is not JSON is just prose about braces
  }
  if (obj === null || typeof obj !== 'object') return { kind: 'prose' };

  const rec = obj as Record<string, unknown>;
  const raw = rec['subtasks'];
  if (Array.isArray(raw) && raw.length > 0) {
    const subtasks: Subtask[] = [];
    for (const s of raw) {
      if (!s || typeof s !== 'object') continue;
      const r = s as Record<string, unknown>;
      const id = str(r['id']);
      if (!id) continue; // an id is what makes it a task; without one this is not a plan
      subtasks.push({
        id,
        description: str(r['description']),
        difficulty: str(r['difficulty']),
        model: str(r['model']),
        files: strArr(r['files']),
        depends_on: strArr(r['depends_on']),
      });
    }
    if (subtasks.length) {
      return { kind: 'plan', subtasks, integration: str(rec['integration']) };
    }
  }
  return { kind: 'json', pretty: JSON.stringify(obj, null, 2) };
}

/** Difficulty as a status tone — easy is the ok green, medium the warn amber, hard the err red; an
 *  unknown word renders as a quiet Chip rather than an invented hue. */
const DIFFICULTY_TONE: Record<string, Tone> = { easy: 'ok', medium: 'warn', hard: 'err' };

/** The plan skeleton rendered as a task list — id, what it builds, the files it owns, what it waits on. */
const PlanSkeleton: React.FC<{ subtasks: Subtask[]; integration?: string }> = ({
  subtasks,
  integration,
}) => (
  <div className="space-y-1.5">
    {subtasks.map((s) => (
      <div key={s.id} className={cx('border border-lz-border px-2 py-1.5', RADIUS.control)}>
        <div className="flex items-center gap-2 flex-wrap">
          <span className={cx('font-mono text-lz-mono text-lz-ink', WEIGHT.semibold)}>{s.id}</span>
          {s.difficulty ? <Chip tone={DIFFICULTY_TONE[s.difficulty]}>{s.difficulty}</Chip> : null}
          {s.depends_on && s.depends_on.length ? (
            <span className="text-lz-meta text-lz-ink-3">after {s.depends_on.join(', ')}</span>
          ) : null}
        </div>
        {s.description ? (
          <div className="mt-0.5 text-lz-body text-lz-ink">{s.description}</div>
        ) : null}
        {s.files && s.files.length ? (
          // font-mono, and NOT markdown — this is where __init__.py used to come out as **init**.py.
          <div className="mt-0.5 break-all font-mono text-lz-mono text-lz-ink-2">
            {s.files.join('  ')}
          </div>
        ) : null}
      </div>
    ))}
    {integration ? (
      <div className="text-lz-meta text-lz-ink-3">
        <span className="text-lz-zone uppercase">Integration</span>{' '}
        <span className="font-mono text-lz-ink">{integration}</span>
      </div>
    ) : null}
  </div>
);

/**
 * THE code surface for the whole panel. Every block of code/output/payload renders through this, so they
 * stop each disagreeing about their own chrome — they were at radius 2 vs 3, 10px vs 11px, wrap vs scroll,
 * on two different background tokens.
 *
 * `wrap` is the ONE legitimate difference and it is now deliberate rather than accidental: shell output is
 * long prose-ish lines that SHOULD wrap, while a JSON payload must scroll — wrapping destroys its
 * indentation, which is the only thing making it readable. Either way the block scrolls inside itself and
 * never widens the panel.
 */
export const CodeBlock: React.FC<{
  text: string;
  wrap?: boolean;
  tone?: 'normal' | 'error';
  className?: string;
}> = ({ text, wrap = false, tone = 'normal', className = '' }) => (
  <pre
    className={cx(
      'm-0 border border-lz-border bg-lz-surface-2 px-2 py-1 font-mono text-lz-mono',
      tone === 'error' ? TONE_TEXT.err : 'text-lz-ink',
      wrap ? 'whitespace-pre-wrap break-words' : 'overflow-x-auto',
      RADIUS.control,
      className
    )}
  >
    {text}
  </pre>
);

/**
 * Render model output as what it IS. The single entry point every code/payload surface in the panel should
 * use, so they all get the same treatment instead of each one deciding for itself.
 */
const StructuredContent: React.FC<{ content: string }> = memo(({ content }) => {
  const parsed = useMemo(() => classifyContent(content), [content]);
  if (parsed.kind === 'plan') {
    return <PlanSkeleton subtasks={parsed.subtasks} integration={parsed.integration} />;
  }
  if (parsed.kind === 'json') return <CodeBlock text={parsed.pretty} />;
  return <MarkdownContent content={content} />;
});
StructuredContent.displayName = 'StructuredContent';

export default StructuredContent;
