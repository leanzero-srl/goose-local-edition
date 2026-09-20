import * as Dialog from '@radix-ui/react-dialog';
import { useConfig } from '../ConfigContext';
import { useId, useRef, useState } from 'react';
import { FolderOpen, Plus, X } from 'lucide-react';
import { Button, Chip, Panel, Segmented, TYPE, WEIGHT, cx, type SegmentedOption } from '../lz';
import { manifestYaml } from './agentWorkModel';

const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'] as const;
const LENSES = ['factual', 'duplication', 'voice'] as const;

const field =
  'h-8 w-full rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-lz-body text-lz-ink';
const label = cx(TYPE.meta, WEIGHT.medium, 'mb-1 block');

/**
 * Creates an agent: a directory with agent.yaml, CHARTER.md, SCRATCHPAD.md, DAILY-LOG.md and
 * PENDING.md. Every control is Studio-built (day chips, a Segmented for approval) — no native
 * select, no window.prompt. The YAML is previewed before it is written.
 */
export function NewAgentDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (dir: string) => void;
}) {
  const id = useId();
  const { extensionsList } = useConfig();
  const [extensions, setExtensions] = useState<string[]>([]);
  const [preview, setPreview] = useState(false);
  const firstRef = useRef<HTMLInputElement>(null);
  const [dir, setDir] = useState('');
  const [name, setName] = useState('');
  const [title, setTitle] = useState('');
  const [charter, setCharter] = useState('');
  const [timezone, setTimezone] = useState(
    Intl.DateTimeFormat().resolvedOptions().timeZone || 'Europe/Bucharest'
  );
  const [from, setFrom] = useState('09:00');
  const [to, setTo] = useState('18:00');
  const [days, setDays] = useState<string[]>(['mon', 'tue', 'wed', 'thu', 'fri']);
  const [always, setAlways] = useState(false);
  const [cadence, setCadence] = useState('30m');
  const [envFile, setEnvFile] = useState('');
  const [poll, setPoll] = useState('');
  const [guard, setGuard] = useState('');
  const [surgeons, setSurgeons] = useState<{ name: string; brief: string; readOnly: boolean }[]>([
    {
      name: 'general',
      brief:
        'Handle one item: do the read-only homework, then hand back a finding or a draft with exact identifiers.',
      readOnly: true,
    },
  ]);
  const [lenses, setLenses] = useState<string[]>([...LENSES]);
  const [postCommand, setPostCommand] = useState('');
  const [approval, setApproval] = useState<'human' | 'none'>('human');
  const [commit, setCommit] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const pick = async () => {
    const d = await window.electron.agentWorkPickDir();
    if (d) {
      setDir(d);
      if (!name)
        setName(
          d
            .split('/')
            .filter(Boolean)
            .pop()
            ?.toLowerCase()
            .replace(/[^a-z0-9_-]/g, '-') ?? ''
        );
    }
  };
  const yaml = manifestYaml({
    name: name.trim() || 'desk',
    title,
    timezone,
    from,
    to,
    days,
    always,
    cadence,
    envFile,
    poll: poll
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean),
    guard: guard
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean),
    surgeons: surgeons.filter((s) => s.name.trim()),
    lenses,
    postCommand,
    approval,
    commit,
    extensions,
  });
  const create = async () => {
    setError(null);
    if (!dir.trim()) return setError('Pick a directory for the agent.');
    if (!/^[A-Za-z0-9_-]+$/.test(name.trim()))
      return setError('The name must be letters, digits, - or _.');
    if (!/^[1-9]\d*[smh]$/.test(cadence.trim())) return setError('Cadence is <n>s, <n>m or <n>h.');
    if (!charter.trim())
      return setError('Describe the agent’s purpose and boundaries in its charter.');
    try {
      new Intl.DateTimeFormat('en', { timeZone: timezone });
    } catch {
      return setError('Enter a valid timezone, such as Europe/London.');
    }
    if (
      !always &&
      (!days.length ||
        !/^([01]\d|2[0-3]):[0-5]\d$/.test(from) ||
        !/^([01]\d|2[0-3]):[0-5]\d$/.test(to))
    )
      return setError('Choose at least one day and valid 24-hour start and end times.');
    if (
      !surgeons.length ||
      surgeons.some((s) => !/^[A-Za-z0-9_-]+$/.test(s.name) || !s.brief.trim())
    )
      return setError('Every worker needs a valid name and a clear assignment.');
    if (new Set(surgeons.map((s) => s.name)).size !== surgeons.length)
      return setError('Worker names must be unique.');
    setBusy(true);
    try {
      const r = await window.electron.agentWorkInit(
        dir.trim(),
        yaml,
        charter.trim() ? `# Charter\n\n${charter.trim()}\n` : ''
      );
      if (!r.ok) return setError(r.error ?? 'could not create the agent');
      onCreated(dir.trim());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const approvalOptions: SegmentedOption<'human' | 'none'>[] = [
    { value: 'human', label: 'I approve every draft' },
    { value: 'none', label: 'post reviewed drafts next tick' },
  ];

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed inset-0 z-50 flex bg-lz-surface pt-8 text-lz-ink"
          onEscapeKeyDown={(event) => {
            if (busy) event.preventDefault();
          }}
        >
          <Panel
            className="flex h-full w-full flex-col rounded-none border-0"
            title={<Dialog.Title id={`${id}-title`}>Create an agent</Dialog.Title>}
            headerRight={
              <Button
                variant="ghost"
                iconOnly
                icon={<X />}
                aria-label="Close"
                onClick={onClose}
                disabled={busy}
              />
            }
            padded={false}
          >
            <div
              className={`grid min-h-0 flex-1 gap-0 overflow-auto ${preview ? 'lg:grid-cols-[minmax(0,1fr)_minmax(300px,35%)]' : 'grid-cols-1'}`}
            >
              <div className="mx-auto flex w-full max-w-4xl flex-col gap-5 p-6 lg:p-8">
                <div>
                  <h2 className="mb-2 text-xl font-semibold">Purpose & workspace</h2>
                  <p className="mb-5 text-sm text-lz-ink-2">
                    Give your agent a clear assignment. Its research, drafts and history stay in its
                    own folder.
                  </p>
                  <span className={label}>Directory</span>
                  <div className="flex gap-2">
                    <input
                      ref={firstRef}
                      className={field}
                      value={dir}
                      onChange={(e) => setDir(e.target.value)}
                      placeholder="~/goose-agents/web-research"
                      aria-label="Agent directory"
                    />
                    <Button variant="secondary" icon={<FolderOpen />} onClick={pick}>
                      Pick
                    </Button>
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <span className={label}>Name</span>
                    <input
                      className={field}
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      placeholder="web-research"
                      aria-label="Agent name"
                    />
                  </div>
                  <div>
                    <span className={label}>Title</span>
                    <input
                      className={field}
                      value={title}
                      onChange={(e) => setTitle(e.target.value)}
                      placeholder="Web research"
                      aria-label="Agent title"
                    />
                  </div>
                </div>
                <div>
                  <span className={label}>Charter — the desk's rules (CHARTER.md)</span>
                  <textarea
                    className={cx(field, 'h-24 py-1')}
                    value={charter}
                    onChange={(e) => setCharter(e.target.value)}
                    placeholder="Who it speaks as, what it may touch, what it must ask about first."
                    aria-label="Charter"
                  />
                </div>
                <h2 className="mt-4 text-xl font-semibold">Schedule</h2>
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  <div>
                    <span className={label}>Cadence</span>
                    <input
                      className={field}
                      value={cadence}
                      onChange={(e) => setCadence(e.target.value)}
                      aria-label="Cadence"
                    />
                  </div>
                  <div>
                    <span className={label}>Timezone</span>
                    <input
                      className={field}
                      value={timezone}
                      onChange={(e) => setTimezone(e.target.value)}
                      aria-label="Timezone"
                    />
                  </div>
                  <div>
                    <span className={label}>From</span>
                    <input
                      className={field}
                      value={from}
                      onChange={(e) => setFrom(e.target.value)}
                      aria-label="Window from"
                      disabled={always}
                    />
                  </div>
                  <div>
                    <span className={label}>To</span>
                    <input
                      className={field}
                      value={to}
                      onChange={(e) => setTo(e.target.value)}
                      aria-label="Window to"
                      disabled={always}
                    />
                  </div>
                </div>
                <div className="flex flex-wrap items-center gap-1.5">
                  {DAYS.map((d) => {
                    const on = days.includes(d);
                    return (
                      <button
                        key={d}
                        type="button"
                        aria-pressed={on}
                        disabled={always}
                        onClick={() =>
                          setDays(
                            on
                              ? days.filter((x) => x !== d)
                              : [...days, d].sort(
                                  (a, b) =>
                                    DAYS.indexOf(a as (typeof DAYS)[number]) -
                                    DAYS.indexOf(b as (typeof DAYS)[number])
                                )
                          )
                        }
                        className={cx(
                          'h-7 rounded-lz-pill px-2.5 text-[12px]',
                          WEIGHT.medium,
                          on ? 'bg-lz-accent text-lz-accent-ink' : 'bg-lz-surface-2 text-lz-ink-2'
                        )}
                      >
                        {d}
                      </button>
                    );
                  })}
                  <button
                    type="button"
                    aria-pressed={always}
                    onClick={() => setAlways(!always)}
                    className={cx(
                      'ml-2 h-7 rounded-lz-pill px-2.5 text-[12px]',
                      WEIGHT.medium,
                      always ? 'bg-lz-accent text-lz-accent-ink' : 'bg-lz-surface-2 text-lz-ink-2'
                    )}
                  >
                    always open
                  </button>
                </div>
                <h2 className="mt-4 text-xl font-semibold">Tools & access</h2>
                <p className="text-sm text-lz-ink-2">
                  Choose saved MCP configurations. Set up credentials and verify connections in the
                  MCPs menu first. Only select tools this agent should use.
                </p>
                <div className="grid gap-2 sm:grid-cols-2">
                  {extensionsList
                    .filter((extension) => extension.type !== 'platform')
                    .map((extension) => (
                      <label
                        key={extension.name}
                        className="flex items-center gap-3 rounded-lg border border-lz-border p-3 text-sm"
                      >
                        <input
                          type="checkbox"
                          checked={extensions.includes(extension.name)}
                          onChange={(event) =>
                            setExtensions(
                              event.target.checked
                                ? [...extensions, extension.name]
                                : extensions.filter((name) => name !== extension.name)
                            )
                          }
                        />
                        {extension.name}
                      </label>
                    ))}
                </div>
                <details className="rounded-xl border border-lz-border p-4">
                  <summary className="cursor-pointer font-semibold">
                    Advanced workflow · scripts, workers & approval
                  </summary>
                  <div className="mt-5 flex flex-col gap-5">
                    <div>
                      <span className={label}>Script environment file (KEY=VALUE)</span>
                      <input
                        className={field}
                        value={envFile}
                        onChange={(e) => setEnvFile(e.target.value)}
                        placeholder="references/credentials.env"
                        aria-label="Env file"
                      />
                    </div>
                    <div>
                      <span className={label}>
                        Poll — read-only commands, one per line; their stdout is the tick's inbox
                      </span>
                      <textarea
                        className={cx(field, 'h-20 py-1 font-mono')}
                        value={poll}
                        onChange={(e) => setPoll(e.target.value)}
                        placeholder={'python3 scripts/inbox.py'}
                        aria-label="Poll commands"
                      />
                    </div>
                    <div>
                      <span className={label}>
                        Guard — commands run first; exit 3 holds the tick
                      </span>
                      <textarea
                        className={cx(field, 'h-14 py-1 font-mono')}
                        value={guard}
                        onChange={(e) => setGuard(e.target.value)}
                        placeholder={'bash scripts/status.sh'}
                        aria-label="Guard commands"
                      />
                    </div>
                    <div>
                      <div className="flex items-center justify-between">
                        <span className={label}>Workers — specialist assignments</span>
                        <Button
                          variant="ghost"
                          size="sm"
                          icon={<Plus />}
                          onClick={() =>
                            setSurgeons([...surgeons, { name: '', brief: '', readOnly: true }])
                          }
                        >
                          add
                        </Button>
                      </div>
                      <div className="flex flex-col gap-2">
                        {surgeons.map((s, i) => (
                          <div
                            key={i}
                            className="grid grid-cols-[140px_1fr_auto_auto] items-start gap-2"
                          >
                            <input
                              className={field}
                              value={s.name}
                              onChange={(e) =>
                                setSurgeons(
                                  surgeons.map((x, j) =>
                                    j === i ? { ...x, name: e.target.value } : x
                                  )
                                )
                              }
                              placeholder="access"
                              aria-label={`Surgeon ${i + 1} name`}
                            />
                            <input
                              className={field}
                              value={s.brief}
                              onChange={(e) =>
                                setSurgeons(
                                  surgeons.map((x, j) =>
                                    j === i ? { ...x, brief: e.target.value } : x
                                  )
                                )
                              }
                              placeholder="what this surgeon handles and returns"
                              aria-label={`Surgeon ${i + 1} brief`}
                            />
                            <button
                              type="button"
                              aria-pressed={s.readOnly}
                              onClick={() =>
                                setSurgeons(
                                  surgeons.map((x, j) =>
                                    j === i ? { ...x, readOnly: !x.readOnly } : x
                                  )
                                )
                              }
                              className={cx(
                                'h-8 rounded-lz-control px-2 text-[12px]',
                                WEIGHT.medium,
                                s.readOnly
                                  ? 'bg-lz-ok-solid text-white'
                                  : 'bg-lz-warn-solid text-white'
                              )}
                            >
                              {s.readOnly ? 'restricted tools' : 'may write'}
                            </button>
                            <Button
                              variant="ghost"
                              size="sm"
                              iconOnly
                              icon={<X />}
                              aria-label={`Remove surgeon ${i + 1}`}
                              onClick={() => setSurgeons(surgeons.filter((_, j) => j !== i))}
                            />
                          </div>
                        ))}
                      </div>
                    </div>
                    <div>
                      <span className={label}>Review lenses — each one attacks every draft</span>
                      <div className="flex flex-wrap gap-1.5">
                        {LENSES.map((l) => {
                          const on = lenses.includes(l);
                          return (
                            <button
                              key={l}
                              type="button"
                              aria-pressed={on}
                              onClick={() =>
                                setLenses(on ? lenses.filter((x) => x !== l) : [...lenses, l])
                              }
                              className={cx(
                                'h-7 rounded-lz-pill px-2.5 text-[12px]',
                                WEIGHT.medium,
                                on
                                  ? 'bg-lz-accent text-lz-accent-ink'
                                  : 'bg-lz-surface-2 text-lz-ink-2'
                              )}
                            >
                              {l}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                    <div>
                      <span className={label}>
                        Posting adapter command ({'{id}'} = draft id, {'{file}'} = its body)
                      </span>
                      <input
                        className={cx(field, 'font-mono')}
                        value={postCommand}
                        onChange={(e) => setPostCommand(e.target.value)}
                        placeholder="python3 scripts/post_comment.py --prepared {id}"
                        aria-label="Post command"
                      />
                      <div className="mt-2">
                        <Segmented
                          size="sm"
                          options={approvalOptions}
                          value={approval}
                          onChange={setApproval}
                          aria-label="Approval"
                        />
                      </div>
                    </div>
                    <label className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={commit}
                        onChange={(e) => setCommit(e.target.checked)}
                      />
                      <span className={TYPE.body}>commit the agent directory after every tick</span>
                    </label>
                    <p className="text-xs text-lz-ink-2">
                      Restricted tools are not a sandbox. Shell commands and selected MCPs retain
                      their own permissions. Posting adapters must enforce the destination’s
                      approval rules.
                    </p>
                  </div>
                </details>
                {error && (
                  <div
                    className={cx(
                      'rounded-lz-control bg-lz-err-solid px-3 py-2 text-lz-body text-white',
                      WEIGHT.medium
                    )}
                    role="alert"
                  >
                    {error}
                  </div>
                )}
              </div>
              {preview && (
                <div className="flex min-h-0 flex-col bg-lz-surface-2 p-6">
                  <div className="mb-2 flex items-center justify-between">
                    <span className={TYPE.zone}>agent.yaml</span>
                    <Chip>{yaml.split('\n').length} lines</Chip>
                  </div>
                  <pre
                    className={cx(TYPE.mono, 'min-h-0 flex-1 overflow-auto whitespace-pre-wrap')}
                  >
                    {yaml}
                  </pre>
                </div>
              )}
            </div>
            <div className="flex items-center justify-end gap-2 border-t border-lz-border px-4 py-3">
              <Button
                className="mr-auto"
                variant="ghost"
                onClick={() => setPreview(!preview)}
                aria-pressed={preview}
              >
                {preview ? 'Hide configuration' : 'Preview configuration'}
              </Button>
              <Button variant="secondary" onClick={onClose} disabled={busy}>
                Cancel
              </Button>
              <Button variant="primary" onClick={create} disabled={busy}>
                Create agent
              </Button>
            </div>
          </Panel>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
