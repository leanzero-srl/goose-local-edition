import MarkdownContent from '../MarkdownContent';
import { ActivityDisclosure } from '../activity/ActivityDisclosure';

function prose(text: string) {
  // Preserve literal HTML evidence without changing fenced or inline code.
  return text
    .split(/(```[\s\S]*?```|`[^`]*`)/g)
    .map((part, index) =>
      index % 2
        ? part
        : part.replace(
            /<\/?[a-z][a-z0-9-]*(?:\s[^<>]*?)?\/?>/gi,
            (tag) => `&lt;${tag.slice(1, -1)}&gt;`
          )
    )
    .join('');
}

function FieldValue({ value }: { value: unknown }) {
  if (typeof value === 'string') return <MarkdownContent content={prose(value)} />;
  if (Array.isArray(value)) {
    return value.length ? (
      <ul className="list-disc space-y-3 pl-5">
        {value.map((item, index) => (
          <li key={index} className="min-w-0">
            <FieldValue value={item} />
          </li>
        ))}
      </ul>
    ) : (
      <p className="text-lz-ink-2">None</p>
    );
  }
  if (value !== null && typeof value === 'object') {
    return (
      <div className="space-y-5">
        {Object.entries(value).map(([key, item]) => (
          <section key={key} className="min-w-0">
            <h3 className="mb-2 text-sm font-semibold text-lz-ink">
              {key.replace(/_/g, ' ').replace(/^./, (letter) => letter.toUpperCase())}
            </h3>
            <FieldValue value={item} />
          </section>
        ))}
      </div>
    );
  }
  return <p className="text-sm text-lz-ink">{value === null ? 'Not provided' : String(value)}</p>;
}

function ReportText({ text }: { text: string }) {
  const fenced = text.trim().match(/^```(?:json)?\s*\n([\s\S]*?)\n```$/i);
  const candidate = (fenced?.[1] ?? text).trim();
  if (candidate.startsWith('{') || candidate.startsWith('[')) {
    try {
      return <FieldValue value={JSON.parse(candidate)} />;
    } catch {
      // Streaming and interrupted responses are still readable verbatim below.
      return <pre className="whitespace-pre-wrap break-words font-mono text-sm">{text}</pre>;
    }
  }
  return <MarkdownContent content={prose(text)} />;
}

/** Decode complete reports only; keep every attempt and the original transcript available. */
export function AgentText({ text, raw = false }: { text: string; raw?: boolean }) {
  const parts = text.split(/^(===== swarm attempt .* =====)\r?$/m);
  return (
    <div className="min-w-0 space-y-4 [overflow-wrap:anywhere]">
      {parts.map((part, index) =>
        !part.trim() ? null : index % 2 ? (
          <p key={index} className="border-b border-lz-border pb-2 text-xs text-lz-ink-2">
            {part.replace(/^===== | =====$/g, '')}
          </p>
        ) : (
          <ReportText key={index} text={part} />
        )
      )}
      {raw && (
        <ActivityDisclosure label="Original text">
          <pre className="whitespace-pre-wrap break-words p-3 font-mono text-xs">{text}</pre>
        </ActivityDisclosure>
      )}
    </div>
  );
}
