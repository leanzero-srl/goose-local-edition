import { useState } from 'react';
import { Check, MessageSquare, Send, X } from 'lucide-react';
import { Button, Chip, EmptyState, Panel, TNUM, TYPE, WEIGHT, cx } from '../lz';
import type { AskRow, DeskModel, PreparedRow } from './agentWorkModel';

/**
 * What only the human can do: answer the desk's asks, and approve / decline the drafts the review
 * let through. A decision is a line in decisions.jsonl; the engine folds it at its next GUARD and
 * the approved draft posts on that tick through the desk's one write path. Nothing here is native
 * chrome — the reply box is an inline Studio form.
 */
export function NeedsYou({ model, onDecide, requiresApproval, hasPostCommand }: {
  model: DeskModel;
  onDecide: (id: string, decision: string, text: string) => Promise<void>;
  requiresApproval: boolean;
  hasPostCommand: boolean;
}) {
  const count = model.openAsks.length + model.pendingDrafts.length;
  return (
    <Panel title="Needs you" count={count} padded={false}>
      {count === 0 ? (
        <EmptyState icon={<Check />} title="Nothing waits on you" body="Asks and staged drafts land here." />
      ) : (
        <ul className="divide-y divide-lz-border" data-testid="needs-you-list">
          {model.openAsks.map((a) => (
            <AskItem key={a.id} ask={a} onDecide={onDecide} />
          ))}
          {model.pendingDrafts.map((d) => (
            <DraftItem key={d.id} draft={d} onDecide={onDecide} requiresApproval={requiresApproval} hasPostCommand={hasPostCommand} />
          ))}
        </ul>
      )}
    </Panel>
  );
}

function AskItem({ ask, onDecide }: { ask: AskRow; onDecide: (id: string, decision: string, text: string) => Promise<void> }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const send = async (decision: string) => {
    setBusy(true);
    try {
      await onDecide(ask.id, decision, text);
      setText('');
    } finally {
      setBusy(false);
    }
  };
  return (
    <li className="flex flex-col gap-2 px-4 py-3" data-testid="ask-item">
      <div className="flex items-center gap-2">
        <Chip tone="warn" icon={<MessageSquare />}>ask · tick {ask.tick}</Chip>
        <span className={cx(TYPE.meta, TNUM)}>{ask.id}</span>
      </div>
      <p className={cx(TYPE.body, WEIGHT.semibold)}>{ask.question}</p>
      {ask.why && <p className={TYPE.bodyMuted}>{ask.why}</p>}
      <div className="flex items-center gap-2">
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && text.trim()) send('reply');
          }}
          placeholder="your answer — the next tick reads it"
          aria-label={`Answer ${ask.id}`}
          className="h-8 min-w-0 flex-1 rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-lz-body text-lz-ink"
        />
        <Button variant="primary" size="sm" icon={<Send />} disabled={busy || !text.trim()} onClick={() => send('reply')}>
          Answer
        </Button>
        <Button variant="ghost" size="sm" icon={<X />} disabled={busy} onClick={() => send('dismiss')}>
          Dismiss
        </Button>
      </div>
    </li>
  );
}

function DraftItem({ draft, onDecide, requiresApproval, hasPostCommand }: {
  draft: PreparedRow;
  onDecide: (id: string, decision: string, text: string) => Promise<void>;
  requiresApproval: boolean;
  hasPostCommand: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const act = async (decision: string) => {
    setBusy(true);
    try {
      await onDecide(draft.id, decision, '');
    } finally {
      setBusy(false);
    }
  };
  const refuted = (draft.review ?? []).filter((r) => r.verdict !== 'PASS');
  const waitingOn = draft.status === 'approved'
    ? hasPostCommand ? 'approved — posts on the next tick' : 'approved — no post command: mark it done once you have posted it'
    : draft.status === 'failed'
      ? 'the post command failed — approve again to retry'
      : requiresApproval
        ? 'staged — waits for your approval'
        : hasPostCommand ? 'staged — posts on the next tick unless you decline' : 'staged — no post command: this desk hands drafts to you';
  return (
    <li className="flex flex-col gap-2 px-4 py-3" data-testid="draft-item">
      <div className="flex flex-wrap items-center gap-2">
        <Chip tone={draft.status === 'failed' ? 'err' : draft.status === 'approved' ? 'ok' : 'accent'}>{draft.kind} → {draft.target}</Chip>
        <span className={cx(TYPE.meta, TNUM)}>{draft.id} · tick {draft.tick}{draft.surgeon ? ` · ${draft.surgeon}` : ''}</span>
        {(draft.review ?? []).map((r) => (
          <Chip key={r.lens} tone={r.verdict === 'PASS' ? 'ok' : 'err'}>{r.lens} {r.verdict}</Chip>
        ))}
      </div>
      <p className={TYPE.meta}>{waitingOn}</p>
      <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} className={cx('rounded-lz-control bg-lz-surface-2 p-3 text-left', TYPE.body)}>
        <span className={cx(!open && 'line-clamp-3', 'whitespace-pre-wrap')}>{draft.body}</span>
      </button>
      {open && refuted.length > 0 && (
        <ul className="flex flex-col gap-1">
          {refuted.map((r) => (
            <li key={r.lens} className={cx(TYPE.bodyMuted)}><span className={WEIGHT.semibold}>{r.lens}:</span> {r.notes}</li>
          ))}
        </ul>
      )}
      {open && draft.result && <pre className={cx(TYPE.mono, 'whitespace-pre-wrap break-words rounded-lz-control border border-lz-err p-2')}>{draft.result}</pre>}
      <div className="flex items-center gap-2">
        {draft.status !== 'approved' && (
          <Button variant="primary" size="sm" icon={<Check />} disabled={busy} onClick={() => act('approve')}>
            {hasPostCommand ? 'Approve — post next tick' : 'Approve'}
          </Button>
        )}
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => act('done')}>Mark done</Button>
        <Button variant="destructive" size="sm" icon={<X />} disabled={busy} onClick={() => act('decline')}>Decline</Button>
      </div>
    </li>
  );
}
